use zode_app_model::{
    demo_state, reduce_navigation_command, AppCommand, AttachmentMetadata, IntegrationCatalog,
    LoadState, MessageQueueState, NavigationOutcome, ProjectState, SecondaryPane, SettingsCategory,
    ShellPage, ShellRoute,
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

#[test]
fn begin_task_preserves_draft_attachments_and_background_session_state() {
    let mut state = demo_state();
    let previous = workspace("file:///repo/previous");
    let selected = workspace("file:///repo/selected");
    add_project(&mut state, previous.clone());
    add_project(&mut state, selected.clone());
    state.active_workspace = Some(previous.clone());
    let session = SessionLocator::new(state.host.node_id, "background-session");
    state.current_session = Some(session.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: previous,
        title: "background".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Running,
    });
    let turn = TurnId::new();
    state.active_turns.insert(session.clone(), turn);
    let mut queue = MessageQueueState::default();
    let queued = queue.enqueue("queued".into(), Vec::new()).unwrap();
    state.message_queues.insert(session, queue);
    state.composer.draft = "draft before queue edit".into();
    state.composer.attachments.push(AttachmentMetadata {
        id: "attachment-1".into(),
        path: None,
        display_name: "reference.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    });
    state
        .composer
        .begin_queue_edit(queued, "temporary queued edit");
    state.composer.queue_menu = Some(queued);
    state.project_picker.open = true;
    state.project_picker.search = "selected".into();
    state.project_picker.active_index = 2;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    state.review.open = true;
    state.review.dirty = true;
    state.shell.page = ShellPage::Settings;
    let turns_before = state.active_turns.clone();
    let queues_before = state.message_queues.clone();
    let attachments_before = state.composer.attachments.clone();

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: Some(selected.clone()),
            },
        ),
        NavigationOutcome::Applied,
    );

    assert_eq!(state.current_session, None);
    assert_eq!(state.active_workspace, Some(selected));
    assert_eq!(state.active_turns, turns_before);
    assert_eq!(state.message_queues, queues_before);
    assert_eq!(state.composer.draft, "draft before queue edit");
    assert_eq!(state.composer.attachments, attachments_before);
    assert_eq!(state.composer.editing_queued_message, None);
    assert_eq!(state.composer.queue_menu, None);
    assert!(!state.project_picker.open);
    assert!(state.project_picker.search.is_empty());
    assert_eq!(state.project_picker.active_index, 0);
    assert_eq!(state.presentation.route, ShellRoute::Conversation);
    assert_eq!(state.presentation.secondary_pane, None);
    assert!(!state.review.open);
    assert!(state.review.dirty);
    assert_eq!(state.shell.page, ShellPage::Conversation);
}

#[test]
fn begin_task_rejects_an_unavailable_project_without_mutating_state() {
    let mut state = demo_state();
    let available = workspace("file:///repo/available");
    add_project(&mut state, available.clone());
    state.active_workspace = Some(available);
    let session = SessionLocator::new(state.host.node_id, "current");
    state.current_session = Some(session);
    state.project_picker.open = true;
    let before = state.clone();

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: Some(workspace("file:///repo/missing")),
            },
        ),
        NavigationOutcome::Ignored,
    );
    assert_eq!(state, before);
}

#[test]
fn project_picker_commands_update_only_picker_state() {
    let mut state = demo_state();

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleProjectPicker),
        NavigationOutcome::Applied,
    );
    assert!(state.project_picker.open);
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetProjectSearch("zode".into())),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SetProjectPickerActive(3)),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.project_picker.search, "zode");
    assert_eq!(state.project_picker.active_index, 3);

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::CloseProjectPicker),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.project_picker, Default::default());

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::ToggleProjectPicker),
        NavigationOutcome::Applied,
    );
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::CreateProject),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(state.project_picker, Default::default());
}

#[test]
fn projectless_workspace_recognizes_root_and_path_bounded_descendants() {
    let mut state = demo_state();
    let root = workspace("file:///Users/fini/.zode/tasks");
    let child = workspace("file:///Users/fini/.zode/tasks/session-1");
    let sibling = workspace("file:///Users/fini/.zode/other");
    let prefix_collision = workspace("file:///Users/fini/.zode/tasks-other/session-2");
    state.projectless_workspace_root = Some(root.clone());

    assert!(state.is_projectless_workspace(&root));
    assert!(state.is_projectless_workspace(&child));
    assert!(!state.is_projectless_workspace(&sibling));
    assert!(!state.is_projectless_workspace(&prefix_collision));
    assert!(
        !state.is_projectless_workspace(&workspace("file:///Users/fini/.zode/tasks/../outside"))
    );
    assert!(!state
        .is_projectless_workspace(&workspace("file:///Users/fini/.zode/tasks/%2E%2E/outside")));

    state.projectless_workspace_root = Some(workspace("file:///Users/fini/.zode/tasks/"));
    assert!(state.is_projectless_workspace(&root));
    assert!(state.is_projectless_workspace(&child));

    let session = SessionLocator::new(state.host.node_id, "projectless-session");
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: child.clone(),
        title: "task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    assert!(!state.available_workspace(&child));
    assert_eq!(
        state.available_workspace_for_session(&session),
        Some(&child)
    );
}

#[test]
fn selecting_a_projectless_session_clears_a_stale_active_project() {
    let mut state = demo_state();
    let project = workspace("file:///repo/zode");
    let task_root = workspace("file:///Users/fini/.zode/tasks");
    let task_workspace = workspace("file:///Users/fini/.zode/tasks/task-1");
    add_project(&mut state, project.clone());
    state.active_workspace = Some(project);
    state.projectless_workspace_root = Some(task_root);
    let session = SessionLocator::new(state.host.node_id, "task-session");
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: task_workspace,
        title: "projectless task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });

    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(session.clone())),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.current_session, Some(session));
    assert_eq!(state.active_workspace, None);
}

#[test]
fn begin_projectless_task_clears_the_active_project() {
    let mut state = demo_state();
    let project = workspace("file:///repo/zode");
    add_project(&mut state, project.clone());
    state.active_workspace = Some(project);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: None,
            },
        ),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.active_workspace, None);
}

#[test]
fn selecting_a_project_refreshes_recency_and_clears_a_stale_catalog() {
    let mut state = demo_state();
    let previous = workspace("file:///repo/previous");
    let selected = workspace("file:///repo/selected");
    add_project(&mut state, previous.clone());
    add_project(&mut state, selected.clone());
    state.projects[0].last_opened_ms = 20;
    state.projects[1].last_opened_ms = 1;
    state.active_workspace = Some(previous.clone());
    state.presentation.integrations = LoadState::Ready(IntegrationCatalog {
        workspace_uri: previous,
        installed: Vec::new(),
        sections: Vec::new(),
        directory_error: None,
    });

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::BeginTask {
                workspace_uri: Some(selected.clone()),
            },
        ),
        NavigationOutcome::Applied
    );

    assert_eq!(state.active_workspace, Some(selected.clone()));
    assert_eq!(state.projects[1].workspace_uri, selected);
    assert_eq!(state.projects[1].last_opened_ms, 21);
    assert_eq!(state.presentation.integrations, LoadState::Idle);
}
