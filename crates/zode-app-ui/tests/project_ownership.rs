use jian_widgets::Rect;
use zode_app_model::{demo_state, ProjectState};
use zode_app_ui::{ProjectSidebar, SidebarRowTarget};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn summary(
    state: &zode_app_model::ZodeAppState,
    id: &str,
    execution: WorkspaceUri,
) -> ThreadSummary {
    ThreadSummary {
        session: SessionLocator::new(state.host.node_id, id),
        workspace_uri: execution,
        title: id.into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    }
}

#[test]
fn sidebar_groups_by_owner_and_keeps_explicit_projectless_tasks_separate() {
    let mut state = demo_state();
    let project = WorkspaceUri::new("file:///repo/zode").unwrap();
    let owned = summary(
        &state,
        "owned",
        WorkspaceUri::new("file:///repo/zode/.worktrees/owned").unwrap(),
    );
    let projectless = summary(
        &state,
        "projectless",
        WorkspaceUri::new("file:///tmp/arbitrary-task").unwrap(),
    );
    state.projects.push(ProjectState {
        workspace_uri: project.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads = vec![owned.clone(), projectless.clone()];
    state.hydrate_thread_affiliations(
        &std::collections::BTreeMap::from([("owned".to_string(), project.clone())]),
        &std::collections::BTreeSet::from(["projectless".to_string()]),
    );

    let rows = ProjectSidebar::dynamic_row_layout(Rect::xywh(0.0, 0.0, 260.0, 900.0), &state);
    assert!(rows
        .iter()
        .any(|row| row.target == SidebarRowTarget::Project(project.clone())));
    assert!(rows
        .iter()
        .any(|row| row.target == SidebarRowTarget::Session(owned.session.clone())));
    assert!(rows
        .iter()
        .any(|row| row.target == SidebarRowTarget::Task(projectless.session.clone())));
}
