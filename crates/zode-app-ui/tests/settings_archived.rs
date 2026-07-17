use accesskit::{Action, Role, Toggled};
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, reduce_navigation_command, NavigationOutcome, SettingsCategory, ShellPage,
    ShellRoute,
};
use zode_app_ui::{Insets, SettingsPanel, WorkspaceSnapshot, ZodeTheme};
use zode_node_protocol::{NodeId, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
}

impl Painter for PaintCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.texts.push(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        );
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
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
fn archived_page_projects_only_real_archived_threads_and_restores_one() {
    let (mut state, archived_openpencil, _, live) = archived_fixture();
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let content = SettingsPanel::page_layout(snapshot.layout.primary_surface).0;
    let layout = SettingsPanel::archived_task_layout(content, &state);

    assert_eq!(layout.groups.len(), 2);
    assert_eq!(layout.groups[0].label, "openpencil");
    assert_eq!(layout.groups[0].rows.len(), 2);
    assert_eq!(layout.groups[0].rows[0].title, "最新的 OpenPencil 任务");
    assert_eq!(layout.groups[1].label, "zode");
    assert!(layout
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .all(|row| row.session != live));

    let mut painter = PaintCapture::default();
    SettingsPanel::paint_page(
        &mut painter,
        &snapshot,
        &state,
        SettingsPanel::active_workspace_uri(&state),
        &ZodeTheme::light(),
    );
    let text = painter.texts.join("\n");
    for expected in [
        "已归档任务",
        "本机任务",
        "openpencil",
        "2 个任务",
        "最新的 OpenPencil 任务",
        "取消归档",
    ] {
        assert!(
            text.contains(expected),
            "missing archived state: {expected}"
        );
    }
    assert!(!text.contains("仍在侧栏中的任务"));

    let row = layout.groups[0]
        .rows
        .iter()
        .find(|row| row.session == archived_openpencil)
        .expect("archived OpenPencil row");
    assert_eq!(
        SettingsPanel::command_for_widget(&state, row.id),
        Some(row.command.clone())
    );
    assert_eq!(
        reduce_navigation_command(&mut state, row.command.clone()),
        NavigationOutcome::NeedsEffect,
    );
    assert!(!state.archived_sessions.contains(&archived_openpencil));
}

#[test]
fn archived_restore_action_shares_visual_hit_and_accessibility_geometry() {
    let (state, archived, _, _) = archived_fixture();
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let content = SettingsPanel::page_layout(snapshot.layout.primary_surface).0;
    let layout = SettingsPanel::archived_task_layout(content, &state);
    let row = layout
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .find(|row| row.session == archived)
        .expect("archived action row");
    let node = snapshot.node(row.id).expect("archived action node");

    assert_eq!(node.rect, row.action_rect);
    assert_eq!(node.role, Role::Button);
    assert_eq!(node.name, format!("取消归档 {}", row.title));
    assert_eq!(node.value.as_deref(), Some("file:///repo/openpencil"));
    assert!(node.actions.contains(&Action::Click));
    assert!(node.actions.contains(&Action::Focus));
    assert_eq!(
        snapshot.hit_test(rect_center(row.action_rect)),
        Some(row.id)
    );

    let category_id = SettingsPanel::category_widget_id(SettingsCategory::ArchivedTasks);
    let category = snapshot.node(category_id).expect("archived category node");
    assert_eq!(category.toggled, Some(Toggled::True));
    assert!(!category.disabled);
}

#[test]
fn archived_page_has_an_honest_empty_state_without_placeholder_tasks() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::ArchivedTasks);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let content = SettingsPanel::page_layout(snapshot.layout.primary_surface).0;
    let layout = SettingsPanel::archived_task_layout(content, &state);

    assert!(layout.groups.is_empty());
    assert!(layout.empty_card.is_some());
    assert_eq!(SettingsPanel::max_scroll_offset(content, &state), 0.0);

    let mut painter = PaintCapture::default();
    SettingsPanel::paint_page(&mut painter, &snapshot, &state, None, &ZodeTheme::light());
    let text = painter.texts.join("\n");
    assert!(text.contains("暂无已归档任务"));
    assert!(text.contains("归档的任务会保留在本机"));
    assert!(!text.contains("示例任务"));
    assert!(snapshot
        .nodes
        .iter()
        .all(|node| !node.name.starts_with("取消归档 ")));
}

fn archived_fixture() -> (
    zode_app_model::ZodeAppState,
    SessionLocator,
    SessionLocator,
    SessionLocator,
) {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::ArchivedTasks);
    let openpencil = WorkspaceUri::new("file:///repo/openpencil").unwrap();
    let zode = WorkspaceUri::new("file:///repo/zode").unwrap();
    let archived_openpencil = add_thread(
        &mut state,
        &openpencil,
        "archived-openpencil",
        "较早的 OpenPencil 任务",
        10,
    );
    let newest_openpencil = add_thread(
        &mut state,
        &openpencil,
        "newest-openpencil",
        "最新的 OpenPencil 任务",
        30,
    );
    let live = add_thread(
        &mut state,
        &openpencil,
        "live-openpencil",
        "仍在侧栏中的任务",
        40,
    );
    let archived_zode = add_thread(&mut state, &zode, "archived-zode", "Zode 任务", 20);
    state.archived_sessions.extend([
        archived_openpencil.clone(),
        newest_openpencil.clone(),
        archived_zode,
    ]);
    (state, archived_openpencil, newest_openpencil, live)
}

fn add_thread(
    state: &mut zode_app_model::ZodeAppState,
    workspace_uri: &WorkspaceUri,
    id: &str,
    title: &str,
    updated_at_ms: i64,
) -> SessionLocator {
    let session = SessionLocator::new(NodeId::new(), id);
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: title.into(),
        updated_at_ms,
        status: ThreadStatus::Idle,
    });
    session
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
