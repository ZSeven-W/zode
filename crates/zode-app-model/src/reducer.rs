use crate::{default_tool_expanded, AppCommand, TranscriptItem, TranscriptState, ZodeAppState};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, BackgroundProcessSnapshot, RuntimeOptions, SessionLocator,
    SubagentSnapshot, SubagentStatus, ToolCall,
};

#[path = "reducer/presentation-integrations.rs"]
mod presentation_integrations;
#[path = "reducer/presentation-plugin-market.rs"]
mod presentation_plugin_market;
#[path = "reducer/subagent-chips.rs"]
mod subagent_chips;
#[path = "reducer/transcript-find.rs"]
mod transcript_find;

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
    if let Some(outcome) =
        presentation_integrations::reduce_integration_presentation_command(state, &command)
    {
        return outcome;
    }
    if let Some(outcome) =
        presentation_plugin_market::reduce_plugin_market_presentation_command(state, &command)
    {
        return outcome;
    }
    let previous_route = state.presentation.route;
    let route = match command {
        AppCommand::Navigate(route) => Some(route),
        AppCommand::SelectSettingsCategory(category) => Some(crate::ShellRoute::Settings(category)),
        AppCommand::SelectIntegrationsTab(tab) => Some(crate::ShellRoute::Integrations(tab)),
        AppCommand::TogglePrimarySidebar => {
            if matches!(state.presentation.route, crate::ShellRoute::Settings(_)) {
                return PresentationCommandOutcome::Ignored;
            }
            state.shell.sidebar_open = !state.shell.sidebar_open;
            state.ui_preferences.primary_sidebar_open = state.shell.sidebar_open;
            state.close_session_action_surfaces();
            state.composer.queue_menu = None;
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::SetPrimarySidebarWidth(width) => {
            state.ui_preferences.primary_sidebar_width = width.clamp(
                crate::MIN_PRIMARY_SIDEBAR_WIDTH,
                crate::MAX_PRIMARY_SIDEBAR_WIDTH,
            );
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::ToggleSidebar => {
            if state.presentation.route != crate::ShellRoute::Conversation {
                return PresentationCommandOutcome::Ignored;
            }
            state.presentation.secondary_sidebar_open = !state.presentation.secondary_sidebar_open;
            state.close_session_action_surfaces();
            state.composer.queue_menu = None;
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::SetSecondarySidebarWidth(width) => {
            state.ui_preferences.secondary_sidebar_width = width.clamp(
                crate::MIN_SECONDARY_SIDEBAR_WIDTH,
                crate::MAX_SECONDARY_SIDEBAR_WIDTH,
            );
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::OpenSecondary(pane) => {
            open_secondary(state, pane);
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::CloseSecondary => {
            close_secondary(state);
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::SetPinnedSummaryAutoHidden(hidden) => {
            if state.presentation.route != crate::ShellRoute::Conversation {
                return PresentationCommandOutcome::Ignored;
            }
            state.presentation.pinned_summary_auto_hidden = hidden;
            if hidden {
                state.presentation.pinned_summary_overlay_open = false;
            }
            return PresentationCommandOutcome::Applied;
        }
        AppCommand::SetPinnedSummaryOverlayOpen(open) => {
            if state.presentation.route != crate::ShellRoute::Conversation {
                return PresentationCommandOutcome::Ignored;
            }
            state.presentation.pinned_summary_overlay_open = open;
            if open {
                state.presentation.pinned_summary_auto_hidden = false;
                state.close_session_action_surfaces();
                state.composer.queue_menu = None;
            }
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
        AppCommand::ShowMoreCompletedSubagents { session } => {
            let presentation = state.presentation.sessions.entry(session).or_default();
            presentation.subagents_shown =
                crate::subagents_visible_count(presentation) + crate::SUBAGENTS_PAGE_SIZE;
            return PresentationCommandOutcome::Applied;
        }
        _ => None,
    };

    let Some(route) = route else {
        return PresentationCommandOutcome::Ignored;
    };
    if route != crate::ShellRoute::Conversation {
        close_secondary(state);
        state.presentation.pinned_summary_overlay_open = false;
    }
    if matches!(route, crate::ShellRoute::Integrations(_)) && route != previous_route {
        state.integration_scroll_offset = 0.0;
    }
    state.close_session_action_surfaces();
    state.presentation.route = route;
    state.shell.page = route.legacy_page();
    PresentationCommandOutcome::Applied
}

fn open_secondary(state: &mut ZodeAppState, pane: crate::SecondaryPane) {
    state.close_session_action_surfaces();
    state.composer.queue_menu = None;
    state.presentation.route = crate::ShellRoute::Conversation;
    if pane == crate::SecondaryPane::Environment {
        state.presentation.pinned_summary_auto_hidden = false;
        state.presentation.pinned_summary_overlay_open = true;
        state.shell.page = crate::ShellPage::Conversation;
        return;
    }
    state.presentation.secondary_pane = Some(pane);
    state.presentation.secondary_sidebar_open = true;
    state.shell.page = crate::ShellPage::Conversation;
    state.review.open = pane == crate::SecondaryPane::Review;
    state.terminal.open = pane == crate::SecondaryPane::Terminal;
    state.terminal.focused = pane == crate::SecondaryPane::Terminal;
}

fn close_secondary(state: &mut ZodeAppState) {
    state.presentation.secondary_pane = None;
    state.review.open = false;
    state.terminal.open = false;
    state.terminal.focused = false;
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
    // The toggle changes how an unmeasured item's height is *estimated*
    // (expanded vs. collapsed), even when the cached height above was
    // already 0.0, so cached layout must be invalidated unconditionally.
    transcript.touch_layout();
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
        | AppCommand::ReorderQueuedMessage { session, .. }
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
            front,
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
            let queue = state.message_queues.entry(session.clone()).or_default();
            let enqueued = if *front {
                queue.enqueue_front(text, attachments.clone())
            } else {
                queue.enqueue(text, attachments.clone())
            };
            enqueued.map_or(QueueCommandOutcome::Ignored, QueueCommandOutcome::Enqueued)
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
            state.close_session_action_surfaces();
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
        // The message being edited is tracked by id (`composer.editing_queued_message`),
        // never by position, so reordering never needs to touch composer ephemeral state.
        AppCommand::ReorderQueuedMessage { session, id, op } => {
            let applied = state
                .message_queues
                .get_mut(session)
                .is_some_and(|queue| match op {
                    crate::QueueReorderOp::Front => queue.move_to_front(*id),
                    crate::QueueReorderOp::Up => queue.move_up(*id),
                    crate::QueueReorderOp::Down => queue.move_down(*id),
                });
            if !applied {
                return QueueCommandOutcome::Ignored;
            }
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
    state.composer.available_models.clone_from(&options.models);
    state.composer.approval_mode = options.approval_mode;
    state.composer.sandbox_mode = options.sandbox_mode;
    state.composer.sandbox_network = options.sandbox_network;
    state.composer.sandbox_label = match options.approval_mode {
        zode_node_protocol::ApprovalMode::Request => "请求批准",
        zode_node_protocol::ApprovalMode::Auto => "替我审批",
        zode_node_protocol::ApprovalMode::Full => "完全访问",
    }
    .into();
    state.composer.footer_menu = None;
}

/// Applies viewport state emitted by the transcript widget without involving
/// the endpoint. Measurements are scoped to the addressed session.
pub fn reduce_transcript_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> TranscriptCommandOutcome {
    if let Some(outcome) = transcript_find::reduce_transcript_find_command(state, &command) {
        return outcome;
    }
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
            transcript.touch_layout();
            TranscriptCommandOutcome::Applied
        }
        AppCommand::SetTranscriptImageDimensions {
            session,
            item_id,
            width,
            height,
        } if width > 0 && height > 0 => {
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return TranscriptCommandOutcome::Ignored;
            };
            let Some(TranscriptItem::Image(image)) = transcript
                .items
                .iter_mut()
                .find(|item| matches!(item, TranscriptItem::Image(image) if image.id == item_id))
            else {
                return TranscriptCommandOutcome::Ignored;
            };
            if image.width == Some(width) && image.height == Some(height) {
                return TranscriptCommandOutcome::Ignored;
            }
            image.width = Some(width);
            image.height = Some(height);
            transcript.touch_layout();
            TranscriptCommandOutcome::Applied
        }
        AppCommand::SetMessageFeedback {
            session,
            index,
            feedback,
        } => {
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return TranscriptCommandOutcome::Ignored;
            };
            let Some(TranscriptItem::AssistantText { feedback: slot, .. }) =
                transcript.items.get_mut(index)
            else {
                return TranscriptCommandOutcome::Ignored;
            };
            // A reaction toggle never changes measured content, so it does
            // not need `touch_layout()` - unlike `SetToolExpanded`, which
            // changes how an unmeasured item's height is estimated.
            *slot = feedback;
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
    if let Some(outcome) = crate::open_with_navigation::reduce_open_with_navigation(state, &command)
    {
        return outcome;
    }
    if let Some(outcome) = crate::task_navigation::reduce_task_navigation(state, &command) {
        return outcome;
    }
    if let Some(outcome) = crate::sidebar_navigation::reduce_sidebar_navigation(state, &command) {
        return outcome;
    }
    if let Some(outcome) = crate::session_navigation::reduce_session_navigation(state, &command) {
        return outcome;
    }
    match command {
        AppCommand::SelectSession(session) => {
            let Some(_) = state
                .threads
                .iter()
                .find(|thread| thread.session == session)
            else {
                return NavigationOutcome::Ignored;
            };
            let project_workspace = state.project_workspace_for_session(&session).cloned();
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
            state.close_session_action_surfaces();
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
            state.active_workspace =
                project_workspace.filter(|workspace_uri| state.available_workspace(workspace_uri));
            NavigationOutcome::Applied
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
            state.thread_workspace_root_hints.remove(&session);
            state.projectless_sessions.remove(&session);
            state.pinned_sessions.remove(&session);
            state.archived_sessions.remove(&session);
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
                state.close_session_action_surfaces();
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
            state.close_session_action_surfaces();
            NavigationOutcome::NeedsEffect
        }
        _ => NavigationOutcome::Ignored,
    }
}

/// Applies a current-turn event while rejecting unknown or stale event streams.
pub fn reduce_agent_event(state: &mut ZodeAppState, event: AgentEvent) -> ReduceOutcome {
    reduce_agent_event_at(state, event, Instant::now())
}

/// Deterministic-clock variant used by the event bridge and model tests.
pub fn reduce_agent_event_at(
    state: &mut ZodeAppState,
    event: AgentEvent,
    received_at: Instant,
) -> ReduceOutcome {
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
        turn_id,
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
        AgentEventKind::ToolCompleted { tool } => {
            // Captured before `complete_existing_tool` consumes `tool`, so a
            // completed `FileRead`/`FileWrite`/`FileEdit` on a recognized
            // image path also surfaces as an inline `Image` item right after
            // its tool card - see `TranscriptItem::image_from_completed_tool`.
            let image = TranscriptItem::image_from_completed_tool(&tool);
            complete_existing_tool(transcript, tool);
            if let Some(image) = image {
                transcript.items.push(image);
                transcript.touch_layout();
            }
        }
        AgentEventKind::ApprovalRequested {
            approval_id,
            tool,
            summary: _,
        } => {
            transcript.items.push(TranscriptItem::Approval {
                id: approval_id.clone(),
                tool,
            });
            transcript.touch_layout();
            state.approvals.insert(approval_id, session);
        }
        AgentEventKind::SubagentUpdate { subagent } => {
            subagent_chips::apply_subagent_chip(transcript, &subagent);
            upsert_subagent(
                &mut state
                    .presentation
                    .sessions
                    .entry(session.clone())
                    .or_default()
                    .subagents,
                subagent,
            );
        }
        AgentEventKind::BackgroundProcessUpdate { process } => {
            upsert_background_process(
                &mut state
                    .presentation
                    .sessions
                    .entry(session.clone())
                    .or_default()
                    .background_processes,
                process,
            );
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
            transcript.touch_layout();
        }
        AgentEventKind::TurnFinished { interrupted } => {
            // Safety net: the runtime already emits an authoritative final
            // `SubagentUpdate` from the registry before `TurnFinished`, but a
            // dangling `Running` row must never linger if that diff was
            // missed (e.g. the registry itself hadn't observed the abort yet).
            // Runs before the turn is closed below so a chip written by that
            // correction still lands inside this turn's item range, ahead of
            // the turn divider rather than orphaned after it.
            if let Some(presentation) = state.presentation.sessions.get_mut(&session) {
                finalize_running_subagents(&mut presentation.subagents, interrupted);
                subagent_chips::finalize_subagent_chips(transcript, &presentation.subagents);
            }
            let _ = transcript.finish_turn_at(turn_id, interrupted, received_at);
            transcript.busy = false;
            state.active_turns.remove(&session);
        }
        AgentEventKind::Error { message, retryable } => {
            if !retryable {
                let _ = transcript.record_terminal_error(turn_id);
            }
            transcript
                .items
                .push(TranscriptItem::Error { message, retryable });
            transcript.touch_layout();
        }
        AgentEventKind::Unknown => {
            transcript.items.push(TranscriptItem::Status {
                code: UNKNOWN_EVENT_CODE.to_owned(),
                message: UNKNOWN_EVENT_MESSAGE.to_owned(),
            });
            transcript.touch_layout();
        }
    }

    ReduceOutcome::Applied
}

/// Inserts or replaces a sub-agent entry by id, preserving first-appearance
/// order (registry ids are not lexicographically stable, so a `BTreeMap`
/// would reorder rows as new agents spawn).
fn upsert_subagent(subagents: &mut Vec<SubagentSnapshot>, subagent: SubagentSnapshot) {
    match subagents
        .iter_mut()
        .find(|existing| existing.id == subagent.id)
    {
        Some(existing) => *existing = subagent,
        None => subagents.push(subagent),
    }
}

/// Inserts or replaces a background-process entry by id, mirroring
/// `upsert_subagent`'s first-appearance ordering.
fn upsert_background_process(
    processes: &mut Vec<BackgroundProcessSnapshot>,
    process: BackgroundProcessSnapshot,
) {
    match processes
        .iter_mut()
        .find(|existing| existing.id == process.id)
    {
        Some(existing) => *existing = process,
        None => processes.push(process),
    }
}

/// Corrects any entry still `Running` to a terminal status once its turn has
/// ended, so the UI never shows a sub-agent as running forever. Also stamps
/// `completed_at_ms` when the runtime's own authoritative diff never arrived
/// (this is a safety net, not the primary source of that timestamp - see
/// `EventNormalizer::diff_subagents` in zode-app-runtime).
fn finalize_running_subagents(subagents: &mut [SubagentSnapshot], interrupted: bool) {
    let terminal = if interrupted {
        SubagentStatus::Failed
    } else {
        SubagentStatus::Completed
    };
    for subagent in subagents.iter_mut() {
        if subagent.status == SubagentStatus::Running {
            subagent.status = terminal;
            subagent.completed_at_ms.get_or_insert_with(now_epoch_ms);
        }
    }
}

fn append_assistant_text(transcript: &mut TranscriptState, delta: String) {
    match transcript.items.last_mut() {
        Some(TranscriptItem::AssistantText { text, .. }) => {
            text.push_str(&delta);
            invalidate_item_height(transcript, transcript.items.len().saturating_sub(1));
        }
        _ => transcript
            .items
            .push(TranscriptItem::assistant_text_at(delta, now_epoch_ms())),
    }
    transcript.touch_layout();
}

/// Wall-clock read for `TranscriptItem::timestamp_ms`, taken directly at the
/// point a message is first created - this crate has no injected clock for
/// calendar time (only `Instant`, used for turn *durations*), so this
/// mirrors how the desktop app already reads wall time elsewhere (e.g.
/// `command_projection::now_ms` in the `zode-app` crate).
fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn append_thinking(transcript: &mut TranscriptState, delta: String) {
    match transcript.items.last_mut() {
        Some(TranscriptItem::Thinking(text)) => {
            text.push_str(&delta);
            invalidate_item_height(transcript, transcript.items.len().saturating_sub(1));
        }
        _ => transcript.items.push(TranscriptItem::Thinking(delta)),
    }
    transcript.touch_layout();
}

fn upsert_started_tool(transcript: &mut TranscriptState, tool: ToolCall) {
    if let Some(index) = find_tool_index(transcript, &tool.id) {
        let _ = transcript.replace_item(index, TranscriptItem::Tool(tool));
    } else {
        transcript.items.push(TranscriptItem::Tool(tool));
        transcript.touch_layout();
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
