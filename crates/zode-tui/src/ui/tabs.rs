//! Right session sidebar: current session metadata plus one row per open tab.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tab::SessionTab;
use crate::theme::Theme;
use crate::ui::layout::compact_path;

pub struct SidebarInfo<'a> {
    pub session_title: &'a str,
    pub theme_name: &'a str,
    pub model: &'a str,
    pub cwd: &'a std::path::Path,
    pub mode: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_label: &'a str,
    pub yolo: bool,
    pub sandbox: bool,
}

pub fn tab_label(index: usize, title: &str, busy: bool) -> String {
    if busy {
        format!("{index} ● {title}")
    } else {
        format!("{index} {title}")
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TabRowParts {
    index: String,
    status: String,
    title: String,
    padding: String,
}

#[cfg(test)]
fn format_tab_row(index: usize, title: &str, busy: bool, width: usize) -> String {
    let parts = tab_row_parts(index, title, busy, width);
    format!(
        "{}{}{}{}",
        parts.index, parts.status, parts.title, parts.padding
    )
}

fn tab_row_parts(index: usize, title: &str, busy: bool, width: usize) -> TabRowParts {
    let index = format!("{index:>2}");
    let status = if busy {
        " ● ".to_string()
    } else {
        "   ".to_string()
    };
    let prefix_width =
        UnicodeWidthStr::width(index.as_str()) + UnicodeWidthStr::width(status.as_str());
    let title_width = width.saturating_sub(prefix_width);
    let title = truncate_to_width(title, title_width);
    let used = prefix_width + UnicodeWidthStr::width(title.as_str());
    let padding = " ".repeat(width.saturating_sub(used));

    TabRowParts {
        index,
        status,
        title,
        padding,
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    let width = UnicodeWidthStr::width(text);
    if width <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut used = 0;
    let body_width = max_width.saturating_sub(1);
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > body_width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

pub fn render_tabs(f: &mut Frame, area: Rect, tabs: &[SessionTab], active: usize, theme: &Theme) {
    render_tab_list(f, area, tabs, active, theme, 0);
}

pub fn render_sidebar(
    f: &mut Frame,
    area: Rect,
    tabs: &[SessionTab],
    active: usize,
    info: SidebarInfo<'_>,
    theme: &Theme,
) {
    let row_width = area.width.saturating_sub(1) as usize;
    let mut lines = sidebar_summary_lines(&info, row_width)
        .into_iter()
        .map(|line| styled_sidebar_line(&line, row_width, theme))
        .collect::<Vec<_>>();
    lines.push(header_line(row_width, theme));
    append_tab_rows(
        &mut lines,
        row_width,
        area.height as usize,
        tabs,
        active,
        theme,
    );
    render_sidebar_block(f, area, lines, theme);
}

fn render_tab_list(
    f: &mut Frame,
    area: Rect,
    tabs: &[SessionTab],
    active: usize,
    theme: &Theme,
    top_padding: usize,
) {
    let row_width = area.width.saturating_sub(1) as usize;
    let mut lines = Vec::new();
    lines.extend((0..top_padding).map(|_| Line::from("")));
    lines.push(header_line(row_width, theme));
    append_tab_rows(
        &mut lines,
        row_width,
        area.height as usize,
        tabs,
        active,
        theme,
    );
    render_sidebar_block(f, area, lines, theme);
}

fn append_tab_rows(
    lines: &mut Vec<Line<'static>>,
    row_width: usize,
    area_height: usize,
    tabs: &[SessionTab],
    active: usize,
    theme: &Theme,
) {
    let content_width = row_width.saturating_sub(2);
    let remaining_rows = area_height.saturating_sub(lines.len());
    for (i, tab) in tabs.iter().enumerate().take(remaining_rows) {
        let row_active = i == active;
        let row_bg = if row_active {
            theme.bg_input
        } else {
            theme.bg_secondary
        };
        let marker_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.bg_secondary)
        };
        let index_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.fg_subtle)
        };
        let title_style = if row_active {
            Style::default()
                .bg(row_bg)
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.fg_text)
        };
        let parts = tab_row_parts(i + 1, &tab.title, tab.is_busy(), content_width);
        lines.push(Line::from(vec![
            Span::styled(if row_active { "▌" } else { " " }, marker_style),
            Span::styled(" ", Style::default().bg(row_bg)),
            Span::styled(parts.index, index_style),
            Span::styled(
                parts.status,
                Style::default().bg(row_bg).fg(theme.accent_secondary),
            ),
            Span::styled(parts.title, title_style),
            Span::styled(parts.padding, Style::default().bg(row_bg)),
        ]));
    }
}

fn render_sidebar_block(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, theme: &Theme) {
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(theme.separator)),
            )
            .style(Style::default().bg(theme.bg_secondary)),
        area,
    );
}

fn sidebar_summary_lines(info: &SidebarInfo<'_>, width: usize) -> Vec<String> {
    let flags = match (info.yolo, info.sandbox) {
        (true, true) => "yolo · sandbox".to_string(),
        (true, false) => "yolo".to_string(),
        (false, true) => "sandbox".to_string(),
        (false, false) => "standard".to_string(),
    };
    vec![
        sidebar_line("session", width),
        sidebar_line(info.session_title, width),
        String::new(),
        sidebar_line("context", width),
        sidebar_line(
            &format!("↑{} ↓{} tokens", info.input_tokens, info.output_tokens),
            width,
        ),
        sidebar_line(&format!("cost {}", info.cost_label), width),
        sidebar_line(&format!("{} · {}", info.mode, flags), width),
        String::new(),
        sidebar_line("model", width),
        sidebar_line(info.model, width),
        sidebar_line(&format!("theme {}", info.theme_name), width),
        String::new(),
        sidebar_line("workspace", width),
        sidebar_line(&compact_path(info.cwd), width),
        String::new(),
    ]
}

fn sidebar_line(text: &str, width: usize) -> String {
    let content_width = width.saturating_sub(1);
    let content = truncate_to_width(text, content_width);
    let used = UnicodeWidthStr::width(content.as_str());
    format!(
        " {content}{}",
        " ".repeat(content_width.saturating_sub(used))
    )
}

fn styled_sidebar_line(line: &str, width: usize, theme: &Theme) -> Line<'static> {
    let text = pad_to_width(line, width);
    let trimmed = text.trim();
    let style = match trimmed {
        "session" | "context" | "model" | "workspace" => Style::default()
            .fg(theme.accent)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD),
        "" => Style::default().bg(theme.bg_secondary),
        _ => Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
    };
    Line::from(Span::styled(text, style))
}

fn pad_to_width(text: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(text);
    if used >= width {
        truncate_to_width(text, width)
    } else {
        format!("{text}{}", " ".repeat(width - used))
    }
}

fn header_line(width: usize, theme: &Theme) -> Line<'static> {
    let title = " sessions";
    let padding = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(title)));
    Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(theme.fg_subtle)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(padding, Style::default().bg(theme.bg_secondary)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_label_marks_busy_tabs() {
        assert_eq!(tab_label(1, "main", false), "1 main");
        assert_eq!(tab_label(2, "work", true), "2 ● work");
    }

    #[test]
    fn format_tab_row_truncates_titles_to_inner_width() {
        assert_eq!(
            format_tab_row(12, "very-long-session-title", true, 14),
            "12 ● very-lon…"
        );
    }

    #[test]
    fn sidebar_summary_contains_workspace_context_and_sessions() {
        let info = SidebarInfo {
            session_title: "implement tui sidebar",
            theme_name: "Minimal",
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/Users/kayshen/Workspace/ZSeven-W/zode/target/debug"),
            mode: "ready",
            input_tokens: 120,
            output_tokens: 80,
            cost_label: "$0.0008",
            yolo: false,
            sandbox: true,
        };
        let lines = sidebar_summary_lines(&info, 34);
        let joined = lines.join("\n");
        assert!(joined.contains("session"));
        assert!(joined.contains("implement tui sidebar"));
        assert!(joined.contains("context"));
        assert!(joined.contains("↑120 ↓80"));
        assert!(joined.contains("cost $0.0008"));
        assert!(joined.contains("deepseek-v4-pro"));
        assert!(joined.contains("Minimal"));
        assert!(joined.contains("target/debug"));
        assert!(joined.contains("sandbox"));
    }
}
