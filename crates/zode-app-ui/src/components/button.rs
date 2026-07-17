//! Shared paint-level button, consuming only [`Tokens`].
//!
//! Geometry is derived from the label so a rect narrower than the label can
//! never clip it: [`Button::min_width`] is the single source of truth for
//! how much horizontal space a button needs, and [`Button::paint`] lays out
//! the icon/label using the same constants. This closes the class of bug
//! the settings provider-row buttons used to hit - a hand-picked fixed
//! width (see `widgets/settings/provider_models.rs::ROW_BUTTON_WIDTH`'s
//! history) that silently clipped long labels because nothing tied the
//! rect width to the text it had to hold.

use jian_widgets::{Color, HorizontalAlign, Painter, Point2D, Rect, Tokens};

use crate::{paint_single_line, SemanticIcon};

const ICON_X: f32 = 10.0;
const ICON_SIZE: f32 = 14.0;
const ICON_STROKE_WIDTH: f32 = 1.25;
/// Label start when an icon precedes it (icon inset + icon size + gap).
const LABEL_X_WITH_ICON: f32 = 30.0;
const LABEL_TRAILING_PAD: f32 = 8.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const LABEL_FONT_WEIGHT: u16 = 500;

/// Visual treatment. `Destructive` deliberately keeps the neutral
/// `Secondary` chrome and only tints the foreground red - the outline
/// convention already used by the settings provider row's 删除 button, not
/// a solid red fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Destructive,
    Ghost,
}

impl ButtonVariant {
    fn colors(self, tokens: &Tokens) -> (Color, Option<Color>, Color) {
        match self {
            ButtonVariant::Primary => (tokens.primary, None, tokens.primary_foreground),
            ButtonVariant::Secondary => (tokens.muted, Some(tokens.border), tokens.foreground),
            ButtonVariant::Destructive => (tokens.muted, Some(tokens.border), tokens.destructive),
            ButtonVariant::Ghost => (Color::TRANSPARENT, None, tokens.foreground),
        }
    }
}

pub struct Button;

impl Button {
    /// Minimum rect width that guarantees `label` is never clipped, given
    /// whether an icon precedes it. Callers should size the button rect to
    /// at least this instead of guessing a fixed constant.
    pub fn min_width(painter: &mut dyn Painter, label: &str, icon: Option<SemanticIcon>) -> f32 {
        let label_w = painter.measure_text_weighted(label, LABEL_FONT_SIZE, LABEL_FONT_WEIGHT);
        let label_x = if icon.is_some() {
            LABEL_X_WITH_ICON
        } else {
            ICON_X
        };
        label_x + label_w + LABEL_TRAILING_PAD
    }

    /// `radius` is caller-supplied rather than always `tokens.radius` - call
    /// sites migrating from a hand-rolled button often already committed to
    /// a specific step of the radius scale (e.g. `md` = 8.0), and silently
    /// forcing `sm` here would shrink their corners as a side effect of
    /// adopting the component.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        radius: f32,
        label: &str,
        icon: Option<SemanticIcon>,
        variant: ButtonVariant,
        disabled: bool,
        tokens: &Tokens,
    ) {
        let (fill, border, mut foreground) = variant.colors(tokens);
        if disabled {
            foreground = tokens.muted_foreground.with_alpha(0.55);
        }
        painter.fill_round_rect(rect, radius, fill);
        if let Some(border) = border {
            painter.stroke_round_rect(rect, radius, border, 1.0);
        }
        let label_x = if let Some(icon) = icon {
            painter.stroke_svg_path(
                icon.path(),
                Point2D::new(
                    rect.origin.x + ICON_X,
                    rect.origin.y + (rect.size.y - ICON_SIZE) / 2.0,
                ),
                ICON_SIZE,
                foreground,
                ICON_STROKE_WIDTH,
            );
            LABEL_X_WITH_ICON
        } else {
            ICON_X
        };
        paint_single_line(
            painter,
            label,
            Rect::xywh(
                rect.origin.x + label_x,
                rect.origin.y,
                (rect.size.x - label_x - LABEL_TRAILING_PAD).max(0.0),
                rect.size.y,
            ),
            LABEL_FONT_SIZE,
            LABEL_FONT_WEIGHT,
            foreground,
            HorizontalAlign::Start,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::test_support::RecordingPainter;

    const HEIGHT: f32 = 28.0;

    #[test]
    fn min_width_reserves_exactly_the_measured_label_width() {
        let mut painter = RecordingPainter::default();
        for label in [
            "OK",
            "编辑",
            "删除",
            "添加 Provider",
            "A very long english label",
        ] {
            let with_icon = Button::min_width(&mut painter, label, Some(SemanticIcon::Edit));
            let measured = painter.measure_text_weighted(label, LABEL_FONT_SIZE, LABEL_FONT_WEIGHT);
            let available = with_icon - LABEL_X_WITH_ICON - LABEL_TRAILING_PAD;
            assert!(
                available + 0.01 >= measured,
                "label {label:?} needs {measured}, only {available} reserved"
            );

            let without_icon = Button::min_width(&mut painter, label, None);
            let available_no_icon = without_icon - ICON_X - LABEL_TRAILING_PAD;
            assert!(available_no_icon + 0.01 >= measured);
            assert!(
                without_icon < with_icon,
                "an icon-less button should need less width than one with an icon"
            );
        }
    }

    #[test]
    fn painting_at_min_width_never_clips_the_label() {
        let mut painter = RecordingPainter::default();
        for label in [
            "OK",
            "编辑",
            "删除",
            "添加 Provider",
            "A very long english label",
        ] {
            let width = Button::min_width(&mut painter, label, Some(SemanticIcon::Edit));
            let rect = Rect::xywh(0.0, 0.0, width, HEIGHT);
            Button::paint(
                &mut painter,
                rect,
                8.0,
                label,
                Some(SemanticIcon::Edit),
                ButtonVariant::Secondary,
                false,
                &Tokens::light(),
            );
            let measured = painter.measure_text_weighted(label, LABEL_FONT_SIZE, LABEL_FONT_WEIGHT);
            let text_rect = painter.texts.last().expect("label was painted").rect;
            assert!(
                text_rect.size.x + 0.01 >= measured,
                "label {label:?} clipped: box {} < measured {measured}",
                text_rect.size.x
            );
        }
    }

    #[test]
    fn destructive_variant_tints_the_foreground_without_changing_the_chrome() {
        let tokens = Tokens::light();
        let mut painter = RecordingPainter::default();
        let rect = Rect::xywh(0.0, 0.0, 80.0, HEIGHT);
        Button::paint(
            &mut painter,
            rect,
            8.0,
            "删除",
            Some(SemanticIcon::Delete),
            ButtonVariant::Destructive,
            false,
            &tokens,
        );
        assert_eq!(painter.fills.last().unwrap().color, tokens.muted);
        assert_eq!(painter.strokes.last().unwrap().color, tokens.border);
        assert_eq!(
            painter.svg_strokes.last().unwrap().color,
            tokens.destructive
        );
    }

    #[test]
    fn disabled_state_overrides_variant_foreground() {
        let tokens = Tokens::light();
        let mut painter = RecordingPainter::default();
        let rect = Rect::xywh(0.0, 0.0, 80.0, HEIGHT);
        Button::paint(
            &mut painter,
            rect,
            8.0,
            "编辑",
            Some(SemanticIcon::Edit),
            ButtonVariant::Primary,
            true,
            &tokens,
        );
        assert_eq!(
            painter.svg_strokes.last().unwrap().color,
            tokens.muted_foreground.with_alpha(0.55)
        );
    }
}
