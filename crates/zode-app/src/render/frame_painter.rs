use jian_widgets::{Color, ImageAdjustments, ImageDrawMode, Painter, Point2D, Rect, TextLayout};
use skia_safe::PaintStyle;

use super::native_backend::{skia_paint, to_sk_rect};
use super::NativeBackend;

/// Frame-scoped Jian painter. The canvas borrow cannot escape the host frame.
pub struct FramePainter<'a> {
    backend: &'a mut NativeBackend,
    canvas: &'a skia_safe::Canvas,
}

impl<'a> FramePainter<'a> {
    pub fn new(backend: &'a mut NativeBackend, canvas: &'a skia_safe::Canvas) -> Self {
        Self { backend, canvas }
    }
}

impl Painter for FramePainter<'_> {
    fn begin_frame(&mut self) {}

    fn end_frame(&mut self) {}

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.backend.fill_rect(self.canvas, rect, color);
    }

    fn stroke_rect(&mut self, rect: Rect, color: Color, width: f32) {
        self.backend.stroke_rect(self.canvas, rect, color, width);
    }

    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        self.backend.draw_text(self.canvas, layout, origin);
    }

    fn clip_rect(&mut self, rect: Rect) {
        self.canvas.clip_rect(to_sk_rect(rect), None, true);
    }

    fn clip_round_rect(&mut self, rect: Rect, radius: f32) {
        self.canvas.clip_rrect(
            skia_safe::RRect::new_rect_xy(to_sk_rect(rect), radius, radius),
            None,
            true,
        );
    }

    fn stroke_line(&mut self, from: Point2D, to: Point2D, color: Color, width: f32) {
        self.canvas.draw_line(
            (from.x, from.y),
            (to.x, to.y),
            &skia_paint(color, PaintStyle::Stroke, width),
        );
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.backend
            .fill_round_rect(self.canvas, rect, radius, color);
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: f32, color: Color, width: f32) {
        self.backend
            .stroke_round_rect(self.canvas, rect, radius, color, width);
    }

    fn stroke_svg_path(&mut self, d: &str, top_left: Point2D, size: f32, color: Color, width: f32) {
        let rect = Rect::xywh(top_left.x, top_left.y, size, size);
        self.stroke_svg_path_in_rect(d, rect, color, width);
    }

    fn fill_svg_path(&mut self, d: &str, top_left: Point2D, size: f32, viewbox: f32, color: Color) {
        let scale = if viewbox > 0.0 { size / viewbox } else { size };
        let Some(path) = skia_safe::utils::parse_path::from_svg(d) else {
            return;
        };
        let mut matrix = skia_safe::Matrix::new_identity();
        matrix.pre_translate((top_left.x, top_left.y));
        matrix.pre_scale((scale, scale), None);
        self.canvas.draw_path(
            &path.with_transform(&matrix),
            &skia_paint(color, PaintStyle::Fill, 0.0),
        );
    }

    fn fill_svg_path_in_rect(&mut self, d: &str, rect: Rect, color: Color) {
        if let Some(path) = self.backend.svg_path(d, rect) {
            self.canvas
                .draw_path(&path, &skia_paint(color, PaintStyle::Fill, 0.0));
        }
    }

    fn stroke_svg_path_in_rect(&mut self, d: &str, rect: Rect, color: Color, width: f32) {
        if let Some(path) = self.backend.svg_path(d, rect) {
            self.canvas
                .draw_path(&path, &skia_paint(color, PaintStyle::Stroke, width));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_inner_shadow_svg_path(
        &mut self,
        d: &str,
        rect: Rect,
        offset_x: f32,
        offset_y: f32,
        blur: f32,
        color: Color,
    ) {
        if let Some(path) = self.backend.svg_path(d, rect) {
            self.backend.fill_inner_shadow_svg_path(
                self.canvas,
                &path,
                Point2D::new(offset_x, offset_y),
                blur,
                color,
            );
        }
    }

    fn fill_drop_shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
        self.backend
            .fill_drop_shadow(self.canvas, rect, radius, blur, color);
    }

    fn push_blur_layer(&mut self, sigma: f32) {
        self.backend.push_blur_layer(self.canvas, sigma);
    }

    fn fill_oval(&mut self, bounds: Rect, color: Color) {
        self.canvas.draw_oval(
            to_sk_rect(bounds),
            &skia_paint(color, PaintStyle::Fill, 0.0),
        );
    }

    fn stroke_oval(&mut self, bounds: Rect, color: Color, width: f32) {
        self.canvas.draw_oval(
            to_sk_rect(bounds),
            &skia_paint(color, PaintStyle::Stroke, width),
        );
    }

    fn fill_polygon(&mut self, points: &[Point2D], color: Color) {
        if let Some(path) = polygon(points) {
            self.canvas
                .draw_path(&path, &skia_paint(color, PaintStyle::Fill, 0.0));
        }
    }

    fn stroke_polygon(&mut self, points: &[Point2D], color: Color, width: f32) {
        if let Some(path) = polygon(points) {
            self.canvas
                .draw_path(&path, &skia_paint(color, PaintStyle::Stroke, width));
        }
    }

    fn draw_image(&mut self, rect: Rect, _image_id: u64, encoded: &[u8]) {
        self.backend.draw_image(
            self.canvas,
            rect,
            encoded,
            ImageDrawMode::Fill,
            ImageAdjustments::default(),
            1.0,
            0.0,
        );
    }

    fn draw_image_with_mode(
        &mut self,
        rect: Rect,
        _image_id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
    ) {
        self.backend.draw_image(
            self.canvas,
            rect,
            encoded,
            mode,
            ImageAdjustments::default(),
            1.0,
            0.0,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_image_with_options(
        &mut self,
        rect: Rect,
        _image_id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
        adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
    ) {
        self.backend.draw_image(
            self.canvas,
            rect,
            encoded,
            mode,
            adjustments,
            opacity,
            corner_radius,
        );
    }

    fn fill_round_rect_linear_gradient(
        &mut self,
        rect: Rect,
        radius: f32,
        stops: &[(f32, Color)],
        angle_deg: f32,
        opacity: f32,
    ) {
        self.backend
            .fill_linear_gradient(self.canvas, rect, radius, stops, angle_deg, opacity);
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_round_rect_radial_gradient(
        &mut self,
        rect: Rect,
        radius: f32,
        stops: &[(f32, Color)],
        cx_frac: f32,
        cy_frac: f32,
        radius_frac: f32,
        opacity: f32,
    ) {
        self.backend.fill_radial_gradient(
            self.canvas,
            rect,
            radius,
            stops,
            cx_frac,
            cy_frac,
            radius_frac,
            opacity,
        );
    }

    fn fill_round_rect_mesh_gradient(
        &mut self,
        rect: Rect,
        radius: f32,
        rows: u32,
        cols: u32,
        colors: &[Color],
        opacity: f32,
    ) {
        self.backend
            .fill_mesh_gradient(self.canvas, rect, radius, rows, cols, colors, opacity);
    }

    fn fill_round_rect_shader(
        &mut self,
        rect: Rect,
        radius: f32,
        sksl: &str,
        uniforms: &[(&str, &[f32])],
        opacity: f32,
        fallback: Color,
    ) {
        self.backend
            .fill_shader(self.canvas, rect, radius, sksl, uniforms, opacity, fallback);
    }

    fn save(&mut self) {
        self.canvas.save();
    }

    fn restore(&mut self) {
        self.canvas.restore();
    }

    fn translate(&mut self, offset: Point2D) {
        self.canvas.translate((offset.x, offset.y));
    }

    fn scale(&mut self, scale: Point2D, pivot: Point2D) {
        self.canvas.translate((pivot.x, pivot.y));
        self.canvas.scale((scale.x, scale.y));
        self.canvas.translate((-pivot.x, -pivot.y));
    }

    fn rotate(&mut self, radians: f32, pivot: Point2D) {
        self.canvas.translate((pivot.x, pivot.y));
        self.canvas.rotate(radians.to_degrees(), None);
        self.canvas.translate((-pivot.x, -pivot.y));
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    fn dpi_scale(&self) -> f32 {
        self.backend.dpi_scale()
    }

    fn measure_text_styled(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.backend
            .measure_text(text, font_size, None, weight, italic)
    }

    fn measure_text_family_styled(
        &mut self,
        text: &str,
        font_size: f32,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.backend
            .measure_text(text, font_size, Some(family), weight, italic)
    }
}

fn polygon(points: &[Point2D]) -> Option<skia_safe::Path> {
    let (first, rest) = points.split_first()?;
    let mut path = skia_safe::Path::new();
    path.move_to((first.x, first.y));
    for point in rest {
        path.line_to((point.x, point.y));
    }
    path.close();
    Some(path)
}
