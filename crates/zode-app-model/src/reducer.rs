use crate::{default_tool_expanded, AppCommand, TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::{AgentEvent, AgentEventKind, ToolCall};

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
pub enum TerminalCommandOutcome {
    Applied,
    NeedsEffect,
    Ignored,
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
    let AppCommand::SetToolExpanded { tool_id, expanded } = command else {
        return ToolCommandOutcome::Ignored;
    };
    let exists = state.transcripts.values().any(|transcript| {
        transcript
            .items
            .iter()
            .any(|item| matches!(item, TranscriptItem::Tool(tool) if tool.id == tool_id))
    });
    if !exists {
        return ToolCommandOutcome::Ignored;
    }
    state.tool_expanded.insert(tool_id, expanded);
    ToolCommandOutcome::Applied
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
            if tools.is_empty() {
                state.project_permissions.remove(&workspace_uri);
            } else {
                state.project_permissions.insert(workspace_uri, tools);
            }
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
            let Some(tools) = state.project_permissions.get_mut(&workspace_uri) else {
                return SettingsCommandOutcome::Ignored;
            };
            let previous_len = tools.len();
            tools.retain(|candidate| candidate != &tool);
            if tools.len() == previous_len {
                return SettingsCommandOutcome::Ignored;
            }
            if tools.is_empty() {
                state.project_permissions.remove(&workspace_uri);
            }
            SettingsCommandOutcome::Applied
        }
        _ => SettingsCommandOutcome::Ignored,
    }
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
            state.current_session = Some(session);
            if state.available_workspace(&workspace_uri) {
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
            state.threads.retain(|thread| thread.session != session);
            state.transcripts.remove(&session);
            state.active_turns.remove(&session);
            state.usage.remove(&session);
            state
                .approvals
                .retain(|_, approval_session| approval_session != &session);
            if state.current_session.as_ref() == Some(&session) {
                state.current_session = None;
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
            state.tool_expanded.entry(id).or_insert(expanded);
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
        AgentEventKind::DiffInvalidated => state.review.dirty = true,
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
            if !retryable {
                transcript.busy = false;
            }
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
        Some(TranscriptItem::AssistantText(text)) => text.push_str(&delta),
        _ => transcript.items.push(TranscriptItem::AssistantText(delta)),
    }
}

fn append_thinking(transcript: &mut TranscriptState, delta: String) {
    match transcript.items.last_mut() {
        Some(TranscriptItem::Thinking(text)) => text.push_str(&delta),
        _ => transcript.items.push(TranscriptItem::Thinking(delta)),
    }
}

fn upsert_started_tool(transcript: &mut TranscriptState, tool: ToolCall) {
    if let Some(item) = find_tool_mut(transcript, &tool.id) {
        *item = TranscriptItem::Tool(tool);
    } else {
        transcript.items.push(TranscriptItem::Tool(tool));
    }
}

fn complete_existing_tool(transcript: &mut TranscriptState, tool: ToolCall) {
    if let Some(item) = find_tool_mut(transcript, &tool.id) {
        *item = TranscriptItem::Tool(tool);
    }
}

fn find_tool_mut<'a>(
    transcript: &'a mut TranscriptState,
    tool_id: &str,
) -> Option<&'a mut TranscriptItem> {
    transcript
        .items
        .iter_mut()
        .find(|item| matches!(item, TranscriptItem::Tool(existing) if existing.id == tool_id))
}
