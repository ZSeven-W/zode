use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, ShellPage};
use zode_app_ui::{
    Insets, ProjectSidebar, RectExt, SidebarAction, TerminalGrid, WorkspaceLayout, WorkspaceShell,
    ZodeTheme,
};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    Fill(Rect, Color),
    FillRound(Rect, f32, Color),
    Text(String),
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
    dpi: f32,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.operations.push(PaintOp::Fill(rect, color));
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        let text = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        self.operations.push(PaintOp::Text(text));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.operations
            .push(PaintOp::FillRound(rect, radius, color));
    }
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
        self.dpi.max(1.0)
    }
}

#[test]
fn workspace_shell_paints_the_shared_geometry_boundaries() {
    let state = demo_state();
    let theme = ZodeTheme::light();
    let viewport = Rect::xywh(0.0, 0.0, 1221.0, 992.0);
    let geometry = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);
    let mut painter = CapturePainter::default();

    WorkspaceShell::paint(&mut painter, viewport, Insets::ZERO, &state, &theme);

    assert!(painter
        .operations
        .contains(&PaintOp::Fill(viewport, theme.tokens.background)));
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(geometry.sidebar, theme.sidebar)));
    assert!(painter
        .operations
        .contains(&PaintOp::Fill(geometry.top_bar, theme.tokens.background)));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, _, color)
            if *rect == geometry.composer && *color == theme.tokens.card
    )));
    assert_eq!(geometry.composer.max_y(), 978.0);
}

#[test]
fn sidebar_has_no_dead_navigation_entries() {
    let items = ProjectSidebar::navigation_items();
    let labels = items.iter().map(|item| item.label).collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![
            "新建任务",
            "工作流",
            "插件",
            "OpenPencil",
            "浏览器",
            "账户与设置",
        ]
    );
    assert!(matches!(items[0].action, SidebarAction::NewSession));
    for item in &items[1..5] {
        assert!(matches!(
            item.action,
            SidebarAction::Navigate {
                page: ShellPage::ComingSoon,
                feature: Some(_),
            }
        ));
    }
    assert!(matches!(
        items[5].action,
        SidebarAction::Navigate {
            page: ShellPage::Settings,
            feature: None,
        }
    ));
}

#[test]
fn shell_uses_theme_tokens_for_both_color_schemes() {
    let light = ZodeTheme::light();
    let dark = ZodeTheme::dark();

    assert_ne!(light.tokens.background, dark.tokens.background);
    assert_ne!(light.sidebar, dark.sidebar);
    assert_eq!(light.zode_purple, dark.zode_purple);
    assert_ne!(light.user_bubble, dark.user_bubble);
    assert!(light.success != light.warning && dark.success != dark.warning);
}

#[test]
fn workspace_shell_routes_the_terminal_page_through_terminal_panel() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Terminal;
    state.terminal.open = true;
    state.terminal.unavailable_reason = Some("Terminal unavailable on this node".into());
    let grid = TerminalGrid::new(80, 24);
    let mut painter = CapturePainter::default();

    WorkspaceShell::paint_with_terminal(
        &mut painter,
        Rect::xywh(0.0, 0.0, 1221.0, 992.0),
        Insets::ZERO,
        &state,
        &grid,
        None,
        &ZodeTheme::light(),
    );

    assert!(painter
        .operations
        .iter()
        .any(|operation| matches!(operation, PaintOp::Text(text) if text == "Terminal unavailable on this node")));
}
