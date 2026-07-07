use agent::stream::Event;
use serde_json::json;
use zode_app_server_protocol::JsonRpcNotification;

pub fn event_to_notification(
    thread_id: &str,
    turn_id: &str,
    event: Event,
) -> Option<JsonRpcNotification> {
    let params_prefix = |item: serde_json::Value| {
        Some(json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "item": item
        }))
    };
    match event {
        Event::TextDelta { delta } => Some(JsonRpcNotification {
            method: "item/agentMessage/delta".to_string(),
            params: Some(json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "delta": delta
            })),
        }),
        Event::ToolUse { id, name, input } => Some(JsonRpcNotification {
            method: "item/started".to_string(),
            params: params_prefix(json!({
                "id": id,
                "type": "dynamicToolCall",
                "tool": name,
                "arguments": input,
                "status": "inProgress"
            })),
        }),
        Event::ToolResult { id, ok, output } => Some(JsonRpcNotification {
            method: "item/completed".to_string(),
            params: params_prefix(json!({
                "id": id,
                "type": "dynamicToolCall",
                "status": if ok { "completed" } else { "failed" },
                "output": output
            })),
        }),
        Event::Error { code, message } => Some(JsonRpcNotification {
            method: "turn/error".to_string(),
            params: Some(json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "error": {"code": code, "message": message}
            })),
        }),
        _ => None,
    }
}
