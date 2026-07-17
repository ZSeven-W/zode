use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{subagents_summary_avatar_ids, EnvironmentEntry, EnvironmentSectionKind};

use crate::widgets::subagent_avatar::subagent_avatar_color;
use crate::{paint_single_line, RectExt, SemanticIcon, ZodeTheme};

pub(super) const ROW_HEIGHT: f32 = 31.0;
const ICON_SIZE: f32 = 17.0;
const ICON_TEXT_GAP: f32 = 10.0;
pub(super) const SOURCES_VIEW_ALL_ID: &str = "sources-view-all";

/// Dot sizing for the compact avatar strip, standing in for Codex's
/// "colored avatar per agent" affordance, scoped down to a plain dot for
/// M1. Color itself comes from `subagent_avatar::subagent_avatar_color`,
/// shared with the M2 dedicated panel so a sub-agent's dot never disagrees
/// between the two surfaces.
const SUBAGENT_DOT_SIZE: f32 = 14.0;
const SUBAGENT_DOT_OVERLAP: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentRowLayout {
    pub entry: EnvironmentEntry,
    pub kind: EnvironmentSectionKind,
    pub rect: Rect,
    pub painted_by_action: bool,
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    layout: &EnvironmentRowLayout,
    armed_stop_process_id: Option<&str>,
    theme: &ZodeTheme,
) {
    if layout.painted_by_action {
        return;
    }
    if layout.kind == EnvironmentSectionKind::Subagents {
        paint_subagent_summary(painter, layout, theme);
        return;
    }
    let icon_rect = Rect::xywh(
        layout.rect.origin.x,
        layout.rect.origin.y + (layout.rect.size.y - ICON_SIZE) / 2.0,
        ICON_SIZE,
        ICON_SIZE,
    );
    let icon = icon_for(layout.kind, &layout.entry);
    painter.stroke_svg_path(
        icon.path(),
        icon_rect.origin,
        ICON_SIZE,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
    let text_x = icon_rect.max_x() + ICON_TEXT_GAP;
    let text_width = (layout.rect.max_x() - text_x).max(0.0);
    if layout.kind == EnvironmentSectionKind::Sources {
        paint_source(painter, layout, text_x, text_width, theme);
        return;
    }
    if layout.kind == EnvironmentSectionKind::Branch {
        paint_branch(painter, layout, text_x, text_width, theme);
        return;
    }
    if layout.kind == EnvironmentSectionKind::BackgroundProcesses {
        paint_background_process(
            painter,
            layout,
            armed_stop_process_id,
            text_x,
            text_width,
            theme,
        );
        return;
    }
    let label_width = if layout.entry.value.is_some() {
        (text_width * 0.38).clamp(48.0, 92.0)
    } else {
        text_width
    };
    paint_single_line(
        painter,
        &layout.entry.label,
        Rect::xywh(
            text_x,
            layout.rect.origin.y,
            label_width.min(text_width),
            layout.rect.size.y,
        ),
        14.0,
        400,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let Some(value) = layout.entry.value.as_deref() else {
        return;
    };
    let value_x = text_x + label_width + 8.0;
    let value_rect = Rect::xywh(
        value_x,
        layout.rect.origin.y,
        (layout.rect.max_x() - value_x).max(0.0),
        layout.rect.size.y,
    );
    let visible_value = middle_ellipsize(painter, value, value_rect.size.x, 14.0, 400);
    paint_single_line(
        painter,
        &visible_value,
        value_rect,
        14.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
}

fn paint_branch(
    painter: &mut dyn Painter,
    layout: &EnvironmentRowLayout,
    text_x: f32,
    text_width: f32,
    theme: &ZodeTheme,
) {
    let icon_space = 23.0;
    let branch = layout
        .entry
        .value
        .as_deref()
        .unwrap_or(layout.entry.label.as_str());
    let visible = middle_ellipsize(
        painter,
        branch,
        (text_width - icon_space).max(0.0),
        14.0,
        400,
    );
    paint_single_line(
        painter,
        &visible,
        Rect::xywh(
            text_x,
            layout.rect.origin.y,
            (text_width - icon_space).max(0.0),
            layout.rect.size.y,
        ),
        14.0,
        400,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_trailing_icon(painter, layout.rect, SemanticIcon::ChevronDown, theme);
}

fn paint_source(
    painter: &mut dyn Painter,
    layout: &EnvironmentRowLayout,
    text_x: f32,
    text_width: f32,
    theme: &ZodeTheme,
) {
    // A web-activity aggregate row (see `EnvironmentEntry::count`) has no
    // per-file identity to show, just a label ("网页"/"网页搜索") and a
    // count - render that instead of the filename + activity chip layout.
    if let Some(count) = layout.entry.count {
        let count_width = 40.0_f32.min(text_width * 0.3);
        let label_width = (text_width - count_width).max(0.0);
        paint_single_line(
            painter,
            &layout.entry.label,
            Rect::xywh(
                text_x,
                layout.rect.origin.y,
                label_width,
                layout.rect.size.y,
            ),
            14.0,
            400,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            &count.to_string(),
            Rect::xywh(
                text_x + label_width,
                layout.rect.origin.y,
                count_width,
                layout.rect.size.y,
            ),
            14.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
        return;
    }
    let is_view_all = layout.entry.id == SOURCES_VIEW_ALL_ID;
    let source_name = if is_view_all {
        layout.entry.label.as_str()
    } else {
        layout
            .entry
            .value
            .as_deref()
            .map(file_name)
            .filter(|value| !value.is_empty())
            .unwrap_or(layout.entry.label.as_str())
    };
    // A normal per-file row reserves room on the right for its activity
    // chip ("已创建"/"已提供"/"已读取"/"已更新"); the view-all row and any
    // row with no attributed activity keep the full width for the name.
    let activity_width = layout
        .entry
        .activity
        .map(|_| 56.0_f32.min(text_width * 0.4))
        .unwrap_or(0.0);
    let name_width = (text_width - activity_width).max(0.0);
    let visible = middle_ellipsize(painter, source_name, name_width, 14.0, 400);
    paint_single_line(
        painter,
        &visible,
        Rect::xywh(text_x, layout.rect.origin.y, name_width, layout.rect.size.y),
        14.0,
        400,
        if is_view_all {
            theme.tokens.muted_foreground
        } else {
            theme.tokens.foreground
        },
        HorizontalAlign::Start,
    );
    if let Some(activity) = layout.entry.activity {
        paint_single_line(
            painter,
            activity.label(),
            Rect::xywh(
                text_x + name_width,
                layout.rect.origin.y,
                activity_width,
                layout.rect.size.y,
            ),
            14.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
    }
}

/// `EnvironmentEntry::id` prefix a BackgroundProcesses row uses to carry the
/// live shell id - see `zode_app_model::presentation::background_process_entries`.
pub(super) const BACKGROUND_PROCESS_ID_PREFIX: &str = "bg:";

/// Recovers the live shell id from a BackgroundProcesses row's entry id.
pub(super) fn background_process_id(entry_id: &str) -> Option<&str> {
    entry_id.strip_prefix(BACKGROUND_PROCESS_ID_PREFIX)
}

/// Paints one BackgroundProcesses row: the command (left, ellipsized) and,
/// on the right, either the process's real lifecycle label or - while this
/// exact process is armed for a stop confirmation (`armed_stop_process_id`,
/// set by `AppCommand::ArmBackgroundProcessStop`) - a one-tap "确认停止"
/// warning in place of it. A trailing icon is always drawn as the "查看
/// 输出" (view output) hit target; `EnvironmentPanel::command_for_widget`
/// resolves both targets' enabled state against the live process list at
/// click time rather than baking it into this paint pass.
fn paint_background_process(
    painter: &mut dyn Painter,
    layout: &EnvironmentRowLayout,
    armed_stop_process_id: Option<&str>,
    text_x: f32,
    text_width: f32,
    theme: &ZodeTheme,
) {
    let icon_space = 23.0;
    let armed = background_process_id(&layout.entry.id)
        .is_some_and(|id| armed_stop_process_id.is_some_and(|armed_id| armed_id == id));
    let value_width = ((text_width - icon_space) * 0.4).clamp(56.0, 104.0);
    let label_width = (text_width - icon_space - value_width).max(0.0);
    let visible_label = middle_ellipsize(painter, &layout.entry.label, label_width, 14.0, 400);
    paint_single_line(
        painter,
        &visible_label,
        Rect::xywh(
            text_x,
            layout.rect.origin.y,
            label_width,
            layout.rect.size.y,
        ),
        14.0,
        400,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let (value, value_color) = if armed {
        ("确认停止", theme.tokens.destructive)
    } else {
        (
            layout.entry.value.as_deref().unwrap_or_default(),
            theme.tokens.muted_foreground,
        )
    };
    paint_single_line(
        painter,
        value,
        Rect::xywh(
            text_x + label_width,
            layout.rect.origin.y,
            value_width,
            layout.rect.size.y,
        ),
        14.0,
        400,
        value_color,
        HorizontalAlign::End,
    );
    paint_trailing_icon(painter, layout.rect, SemanticIcon::ExternalOpen, theme);
}

/// Paints the Subagents section's one compact row: up to
/// `MAX_SUBAGENT_AVATARS` overlapping colored dots (Codex's avatar-strip
/// affordance, standing in for real avatar images) followed by the
/// running/completed count text.
fn paint_subagent_summary(
    painter: &mut dyn Painter,
    layout: &EnvironmentRowLayout,
    theme: &ZodeTheme,
) {
    let avatar_ids = subagents_summary_avatar_ids(&layout.entry.id);
    let dot_size = SUBAGENT_DOT_SIZE.min(layout.rect.size.y - 4.0).max(0.0);
    let step = (dot_size - SUBAGENT_DOT_OVERLAP).max(1.0);
    let strip_width = if avatar_ids.is_empty() {
        0.0
    } else {
        dot_size + (avatar_ids.len() as f32 - 1.0) * step
    };
    let y = layout.rect.origin.y + (layout.rect.size.y - dot_size) / 2.0;
    for (index, id) in avatar_ids.iter().enumerate() {
        let x = layout.rect.origin.x + index as f32 * step;
        let color = subagent_avatar_color(id);
        painter.fill_round_rect(Rect::xywh(x, y, dot_size, dot_size), dot_size / 2.0, color);
    }
    let text_x = layout.rect.origin.x
        + strip_width
        + if avatar_ids.is_empty() {
            0.0
        } else {
            ICON_TEXT_GAP
        };
    let text_width = (layout.rect.max_x() - text_x).max(0.0);
    paint_single_line(
        painter,
        layout.entry.value.as_deref().unwrap_or_default(),
        Rect::xywh(text_x, layout.rect.origin.y, text_width, layout.rect.size.y),
        14.0,
        400,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

fn icon_for(kind: EnvironmentSectionKind, entry: &EnvironmentEntry) -> SemanticIcon {
    match kind {
        EnvironmentSectionKind::Changes => SemanticIcon::Diff,
        EnvironmentSectionKind::Host => SemanticIcon::Host,
        EnvironmentSectionKind::Branch => SemanticIcon::Branch,
        EnvironmentSectionKind::RepositoryActions => SemanticIcon::Git,
        EnvironmentSectionKind::Comparisons => SemanticIcon::Compare,
        EnvironmentSectionKind::Subagents => SemanticIcon::Sparkles,
        EnvironmentSectionKind::ComputerUse => SemanticIcon::Computer,
        EnvironmentSectionKind::BackgroundProcesses => SemanticIcon::Terminal,
        EnvironmentSectionKind::Sources if entry.id == SOURCES_VIEW_ALL_ID => {
            SemanticIcon::ExternalOpen
        }
        EnvironmentSectionKind::Sources if entry.count.is_some() => SemanticIcon::Search,
        EnvironmentSectionKind::Sources => SemanticIcon::FileText,
    }
}

fn paint_trailing_icon(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    theme: &ZodeTheme,
) {
    let size = 15.0;
    painter.stroke_svg_path(
        icon.path(),
        jian_widgets::Point2D::new(
            rect.max_x() - size,
            rect.origin.y + (rect.size.y - size) / 2.0,
        ),
        size,
        theme.tokens.muted_foreground,
        icon.stroke_width(),
    );
}

fn file_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
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
