use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::ComposerState;
use zode_app_ui::{Composer, ZodeTheme};

#[derive(Default)]
struct TextCapture {
    texts: Vec<String>,
}

impl Painter for TextCapture {
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
fn composer_uses_text_area_multiline_layout() {
    let state = ComposerState {
        draft: "first line\nsecond line".into(),
        focused: true,
        ..ComposerState::default()
    };
    let mut painter = TextCapture::default();

    Composer::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 500.0, 120.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|text| text == "first line"));
    assert!(painter.texts.iter().any(|text| text == "second line"));
    assert!(!painter
        .texts
        .iter()
        .any(|text| text == "first line\nsecond line"));
}
