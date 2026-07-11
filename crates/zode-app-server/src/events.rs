use agent::stream::Event;
use zode_app_server_protocol::{notify, JsonRpcNotification};

pub fn event_to_notification(
    thread_id: &str,
    turn_id: &str,
    event: Event,
) -> Option<JsonRpcNotification> {
    match event {
        Event::TextDelta { delta } => Some(notify::agent_message_delta(thread_id, turn_id, &delta)),
        Event::ToolUse { id, name, input } => {
            Some(notify::item_started(thread_id, turn_id, &id, &name, &input))
        }
        Event::ToolResult { id, ok, output } => {
            Some(notify::item_completed(thread_id, turn_id, &id, ok, &output))
        }
        Event::Error { code, message } => {
            Some(notify::turn_error(thread_id, turn_id, &code, &message))
        }
        _ => None,
    }
}
