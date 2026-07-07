use crate::events::event_to_notification;
use agent::stream::Event;

#[test]
fn text_delta_maps_to_agent_message_delta_notification() {
    let notification = event_to_notification(
        "thread-1",
        "turn-1",
        Event::TextDelta {
            delta: "hello".to_string(),
        },
    )
    .unwrap();
    assert_eq!(notification.method, "item/agentMessage/delta");
    assert_eq!(
        notification.params.unwrap()["delta"],
        serde_json::Value::String("hello".to_string())
    );
}

#[test]
fn tool_use_maps_to_item_started() {
    let notification = event_to_notification(
        "thread-1",
        "turn-1",
        Event::ToolUse {
            id: "tool-1".to_string(),
            name: "shell".to_string(),
            input: serde_json::json!({"cmd":"pwd"}),
        },
    )
    .unwrap();
    assert_eq!(notification.method, "item/started");
    assert_eq!(notification.params.unwrap()["item"]["id"], "tool-1");
}

#[test]
fn tool_result_maps_to_item_completed() {
    let notification = event_to_notification(
        "thread-1",
        "turn-1",
        Event::ToolResult {
            id: "tool-1".to_string(),
            ok: false,
            output: serde_json::json!({"stderr":"nope"}),
        },
    )
    .unwrap();
    let params = notification.params.unwrap();
    assert_eq!(notification.method, "item/completed");
    assert_eq!(params["item"]["id"], "tool-1");
    assert_eq!(params["item"]["status"], "failed");
    assert_eq!(
        params["item"]["output"],
        serde_json::json!({"stderr":"nope"})
    );
}

#[test]
fn error_maps_to_turn_error() {
    let notification = event_to_notification(
        "thread-1",
        "turn-1",
        Event::Error {
            code: "provider_error".to_string(),
            message: "boom".to_string(),
        },
    )
    .unwrap();
    let params = notification.params.unwrap();
    assert_eq!(notification.method, "turn/error");
    assert_eq!(params["error"]["code"], "provider_error");
    assert_eq!(params["error"]["message"], "boom");
}

#[test]
fn unsupported_event_returns_none() {
    assert!(event_to_notification(
        "thread-1",
        "turn-1",
        Event::Thinking {
            delta: "private reasoning".to_string(),
        },
    )
    .is_none());
}
