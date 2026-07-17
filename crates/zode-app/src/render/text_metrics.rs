use jian_core::layout::measure::{FontStyleKind, MeasureBackend, MeasureRequest, StyledRun};
use jian_widgets::TextMetrics;

#[derive(Debug)]
struct CachedTextMetrics {
    text: Box<str>,
    family: Option<Box<str>>,
    font_size_bits: u32,
    weight: u16,
    italic: bool,
    metrics: TextMetrics,
}

impl CachedTextMetrics {
    fn matches(
        &self,
        text: &str,
        family: Option<&str>,
        font_size: f32,
        weight: u16,
        italic: bool,
    ) -> bool {
        self.text.as_ref() == text
            && self.family.as_deref() == family
            && self.font_size_bits == font_size.to_bits()
            && self.weight == weight
            && self.italic == italic
    }
}

const MAX_CACHE_ENTRIES: usize = 512;

pub(super) struct TextMetricsEngine {
    fonts: jian_skia::FontResolver,
    paragraph: jian_skia::SkiaMeasure,
    cache: Vec<CachedTextMetrics>,
    font_generation: u64,
}

impl TextMetricsEngine {
    pub(super) fn new() -> Self {
        Self {
            fonts: jian_skia::FontResolver::new(skia_safe::FontMgr::new()),
            paragraph: jian_skia::SkiaMeasure::new(),
            cache: Vec::new(),
            font_generation: jian_skia::font_generation(),
        }
    }

    pub(super) fn measure_width(
        &self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> f32 {
        self.fonts
            .measure_text(text, font_size, family, weight, italic)
    }

    pub(super) fn measure(
        &mut self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> TextMetrics {
        self.refresh_cache_generation();
        if let Some(cached) = self
            .cache
            .iter()
            .find(|cached| cached.matches(text, family, font_size, weight, italic))
        {
            return cached.metrics;
        }

        let metrics = self.compute(text, font_size, family, weight, italic);
        let cached = CachedTextMetrics {
            text: text.into(),
            family: family.map(Into::into),
            font_size_bits: font_size.to_bits(),
            weight,
            italic,
            metrics,
        };
        if self.cache.len() >= MAX_CACHE_ENTRIES {
            self.cache.clear();
        }
        self.cache.push(cached);
        metrics
    }

    fn refresh_cache_generation(&mut self) {
        let generation = jian_skia::font_generation();
        if generation != self.font_generation {
            self.cache.clear();
            self.font_generation = generation;
        }
    }

    fn compute(
        &self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> TextMetrics {
        let width = self.measure_width(text, font_size, family, weight, italic);
        let runs = [StyledRun {
            text,
            font_family: family,
            font_size,
            font_weight: weight,
            font_style: if italic {
                FontStyleKind::Italic
            } else {
                FontStyleKind::Normal
            },
            letter_spacing: 0.0,
        }];
        let line = self.paragraph.measure(&MeasureRequest {
            runs: &runs,
            line_height: 0.0,
            max_width: None,
        });
        let Some((ink_top, ink_bottom)) =
            self.baseline_relative_ink_bounds(text, font_size, family, weight, italic)
        else {
            return TextMetrics {
                width,
                line_height: line.height,
                baseline: line.baseline,
                ink_top: 0.0,
                ink_bottom: line.height,
            };
        };

        TextMetrics {
            width,
            line_height: line.height,
            baseline: line.baseline,
            ink_top: line.baseline + ink_top,
            ink_bottom: line.baseline + ink_bottom,
        }
    }

    fn baseline_relative_ink_bounds(
        &self,
        text: &str,
        font_size: f32,
        family: Option<&str>,
        weight: u16,
        italic: bool,
    ) -> Option<(f32, f32)> {
        let mut ink_top = f32::INFINITY;
        let mut ink_bottom = f32::NEG_INFINITY;
        for segment in self.fonts.segment_text(text, family, weight, italic) {
            if segment.text.chars().all(char::is_whitespace) {
                continue;
            }
            let mut font = skia_safe::Font::new(&segment.typeface, font_size);
            if segment.synthetic_italic {
                font.set_skew_x(jian_skia::SYNTHETIC_ITALIC_SKEW);
            }
            let mut found_outline = false;
            for glyph in font.str_to_glyphs_vec(&segment.text) {
                let Some(path) = font.get_path(glyph) else {
                    continue;
                };
                let bounds = path.compute_tight_bounds();
                if valid_vertical_bounds(bounds) {
                    ink_top = ink_top.min(bounds.top);
                    ink_bottom = ink_bottom.max(bounds.bottom);
                    found_outline = true;
                }
            }
            if !found_outline {
                let (_, bounds) = font.measure_str(&segment.text, None);
                ink_top = ink_top.min(bounds.top);
                ink_bottom = ink_bottom.max(bounds.bottom);
            }
        }
        (ink_top.is_finite() && ink_bottom.is_finite() && ink_bottom >= ink_top)
            .then_some((ink_top, ink_bottom))
    }

    #[cfg(test)]
    pub(super) fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

fn valid_vertical_bounds(bounds: skia_safe::Rect) -> bool {
    bounds.top.is_finite()
        && bounds.bottom.is_finite()
        && bounds.bottom >= bounds.top
        && bounds.height() > 0.0
}
