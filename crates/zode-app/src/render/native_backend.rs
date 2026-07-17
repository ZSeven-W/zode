use std::{collections::HashMap, sync::Arc};

use jian_core::{
    geometry::{Point, Rect as JianRect, Size},
    render::{
        BorderRadii, DrawOp, GradientStop, ImageSource, LinearGradient, MeshGradient, Paint,
        RadialGradient, ShaderSpec, ShaderUniform, ShadowSpec, StrokeOp, TextRun,
    },
};
use jian_widgets::{
    Color, ImageAdjustments, ImageDrawMode, Point2D, Rect, TextLayout, TextMetrics,
};
use skia_safe::{canvas::SaveLayerRec, image_filters, BlurStyle, MaskFilter, PaintStyle};

use super::text_metrics::TextMetricsEngine;

#[path = "text_picture_cache.rs"]
mod text_picture_cache;

use text_picture_cache::TextPictureCache;

const TEXT_PICTURE_CULL_EXTENT: f32 = 1_048_576.0;

/// Upper bound on cached parsed SVG icon paths before the cache is cleared.
/// Icon sets are small (well under a hundred distinct `d` strings per app),
/// so this is a generous ceiling that only exists to bound memory if callers
/// ever feed pathologically many distinct path strings through; re-parsing
/// on the next call is always a safe fallback.
const SVG_PATH_CACHE_CAP: usize = 256;

/// Upper bound on distinct `image_id`s cached before least-recently-used
/// eviction kicks in. Static assets (icons, logos) reuse one id forever and
/// stay hot under this cap. A live frame stream (the browser spectator
/// panel - see `browser_panel`) mints a fresh id per decoded frame, since
/// re-using one id would leave the (content-addressed) skia image cache
/// permanently stuck on the first frame; without a cap that would grow this
/// map's raw-byte entries without bound over a long streaming session. Each
/// entry only holds encoded bytes (the decoded bitmap itself is bounded
/// separately by jian-skia's own 128 MB `ImageCache`), so the cap just
/// needs to comfortably exceed the app's static-asset count, which is a
/// small constant.
const IMAGE_SOURCE_CACHE_CAP: usize = 256;

struct CachedImageSource {
    key: Arc<str>,
    bytes: Arc<Vec<u8>>,
    /// Monotonically increasing access tick - bumped on every lookup (hit
    /// or insert) and used to find the least-recently-used entry once the
    /// cache is over `IMAGE_SOURCE_CACHE_CAP`. Mirrors jian-skia's own
    /// `ImageCache` eviction strategy (see `jian-skia/src/image.rs`).
    last_used: u64,
}

impl CachedImageSource {
    fn new(image_id: u64, encoded: &[u8], last_used: u64) -> Self {
        Self {
            key: Arc::from(format!("zode-image:{image_id:016x}")),
            bytes: Arc::new(encoded.to_vec()),
            last_used,
        }
    }

    fn to_image_source(&self) -> ImageSource {
        ImageSource::KeyedBytes {
            key: Arc::clone(&self.key),
            bytes: Arc::clone(&self.bytes),
        }
    }
}

/// Canvas-independent renderer state kept across frames for image and shader caches.
pub struct NativeBackend {
    skia: jian_skia::SkiaBackend,
    image_sources: HashMap<u64, CachedImageSource>,
    /// Next access tick handed to `CachedImageSource::last_used`; see
    /// `IMAGE_SOURCE_CACHE_CAP`.
    image_tick: u64,
    dpi: f32,
    text_metrics: TextMetricsEngine,
    text_pictures: TextPictureCache,
    /// Parsed (untransformed) icon paths keyed by their raw SVG `d` string.
    /// `skia_safe::utils::parse_path::from_svg` is a string parse that ran
    /// on every icon draw every frame; the per-call rect/viewbox only affect
    /// a cheap `with_transform` applied after cache retrieval, so caching
    /// just the parsed path (shared by both `svg_path` and
    /// `svg_path_with_viewbox`) captures effectively all of the savings.
    svg_paths: HashMap<String, skia_safe::Path>,
    font_family_override: Option<String>,
}

impl NativeBackend {
    pub fn new(dpi: f32) -> Self {
        Self::with_font_family(dpi, None)
    }

    pub(crate) fn with_font_family(dpi: f32, font_family_override: Option<String>) -> Self {
        Self {
            skia: jian_skia::SkiaBackend::new(),
            image_sources: HashMap::new(),
            image_tick: 0,
            dpi,
            text_metrics: TextMetricsEngine::new(),
            text_pictures: TextPictureCache::new(),
            svg_paths: HashMap::new(),
            font_family_override,
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
            let run = self.prepare_text_run(source, origin);
            self.draw_text_run(canvas, &run);
        }
    }

    fn draw_text_run(&mut self, canvas: &skia_safe::Canvas, run: &TextRun) {
        let (picture, recorded_x, recorded_y) =
            if let Some(cached) = self.text_pictures.get(run, self.dpi) {
                cached
            } else {
                let mut recorder = skia_safe::PictureRecorder::new();
                let cull = skia_safe::Rect::from_xywh(
                    -TEXT_PICTURE_CULL_EXTENT,
                    -TEXT_PICTURE_CULL_EXTENT,
                    TEXT_PICTURE_CULL_EXTENT * 2.0,
                    TEXT_PICTURE_CULL_EXTENT * 2.0,
                );
                let recording_canvas = recorder.begin_recording(cull, false);
                self.skia
                    .draw_on_canvas(recording_canvas, &DrawOp::Text(run.clone()));
                let Some(picture) = recorder.finish_recording_as_picture(None) else {
                    self.draw_op(canvas, &DrawOp::Text(run.clone()));
                    return;
                };
                self.text_pictures.insert(run, self.dpi, picture.clone());
                (picture, run.origin.x, run.origin.y)
            };

        let offset_x = run.origin.x - recorded_x;
        let offset_y = run.origin.y - recorded_y;
        if offset_x == 0.0 && offset_y == 0.0 {
            canvas.draw_picture(&picture, None, None);
        } else {
            let matrix = skia_safe::Matrix::translate((offset_x, offset_y));
            canvas.draw_picture(&picture, Some(&matrix), None);
        }
    }

    fn prepare_text_run(&self, source: &TextRun, origin: Point2D) -> TextRun {
        let mut run = source.clone();
        if let Some(family) = self.font_family_override.as_deref() {
            run.font_family = family.to_string();
        }
        run.origin = Point::new(run.origin.x + origin.x, run.origin.y + origin.y);
        run
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        image_id: u64,
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
        let source = self.image_source(image_id, encoded);
        self.draw_op(
            canvas,
            &DrawOp::Image {
                source,
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

    pub fn svg_path(&mut self, d: &str, rect: Rect) -> Option<skia_safe::Path> {
        let path = self.parsed_svg_path(d)?;
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

    pub fn svg_path_with_viewbox(
        &mut self,
        d: &str,
        rect: Rect,
        viewbox: f32,
    ) -> Option<skia_safe::Path> {
        if viewbox <= 0.0 || rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return None;
        }
        let path = self.parsed_svg_path(d)?;
        let scale = (rect.size.x / viewbox).min(rect.size.y / viewbox);
        let scaled_viewbox = viewbox * scale;
        let mut matrix = skia_safe::Matrix::new_identity();
        matrix.set_scale_translate(
            (scale, scale),
            (
                rect.origin.x + (rect.size.x - scaled_viewbox) / 2.0,
                rect.origin.y + (rect.size.y - scaled_viewbox) / 2.0,
            ),
        );
        Some(path.with_transform(&matrix))
    }

    /// Returns the parsed, untransformed path for `d`, reusing a cached
    /// parse when available. Cloning a `skia_safe::Path` is cheap (it is
    /// ref-counted internally), so cache hits avoid the string parse
    /// entirely while still letting each caller apply its own transform.
    fn parsed_svg_path(&mut self, d: &str) -> Option<skia_safe::Path> {
        if let Some(cached) = self.svg_paths.get(d) {
            return Some(cached.clone());
        }
        let path = skia_safe::utils::parse_path::from_svg(d)?;
        if self.svg_paths.len() >= SVG_PATH_CACHE_CAP {
            self.svg_paths.clear();
        }
        self.svg_paths.insert(d.to_owned(), path.clone());
        Some(path)
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
        let family = self.font_family_for_measure(family);
        self.text_metrics
            .measure_width(text, font_size, family, weight, italic)
    }

    pub fn measure_text_metrics(
        &mut self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> TextMetrics {
        let family = self.font_family_override.as_deref().or(family);
        self.text_metrics
            .measure(text, font_size, family, weight, italic)
    }

    fn font_family_for_measure<'a>(&'a self, requested: Option<&'a str>) -> Option<&'a str> {
        self.font_family_override.as_deref().or(requested)
    }

    /// Resolve an encoded source by the stable caller-provided image ID.
    ///
    /// The first ID-to-bytes binding wins. Later lookups clone the cached Arc
    /// without reading or comparing `encoded`, so every cache hit stays O(1).
    fn image_source(&mut self, image_id: u64, encoded: &[u8]) -> ImageSource {
        self.image_tick = self.image_tick.wrapping_add(1);
        let tick = self.image_tick;
        match self.image_sources.entry(image_id) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let cached = entry.into_mut();
                cached.last_used = tick;
                cached.to_image_source()
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let source = entry
                    .insert(CachedImageSource::new(image_id, encoded, tick))
                    .to_image_source();
                self.evict_image_sources_if_over_cap();
                source
            }
        }
    }

    /// Drops the least-recently-used `image_sources` entries once the cache
    /// exceeds `IMAGE_SOURCE_CACHE_CAP`. A static asset that gets evicted
    /// this way isn't lost - its caller re-supplies the same `encoded`
    /// bytes on its next draw, which simply re-inserts it - so this only
    /// trades a little decode/alloc churn under sustained pressure (a live
    /// frame stream) for a bounded memory footprint.
    fn evict_image_sources_if_over_cap(&mut self) {
        while self.image_sources.len() > IMAGE_SOURCE_CACHE_CAP {
            let Some(victim) = self
                .image_sources
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            self.image_sources.remove(&victim);
        }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jian_core::render::{DrawOp, ImageSource};
    use jian_widgets::{Color, Point2D, Rect, TextLayout};

    use crate::render::RasterSurface;

    use super::NativeBackend;

    const SNAPSHOT_REGULAR: &[u8] =
        include_bytes!("../../tests/fonts/NotoSansSC-Regular.subset.ttf");
    const SNAPSHOT_SEMIBOLD: &[u8] =
        include_bytes!("../../tests/fonts/NotoSansSC-SemiBold.subset.ttf");

    #[test]
    fn svg_path_with_viewbox_preserves_icon_aspect_ratio_and_padding() {
        const FOLDER_ICON: &str = "M3 5H10L12 7H21V19H3Z";

        let mut backend = NativeBackend::new(1.0);
        let target = Rect::xywh(10.0, 20.0, 16.0, 16.0);
        let path = backend
            .svg_path_with_viewbox(FOLDER_ICON, target, 24.0)
            .expect("folder icon should parse");
        let bounds = path.compute_tight_bounds();

        assert!((bounds.width() / bounds.height() - 18.0 / 14.0).abs() < 0.001);
        assert!(bounds.width() < target.size.x);
        assert!(bounds.height() < target.size.y);
        assert!(bounds.left >= target.origin.x);
        assert!(bounds.top >= target.origin.y);
        assert!(bounds.right <= target.origin.x + target.size.x);
        assert!(bounds.bottom <= target.origin.y + target.size.y);
        assert!((bounds.center_x() - (target.origin.x + target.size.x / 2.0)).abs() < 0.001);
        assert!((bounds.center_y() - (target.origin.y + target.size.y / 2.0)).abs() < 0.001);
    }

    #[test]
    fn repeated_svg_path_reuses_the_cached_parse_across_different_rects() {
        const FOLDER_ICON: &str = "M3 5H10L12 7H21V19H3Z";
        let mut backend = NativeBackend::new(1.0);

        let first = backend
            .svg_path(FOLDER_ICON, Rect::xywh(0.0, 0.0, 16.0, 16.0))
            .expect("folder icon should parse");
        assert_eq!(backend.svg_paths.len(), 1);

        let second = backend
            .svg_path(FOLDER_ICON, Rect::xywh(40.0, 12.0, 20.0, 20.0))
            .expect("cached parse should still transform for a different rect");
        assert_eq!(
            backend.svg_paths.len(),
            1,
            "second call with the same `d` must reuse the cached parse"
        );

        // Different target rects must still produce independently transformed paths.
        assert_ne!(first.compute_tight_bounds(), second.compute_tight_bounds());
    }

    #[test]
    fn svg_path_and_viewbox_variant_share_the_same_cache_entry() {
        const ICON: &str = "M3 5H10L12 7H21V19H3Z";
        let mut backend = NativeBackend::new(1.0);
        let rect = Rect::xywh(0.0, 0.0, 24.0, 24.0);

        backend
            .svg_path(ICON, rect)
            .expect("plain path should parse");
        assert_eq!(backend.svg_paths.len(), 1);

        backend
            .svg_path_with_viewbox(ICON, rect, 24.0)
            .expect("viewbox path should parse");
        assert_eq!(
            backend.svg_paths.len(),
            1,
            "the viewbox variant should hit the same cache entry as `svg_path`"
        );
    }

    #[test]
    fn svg_path_cache_clears_once_over_capacity() {
        let mut backend = NativeBackend::new(1.0);
        let rect = Rect::xywh(0.0, 0.0, 16.0, 16.0);

        for i in 0..super::SVG_PATH_CACHE_CAP {
            let d = format!("M0 0H10V{}H0Z", 15 + i);
            assert!(
                backend.svg_path(&d, rect).is_some(),
                "distinct rectangle path should parse"
            );
        }
        assert_eq!(backend.svg_paths.len(), super::SVG_PATH_CACHE_CAP);

        assert!(backend.svg_path("M0 0H10V1000H0Z", rect).is_some());
        assert_eq!(
            backend.svg_paths.len(),
            1,
            "cache should clear before inserting past capacity"
        );
    }

    #[test]
    fn repeated_image_id_reuses_the_same_encoded_bytes() {
        let mut backend = NativeBackend::new(1.0);

        let first = backend.image_source(7, b"full repository png");
        let second = backend.image_source(7, b"full repository png");
        let (
            ImageSource::KeyedBytes {
                key: first_key,
                bytes: first_bytes,
            },
            ImageSource::KeyedBytes {
                key: second_key,
                bytes: second_bytes,
            },
        ) = (first, second)
        else {
            panic!("native images should use keyed encoded bytes");
        };

        assert_eq!(first_key.as_ref(), "zode-image:0000000000000007");
        assert!(Arc::ptr_eq(&first_key, &second_key));
        assert!(Arc::ptr_eq(&first_bytes, &second_bytes));
    }

    #[test]
    fn image_source_cache_stays_bounded_under_sustained_fresh_ids() {
        // Simulates a live frame stream (the browser spectator panel): a
        // fresh `image_id` per decoded frame, well past the cache cap.
        let mut backend = NativeBackend::new(1.0);
        for id in 0..(super::IMAGE_SOURCE_CACHE_CAP as u64 + 50) {
            backend.image_source(id, b"frame bytes");
        }
        assert_eq!(backend.image_sources.len(), super::IMAGE_SOURCE_CACHE_CAP);
    }

    #[test]
    fn image_source_cache_evicts_the_least_recently_used_entry_first() {
        let mut backend = NativeBackend::new(1.0);
        for id in 0..super::IMAGE_SOURCE_CACHE_CAP as u64 {
            backend.image_source(id, b"static asset bytes");
        }
        assert_eq!(backend.image_sources.len(), super::IMAGE_SOURCE_CACHE_CAP);

        // id 1 is untouched since insertion; id 0 is "hot" (re-drawn every
        // tick, like a chrome icon) and gets touched again right before the
        // cache tips over capacity.
        backend.image_source(0, b"static asset bytes");
        backend.image_source(super::IMAGE_SOURCE_CACHE_CAP as u64, b"new entry");

        assert!(
            backend
                .image_sources
                .contains_key(&(super::IMAGE_SOURCE_CACHE_CAP as u64)),
            "the newly inserted entry must survive its own insertion"
        );
        assert!(
            backend.image_sources.contains_key(&0),
            "a recently re-touched entry must not be evicted"
        );
        assert!(
            !backend.image_sources.contains_key(&1),
            "the least-recently-used entry should be evicted, not an arbitrary one"
        );
    }

    #[test]
    fn evicted_static_asset_id_is_transparently_recreated_on_next_draw() {
        // An evicted static asset isn't lost - the caller always re-supplies
        // its own encoded bytes on the next draw, so eviction only costs a
        // re-insert, never stale/missing content.
        let mut backend = NativeBackend::new(1.0);
        for id in 0..(super::IMAGE_SOURCE_CACHE_CAP as u64 + 1) {
            backend.image_source(id, b"static asset bytes");
        }
        assert!(!backend.image_sources.contains_key(&0), "id 0 was evicted");

        let ImageSource::KeyedBytes { key, .. } = backend.image_source(0, b"static asset bytes")
        else {
            panic!("native images should use keyed encoded bytes");
        };
        assert_eq!(key.as_ref(), "zode-image:0000000000000000");
    }

    #[test]
    fn repeated_image_id_keeps_the_first_encoded_bytes() {
        let mut backend = NativeBackend::new(1.0);

        let first = backend.image_source(7, b"first png");
        let second = backend.image_source(7, b"different png");
        let (
            ImageSource::KeyedBytes {
                key: first_key,
                bytes: first_bytes,
            },
            ImageSource::KeyedBytes {
                key: second_key,
                bytes: second_bytes,
            },
        ) = (first, second)
        else {
            panic!("native images should use keyed encoded bytes");
        };

        assert!(Arc::ptr_eq(&first_key, &second_key));
        assert!(Arc::ptr_eq(&first_bytes, &second_bytes));
        assert_eq!(second_bytes.as_slice(), b"first png");
    }

    #[test]
    fn injected_family_overrides_paint_and_measure_requests() {
        jian_skia::register_bundled_fonts(vec![
            SNAPSHOT_REGULAR.to_vec(),
            SNAPSHOT_SEMIBOLD.to_vec(),
        ]);
        let mut backend =
            NativeBackend::with_font_family(1.0, Some("Zode Snapshot Sans SC".to_string()));
        let mut explicit = NativeBackend::new(1.0);
        let layout = TextLayout::single_run(
            "snapshot",
            "system-ui",
            14.0,
            Color::BLACK.to_jian(),
            Point2D::new(2.0, 3.0),
        );

        let painted = backend.prepare_text_run(&layout.runs()[0], Point2D::new(5.0, 7.0));
        let measured_with_override =
            backend.measure_text("snapshot", 14.0, Some("system-ui"), 400, false);
        let measured_explicitly =
            explicit.measure_text("snapshot", 14.0, Some("Zode Snapshot Sans SC"), 400, false);

        assert_eq!(painted.font_family, "Zode Snapshot Sans SC");
        assert_eq!(painted.origin.x, 7.0);
        assert_eq!(painted.origin.y, 10.0);
        assert!(measured_with_override > 0.0);
        assert!((measured_with_override - measured_explicitly).abs() < f32::EPSILON);
        assert_eq!(
            backend.font_family_for_measure(Some("ui-monospace")),
            Some("Zode Snapshot Sans SC")
        );
        assert_eq!(
            backend.font_family_for_measure(None),
            Some("Zode Snapshot Sans SC")
        );
    }

    #[test]
    fn default_backend_preserves_requested_system_family() {
        let backend = NativeBackend::new(1.0);
        let layout = TextLayout::single_run(
            "runtime",
            "system-ui",
            14.0,
            Color::BLACK.to_jian(),
            Point2D::new(0.0, 0.0),
        );

        let painted = backend.prepare_text_run(&layout.runs()[0], Point2D::new(0.0, 0.0));

        assert_eq!(painted.font_family, "system-ui");
        assert_eq!(
            backend.font_family_for_measure(Some("system-ui")),
            Some("system-ui")
        );
        assert_eq!(backend.font_family_for_measure(None), None);
    }

    #[test]
    fn repeated_text_metrics_are_cached_per_style() {
        let mut backend = NativeBackend::new(1.0);

        let first =
            backend.measure_text_metrics("cached control", 13.0, Some("system-ui"), 400, false);
        let second =
            backend.measure_text_metrics("cached control", 13.0, Some("system-ui"), 400, false);

        assert_eq!(first, second);
        assert_eq!(backend.text_metrics.cache_len(), 1);

        backend.measure_text_metrics("cached control", 14.0, Some("system-ui"), 400, false);
        assert_eq!(backend.text_metrics.cache_len(), 2);
    }

    #[test]
    fn cached_text_picture_matches_direct_jian_pixels() {
        jian_skia::with_font_lock(assert_cached_text_picture_matches_direct_jian_pixels);
    }

    fn assert_cached_text_picture_matches_direct_jian_pixels() {
        const DPI: f32 = 1.5;
        const WIDTH: u32 = 480;
        const HEIGHT: u32 = 144;
        let layout = TextLayout::single_run(
            "Cached text 文字",
            "system-ui",
            17.0,
            Color::BLACK.to_jian(),
            Point2D::new(3.25, 2.5),
        )
        .with_font_weight(600);
        let origin = Point2D::new(21.5, 19.25);
        // Two logical pixels are three whole device pixels at 1.5x. The
        // relocated draw must therefore reuse the same cached phase.
        let moved_origin = Point2D::new(origin.x + 2.0, origin.y + 2.0);
        let mut cached_backend = NativeBackend::new(DPI);
        let direct_run = cached_backend.prepare_text_run(&layout.runs()[0], origin);
        let moved_direct_run = cached_backend.prepare_text_run(&layout.runs()[0], moved_origin);

        let mut direct = RasterSurface::new(WIDTH, HEIGHT).expect("direct surface");
        direct.canvas().clear(skia_safe::Color::WHITE);
        direct.canvas().scale((DPI, DPI));
        let mut jian = jian_skia::SkiaBackend::new();
        jian.draw_on_canvas(direct.canvas(), &DrawOp::Text(direct_run));

        let mut moved_direct = RasterSurface::new(WIDTH, HEIGHT).expect("moved direct surface");
        moved_direct.canvas().clear(skia_safe::Color::WHITE);
        moved_direct.canvas().scale((DPI, DPI));
        jian.draw_on_canvas(moved_direct.canvas(), &DrawOp::Text(moved_direct_run));

        let mut first = RasterSurface::new(WIDTH, HEIGHT).expect("first cached surface");
        first.canvas().clear(skia_safe::Color::WHITE);
        first.canvas().scale((DPI, DPI));
        cached_backend.draw_text(first.canvas(), &layout, origin);

        let mut second = RasterSurface::new(WIDTH, HEIGHT).expect("hit surface");
        second.canvas().clear(skia_safe::Color::WHITE);
        second.canvas().scale((DPI, DPI));
        cached_backend.draw_text(second.canvas(), &layout, moved_origin);

        let mut direct_pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
        let mut moved_direct_pixels = vec![0; direct_pixels.len()];
        let mut first_pixels = vec![0; direct_pixels.len()];
        let mut second_pixels = vec![0; direct_pixels.len()];
        assert!(direct.read_rgba8(&mut direct_pixels));
        assert!(moved_direct.read_rgba8(&mut moved_direct_pixels));
        assert!(first.read_rgba8(&mut first_pixels));
        assert!(second.read_rgba8(&mut second_pixels));
        assert_eq!(
            first_pixels, direct_pixels,
            "cache miss changed text pixels"
        );
        assert_eq!(
            second_pixels, moved_direct_pixels,
            "cache hit changed text pixels"
        );
        assert_eq!(cached_backend.text_pictures.stats(), (1, 1, 0, 1));
    }
}
