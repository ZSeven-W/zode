use zode_node_protocol::{
    AgentQuery, AgentSnapshot, HistoryItem, NodeId, SessionLocator, ThreadHistory, ToolCall,
    ToolStatus,
};

#[test]
fn history_snapshot_carries_renderable_transcript_content() {
    let session = SessionLocator::new(NodeId::new(), "history-session");
    let history = ThreadHistory {
        session: session.clone(),
        items: vec![
            HistoryItem::UserText {
                text: "restore the shell".into(),
            },
            HistoryItem::AssistantText {
                text: "restored".into(),
            },
            HistoryItem::Tool {
                tool: ToolCall {
                    id: "tool-1".into(),
                    name: "read_file".into(),
                    status: ToolStatus::Completed,
                    summary: "read_file".into(),
                    detail: Some("contents".into()),
                },
            },
            HistoryItem::Status {
                code: "history.progress".into(),
                message: "saved".into(),
            },
        ],
    };

    let query = AgentQuery::History {
        session: session.clone(),
    };
    assert_eq!(query, AgentQuery::History { session });
    assert_eq!(
        AgentSnapshot::History(history.clone()),
        AgentSnapshot::History(history)
    );
}
