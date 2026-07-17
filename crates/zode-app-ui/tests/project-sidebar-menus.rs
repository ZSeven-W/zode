use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, ProjectDisplayMode, ProjectSortMode, ProjectState, SidebarSectionMenu,
};
use zode_app_ui::{
    ProjectSidebar, SemanticIcon, SidebarRowTarget, ZodeTheme, NEW_SESSION_ID,
    SIDEBAR_PROJECTS_MENU_FLAT_ID, SIDEBAR_PROJECTS_MENU_RECENT_ID, SIDEBAR_PROJECTS_MORE_ID,
    SIDEBAR_PROJECTS_SECTION_ID, SIDEBAR_PROJECT_MENU_FINDER_ID, SIDEBAR_PROJECT_MENU_PIN_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    FillRound(Rect, Color),
    Text(String),
    Svg(String),
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.operations.push(PaintOp::Text(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        ));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, color: Color) {
        self.operations.push(PaintOp::FillRound(rect, color));
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn fill_drop_shadow(&mut self, _rect: Rect, _radius: f32, _blur: f32, _color: Color) {}
    fn stroke_svg_path(
        &mut self,
        d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.operations.push(PaintOp::Svg(d.to_owned()));
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn project_row_keeps_its_hover_background() {
    let mut state = demo_state();
    state.projects.push(ProjectState {
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        expanded: false,
        available: true,
        last_opened_ms: 1,
    });
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let project = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| matches!(row.target, SidebarRowTarget::Project(_)))
        .unwrap();
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint_with_interaction(
        &mut painter,
        rect,
        &state,
        None,
        Some(project.id),
        false,
        &theme,
    );

    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(active, color)
            if *active == project.rect && *color == theme.tokens.muted.with_alpha(0.72)
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path) if path == SemanticIcon::More.path()
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path) if path == SemanticIcon::NewTask.path()
    )));
    let workspace = match project.target {
        SidebarRowTarget::Project(workspace) => workspace,
        _ => unreachable!(),
    };
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, project.new_id.unwrap()),
        Some(AppCommand::BeginTask {
            workspace_uri: Some(workspace.clone()),
        })
    );
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, project.more_id.unwrap()),
        Some(AppCommand::ToggleProjectMenu {
            workspace_uri: workspace,
        })
    );
}

#[test]
fn new_task_and_project_section_reveal_reference_hover_affordances() {
    let state = demo_state();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let theme = ZodeTheme::light();
    let mut new_task = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(
        &mut new_task,
        rect,
        &state,
        None,
        Some(NEW_SESSION_ID),
        false,
        &theme,
    );
    assert!(painted_labels(&new_task).contains(&"⌘N"));

    let mut normal = CapturePainter::default();
    ProjectSidebar::paint(&mut normal, rect, &state, &theme);
    assert!(!normal.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path) if path == SemanticIcon::More.path()
    )));

    let mut section_hover = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(
        &mut section_hover,
        rect,
        &state,
        None,
        Some(SIDEBAR_PROJECTS_SECTION_ID),
        false,
        &theme,
    );
    assert!(section_hover.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path) if path == SemanticIcon::More.path()
    )));
    assert!(section_hover.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path) if path == SemanticIcon::Plus.path()
    )));
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, SIDEBAR_PROJECTS_MORE_ID),
        Some(AppCommand::ToggleSidebarSectionMenu(
            SidebarSectionMenu::Projects
        ))
    );
}

#[test]
fn project_and_section_menus_emit_commands_and_change_real_layout_order() {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    let old = WorkspaceUri::new("file:///repo/old").unwrap();
    let recent = WorkspaceUri::new("file:///repo/recent").unwrap();
    for (workspace_uri, last_opened_ms) in [(old.clone(), 1), (recent.clone(), 20)] {
        state.projects.push(ProjectState {
            workspace_uri: workspace_uri.clone(),
            expanded: true,
            available: true,
            last_opened_ms,
        });
        state.threads.push(ThreadSummary {
            session: SessionLocator::new(state.host.node_id, workspace_uri.as_str()),
            workspace_uri,
            title: format!("task-{last_opened_ms}"),
            updated_at_ms: last_opened_ms,
            status: ThreadStatus::Idle,
        });
    }
    let rect = Rect::xywh(0.0, 0.0, 240.0, 900.0);

    state.sidebar.project_sort_mode = ProjectSortMode::Manual;
    let manual = ProjectSidebar::dynamic_row_layout(rect, &state);
    assert!(matches!(&manual[0].target, SidebarRowTarget::Project(workspace) if workspace == &old));
    state.sidebar.project_sort_mode = ProjectSortMode::RecentlyUpdated;
    let sorted = ProjectSidebar::dynamic_row_layout(rect, &state);
    assert!(
        matches!(&sorted[0].target, SidebarRowTarget::Project(workspace) if workspace == &recent)
    );
    state.sidebar.project_sort_mode = ProjectSortMode::Priority;
    state.sidebar.pinned_projects.insert(old.clone());
    let priority = ProjectSidebar::dynamic_row_layout(rect, &state);
    assert!(
        matches!(&priority[0].target, SidebarRowTarget::Project(workspace) if workspace == &old)
    );
    state.sidebar.project_display_mode = ProjectDisplayMode::Flat;
    let flat = ProjectSidebar::dynamic_row_layout(rect, &state);
    assert!(flat
        .iter()
        .all(|row| !matches!(row.target, SidebarRowTarget::Project(_))));

    state.sidebar.section_menu = Some(SidebarSectionMenu::Projects);
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, SIDEBAR_PROJECTS_MENU_FLAT_ID),
        Some(AppCommand::SetProjectDisplayMode(ProjectDisplayMode::Flat))
    );
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, SIDEBAR_PROJECTS_MENU_RECENT_ID),
        Some(AppCommand::SetProjectSortMode(
            ProjectSortMode::RecentlyUpdated
        ))
    );

    state.sidebar.section_menu = None;
    state.sidebar.project_display_mode = ProjectDisplayMode::Grouped;
    state.sidebar.pinned_projects.remove(&old);
    state.sidebar.project_menu = Some(old.clone());
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, SIDEBAR_PROJECT_MENU_PIN_ID),
        Some(AppCommand::SetProjectPinned {
            workspace_uri: old.clone(),
            pinned: true,
        })
    );
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, SIDEBAR_PROJECT_MENU_FINDER_ID),
        Some(AppCommand::OpenProjectInFinder { workspace_uri: old })
    );
    assert!(ProjectSidebar::menu_layout(rect, &state).is_some());
}

#[test]
fn orphan_project_group_does_not_expose_dead_project_actions() {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    let workspace = WorkspaceUri::new("file:///repo/history-only").unwrap();
    state.threads.push(ThreadSummary {
        session: SessionLocator::new(state.host.node_id, "history-only"),
        workspace_uri: workspace.clone(),
        title: "History-only task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });

    let row = ProjectSidebar::dynamic_row_layout(Rect::xywh(0.0, 0.0, 240.0, 800.0), &state)
        .into_iter()
        .find(|row| matches!(&row.target, SidebarRowTarget::Project(uri) if uri == &workspace))
        .expect("history-only workspace is grouped for browsing");

    assert!(!row.actionable);
    assert_eq!(row.more_id, None);
    assert_eq!(row.new_id, None);
}

fn painted_labels(painter: &CapturePainter) -> Vec<&str> {
    painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::Text(text) => Some(text.as_str()),
            PaintOp::FillRound(..) | PaintOp::Svg(_) => None,
        })
        .collect()
}
