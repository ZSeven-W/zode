//! Compact "已安装插件" strip on the Plugins tab: one row per git-installed
//! plugin bundle (as opposed to the individual skill/MCP/tool-group rows the
//! existing catalog already renders - a plugin bundles several of those).
//! Clicking a row opens the detail overlay ([`super::plugin_detail`]).

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_node_protocol::{InstalledPluginSummary, PluginTrustState};

use crate::{paint_single_line, stable_widget_id, WidgetId, ZodeTheme};

/// Mirrors `MAX_SUBAGENT_AVATARS`'s "show a few, fold the rest into a count"
/// convention rather than building a second scrollable region on this page.
pub const MAX_VISIBLE_PLUGIN_ROWS: usize = 4;
const ROW_H: f32 = 40.0;
const ROW_GAP: f32 = 6.0;

pub fn plugin_row_widget_id(plugin_id: &str) -> WidgetId {
    stable_widget_id(0x80, plugin_id)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginRowLayout {
    pub id: WidgetId,
    pub plugin_id: String,
    pub repo: String,
    pub reference: String,
    pub trust_label: &'static str,
    pub rect: Rect,
}

/// Total strip height for `count` installed plugins, including the trailing
/// "+N 更多" line when `count` exceeds [`MAX_VISIBLE_PLUGIN_ROWS`]. Callers
/// use this to reserve space above the catalog only when there is something
/// to show.
pub fn strip_height(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let visible = count.min(MAX_VISIBLE_PLUGIN_ROWS) as f32;
    let rows_height = visible * ROW_H + (visible - 1.0).max(0.0) * ROW_GAP;
    let overflow_line = if count > MAX_VISIBLE_PLUGIN_ROWS {
        20.0
    } else {
        0.0
    };
    rows_height + overflow_line
}

pub fn layout(
    origin_x: f32,
    origin_y: f32,
    width: f32,
    plugins: &[InstalledPluginSummary],
) -> Vec<PluginRowLayout> {
    plugins
        .iter()
        .take(MAX_VISIBLE_PLUGIN_ROWS)
        .enumerate()
        .map(|(index, plugin)| PluginRowLayout {
            id: plugin_row_widget_id(&plugin.id),
            plugin_id: plugin.id.clone(),
            repo: plugin.repo.clone(),
            reference: plugin.reference.clone(),
            trust_label: trust_label(&plugin.trust),
            rect: Rect::xywh(
                origin_x,
                origin_y + index as f32 * (ROW_H + ROW_GAP),
                width,
                ROW_H,
            ),
        })
        .collect()
}

fn trust_label(trust: &PluginTrustState) -> &'static str {
    match trust {
        PluginTrustState::Trusted => "已信任",
        PluginTrustState::NeedsReview(_) => "待审查",
        PluginTrustState::Drifted(_) => "需重新审查",
    }
}

pub fn paint(
    painter: &mut dyn Painter,
    rows: &[PluginRowLayout],
    plugin_count: usize,
    theme: &ZodeTheme,
) {
    for row in rows {
        painter.fill_round_rect(row.rect, 10.0, theme.tokens.card);
        painter.stroke_round_rect(row.rect, 10.0, theme.tokens.border, 1.0);
        let text_x = row.rect.origin.x + 12.0;
        let badge_width = 76.0_f32.min((row.rect.size.x * 0.3).max(0.0));
        let badge = Rect::xywh(
            row.rect.origin.x + row.rect.size.x - badge_width - 10.0,
            row.rect.origin.y + (row.rect.size.y - 22.0) / 2.0,
            badge_width,
            22.0,
        );
        paint_single_line(
            painter,
            &row.repo,
            Rect::xywh(
                text_x,
                row.rect.origin.y + 6.0,
                (badge.origin.x - text_x - 8.0).max(0.0),
                16.0,
            ),
            12.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            &row.reference,
            Rect::xywh(
                text_x,
                row.rect.origin.y + 22.0,
                (badge.origin.x - text_x - 8.0).max(0.0),
                14.0,
            ),
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        painter.fill_round_rect(badge, 11.0, theme.tokens.muted);
        paint_single_line(
            painter,
            row.trust_label,
            badge,
            10.0,
            550,
            if row.trust_label == "已信任" {
                theme.success
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Center,
        );
    }
    if plugin_count > MAX_VISIBLE_PLUGIN_ROWS {
        if let Some(last) = rows.last() {
            paint_single_line(
                painter,
                &format!("共 {plugin_count} 个插件"),
                Rect::xywh(
                    last.rect.origin.x,
                    last.rect.origin.y + last.rect.size.y + 4.0,
                    last.rect.size.x,
                    18.0,
                ),
                11.0,
                450,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
    }
}
