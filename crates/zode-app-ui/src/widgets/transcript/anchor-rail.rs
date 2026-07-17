//! Conversation anchor rail: a thin vertical strip of tick marks to the left
//! of the transcript, one per turn, that highlights which turn is on screen
//! and jumps to a turn on click. See
//! `docs/proposals/conversation-anchor-rail.md` for the design.
//!
//! Geometry is derived at paint time from the transcript content rect and
//! the outer primary-surface rect - no `layout.rs` field exists for this
//! widget, matching the design doc's "derive geometry from existing rects"
//! constraint. Anchor derivation itself (excerpts, files, y-offsets) is
//! memoized in `zode-app-model` (`TranscriptState::anchors`); this module
//! only positions, paints and hit-tests the resulting list.

use std::collections::BTreeMap;

use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, ConversationAnchor, TranscriptState, ZodeAppState};
use zode_node_protocol::SessionLocator;

use crate::{
    paint_elevated_surface, paint_single_line, stable_widget_id, Pill, RectExt, WidgetId, ZodeTheme,
};

use super::ThreadTranscript;

/// Gap between the rail and the transcript content column, and between the
/// rail and the outer primary-surface edge.
const RAIL_GAP: f32 = 12.0;
/// Width of the tick-mark strip itself.
const RAIL_TICK_WIDTH: f32 = 16.0;
/// The rail hides entirely once the left gutter (`content_rect.min_x() -
/// main_rect.min_x()`) drops below this - not enough room for the rail plus
/// its two gaps.
pub const HIDE_THRESHOLD: f32 = RAIL_TICK_WIDTH + 2.0 * RAIL_GAP;

const MIN_TICK_SPACING: f32 = 6.0;
const TICK_HEIGHT: f32 = 2.0;
const TICK_HEIGHT_HOVERED: f32 = 3.0;
const TICK_WIDTH_HOVERED: f32 = RAIL_TICK_WIDTH + 4.0;
/// Enlarged hit/hover target height around each tick's visual center.
const HOVER_HIT_HEIGHT: f32 = 12.0;

const CARD_WIDTH: f32 = 260.0;
const CARD_PAD: f32 = 12.0;
const CARD_GAP: f32 = 8.0;
const CARD_RADIUS: f32 = 10.0;
const CARD_TITLE_SIZE: f32 = 13.0;
const CARD_BODY_SIZE: f32 = 12.0;
const CARD_LINE_HEIGHT: f32 = 17.0;
const CARD_MAX_REPLY_LINES: usize = 3;
const CARD_ROW_GAP: f32 = 6.0;
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_GAP: f32 = 6.0;
const MAX_CHIPS: usize = 3;

/// One positioned, hit-testable anchor: the derived data plus everything
/// paint, hit-testing and the accessibility tree need to agree on.
#[derive(Debug, Clone)]
pub struct AnchorTick {
    pub anchor: ConversationAnchor,
    pub widget_id: WidgetId,
    /// Thin rect actually painted as the tick mark.
    pub tick_rect: Rect,
    /// Enlarged rect used for hover/click hit-testing and the a11y node.
    pub hit_rect: Rect,
    /// Whether this turn's range intersects the currently visible viewport.
    pub active: bool,
}

pub struct AnchorRail;

impl AnchorRail {
    /// Whether the rail has room to paint at all: the gutter between the
    /// outer primary surface and the transcript content column must be at
    /// least `HIDE_THRESHOLD` wide.
    pub fn visible(content_rect: Rect, main_rect: Rect) -> bool {
        content_rect.min_x() - main_rect.min_x() >= HIDE_THRESHOLD
    }

    pub fn widget_id(session: &SessionLocator, item_index: usize) -> WidgetId {
        stable_widget_id(0x38, &(session, item_index))
    }

    /// Positions one tick per turn, or an empty list when the rail is
    /// hidden (not enough gutter width) or there is nothing to anchor.
    /// Callers (paint, the accessibility tree, and click dispatch) all call
    /// this independently and get the same answer, mirroring how
    /// `ThreadTranscript::back_to_bottom_layout` is shared.
    pub fn layout(
        content_rect: Rect,
        main_rect: Rect,
        transcript: &TranscriptState,
        session: &SessionLocator,
        tool_expanded: &BTreeMap<String, bool>,
    ) -> Vec<AnchorTick> {
        if content_rect.size.x <= 0.0 || content_rect.size.y <= 0.0 {
            return Vec::new();
        }
        if !Self::visible(content_rect, main_rect) {
            return Vec::new();
        }
        let width = content_rect.size.x;
        let cache = transcript.layout_offsets(width, || {
            ThreadTranscript::compute_offsets(transcript, width, tool_expanded)
        });
        let total_height = cache.total_height;
        let anchors = transcript.anchors(width, &cache.offsets);
        if anchors.is_empty() {
            return Vec::new();
        }

        let max_offset = (total_height - content_rect.size.y).max(0.0);
        let viewport_start = if transcript.follow_tail {
            max_offset
        } else {
            transcript.scroll_offset.clamp(0.0, max_offset)
        };
        let viewport_end = viewport_start + content_rect.size.y;

        let rail_x = content_rect.min_x() - RAIL_GAP - RAIL_TICK_WIDTH;
        let lo = content_rect.min_y();
        let hi = content_rect.max_y();
        let raw: Vec<f32> = anchors
            .iter()
            .map(|anchor| lo + (anchor.y_offset / total_height.max(1.0)) * (hi - lo))
            .collect();
        let positions = cluster_positions(&raw, lo, hi, MIN_TICK_SPACING);

        anchors
            .iter()
            .zip(positions)
            .enumerate()
            .map(|(index, (anchor, y))| {
                let range_end = anchors
                    .get(index + 1)
                    .map(|next| next.y_offset)
                    .unwrap_or(total_height);
                let active = anchor.y_offset < viewport_end && range_end > viewport_start;
                let tick_rect =
                    Rect::xywh(rail_x, y - TICK_HEIGHT / 2.0, RAIL_TICK_WIDTH, TICK_HEIGHT);
                let hit_rect = Rect::xywh(
                    rail_x,
                    y - HOVER_HIT_HEIGHT / 2.0,
                    RAIL_TICK_WIDTH,
                    HOVER_HIT_HEIGHT,
                );
                AnchorTick {
                    anchor: anchor.clone(),
                    widget_id: Self::widget_id(session, anchor.item_index),
                    tick_rect,
                    hit_rect,
                    active,
                }
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        content_rect: Rect,
        main_rect: Rect,
        transcript: &TranscriptState,
        session: &SessionLocator,
        tool_expanded: &BTreeMap<String, bool>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        let ticks = Self::layout(content_rect, main_rect, transcript, session, tool_expanded);
        for tick in &ticks {
            let is_hovered = hovered == Some(tick.widget_id);
            let color = if is_hovered {
                theme.tokens.foreground
            } else if tick.active {
                theme.tokens.foreground.with_alpha(0.6)
            } else {
                theme.tokens.muted_foreground.with_alpha(0.3)
            };
            let rect = if is_hovered {
                let center = tick.tick_rect.origin.y + tick.tick_rect.size.y / 2.0;
                Rect::xywh(
                    tick.tick_rect.origin.x - 2.0,
                    center - TICK_HEIGHT_HOVERED / 2.0,
                    TICK_WIDTH_HOVERED,
                    TICK_HEIGHT_HOVERED,
                )
            } else {
                tick.tick_rect
            };
            painter.fill_round_rect(rect, rect.size.y / 2.0, color);
        }
        if let Some(hovered_id) = hovered {
            if let Some(tick) = ticks.iter().find(|tick| tick.widget_id == hovered_id) {
                paint_hover_card(painter, tick, content_rect, theme);
            }
        }
    }

    /// Resolves a click on an anchor tick to a `SetTranscriptViewport`
    /// command. Separate from `ThreadTranscript::command_for_widget`
    /// (matched purely by widget id) for the same reason
    /// `back_to_bottom_command` is: it needs the transcript viewport and
    /// primary-surface rects, which that path does not have.
    pub fn command_for_widget(
        state: &ZodeAppState,
        content_rect: Rect,
        main_rect: Rect,
        tool_expanded: &BTreeMap<String, bool>,
        id: WidgetId,
    ) -> Option<AppCommand> {
        let session = state.current_session.as_ref()?;
        let transcript = state.transcripts.get(session)?;
        let ticks = Self::layout(content_rect, main_rect, transcript, session, tool_expanded);
        let tick = ticks.into_iter().find(|tick| tick.widget_id == id)?;

        let width = content_rect.size.x;
        let cache = transcript.layout_offsets(width, || {
            ThreadTranscript::compute_offsets(transcript, width, tool_expanded)
        });
        let max_offset = (cache.total_height - content_rect.size.y).max(0.0);
        let scroll_offset =
            (tick.anchor.y_offset - content_rect.size.y * 0.15).clamp(0.0, max_offset);
        Some(AppCommand::SetTranscriptViewport {
            session: session.clone(),
            scroll_offset,
            follow_tail: false,
        })
    }
}

/// Proportional positions for `raw` clamped into `[lo, hi]`, then nudged
/// apart so consecutive ticks are never closer than `min_spacing`. When
/// there isn't room for that (very many turns in a short rail), falls back
/// to an even spread across the full `[lo, hi]` span rather than clamping
/// each position independently, which would collapse the excess onto the
/// same pixel instead of redistributing it.
fn cluster_positions(raw: &[f32], lo: f32, hi: f32, min_spacing: f32) -> Vec<f32> {
    let n = raw.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![raw[0].clamp(lo, hi)];
    }
    let available = (hi - lo).max(0.0);
    let required = min_spacing * (n - 1) as f32;
    if required > available {
        let step = available / (n - 1) as f32;
        return (0..n).map(|index| lo + step * index as f32).collect();
    }
    let mut positions: Vec<f32> = raw.iter().map(|&value| value.clamp(lo, hi)).collect();
    for index in 1..n {
        if positions[index] < positions[index - 1] + min_spacing {
            positions[index] = positions[index - 1] + min_spacing;
        }
    }
    if let Some(&last) = positions.last() {
        if last > hi {
            let shift = last - hi;
            for position in &mut positions {
                *position -= shift;
            }
        }
    }
    positions
}

fn paint_hover_card(
    painter: &mut dyn Painter,
    tick: &AnchorTick,
    content_rect: Rect,
    theme: &ZodeTheme,
) {
    let reply_lines = if tick.anchor.reply_excerpt.is_empty() {
        Vec::new()
    } else {
        wrap_lines(
            painter,
            &tick.anchor.reply_excerpt,
            CARD_WIDTH - CARD_PAD * 2.0,
            CARD_BODY_SIZE,
            400,
            CARD_MAX_REPLY_LINES,
        )
    };
    let has_chips = !tick.anchor.files.is_empty();
    let mut height = CARD_PAD * 2.0 + CARD_LINE_HEIGHT;
    if !reply_lines.is_empty() {
        height += CARD_ROW_GAP + reply_lines.len() as f32 * CARD_LINE_HEIGHT;
    }
    if has_chips {
        height += CARD_ROW_GAP + CHIP_HEIGHT;
    }

    let card_x = tick.tick_rect.max_x() + CARD_GAP;
    let center_y = tick.tick_rect.origin.y + tick.tick_rect.size.y / 2.0;
    let card_y =
        (center_y - height / 2.0).clamp(content_rect.min_y(), content_rect.max_y() - height);
    let card_rect = Rect::xywh(card_x, card_y, CARD_WIDTH, height);

    paint_elevated_surface(painter, card_rect, CARD_RADIUS, theme);
    painter.fill_round_rect(card_rect, CARD_RADIUS, theme.tokens.popover);
    painter.stroke_round_rect(card_rect, CARD_RADIUS, theme.tokens.border, 1.0);

    let mut y = card_rect.origin.y + CARD_PAD;
    let title = ellipsize(
        painter,
        &tick.anchor.user_excerpt,
        CARD_WIDTH - CARD_PAD * 2.0,
        CARD_TITLE_SIZE,
        600,
    );
    paint_single_line(
        painter,
        &title,
        Rect::xywh(
            card_rect.origin.x + CARD_PAD,
            y,
            CARD_WIDTH - CARD_PAD * 2.0,
            CARD_LINE_HEIGHT,
        ),
        CARD_TITLE_SIZE,
        600,
        theme.tokens.popover_foreground,
        HorizontalAlign::Start,
    );
    y += CARD_LINE_HEIGHT;

    if !reply_lines.is_empty() {
        y += CARD_ROW_GAP;
        for line in &reply_lines {
            paint_single_line(
                painter,
                line,
                Rect::xywh(
                    card_rect.origin.x + CARD_PAD,
                    y,
                    CARD_WIDTH - CARD_PAD * 2.0,
                    CARD_LINE_HEIGHT,
                ),
                CARD_BODY_SIZE,
                400,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
            y += CARD_LINE_HEIGHT;
        }
    }

    if has_chips {
        y += CARD_ROW_GAP;
        paint_file_chips(
            painter,
            &tick.anchor.files,
            Point2D::new(card_rect.origin.x + CARD_PAD, y),
            CARD_WIDTH - CARD_PAD * 2.0,
            theme,
        );
    }
}

fn paint_file_chips(
    painter: &mut dyn Painter,
    files: &[String],
    origin: Point2D,
    max_width: f32,
    theme: &ZodeTheme,
) {
    let shown = files.len().min(MAX_CHIPS);
    let overflow = files.len() - shown;
    let mut x = origin.x;
    let right_edge = origin.x + max_width;
    for label in &files[..shown] {
        let width = Pill::min_width(painter, label);
        if x + width > right_edge && x > origin.x {
            break;
        }
        let rect = Rect::xywh(x, origin.y, width.min(max_width), CHIP_HEIGHT);
        Pill::paint(painter, rect, label, &theme.tokens);
        x += rect.size.x + CHIP_GAP;
    }
    if overflow > 0 {
        let label = format!("+{overflow}");
        let width = Pill::min_width(painter, &label);
        if x + width <= right_edge || x == origin.x {
            Pill::paint(
                painter,
                Rect::xywh(x, origin.y, width, CHIP_HEIGHT),
                &label,
                &theme.tokens,
            );
        }
    }
}

/// Greedy word-wrap capped at `max_lines`, ellipsizing the last line when
/// more text remains than fit. Mirrors the manual-measurement wrap already
/// used for assistant markdown (`markdown::wrapped_inline`), but simpler
/// since a hover card's excerpt is already plain, pre-truncated text.
fn wrap_lines(
    painter: &mut dyn Painter,
    text: &str,
    max_width: f32,
    font_size: f32,
    weight: u16,
    max_lines: usize,
) -> Vec<String> {
    if max_width <= 0.0 || max_lines == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut next_word = 0;
    while next_word < words.len() {
        let word = words[next_word];
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if current.is_empty()
            || painter.measure_text_weighted(&candidate, font_size, weight) <= max_width
        {
            current = candidate;
            next_word += 1;
        } else {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                break;
            }
        }
    }
    let truncated = next_word < words.len();
    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }
    if truncated {
        if let Some(last) = lines.last_mut() {
            while !last.is_empty()
                && painter.measure_text_weighted(&format!("{last}…"), font_size, weight) > max_width
            {
                last.pop();
            }
            last.push('…');
        }
    }
    lines
}

/// End-ellipsizes `text` to fit `max_width`. Same approach as the
/// `end_ellipsize` helpers already duplicated per-widget elsewhere in this
/// crate (e.g. `composer::context`) - none of those are exported, so this
/// is its own copy rather than a shared import.
fn ellipsize(
    painter: &mut dyn Painter,
    text: &str,
    max_width: f32,
    font_size: f32,
    weight: u16,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if painter.measure_text_weighted(text, font_size, weight) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    if painter.measure_text_weighted(ellipsis, font_size, weight) > max_width {
        return String::new();
    }
    let mut end = text.len();
    while end > 0 {
        end = text[..end]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        let candidate = format!("{}…", &text[..end]);
        if painter.measure_text_weighted(&candidate, font_size, weight) <= max_width {
            return candidate;
        }
    }
    ellipsis.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_positions_preserves_order_and_min_spacing_when_there_is_room() {
        let raw = vec![0.0, 1.0, 2.0, 100.0];
        let positions = cluster_positions(&raw, 0.0, 200.0, 6.0);
        assert_eq!(positions.len(), 4);
        for window in positions.windows(2) {
            assert!(window[1] - window[0] + 1e-6 >= 6.0);
        }
        assert!(positions.windows(2).all(|window| window[1] >= window[0]));
        // The far-apart last point should be left essentially where it was.
        assert!((positions[3] - 100.0).abs() < 1.0);
    }

    #[test]
    fn cluster_positions_spreads_evenly_when_overcrowded() {
        let raw = vec![50.0; 10];
        let positions = cluster_positions(&raw, 0.0, 20.0, 6.0);
        assert_eq!(positions.len(), 10);
        assert!((positions[0] - 0.0).abs() < 1e-4);
        assert!((positions[9] - 20.0).abs() < 1e-4);
        let step = positions[1] - positions[0];
        for window in positions.windows(2) {
            assert!((window[1] - window[0] - step).abs() < 1e-4);
        }
    }

    #[test]
    fn cluster_positions_clamps_a_single_anchor_into_bounds() {
        assert_eq!(cluster_positions(&[500.0], 10.0, 100.0, 6.0), vec![100.0]);
        assert_eq!(cluster_positions(&[-5.0], 10.0, 100.0, 6.0), vec![10.0]);
    }

    #[test]
    fn cluster_positions_handles_the_empty_case() {
        assert!(cluster_positions(&[], 0.0, 100.0, 6.0).is_empty());
    }
}
