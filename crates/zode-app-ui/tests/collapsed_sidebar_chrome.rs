use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::demo_state;
use zode_app_ui::{
    CollapsedSidebarChrome, Insets, SemanticIcon, ThreadHeader, WidgetId, WorkspaceShell,
    ZodeTheme, COLLAPSED_SIDEBAR_BACK_ID, COLLAPSED_SIDEBAR_CHROME_TRAILING_EDGE,
    COLLAPSED_SIDEBAR_FORWARD_ID, NEW_SESSION_ID, SIDEBAR_TOGGLE_ID,
};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    FillRound(Rect, f32, Color),
    StrokeRound(Rect, f32, Color, f32),
    Svg(String, Point2D, f32, Color, f32),
    Line(Point2D, Point2D, Color, f32),
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
    fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, from: Point2D, to: Point2D, color: Color, width: f32) {
        self.operations.push(PaintOp::Line(from, to, color, width));
    }
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.operations
            .push(PaintOp::FillRound(rect, radius, color));
    }
    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, color: Color, width: f32) {
        self.operations
            .push(PaintOp::StrokeRound(rect, radius, color, width));
    }
    fn stroke_svg_path(
        &mut self,
        path: &str,
        top_left: Point2D,
        size: f32,
        color: Color,
        width: f32,
    ) {
        self.operations
            .push(PaintOp::Svg(path.into(), top_left, size, color, width));
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
fn collapsed_sidebar_chrome_preserves_the_four_reference_buttons() {
    let top_bar = Rect::xywh(0.0, 0.0, 1_000.0, 46.0);
    let layout = CollapsedSidebarChrome::layout(top_bar);

    assert_eq!(layout.toggle, Rect::xywh(88.0, 11.0, 24.0, 24.0));
    assert_eq!(layout.back, Rect::xywh(123.0, 11.0, 24.0, 24.0));
    assert_eq!(layout.forward, Rect::xywh(157.0, 11.0, 24.0, 24.0));
    assert_eq!(layout.new_task, Rect::xywh(191.0, 11.0, 24.0, 24.0));
    assert_eq!(layout.trailing_edge, COLLAPSED_SIDEBAR_CHROME_TRAILING_EDGE);
    assert_eq!(
        CollapsedSidebarChrome::content_rect(top_bar),
        Rect::xywh(224.0, 0.0, 776.0, 46.0)
    );
}

#[test]
fn collapsed_sidebar_chrome_uses_regular_icons_and_muted_hover_only() {
    let top_bar = Rect::xywh(0.0, 0.0, 1_000.0, 46.0);
    let layout = CollapsedSidebarChrome::layout(top_bar);
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    CollapsedSidebarChrome::paint(&mut painter, top_bar, None, Some(SIDEBAR_TOGGLE_ID), &theme);

    assert!(painter.operations.contains(&PaintOp::FillRound(
        layout.toggle,
        6.0,
        theme.tokens.muted,
    )));
    assert!(!painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(rect, ..) if *rect == layout.back || *rect == layout.forward
    )));
    assert!(!painter
        .operations
        .iter()
        .any(|operation| matches!(operation, PaintOp::StrokeRound(..))));

    for (icon, origin) in [
        (SemanticIcon::Sidebar, Point2D::new(93.0, 16.0)),
        (SemanticIcon::Back, Point2D::new(128.0, 16.0)),
        (SemanticIcon::Forward, Point2D::new(162.0, 16.0)),
        (SemanticIcon::NewTask, Point2D::new(196.0, 16.0)),
    ] {
        assert!(painter.operations.iter().any(|operation| matches!(
            operation,
            PaintOp::Svg(path, top_left, 14.0, _, width)
                if path == icon.path()
                    && *top_left == origin
                    && *width == icon.stroke_width()
        )));
    }

    let mut new_task_hover = CapturePainter::default();
    CollapsedSidebarChrome::paint(
        &mut new_task_hover,
        top_bar,
        None,
        Some(NEW_SESSION_ID),
        &theme,
    );
    assert!(new_task_hover.operations.contains(&PaintOp::FillRound(
        layout.new_task,
        6.0,
        theme.tokens.muted,
    )));

    for disabled in [COLLAPSED_SIDEBAR_BACK_ID, COLLAPSED_SIDEBAR_FORWARD_ID] {
        let mut disabled_hover = CapturePainter::default();
        CollapsedSidebarChrome::paint(
            &mut disabled_hover,
            top_bar,
            Some(disabled),
            Some(disabled),
            &theme,
        );
        assert!(!disabled_hover
            .operations
            .iter()
            .any(|operation| matches!(operation, PaintOp::FillRound(..))));
    }
}

#[test]
fn collapsed_chrome_fades_with_sidebar_motion_instead_of_appearing_at_once() {
    let top_bar = Rect::xywh(0.0, 0.0, 1_000.0, 46.0);
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    CollapsedSidebarChrome::paint_with_opacity(&mut painter, top_bar, None, None, 0.5, &theme);

    let expected = theme
        .tokens
        .muted_foreground
        .with_alpha(theme.tokens.muted_foreground.a * 0.5);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path, _, _, color, _)
            if path == SemanticIcon::Sidebar.path() && *color == expected
    )));

    let mut hidden = CapturePainter::default();
    CollapsedSidebarChrome::paint_with_opacity(&mut hidden, top_bar, None, None, 0.0, &theme);
    assert!(hidden.operations.is_empty());
}

#[test]
fn collapsed_header_offsets_only_its_content_and_keeps_the_separator_full_width() {
    let raw = Rect::xywh(0.0, 0.0, 1_000.0, 46.0);
    let mut state = demo_state();
    state.shell.sidebar_open = false;

    assert_eq!(
        ThreadHeader::content_rect(raw, &state),
        Rect::xywh(224.0, 0.0, 776.0, 46.0)
    );
    assert_eq!(ThreadHeader::layout(raw, &state).title.origin.x, 264.0);

    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();
    ThreadHeader::paint(&mut painter, raw, &state, &theme);
    assert!(painter.operations.contains(&PaintOp::Line(
        Point2D::new(0.0, 46.0),
        Point2D::new(1_000.0, 46.0),
        theme.tokens.border,
        1.0,
    )));

    state.shell.sidebar_open = true;
    assert_eq!(ThreadHeader::content_rect(raw, &state), raw);
    assert_eq!(ThreadHeader::layout(raw, &state).title.origin.x, 40.0);
}

#[test]
fn header_leading_edge_tracks_sidebar_visibility_without_a_boolean_jump() {
    let open_bar = Rect::xywh(293.0, 0.0, 1_507.0, 46.0);
    let halfway_bar = Rect::xywh(146.5, 0.0, 1_653.5, 46.0);
    let closed_bar = Rect::xywh(0.0, 0.0, 1_800.0, 46.0);

    let open = ThreadHeader::content_rect_with_sidebar_visibility(open_bar, 1.0);
    let halfway = ThreadHeader::content_rect_with_sidebar_visibility(halfway_bar, 0.5);
    let closed = ThreadHeader::content_rect_with_sidebar_visibility(closed_bar, 0.0);

    assert_eq!(open.origin.x, 293.0);
    assert_eq!(halfway.origin.x, 258.5);
    assert_eq!(closed.origin.x, 224.0);
    assert!(open.origin.x > halfway.origin.x);
    assert!(halfway.origin.x > closed.origin.x);
}

#[test]
fn chrome_action_ids_are_distinct() {
    let ids: [WidgetId; 4] = [
        SIDEBAR_TOGGLE_ID,
        COLLAPSED_SIDEBAR_BACK_ID,
        COLLAPSED_SIDEBAR_FORWARD_ID,
        NEW_SESSION_ID,
    ];
    for (index, id) in ids.iter().enumerate() {
        assert!(!ids[index + 1..].contains(id));
    }
}

#[test]
fn workspace_shell_paints_collapsed_chrome_without_reflowing_the_top_bar() {
    let viewport = Rect::xywh(0.0, 0.0, 1_000.0, 700.0);
    let mut state = demo_state();
    state.shell.sidebar_open = false;
    let mut painter = CapturePainter::default();

    let geometry = WorkspaceShell::paint(
        &mut painter,
        viewport,
        Insets::ZERO,
        &state,
        &ZodeTheme::light(),
    );

    assert_eq!(geometry.sidebar.size.x, 0.0);
    assert_eq!(geometry.top_bar, Rect::xywh(0.0, 0.0, 1_000.0, 46.0));
    for (icon, origin) in [
        (SemanticIcon::Sidebar, Point2D::new(93.0, 16.0)),
        (SemanticIcon::Back, Point2D::new(128.0, 16.0)),
        (SemanticIcon::Forward, Point2D::new(162.0, 16.0)),
        (SemanticIcon::NewTask, Point2D::new(196.0, 16.0)),
    ] {
        assert!(painter.operations.iter().any(|operation| matches!(
            operation,
            PaintOp::Svg(path, top_left, 14.0, _, width)
                if path == icon.path()
                    && *top_left == origin
                    && *width == icon.stroke_width()
        )));
    }
}
