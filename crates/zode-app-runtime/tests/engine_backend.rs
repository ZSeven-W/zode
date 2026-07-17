use agent::stream::{Event, ResultData};
use zode_app_runtime::EventNormalizer;
use zode_node_protocol::{AgentEventKind, ToolStatus};

fn normalize(normalizer: &mut EventNormalizer, event: Event) -> Option<AgentEventKind> {
    normalizer.normalize(event)
}

#[test]
fn text_delta_maps_without_rewriting_content() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::TextDelta {
            delta: "hello".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::TextDelta {
            delta: "hello".into()
        })
    );
}

#[test]
fn thinking_maps_to_thinking_delta() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Thinking {
            delta: "considering".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::ThinkingDelta {
            delta: "considering".into()
        })
    );
}

#[test]
fn tool_use_emits_a_safe_summary_without_raw_arguments() {
    let mut normalizer = EventNormalizer::new();
    let secret = "raw-api-token-should-never-reach-ui";

    let event = normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-1".into(),
            name: "FileWrite".into(),
            input: serde_json::json!({
                "path": "/tmp/note.md",
                "api_token": secret,
                "content": "large raw contents",
            }),
        },
    )
    .unwrap();

    let AgentEventKind::ToolStarted { tool } = event else {
        panic!("expected ToolStarted");
    };
    assert_eq!(tool.id, "tool-1");
    assert_eq!(tool.name, "FileWrite");
    assert_eq!(tool.status, ToolStatus::Running);
    assert!(tool.summary.contains("/tmp/note.md"));
    assert_eq!(tool.detail, None);
    assert!(!format!("{tool:?}").contains(secret));
    assert!(!tool.summary.contains("large raw contents"));
}

#[test]
fn successful_tool_result_uses_the_cached_tool_name() {
    let mut normalizer = EventNormalizer::new();
    normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-ok".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "pwd"}),
        },
    );

    let event = normalize(
        &mut normalizer,
        Event::ToolResult {
            id: "tool-ok".into(),
            ok: true,
            output: serde_json::json!({"stdout": "/tmp"}),
        },
    )
    .unwrap();

    let AgentEventKind::ToolCompleted { tool } = event else {
        panic!("expected ToolCompleted");
    };
    assert_eq!(tool.id, "tool-ok");
    assert_eq!(tool.name, "Bash");
    assert_eq!(tool.status, ToolStatus::Completed);
}

#[test]
fn failed_tool_result_maps_to_failed_status() {
    let mut normalizer = EventNormalizer::new();
    normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-failed".into(),
            name: "FileRead".into(),
            input: serde_json::json!({"path": "/missing"}),
        },
    );

    let event = normalize(
        &mut normalizer,
        Event::ToolResult {
            id: "tool-failed".into(),
            ok: false,
            output: serde_json::json!({"error": "not found"}),
        },
    )
    .unwrap();

    let AgentEventKind::ToolCompleted { tool } = event else {
        panic!("expected ToolCompleted");
    };
    assert_eq!(tool.name, "FileRead");
    assert_eq!(tool.status, ToolStatus::Failed);
}

#[test]
fn usage_frames_remain_cumulative_instead_of_being_double_counted() {
    let mut normalizer = EventNormalizer::new();

    let first = normalize(
        &mut normalizer,
        Event::Usage {
            input_tokens: 10,
            output_tokens: 2,
            cache_read: 0,
            cache_create: 0,
        },
    )
    .unwrap();
    let second = normalize(
        &mut normalizer,
        Event::Usage {
            input_tokens: 15,
            output_tokens: 4,
            cache_read: 0,
            cache_create: 0,
        },
    )
    .unwrap();

    let AgentEventKind::Usage { usage: first } = first else {
        panic!("expected Usage");
    };
    let AgentEventKind::Usage { usage: second } = second else {
        panic!("expected Usage");
    };
    assert_eq!((first.input_tokens, first.output_tokens), (10, 2));
    assert_eq!((second.input_tokens, second.output_tokens), (15, 4));
}

#[test]
fn notice_preserves_the_diagnostic_code_and_message() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Notice {
            code: "api_retry".into(),
            message: "retrying request".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::StatusNotice {
            code: "api_retry".into(),
            message: "retrying request".into(),
        })
    );
}

#[test]
fn recoverable_agent_error_keeps_the_stream_retryable() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Error {
            code: "provider_error".into(),
            message: "request failed".into(),
        },
    )
    .unwrap();

    let AgentEventKind::Error { message, retryable } = event else {
        panic!("expected Error");
    };
    assert!(message.contains("request failed"));
    assert!(retryable);
}

#[test]
fn unknown_event_becomes_a_diagnostic_notice() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(&mut normalizer, Event::Unknown);

    assert!(matches!(
        event,
        Some(AgentEventKind::StatusNotice { code, message })
            if code == "agent.event.unknown" && !message.is_empty()
    ));
}

#[test]
fn result_metadata_does_not_finish_the_turn_early() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Result {
            data: ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("test-model".into()),
                metadata: Default::default(),
            },
        },
    );

    assert_eq!(event, None);
}
