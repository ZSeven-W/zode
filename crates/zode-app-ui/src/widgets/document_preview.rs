use jian_widgets::{
    components::markdown::{parse_blocks, parse_inline, MdBlock, MdRun},
    Color, HorizontalAlign, Painter, Point2D, Rect, TextLayout,
};
use zode_app_model::{AppCommand, PreviewKind, PreviewState, ZodeAppState};

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const DOCUMENT_PREVIEW_CLOSE_ID: WidgetId = WidgetId(103);
pub const DOCUMENT_PREVIEW_EXTERNAL_ID: WidgetId = WidgetId(104);
pub const DOCUMENT_PREVIEW_RETRY_ID: WidgetId = WidgetId(105);
pub const DOCUMENT_PREVIEW_CONTENT_ID: WidgetId = WidgetId(106);

const HEADER_HEIGHT: f32 = 46.0;
const BREADCRUMB_HEIGHT: f32 = 30.0;
const TITLE_HEIGHT: f32 = 44.0;
const BUTTON_HEIGHT: f32 = 26.0;
const INSET: f32 = 16.0;
const MARKDOWN_LIMIT: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocumentPreviewLayout {
    pub panel: Rect,
    pub header: Rect,
    pub tab: Rect,
    pub breadcrumb: Rect,
    pub title: Rect,
    pub content: Rect,
    pub close_button: Rect,
    pub external_button: Option<Rect>,
    pub retry_button: Option<Rect>,
    pub close_id: WidgetId,
    pub external_id: WidgetId,
    pub retry_id: WidgetId,
    pub content_id: WidgetId,
}

pub struct DocumentPreview;

impl DocumentPreview {
    pub fn layout(rect: Rect, state: &ZodeAppState) -> DocumentPreviewLayout {
        let width = rect.size.x.max(0.0);
        let height = rect.size.y.max(0.0);
        let header_height = HEADER_HEIGHT.min(height);
        let header = Rect::xywh(rect.origin.x, rect.origin.y, width, header_height);
        let close_size = 24.0_f32.min(width).min(header_height);
        let close_button = Rect::xywh(
            (rect.max_x() - INSET - close_size).max(rect.origin.x),
            rect.origin.y + (header_height - close_size).max(0.0) / 2.0,
            close_size,
            close_size,
        );
        let preview = current_preview(state);
        let show_external = matches!(
            preview,
            Some(PreviewState::Ready { .. } | PreviewState::Failed { .. })
        );
        let show_retry = matches!(preview, Some(PreviewState::Failed { .. }));
        let external_button = show_external.then(|| {
            let button_width = 86.0_f32.min((close_button.origin.x - rect.origin.x).max(0.0));
            Rect::xywh(
                (close_button.origin.x - 8.0 - button_width).max(rect.origin.x),
                rect.origin.y + (header_height - BUTTON_HEIGHT).max(0.0) / 2.0,
                button_width,
                BUTTON_HEIGHT.min(header_height),
            )
        });
        let retry_button = if show_retry {
            external_button.map(|external| {
                let button_width = 58.0_f32.min((external.origin.x - rect.origin.x).max(0.0));
                Rect::xywh(
                    (external.origin.x - 8.0 - button_width).max(rect.origin.x),
                    external.origin.y,
                    button_width,
                    external.size.y,
                )
            })
        } else {
            None
        };
        let controls_left = retry_button
            .or(external_button)
            .map_or(close_button.origin.x, |button| button.origin.x);
        let tab_x = rect.origin.x + 8.0_f32.min(width);
        let tab = Rect::xywh(
            tab_x,
            rect.origin.y + 6.0_f32.min(header_height),
            (controls_left - 8.0 - tab_x).clamp(0.0, 220.0),
            34.0_f32.min(header_height),
        );
        let breadcrumb_y = header.max_y();
        let breadcrumb_height = BREADCRUMB_HEIGHT.min((rect.max_y() - breadcrumb_y).max(0.0));
        let breadcrumb = Rect::xywh(
            rect.origin.x + INSET.min(width),
            breadcrumb_y,
            (width - INSET * 2.0).max(0.0),
            breadcrumb_height,
        );
        let title_y = breadcrumb.max_y();
        let title_height = TITLE_HEIGHT.min((rect.max_y() - title_y).max(0.0));
        let title = Rect::xywh(
            rect.origin.x + INSET.min(width),
            title_y,
            (width - INSET * 2.0).max(0.0),
            title_height,
        );
        let content_y = title.max_y();
        let content = Rect::xywh(
            rect.origin.x + INSET.min(width),
            content_y,
            (width - INSET * 2.0).max(0.0),
            (rect.max_y() - content_y - INSET.min(height)).max(0.0),
        );
        DocumentPreviewLayout {
            panel: rect,
            header,
            tab,
            breadcrumb,
            title,
            content,
            close_button,
            external_button,
            retry_button,
            close_id: DOCUMENT_PREVIEW_CLOSE_ID,
            external_id: DOCUMENT_PREVIEW_EXTERNAL_ID,
            retry_id: DOCUMENT_PREVIEW_RETRY_ID,
            content_id: DOCUMENT_PREVIEW_CONTENT_ID,
        }
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        if id == DOCUMENT_PREVIEW_CLOSE_ID {
            return Some(AppCommand::CloseSecondary);
        }
        let session = state.current_session.as_ref()?.clone();
        let target = current_preview(state)?.target()?.clone();
        if state.available_workspace_for_session(&session) != Some(&target.workspace_uri) {
            return None;
        }
        match id {
            DOCUMENT_PREVIEW_EXTERNAL_ID => Some(AppCommand::OpenPreviewExternally {
                session,
                relative_path: target.relative_path,
            }),
            DOCUMENT_PREVIEW_RETRY_ID => Some(AppCommand::PreviewWorkspaceFile {
                session,
                relative_path: target.relative_path,
            }),
            _ => None,
        }
    }

    pub fn paint(painter: &mut dyn Painter, rect: Rect, state: &ZodeAppState, theme: &ZodeTheme) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let layout = Self::layout(rect, state);
        painter.fill_rect(rect, theme.tokens.background);
        painter.save();
        painter.clip_rect(rect);
        if layout.tab.size.x > 0.0 && layout.tab.size.y > 0.0 {
            painter.fill_round_rect(layout.tab, 8.0, theme.tokens.muted);
            painter.stroke_round_rect(layout.tab, 8.0, theme.tokens.border, 1.0);
            let icon_slot = Rect::xywh(
                layout.tab.origin.x + 8.0,
                layout.tab.origin.y,
                14.0_f32.min(layout.tab.size.x),
                layout.tab.size.y,
            );
            let icon = paint_centered_icon(
                painter,
                SemanticIcon::FileText,
                icon_slot,
                14.0,
                theme.tokens.muted_foreground,
            );
            let label_x = icon.max_x() + 6.0;
            paint_single_line(
                painter,
                &Self::tab_label(state),
                Rect::xywh(
                    label_x,
                    layout.tab.origin.y,
                    (layout.tab.max_x() - label_x - 8.0).max(0.0),
                    layout.tab.size.y,
                ),
                12.0,
                550,
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
        }
        paint_centered_icon(
            painter,
            SemanticIcon::Close,
            layout.close_button,
            14.0,
            theme.tokens.muted_foreground,
        );
        if let Some(button) = layout.external_button {
            paint_icon_button(
                painter,
                SemanticIcon::ExternalOpen,
                "外部打开",
                button,
                theme,
            );
        }
        if let Some(button) = layout.retry_button {
            paint_icon_button(painter, SemanticIcon::Refresh, "重试", button, theme);
        }
        painter.stroke_line(
            Point2D::new(layout.header.origin.x, layout.header.max_y()),
            Point2D::new(layout.header.max_x(), layout.header.max_y()),
            theme.tokens.border,
            1.0,
        );

        match current_preview(state) {
            None | Some(PreviewState::Idle) => {
                paint_status(painter, layout.content, "选择工作区文件以预览", theme)
            }
            Some(PreviewState::Loading { target }) => {
                paint_path_and_title(painter, &layout, &target.relative_path, "加载中", theme);
                paint_status(painter, layout.content, "正在读取文件…", theme);
            }
            Some(PreviewState::Failed { target, message }) => {
                paint_path_and_title(
                    painter,
                    &layout,
                    &target.relative_path,
                    file_name(&target.relative_path),
                    theme,
                );
                paint_status(
                    painter,
                    layout.content,
                    &format!("无法预览：{message}"),
                    theme,
                );
            }
            Some(PreviewState::Ready {
                target,
                title,
                content,
                kind,
            }) => {
                paint_path_and_title(painter, &layout, &target.relative_path, title, theme);
                painter.save();
                painter.clip_rect(layout.content);
                match kind {
                    PreviewKind::Markdown => {
                        paint_markdown(painter, layout.content, content, theme)
                    }
                    PreviewKind::PlainText => {
                        paint_plain_text(painter, layout.content, content, theme)
                    }
                }
                painter.restore();
            }
        }
        painter.restore();
    }

    pub fn tab_label(state: &ZodeAppState) -> String {
        match current_preview(state) {
            Some(PreviewState::Ready { title, .. }) if !title.trim().is_empty() => title.clone(),
            Some(PreviewState::Loading { target } | PreviewState::Failed { target, .. }) => {
                file_name(&target.relative_path).to_owned()
            }
            Some(PreviewState::Ready { target, .. }) => file_name(&target.relative_path).to_owned(),
            None | Some(PreviewState::Idle) => "文档预览".into(),
        }
    }

    pub(crate) fn current_state(state: &ZodeAppState) -> Option<&PreviewState> {
        current_preview(state)
    }
}

pub(crate) fn current_preview(state: &ZodeAppState) -> Option<&PreviewState> {
    let session = state.current_session.as_ref()?;
    let workspace = state.available_workspace_for_session(session)?;
    state
        .presentation
        .sessions
        .get(session)
        .map(|presentation| &presentation.preview)
        .filter(|preview| {
            preview
                .target()
                .is_none_or(|target| &target.workspace_uri == workspace)
        })
}

fn file_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(path)
}

fn paint_centered_icon(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    bounds: Rect,
    size: f32,
    color: Color,
) -> Rect {
    let size = size.min(bounds.size.x.max(0.0)).min(bounds.size.y.max(0.0));
    let icon_rect = Rect::xywh(
        bounds.origin.x + (bounds.size.x - size).max(0.0) / 2.0,
        bounds.origin.y + (bounds.size.y - size).max(0.0) / 2.0,
        size,
        size,
    );
    painter.stroke_svg_path(
        icon.path(),
        icon_rect.origin,
        icon_rect.size.x,
        color,
        icon.stroke_width(),
    );
    icon_rect
}

fn paint_icon_button(
    painter: &mut dyn Painter,
    icon: SemanticIcon,
    label: &str,
    button: Rect,
    theme: &ZodeTheme,
) {
    painter.stroke_round_rect(button, 8.0, theme.tokens.border, 1.0);
    let icon_size = 12.0_f32.min(button.size.y.max(0.0));
    let gap = 4.0;
    let label_width = painter.measure_text_weighted(label, 11.0, 500);
    let group_width = icon_size + gap + label_width;
    let start_x = button.origin.x + (button.size.x - group_width).max(0.0) / 2.0;
    let icon_bounds = Rect::xywh(start_x, button.origin.y, icon_size, button.size.y);
    let icon_rect = paint_centered_icon(
        painter,
        icon,
        icon_bounds,
        icon_size,
        theme.tokens.foreground,
    );
    let label_x = icon_rect.max_x() + gap;
    paint_single_line(
        painter,
        label,
        Rect::xywh(
            label_x,
            button.origin.y,
            (button.max_x() - label_x).max(0.0),
            button.size.y,
        ),
        11.0,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

fn paint_path_and_title(
    painter: &mut dyn Painter,
    layout: &DocumentPreviewLayout,
    path: &str,
    title: &str,
    theme: &ZodeTheme,
) {
    paint_single_line(
        painter,
        path,
        layout.breadcrumb,
        10.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        title,
        layout.title,
        18.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
}

fn paint_status(painter: &mut dyn Painter, rect: Rect, message: &str, theme: &ZodeTheme) {
    draw_text(
        painter,
        message,
        Point2D::new(rect.origin.x, rect.origin.y + 20.0),
        "system-ui",
        12.0,
        400,
        theme.tokens.muted_foreground,
    );
}

fn paint_markdown(painter: &mut dyn Painter, rect: Rect, markdown: &str, theme: &ZodeTheme) {
    let mut y = rect.origin.y + 18.0;
    let max_chars = chars_per_line(rect.size.x, 13.0);
    for block in parse_blocks(markdown, MARKDOWN_LIMIT) {
        if y > rect.max_y() {
            break;
        }
        match block {
            MdBlock::Heading { level, text } => {
                let size = if level <= 2 { 18.0 } else { 15.0 };
                let lines = paint_inline_lines(
                    painter,
                    &text,
                    Point2D::new(rect.origin.x, y),
                    chars_per_line(rect.size.x, size),
                    size,
                    size + 6.0,
                    650,
                    rect.max_y(),
                    theme,
                );
                y += lines as f32 * (size + 6.0) + 3.0;
            }
            MdBlock::Bullet(text) => {
                draw_text(
                    painter,
                    "•",
                    Point2D::new(rect.origin.x, y),
                    "system-ui",
                    13.0,
                    600,
                    theme.zode_purple,
                );
                let lines = paint_inline_lines(
                    painter,
                    &text,
                    Point2D::new(rect.origin.x + 16.0, y),
                    chars_per_line((rect.size.x - 16.0).max(0.0), 13.0),
                    13.0,
                    20.0,
                    400,
                    rect.max_y(),
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
            MdBlock::Paragraph(text) => {
                let lines = paint_inline_lines(
                    painter,
                    &text,
                    Point2D::new(rect.origin.x, y),
                    max_chars,
                    13.0,
                    20.0,
                    400,
                    rect.max_y(),
                    theme,
                );
                y += lines as f32 * 20.0 + 4.0;
            }
        }
    }
}

fn paint_plain_text(painter: &mut dyn Painter, rect: Rect, content: &str, theme: &ZodeTheme) {
    let max_chars = chars_per_line(rect.size.x, 12.0);
    let mut y = rect.origin.y + 18.0;
    for line in content.lines() {
        let remaining_lines = visible_line_budget(y, rect.max_y(), 19.0);
        for segment in hard_wrap_text_limited(line, max_chars, remaining_lines) {
            if y > rect.max_y() {
                return;
            }
            draw_text(
                painter,
                &segment,
                Point2D::new(rect.origin.x, y),
                "monospace",
                12.0,
                400,
                theme.tokens.foreground,
            );
            y += 19.0;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_inline_lines(
    painter: &mut dyn Painter,
    source: &str,
    origin: Point2D,
    max_chars: usize,
    size: f32,
    line_height: f32,
    base_weight: u16,
    max_y: f32,
    theme: &ZodeTheme,
) -> usize {
    let max_lines = visible_line_budget(origin.y, max_y, line_height);
    let char_budget = max_chars.max(1).saturating_mul(max_lines.max(1));
    let bounded = char_prefix(source, char_budget);
    let lines = hard_wrap_runs_limited(&parse_inline(bounded), max_chars, max_lines);
    let mut painted = 0;
    for (line_index, line) in lines.iter().enumerate() {
        let y = origin.y + line_index as f32 * line_height;
        if y > max_y {
            break;
        }
        let mut x = origin.x;
        for run in line {
            let (weight, color) = match run {
                MdRun::Bold(_) => (650, theme.tokens.foreground),
                MdRun::Code(_) | MdRun::Color(_) => (500, theme.zode_purple),
                MdRun::Plain(_) => (base_weight, theme.tokens.foreground),
            };
            let text = run.text();
            draw_text(
                painter,
                text,
                Point2D::new(x, y),
                "system-ui",
                size,
                weight,
                color,
            );
            x += painter.measure_text_weighted(text, size, weight);
        }
        painted += 1;
    }
    painted.max(1)
}

fn chars_per_line(width: f32, size: f32) -> usize {
    (width.max(0.0) / size.max(1.0)).floor().max(1.0) as usize
}

#[cfg(test)]
fn hard_wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    hard_wrap_text_limited(text, max_chars, usize::MAX)
}

fn hard_wrap_text_limited(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    if max_lines == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0;
    for character in text.chars() {
        if line_len == max_chars {
            lines.push(std::mem::take(&mut line));
            if lines.len() == max_lines {
                return lines;
            }
            line_len = 0;
        }
        line.push(character);
        line_len += 1;
    }
    if (!line.is_empty() || lines.is_empty()) && lines.len() < max_lines {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
fn hard_wrap_runs(runs: &[MdRun], max_chars: usize) -> Vec<Vec<MdRun>> {
    hard_wrap_runs_limited(runs, max_chars, usize::MAX)
}

fn hard_wrap_runs_limited(runs: &[MdRun], max_chars: usize, max_lines: usize) -> Vec<Vec<MdRun>> {
    let max_chars = max_chars.max(1);
    if max_lines == 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut line = Vec::new();
    let mut used = 0;
    for run in runs {
        let mut segment = String::new();
        for character in run.text().chars() {
            if used == max_chars {
                append_run(&mut line, run, &segment);
                segment.clear();
                result.push(std::mem::take(&mut line));
                if result.len() == max_lines {
                    return result;
                }
                used = 0;
            }
            segment.push(character);
            used += 1;
        }
        append_run(&mut line, run, &segment);
    }
    if !line.is_empty() && result.len() < max_lines {
        result.push(line);
    }
    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

fn visible_line_budget(y: f32, max_y: f32, line_height: f32) -> usize {
    if !y.is_finite() || !max_y.is_finite() || y > max_y {
        return 0;
    }
    (((max_y - y) / line_height.max(1.0)).floor() as usize).saturating_add(1)
}

fn char_prefix(source: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match source.char_indices().nth(max_chars) {
        Some((index, _)) => &source[..index],
        None => source,
    }
}

fn append_run(line: &mut Vec<MdRun>, style: &MdRun, text: &str) {
    if text.is_empty() {
        return;
    }
    let run = match style {
        MdRun::Plain(_) => MdRun::Plain(text.to_owned()),
        MdRun::Bold(_) => MdRun::Bold(text.to_owned()),
        MdRun::Code(_) => MdRun::Code(text.to_owned()),
        MdRun::Color(_) => MdRun::Color(text.to_owned()),
    };
    line.push(run);
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    family: &str,
    size: f32,
    weight: u16,
    color: Color,
) {
    let layout = TextLayout::single_run(text, family, size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_wrapping_bounds_plain_and_styled_unbroken_text() {
        let plain = hard_wrap_text("abcdefghijklmnop", 5);
        assert_eq!(plain, ["abcde", "fghij", "klmno", "p"]);

        let styled = hard_wrap_runs(&parse_inline("**abcdefghijklmnop**"), 5);
        assert!(styled.iter().all(|line| {
            line.iter()
                .map(|run| run.text().chars().count())
                .sum::<usize>()
                <= 5
        }));
        assert_eq!(
            styled
                .iter()
                .flat_map(|line| line.iter())
                .map(MdRun::text)
                .collect::<String>(),
            "abcdefghijklmnop"
        );

        let huge = "界".repeat(1_048_576);
        let visible = hard_wrap_text_limited(&huge, 80, 12);
        assert_eq!(visible.len(), 12);
        assert!(visible.iter().all(|line| line.chars().count() == 80));
        assert_eq!(char_prefix(&huge, 7), "界界界界界界界");
    }
}
