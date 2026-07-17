use zode_app_model::{
    apply_session_runtime_options, reduce_settings_command, AppCommand, LoadState,
    ProviderModelsStatus, TranscriptItem, ZodeAppState,
};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, ApprovalDecision, EndpointErrorKind, SessionLocator,
    ThreadStatus, ThreadSummary, WorkspaceUri,
};

use crate::command_projection::{
    now_ms, project_global_error, project_workspace_error, replace_threads,
};

use super::{integrations, plugin_market, CompletionResult};

pub(super) fn apply_success(
    state: &mut ZodeAppState,
    _command: &AgentCommand,
    completion: CompletionResult,
) {
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
                let _ = transcript.remove_approval(&approval_id);
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
        CompletionResult::ProviderRuntimeOptions { global, session } => {
            state.composer.available_models.clone_from(&global.models);
            if state.current_session.is_none() {
                state.composer.model.clone_from(&global.active_model);
                state.composer.effort.clone_from(&global.effort);
                state.composer.approval_mode = global.approval_mode;
                state.composer.sandbox_mode = global.sandbox_mode;
                state.composer.sandbox_network = global.sandbox_network;
                state.composer.sandbox_label = approval_mode_label(global.approval_mode).into();
            }
            if let Some((session, options)) = session {
                let _ = apply_session_runtime_options(state, session, options);
            }
            state.composer.footer_menu = None;
        }
        CompletionResult::Integrations(snapshot) => integrations::apply_success(state, snapshot),
        CompletionResult::InstalledPlugins(plugins) => plugin_market::apply_success(state, plugins),
        CompletionResult::Threads(threads) => replace_threads(state, threads),
    }
}

fn apply_failure(state: &mut ZodeAppState, command: &AgentCommand, message: String) {
    if matches!(command.kind, AgentCommandKind::StartTurn { .. }) {
        state.active_turns.remove(&command.session);
        if let Some(transcript) = state.transcripts.get_mut(&command.session) {
            if let Some(turn_id) = command.turn_id {
                let _ = transcript.fail_turn(turn_id);
            }
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
    integrations::mark_failure(state, command, &message);
    plugin_market::mark_failure(state, &command.kind, &message);
    if matches!(command.kind, AgentCommandKind::ReloadProviderConfiguration) {
        project_provider_reload_failure(state, message);
        return;
    }
    project_retryable_error(state, &command.session, format!("命令执行失败：{message}"));
}

pub(super) fn apply_completion_failure(
    state: &mut ZodeAppState,
    command: &AgentCommand,
    message: String,
) {
    integrations::mark_failure(state, command, &message);
    plugin_market::mark_failure(state, &command.kind, &message);
    if matches!(command.kind, AgentCommandKind::ReloadProviderConfiguration) {
        project_provider_reload_failure(state, message);
        return;
    }
    let runtime_sync = matches!(
        command.kind,
        AgentCommandKind::StartTurn { .. }
            | AgentCommandKind::SetModel { .. }
            | AgentCommandKind::SetEffort { .. }
            | AgentCommandKind::SetSandbox { .. }
            | AgentCommandKind::SetPermissionPreset { .. }
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

fn project_provider_reload_failure(state: &mut ZodeAppState, message: String) {
    let provider_id = match &state.provider_models.status {
        ProviderModelsStatus::Saved { provider_id }
        | ProviderModelsStatus::Removed { provider_id }
        | ProviderModelsStatus::Saving { provider_id }
        | ProviderModelsStatus::Removing { provider_id } => Some(provider_id.clone()),
        ProviderModelsStatus::Failed { provider_id, .. } => provider_id.clone(),
        ProviderModelsStatus::Idle => None,
    };
    state.provider_models.status = ProviderModelsStatus::Failed {
        provider_id,
        message: format!("配置已保存，但运行时重新加载失败：{message}"),
    };
    state.composer.footer_menu = None;
}

fn approval_mode_label(mode: zode_node_protocol::ApprovalMode) -> &'static str {
    match mode {
        zode_node_protocol::ApprovalMode::Request => "请求批准",
        zode_node_protocol::ApprovalMode::Auto => "替我审批",
        zode_node_protocol::ApprovalMode::Full => "完全访问",
    }
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

pub(super) fn apply_batch_failure(
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
        let failed_workspace = match &failed_command.kind {
            AgentCommandKind::CreateSession { workspace_uri, .. } => Some(workspace_uri.clone()),
            _ => None,
        };
        let focus_error = state.current_session.as_ref() == Some(&recovery_command.session);
        state.active_turns.remove(&recovery_command.session);
        state
            .threads
            .retain(|thread| thread.session != recovery_command.session);
        state
            .thread_workspace_root_hints
            .remove(&recovery_command.session);
        state.projectless_sessions.remove(&recovery_command.session);
        state.transcripts.remove(&recovery_command.session);
        state.message_queues.remove(&recovery_command.session);
        if focus_error {
            state.current_session = None;
        }
        if let Some(workspace_uri) = failed_workspace {
            cleanup_failed_projectless_workspace(state, &workspace_uri, &recovery_command.session);
            project_workspace_error(state, workspace_uri, message, focus_error);
        } else {
            project_global_error(state, message);
        }
    } else {
        apply_failure(state, recovery_command, message);
    }
}

fn cleanup_failed_projectless_workspace(
    state: &ZodeAppState,
    workspace_uri: &WorkspaceUri,
    session: &SessionLocator,
) {
    if !state.is_projectless_workspace(workspace_uri) {
        return;
    }
    let Some(root_uri) = state.projectless_workspace_root.as_ref() else {
        return;
    };
    let (Ok(root), Ok(workspace)) = (
        zode_app_runtime::workspace_uri_to_path(root_uri),
        zode_app_runtime::workspace_uri_to_path(workspace_uri),
    ) else {
        return;
    };
    let (Ok(root), Ok(workspace)) = (root.canonicalize(), workspace.canonicalize()) else {
        return;
    };
    let direct_child = workspace.parent() == Some(root.as_path())
        && workspace
            .file_name()
            .is_some_and(|name| name == std::ffi::OsStr::new(&session.session_id));
    if direct_child {
        let _ = std::fs::remove_dir_all(workspace);
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
        let _ = transcript.remove_approval(approval_id);
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
        let _ = transcript.remove_approval(approval_id);
        transcript.items.push(TranscriptItem::Status {
            code: "approval.allow_always_fallback".into(),
            message: format!("已仅允许一次，记忆失败：{detail}"),
        });
    }
}
