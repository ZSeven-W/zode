use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, NavigationOutcome, ProjectState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn state_with_thread() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "task-menu");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Old title".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.current_session = Some(session.clone());
    (state, session)
}

#[test]
fn rename_dialog_owns_a_draft_and_projects_a_non_empty_title() {
    let (mut state, session) = state_with_thread();
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginRenameSession {
                session: session.clone()
            }
        ),
        NavigationOutcome::Applied
    );
    assert_eq!(state.session_rename.as_ref().unwrap().draft, "Old title");

    let _ = reduce_navigation_command(
        &mut state,
        AppCommand::SetSessionRenameDraft {
            session: session.clone(),
            draft: "New title".into(),
        },
    );
    assert_eq!(state.session_rename.as_ref().unwrap().draft, "New title");
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::RenameSession {
                session: session.clone(),
                title: " New title ".into(),
            }
        ),
        NavigationOutcome::NeedsEffect
    );
    assert_eq!(
        state
            .threads
            .iter()
            .find(|thread| thread.session == session)
            .unwrap()
            .title,
        "New title"
    );
    assert!(state.session_rename.is_none());
}

#[test]
fn copy_submenu_is_nested_and_side_pane_stays_disabled_by_contract() {
    let (mut state, session) = state_with_thread();
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleSessionCopyMenu {
                session: session.clone()
            }
        ),
        NavigationOutcome::Ignored
    );
    let _ = reduce_navigation_command(
        &mut state,
        AppCommand::ToggleSessionMenu {
            session: session.clone(),
        },
    );
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleSessionCopyMenu {
                session: session.clone()
            }
        ),
        NavigationOutcome::Applied
    );
    assert_eq!(state.session_copy_menu, Some(session.clone()));
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::OpenSessionInSidePane { session }),
        NavigationOutcome::Ignored
    );
}

#[test]
fn opening_a_real_task_window_is_reported_as_an_effect() {
    let (mut state, session) = state_with_thread();
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::OpenSessionInNewWindow { session }),
        NavigationOutcome::NeedsEffect
    );
}
