use accesskit::{Action, Role, Toggled};
use jian_widgets::Point2D;
use std::collections::BTreeSet;
use zode_app_model::{demo_state, AppCommand, ProjectState, SettingsCategory};
use zode_app_ui::{
    Composer, Insets, ProjectPicker, ProjectPickerViewState, ProjectSidebar, RectExt,
    SettingsPanel, WorkspaceSnapshot, PROJECT_DETACH_ID, PROJECT_PICKER_NEW_ID,
    PROJECT_PICKER_PROJECTLESS_ID, PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_SURFACE_ID,
    PROJECT_PICKER_TRIGGER_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn workspace(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).unwrap()
}

fn state_with_projects() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let zode = workspace("file:///repo/zode");
    state.projects = vec![
        ProjectState {
            workspace_uri: zode.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 20,
        },
        ProjectState {
            workspace_uri: workspace("file:///repo/openpencil"),
            expanded: true,
            available: true,
            last_opened_ms: 10,
        },
    ];
    state.active_workspace = Some(zode);
    state
}

#[test]
fn welcome_project_detach_and_picker_share_real_interaction_nodes() {
    let state = state_with_projects();
    let closed = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let trigger = closed
        .node(PROJECT_PICKER_TRIGGER_ID)
        .expect("welcome project trigger");
    assert_eq!(trigger.role, Role::Button);
    assert_eq!(trigger.name, "切换项目");
    assert_eq!(trigger.value.as_deref(), Some("zode"));
    assert!(trigger.actions.contains(&Action::Click));

    let detach = closed
        .node(PROJECT_DETACH_ID)
        .expect("empty-task composer detach");
    assert_eq!(detach.name, "不在项目中工作");
    assert_eq!(
        Composer::command_for_widget(&state, PROJECT_DETACH_ID),
        Some(AppCommand::BeginTask {
            workspace_uri: None,
        })
    );

    let ids = [
        PROJECT_PICKER_TRIGGER_ID,
        PROJECT_PICKER_SURFACE_ID,
        PROJECT_PICKER_SEARCH_ID,
        PROJECT_PICKER_NEW_ID,
        PROJECT_PICKER_PROJECTLESS_ID,
        PROJECT_DETACH_ID,
    ];
    assert_eq!(ids.into_iter().collect::<BTreeSet<_>>().len(), ids.len());
    for category in [
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Permissions,
        SettingsCategory::KeyboardShortcuts,
        SettingsCategory::Environment,
    ] {
        assert!(!ids.contains(&SettingsPanel::category_widget_id(category)));
    }
}

#[test]
fn open_picker_overlay_is_last_in_hit_order_and_exposes_search_and_actions() {
    let state = state_with_projects();
    let snapshot = WorkspaceSnapshot::build_with_project_picker(
        &state,
        1_800.0,
        1_080.0,
        Insets::ZERO,
        &ProjectPickerViewState {
            open: true,
            query: String::new(),
        },
    );
    assert_eq!(snapshot.focused, Some(PROJECT_PICKER_SEARCH_ID));
    assert_eq!(
        snapshot.node(PROJECT_PICKER_SURFACE_ID).unwrap().role,
        Role::Menu
    );
    assert_eq!(
        snapshot.node(PROJECT_PICKER_SEARCH_ID).unwrap().role,
        Role::SearchInput
    );
    assert_eq!(
        snapshot.node(PROJECT_PICKER_NEW_ID).unwrap().name,
        "新建项目"
    );
    assert_eq!(
        snapshot.node(PROJECT_PICKER_PROJECTLESS_ID).unwrap().name,
        "不在项目中工作"
    );

    let selected_id = ProjectPicker::project_widget_id(&workspace("file:///repo/zode"));
    let selected = snapshot.node(selected_id).expect("selected project row");
    assert_eq!(selected.toggled, Some(Toggled::True));
    let center = Point2D::new(
        selected.rect.min_x() + selected.rect.width() / 2.0,
        selected.rect.min_y() + selected.rect.height() / 2.0,
    );
    assert_eq!(snapshot.hit_test(center), Some(selected_id));
    assert!(
        snapshot
            .nodes
            .iter()
            .position(|node| node.id == PROJECT_PICKER_SURFACE_ID)
            > snapshot
                .nodes
                .iter()
                .position(|node| node.id == PROJECT_PICKER_TRIGGER_ID)
    );
}

#[test]
fn projectless_state_removes_project_trigger_and_detach_control() {
    let mut state = state_with_projects();
    state.active_workspace = None;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    assert!(snapshot.node(PROJECT_PICKER_TRIGGER_ID).is_none());
    assert!(snapshot.node(PROJECT_DETACH_ID).is_none());
}

#[test]
fn hidden_projectless_workspace_never_becomes_a_project_folder_row() {
    let mut state = state_with_projects();
    let scratch = workspace("file:///private/tmp/zode-projectless-secret");
    state.projectless_workspace_root = Some(scratch.clone());
    state.projects.push(ProjectState {
        workspace_uri: scratch.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 100,
    });
    state.threads.push(ThreadSummary {
        session: SessionLocator::new(state.host.node_id, "task-1"),
        workspace_uri: scratch.clone(),
        title: "独立任务".into(),
        updated_at_ms: 99,
        status: ThreadStatus::Idle,
    });
    let rows = ProjectSidebar::dynamic_row_layout(
        jian_widgets::Rect::xywh(0.0, 0.0, 240.0, 900.0),
        &state,
    );
    assert!(rows.iter().all(|row| match &row.target {
        zode_app_ui::SidebarRowTarget::Project(workspace) => workspace != &scratch,
        zode_app_ui::SidebarRowTarget::Task(_) => true,
        zode_app_ui::SidebarRowTarget::Session(_) => true,
    }));
    assert!(rows
        .iter()
        .all(|row| !row.label.contains("zode-projectless-secret")));
    assert!(ProjectPicker::choices(&state, "")
        .iter()
        .all(|choice| choice.workspace_uri != scratch));
    assert!(rows.iter().any(|row| {
        row.target
            == zode_app_ui::SidebarRowTarget::Task(SessionLocator::new(
                state.host.node_id,
                "task-1",
            ))
            && row.label == "独立任务"
    }));
}
