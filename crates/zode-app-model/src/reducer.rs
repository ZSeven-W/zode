use crate::{TranscriptItem, TranscriptState, ZodeAppState};
use zode_node_protocol::{AgentEvent, AgentEventKind, ToolCall};

const UNKNOWN_EVENT_CODE: &str = "agent.event.unknown";
const UNKNOWN_EVENT_MESSAGE: &str = "Ignored an unknown agent event";

/// Result of reducing an ordered agent event into application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOutcome {
    Applied,
    IgnoredStale,
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
        AgentEventKind::ToolStarted { tool } => upsert_started_tool(transcript, tool),
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
