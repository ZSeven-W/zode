use crate::rpc::JsonRpcNotification;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

fn notification(method: &str, params: Value) -> JsonRpcNotification {
    JsonRpcNotification::new(method, Some(params))
}

pub fn turn_started(thread_id: &str, turn_id: &str) -> JsonRpcNotification {
    notification(
        "turn/started",
        json!({"threadId": thread_id, "turnId": turn_id}),
    )
}

pub fn agent_message_delta(thread_id: &str, turn_id: &str, delta: &str) -> JsonRpcNotification {
    notification(
        "item/agentMessage/delta",
        json!({"threadId": thread_id, "turnId": turn_id, "delta": delta}),
    )
}

pub fn item_started(
    thread_id: &str,
    turn_id: &str,
    item_id: &str,
    tool: &str,
    input: &Value,
) -> JsonRpcNotification {
    notification(
        "item/started",
        json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "itemId": item_id,
            "item": {
                "id": item_id,
                "type": "dynamicToolCall",
                "tool": tool,
                "arguments": input,
                "status": "inProgress",
            },
        }),
    )
}

pub fn item_completed(
    thread_id: &str,
    turn_id: &str,
    item_id: &str,
    ok: bool,
    output: &Value,
) -> JsonRpcNotification {
    notification(
        "item/completed",
        json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "itemId": item_id,
            "item": {
                "id": item_id,
                "type": "dynamicToolCall",
                "status": if ok { "completed" } else { "failed" },
                "output": output,
            },
        }),
    )
}

pub fn turn_error(
    thread_id: &str,
    turn_id: &str,
    code: &str,
    message: &str,
) -> JsonRpcNotification {
    notification(
        "turn/error",
        json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "error": {"code": code, "message": message},
        }),
    )
}

pub fn turn_completed(
    thread_id: &str,
    turn_id: &str,
    final_text: &str,
    usage: &TurnUsage,
) -> JsonRpcNotification {
    notification(
        "turn/completed",
        json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "finalText": final_text,
            "usage": usage,
        }),
    )
}

pub fn turn_interrupted(thread_id: &str, turn_id: &str) -> JsonRpcNotification {
    notification(
        "turn/interrupted",
        json!({"threadId": thread_id, "turnId": turn_id}),
    )
}

pub fn turn_failed(thread_id: &str, turn_id: &str, error: &str) -> JsonRpcNotification {
    notification(
        "turn/failed",
        json!({"threadId": thread_id, "turnId": turn_id, "error": error}),
    )
}
