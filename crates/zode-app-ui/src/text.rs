use jian_widgets::{Color, HorizontalAlign, Painter, Rect, TextBox, VerticalAlign};

/// Semantic typography roles shared by the desktop presentation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypographyRole {
    UiDisplay,
    UiBody,
    UiLabel,
    UiCaption,
    Code,
}

/// Resolved typography values for one semantic role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypographyStyle {
    pub family: &'static str,
    pub size: f32,
    pub weight: u16,
    pub line_height: f32,
}

impl TypographyRole {
    pub const fn style(self) -> TypographyStyle {
        match self {
            Self::UiDisplay => TypographyStyle {
                family: "system-ui",
                size: 28.0,
                weight: 650,
                line_height: 36.0,
            },
            Self::UiBody => TypographyStyle {
                family: "system-ui",
                size: 14.0,
                weight: 400,
                line_height: 22.0,
            },
            Self::UiLabel => TypographyStyle {
                family: "system-ui",
                size: 13.0,
                weight: 400,
                line_height: 18.0,
            },
            Self::UiCaption => TypographyStyle {
                family: "system-ui",
                size: 11.0,
                weight: 400,
                line_height: 16.0,
            },
            Self::Code => TypographyStyle {
                family: "monospace",
                size: 12.0,
                weight: 400,
                line_height: 18.0,
            },
        }
    }
}

pub fn paint_role_single_line(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    role: TypographyRole,
    color: Color,
    horizontal_align: HorizontalAlign,
) {
    let style = role.style();
    TextBox::new(text)
        .with_font_family(style.family)
        .with_font_size(style.size)
        .with_font_weight(style.weight)
        .with_color(color)
        .with_horizontal_align(horizontal_align)
        .with_vertical_align(VerticalAlign::Center)
        .paint(painter, rect);
}

pub(crate) fn paint_single_line(
    painter: &mut dyn Painter,
    text: &str,
    rect: Rect,
    size: f32,
    weight: u16,
    color: Color,
    horizontal_align: HorizontalAlign,
) {
    TextBox::new(text)
        .with_font_family("system-ui")
        .with_font_size(size)
        .with_font_weight(weight)
        .with_color(color)
        .with_horizontal_align(horizontal_align)
        .with_vertical_align(VerticalAlign::Center)
        .paint(painter, rect);
}

/// End-anchored ellipsis truncation (unlike `environment/row.rs`'s
/// `middle_ellipsize`, which favors keeping both a path's ends visible) -
/// a task name, a result summary or a chip headline reads better with its
/// beginning intact.
pub(crate) fn ellipsize_end(
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
    for kept in (0..characters.len()).rev() {
        let mut candidate = characters[..kept].iter().collect::<String>();
        candidate.push('…');
        if painter.measure_text_weighted(&candidate, font_size, weight) <= max_width {
            return candidate;
        }
    }
    ELLIPSIS.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_roles_keep_ui_and_code_families_distinct() {
        assert_eq!(TypographyRole::Code.style().family, "monospace");
        assert_eq!(TypographyRole::UiLabel.style().family, "system-ui");
        assert_eq!(TypographyRole::UiLabel.style().weight, 400);
        for role in [
            TypographyRole::UiDisplay,
            TypographyRole::UiBody,
            TypographyRole::UiLabel,
            TypographyRole::UiCaption,
            TypographyRole::Code,
        ] {
            let style = role.style();
            assert!(style.size > 0.0);
            assert!(style.line_height >= style.size);
            assert!(style.weight > 0);
        }
    }
}
