use zode_app_model::{
    demo_state, reduce_tool_command, AppCommand, ToolCommandOutcome, TranscriptItem,
    TranscriptState,
};
use zode_app_ui::{ApprovalAction, ApprovalCard, ToolCard, ToolTone, UsageChip, UsageDisplay};
use zode_node_protocol::{ApprovalDecision, SessionLocator, ToolCall, ToolStatus, UsageSnapshot};

fn tool(id: &str, name: &str, status: ToolStatus) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        status,
        summary: "summary".into(),
        detail: None,
    }
}

#[test]
fn approval_actions_map_to_the_three_protocol_decisions() {
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::AllowOnce),
        ApprovalDecision::AllowOnce,
    );
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::AllowAlways),
        ApprovalDecision::AllowAlways,
    );
    assert_eq!(
        ApprovalCard::decision(ApprovalAction::Deny),
        ApprovalDecision::Deny,
    );
}

#[test]
fn tool_cards_default_closed_for_read_create_and_open_for_mutation() {
    assert!(!ToolCard::default_expanded(&tool(
        "1",
        "read_file",
        ToolStatus::Running,
    )));
    assert!(!ToolCard::default_expanded(&tool(
        "2",
        "create_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "3",
        "edit_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "4",
        "delete_file",
        ToolStatus::Running,
    )));
    assert!(ToolCard::default_expanded(&tool(
        "5",
        "task_agent",
        ToolStatus::Running,
    )));
}

#[test]
fn tool_status_maps_to_running_success_and_failure_tones() {
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Running)),
        ToolTone::Running,
    );
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Completed)),
        ToolTone::Success,
    );
    assert_eq!(
        ToolCard::tone(&tool("1", "read_file", ToolStatus::Failed)),
        ToolTone::Failure,
    );
}

#[test]
fn expanded_state_is_keyed_by_tool_id() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "tools");
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![TranscriptItem::Tool(tool(
                "tool-1",
                "read_file",
                ToolStatus::Running,
            ))],
            ..TranscriptState::default()
        },
    );

    assert_eq!(
        reduce_tool_command(
            &mut state,
            AppCommand::SetToolExpanded {
                session: session.clone(),
                tool_id: "tool-1".into(),
                expanded: true,
            },
        ),
        ToolCommandOutcome::Applied,
    );
    assert_eq!(state.tool_expanded[&session].get("tool-1"), Some(&true));
}

#[test]
fn usage_display_includes_model_context_tokens_and_optional_cost() {
    let usage = UsageSnapshot {
        input_tokens: 1_200,
        output_tokens: 300,
        context_used: Some(0.423),
        cost_usd: Some(0.1234),
    };
    assert_eq!(
        UsageChip::display(Some("gpt-5.2"), &usage),
        UsageDisplay {
            model: "gpt-5.2".into(),
            context: "42%".into(),
            tokens: "1,500".into(),
            cost: "$0.1234".into(),
        },
    );

    let no_price = UsageSnapshot {
        cost_usd: None,
        ..usage
    };
    assert_eq!(UsageChip::display(None, &no_price).cost, "n/a");
}
