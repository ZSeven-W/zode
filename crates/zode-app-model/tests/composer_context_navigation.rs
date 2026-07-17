use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, BranchCatalog, BranchCatalogState,
    ComposerContextMenu, NavigationOutcome, ProjectPickerAnchor, ProjectState, TaskLaunchMode,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, TurnId, WorkspaceUri};

fn workspace(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).unwrap()
}

fn add_project(state: &mut zode_app_model::ZodeAppState, workspace_uri: WorkspaceUri) {
    state.projects.push(ProjectState {
        workspace_uri,
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
}

fn catalog(workspace_uri: WorkspaceUri) -> BranchCatalog {
    BranchCatalog {
        workspace_uri,
        current: "main".into(),
        branches: vec!["main".into(), "codex/composer-context".into()],
        dirty_files: 2,
    }
}

#[test]
fn project_picker_anchors_and_context_menus_are_mutually_exclusive() {
    let mut state = demo_state();

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleProjectPicker),
        NavigationOutcome::Applied,
    );
    assert!(state.project_picker.open);
    assert_eq!(state.project_picker.anchor, ProjectPickerAnchor::Welcome);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleComposerProjectPicker),
        NavigationOutcome::Applied,
    );
    assert!(state.project_picker.open);
    assert_eq!(state.project_picker.anchor, ProjectPickerAnchor::Composer);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Location),
        ),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.project_picker, Default::default());
    assert_eq!(
        state.composer.context_menu,
        Some(ComposerContextMenu::Location)
    );

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Branch),
        ),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        state.composer.context_menu,
        Some(ComposerContextMenu::Branch)
    );

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleComposerProjectPicker),
        NavigationOutcome::Applied,
    );
    assert!(state.project_picker.open);
    assert_eq!(state.project_picker.anchor, ProjectPickerAnchor::Composer);
    assert_eq!(state.composer.context_menu, None);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleComposerProjectPicker),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.project_picker, Default::default());
}

#[test]
fn unavailable_worktree_launch_mode_is_ignored() {
    let mut state = demo_state();
    state.composer.context_menu = Some(ComposerContextMenu::Location);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SelectTaskLaunchMode(TaskLaunchMode::Worktree),
        ),
        NavigationOutcome::Ignored,
    );
    assert_eq!(state.composer.launch_mode, TaskLaunchMode::Local);
    assert_eq!(
        state.composer.context_menu,
        Some(ComposerContextMenu::Location)
    );
}

#[test]
fn branch_catalog_responses_must_match_the_active_loading_workspace() {
    let mut state = demo_state();
    let active = workspace("file:///repo/zode");
    let stale = workspace("file:///repo/other");
    add_project(&mut state, active.clone());
    add_project(&mut state, stale.clone());
    state.active_workspace = Some(active.clone());

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::LoadBranches {
                workspace_uri: stale.clone(),
            },
        ),
        NavigationOutcome::Ignored,
    );
    assert_eq!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Idle
    );

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::LoadBranches {
                workspace_uri: active.clone(),
            },
        ),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Loading {
            workspace_uri: active.clone()
        }
    );

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BranchesFailed {
                workspace_uri: stale.clone(),
                message: "stale".into(),
            },
        ),
        NavigationOutcome::Ignored,
    );
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::BranchesLoaded(catalog(stale)),),
        NavigationOutcome::Ignored,
    );
    assert!(matches!(
        &state.composer.branch_picker.catalog,
        BranchCatalogState::Loading { .. }
    ));

    let mut loaded = catalog(active.clone());
    loaded.dirty_files = 0;
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::BranchesLoaded(loaded.clone()),),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Ready(loaded)
    );
}

#[test]
fn branch_selection_waits_for_a_confirmed_checkout() {
    let mut state = demo_state();
    let active = workspace("file:///repo/zode");
    add_project(&mut state, active.clone());
    state.active_workspace = Some(active.clone());
    let mut loaded = catalog(active.clone());
    loaded.dirty_files = 0;
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(loaded);
    state.composer.context_menu = Some(ComposerContextMenu::Branch);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SelectBranch {
                workspace_uri: active.clone(),
                branch: "codex/composer-context".into(),
            },
        ),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(state.composer.selected_branch, None);
    assert_eq!(
        state.composer.context_menu,
        Some(ComposerContextMenu::Branch)
    );
    assert_eq!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Switching {
            workspace_uri: active.clone(),
            from: "main".into(),
            branch: "codex/composer-context".into(),
        }
    );

    let switched = BranchCatalog {
        workspace_uri: active,
        current: "codex/composer-context".into(),
        branches: vec!["main".into(), "codex/composer-context".into()],
        dirty_files: 0,
    };
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::BranchesLoaded(switched.clone())),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        state.composer.selected_branch.as_deref(),
        Some("codex/composer-context")
    );
    assert_eq!(state.composer.context_menu, None);
    assert_eq!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Ready(switched)
    );
}

#[test]
fn branch_selection_is_blocked_while_the_workspace_has_an_active_turn() {
    let mut state = demo_state();
    let active = workspace("file:///repo/zode");
    add_project(&mut state, active.clone());
    state.active_workspace = Some(active.clone());
    let session = SessionLocator::new(state.host.node_id, "running");
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: active.clone(),
        title: "running".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Running,
    });
    state.active_turns.insert(session, TurnId::new());
    let mut loaded = catalog(active.clone());
    loaded.dirty_files = 0;
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(loaded);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::SelectBranch {
                workspace_uri: active,
                branch: "codex/composer-context".into(),
            },
        ),
        NavigationOutcome::Ignored,
    );
    assert!(matches!(
        state.composer.branch_picker.catalog,
        BranchCatalogState::Ready(_)
    ));
}

#[test]
fn beginning_a_task_resets_workspace_scoped_composer_context() {
    let mut state = demo_state();
    let selected = workspace("file:///repo/selected");
    add_project(&mut state, selected.clone());
    state.composer.context_menu = Some(ComposerContextMenu::Branch);
    state.composer.launch_mode = TaskLaunchMode::Worktree;
    state.composer.branch_picker.query = "codex".into();
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(catalog(selected.clone()));
    state.composer.selected_branch = Some("codex/composer-context".into());

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: Some(selected),
            },
        ),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.composer.context_menu, None);
    assert_eq!(state.composer.launch_mode, TaskLaunchMode::Local);
    assert_eq!(state.composer.branch_picker, Default::default());
    assert_eq!(state.composer.selected_branch, None);
}
