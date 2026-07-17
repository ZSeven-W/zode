use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::EnvironmentSection;

use crate::{paint_single_line, RectExt, ZodeTheme};

use super::row::{self, EnvironmentRowLayout, ROW_HEIGHT};

pub(super) const SECTION_HEADER_HEIGHT: f32 = 26.0;
pub(super) const SECTION_GAP: f32 = 14.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentSectionLayout {
    pub section: EnvironmentSection,
    pub rect: Rect,
    pub rows: Vec<EnvironmentRowLayout>,
    pub footer: bool,
    pub header_title: Option<&'static str>,
    pub separator: bool,
}

impl EnvironmentSectionLayout {
    pub fn last_row(&self) -> Option<Rect> {
        self.rows.last().map(|row| row.rect)
    }
}

pub(super) fn height(section: &EnvironmentSection, shows_header: bool) -> f32 {
    (if shows_header {
        SECTION_HEADER_HEIGHT
    } else {
        0.0
    }) + ROW_HEIGHT * section.entries.len() as f32
}

pub(super) fn layout(
    section: EnvironmentSection,
    rect: Rect,
    y: f32,
    header_title: Option<&'static str>,
    separator: bool,
) -> EnvironmentSectionLayout {
    let header_height = header_title.map_or(0.0, |_| SECTION_HEADER_HEIGHT);
    let height = height(&section, header_title.is_some());
    let kind = section.kind;
    let rows = section
        .entries
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, entry)| EnvironmentRowLayout {
            entry,
            kind,
            rect: Rect::xywh(
                rect.origin.x,
                y + header_height + index as f32 * ROW_HEIGHT,
                rect.size.x,
                ROW_HEIGHT,
            ),
            painted_by_action: false,
        })
        .collect();
    EnvironmentSectionLayout {
        section,
        rect: Rect::xywh(rect.origin.x, y, rect.size.x, height),
        rows,
        footer: false,
        header_title,
        separator,
    }
}

pub(super) fn footer(section: EnvironmentSection, rect: Rect) -> EnvironmentSectionLayout {
    let rows = section
        .entries
        .iter()
        .cloned()
        .map(|entry| EnvironmentRowLayout {
            entry,
            kind: section.kind,
            rect,
            painted_by_action: true,
        })
        .collect();
    EnvironmentSectionLayout {
        section,
        rect,
        rows,
        footer: true,
        header_title: None,
        separator: false,
    }
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    layout: &EnvironmentSectionLayout,
    armed_stop_process_id: Option<&str>,
    theme: &ZodeTheme,
) {
    if layout.footer {
        return;
    }
    if let Some(title) = layout.header_title {
        paint_single_line(
            painter,
            title,
            Rect::xywh(
                layout.rect.origin.x,
                layout.rect.origin.y,
                layout.rect.size.x,
                SECTION_HEADER_HEIGHT,
            ),
            14.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    for row in &layout.rows {
        row::paint(painter, row, armed_stop_process_id, theme);
    }
    if layout.separator {
        let separator_y = layout.rect.max_y() + SECTION_GAP / 2.0;
        painter.stroke_line(
            jian_widgets::Point2D::new(layout.rect.origin.x, separator_y),
            jian_widgets::Point2D::new(layout.rect.max_x(), separator_y),
            theme.tokens.border.with_alpha(0.72),
            1.0,
        );
    }
}

pub(super) fn accessibility_name(layout: &EnvironmentSectionLayout) -> String {
    let entries = layout
        .section
        .entries
        .iter()
        .map(|entry| match entry.value.as_deref() {
            Some(value) => format!("{}：{value}", entry.label),
            None => entry.label.clone(),
        })
        .collect::<Vec<_>>()
        .join("；");
    format!("{}：{entries}", layout.section.kind.title())
}
