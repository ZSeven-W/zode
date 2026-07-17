use std::sync::Arc;

use jian_core::{
    geometry::{Point, Rect as JianRect, Size},
    render::{
        BorderRadii, DrawOp, GradientStop, ImageSource, LinearGradient, MeshGradient, Paint,
        RadialGradient, ShaderSpec, ShaderUniform, ShadowSpec, StrokeOp,
    },
};
use jian_widgets::{Color, ImageAdjustments, ImageDrawMode, Point2D, Rect, TextLayout};
use skia_safe::{canvas::SaveLayerRec, image_filters, BlurStyle, MaskFilter, PaintStyle};

/// Canvas-independent renderer state kept across frames for image and shader caches.
pub struct NativeBackend {
    skia: jian_skia::SkiaBackend,
    dpi: f32,
    fonts: jian_skia::FontResolver,
}

impl NativeBackend {
    pub fn new(dpi: f32) -> Self {
        Self {
            skia: jian_skia::SkiaBackend::new(),
            dpi,
            fonts: jian_skia::FontResolver::new(skia_safe::FontMgr::new()),
        }
    }

    pub fn dpi_scale(&self) -> f32 {
        self.dpi
    }

    pub fn draw_op(&mut self, canvas: &skia_safe::Canvas, op: &DrawOp) {
        self.skia.draw_on_canvas(canvas, op);
    }

    pub fn fill_rect(&mut self, canvas: &skia_safe::Canvas, rect: Rect, color: Color) {
        self.draw_op(
            canvas,
            &DrawOp::Rect {
                rect: to_jian_rect(rect),
                paint: solid(color),
            },
        );
    }

    pub fn stroke_rect(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        color: Color,
        width: f32,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::Rect {
                rect: to_jian_rect(rect),
                paint: stroke(color, width),
            },
        );
    }

    pub fn draw_text(&mut self, canvas: &skia_safe::Canvas, layout: &TextLayout, origin: Point2D) {
        for source in layout.runs() {
            let mut run = source.clone();
            run.origin = Point::new(run.origin.x + origin.x, run.origin.y + origin.y);
            self.draw_op(canvas, &DrawOp::Text(run));
        }
    }

    pub fn draw_image(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        encoded: &[u8],
        _mode: ImageDrawMode,
        _adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
    ) {
        let clipped = corner_radius > 0.0;
        if clipped {
            canvas.save();
            canvas.clip_rrect(round_rect(rect, corner_radius), None, true);
        }
        self.draw_op(
            canvas,
            &DrawOp::Image {
                source: ImageSource::Bytes(Arc::new(encoded.to_vec())),
                dst: to_jian_rect(rect),
                opacity: opacity.clamp(0.0, 1.0),
            },
        );
        if clipped {
            canvas.restore();
        }
    }

    pub fn fill_round_rect(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        color: Color,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::RoundedRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                paint: solid(color),
            },
        );
    }

    pub fn stroke_round_rect(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        color: Color,
        width: f32,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::RoundedRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                paint: stroke(color, width),
            },
        );
    }

    pub fn fill_linear_gradient(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        stops: &[(f32, Color)],
        angle_deg: f32,
        opacity: f32,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::LinearGradientRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                gradient: LinearGradient {
                    angle_deg,
                    stops: gradient_stops(stops),
                    opacity: opacity.clamp(0.0, 1.0),
                },
                stroke: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_radial_gradient(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        stops: &[(f32, Color)],
        cx: f32,
        cy: f32,
        extent: f32,
        opacity: f32,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::RadialGradientRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                gradient: RadialGradient {
                    cx,
                    cy,
                    radius: extent,
                    stops: gradient_stops(stops),
                    opacity: opacity.clamp(0.0, 1.0),
                },
                stroke: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_mesh_gradient(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        rows: u32,
        cols: u32,
        colors: &[Color],
        opacity: f32,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::MeshGradientRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                gradient: MeshGradient {
                    rows,
                    cols,
                    colors: colors.iter().map(|color| color.to_jian()).collect(),
                    opacity: opacity.clamp(0.0, 1.0),
                },
                stroke: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill_shader(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        sksl: &str,
        uniforms: &[(&str, &[f32])],
        opacity: f32,
        fallback: Color,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::ShaderRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                shader: ShaderSpec {
                    sksl: sksl.to_owned(),
                    uniforms: uniforms
                        .iter()
                        .map(|(name, values)| ShaderUniform {
                            name: (*name).to_owned(),
                            values: values.to_vec(),
                        })
                        .collect(),
                    opacity: opacity.clamp(0.0, 1.0),
                    fallback: fallback.to_jian(),
                },
                stroke: None,
            },
        );
    }

    pub fn fill_drop_shadow(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color,
    ) {
        self.draw_op(
            canvas,
            &DrawOp::ShadowedRect {
                rect: to_jian_rect(rect),
                radii: BorderRadii::uniform(radius.max(0.0)),
                shadow: ShadowSpec {
                    color: color.to_jian(),
                    dx: 0.0,
                    dy: 2.0,
                    blur: blur.max(0.0),
                    spread: 0.0,
                },
            },
        );
    }

    pub fn svg_path(&self, d: &str, rect: Rect) -> Option<skia_safe::Path> {
        let path = skia_safe::utils::parse_path::from_svg(d)?;
        let bounds = path.compute_tight_bounds();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return None;
        }
        let mut matrix = skia_safe::Matrix::new_identity();
        matrix.set_scale_translate(
            (rect.size.x / bounds.width(), rect.size.y / bounds.height()),
            (
                rect.origin.x - bounds.left * rect.size.x / bounds.width(),
                rect.origin.y - bounds.top * rect.size.y / bounds.height(),
            ),
        );
        Some(path.with_transform(&matrix))
    }

    pub fn fill_inner_shadow_svg_path(
        &self,
        canvas: &skia_safe::Canvas,
        path: &skia_safe::Path,
        offset: Point2D,
        blur: f32,
        color: Color,
    ) {
        let mut transform = skia_safe::Matrix::new_identity();
        transform.set_translate((offset.x, offset.y));
        let shifted = path.with_transform(&transform);
        canvas.save();
        canvas.clip_path(path, skia_safe::ClipOp::Intersect, true);
        canvas.save_layer(&SaveLayerRec::default());
        let mut paint = skia_paint(color, PaintStyle::Fill, 0.0);
        canvas.draw_path(path, &paint);
        paint.set_blend_mode(skia_safe::BlendMode::DstOut);
        if blur > 0.0 {
            paint.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, blur * 0.5, false));
        }
        canvas.draw_path(&shifted, &paint);
        canvas.restore();
        canvas.restore();
    }

    pub fn push_blur_layer(&self, canvas: &skia_safe::Canvas, sigma: f32) {
        if sigma <= 0.0 {
            canvas.save();
            return;
        }
        let mut paint = skia_safe::Paint::default();
        paint.set_image_filter(image_filters::blur(
            (sigma, sigma),
            skia_safe::TileMode::Decal,
            None,
            None,
        ));
        canvas.save_layer(&SaveLayerRec::default().paint(&paint));
    }

    pub fn measure_text(
        &mut self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.fonts
            .measure_text(text, font_size, family, weight, italic)
    }
}

pub fn to_jian_rect(rect: Rect) -> JianRect {
    JianRect::new(
        Point::new(rect.origin.x, rect.origin.y),
        Size::new(rect.size.x, rect.size.y),
    )
}

pub fn to_sk_rect(rect: Rect) -> skia_safe::Rect {
    skia_safe::Rect::from_xywh(rect.origin.x, rect.origin.y, rect.size.x, rect.size.y)
}

pub fn skia_paint(color: Color, style: PaintStyle, width: f32) -> skia_safe::Paint {
    let mut paint = skia_safe::Paint::new(
        skia_safe::Color4f::new(color.r, color.g, color.b, color.a),
        None,
    );
    paint.set_anti_alias(true);
    paint.set_style(style);
    paint.set_stroke_width(width);
    paint
}

fn solid(color: Color) -> Paint {
    let mut paint = Paint::solid(color.to_jian());
    paint.opacity = color.a.clamp(0.0, 1.0);
    paint
}

fn stroke(color: Color, width: f32) -> Paint {
    Paint {
        fill: None,
        stroke: Some(StrokeOp {
            color: color.to_jian(),
            width: width.max(0.0),
        }),
        opacity: color.a.clamp(0.0, 1.0),
    }
}

fn gradient_stops(stops: &[(f32, Color)]) -> Vec<GradientStop> {
    stops
        .iter()
        .map(|(offset, color)| GradientStop {
            offset: offset.clamp(0.0, 1.0),
            color: color.to_jian(),
        })
        .collect()
}

fn round_rect(rect: Rect, radius: f32) -> skia_safe::RRect {
    skia_safe::RRect::new_rect_xy(to_sk_rect(rect), radius, radius)
}
