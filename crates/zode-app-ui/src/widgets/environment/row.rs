use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{EnvironmentEntry, EnvironmentSectionKind};

use crate::{paint_single_line, RectExt, SemanticIcon, ZodeTheme};

pub(super) const ROW_HEIGHT: f32 = 31.0;
const ICON_SIZE: f32 = 17.0;
const ICON_TEXT_GAP: f32 = 10.0;
pub(super) const SOURCES_VIEW_ALL_ID: &str = "sources-view-all";

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentRowLayout {
    pub entry: EnvironmentEntry,
    pub kind: EnvironmentSectionKind,
    pub rect: Rect,
    pub painted_by_action: bool,
}

pub(super) fn paint(painter: &mut dyn Painter, layout: &EnvironmentRowLayout, theme: &ZodeTheme) {
    if layout.painted_by_action {
        return;
    }
    let icon = icon_for(layout.kind, &layout.entry);
    let icon_rect = Rect::xywh(
        layout.rect.origin.x,
        layout.rect.origin.y + (layout.rect.size.y - ICON_SIZE) / 2.0,
        ICON_SIZE,
        ICON_SIZE,
    );
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
    let visible = middle_ellipsize(painter, source_name, text_width, 14.0, 400);
    paint_single_line(
        painter,
        &visible,
        Rect::xywh(text_x, layout.rect.origin.y, text_width, layout.rect.size.y),
        14.0,
        400,
        if is_view_all {
            theme.tokens.muted_foreground
        } else {
            theme.tokens.foreground
        },
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
        EnvironmentSectionKind::BackgroundProcesses => SemanticIcon::Terminal,
        EnvironmentSectionKind::Sources if entry.id == SOURCES_VIEW_ALL_ID => {
            SemanticIcon::ExternalOpen
        }
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
