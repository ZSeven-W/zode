use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, ComposerContextMenu, ComposerFooterMenu,
    GlobalSearchState, NavigationOutcome, ProjectState, SidebarSectionMenu,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn workspace(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).unwrap()
}

fn open_global_search(state: &mut zode_app_model::ZodeAppState) {
    assert_eq!(
        reduce_navigation_command(state, AppCommand::ToggleGlobalSearch),
        NavigationOutcome::Applied,
    );
    assert!(state.global_search.open);
}

#[test]
fn global_search_commands_manage_query_selection_and_lifecycle() {
    let mut state = demo_state();

    open_global_search(&mut state);
    assert_eq!(
        state.global_search,
        GlobalSearchState {
            open: true,
            query: String::new(),
            active_index: 0,
        }
    );

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetGlobalSearchActive(4)),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetGlobalSearchQuery("zode".into()),),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.global_search.query, "zode");
    assert_eq!(state.global_search.active_index, 0);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetGlobalSearchActive(2)),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.global_search.active_index, 2);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::CloseGlobalSearch),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.global_search, GlobalSearchState::default());

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleGlobalSearch),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.global_search, GlobalSearchState::default());

    open_global_search(&mut state);
    state.global_search.query = "new project".into();
    state.global_search.active_index = 3;
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::CreateProject),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(state.global_search, GlobalSearchState::default());
}

#[test]
fn opening_global_search_closes_other_transient_navigation_surfaces() {
    let mut state = demo_state();
    let project = workspace("file:///repo/zode");
    let session = SessionLocator::new(state.host.node_id, "session");
    state.project_picker.open = true;
    state.composer.context_menu = Some(ComposerContextMenu::Location);
    state.composer.footer_menu = Some(ComposerFooterMenu::Add);
    state.sidebar.project_menu = Some(project);
    state.sidebar.section_menu = Some(SidebarSectionMenu::Projects);
    state.session_menu = Some(session.clone());
    state.session_copy_menu = Some(session);
    state.open_with.menu_open = true;

    open_global_search(&mut state);

    assert_eq!(state.project_picker, Default::default());
    assert_eq!(state.composer.context_menu, None);
    assert_eq!(state.composer.footer_menu, None);
    assert_eq!(state.sidebar.project_menu, None);
    assert_eq!(state.sidebar.section_menu, None);
    assert_eq!(state.session_menu, None);
    assert_eq!(state.session_copy_menu, None);
    assert!(!state.open_with.menu_open);
}

#[test]
fn other_navigation_surfaces_close_global_search_when_opened() {
    let mut state = demo_state();
    let project = workspace("file:///repo/zode");
    let session = SessionLocator::new(state.host.node_id, "session");
    state.projects.push(ProjectState {
        workspace_uri: project.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: project.clone(),
        title: "task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleProjectPicker),
        NavigationOutcome::Applied,
    );
    assert!(!state.global_search.open);

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Location),
        ),
        NavigationOutcome::Applied,
    );
    assert!(!state.global_search.open);

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleComposerFooterMenu(ComposerFooterMenu::Add),
        ),
        NavigationOutcome::Applied,
    );
    assert!(!state.global_search.open);

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleProjectMenu {
                workspace_uri: project,
            },
        ),
        NavigationOutcome::Applied,
    );
    assert!(!state.global_search.open);

    open_global_search(&mut state);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleSessionMenu { session },),
        NavigationOutcome::Applied,
    );
    assert!(!state.global_search.open);
}
