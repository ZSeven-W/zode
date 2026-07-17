use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, NavigationOutcome, ProjectState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn state_with_thread() -> (zode_app_model::ZodeAppState, SessionLocator, WorkspaceUri) {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "session");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "session".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    (state, session, workspace)
}

#[test]
fn pin_and_archive_commands_update_memory_before_requesting_persistence() {
    let (mut state, session, _) = state_with_thread();
    state.current_session = Some(session.clone());

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SetSessionPinned {
                session: session.clone(),
                pinned: true,
            },
        ),
        NavigationOutcome::NeedsEffect
    );
    assert!(state.pinned_sessions.contains(&session));

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SetSessionArchived {
                session: session.clone(),
                archived: true,
            },
        ),
        NavigationOutcome::NeedsEffect
    );
    assert!(state.archived_sessions.contains(&session));
    assert_eq!(state.current_session, None);

    let _ = reduce_navigation_command(
        &mut state,
        AppCommand::SetSessionPinned {
            session: session.clone(),
            pinned: false,
        },
    );
    let _ = reduce_navigation_command(
        &mut state,
        AppCommand::SetSessionArchived {
            session: session.clone(),
            archived: false,
        },
    );
    assert!(!state.pinned_sessions.contains(&session));
    assert!(!state.archived_sessions.contains(&session));
}

#[test]
fn task_menu_is_session_scoped_and_closes_after_a_real_action() {
    let (mut state, session, _) = state_with_thread();
    state.current_session = Some(session.clone());

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleSessionMenu {
                session: session.clone(),
            },
        ),
        NavigationOutcome::Applied
    );
    assert_eq!(state.session_menu, Some(session.clone()));

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SetSessionPinned {
                session: session.clone(),
                pinned: true,
            },
        ),
        NavigationOutcome::NeedsEffect
    );
    assert!(state.pinned_sessions.contains(&session));
    assert_eq!(state.session_menu, None);

    let stale = SessionLocator::new(state.host.node_id, "stale");
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleSessionMenu { session: stale },),
        NavigationOutcome::Ignored
    );
    assert_eq!(state.session_menu, None);
}

#[test]
fn sidebar_commands_update_transient_navigation_state() {
    let (mut state, _, workspace) = state_with_thread();
    assert!(state.sidebar.tasks_expanded);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetSidebarScroll { offset: 42.0 }),
        NavigationOutcome::Applied
    );
    assert_eq!(state.sidebar.scroll_offset, 42.0);

    let _ = reduce_navigation_command(&mut state, AppCommand::ToggleSidebarTasks);
    let _ = reduce_navigation_command(&mut state, AppCommand::ShowAllProjects);
    let _ = reduce_navigation_command(
        &mut state,
        AppCommand::ShowAllProjectSessions {
            workspace_uri: workspace.clone(),
        },
    );
    assert!(!state.sidebar.tasks_expanded);
    assert!(state.sidebar.show_all_projects);
    assert!(state.sidebar.show_all_project_sessions.contains(&workspace));
}

#[test]
fn deleting_a_session_clears_pin_and_archive_metadata() {
    let (mut state, session, _) = state_with_thread();
    state.pinned_sessions.insert(session.clone());
    state.archived_sessions.insert(session.clone());
    state.pending_session_delete = Some(session.clone());

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::DeleteSession(session.clone())),
        NavigationOutcome::NeedsEffect
    );
    assert!(!state.pinned_sessions.contains(&session));
    assert!(!state.archived_sessions.contains(&session));
}
