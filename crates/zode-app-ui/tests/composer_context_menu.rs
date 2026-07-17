use jian_core::text_input::TextInputState;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, BranchCatalog, BranchCatalogState,
    ComposerContextMenu as ContextMenuKind, ProjectState, TaskLaunchMode,
};
use zode_app_ui::{
    Composer, ComposerContextMenu, WidgetId, ZodeTheme, COMPOSER_BRANCH_CREATE_ID,
    COMPOSER_BRANCH_SEARCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID, COMPOSER_LOCATION_LOCAL_ID,
    COMPOSER_LOCATION_WORKTREE_ID,
};
use zode_node_protocol::WorkspaceUri;

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    round_fills: Vec<Rect>,
    round_strokes: Vec<Rect>,
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
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.round_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color, _width: f32) {
        self.round_strokes.push(rect);
    }
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

fn workspace() -> WorkspaceUri {
    WorkspaceUri::new("file:///tmp/zode-composer-menu").unwrap()
}

fn state() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let workspace_uri = workspace();
    state.projects = vec![ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    }];
    state.active_workspace = Some(workspace_uri);
    state.current_session = None;
    state
}

fn context(state: &zode_app_model::ZodeAppState) -> zode_app_ui::ComposerContextLayout {
    Composer::context_interaction_layout(
        Rect::xywh(100.0, 700.0, 736.0, 138.0),
        state,
        Some("zode"),
        Some("本地"),
        Some("main"),
    )
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn location_menu_is_anchored_above_and_keeps_worktree_truthfully_disabled() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Location);
    let chips = context(&state);
    let anchor = chips.location.unwrap().rect;
    let menu =
        ComposerContextMenu::layout(Rect::xywh(0.0, 0.0, 1_000.0, 900.0), chips, &state).unwrap();

    assert_eq!(menu.surface.size.x, 260.0);
    assert!(menu.surface.origin.y + menu.surface.size.y < anchor.origin.y);
    assert_eq!(menu.rows[0].id, COMPOSER_LOCATION_LOCAL_ID);
    assert!(menu.rows[0].enabled && menu.rows[0].selected);
    assert_eq!(menu.rows[1].id, COMPOSER_LOCATION_WORKTREE_ID);
    assert!(!menu.rows[1].enabled);
    assert_eq!(
        menu.rows[1].secondary.as_deref(),
        Some("尚未接入工作树创建")
    );
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, COMPOSER_LOCATION_LOCAL_ID),
        Some(AppCommand::SelectTaskLaunchMode(TaskLaunchMode::Local))
    );
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, COMPOSER_LOCATION_WORKTREE_ID),
        None
    );
    assert_eq!(
        ComposerContextMenu::focus_ids(&menu),
        vec![COMPOSER_LOCATION_LOCAL_ID]
    );
    assert_eq!(
        ComposerContextMenu::widget_at(&menu, center(menu.rows[1].rect)),
        Some(COMPOSER_CONTEXT_MENU_SURFACE_ID)
    );
}

#[test]
fn branch_menu_filters_real_catalog_and_emits_a_checkout_intent() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Branch);
    state.composer.branch_picker.query = "feature".into();
    state.composer.selected_branch = Some("feature/menu".into());
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
        workspace_uri: workspace(),
        current: "main".into(),
        branches: vec![
            "main".into(),
            "feature/menu".into(),
            "feature/search".into(),
            "release".into(),
        ],
        dirty_files: 0,
    });
    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();

    assert_eq!(menu.surface.size.x, 300.0);
    assert!(menu.search.is_some());
    assert_eq!(menu.rows.len(), 2);
    assert!(menu.rows[0].selected);
    assert_eq!(menu.rows[0].label, "feature/menu");
    assert_eq!(menu.create.as_ref().unwrap().id, COMPOSER_BRANCH_CREATE_ID);
    assert!(!menu.create.as_ref().unwrap().enabled);
    assert_eq!(
        ComposerContextMenu::focus_ids(&menu),
        vec![COMPOSER_BRANCH_SEARCH_ID, menu.rows[0].id, menu.rows[1].id]
    );
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, menu.rows[1].id),
        Some(AppCommand::SelectBranch {
            workspace_uri: workspace(),
            branch: "feature/search".into(),
        })
    );
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, COMPOSER_BRANCH_CREATE_ID),
        None
    );
}

#[test]
fn dirty_workspace_keeps_the_current_branch_actionable_and_blocks_other_checkouts() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Branch);
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
        workspace_uri: workspace(),
        current: "main".into(),
        branches: vec!["main".into(), "feature/menu".into()],
        dirty_files: 2,
    });
    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();

    let current = menu.rows.iter().find(|row| row.label == "main").unwrap();
    assert!(current.enabled);
    let feature = menu
        .rows
        .iter()
        .find(|row| row.label == "feature/menu")
        .unwrap();
    assert!(!feature.enabled);
    assert_eq!(feature.secondary.as_deref(), Some("请先提交或储藏更改"));
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, feature.id),
        None
    );
}

#[test]
fn active_workspace_turn_blocks_noncurrent_branch_actions() {
    let mut state = state();
    let workspace_uri = workspace();
    let session = zode_node_protocol::SessionLocator::new(state.host.node_id, "active-turn");
    state.threads.push(zode_node_protocol::ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace_uri.clone(),
        title: "running".into(),
        updated_at_ms: 0,
        status: zode_node_protocol::ThreadStatus::Running,
    });
    state
        .active_turns
        .insert(session, zode_node_protocol::TurnId::new());
    state.composer.context_menu = Some(ContextMenuKind::Branch);
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
        workspace_uri,
        current: "main".into(),
        branches: vec!["main".into(), "feature/menu".into()],
        dirty_files: 0,
    });

    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();
    let feature = menu
        .rows
        .iter()
        .find(|row| row.label == "feature/menu")
        .unwrap();

    assert!(!feature.enabled);
    assert_eq!(feature.secondary.as_deref(), Some("工作区有运行中的任务"));
    assert_eq!(
        ComposerContextMenu::command_for_widget(&state, feature.id),
        None
    );
}

#[test]
fn branch_catalog_never_leaks_across_workspaces() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Branch);
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
        workspace_uri: WorkspaceUri::new("file:///tmp/other-project").unwrap(),
        current: "other".into(),
        branches: vec!["other".into()],
        dirty_files: 0,
    });
    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();

    assert!(menu.rows.is_empty());
    assert_eq!(menu.status.as_ref().unwrap().message, "正在读取分支…");
}

#[test]
fn focused_rows_use_a_fill_state_without_a_focus_stroke() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Location);
    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();
    let mut painter = PaintCapture::default();
    ComposerContextMenu::paint(
        &mut painter,
        &menu,
        &TextInputState::default(),
        Some(COMPOSER_LOCATION_LOCAL_ID),
        None,
        &ZodeTheme::light(),
    );

    assert!(painter.round_fills.contains(&menu.rows[0].rect));
    assert!(!painter.round_strokes.contains(&menu.rows[0].rect));
    assert!(painter.texts.iter().any(|text| text == "启动模式"));
    assert!(painter
        .texts
        .iter()
        .any(|text| text == "尚未接入工作树创建"));
}

#[test]
fn branch_search_ime_anchor_tracks_the_painted_caret() {
    let mut state = state();
    state.composer.context_menu = Some(ContextMenuKind::Branch);
    state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
        workspace_uri: workspace(),
        current: "main".into(),
        branches: vec!["main".into()],
        dirty_files: 0,
    });
    let menu = ComposerContextMenu::layout(
        Rect::xywh(0.0, 0.0, 1_000.0, 900.0),
        context(&state),
        &state,
    )
    .unwrap();
    let search = menu.search.unwrap();
    let input = TextInputState::with_text("feature/menu");
    let mut metrics = PaintCapture::default();

    let caret = ComposerContextMenu::branch_search_ime_cursor_area(
        &mut metrics,
        search,
        &input,
        &ZodeTheme::light(),
    )
    .unwrap();

    assert!(caret.origin.x > search.origin.x + 20.0);
    assert!(caret.origin.x < search.origin.x + search.size.x);
    assert!(caret.origin.y >= search.origin.y);
    assert!(caret.origin.y + caret.size.y <= search.origin.y + search.size.y);
}
