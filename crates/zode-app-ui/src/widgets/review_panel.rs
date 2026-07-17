use std::ops::Range;

use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, LoadState, ZodeAppState};
use zode_node_protocol::{DiffSnapshot, SessionLocator};

use crate::{
    stable_widget_id, visible_range, MeasuredItem, RectExt, SemanticIcon, WidgetId, ZodeTheme,
    REVIEW_CLOSE_ID,
};

const REVIEW_HEADER_HEIGHT: f32 = 46.0;
const REVIEW_CLOSE_SIZE: f32 = 24.0;
const REVIEW_INSET: f32 = 12.0;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewPanelLayout {
    pub header: Rect,
    pub content: Rect,
    pub close_button: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFileRowLayout {
    pub id: WidgetId,
    pub path: String,
    pub rect: Rect,
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
    pub fn layout(rect: Rect) -> ReviewPanelLayout {
        let width = finite_non_negative(rect.size.x);
        let height = finite_non_negative(rect.size.y);
        let header_height = REVIEW_HEADER_HEIGHT.min(height);
        let header = Rect::xywh(rect.origin.x, rect.origin.y, width, header_height);
        let close_size = REVIEW_CLOSE_SIZE.min(width).min(header_height);
        let close_button = Rect::xywh(
            (rect.origin.x + width - REVIEW_INSET - close_size).max(rect.origin.x),
            rect.origin.y + (header_height - close_size).max(0.0) / 2.0,
            close_size,
            close_size,
        );
        ReviewPanelLayout {
            header,
            content: Rect::xywh(
                rect.origin.x,
                rect.origin.y + header_height,
                width,
                (height - header_height).max(0.0),
            ),
            close_button,
        }
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == REVIEW_CLOSE_ID {
            return Some(AppCommand::CloseSecondary);
        }
        let session = state.current_session.as_ref()?.clone();
        let row = Self::file_row_layouts(Rect::xywh(0.0, 0.0, 1.0, f32::MAX), state)
            .into_iter()
            .find(|row| row.id == id)?;
        Some(Self::open_file_command(session, row.path))
    }

    pub fn file_widget_id(session: &SessionLocator, path: &str) -> WidgetId {
        stable_widget_id(0x41, &(session, path))
    }

    pub fn file_row_layouts(rect: Rect, state: &ZodeAppState) -> Vec<ReviewFileRowLayout> {
        let Some(session) = state.current_session.as_ref() else {
            return Vec::new();
        };
        if !state
            .available_workspace_for_session(session)
            .is_some_and(|workspace| workspace.as_str().starts_with("file://"))
        {
            return Vec::new();
        }
        let Some(snapshot) = state
            .presentation
            .sessions
            .get(session)
            .and_then(|presentation| presentation.diff.load.ready())
            .filter(|snapshot| &snapshot.session == session)
        else {
            return Vec::new();
        };
        let content = Self::layout(rect).content;
        let width = 210.0_f32.min(content.size.x.max(0.0));
        snapshot
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| {
                let y = content.origin.y + index as f32 * 28.0;
                let height = (content.max_y() - y).clamp(0.0, 28.0);
                let rect = Rect::xywh(content.origin.x, y, width, height);
                (rect.size.x > 0.0 && rect.size.y > 0.0 && rect.origin.y < content.max_y()).then(
                    || ReviewFileRowLayout {
                        id: Self::file_widget_id(session, &file.path),
                        path: file.path.clone(),
                        rect,
                    },
                )
            })
            .collect()
    }

    pub fn paint_state(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(rect);
        if layout.header.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.fill_rect(rect, theme.tokens.background);
        painter.save();
        painter.clip_rect(rect);
        draw_ui_text(
            painter,
            "变更",
            Point2D::new(layout.header.origin.x + 16.0, layout.header.origin.y + 29.0),
            14.0,
            600,
            theme.tokens.foreground,
        );
        let close_icon_size = 14.0_f32
            .min(layout.close_button.size.x)
            .min(layout.close_button.size.y);
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            Point2D::new(
                layout.close_button.origin.x + (layout.close_button.size.x - close_icon_size) / 2.0,
                layout.close_button.origin.y + (layout.close_button.size.y - close_icon_size) / 2.0,
            ),
            close_icon_size,
            theme.tokens.muted_foreground,
            SemanticIcon::Close.stroke_width(),
        );
        painter.stroke_line(
            Point2D::new(layout.header.origin.x, layout.header.max_y()),
            Point2D::new(layout.header.max_x(), layout.header.max_y()),
            theme.tokens.border,
            1.0,
        );

        let Some(session) = state.current_session.as_ref() else {
            paint_status(painter, layout.content, "选择任务以查看变更", theme);
            painter.restore();
            return;
        };
        let diff = state
            .presentation
            .sessions
            .get(session)
            .map(|presentation| &presentation.diff.load);
        match diff {
            None | Some(LoadState::Idle) => {
                paint_status(painter, layout.content, "变更尚未加载", theme)
            }
            Some(LoadState::Loading) => paint_status(painter, layout.content, "变更加载中", theme),
            Some(LoadState::Failed(error)) => paint_status(
                painter,
                layout.content,
                &format!("变更加载失败：{error}"),
                theme,
            ),
            Some(LoadState::Ready(snapshot)) => {
                let (additions, deletions) =
                    snapshot.files.iter().fold((0_u64, 0_u64), |totals, file| {
                        (
                            totals.0 + u64::from(file.additions),
                            totals.1 + u64::from(file.deletions),
                        )
                    });
                let summary = format!("{} 个文件  +{additions} -{deletions}", snapshot.files.len());
                draw_ui_text(
                    painter,
                    &summary,
                    Point2D::new(layout.header.origin.x + 64.0, layout.header.origin.y + 29.0),
                    12.0,
                    450,
                    theme.tokens.muted_foreground,
                );
                Self::paint(painter, layout.content, snapshot, 0.0, theme);
                if snapshot.files.is_empty() {
                    paint_status(painter, layout.content, "没有变更", theme);
                }
            }
        }
        painter.restore();
    }

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
        session: SessionLocator,
        relative_path: impl Into<String>,
    ) -> AppCommand {
        AppCommand::PreviewWorkspaceFile {
            session,
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
        const LINE_HEIGHT: f32 = 24.0;
        let file_rect = Rect::xywh(
            rect.origin.x,
            rect.origin.y,
            FILE_LIST_WIDTH.min(rect.size.x),
            rect.size.y,
        );
        painter.fill_rect(file_rect, theme.tokens.muted);
        painter.save();
        painter.clip_rect(file_rect);
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
        painter.restore();

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
                Point2D::new(code_rect.origin.x + 8.0, y + 5.0),
                10.0,
                400,
                theme.tokens.muted_foreground,
            );
            draw_text(
                painter,
                &line.text,
                Point2D::new(code_rect.origin.x + 82.0, y + 5.0),
                11.0,
                400,
                theme.tokens.foreground,
            );
        }
        painter.restore();
    }
}

fn paint_status(painter: &mut dyn Painter, rect: Rect, status: &str, theme: &ZodeTheme) {
    draw_ui_text(
        painter,
        status,
        Point2D::new(rect.origin.x + 20.0, rect.origin.y + 30.0),
        13.0,
        450,
        theme.tokens.muted_foreground,
    );
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

fn draw_ui_text(
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

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
