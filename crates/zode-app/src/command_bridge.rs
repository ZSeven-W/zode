use std::sync::Arc;

use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{AppCommand, TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentQuery, AgentSnapshot, EndpointErrorKind,
    RuntimeOptions, SessionLocator, ThreadStatus, ThreadSummary, TurnId, UserContent, WorkspaceUri,
    PROTOCOL_VERSION,
};

use crate::command_projection::project_global_error;
use crate::window_state::AppWake;

mod approval;
mod first_submit;
mod integrations;
mod plugin_market;
#[path = "command_bridge/result-projection.rs"]
mod result_projection;
mod runtime;
use first_submit::prepare_first_submit;
use result_projection::{apply_batch_failure, apply_completion_failure, apply_success};
use runtime::{
    ensure_runtime_idle, prepare_permission_preset, prepare_reset_runtime,
    query_session_runtime_options,
};

#[derive(Debug)]
pub struct CommandDispatch {
    commands: Vec<AgentCommand>,
    completion: Completion,
}

#[derive(Debug)]
enum Completion {
    None,
    NewSession {
        workspace_uri: WorkspaceUri,
        session: SessionLocator,
    },
    Approval {
        approval_id: String,
    },
    RefreshPermissions {
        workspace_uri: WorkspaceUri,
    },
    RefreshRuntimeOptions {
        session: SessionLocator,
    },
    RefreshProviderRuntimeOptions {
        session: Option<SessionLocator>,
    },
    RefreshIntegrations {
        workspace_uri: WorkspaceUri,
    },
    RefreshInstalledPlugins,
    RefreshThreads,
}

enum CommandResult {
    Succeeded {
        command: AgentCommand,
        completion: CompletionResult,
    },
    CompletionFailed {
        command: AgentCommand,
        message: String,
    },
    CommandFailed {
        failed_command: AgentCommand,
        recovery_command: AgentCommand,
        executed_prefix: usize,
        kind: Option<EndpointErrorKind>,
        message: String,
    },
}

enum CompletionResult {
    None,
    NewSession {
        workspace_uri: WorkspaceUri,
        session: SessionLocator,
        runtime_options: Result<RuntimeOptions, String>,
    },
    Approval {
        approval_id: String,
    },
    ProjectPermissions {
        workspace_uri: WorkspaceUri,
        tools: Vec<String>,
    },
    RuntimeOptions {
        session: SessionLocator,
        options: RuntimeOptions,
    },
    ProviderRuntimeOptions {
        global: RuntimeOptions,
        session: Option<(SessionLocator, RuntimeOptions)>,
    },
    Integrations(zode_node_protocol::IntegrationRegistrySnapshot),
    InstalledPlugins(Vec<zode_node_protocol::InstalledPluginSummary>),
    Threads(Vec<ThreadSummary>),
}

pub struct CommandBridge {
    sender: mpsc::UnboundedSender<CommandDispatch>,
    results: mpsc::UnboundedReceiver<CommandResult>,
}

impl CommandBridge {
    pub fn spawn(endpoint: Arc<dyn AgentEndpoint>, proxy: EventLoopProxy<AppWake>) -> Self {
        Self::spawn_with_wake(endpoint, move || {
            let _ = proxy.send_event(AppWake::Redraw);
        })
    }

    fn spawn_with_wake(
        endpoint: Arc<dyn AgentEndpoint>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (sender, mut commands) = mpsc::unbounded_channel::<CommandDispatch>();
        let (result_sender, results) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(dispatch) = commands.recv().await {
                let command = dispatch
                    .commands
                    .last()
                    .cloned()
                    .expect("every prepared dispatch has at least one command");
                let mut failure = None;
                let mut executed_prefix = 0;
                for wire_command in dispatch.commands {
                    match endpoint.command(wire_command.clone()).await {
                        Ok(()) => executed_prefix += 1,
                        Err(error) => {
                            failure = Some((wire_command, error.kind, error.message));
                            break;
                        }
                    }
                }
                let result = if let Some((failed_command, kind, message)) = failure {
                    CommandResult::CommandFailed {
                        failed_command,
                        recovery_command: command,
                        executed_prefix,
                        kind: Some(kind),
                        message,
                    }
                } else {
                    match complete(&*endpoint, dispatch.completion).await {
                        Ok(completion) => CommandResult::Succeeded {
                            command,
                            completion,
                        },
                        Err(message) => CommandResult::CompletionFailed { command, message },
                    }
                };
                if result_sender.send(result).is_ok() {
                    wake();
                }
            }
        });
        Self { sender, results }
    }

    pub fn dispatch(&self, dispatch: CommandDispatch) -> Result<(), CommandDispatch> {
        self.sender.send(dispatch).map_err(|error| error.0)
    }

    pub fn drain_into(&mut self, state: &mut ZodeAppState) -> usize {
        let mut applied = 0;
        while let Ok(result) = self.results.try_recv() {
            match result {
                CommandResult::Succeeded {
                    command,
                    completion,
                } => apply_success(state, &command, completion),
                CommandResult::CompletionFailed { command, message } => {
                    apply_completion_failure(state, &command, message)
                }
                CommandResult::CommandFailed {
                    failed_command,
                    recovery_command,
                    executed_prefix,
                    kind,
                    message,
                } => apply_batch_failure(
                    state,
                    &failed_command,
                    &recovery_command,
                    executed_prefix,
                    kind,
                    message,
                ),
            }
            applied += 1;
        }
        applied
    }
}

pub fn reject_dispatch(state: &mut ZodeAppState, dispatch: CommandDispatch, message: String) {
    if let (Some(failed), Some(recovery)) = (dispatch.commands.first(), dispatch.commands.last()) {
        apply_batch_failure(state, failed, recovery, 0, None, message);
    } else {
        project_global_error(state, message);
    }
}

pub fn project_command_error(state: &mut ZodeAppState, message: String) {
    project_global_error(state, message);
}

async fn complete(
    endpoint: &dyn AgentEndpoint,
    completion: Completion,
) -> Result<CompletionResult, String> {
    match completion {
        Completion::None => Ok(CompletionResult::None),
        Completion::NewSession {
            workspace_uri,
            session,
        } => Ok(CompletionResult::NewSession {
            workspace_uri,
            runtime_options: query_session_runtime_options(endpoint, &session).await,
            session,
        }),
        Completion::Approval { approval_id } => Ok(CompletionResult::Approval { approval_id }),
        Completion::RefreshPermissions { workspace_uri } => {
            match endpoint
                .query(AgentQuery::ProjectPermissions {
                    workspace_uri: workspace_uri.clone(),
                })
                .await
                .map_err(|error| error.to_string())?
            {
                AgentSnapshot::ProjectPermissions(tools) => {
                    Ok(CompletionResult::ProjectPermissions {
                        workspace_uri,
                        tools,
                    })
                }
                _ => Err("the endpoint returned the wrong project-permissions snapshot".into()),
            }
        }
        Completion::RefreshRuntimeOptions { session } => {
            let options = query_session_runtime_options(endpoint, &session).await?;
            Ok(CompletionResult::RuntimeOptions { session, options })
        }
        Completion::RefreshProviderRuntimeOptions { session } => {
            let global = match endpoint
                .query(AgentQuery::RuntimeOptions)
                .await
                .map_err(|error| error.to_string())?
            {
                AgentSnapshot::RuntimeOptions(options) => options,
                _ => return Err("the endpoint returned the wrong runtime-options snapshot".into()),
            };
            let session = match session {
                Some(session) => query_session_runtime_options(endpoint, &session)
                    .await
                    .ok()
                    .map(|options| (session, options)),
                None => None,
            };
            Ok(CompletionResult::ProviderRuntimeOptions { global, session })
        }
        Completion::RefreshIntegrations { workspace_uri } => {
            integrations::complete(endpoint, workspace_uri).await
        }
        Completion::RefreshInstalledPlugins => {
            plugin_market::complete_installed_plugins(endpoint).await
        }
        Completion::RefreshThreads => match endpoint
            .query(AgentQuery::Threads)
            .await
            .map_err(|error| error.to_string())?
        {
            AgentSnapshot::Threads(threads) => Ok(CompletionResult::Threads(threads)),
            _ => Err("the endpoint returned the wrong thread snapshot".into()),
        },
    }
}

pub fn prepare_dispatch(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> Result<Option<CommandDispatch>, String> {
    if matches!(command, AppCommand::ResetComposerRuntime) {
        return prepare_reset_runtime(state).map(Some);
    }
    let (session, turn_id, kind, completion) = match command {
        AppCommand::NewSession { workspace_uri } => {
            if !state.available_workspace(&workspace_uri) {
                return Err("the workspace is unavailable for a new session".into());
            }
            let session = SessionLocator::new(state.host.node_id, uuid::Uuid::new_v4().to_string());
            let model = state.composer.model.clone();
            (
                session.clone(),
                None,
                AgentCommandKind::CreateSession {
                    workspace_uri: workspace_uri.clone(),
                    project_uri: Some(workspace_uri.clone()),
                    projectless: false,
                    model,
                },
                Completion::NewSession {
                    workspace_uri,
                    session,
                },
            )
        }
        AppCommand::RenameSession { session, title } => (
            session,
            None,
            AgentCommandKind::RenameSession { title },
            Completion::RefreshThreads,
        ),
        AppCommand::DeleteSession(session) => (
            session,
            None,
            AgentCommandKind::DeleteSession,
            Completion::RefreshThreads,
        ),
        AppCommand::Submit(input) => {
            let session = state
                .current_session
                .clone()
                .filter(|session| !session.session_id.starts_with("local-error-"))
                .filter(|session| state.available_workspace_for_session(session).is_some());
            let Some(session) = session else {
                return prepare_first_submit(state, input).map(Some);
            };
            return prepare_queued_start(state, session, input).map(Some);
        }
        AppCommand::Steer(input) => {
            let session = current_session(state)?;
            return prepare_queued_steer(state, session, input).map(Some);
        }
        AppCommand::Interrupt => {
            let session = current_session(state)?;
            let turn_id = active_turn(state, &session)?;
            (
                session,
                Some(turn_id),
                AgentCommandKind::InterruptTurn,
                Completion::None,
            )
        }
        AppCommand::Approve { id, decision } => {
            let session = state
                .approvals
                .get(&id)
                .cloned()
                .ok_or_else(|| "the approval is no longer pending".to_owned())?;
            let turn_id = state.active_turns.get(&session).copied();
            let decision = approval::scoped_decision(state, &session, decision);
            (
                session,
                turn_id,
                AgentCommandKind::Approve {
                    approval_id: id.clone(),
                    decision,
                },
                Completion::Approval { approval_id: id },
            )
        }
        AppCommand::RevokeProjectPermission {
            workspace_uri,
            tool,
        } => {
            let session = state
                .threads
                .iter()
                .find(|thread| thread.workspace_uri == workspace_uri)
                .map(|thread| thread.session.clone())
                .unwrap_or_else(|| {
                    SessionLocator::new(state.host.node_id, "settings-permission-revoke")
                });
            (
                session,
                None,
                AgentCommandKind::RevokeProjectPermission {
                    workspace_uri: workspace_uri.clone(),
                    tool,
                },
                Completion::RefreshPermissions { workspace_uri },
            )
        }
        AppCommand::SetIntegrationEnabled {
            workspace_uri,
            source_id,
            enabled,
        } => {
            let parts = integrations::prepare(state, workspace_uri, source_id, enabled)?;
            (parts.session, None, parts.kind, parts.completion)
        }
        AppCommand::InstallPlugin => {
            let parts = plugin_market::prepare_install(state)?;
            (parts.session, None, parts.kind, parts.completion)
        }
        AppCommand::ConfirmUninstallPlugin => {
            let parts = plugin_market::prepare_uninstall(state)?;
            (parts.session, None, parts.kind, parts.completion)
        }
        AppCommand::GrantPluginTrust { keys } => {
            let parts = plugin_market::prepare_grant(state, keys)?;
            (parts.session, None, parts.kind, parts.completion)
        }
        AppCommand::SetModel(model) => {
            let session = state.current_session.clone().filter(|session| {
                !session.session_id.starts_with("local-error-")
                    && state.transcripts.contains_key(session)
                    && state.available_workspace_for_session(session).is_some()
            });
            let Some(session) = session else {
                if !state
                    .composer
                    .available_models
                    .iter()
                    .any(|candidate| candidate == &model)
                {
                    return Err("the selected model is not available".into());
                }
                state.composer.model = Some(model);
                state.composer.footer_menu = None;
                return Ok(None);
            };
            ensure_runtime_idle(state, &session)?;
            state.composer.footer_menu = None;
            (
                session.clone(),
                None,
                AgentCommandKind::SetModel { model },
                Completion::RefreshRuntimeOptions { session },
            )
        }
        AppCommand::ReloadProviderConfiguration => {
            let active_session = state.current_session.clone().filter(|session| {
                !session.session_id.starts_with("local-error-")
                    && state.transcripts.contains_key(session)
            });
            let command_session = active_session.clone().unwrap_or_else(|| {
                SessionLocator::new(state.host.node_id, "provider-configuration-reload")
            });
            (
                command_session,
                None,
                AgentCommandKind::ReloadProviderConfiguration,
                Completion::RefreshProviderRuntimeOptions {
                    session: active_session,
                },
            )
        }
        AppCommand::SetEffort(effort) => {
            let session = current_session(state)?;
            ensure_runtime_idle(state, &session)?;
            state.composer.footer_menu = None;
            (
                session.clone(),
                None,
                AgentCommandKind::SetEffort { effort },
                Completion::RefreshRuntimeOptions { session },
            )
        }
        AppCommand::SetSandbox { mode, network } => {
            let session = current_session(state)?;
            ensure_runtime_idle(state, &session)?;
            state.composer.footer_menu = None;
            (
                session.clone(),
                None,
                AgentCommandKind::SetSandbox { mode, network },
                Completion::RefreshRuntimeOptions { session },
            )
        }
        command @ AppCommand::SetPermissionPreset { .. } => {
            return prepare_permission_preset(state, command).map(Some);
        }
        _ => return Ok(None),
    };
    Ok(Some(CommandDispatch {
        commands: vec![AgentCommand {
            version: PROTOCOL_VERSION,
            session,
            turn_id,
            kind,
        }],
        completion,
    }))
}

pub(crate) fn prepare_queued_start(
    state: &mut ZodeAppState,
    session: SessionLocator,
    input: Vec<UserContent>,
) -> Result<CommandDispatch, String> {
    validate_target_session(state, &session)?;
    let transcript_busy = state
        .transcripts
        .get(&session)
        .is_some_and(|transcript| transcript.busy);
    if transcript_busy || state.active_turns.contains_key(&session) {
        return Err("the target session already has an active turn".into());
    }
    let turn_id = TurnId::new();
    let transcript = state
        .transcripts
        .get_mut(&session)
        .expect("the target session was validated before preparing its turn");
    let start_item_index = transcript.items.len();
    append_user_content(transcript, &input);
    let response_item_index = transcript.items.len();
    if !transcript.begin_turn(turn_id, start_item_index, response_item_index) {
        transcript.items.truncate(start_item_index);
        return Err("the target session could not record its active turn".into());
    }
    state.active_turns.insert(session.clone(), turn_id);
    if let Some(thread) = state
        .threads
        .iter_mut()
        .find(|thread| thread.session == session)
    {
        thread.status = ThreadStatus::Running;
    }

    Ok(CommandDispatch {
        commands: vec![AgentCommand {
            version: PROTOCOL_VERSION,
            session,
            turn_id: Some(turn_id),
            kind: AgentCommandKind::StartTurn { input },
        }],
        completion: Completion::None,
    })
}

pub(crate) fn prepare_queued_steer(
    state: &mut ZodeAppState,
    session: SessionLocator,
    input: Vec<UserContent>,
) -> Result<CommandDispatch, String> {
    validate_target_session(state, &session)?;
    let turn_id = active_turn(state, &session)?;
    let transcript = state
        .transcripts
        .get_mut(&session)
        .expect("the target session was validated before preparing its steer");
    append_user_content(transcript, &input);

    Ok(CommandDispatch {
        commands: vec![AgentCommand {
            version: PROTOCOL_VERSION,
            session,
            turn_id: Some(turn_id),
            kind: AgentCommandKind::SteerTurn { input },
        }],
        completion: Completion::None,
    })
}

fn validate_target_session(state: &ZodeAppState, session: &SessionLocator) -> Result<(), String> {
    let live = !session.session_id.starts_with("local-error-")
        && state.transcripts.contains_key(session)
        && state.available_workspace_for_session(session).is_some();
    live.then_some(())
        .ok_or_else(|| "the target session is unavailable".to_owned())
}

fn current_session(state: &ZodeAppState) -> Result<SessionLocator, String> {
    state
        .current_session
        .clone()
        .ok_or_else(|| "there is no active session for this command".to_owned())
}

fn active_turn(state: &ZodeAppState, session: &SessionLocator) -> Result<TurnId, String> {
    state
        .active_turns
        .get(session)
        .copied()
        .ok_or_else(|| "there is no active turn for this command".to_owned())
}

fn append_user_content(transcript: &mut TranscriptState, input: &[UserContent]) {
    // One wall-clock read per submission (not per content block), from the
    // same `now_ms` source the command-projection layer already uses to
    // stamp `ThreadSummary::updated_at_ms`.
    let timestamp_ms = crate::command_projection::now_ms();
    for content in input {
        match content {
            UserContent::Text { text } => transcript
                .items
                .push(TranscriptItem::user_text_at(text.clone(), timestamp_ms)),
            UserContent::Image { .. } => {}
        }
    }
}

#[cfg(test)]
#[path = "command_bridge_tests.rs"]
mod tests;
