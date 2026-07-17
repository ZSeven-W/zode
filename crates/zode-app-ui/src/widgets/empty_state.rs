use jian_widgets::{Color, HorizontalAlign, Painter, Point2D, Rect, TextLayout};

use crate::{
    paint_single_line, BrandMark, ProjectPicker, RectExt, SemanticIcon, WelcomeTitleLayout,
    ZodeTheme,
};

const SUGGESTIONS: [(&str, SemanticIcon); 4] = [
    ("探索并理解代码", SemanticIcon::ExploreCode),
    ("构建新功能、应用或工具", SemanticIcon::BuildFeature),
    ("审查代码并提出修改建议", SemanticIcon::ReviewChange),
    ("修复问题和失败", SemanticIcon::FixIssue),
];

const REFERENCE_HEIGHT: f32 = 868.0;
const MARK_TOP: f32 = 348.0;
const TITLE_TOP: f32 = 428.0;
const CARDS_TOP: f32 = 493.0;
const TITLE_HEIGHT: f32 = 33.0;
const CARD_INSET: f32 = 12.0;
const CARD_GAP: f32 = 10.0;
const CARD_HEIGHT: f32 = 106.0;
const COMPACT_BREAKPOINT: f32 = 600.0;
const COMPACT_MIN_WIDTH: f32 = 160.0;
const COMPACT_MIN_HEIGHT: f32 = 180.0;
const COMPACT_INSET: f32 = 8.0;
const COMPACT_TITLE_TOP: f32 = 8.0;
const COMPACT_TITLE_HEIGHT: f32 = 28.0;
const COMPACT_TITLE_GAP: f32 = 10.0;
const WIDE_TEXT_BOTTOM_INSET: f32 = 14.0;
const WIDE_SINGLE_LINE_BLOCK_HEIGHT: f32 = 14.0;
const WIDE_TEXT_LINE_HEIGHT: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmptyStateMode {
    Wide,
    Compact,
    Omitted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EmptyStateLayout {
    mark: Rect,
    title: Rect,
    cards: [Rect; 4],
    mode: EmptyStateMode,
}

impl EmptyStateLayout {
    fn compute(rect: Rect) -> Self {
        let rect = normalized_rect(rect);
        if rect.width() < COMPACT_MIN_WIDTH || rect.height() < COMPACT_MIN_HEIGHT {
            return Self::omitted(rect);
        }
        if rect.width() < COMPACT_BREAKPOINT || rect.height() < COMPACT_BREAKPOINT {
            return Self::compact(rect);
        }

        Self::wide(rect)
    }

    fn wide(rect: Rect) -> Self {
        let mark_size = BrandMark::SIZE.min(rect.width()).min(rect.height());
        let mark = Rect::xywh(
            rect.min_x() + (rect.width() - mark_size) / 2.0,
            anchored_y(rect, MARK_TOP / REFERENCE_HEIGHT, mark_size),
            mark_size,
            mark_size,
        );

        let title_height = TITLE_HEIGHT.min(rect.height());
        let title = Rect::xywh(
            rect.min_x(),
            anchored_y(rect, TITLE_TOP / REFERENCE_HEIGHT, title_height),
            rect.width(),
            title_height,
        );

        let inset = CARD_INSET.min(rect.width() / 2.0);
        let cards_width = (rect.width() - inset * 2.0).max(0.0);
        let gap = CARD_GAP.min(cards_width / 3.0);
        let card_width = ((cards_width - gap * 3.0) / 4.0).max(0.0);
        let card_height = CARD_HEIGHT.min(rect.height());
        let card_y = anchored_y(rect, CARDS_TOP / REFERENCE_HEIGHT, card_height);
        let cards = std::array::from_fn(|index| {
            Rect::xywh(
                rect.min_x() + inset + index as f32 * (card_width + gap),
                card_y,
                card_width,
                card_height,
            )
        });

        Self {
            mark,
            title,
            cards,
            mode: EmptyStateMode::Wide,
        }
    }

    fn compact(rect: Rect) -> Self {
        let mark = Rect::xywh(rect.min_x() + rect.width() / 2.0, rect.min_y(), 0.0, 0.0);
        let title_top = COMPACT_TITLE_TOP.min(rect.height());
        let title_height = COMPACT_TITLE_HEIGHT.min((rect.height() - title_top).max(0.0));
        let title = Rect::xywh(
            rect.min_x(),
            rect.min_y() + title_top,
            rect.width(),
            title_height,
        );

        let inset = COMPACT_INSET.min(rect.width() / 2.0);
        let grid_width = (rect.width() - inset * 2.0).max(0.0);
        let card_width = ((grid_width - CARD_GAP) / 2.0).max(0.0);
        let cards_y = title.max_y() + COMPACT_TITLE_GAP;
        let cards_bottom = (rect.max_y() - inset).max(cards_y);
        let grid_height = cards_bottom - cards_y;
        let card_height = ((grid_height - CARD_GAP) / 2.0).max(0.0);
        let cards = std::array::from_fn(|index| {
            let column = index % 2;
            let row = index / 2;
            Rect::xywh(
                rect.min_x() + inset + column as f32 * (card_width + CARD_GAP),
                cards_y + row as f32 * (card_height + CARD_GAP),
                card_width,
                card_height,
            )
        });

        Self {
            mark,
            title,
            cards,
            mode: EmptyStateMode::Compact,
        }
    }

    fn omitted(rect: Rect) -> Self {
        let empty = Rect::xywh(rect.min_x(), rect.min_y(), 0.0, 0.0);
        Self {
            mark: empty,
            title: empty,
            cards: [empty; 4],
            mode: EmptyStateMode::Omitted,
        }
    }
}

pub struct EmptyState;

impl EmptyState {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
        Self::paint_with_workspace(painter, rect, None, false, false, theme);
    }

    pub fn welcome_title_layout(
        rect: Rect,
        workspace_label: Option<&str>,
    ) -> Option<WelcomeTitleLayout> {
        let layout = EmptyStateLayout::compute(rect);
        if layout.mode == EmptyStateMode::Omitted {
            return None;
        }
        let compact = layout.mode == EmptyStateMode::Compact;
        Some(ProjectPicker::welcome_title_layout(
            layout.title,
            workspace_label,
            ProjectPicker::title_font_size(compact),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_with_workspace(
        painter: &mut dyn Painter,
        rect: Rect,
        workspace_label: Option<&str>,
        project_focused: bool,
        project_hovered: bool,
        theme: &ZodeTheme,
    ) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let layout = EmptyStateLayout::compute(rect);
        if layout.mode == EmptyStateMode::Omitted {
            return;
        }

        if layout.mode == EmptyStateMode::Wide
            && layout.mark.width() >= BrandMark::SIZE
            && layout.mark.height() >= BrandMark::SIZE
        {
            BrandMark::paint(
                painter,
                Point2D::new(
                    layout.mark.min_x() + layout.mark.width() / 2.0,
                    layout.mark.min_y() + layout.mark.height() / 2.0,
                ),
                theme,
            );
        }

        if let Some(title) = Self::welcome_title_layout(rect, workspace_label) {
            paint_welcome_title(
                painter,
                title,
                workspace_label,
                project_focused,
                project_hovered,
                theme,
            );
        }

        for (index, ((text, icon), card)) in SUGGESTIONS.iter().zip(layout.cards).enumerate() {
            if card.width() <= 0.0 || card.height() <= 0.0 {
                continue;
            }
            painter.fill_round_rect(card, 12.0, theme.tokens.card);
            painter.stroke_round_rect(card, 12.0, theme.tokens.border, 1.0);
            painter.save();
            painter.clip_rect(card);
            let horizontal_inset = 14.0_f32.min(card.width() / 2.0);
            let icon_size = 18.0_f32
                .min((card.width() - horizontal_inset * 2.0).max(0.0))
                .min((card.height() - 28.0).max(0.0));
            if icon_size > 0.0 {
                painter.stroke_svg_path(
                    icon.path(),
                    Point2D::new(card.min_x() + horizontal_inset, card.min_y() + 14.0),
                    icon_size,
                    suggestion_color(index),
                    icon.stroke_width(),
                );
            }

            let (text_size, line_height) = match layout.mode {
                EmptyStateMode::Wide => (13.0, WIDE_TEXT_LINE_HEIGHT),
                EmptyStateMode::Compact => (11.0, 15.0),
                EmptyStateMode::Omitted => unreachable!(),
            };
            let text_width = (card.width() - horizontal_inset * 2.0).max(0.0);
            if let Some(lines) = split_text_lines(painter, text, text_size, text_width) {
                let text_y = match layout.mode {
                    EmptyStateMode::Wide => {
                        let block_height = if lines.len() == 1 {
                            WIDE_SINGLE_LINE_BLOCK_HEIGHT
                        } else {
                            lines.len() as f32 * line_height
                        };
                        card.max_y() - WIDE_TEXT_BOTTOM_INSET - block_height
                    }
                    EmptyStateMode::Compact => card.min_y() + 46.0,
                    EmptyStateMode::Omitted => unreachable!(),
                };
                let text_bottom = text_y + lines.len() as f32 * line_height;
                if text_bottom <= card.max_y() {
                    for (line_index, line) in lines.iter().enumerate() {
                        draw_text(
                            painter,
                            line,
                            Point2D::new(
                                card.min_x() + horizontal_inset,
                                text_y + line_index as f32 * line_height,
                            ),
                            text_size,
                            500,
                            theme.tokens.foreground,
                        );
                    }
                }
            }
            painter.restore();
        }
    }
}

fn normalized_rect(rect: Rect) -> Rect {
    let (x, width) = normalized_axis(rect.origin.x, rect.size.x);
    let (y, height) = normalized_axis(rect.origin.y, rect.size.y);
    Rect::xywh(x, y, width, height)
}

fn normalized_axis(origin: f32, size: f32) -> (f32, f32) {
    let origin = finite_or_zero(origin);
    let maximum_size = if origin > 0.0 {
        f32::MAX - origin
    } else {
        f32::MAX
    };
    let size = finite_non_negative(size).min(maximum_size);
    if (origin + size).is_finite() {
        (origin, size)
    } else {
        (origin, 0.0)
    }
}

fn anchored_y(rect: Rect, ratio: f32, height: f32) -> f32 {
    let preferred = rect.min_y() + rect.height() * ratio;
    preferred.clamp(rect.min_y(), rect.max_y() - height)
}

fn split_text_lines(
    painter: &mut dyn Painter,
    text: &str,
    font_size: f32,
    available_width: f32,
) -> Option<Vec<String>> {
    if available_width <= 0.0 {
        return None;
    }
    if painter.measure_text_weighted(text, font_size, 500) <= available_width {
        return Some(vec![text.to_owned()]);
    }

    let mut characters = text.chars().collect::<Vec<_>>();
    let candidates = (1..characters.len())
        .filter(|split| {
            let first = characters[..*split].iter().collect::<String>();
            let second = characters[*split..].iter().collect::<String>();
            painter.measure_text_weighted(&first, font_size, 500) <= available_width
                && painter.measure_text_weighted(&second, font_size, 500) <= available_width
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    let preferred = candidates
        .iter()
        .copied()
        .filter(|split| matches!(characters[split - 1], '、' | '，' | '。'))
        .min_by_key(|split| split.abs_diff(characters.len() / 2))
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .min_by_key(|split| split.abs_diff(characters.len() / 2))
        })?;
    let second = characters.split_off(preferred).into_iter().collect();
    let first = characters.into_iter().collect();
    Some(vec![first, second])
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn finite_non_negative(value: f32) -> f32 {
    finite_or_zero(value).max(0.0)
}

fn suggestion_color(index: usize) -> Color {
    match index {
        0 => Color::rgb_u8(59, 130, 246),
        1 => Color::rgb_u8(139, 92, 246),
        2 => Color::rgb_u8(34, 197, 94),
        _ => Color::rgb_u8(249, 115, 22),
    }
}

fn paint_welcome_title(
    painter: &mut dyn Painter,
    layout: WelcomeTitleLayout,
    workspace_label: Option<&str>,
    project_focused: bool,
    project_hovered: bool,
    theme: &ZodeTheme,
) {
    let Some(project) = workspace_label.filter(|label| !label.trim().is_empty()) else {
        paint_single_line(
            painter,
            "我们应该构建什么？",
            layout.prefix,
            layout.font_size,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        return;
    };
    paint_single_line(
        painter,
        "我们应该在 ",
        layout.prefix,
        layout.font_size,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    if let Some(project_rect) = layout.project {
        paint_single_line(
            painter,
            project,
            project_rect,
            layout.font_size,
            500,
            if project_focused || project_hovered {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Start,
        );
        let underline_y = project_rect.max_y() - 3.0;
        painter.stroke_line(
            Point2D::new(project_rect.min_x(), underline_y),
            Point2D::new(project_rect.max_x(), underline_y),
            theme.tokens.muted_foreground,
            if project_focused { 1.5 } else { 1.0 },
        );
        if project_focused {
            painter.stroke_round_rect(project_rect, 3.0, theme.tokens.ring, 1.5);
        }
    }
    paint_single_line(
        painter,
        " 中构建什么？",
        layout.suffix,
        layout.font_size,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32, tolerance: f32, label: &str) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{label}: expected {expected} ± {tolerance}, got {actual}"
        );
    }

    fn assert_contained(child: Rect, parent: Rect, label: &str) {
        for value in [
            child.min_x(),
            child.min_y(),
            child.width(),
            child.height(),
            child.max_x(),
            child.max_y(),
        ] {
            assert!(value.is_finite(), "{label} must stay finite");
        }
        assert!(child.min_x() >= parent.min_x(), "{label}.min_x");
        assert!(child.min_y() >= parent.min_y(), "{label}.min_y");
        assert!(child.max_x() <= parent.max_x(), "{label}.max_x");
        assert!(child.max_y() <= parent.max_y(), "{label}.max_y");
    }

    fn overlaps(left: Rect, right: Rect) -> bool {
        left.width() > 0.0
            && left.height() > 0.0
            && right.width() > 0.0
            && right.height() > 0.0
            && left.min_x() < right.max_x()
            && left.max_x() > right.min_x()
            && left.min_y() < right.max_y()
            && left.max_y() > right.min_y()
    }

    #[test]
    fn wide_layout_preserves_the_reference_geometry() {
        let layout = EmptyStateLayout::compute(Rect::xywh(652.0, 70.0, 736.0, 868.0));

        assert_close(layout.mark.min_x(), 996.0, 2.0, "mark x");
        assert_close(layout.mark.min_y(), 418.0, 2.0, "mark y");
        assert_close(layout.title.min_y(), 498.0, 2.0, "title y");
        for (index, card) in layout.cards.iter().enumerate() {
            assert_close(card.min_x(), 664.0 + index as f32 * 180.5, 2.0, "card x");
            assert_close(card.min_y(), 563.0, 2.0, "card y");
            assert_close(card.width(), 170.5, 2.0, "card width");
            assert_close(card.height(), 106.0, 2.0, "card height");
        }
    }

    #[test]
    fn compact_layout_uses_a_contained_non_overlapping_two_by_two_grid() {
        let bounds = Rect::xywh(10.0, 20.0, 320.0, 268.0);
        let layout = EmptyStateLayout::compute(bounds);

        assert_eq!(layout.mark.width(), 0.0, "compact layout hides the mark");
        assert_close(layout.title.min_y(), 28.0, 0.1, "compact title top");
        for (index, card) in layout.cards.iter().enumerate() {
            assert_contained(*card, bounds, &format!("card {index}"));
        }
        assert_close(
            layout.cards[1].min_x() - layout.cards[0].max_x(),
            10.0,
            0.1,
            "column gap",
        );
        assert_close(
            layout.cards[2].min_y() - layout.cards[0].max_y(),
            10.0,
            0.1,
            "row gap",
        );
        assert_close(
            layout.cards[0].min_y(),
            layout.cards[1].min_y(),
            0.1,
            "row 1",
        );
        assert_close(
            layout.cards[2].min_y(),
            layout.cards[3].min_y(),
            0.1,
            "row 2",
        );
        assert!(!overlaps(layout.mark, layout.title));
        assert!(!overlaps(layout.title, layout.cards[0]));
        for left in 0..layout.cards.len() {
            for right in (left + 1)..layout.cards.len() {
                assert!(!overlaps(layout.cards[left], layout.cards[right]));
            }
        }
    }

    #[test]
    fn tiny_layout_omits_content_and_stays_contained() {
        let bounds = Rect::xywh(4.0, 6.0, 48.0, 40.0);
        let layout = EmptyStateLayout::compute(bounds);

        for (index, child) in std::iter::once(layout.mark)
            .chain(std::iter::once(layout.title))
            .chain(layout.cards)
            .enumerate()
        {
            assert_contained(child, bounds, &format!("child {index}"));
            assert_eq!(child.width() * child.height(), 0.0);
        }
    }

    #[test]
    fn normalization_caps_dimensions_before_right_and_bottom_overflow() {
        for input in [
            Rect::xywh(f32::MAX, f32::MAX, f32::MAX, f32::MAX),
            Rect::xywh(f32::MAX / 2.0, f32::MAX / 2.0, f32::MAX, f32::MAX),
        ] {
            let normalized = normalized_rect(input);
            assert!(normalized.max_x().is_finite());
            assert!(normalized.max_y().is_finite());
            let layout = EmptyStateLayout::compute(input);
            for child in std::iter::once(layout.mark)
                .chain(std::iter::once(layout.title))
                .chain(layout.cards)
            {
                assert!(child.max_x().is_finite());
                assert!(child.max_y().is_finite());
            }
        }
    }
}
