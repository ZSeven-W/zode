use zode_app_model::{demo_state, reduce_agent_event, TranscriptItem, TranscriptState};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, TurnId, WorkspaceUri,
    PROTOCOL_VERSION,
};

#[test]
fn adjacent_text_deltas_extend_one_assistant_item() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "s1");
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap();
    state.threads.push(zode_node_protocol::ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        title: "为 zode 写 app".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Running,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state.active_turns.insert(session.clone(), turn_id);
    reduce_agent_event(
        &mut state,
        AgentEvent {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id,
            sequence: 1,
            kind: AgentEventKind::TextDelta {
                delta: "你".into()
            },
        },
    );
    reduce_agent_event(
        &mut state,
        AgentEvent {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id,
            sequence: 2,
            kind: AgentEventKind::TextDelta {
                delta: "好".into()
            },
        },
    );
    assert_eq!(
        state.transcripts[&session].items,
        vec![TranscriptItem::AssistantText("你好".into())],
    );
}
