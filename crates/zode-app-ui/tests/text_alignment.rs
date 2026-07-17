use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, ComposerState, ConnectionState, SettingsCategory, ShellPage, ShellRoute,
};
use zode_app_ui::{
    Composer, Insets, ProjectSidebar, SettingsPanel, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme,
};

const PLUS_PATH: &str = "M4 12H20M12 4V20";
const MIC_PATH: &str = "M9 5V12A3 3 0 0 0 15 12V5M6 11A6 6 0 0 0 18 11M12 17V21";

#[derive(Debug, Clone)]
struct TextCall {
    text: String,
    origin: Point2D,
    size: f32,
}

#[derive(Debug, Clone)]
struct SvgCall {
    path: String,
    top_left: Point2D,
    size: f32,
}

struct CapturePainter {
    dpi: f32,
    texts: Vec<TextCall>,
    svgs: Vec<SvgCall>,
    rounded_fills: Vec<Rect>,
}

impl CapturePainter {
    fn new(dpi: f32) -> Self {
        Self {
            dpi,
            texts: Vec::new(),
            svgs: Vec::new(),
            rounded_fills: Vec::new(),
        }
    }

    fn text(&self, text: &str) -> &TextCall {
        self.texts
            .iter()
            .find(|call| call.text == text)
            .unwrap_or_else(|| panic!("missing text call: {text}"))
    }

    fn svg(&self, path: &str) -> &SvgCall {
        self.svgs
            .iter()
            .find(|call| call.path == path)
            .unwrap_or_else(|| panic!("missing svg call: {path}"))
    }
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let run = layout.runs().first().expect("single-line control text");
        self.texts.push(TextCall {
            text: run.content.clone(),
            origin,
            size: run.font_size,
        });
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.rounded_fills.push(rect);
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        path: &str,
        top_left: Point2D,
        size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svgs.push(SvgCall {
            path: path.to_owned(),
            top_left,
            size,
        });
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        self.dpi
    }
}

#[test]
fn sidebar_row_uses_a_vertically_centered_13px_line_box() {
    let rect = WorkspaceLayout::compute(1_440.0, 900.0, Insets::ZERO).sidebar;
    let row = ProjectSidebar::navigation_row_layout(rect)[0].rect;
    let mut painter = CapturePainter::new(1.0);

    ProjectSidebar::paint(&mut painter, rect, &demo_state(), &ZodeTheme::light());

    let origin = painter.text("新建任务").origin;
    assert_close(origin.y, row.origin.y + (row.size.y - 13.0) / 2.0, 0.01);
}

#[test]
fn settings_value_row_centers_both_line_boxes_on_the_row() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    state.host.connection = ConnectionState::Unavailable;
    let layout = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        state.presentation.route,
        None,
    );
    let (_, card) = SettingsPanel::page_layout(layout.primary_surface);
    let row_center = card.origin.y + 26.0;
    let mut painter = CapturePainter::new(1.0);

    SettingsPanel::paint_page(
        &mut painter,
        &snapshot(layout),
        &state,
        None,
        &ZodeTheme::light(),
    );

    let left = painter.text("主机连接");
    let right = painter.text("不可用");
    let left_center = left.origin.y + left.size / 2.0;
    let right_center = right.origin.y + right.size / 2.0;
    assert_close(left_center, row_center, 1.0);
    assert_close(right_center, row_center, 1.0);
    assert_close(left_center, right_center, 1.0);
}

#[test]
fn composer_bottom_controls_share_the_send_button_centerline() {
    let rect = WorkspaceLayout::compute(1_440.0, 900.0, Insets::ZERO).composer;
    let state = ComposerState {
        sandbox_label: "工作区写入".into(),
        model: Some("gpt-5".into()),
        effort: Some("高".into()),
        ..ComposerState::default()
    };
    let mut painter = CapturePainter::new(1.0);

    Composer::paint(&mut painter, rect, &state, &ZodeTheme::light());

    let send = painter
        .rounded_fills
        .iter()
        .copied()
        .find(|fill| fill.size == Point2D::new(28.0, 28.0))
        .expect("send button fill");
    let center_y = send.origin.y + send.size.y / 2.0;
    for svg in [painter.svg(PLUS_PATH), painter.svg(MIC_PATH)] {
        assert_close(svg.top_left.y + svg.size / 2.0, center_y, 1.0);
    }
    for label in ["工作区写入", "gpt-5", "高"] {
        let text = painter.text(label);
        assert_close(text.origin.y + text.size / 2.0, center_y, 1.0);
    }
}

#[test]
fn centered_control_error_stays_within_one_and_a_half_physical_pixels() {
    let rect = WorkspaceLayout::compute(1_440.0, 900.0, Insets::ZERO).sidebar;
    let row = ProjectSidebar::navigation_row_layout(rect)[0].rect;
    let expected = row.origin.y + (row.size.y - 13.0) / 2.0;

    for dpi in [1.0, 1.25, 2.0] {
        let mut painter = CapturePainter::new(dpi);
        ProjectSidebar::paint(&mut painter, rect, &demo_state(), &ZodeTheme::light());
        let logical_error = (painter.text("新建任务").origin.y - expected).abs();
        assert!(
            logical_error * painter.dpi_scale() <= 1.5,
            "dpi={dpi}: physical error was {}px",
            logical_error * painter.dpi_scale()
        );
    }
}

fn snapshot(layout: WorkspaceLayout) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        layout,
        nodes: Vec::new(),
        focused: None,
    }
}

fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {actual} to be within {tolerance} of {expected}"
    );
}
