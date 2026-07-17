use zode_app_model::{demo_state, ProjectState};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn thread(
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
fn persisted_owner_is_independent_from_execution_workspace() {
    let mut state = demo_state();
    let project = WorkspaceUri::new("file:///repo/project").unwrap();
    let execution = WorkspaceUri::new("file:///repo/.worktrees/task").unwrap();
    let owned = thread(&state, "owned", execution.clone());
    state.threads.push(owned.clone());
    state.projects.push(ProjectState {
        workspace_uri: project.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.hydrate_thread_affiliations(
        &std::collections::BTreeMap::from([("owned".to_string(), project.clone())]),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(state.project_workspace_for_thread(&owned), Some(&project));
    assert_eq!(
        state.available_workspace_for_session(&owned.session),
        Some(&execution)
    );
}

#[test]
fn explicit_projectless_wins_without_a_special_execution_path() {
    let mut state = demo_state();
    let execution = WorkspaceUri::new("file:///tmp/arbitrary-task").unwrap();
    let task = thread(&state, "task", execution);
    state.threads.push(task.clone());
    state.hydrate_thread_affiliations(
        &std::collections::BTreeMap::new(),
        &std::collections::BTreeSet::from(["task".to_string()]),
    );

    assert!(state.is_projectless_thread(&task));
    assert_eq!(state.project_workspace_for_thread(&task), None);
}
