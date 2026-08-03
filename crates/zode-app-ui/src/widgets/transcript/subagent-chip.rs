//! Inline sub-agent lifecycle chip.
//!
//! Painted like the other lightweight transcript furniture (thinking lines,
//! activity rows): a leading mark, a muted headline, and a dimmer trailing
//! detail - no card, no border. The leading mark is the same colored dot the
//! sub-agent panel gives that agent (`subagent_avatar_color`), so the same
//! child is recognizable in both places without repeating its name twice.

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{SubagentChip, SubagentChipPhase};

use crate::widgets::subagent_avatar::subagent_avatar_color;
use crate::{ellipsize_end, paint_single_line, RectExt, ZodeTheme};

pub(super) const HEIGHT: f32 = 28.0;

const DOT_SIZE: f32 = 8.0;
/// Matches the icon column the neighboring activity rows reserve, so a chip
/// between two of them keeps one text baseline column.
const MARK_COLUMN: f32 = 15.0;
const TEXT_GAP: f32 = 8.0;
const DETAIL_GAP: f32 = 10.0;
const HEADLINE_SIZE: f32 = 14.0;
const HEADLINE_WEIGHT: u16 = 500;
const DETAIL_SIZE: f32 = 13.0;
const DETAIL_WEIGHT: u16 = 400;

pub(super) fn paint(painter: &mut dyn Painter, rect: Rect, chip: &SubagentChip, theme: &ZodeTheme) {
    let dot = Rect::xywh(
        rect.origin.x + (MARK_COLUMN - DOT_SIZE) / 2.0,
        rect.origin.y + (rect.size.y - DOT_SIZE) / 2.0,
        DOT_SIZE,
        DOT_SIZE,
    );
    let mark = if chip.phase == SubagentChipPhase::Failed {
        theme.tokens.destructive
    } else {
        subagent_avatar_color(&chip.agent_id)
    };
    // A finished agent's dot is hollowed out by drawing it at the muted
    // weight instead of full strength, so a glance separates "still mine to
    // wait on" from "already reported back".
    let mark = if chip.phase == SubagentChipPhase::Finished {
        mark.with_alpha(0.55)
    } else {
        mark
    };
    painter.fill_round_rect(dot, DOT_SIZE / 2.0, mark);

    let text_x = rect.origin.x + MARK_COLUMN + TEXT_GAP;
    let available = (rect.max_x() - text_x).max(0.0);
    let headline = chip.headline();
    let headline_width = painter
        .measure_text_weighted(&headline, HEADLINE_SIZE, HEADLINE_WEIGHT)
        .min((available * 0.68).max(0.0));
    let headline = ellipsize_end(
        painter,
        &headline,
        headline_width,
        HEADLINE_SIZE,
        HEADLINE_WEIGHT,
    );
    paint_single_line(
        painter,
        &headline,
        Rect::xywh(text_x, rect.origin.y, headline_width, rect.size.y),
        HEADLINE_SIZE,
        HEADLINE_WEIGHT,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );

    let Some(detail) = chip.detail() else {
        return;
    };
    let detail_x = text_x + headline_width + DETAIL_GAP;
    let detail_width = (rect.max_x() - detail_x).max(0.0);
    let detail = ellipsize_end(painter, &detail, detail_width, DETAIL_SIZE, DETAIL_WEIGHT);
    paint_single_line(
        painter,
        &detail,
        Rect::xywh(detail_x, rect.origin.y, detail_width, rect.size.y),
        DETAIL_SIZE,
        DETAIL_WEIGHT,
        theme.tokens.muted_foreground.with_alpha(0.72),
        HorizontalAlign::Start,
    );
}

/// Screen-reader label: the headline plus whatever detail the chip shows,
/// and the model disclosure even when the detail slot went to a result
/// summary instead.
pub(crate) fn accessibility_label(chip: &SubagentChip) -> String {
    let mut label = chip.headline();
    if let Some(summary) = chip
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
    {
        label.push('；');
        label.push_str(summary);
    }
    if let Some(model) = chip.model_label() {
        label.push('；');
        label.push_str(&model);
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(phase: SubagentChipPhase, summary: Option<&str>) -> SubagentChip {
        SubagentChip {
            agent_id: "1".into(),
            display_name: "审查代码".into(),
            agent_type: "reviewer".into(),
            phase,
            summary: summary.map(str::to_owned),
            model: Some("claude-opus-5".into()),
        }
    }

    #[test]
    fn the_label_discloses_the_model_even_when_a_summary_takes_the_detail_slot() {
        let label = accessibility_label(&chip(SubagentChipPhase::Finished, Some("已读取三个文件")));
        assert_eq!(
            label,
            "审查代码 · 已完成；已读取三个文件；使用 claude-opus-5"
        );
    }

    #[test]
    fn a_running_chip_reads_as_the_headline_plus_the_model() {
        let label = accessibility_label(&chip(SubagentChipPhase::Progress, None));
        assert_eq!(label, "审查代码 · 有进展；使用 claude-opus-5");
    }
}
