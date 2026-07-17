use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::EnvironmentEntry;

use crate::{paint_single_line, ZodeTheme};

pub(super) const ROW_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentRowLayout {
    pub entry: EnvironmentEntry,
    pub rect: Rect,
}

pub(super) fn paint(painter: &mut dyn Painter, layout: &EnvironmentRowLayout, theme: &ZodeTheme) {
    let label_width = if layout.entry.value.is_some() {
        (layout.rect.size.x * 0.38).clamp(56.0, 104.0)
    } else {
        layout.rect.size.x
    };
    paint_single_line(
        painter,
        &layout.entry.label,
        Rect::xywh(
            layout.rect.origin.x,
            layout.rect.origin.y,
            label_width.min(layout.rect.size.x),
            layout.rect.size.y,
        ),
        12.0,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let Some(value) = layout.entry.value.as_deref() else {
        return;
    };
    let value_x = layout.rect.origin.x + label_width + 8.0;
    let value_rect = Rect::xywh(
        value_x,
        layout.rect.origin.y,
        (layout.rect.max_x() - value_x).max(0.0),
        layout.rect.size.y,
    );
    let visible_value = middle_ellipsize(painter, value, value_rect.size.x, 11.0, 400);
    paint_single_line(
        painter,
        &visible_value,
        value_rect,
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
}

fn middle_ellipsize(
    painter: &mut dyn Painter,
    value: &str,
    max_width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(value, font_size, weight) <= max_width {
        return value.to_owned();
    }
    const ELLIPSIS: &str = "…";
    if painter.measure_text_weighted(ELLIPSIS, font_size, weight) > max_width {
        return String::new();
    }
    let characters = value.chars().collect::<Vec<_>>();
    for kept in (1..characters.len()).rev() {
        let leading = kept.div_ceil(2);
        let trailing = kept / 2;
        let mut candidate = characters[..leading].iter().collect::<String>();
        candidate.push('…');
        candidate.extend(characters[characters.len() - trailing..].iter());
        if painter.measure_text_weighted(&candidate, font_size, weight) <= max_width {
            return candidate;
        }
    }
    ELLIPSIS.into()
}

trait RectMax {
    fn max_x(self) -> f32;
}

impl RectMax for Rect {
    fn max_x(self) -> f32 {
        self.origin.x + self.size.x
    }
}
