use zode_app_ui::group_sessions;
use zode_node_protocol::{NodeId, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[test]
fn sessions_group_by_workspace_newest_first() {
    let groups = group_sessions(fixture_sessions());

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].workspace_uri.as_str(), "file:///repo/zode");
    assert!(groups[0].sessions[0].updated_at_ms >= groups[0].sessions[1].updated_at_ms);
    assert_eq!(groups[1].workspace_uri.as_str(), "file:///repo/openpencil");
}

#[test]
fn empty_session_list_has_no_placeholder_group() {
    assert!(group_sessions(Vec::new()).is_empty());
}

fn fixture_sessions() -> Vec<ThreadSummary> {
    let node_id = NodeId::parse("00000000-0000-0000-0000-000000000001").unwrap();
    [
        ("old-zode", "file:///repo/zode", 100),
        ("openpencil", "file:///repo/openpencil", 200),
        ("new-zode", "file:///repo/zode", 300),
    ]
    .into_iter()
    .map(|(id, workspace, updated_at_ms)| ThreadSummary {
        session: SessionLocator::new(node_id, id),
        workspace_uri: WorkspaceUri::new(workspace).unwrap(),
        title: id.into(),
        updated_at_ms,
        status: ThreadStatus::Idle,
    })
    .collect()
}
