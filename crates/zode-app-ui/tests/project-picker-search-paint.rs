use jian_core::text_input::TextInputState;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, ProjectState};
use zode_app_ui::{
    ProjectPicker, ProjectPickerViewState, RectExt, SemanticIcon, ZodeTheme,
    PROJECT_PICKER_SEARCH_ID,
};
use zode_node_protocol::WorkspaceUri;

#[derive(Debug, Clone, Copy)]
struct RoundStroke {
    rect: Rect,
    color: Color,
    width: f32,
}

#[derive(Debug, Clone)]
struct SvgStroke {
    path: String,
    top_left: Point2D,
    size: f32,
}

#[derive(Debug, Clone)]
struct TextCall {
    text: String,
    origin: Point2D,
    size: f32,
}

#[derive(Default)]
struct CapturePainter {
    fills: Vec<(Rect, Color)>,
    rounded_strokes: Vec<RoundStroke>,
    svg_strokes: Vec<SvgStroke>,
    texts: Vec<TextCall>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.fills.push((rect, color));
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let run = layout.runs().first().expect("single-run search text");
        self.texts.push(TextCall {
            text: run.content.clone(),
            origin,
            size: run.font_size,
        });
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, rect: Rect, _radius: f32, color: Color, width: f32) {
        self.rounded_strokes
            .push(RoundStroke { rect, color, width });
    }
    fn stroke_svg_path(
        &mut self,
        path: &str,
        top_left: Point2D,
        size: f32,
        _color: Color,
        _width: f32,
    ) {
        self.svg_strokes.push(SvgStroke {
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
        1.0
    }
}

fn assert_close(left: f32, right: f32) {
    assert!(
        (left - right).abs() < 0.01,
        "expected {left} to be close to {right}"
    );
}

fn layout() -> zode_app_ui::ProjectPickerLayout {
    let mut state = demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects = vec![ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    }];
    state.active_workspace = Some(workspace_uri);
    ProjectPicker::layout(
        Rect::xywh(0.0, 0.0, 900.0, 700.0),
        Rect::xywh(320.0, 500.0, 120.0, 32.0),
        &state,
        &ProjectPickerViewState {
            open: true,
            query: String::new(),
        },
    )
    .expect("open project picker")
}

#[test]
fn focused_search_keeps_input_semantics_without_a_visible_ring() {
    let layout = layout();
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectPicker::paint(
        &mut painter,
        &layout,
        &TextInputState::default(),
        Some(PROJECT_PICKER_SEARCH_ID),
        None,
        &theme,
    );

    let search_strokes = painter
        .rounded_strokes
        .iter()
        .filter(|stroke| stroke.rect == layout.search)
        .collect::<Vec<_>>();
    assert_eq!(search_strokes.len(), 1);
    assert_eq!(search_strokes[0].color, theme.tokens.input);
    assert_close(search_strokes[0].width, 1.0);
    assert!(search_strokes
        .iter()
        .all(|stroke| stroke.color != theme.tokens.ring));

    assert!(painter.fills.iter().any(|(rect, color)| {
        *color == theme.tokens.foreground
            && rect.origin.x >= layout.search.origin.x
            && rect.max_x() <= layout.search.max_x()
            && rect.origin.y >= layout.search.origin.y
            && rect.max_y() <= layout.search.max_y()
            && (rect.size.x - 1.5).abs() < 0.01
    }));
}

#[test]
fn search_icon_and_placeholder_share_the_field_centerline() {
    let layout = layout();
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectPicker::paint(
        &mut painter,
        &layout,
        &TextInputState::default(),
        Some(PROJECT_PICKER_SEARCH_ID),
        None,
        &theme,
    );

    let field_center_y = layout.search.origin.y + layout.search.size.y / 2.0;
    let search_icon = painter
        .svg_strokes
        .iter()
        .find(|stroke| stroke.path == SemanticIcon::Search.path())
        .expect("search icon");
    assert_close(
        search_icon.top_left.y + search_icon.size / 2.0,
        field_center_y,
    );

    let placeholder = painter
        .texts
        .iter()
        .find(|call| call.text == "搜索项目")
        .expect("search placeholder");
    assert_close(
        placeholder.origin.y + placeholder.size / 2.0,
        field_center_y,
    );
}
