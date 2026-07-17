mod support;

use std::time::Duration;

use support::FixtureApp;
use zode_node_protocol::{AgentEventKind, EndpointErrorKind, HistoryItem, ToolStatus};

#[tokio::test]
async fn desktop_first_release_flow_survives_restart() {
    let app = FixtureApp::start("completed").await;
    let session = app.new_session().await;
    let turn = app.send(&session, "edit a.txt").await;
    let approval = app.wait_for_approval("FileEdit").await;

    app.approve_always(&session, &approval).await;
    let events = app.wait_finished(turn).await;

    assert_eq!(events.len(), 7);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::TextDelta { delta } if delta == "edited"
    ));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::ToolStarted { tool }
            if tool.name == "FileEdit" && tool.status == ToolStatus::Running
    ));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::ApprovalRequested { tool, .. } if tool == "FileEdit"
    ));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::ToolCompleted { tool }
            if tool.name == "FileEdit" && tool.status == ToolStatus::Completed
    ));
    assert!(matches!(&events[4].kind, AgentEventKind::Usage { .. }));
    assert!(matches!(&events[5].kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        events.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: false })
    ));
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));

    let diff = app.diff(&session).await;
    assert!(diff.unified.contains("a.txt"));
    assert!(app.project_permissions().await.contains(&"FileEdit".into()));
    assert!(app.production_session_is_persisted(&session));
    let restarted = app.restart().await;
    let history = restarted.resume(&session).await;
    assert!(history
        .items
        .iter()
        .any(|item| matches!(item, HistoryItem::UserText { text } if text == "edit a.txt")));
    assert!(history
        .items
        .iter()
        .any(|item| matches!(item, HistoryItem::AssistantText { text } if text == "edited")));
    assert!(history.items.iter().any(|item| matches!(
        item,
        HistoryItem::Tool { tool }
            if tool.name == "FileEdit" && tool.status == ToolStatus::Completed
    )));
    assert!(restarted
        .project_permissions()
        .await
        .contains(&"FileEdit".into()));
    assert!(restarted.expect_no_event(Duration::from_millis(100)).await);

    let error = restarted.interrupt(&session, turn).await.unwrap_err();
    assert_eq!(error.kind, EndpointErrorKind::NotFound);
    assert!(restarted.expect_no_event(Duration::from_millis(100)).await);
}

#[tokio::test]
async fn interrupted_pending_turn_is_not_replayed_after_restart() {
    let app = FixtureApp::start("interrupted").await;
    let session = app.new_session().await;
    let turn = app.send(&session, "edit a.txt").await;
    let _approval = app.wait_for_approval("FileEdit").await;

    app.interrupt(&session, turn).await.unwrap();
    let events = app.wait_finished(turn).await;
    assert_eq!(events.len(), 5);
    assert!(matches!(&events[0].kind, AgentEventKind::TextDelta { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::ToolStarted { .. }
    ));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::ApprovalRequested { .. }
    ));
    assert!(matches!(&events[3].kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        events.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: true })
    ));
    assert!(!app.diff(&session).await.unified.contains("a.txt"));

    let restarted = app.restart().await;
    let history = restarted.resume(&session).await;
    assert_eq!(
        history
            .items
            .iter()
            .filter(|item| matches!(item, HistoryItem::UserText { text } if text == "edit a.txt"))
            .count(),
        1
    );
    assert!(!history
        .items
        .iter()
        .any(|item| matches!(item, HistoryItem::AssistantText { text } if text == "edited")));
    assert!(restarted.expect_no_event(Duration::from_millis(100)).await);
}
