use serde_json::{json, Value};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEvent, AgentEventKind, ApprovalDecision, EndpointError,
    EndpointErrorKind, NodeId, ProtocolError, RuntimeOptions, SandboxMode, SessionLocator,
    TerminalId, ToolCall, ToolStatus, TurnId, UsageSnapshot, UserContent, WorkspaceUri,
    PROTOCOL_VERSION,
};

const NODE_ID: &str = "00000000-0000-0000-0000-000000000001";
const TURN_ID: &str = "00000000-0000-0000-0000-000000000002";

fn session() -> SessionLocator {
    SessionLocator::new(NodeId::parse(NODE_ID).unwrap(), "session-1")
}

fn turn_id() -> TurnId {
    TurnId::parse(TURN_ID).unwrap()
}

fn command(kind: AgentCommandKind, turn_id: Option<TurnId>) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session: session(),
        turn_id,
        kind,
    }
}

fn event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        version: PROTOCOL_VERSION,
        session: session(),
        turn_id: turn_id(),
        sequence: 7,
        kind,
    }
}

fn command_envelope(command_type: &str) -> Value {
    json!({
        "version": PROTOCOL_VERSION,
        "session": {
            "nodeId": NODE_ID,
            "sessionId": "session-1"
        },
        "type": command_type
    })
}

fn event_envelope(event_type: &str) -> Value {
    json!({
        "version": PROTOCOL_VERSION,
        "session": {
            "nodeId": NODE_ID,
            "sessionId": "session-1"
        },
        "turnId": TURN_ID,
        "sequence": 7,
        "type": event_type
    })
}

#[test]
fn all_command_variants_have_golden_camel_case_wire_shapes() {
    let cases = vec![
        (
            command(
                AgentCommandKind::CreateSession {
                    workspace_uri: WorkspaceUri::new("file:///Users/fini/project").unwrap(),
                    model: None,
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "createSession",
                "workspaceUri": "file:///Users/fini/project",
                "model": null
            }),
        ),
        (
            command(
                AgentCommandKind::RenameSession {
                    title: "Desktop protocol".into(),
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "renameSession",
                "title": "Desktop protocol"
            }),
        ),
        (
            command(AgentCommandKind::DeleteSession, None),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "deleteSession"
            }),
        ),
        (
            command(
                AgentCommandKind::StartTurn {
                    input: vec![UserContent::Text {
                        text: "hello".into(),
                    }],
                },
                Some(turn_id()),
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "type": "startTurn",
                "input": [{ "type": "text", "text": "hello" }]
            }),
        ),
        (
            command(
                AgentCommandKind::SteerTurn {
                    input: vec![UserContent::Image {
                        mime_type: "image/png".into(),
                        data_base64: "aGVsbG8=".into(),
                        display_name: "reference.png".into(),
                    }],
                },
                Some(turn_id()),
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "type": "steerTurn",
                "input": [{
                    "type": "image",
                    "mimeType": "image/png",
                    "dataBase64": "aGVsbG8=",
                    "displayName": "reference.png"
                }]
            }),
        ),
        (
            command(AgentCommandKind::InterruptTurn, Some(turn_id())),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "type": "interruptTurn"
            }),
        ),
        (
            command(
                AgentCommandKind::Approve {
                    approval_id: "approval-1".into(),
                    decision: ApprovalDecision::AllowOnce,
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "approve",
                "approvalId": "approval-1",
                "decision": "allowOnce"
            }),
        ),
        (
            command(
                AgentCommandKind::RevokeProjectPermission {
                    workspace_uri: WorkspaceUri::new("zode-node://desktop/workspace").unwrap(),
                    tool: "shell".into(),
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "revokeProjectPermission",
                "workspaceUri": "zode-node://desktop/workspace",
                "tool": "shell"
            }),
        ),
        (
            command(
                AgentCommandKind::SetModel {
                    model: "gpt-5".into(),
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "setModel",
                "model": "gpt-5"
            }),
        ),
        (
            command(
                AgentCommandKind::SetEffort {
                    effort: "high".into(),
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "setEffort",
                "effort": "high"
            }),
        ),
        (
            command(
                AgentCommandKind::SetSandbox {
                    mode: SandboxMode::WorkspaceWrite,
                    network: true,
                },
                None,
            ),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "type": "setSandbox",
                "mode": "workspaceWrite",
                "network": true
            }),
        ),
    ];

    assert_eq!(cases.len(), 11);
    for (command, expected) in cases {
        assert_eq!(serde_json::to_value(&command).unwrap(), expected);
        let decoded = AgentCommand::decode_json(&expected.to_string()).unwrap();
        assert_eq!(decoded, command);
        let serde_decoded = serde_json::from_value::<AgentCommand>(expected).unwrap();
        assert_eq!(serde_decoded, command);
    }
}

#[test]
fn all_known_event_variants_have_golden_camel_case_wire_shapes() {
    let running_tool = ToolCall {
        id: "tool-1".into(),
        name: "shell".into(),
        status: ToolStatus::Running,
        summary: "Run cargo check".into(),
        detail: Some("cargo check -p zode-node-protocol".into()),
    };
    let completed_tool = ToolCall {
        status: ToolStatus::Completed,
        detail: None,
        ..running_tool.clone()
    };
    let cases = vec![
        (
            event(AgentEventKind::TextDelta {
                delta: "hello".into(),
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "textDelta",
                "delta": "hello"
            }),
        ),
        (
            event(AgentEventKind::ThinkingDelta {
                delta: "reasoning".into(),
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "thinkingDelta",
                "delta": "reasoning"
            }),
        ),
        (
            event(AgentEventKind::ToolStarted {
                tool: running_tool.clone(),
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "toolStarted",
                "tool": {
                    "id": "tool-1",
                    "name": "shell",
                    "status": "running",
                    "summary": "Run cargo check",
                    "detail": "cargo check -p zode-node-protocol"
                }
            }),
        ),
        (
            event(AgentEventKind::ToolCompleted {
                tool: completed_tool,
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "toolCompleted",
                "tool": {
                    "id": "tool-1",
                    "name": "shell",
                    "status": "completed",
                    "summary": "Run cargo check",
                    "detail": null
                }
            }),
        ),
        (
            event(AgentEventKind::ApprovalRequested {
                approval_id: "approval-1".into(),
                tool: "shell".into(),
                summary: "Allow command?".into(),
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "approvalRequested",
                "approvalId": "approval-1",
                "tool": "shell",
                "summary": "Allow command?"
            }),
        ),
        (
            event(AgentEventKind::DiffInvalidated),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "diffInvalidated"
            }),
        ),
        (
            event(AgentEventKind::Usage {
                usage: UsageSnapshot {
                    input_tokens: 100,
                    output_tokens: 20,
                    context_used: Some(0.5),
                    cost_usd: None,
                },
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "usage",
                "usage": {
                    "inputTokens": 100,
                    "outputTokens": 20,
                    "contextUsed": 0.5,
                    "costUsd": null
                }
            }),
        ),
        (
            event(AgentEventKind::StatusNotice {
                code: "reconnecting".into(),
                message: "Reconnecting to model".into(),
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "statusNotice",
                "code": "reconnecting",
                "message": "Reconnecting to model"
            }),
        ),
        (
            event(AgentEventKind::TurnFinished { interrupted: false }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "turnFinished",
                "interrupted": false
            }),
        ),
        (
            event(AgentEventKind::Error {
                message: "model unavailable".into(),
                retryable: true,
            }),
            json!({
                "version": 1,
                "session": { "nodeId": NODE_ID, "sessionId": "session-1" },
                "turnId": TURN_ID,
                "sequence": 7,
                "type": "error",
                "message": "model unavailable",
                "retryable": true
            }),
        ),
    ];

    assert_eq!(cases.len(), 10);
    for (event, expected) in cases {
        assert_eq!(serde_json::to_value(&event).unwrap(), expected);
        let decoded = AgentEvent::decode_json(&expected.to_string()).unwrap();
        assert_eq!(decoded, event);
        let serde_decoded = serde_json::from_value::<AgentEvent>(expected).unwrap();
        assert_eq!(serde_decoded, event);
    }
}

#[test]
fn known_command_variants_ignore_future_fields() {
    let mut value = command_envelope("deleteSession");
    value["futureField"] = json!({ "nested": true });

    let decoded = AgentCommand::decode_json(&value.to_string()).unwrap();
    assert!(matches!(decoded.kind, AgentCommandKind::DeleteSession));
}

#[test]
fn known_event_variants_ignore_future_fields() {
    let mut value = event_envelope("textDelta");
    value["delta"] = json!("hello");
    value["futureField"] = json!({ "nested": true });

    let decoded = AgentEvent::decode_json(&value.to_string()).unwrap();
    assert!(matches!(
        decoded.kind,
        AgentEventKind::TextDelta { ref delta } if delta == "hello"
    ));
}

#[test]
fn unknown_event_tags_decode_to_unknown_for_diagnostics() {
    let mut value = event_envelope("futureEvent");
    value["payload"] = json!({ "meaning": 42 });

    let decoded = AgentEvent::decode_json(&value.to_string()).unwrap();
    assert_eq!(decoded.sequence, 7);
    assert!(matches!(decoded.kind, AgentEventKind::Unknown));
}

#[test]
fn uuid_ids_use_canonical_uuid_strings_on_the_wire() {
    let node = NodeId::parse("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE").unwrap();
    let turn = TurnId::parse("11111111-2222-3333-4444-AAAAAAAAAAAA").unwrap();
    let terminal = TerminalId::parse("FFFFFFFF-EEEE-DDDD-CCCC-BBBBBBBBBBBB").unwrap();

    assert_eq!(
        serde_json::to_value(node).unwrap(),
        json!("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
    assert_eq!(
        serde_json::to_value(turn).unwrap(),
        json!("11111111-2222-3333-4444-aaaaaaaaaaaa")
    );
    assert_eq!(
        serde_json::to_value(terminal).unwrap(),
        json!("ffffffff-eeee-dddd-cccc-bbbbbbbbbbbb")
    );
}

#[test]
fn option_wire_policy_omits_command_turn_id_but_keeps_dto_nulls() {
    let command = command(
        AgentCommandKind::CreateSession {
            workspace_uri: WorkspaceUri::new("file:///tmp/project").unwrap(),
            model: None,
        },
        None,
    );
    let command_value = serde_json::to_value(command).unwrap();
    assert!(command_value.get("turnId").is_none());
    assert_eq!(command_value["model"], Value::Null);

    let options = RuntimeOptions {
        models: vec!["gpt-5".into()],
        active_model: None,
        effort: None,
        sandbox_mode: SandboxMode::ReadOnly,
        sandbox_network: false,
    };
    assert_eq!(
        serde_json::to_value(options).unwrap(),
        json!({
            "models": ["gpt-5"],
            "activeModel": null,
            "effort": null,
            "sandboxMode": "readOnly",
            "sandboxNetwork": false
        })
    );
}

#[test]
fn workspace_uri_accepts_only_supported_schemes() {
    for valid in [
        "file:///tmp/project",
        "file:///",
        "file:///tmp/my%20project",
        "zode-node://phone/project",
    ] {
        assert_eq!(WorkspaceUri::new(valid).unwrap().as_str(), valid);
        assert_eq!(
            WorkspaceUri::try_from(valid.to_owned()).unwrap().as_str(),
            valid
        );
    }

    for invalid in [
        "/tmp/project",
        "https://example.com/project",
        "file:/tmp",
        "file://",
        "file://relative",
        "file:///tmp/my project",
        "zode-node://",
        "zode-node://phone",
        "zode-node:///project",
        "zode-node://phone/",
        "zode-node://phone/my project",
    ] {
        assert!(matches!(
            WorkspaceUri::new(invalid),
            Err(ProtocolError::InvalidWorkspaceUri(value)) if value == invalid
        ));
        assert!(serde_json::from_value::<WorkspaceUri>(json!(invalid)).is_err());
    }
}

#[test]
fn invalid_workspace_uri_wire_is_a_protocol_decode_error() {
    for invalid in [
        "file://",
        "file://relative",
        "file:///tmp/my project",
        "zode-node://",
        "zode-node://phone",
        "zode-node:///project",
        "zode-node://phone/",
        "zode-node://phone/my project",
    ] {
        let mut value = command_envelope("createSession");
        value["workspaceUri"] = json!(invalid);
        value["model"] = Value::Null;

        assert!(matches!(
            AgentCommand::decode_json(&value.to_string()),
            Err(ProtocolError::Decode(_))
        ));
    }
}

#[test]
fn unknown_command_tags_are_protocol_decode_errors() {
    let value = command_envelope("futureCommand");

    assert!(matches!(
        AgentCommand::decode_json(&value.to_string()),
        Err(ProtocolError::Decode(_))
    ));
}

#[test]
fn command_and_event_decoders_reject_missing_or_wrong_versions() {
    let mut command = command_envelope("deleteSession");
    command.as_object_mut().unwrap().remove("version");
    assert!(matches!(
        AgentCommand::decode_json(&command.to_string()),
        Err(ProtocolError::Decode(_))
    ));

    command["version"] = json!(2);
    assert!(matches!(
        AgentCommand::decode_json(&command.to_string()),
        Err(ProtocolError::UnsupportedVersion {
            expected: PROTOCOL_VERSION,
            actual: 2
        })
    ));

    let mut event = event_envelope("diffInvalidated");
    event["version"] = json!(0);
    assert!(matches!(
        AgentEvent::decode_json(&event.to_string()),
        Err(ProtocolError::UnsupportedVersion {
            expected: PROTOCOL_VERSION,
            actual: 0
        })
    ));

    event.as_object_mut().unwrap().remove("version");
    assert!(matches!(
        AgentEvent::decode_json(&event.to_string()),
        Err(ProtocolError::Decode(_))
    ));
}

#[test]
fn public_deserialize_enforces_command_and_event_validation() {
    let mut wrong_command_version = command_envelope("deleteSession");
    wrong_command_version["version"] = json!(2);
    assert!(serde_json::from_value::<AgentCommand>(wrong_command_version).is_err());

    let mut missing_command_turn_id = command_envelope("startTurn");
    missing_command_turn_id["input"] = json!([]);
    assert!(serde_json::from_value::<AgentCommand>(missing_command_turn_id).is_err());

    let mut wrong_event_version = event_envelope("diffInvalidated");
    wrong_event_version["version"] = json!(2);
    assert!(serde_json::from_value::<AgentEvent>(wrong_event_version).is_err());

    let mut missing_event_turn_id = event_envelope("diffInvalidated");
    missing_event_turn_id
        .as_object_mut()
        .unwrap()
        .remove("turnId");
    assert!(serde_json::from_value::<AgentEvent>(missing_event_turn_id).is_err());
}

#[test]
fn turn_commands_require_caller_allocated_turn_ids() {
    let mut start = command_envelope("startTurn");
    start["input"] = json!([]);
    let mut steer = command_envelope("steerTurn");
    steer["input"] = json!([]);
    steer["turnId"] = Value::Null;
    let interrupt = command_envelope("interruptTurn");

    for (value, expected_command) in [
        (start, "startTurn"),
        (steer, "steerTurn"),
        (interrupt, "interruptTurn"),
    ] {
        assert!(matches!(
            AgentCommand::decode_json(&value.to_string()),
            Err(ProtocolError::MissingTurnId { command }) if command == expected_command
        ));
    }
}

#[test]
fn non_turn_commands_may_retain_an_optional_turn_association() {
    let command = command(
        AgentCommandKind::Approve {
            approval_id: "approval-1".into(),
            decision: ApprovalDecision::Deny,
        },
        Some(turn_id()),
    );

    command.validate().unwrap();
    assert_eq!(command.turn_id, Some(turn_id()));
}

#[test]
fn partial_success_is_a_stable_endpoint_error_kind() {
    let value = serde_json::to_value(EndpointError {
        kind: EndpointErrorKind::PartialSuccess,
        message: "fallback applied".into(),
    })
    .unwrap();

    assert_eq!(value["kind"], "partialSuccess");
    assert_eq!(
        serde_json::from_value::<EndpointError>(value).unwrap().kind,
        EndpointErrorKind::PartialSuccess,
    );
}

#[test]
fn request_expired_is_a_stable_endpoint_error_kind() {
    let value = serde_json::to_value(EndpointError {
        kind: EndpointErrorKind::RequestExpired,
        message: "request expired".into(),
    })
    .unwrap();

    assert_eq!(value["kind"], "requestExpired");
    assert_eq!(
        serde_json::from_value::<EndpointError>(value).unwrap().kind,
        EndpointErrorKind::RequestExpired,
    );
}
