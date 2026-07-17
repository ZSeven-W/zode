//! Minimal `Painter` fake shared by this module's unit tests. Records the
//! calls a component made so tests can assert on geometry/colors without a
//! real rendering backend. Not exported outside `components` - integration
//! tests under `tests/` define their own richer fakes when they need one.

use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FillCall {
    pub(crate) rect: Rect,
    pub(crate) radius: f32,
    pub(crate) color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StrokeCall {
    pub(crate) rect: Rect,
    pub(crate) radius: f32,
    pub(crate) color: Color,
    pub(crate) width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgCall {
    pub(crate) top_left: Point2D,
    pub(crate) size: f32,
    pub(crate) color: Color,
    pub(crate) width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowCall {
    pub(crate) rect: Rect,
    pub(crate) radius: f32,
    pub(crate) blur: f32,
    pub(crate) color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextCall {
    pub(crate) rect: Rect,
    /// Raw run color as handed to `draw_text`; compare against
    /// `expected_color.to_jian()` rather than converting back to `Color`.
    pub(crate) color: jian_core::scene::Color,
}

#[derive(Default)]
pub(crate) struct RecordingPainter {
    pub(crate) fills: Vec<FillCall>,
    pub(crate) strokes: Vec<StrokeCall>,
    pub(crate) svg_strokes: Vec<SvgCall>,
    pub(crate) shadows: Vec<ShadowCall>,
    pub(crate) texts: Vec<TextCall>,
}

impl Painter for RecordingPainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}

    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        // `TextBox::paint` always clips right before drawing (`save`,
        // `clip_rect`, `draw_text`, `restore`), so the run's color belongs
        // to whichever clip was recorded immediately before this call.
        if let (Some(call), Some(run)) = (self.texts.last_mut(), layout.runs().first()) {
            call.color = run.color;
        }
    }

    fn clip_rect(&mut self, rect: Rect) {
        // `TextBox::paint` clips to the rect it was given before drawing, so
        // recording the clip lets label-fit tests inspect the exact box a
        // component reserved for its text.
        self.texts.push(TextCall {
            rect,
            color: jian_core::scene::Color::default(),
        });
    }

    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}

    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.fills.push(FillCall {
            rect,
            radius,
            color,
        });
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, color: Color, width: f32) {
        self.strokes.push(StrokeCall {
            rect,
            radius,
            color,
            width,
        });
    }

    fn stroke_svg_path(
        &mut self,
        _d: &str,
        top_left: Point2D,
        size: f32,
        color: Color,
        width: f32,
    ) {
        self.svg_strokes.push(SvgCall {
            top_left,
            size,
            color,
            width,
        });
    }

    fn fill_drop_shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
        self.shadows.push(ShadowCall {
            rect,
            radius,
            blur,
            color,
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
