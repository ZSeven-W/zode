use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, ConnectionState, SettingsCategory, ShellPage, ShellRoute,
};
use zode_app_ui::{Insets, SettingsPanel, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme};
use zode_node_protocol::{NodeCapability, WorkspaceUri};

#[derive(Default)]
struct PaintCapture {
    texts: Vec<String>,
    rounded_fills: Vec<(Rect, f32)>,
    rounded_strokes: Vec<(Rect, f32, f32)>,
    dividers: Vec<(Point2D, Point2D)>,
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
    fn stroke_line(&mut self, from: Point2D, to: Point2D, _color: Color, _width: f32) {
        self.dividers.push((from, to));
    }
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, _color: Color) {
        self.rounded_fills.push((rect, radius));
    }
    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, _color: Color, width: f32) {
        self.rounded_strokes.push((rect, radius, width));
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

#[test]
fn category_rail_has_stable_rows_typed_commands_and_honest_placeholders() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Appearance);
    state.shell.page = ShellPage::Settings;

    let rows = SettingsPanel::category_rows(Rect::xywh(0.0, 0.0, 240.0, 1_080.0), &state);

    assert_eq!(
        rows.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![
            SettingsPanel::category_widget_id(SettingsCategory::General),
            SettingsPanel::category_widget_id(SettingsCategory::Appearance),
            SettingsPanel::category_widget_id(SettingsCategory::Permissions),
            SettingsPanel::category_widget_id(SettingsCategory::KeyboardShortcuts),
            SettingsPanel::category_widget_id(SettingsCategory::Environment),
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.3).collect::<Vec<_>>(),
        vec!["常规", "外观", "权限", "键盘快捷键", "环境"]
    );
    assert_eq!(rows.iter().filter(|row| row.4).count(), 1);
    assert!(rows[1].4);
    assert!(rows[0].5 && rows[1].5 && rows[2].5);
    assert!(!rows[3].5 && !rows[4].5);
    assert_eq!(rows[0].1, Rect::xywh(8.0, 154.0, 224.0, 30.0));
    assert_eq!(rows[4].1, Rect::xywh(8.0, 274.0, 224.0, 30.0));

    for row in rows {
        assert_eq!(
            SettingsPanel::command_for_widget(&state, row.0),
            Some(AppCommand::SelectSettingsCategory(row.2))
        );
    }

    let painter = paint_settings(&state);
    let text = painter.texts.join("\n");
    assert!(text.contains("搜索即将支持"));
    assert_eq!(
        painter
            .texts
            .iter()
            .filter(|text| text.as_str() == "即将支持")
            .count(),
        2
    );
}

#[test]
fn settings_page_centers_a_768px_grouped_card_column() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    state.shell.page = ShellPage::Settings;
    let layout = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        state.presentation.route,
        None,
    );
    let page = SettingsPanel::page_layout(layout.primary_surface);

    assert_eq!(page.0, Rect::xywh(636.0, 70.0, 768.0, 1_010.0));
    assert_eq!(page.1, Rect::xywh(636.0, 154.0, 768.0, 156.0));

    let mut painter = PaintCapture::default();
    let snapshot = snapshot(layout);
    SettingsPanel::paint_page(&mut painter, &snapshot, &state, None, &ZodeTheme::light());
    assert!(painter
        .rounded_strokes
        .iter()
        .any(|(rect, radius, width)| *rect == page.1
            && (10.0..=16.0).contains(radius)
            && *width == 1.0));
    assert_eq!(painter.dividers.len(), 2);
}

#[test]
fn general_page_projects_only_real_local_host_workspace_and_capability_state() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    state.shell.page = ShellPage::Settings;
    state.host.connection = ConnectionState::Local;
    state
        .host
        .capabilities
        .capabilities
        .extend([NodeCapability::Agent, NodeCapability::Terminal]);
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.active_workspace = Some(workspace);

    let painter = paint_settings(&state);
    let text = painter.texts.join("\n");

    for expected in [
        "常规",
        "本地运行状态",
        "主机连接",
        "本地",
        "活动工作区",
        "file:///repo/zode",
        "可用能力",
        "2 项",
    ] {
        assert!(text.contains(expected), "missing real state: {expected}");
    }
    for fabricated in ["账户", "登录", "默认权限", "自动审批", "完全访问权限"] {
        assert!(
            !text.contains(fabricated),
            "fabricated setting: {fabricated}"
        );
    }

    state.active_workspace = None;
    let unselected = paint_settings(&state).texts.join("\n");
    assert!(unselected.contains("未选择"));
    assert!(!unselected.contains("file:///repo/zode"));
}

#[test]
fn appearance_and_permissions_are_isolated_typed_categories() {
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.active_workspace = Some(workspace.clone());
    state
        .project_permissions
        .insert(workspace.clone(), vec!["write_file".into()]);

    state.presentation.route = ShellRoute::Settings(SettingsCategory::Appearance);
    let appearance = paint_settings(&state).texts.join("\n");
    for expected in ["外观", "主题与动态效果", "跟随系统", "减少动画", "高对比度"]
    {
        assert!(appearance.contains(expected));
    }
    assert!(!appearance.contains("项目权限"));
    assert!(!appearance.contains("write_file"));
    assert_eq!(SettingsPanel::appearance_controls(&state).len(), 5);
    assert_eq!(
        SettingsPanel::appearance_control_layout(
            SettingsPanel::page_layout(Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0)).0,
            &state,
        )
        .len(),
        5
    );
    assert!(SettingsPanel::permission_row_layout(
        Rect::xywh(636.0, 70.0, 768.0, 1_010.0),
        &state,
        &workspace,
    )
    .is_empty());

    state.presentation.route = ShellRoute::Settings(SettingsCategory::Permissions);
    let permissions = paint_settings(&state).texts.join("\n");
    for expected in [
        "权限",
        "项目权限",
        "file:///repo/zode",
        "write_file",
        "撤销",
    ] {
        assert!(permissions.contains(expected));
    }
    assert!(!permissions.contains("主题与动态效果"));
    assert!(SettingsPanel::appearance_control_layout(
        Rect::xywh(636.0, 70.0, 768.0, 1_010.0),
        &state,
    )
    .is_empty());
    assert_eq!(
        SettingsPanel::permission_row_layout(
            Rect::xywh(636.0, 70.0, 768.0, 1_010.0),
            &state,
            &workspace,
        )
        .len(),
        1
    );
}

#[test]
fn unfinished_categories_are_explicit_placeholders_without_fake_controls() {
    for (category, title) in [
        (SettingsCategory::KeyboardShortcuts, "键盘快捷键"),
        (SettingsCategory::Environment, "环境"),
    ] {
        let mut state = demo_state();
        state.shell.page = ShellPage::Settings;
        state.presentation.route = ShellRoute::Settings(category);

        let text = paint_settings(&state).texts.join("\n");

        assert!(text.contains(title));
        assert!(text.contains("即将支持"));
        for fake_control in ["启用", "快捷键录制", "环境变量编辑", "保存"] {
            assert!(!text.contains(fake_control));
        }
    }
}

fn paint_settings(state: &zode_app_model::ZodeAppState) -> PaintCapture {
    let layout = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        state.presentation.route,
        None,
    );
    let snapshot = snapshot(layout);
    let mut painter = PaintCapture::default();
    let workspace = SettingsPanel::active_workspace_uri(state);
    SettingsPanel::paint_page(
        &mut painter,
        &snapshot,
        state,
        workspace,
        &ZodeTheme::light(),
    );
    painter
}

fn snapshot(layout: WorkspaceLayout) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        layout,
        nodes: Vec::new(),
        focused: None,
    }
}
