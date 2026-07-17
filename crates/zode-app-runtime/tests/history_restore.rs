use agent::message::{ContentBlock, Header, Message, MessageStore, ToolResultContent};
use tempfile::TempDir;
use zode_app_runtime::{path_to_workspace_uri, LocalSessionRepository};
use zode_app_runtime::session_store::{SessionSaveOutcome, SessionWriteMode};
use zode_node_protocol::{HistoryItem, NodeId, SessionLocator, ThreadHistory, ToolStatus};

fn persisted_store() -> MessageStore {
    let mut store = MessageStore::new();
    store
        .push(Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: "persisted user".into(),
            }],
        })
        .unwrap();
    store
        .push(Message::Assistant {
            header: Header::new(),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "persisted thought".into(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "persisted assistant".into(),
                },
                ContentBlock::ToolUse {
                    id: "tool-1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({ "path": "README.md" }),
                },
            ],
        })
        .unwrap();
    store
        .push(Message::User {
            header: Header::new(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".into(),
                content: ToolResultContent::Text("file contents".into()),
                is_error: false,
            }],
        })
        .unwrap();
    store
        .push(Message::Progress {
            header: Header::new(),
            note: "persisted status".into(),
        })
        .unwrap();
    store
}

#[tokio::test]
async fn history_projection_survives_repository_restart_and_reload() {
    let config = TempDir::new().unwrap();
    let project = config.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let node_id = NodeId::new();
    let session = SessionLocator::new(node_id, "restart-history");
    let first = LocalSessionRepository::new(config.path(), node_id);
    let loaded = first
        .create(
            &session,
            &path_to_workspace_uri(&project).unwrap(),
            "test-model".into(),
        )
        .await
        .unwrap();
    let outcome = first
        .save(
            &session,
            loaded.meta,
            persisted_store(),
            SessionWriteMode::Full,
        )
        .await
        .unwrap();
    assert!(matches!(outcome, SessionSaveOutcome::Saved { .. }));
    drop(first);

    let restarted = LocalSessionRepository::new(config.path(), node_id);
    let ThreadHistory {
        session: restored_session,
        items,
    } = restarted.history(&session).await.unwrap();

    assert_eq!(restored_session, session);
    assert!(matches!(
        &items[0],
        HistoryItem::UserText { text } if text == "persisted user"
    ));
    assert!(matches!(
        &items[1],
        HistoryItem::Thinking { text } if text == "persisted thought"
    ));
    assert!(matches!(
        &items[2],
        HistoryItem::AssistantText { text } if text == "persisted assistant"
    ));
    assert!(matches!(
        &items[3],
        HistoryItem::Tool { tool }
            if tool.id == "tool-1"
                && tool.status == ToolStatus::Completed
                && tool.detail.is_none()
    ));
    assert!(matches!(
        &items[4],
        HistoryItem::Status { code, message }
            if code == "history.progress" && message == "persisted status"
    ));
}
