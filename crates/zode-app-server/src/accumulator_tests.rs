use crate::accumulator::{TurnAccumulator, TurnEndState};
use agent::stream::Event;
use serde_json::json;

#[test]
fn final_text_is_last_segment() {
    let mut a = TurnAccumulator::new("t", "u");
    a.on_event(&Event::TextDelta {
        delta: "draft".into(),
    });
    a.on_event(&Event::ToolUse {
        id: "c1".into(),
        name: "Bash".into(),
        input: json!({}),
    });
    a.on_event(&Event::ToolResult {
        id: "c1".into(),
        ok: true,
        output: json!({}),
    });
    a.on_event(&Event::TextDelta {
        delta: "final ".into(),
    });
    a.on_event(&Event::TextDelta {
        delta: "answer".into(),
    });
    let out = a.finish(TurnEndState::Completed);
    assert_eq!(out.final_text, "final answer");
}

#[test]
fn usage_accumulates_across_events() {
    let mut a = TurnAccumulator::new("t", "u");
    for _ in 0..2 {
        a.on_event(&Event::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_read: 1,
            cache_create: 0,
        });
    }
    assert_eq!(a.finish(TurnEndState::Completed).usage.output_tokens, 10);
}

#[test]
fn error_event_is_not_terminal() {
    let mut a = TurnAccumulator::new("t", "u");
    let n = a.on_event(&Event::Error {
        code: "rate_limit".into(),
        message: "x".into(),
    });
    assert_eq!(n[0].method, "turn/error");
}

#[test]
fn item_ids_are_stable_and_mapped() {
    let mut a = TurnAccumulator::new("t", "u7");
    let started = a.on_event(&Event::ToolUse {
        id: "call_9".into(),
        name: "Read".into(),
        input: json!({}),
    });
    let done = a.on_event(&Event::ToolResult {
        id: "call_9".into(),
        ok: true,
        output: json!({}),
    });
    assert_eq!(started[0].params.as_ref().unwrap()["itemId"], "u7-item-0");
    assert_eq!(done[0].params.as_ref().unwrap()["itemId"], "u7-item-0");
}
