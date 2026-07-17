use std::ops::Range;

use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::AppCommand;
use zode_node_protocol::{DiffSnapshot, WorkspaceUri};

use crate::{visible_range, MeasuredItem, ZodeTheme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewLineKind {
    Hunk,
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLine {
    pub kind: ReviewLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSelection {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewDraft {
    selection: Option<ReviewSelection>,
    comment: String,
}

impl ReviewDraft {
    pub fn select(&mut self, selection: ReviewSelection) {
        self.selection = Some(selection);
    }

    pub fn selection(&self) -> Option<&ReviewSelection> {
        self.selection.as_ref()
    }

    pub fn set_comment(&mut self, comment: String) {
        self.comment = comment;
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }
}

pub struct ReviewPanel;

impl ReviewPanel {
    pub fn parse_unified(unified: &str) -> Vec<ReviewLine> {
        let mut lines = Vec::new();
        let mut old_line = 0;
        let mut new_line = 0;
        for raw in unified.lines() {
            if raw.starts_with("@@ ") {
                if let Some((old, new)) = hunk_starts(raw) {
                    old_line = old;
                    new_line = new;
                }
                lines.push(ReviewLine {
                    kind: ReviewLineKind::Hunk,
                    old_line: None,
                    new_line: None,
                    text: raw.to_owned(),
                });
            } else if raw.starts_with("diff --git ")
                || raw.starts_with("index ")
                || raw.starts_with("--- ")
                || raw.starts_with("+++ ")
                || raw.starts_with("new file mode ")
                || raw.starts_with("deleted file mode ")
                || raw.starts_with("Binary files ")
                || raw.starts_with("\\ No newline")
            {
                continue;
            } else if let Some(text) = raw.strip_prefix('+') {
                lines.push(ReviewLine {
                    kind: ReviewLineKind::Addition,
                    old_line: None,
                    new_line: Some(new_line),
                    text: text.to_owned(),
                });
                new_line = new_line.saturating_add(1);
            } else if let Some(text) = raw.strip_prefix('-') {
                lines.push(ReviewLine {
                    kind: ReviewLineKind::Deletion,
                    old_line: Some(old_line),
                    new_line: None,
                    text: text.to_owned(),
                });
                old_line = old_line.saturating_add(1);
            } else if let Some(text) = raw.strip_prefix(' ') {
                lines.push(ReviewLine {
                    kind: ReviewLineKind::Context,
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    text: text.to_owned(),
                });
                old_line = old_line.saturating_add(1);
                new_line = new_line.saturating_add(1);
            }
        }
        lines
    }

    pub fn visible_line_range(
        line_count: usize,
        offset: f32,
        viewport_height: f32,
        line_height: f32,
    ) -> Range<usize> {
        let line_height = line_height.max(1.0);
        let items = (0..line_count)
            .map(|index| {
                let top = index as f32 * line_height;
                MeasuredItem::new(top, top + line_height)
            })
            .collect::<Vec<_>>();
        visible_range(&items, offset, viewport_height)
    }

    pub fn open_file_command(
        workspace_uri: WorkspaceUri,
        relative_path: impl Into<String>,
    ) -> AppCommand {
        AppCommand::OpenWorkspaceFile {
            workspace_uri,
            relative_path: relative_path.into(),
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        snapshot: &DiffSnapshot,
        offset: f32,
        theme: &ZodeTheme,
    ) {
        const FILE_LIST_WIDTH: f32 = 210.0;
        const LINE_HEIGHT: f32 = 20.0;
        let file_rect = Rect::xywh(
            rect.origin.x,
            rect.origin.y,
            FILE_LIST_WIDTH.min(rect.size.x),
            rect.size.y,
        );
        painter.fill_rect(file_rect, theme.tokens.muted);
        for (index, file) in snapshot.files.iter().enumerate() {
            draw_text(
                painter,
                &file.path,
                Point2D::new(
                    file_rect.origin.x + 10.0,
                    file_rect.origin.y + 22.0 + index as f32 * 28.0,
                ),
                11.0,
                500,
                theme.tokens.foreground,
            );
        }

        let code_rect = Rect::xywh(
            rect.origin.x + file_rect.size.x,
            rect.origin.y,
            (rect.size.x - file_rect.size.x).max(0.0),
            rect.size.y,
        );
        let lines = Self::parse_unified(&snapshot.unified);
        painter.save();
        painter.clip_rect(code_rect);
        for index in Self::visible_line_range(lines.len(), offset, code_rect.size.y, LINE_HEIGHT) {
            let line = &lines[index];
            let y = code_rect.origin.y + index as f32 * LINE_HEIGHT - offset;
            let background = match line.kind {
                ReviewLineKind::Addition => theme.success.with_alpha(0.12),
                ReviewLineKind::Deletion => theme.tokens.destructive.with_alpha(0.12),
                ReviewLineKind::Hunk => theme.zode_purple.with_alpha(0.08),
                ReviewLineKind::Context => theme.tokens.background,
            };
            painter.fill_rect(
                Rect::xywh(code_rect.origin.x, y, code_rect.size.x, LINE_HEIGHT),
                background,
            );
            let numbers = format!(
                "{:>4} {:>4}",
                line.old_line.map_or(String::new(), |line| line.to_string()),
                line.new_line.map_or(String::new(), |line| line.to_string()),
            );
            draw_text(
                painter,
                &numbers,
                Point2D::new(code_rect.origin.x + 8.0, y + 14.0),
                10.0,
                400,
                theme.tokens.muted_foreground,
            );
            draw_text(
                painter,
                &line.text,
                Point2D::new(code_rect.origin.x + 82.0, y + 14.0),
                11.0,
                400,
                theme.tokens.foreground,
            );
        }
        painter.restore();
    }
}

fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let mut parts = header.split_whitespace();
    (parts.next()? == "@@").then_some(())?;
    let old = range_start(parts.next()?, '-')?;
    let new = range_start(parts.next()?, '+')?;
    Some((old, new))
}

fn range_start(value: &str, prefix: char) -> Option<u32> {
    value.strip_prefix(prefix)?.split(',').next()?.parse().ok()
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "ui-monospace", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}
