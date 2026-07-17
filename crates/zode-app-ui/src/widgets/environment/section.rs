use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{EnvironmentSection, EnvironmentSectionKind};

use crate::{paint_single_line, ZodeTheme};

use super::row::{self, EnvironmentRowLayout, ROW_HEIGHT};

pub(super) const SECTION_HEADER_HEIGHT: f32 = 18.0;
pub(super) const SECTION_GAP: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentSectionLayout {
    pub section: EnvironmentSection,
    pub rect: Rect,
    pub rows: Vec<EnvironmentRowLayout>,
    pub footer: bool,
}

impl EnvironmentSectionLayout {
    pub fn last_row(&self) -> Option<Rect> {
        self.rows.last().map(|row| row.rect)
    }
}

pub(super) fn height(section: &EnvironmentSection) -> f32 {
    SECTION_HEADER_HEIGHT + ROW_HEIGHT * section.entries.len() as f32
}

pub(super) fn layout(section: EnvironmentSection, rect: Rect, y: f32) -> EnvironmentSectionLayout {
    let height = height(&section);
    let rows = section
        .entries
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, entry)| EnvironmentRowLayout {
            entry,
            rect: Rect::xywh(
                rect.origin.x,
                y + SECTION_HEADER_HEIGHT + index as f32 * ROW_HEIGHT,
                rect.size.x,
                ROW_HEIGHT,
            ),
        })
        .collect();
    EnvironmentSectionLayout {
        section,
        rect: Rect::xywh(rect.origin.x, y, rect.size.x, height),
        rows,
        footer: false,
    }
}

pub(super) fn footer(section: EnvironmentSection, rect: Rect) -> EnvironmentSectionLayout {
    let rows = section
        .entries
        .iter()
        .cloned()
        .map(|entry| EnvironmentRowLayout { entry, rect })
        .collect();
    EnvironmentSectionLayout {
        section,
        rect,
        rows,
        footer: true,
    }
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    layout: &EnvironmentSectionLayout,
    theme: &ZodeTheme,
) {
    if layout.footer {
        return;
    }
    paint_single_line(
        painter,
        layout.section.kind.title(),
        Rect::xywh(
            layout.rect.origin.x,
            layout.rect.origin.y,
            layout.rect.size.x,
            SECTION_HEADER_HEIGHT,
        ),
        11.0,
        600,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    for row in &layout.rows {
        row::paint(painter, row, theme);
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

pub(super) fn is_repository_action(kind: EnvironmentSectionKind) -> bool {
    kind == EnvironmentSectionKind::RepositoryActions
}
