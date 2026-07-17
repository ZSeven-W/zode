use crate::{default_tool_expanded, AppCommand, TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, RuntimeOptions, SandboxMode, SessionLocator, ToolCall,
};

const UNKNOWN_EVENT_CODE: &str = "agent.event.unknown";
const UNKNOWN_EVENT_MESSAGE: &str = "Ignored an unknown agent event";

/// Result of reducing an ordered agent event into application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOutcome {
    Applied,
    IgnoredStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationOutcome {
    Applied,
    NeedsEffect,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptCommandOutcome {
    Applied,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCommandOutcome {
    Applied,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueCommandOutcome {
    Applied,
    Enqueued(crate::QueuedMessageId),
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCommandOutcome {
    Applied,
    NeedsEffect,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationCommandOutcome {
    Applied,
    Ignored,
}

/// Applies typed shell navigation without dispatching endpoint effects.
pub fn reduce_presentation_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> PresentationCommandOutcome {
    let route = match command {
        AppCommand::Navigate(route) => Some(route),
        AppCommand::SelectSettingsCategory(category) => Some(crate::ShellRoute::Settings(category)),
        AppCommand::SelectIntegrationsTab(tab) => Some(crate::ShellRoute::Integrations(tab)),
        AppCommand::OpenSecondary(pane) => {
            open_secondary(state, pane);
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::CloseSecondary => {
            close_secondary(state);
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::OpenReview => {
            open_secondary(state, crate::SecondaryPane::Review);
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::PreviewWorkspaceFile {
            session,
            relative_path,
        } => {
            let valid_session = state.current_session.as_ref() == Some(&session)
                && !session.session_id.starts_with("local-error-")
                && state.transcripts.contains_key(&session);
            let Some(workspace_uri) = valid_session
                .then(|| state.available_workspace_for_session(&session).cloned())
                .flatten()
                .filter(|workspace| workspace.as_str().starts_with("file://"))
            else {
                return PresentationCommandOutcome::Ignored;
            };
            state
                .presentation
                .sessions
                .entry(session)
                .or_default()
                .preview = crate::PreviewState::Loading {
                target: crate::PreviewTarget {
                    workspace_uri,
                    relative_path,
                },
            };
            open_secondary(state, crate::SecondaryPane::DocumentPreview);
            return PresentationCommandOutcome::Applied;
        }
        _ => None,
    };

    let Some(route) = route else {
        return PresentationCommandOutcome::Ignored;
    };
    if route != crate::ShellRoute::Conversation {
        close_secondary(state);
    }
    state.presentation.route = route;
    state.shell.page = route.legacy_page();
    PresentationCommandOutcome::Applied
}

fn open_secondary(state: &mut ZodeAppState, pane: crate::SecondaryPane) {
    state.presentation.route = crate::ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(pane);
    state.shell.page = crate::ShellPage::Conversation;
    state.review.open = pane == crate::SecondaryPane::Review;
}

fn close_secondary(state: &mut ZodeAppState) {
    state.presentation.secondary_pane = None;
    state.review.open = false;
}

/// Applies terminal-only UI state and identifies commands that must be
/// forwarded to the platform terminal service.
pub fn reduce_terminal_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> TerminalCommandOutcome {
    match command {
        AppCommand::OpenTerminal => {
            state.terminal.open = true;
            state.terminal.focused = true;
            state.presentation.route = crate::ShellRoute::Terminal;
            state.presentation.secondary_pane = None;
            state.review.open = false;
            state.shell.page = crate::ShellPage::Terminal;
            TerminalCommandOutcome::Applied
        }
        AppCommand::SetTerminalFocus(focused) => {
            state.terminal.focused = focused;
            TerminalCommandOutcome::Applied
        }
        AppCommand::SetTerminalScroll {
            offset,
            follow_tail,
        } if offset.is_finite() => {
            state.terminal.scroll_offset = offset.max(0.0);
            state.terminal.follow_tail = follow_tail;
            TerminalCommandOutcome::Applied
        }
        AppCommand::WriteTerminal { id, .. }
        | AppCommand::ResizeTerminal { id, .. }
        | AppCommand::CloseTerminal(id)
            if state.terminal.active_id == Some(id) =>
        {
            TerminalCommandOutcome::NeedsEffect
        }
        _ => TerminalCommandOutcome::Ignored,
    }
}

pub fn reduce_tool_command(state: &mut ZodeAppState, command: AppCommand) -> ToolCommandOutcome {
    let AppCommand::SetToolExpanded {
        session,
        tool_id,
        expanded,
    } = command
    else {
        return ToolCommandOutcome::Ignored;
    };
    let Some(transcript) = state.transcripts.get_mut(&session) else {
        return ToolCommandOutcome::Ignored;
    };
    let mut exists = false;
    for (index, item) in transcript.items.iter().enumerate() {
        if matches!(item, TranscriptItem::Tool(tool) if tool.id == tool_id) {
            exists = true;
            if let Some(height) = transcript.item_heights.get_mut(index) {
                *height = 0.0;
            }
        }
    }
    if !exists {
        return ToolCommandOutcome::Ignored;
    }
    state
        .tool_expanded
        .entry(session)
        .or_default()
        .insert(tool_id, expanded);
    ToolCommandOutcome::Applied
}

/// Applies session-local queue edits. Dispatch commands remain an effect so the
/// controller can remove a message only when it is ready to start the next turn.
pub fn reduce_queue_command(state: &mut ZodeAppState, command: &AppCommand) -> QueueCommandOutcome {
    let session = match command {
        AppCommand::EnqueueMessage { session, .. }
        | AppCommand::EditQueuedMessageText { session, .. }
        | AppCommand::RemoveQueuedMessage { session, .. }
        | AppCommand::ClearMessageQueue { session }
        | AppCommand::ToggleQueuedMessageMenu { session, .. }
        | AppCommand::BeginEditQueuedMessage { session, .. }
        | AppCommand::CancelQueuedMessageEdit { session }
        | AppCommand::GuideQueuedMessage { session, .. }
        | AppCommand::DispatchNextQueuedMessage { session } => session,
        _ => return QueueCommandOutcome::Ignored,
    };
    let live = state
        .threads
        .iter()
        .any(|thread| &thread.session == session)
        && state.transcripts.contains_key(session);
    if !live {
        return QueueCommandOutcome::Ignored;
    }

    match command {
        AppCommand::EnqueueMessage {
            session,
            content,
            attachments,
        } => {
            let text = content
                .iter()
                .filter_map(|content| match content {
                    zode_node_protocol::UserContent::Text { text } => Some(text.as_str()),
                    zode_node_protocol::UserContent::Image { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() && attachments.is_empty() {
                return QueueCommandOutcome::Ignored;
            }
            state
                .message_queues
                .entry(session.clone())
                .or_default()
                .enqueue(text, attachments.clone())
                .map_or(QueueCommandOutcome::Ignored, QueueCommandOutcome::Enqueued)
        }
        AppCommand::EditQueuedMessageText { session, id, text } => {
            let edited = state
                .message_queues
                .get_mut(session)
                .is_some_and(|queue| queue.edit_text(*id, text.clone()));
            if !edited {
                return QueueCommandOutcome::Ignored;
            }
            if state.current_session.as_ref() == Some(session) {
                if state.composer.editing_queued_message == Some(*id) {
                    state.composer.finish_queue_edit();
                }
                if state.composer.queue_menu == Some(*id) {
                    state.composer.queue_menu = None;
                }
            }
            QueueCommandOutcome::Applied
        }
        AppCommand::RemoveQueuedMessage { session, id } => {
            let removed = state
                .message_queues
                .get_mut(session)
                .is_some_and(|queue| queue.remove_exact(*id).is_some());
            if !removed {
                return QueueCommandOutcome::Ignored;
            }
            clear_queue_ephemeral_for_message(state, session, *id);
            QueueCommandOutcome::Applied
        }
        AppCommand::ClearMessageQueue { session } => {
            let cleared = state
                .message_queues
                .get_mut(session)
                .is_some_and(crate::MessageQueueState::clear);
            if !cleared {
                return QueueCommandOutcome::Ignored;
            }
            clear_queue_ephemeral(state, session);
            QueueCommandOutcome::Applied
        }
        AppCommand::ToggleQueuedMessageMenu { session, id } => {
            if state.current_session.as_ref() != Some(session)
                || !queue_contains(state, session, *id)
            {
                return QueueCommandOutcome::Ignored;
            }
            state.composer.queue_menu = (state.composer.queue_menu != Some(*id)).then_some(*id);
            QueueCommandOutcome::Applied
        }
        AppCommand::BeginEditQueuedMessage { session, id } => {
            if state.current_session.as_ref() != Some(session) {
                return QueueCommandOutcome::Ignored;
            }
            let Some(message) = state
                .message_queues
                .get(session)
                .and_then(|queue| queue.items.iter().find(|message| message.id == *id))
            else {
                return QueueCommandOutcome::Ignored;
            };
            let text = message.text.clone();
            state.composer.begin_queue_edit(*id, &text);
            QueueCommandOutcome::Applied
        }
        AppCommand::CancelQueuedMessageEdit { session } => {
            if state.current_session.as_ref() != Some(session)
                || state.composer.editing_queued_message.is_none()
            {
                return QueueCommandOutcome::Ignored;
            }
            state.composer.finish_queue_edit();
            QueueCommandOutcome::Applied
        }
        AppCommand::GuideQueuedMessage { .. } | AppCommand::DispatchNextQueuedMessage { .. } => {
            QueueCommandOutcome::Ignored
        }
        _ => QueueCommandOutcome::Ignored,
    }
}

fn queue_contains(
    state: &ZodeAppState,
    session: &SessionLocator,
    id: crate::QueuedMessageId,
) -> bool {
    state
        .message_queues
        .get(session)
        .is_some_and(|queue| queue.items.iter().any(|message| message.id == id))
}

fn clear_queue_ephemeral_for_message(
    state: &mut ZodeAppState,
    session: &SessionLocator,
    id: crate::QueuedMessageId,
) {
    if state.current_session.as_ref() != Some(session) {
        return;
    }
    if state.composer.queue_menu == Some(id) {
        state.composer.queue_menu = None;
    }
    if state.composer.editing_queued_message == Some(id) {
        state.composer.finish_queue_edit();
    }
}

fn clear_queue_ephemeral(state: &mut ZodeAppState, session: &SessionLocator) {
    if state.current_session.as_ref() != Some(session) {
        return;
    }
    state.composer.queue_menu = None;
    state.composer.finish_queue_edit();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommandOutcome {
    Applied,
    Ignored,
}

pub fn reduce_settings_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> SettingsCommandOutcome {
    match command {
        AppCommand::SetProjectPermissions {
            workspace_uri,
            mut tools,
        } => {
            tools.sort();
            tools.dedup();
            state
                .project_permissions
                .insert(workspace_uri, crate::LoadState::Ready(tools));
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetThemePreference(theme) => {
            state.ui_preferences.theme = theme;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetReducedMotion(reduced_motion) => {
            state.ui_preferences.reduced_motion = reduced_motion;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetHighContrast(high_contrast) => {
            state.ui_preferences.high_contrast = high_contrast;
            SettingsCommandOutcome::Applied
        }
        AppCommand::SetSettingsScroll { offset } if offset.is_finite() => {
            state.settings_scroll_offset = offset.max(0.0);
            SettingsCommandOutcome::Applied
        }
        AppCommand::RevokeProjectPermission {
            workspace_uri,
            tool,
        } => {
            let Some(crate::LoadState::Ready(tools)) =
                state.project_permissions.get_mut(&workspace_uri)
            else {
                return SettingsCommandOutcome::Ignored;
            };
            let previous_len = tools.len();
            tools.retain(|candidate| candidate != &tool);
            if tools.len() == previous_len {
                return SettingsCommandOutcome::Ignored;
            }
            SettingsCommandOutcome::Applied
        }
        _ => SettingsCommandOutcome::Ignored,
    }
}

/// Projects a canonical runtime snapshot into exactly one live session.
/// Unknown or deleted sessions are ignored so delayed query completions cannot
/// resurrect presentation state.
pub fn apply_session_runtime_options(
    state: &mut ZodeAppState,
    session: SessionLocator,
    options: RuntimeOptions,
) -> bool {
    let live = state.threads.iter().any(|thread| thread.session == session)
        && state.transcripts.contains_key(&session);
    if !live {
        return false;
    }
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .runtime_options = crate::LoadState::Ready(options.clone());
    if state.current_session.as_ref() == Some(&session) {
        sync_composer_runtime(state, &options);
    }
    true
}

fn sync_composer_runtime(state: &mut ZodeAppState, options: &RuntimeOptions) {
    state.composer.model.clone_from(&options.active_model);
    state.composer.effort.clone_from(&options.effort);
    state.composer.sandbox_label = match options.sandbox_mode {
        SandboxMode::ReadOnly => "只读",
        SandboxMode::WorkspaceWrite => "工作区写入",
        SandboxMode::Off => "完全访问",
    }
    .into();
}

/// Applies viewport state emitted by the transcript widget without involving
/// the endpoint. Measurements are scoped to the addressed session.
pub fn reduce_transcript_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> TranscriptCommandOutcome {
    match command {
        AppCommand::SetTranscriptViewport {
            session,
            scroll_offset,
            follow_tail,
        } if scroll_offset.is_finite() => {
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return TranscriptCommandOutcome::Ignored;
            };
            transcript.scroll_offset = scroll_offset.max(0.0);
            transcript.follow_tail = follow_tail;
            TranscriptCommandOutcome::Applied
        }
        AppCommand::SetTranscriptItemHeight {
            session,
            index,
            height,
        } if height.is_finite() && height > 0.0 => {
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return TranscriptCommandOutcome::Ignored;
            };
            if index >= transcript.items.len() {
                return TranscriptCommandOutcome::Ignored;
            }
            transcript.item_heights.resize(transcript.items.len(), 0.0);
            transcript.item_heights[index] = height;
            TranscriptCommandOutcome::Applied
        }
        _ => TranscriptCommandOutcome::Ignored,
    }
}

/// Applies navigation-local state immediately and identifies commands whose
/// durable session/app-state mutation must also be executed by the controller.
pub fn reduce_navigation_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> NavigationOutcome {
    if let Some(outcome) = crate::task_navigation::reduce_task_navigation(state, &command) {
        return outcome;
    }
    match command {
        AppCommand::SelectSession(session) => {
            let Some(workspace_uri) = state
                .threads
                .iter()
                .find(|thread| thread.session == session)
                .map(|thread| thread.workspace_uri.clone())
            else {
                return NavigationOutcome::Ignored;
            };
            state
                .presentation
                .sessions
                .entry(session.clone())
                .or_default();
            let changing_session = state.current_session.as_ref() != Some(&session);
            if changing_session {
                state.composer.queue_menu = None;
                state.composer.finish_queue_edit();
            }
            state.project_picker = crate::ProjectPickerState::default();
            state.current_session = Some(session.clone());
            if let Some(options) = state.presentation.sessions[&session]
                .runtime_options
                .ready()
                .cloned()
            {
                sync_composer_runtime(state, &options);
            }
            state.review.dirty = state.presentation.sessions[&session].diff.dirty;
            state.review.open =
                state.presentation.secondary_pane == Some(crate::SecondaryPane::Review);
            if state.is_projectless_workspace(&workspace_uri) {
                state.active_workspace = None;
            } else if state.available_workspace(&workspace_uri) {
                state.active_workspace = Some(workspace_uri);
            }
            NavigationOutcome::Applied
        }
        AppCommand::RenameSession { session, title } => {
            let Some(thread) = state
                .threads
                .iter_mut()
                .find(|thread| thread.session == session)
            else {
                return NavigationOutcome::Ignored;
            };
            thread.title = title;
            NavigationOutcome::NeedsEffect
        }
        AppCommand::SetSessionPinned { session, .. } => {
            if state.threads.iter().any(|thread| thread.session == session) {
                NavigationOutcome::NeedsEffect
            } else {
                NavigationOutcome::Ignored
            }
        }
        AppCommand::RequestDeleteSession(session) => {
            if !state.threads.iter().any(|thread| thread.session == session) {
                return NavigationOutcome::Ignored;
            }
            state.pending_session_delete = Some(session);
            NavigationOutcome::NeedsEffect
        }
        AppCommand::CancelDeleteSession => {
            state.pending_session_delete = None;
            NavigationOutcome::Applied
        }
        AppCommand::DeleteSession(session) => {
            if state.pending_session_delete.as_ref() != Some(&session) {
                return NavigationOutcome::Ignored;
            }
            let deleting_current = state.current_session.as_ref() == Some(&session);
            state.threads.retain(|thread| thread.session != session);
            state.transcripts.remove(&session);
            state.message_queues.remove(&session);
            state.tool_expanded.remove(&session);
            state.active_turns.remove(&session);
            state.usage.remove(&session);
            state.presentation.sessions.remove(&session);
            state
                .approvals
                .retain(|_, approval_session| approval_session != &session);
            if deleting_current {
                state.current_session = None;
                state.composer.queue_menu = None;
                state.composer.finish_queue_edit();
                state.review = crate::ReviewState::default();
                state.presentation.secondary_pane = None;
            }
            state.pending_session_delete = None;
            NavigationOutcome::NeedsEffect
        }
        AppCommand::ToggleProject(workspace_uri) => {
            let Some(project) = state
                .projects
                .iter_mut()
                .find(|project| project.workspace_uri == workspace_uri)
            else {
                return NavigationOutcome::Ignored;
            };
            project.expanded = !project.expanded;
            if project.available {
                state.active_workspace = Some(workspace_uri);
            }
            NavigationOutcome::NeedsEffect
        }
        _ => NavigationOutcome::Ignored,
    }
}

/// Applies a current-turn event while rejecting unknown or stale event streams.
pub fn reduce_agent_event(state: &mut ZodeAppState, event: AgentEvent) -> ReduceOutcome {
    let session_exists = state
        .threads
        .iter()
        .any(|thread| thread.session == event.session)
        && state.transcripts.contains_key(&event.session);
    let turn_matches = state.active_turns.get(&event.session) == Some(&event.turn_id);
    let sequence_advances = state
        .transcripts
        .get(&event.session)
        .is_some_and(|transcript| event.sequence > transcript.last_sequence);

    if !session_exists || !turn_matches || !sequence_advances {
        return ReduceOutcome::IgnoredStale;
    }

    let AgentEvent {
        session,
        sequence,
        kind,
        ..
    } = event;
    let transcript = state
        .transcripts
        .get_mut(&session)
        .expect("the session was validated before reducing its event");
    transcript.last_sequence = sequence;

    match kind {
        AgentEventKind::TextDelta { delta } => append_assistant_text(transcript, delta),
        AgentEventKind::ThinkingDelta { delta } => append_thinking(transcript, delta),
        AgentEventKind::ToolStarted { tool } => {
            let id = tool.id.clone();
            let expanded = default_tool_expanded(&tool.name);
            upsert_started_tool(transcript, tool);
            state
                .tool_expanded
                .entry(session.clone())
                .or_default()
                .entry(id)
                .or_insert(expanded);
        }
        AgentEventKind::ToolCompleted { tool } => complete_existing_tool(transcript, tool),
        AgentEventKind::ApprovalRequested {
            approval_id,
            tool,
            summary: _,
        } => {
            transcript.items.push(TranscriptItem::Approval {
                id: approval_id.clone(),
                tool,
            });
            state.approvals.insert(approval_id, session);
        }
        AgentEventKind::DiffInvalidated => {
            state
                .presentation
                .sessions
                .entry(session.clone())
                .or_default()
                .diff
                .invalidate();
            if state.current_session.as_ref() == Some(&session) {
                state.review.dirty = true;
            }
        }
        AgentEventKind::Usage { usage } => {
            state.usage.insert(session, usage);
        }
        AgentEventKind::StatusNotice { code, message } => {
            transcript
                .items
                .push(TranscriptItem::Status { code, message });
        }
        AgentEventKind::TurnFinished { interrupted: _ } => {
            transcript.busy = false;
            state.active_turns.remove(&session);
        }
        AgentEventKind::Error { message, retryable } => {
            transcript
                .items
                .push(TranscriptItem::Error { message, retryable });
        }
        AgentEventKind::Unknown => {
            transcript.items.push(TranscriptItem::Status {
                code: UNKNOWN_EVENT_CODE.to_owned(),
                message: UNKNOWN_EVENT_MESSAGE.to_owned(),
            });
        }
    }

    ReduceOutcome::Applied
}

fn append_assistant_text(transcript: &mut TranscriptState, delta: String) {
    match transcript.items.last_mut() {
        Some(TranscriptItem::AssistantText(text)) => {
            text.push_str(&delta);
            invalidate_item_height(transcript, transcript.items.len().saturating_sub(1));
        }
        _ => transcript.items.push(TranscriptItem::AssistantText(delta)),
    }
}

fn append_thinking(transcript: &mut TranscriptState, delta: String) {
    match transcript.items.last_mut() {
        Some(TranscriptItem::Thinking(text)) => {
            text.push_str(&delta);
            invalidate_item_height(transcript, transcript.items.len().saturating_sub(1));
        }
        _ => transcript.items.push(TranscriptItem::Thinking(delta)),
    }
}

fn upsert_started_tool(transcript: &mut TranscriptState, tool: ToolCall) {
    if let Some(index) = find_tool_index(transcript, &tool.id) {
        let _ = transcript.replace_item(index, TranscriptItem::Tool(tool));
    } else {
        transcript.items.push(TranscriptItem::Tool(tool));
    }
}

fn complete_existing_tool(transcript: &mut TranscriptState, tool: ToolCall) {
    if let Some(index) = find_tool_index(transcript, &tool.id) {
        let _ = transcript.replace_item(index, TranscriptItem::Tool(tool));
    }
}

fn find_tool_index(transcript: &TranscriptState, tool_id: &str) -> Option<usize> {
    transcript
        .items
        .iter()
        .position(|item| matches!(item, TranscriptItem::Tool(existing) if existing.id == tool_id))
}

fn invalidate_item_height(transcript: &mut TranscriptState, index: usize) {
    if let Some(height) = transcript.item_heights.get_mut(index) {
        *height = 0.0;
    }
}
