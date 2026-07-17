use jian_core::text_input::TextInputState;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout, TextMetrics};

use crate::ZodeTheme;

use super::input::{text_area, text_area_rect};

pub(super) fn cursor_area(
    metrics: &mut dyn Painter,
    rect: Rect,
    input: &TextInputState,
    theme: &ZodeTheme,
) -> Option<Rect> {
    if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return None;
    }
    let mut caret_state = input.clone();
    caret_state.set_caret(input.caret(), 0);
    let mut probe = CaretProbe {
        metrics,
        caret: None,
    };
    text_area(&caret_state, true).paint(&mut probe, text_area_rect(rect), &theme.tokens);
    probe.caret
}

struct CaretProbe<'a> {
    metrics: &'a mut dyn Painter,
    caret: Option<Rect>,
}

impl Painter for CaretProbe<'_> {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, _color: Color) {
        self.caret = Some(rect);
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
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
        self.metrics.dpi_scale()
    }
    fn measure_text_family(&mut self, text: &str, font_size: f32, family: &str) -> f32 {
        self.metrics.measure_text_family(text, font_size, family)
    }
    fn measure_text_family_styled(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.metrics
            .measure_text_family_styled(text, font_size, family, weight, italic)
    }
    fn measure_text_metrics_family_styled(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> TextMetrics {
        self.metrics
            .measure_text_metrics_family_styled(text, font_size, family, weight, italic)
    }
}
