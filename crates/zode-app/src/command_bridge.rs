use std::sync::Arc;

use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{
    apply_session_runtime_options, reduce_settings_command, AppCommand, LoadState, TranscriptItem,
    TranscriptState, ZodeAppState,
};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentQuery, AgentSnapshot, ApprovalDecision,
    EndpointErrorKind, RuntimeOptions, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
    UserContent, WorkspaceUri, PROTOCOL_VERSION,
};

use crate::command_projection::{now_ms, project_global_error, replace_threads};
use crate::window_state::AppWake;

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
    Threads(Vec<ThreadSummary>),
}

/// Sequential endpoint command pump. Keeping one worker preserves user intent
/// order (for example, CreateSession before a later StartTurn) without ever
/// blocking winit's main thread.
pub struct CommandBridge {
    sender: mpsc::UnboundedSender<CommandDispatch>,
    results: mpsc::UnboundedReceiver<CommandResult>,
}

impl CommandBridge {
    /// Must be called while a Tokio runtime is entered.
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

async fn query_session_runtime_options(
    endpoint: &dyn AgentEndpoint,
    session: &SessionLocator,
) -> Result<RuntimeOptions, String> {
    match endpoint
        .query(AgentQuery::SessionRuntimeOptions {
            session: session.clone(),
        })
        .await
        .map_err(|error| error.to_string())?
    {
        AgentSnapshot::SessionRuntimeOptions {
            session: snapshot_session,
            options,
        } if &snapshot_session == session => Ok(options),
        AgentSnapshot::SessionRuntimeOptions { .. } => {
            Err("the endpoint returned runtime options for the wrong session".into())
        }
        _ => Err("the endpoint returned the wrong runtime-options snapshot".into()),
    }
}

pub fn prepare_dispatch(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> Result<Option<CommandDispatch>, String> {
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
        AppCommand::SetModel(model) => {
            let session = current_session(state)?;
            (
                session.clone(),
                None,
                AgentCommandKind::SetModel { model },
                Completion::RefreshRuntimeOptions { session },
            )
        }
        AppCommand::SetEffort(effort) => {
            let session = current_session(state)?;
            (
                session.clone(),
                None,
                AgentCommandKind::SetEffort { effort },
                Completion::RefreshRuntimeOptions { session },
            )
        }
        AppCommand::SetSandbox { mode, network } => {
            let session = current_session(state)?;
            (
                session.clone(),
                None,
                AgentCommandKind::SetSandbox { mode, network },
                Completion::RefreshRuntimeOptions { session },
            )
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

/// Targets the queue owner and claims its busy slot before endpoint dispatch.
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
    state.active_turns.insert(session.clone(), turn_id);
    let transcript = state
        .transcripts
        .get_mut(&session)
        .expect("the target session was validated before preparing its turn");
    append_user_content(transcript, &input);
    transcript.busy = true;
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

/// Targets an explicit queued-message guide at its owning active turn.
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

fn prepare_first_submit(
    state: &mut ZodeAppState,
    input: Vec<UserContent>,
) -> Result<CommandDispatch, String> {
    let workspace_uri = state
        .active_available_workspace()
        .cloned()
        .or_else(|| {
            state
                .projects
                .iter()
                .find(|project| project.available)
                .map(|project| project.workspace_uri.clone())
        })
        .ok_or_else(|| "there is no workspace available for a new session".to_owned())?;
    let session = SessionLocator::new(state.host.node_id, uuid::Uuid::new_v4().to_string());
    let turn_id = TurnId::new();
    let model = state.composer.model.clone();
    let create = AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id: None,
        kind: AgentCommandKind::CreateSession {
            workspace_uri: workspace_uri.clone(),
            model,
        },
    };
    let start = AgentCommand {
        version: PROTOCOL_VERSION,
        session: session.clone(),
        turn_id: Some(turn_id),
        kind: AgentCommandKind::StartTurn {
            input: input.clone(),
        },
    };
    let mut transcript = TranscriptState::default();
    append_user_content(&mut transcript, &input);
    transcript.busy = true;
    state.threads.insert(
        0,
        ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace_uri.clone(),
            title: "新任务".into(),
            updated_at_ms: now_ms(),
            status: ThreadStatus::Running,
        },
    );
    state.transcripts.insert(session.clone(), transcript);
    state.active_turns.insert(session.clone(), turn_id);
    state.active_workspace = Some(workspace_uri);
    state.current_session = Some(session.clone());
    Ok(CommandDispatch {
        commands: vec![create, start],
        completion: Completion::RefreshRuntimeOptions { session },
    })
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
    for content in input {
        match content {
            UserContent::Text { text } => transcript
                .items
                .push(TranscriptItem::UserText(text.clone())),
            // The desktop controller appends the real lightweight attachment metadata
            // after this payload is prepared. Do not downgrade it to a fabricated status.
            UserContent::Image { .. } => {}
        }
    }
}

fn apply_success(state: &mut ZodeAppState, _command: &AgentCommand, completion: CompletionResult) {
    match completion {
        CompletionResult::None => {}
        CompletionResult::NewSession {
            workspace_uri,
            session,
            runtime_options,
        } => {
            state.threads.insert(
                0,
                ThreadSummary {
                    session: session.clone(),
                    workspace_uri: workspace_uri.clone(),
                    title: "新任务".into(),
                    updated_at_ms: now_ms(),
                    status: ThreadStatus::Idle,
                },
            );
            state.transcripts.entry(session.clone()).or_default();
            if !state
                .projects
                .iter()
                .any(|project| project.workspace_uri == workspace_uri)
            {
                state.projects.push(zode_app_model::ProjectState {
                    workspace_uri: workspace_uri.clone(),
                    expanded: true,
                    available: true,
                    last_opened_ms: now_ms(),
                });
            }
            state.active_workspace = Some(workspace_uri);
            state.current_session = Some(session.clone());
            match runtime_options {
                Ok(options) => {
                    let _ = apply_session_runtime_options(state, session, options);
                }
                Err(message) => {
                    state
                        .presentation
                        .sessions
                        .entry(session.clone())
                        .or_default()
                        .runtime_options = LoadState::Failed(message.clone());
                    if let Some(transcript) = state.transcripts.get_mut(&session) {
                        transcript.items.push(TranscriptItem::Error {
                            message: format!("运行设置加载失败：{message}"),
                            retryable: true,
                        });
                    }
                }
            }
        }
        CompletionResult::Approval { approval_id } => {
            let session = state.approvals.remove(&approval_id);
            if let Some(transcript) =
                session.and_then(|session| state.transcripts.get_mut(&session))
            {
                transcript.items.retain(
                    |item| !matches!(item, TranscriptItem::Approval { id, .. } if id == &approval_id),
                );
            }
        }
        CompletionResult::ProjectPermissions {
            workspace_uri,
            tools,
        } => {
            let _ = reduce_settings_command(
                state,
                AppCommand::SetProjectPermissions {
                    workspace_uri,
                    tools,
                },
            );
        }
        CompletionResult::RuntimeOptions { session, options } => {
            let _ = apply_session_runtime_options(state, session, options);
        }
        CompletionResult::Threads(threads) => replace_threads(state, threads),
    }
}

fn apply_failure(state: &mut ZodeAppState, command: &AgentCommand, message: String) {
    if matches!(command.kind, AgentCommandKind::StartTurn { .. }) {
        state.active_turns.remove(&command.session);
        if let Some(transcript) = state.transcripts.get_mut(&command.session) {
            transcript.busy = false;
        }
        if let Some(thread) = state
            .threads
            .iter_mut()
            .find(|thread| thread.session == command.session)
        {
            thread.status = ThreadStatus::Failed;
        }
    }
    project_retryable_error(state, &command.session, format!("命令执行失败：{message}"));
}

fn apply_completion_failure(state: &mut ZodeAppState, command: &AgentCommand, message: String) {
    let runtime_sync = matches!(
        command.kind,
        AgentCommandKind::StartTurn { .. }
            | AgentCommandKind::SetModel { .. }
            | AgentCommandKind::SetEffort { .. }
            | AgentCommandKind::SetSandbox { .. }
    );
    let summary = if runtime_sync {
        let runtime_options = &mut state
            .presentation
            .sessions
            .entry(command.session.clone())
            .or_default()
            .runtime_options;
        if !matches!(runtime_options, LoadState::Ready(_)) {
            *runtime_options = LoadState::Failed(message.clone());
        }
        format!("运行设置同步失败：{message}")
    } else {
        format!("状态同步失败：{message}")
    };
    project_retryable_error(state, &command.session, summary);
}

fn project_retryable_error(
    state: &mut ZodeAppState,
    command_session: &SessionLocator,
    message: String,
) {
    let target_session = if state.transcripts.contains_key(command_session) {
        Some(command_session.clone())
    } else {
        state
            .current_session
            .as_ref()
            .filter(|session| state.transcripts.contains_key(*session))
            .cloned()
    };
    if let Some(transcript) = target_session.and_then(|session| state.transcripts.get_mut(&session))
    {
        transcript.items.push(TranscriptItem::Error {
            message,
            retryable: true,
        });
    } else {
        project_global_error(state, message);
    }
}

fn apply_batch_failure(
    state: &mut ZodeAppState,
    failed_command: &AgentCommand,
    recovery_command: &AgentCommand,
    executed_prefix: usize,
    kind: Option<EndpointErrorKind>,
    message: String,
) {
    if kind == Some(EndpointErrorKind::PartialSuccess) {
        if let AgentCommandKind::Approve {
            approval_id,
            decision: ApprovalDecision::AllowAlways,
        } = &failed_command.kind
        {
            apply_approval_fallback(state, &failed_command.session, approval_id, message);
            return;
        }
    }
    if kind == Some(EndpointErrorKind::RequestExpired) {
        if let AgentCommandKind::Approve { approval_id, .. } = &failed_command.kind {
            apply_expired_approval(state, &failed_command.session, approval_id);
            return;
        }
    }
    let create_never_succeeded = executed_prefix == 0
        && matches!(failed_command.kind, AgentCommandKind::CreateSession { .. })
        && matches!(recovery_command.kind, AgentCommandKind::StartTurn { .. });
    if create_never_succeeded {
        state.active_turns.remove(&recovery_command.session);
        state
            .threads
            .retain(|thread| thread.session != recovery_command.session);
        state.transcripts.remove(&recovery_command.session);
        state.message_queues.remove(&recovery_command.session);
        if state.current_session.as_ref() == Some(&recovery_command.session) {
            state.current_session = None;
        }
        project_global_error(state, message);
    } else {
        apply_failure(state, recovery_command, message);
    }
}

fn apply_expired_approval(
    state: &mut ZodeAppState,
    command_session: &SessionLocator,
    approval_id: &str,
) {
    let session = state
        .approvals
        .remove(approval_id)
        .unwrap_or_else(|| command_session.clone());
    if let Some(transcript) = state.transcripts.get_mut(&session) {
        transcript.items.retain(
            |item| !matches!(item, TranscriptItem::Approval { id, .. } if id == approval_id),
        );
        transcript.items.push(TranscriptItem::Status {
            code: "approval.request_expired".into(),
            message: "批准请求已失效；请重新触发该操作。".into(),
        });
    }
}

fn apply_approval_fallback(
    state: &mut ZodeAppState,
    command_session: &SessionLocator,
    approval_id: &str,
    detail: String,
) {
    let session = state
        .approvals
        .remove(approval_id)
        .unwrap_or_else(|| command_session.clone());
    if let Some(transcript) = state.transcripts.get_mut(&session) {
        transcript.items.retain(
            |item| !matches!(item, TranscriptItem::Approval { id, .. } if id == approval_id),
        );
        transcript.items.push(TranscriptItem::Status {
            code: "approval.allow_always_fallback".into(),
            message: format!("已仅允许一次，记忆失败：{detail}"),
        });
    }
}

#[cfg(test)]
#[path = "command_bridge_tests.rs"]
mod tests;
