//! Right session sidebar: current session metadata plus one row per open tab.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use zode_core::TodoItem;

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
    /// Active tab's cached todo snapshot, rendered in the foot box.
    pub todos: &'a [TodoItem],
    /// Whether the active tab has a turn in flight (drives the box title).
    pub busy: bool,
    /// Active tab's Task-spawned sub-agents (newest first), rendered as their
    /// own section beneath the sessions list.
    pub subagents: &'a [zode_core::SubAgent],
    /// The active goal (`/goal`) while its auto-loop runs — shown as its own
    /// `goal` section. `None` hides the section.
    pub goal: Option<&'a str>,
    /// How long the goal loop has been running (pre-formatted, e.g. `2m 05s`).
    pub goal_elapsed: Option<String>,
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

pub(crate) fn truncate_to_width(text: &str, max_width: usize) -> String {
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
    // The todo box sits directly beneath the sessions list (not pinned to the
    // foot), with a one-row gap. `todo_box_height` returns 0 when the sidebar
    // is too short to host both the summary/sessions block and the box.
    const GAP: u16 = 1;
    let box_h = crate::ui::todo::todo_box_height(info.todos.len(), area.height as usize) as u16;
    let show_box = box_h > 0 && box_h + GAP < area.height && area.width >= 4;

    let row_width = area.width.saturating_sub(1) as usize;
    // The "subagents" section flows beneath the tabs (empty → no-op); the todo
    // box is the carved foot box. Reserve room for both so the tab list yields.
    let sub_h = crate::ui::subagents_sidebar::section_height(info.subagents.len());
    let todo_foot = if show_box { (box_h + GAP) as usize } else { 0 };

    let mut lines = sidebar_summary_lines(&info, row_width)
        .into_iter()
        .map(|(is_header, line)| styled_sidebar_line(is_header, &line, row_width, theme))
        .collect::<Vec<_>>();
    lines.push(header_line(row_width, theme));
    // Cap the tab list so the subagents section AND the todo foot box fit.
    let tabs_budget = (area.height as usize).saturating_sub(todo_foot + sub_h);
    append_tab_rows(&mut lines, row_width, tabs_budget, tabs, active, theme);
    // Flow the subagents section right after the tabs (no-op when empty).
    lines.extend(crate::ui::subagents_sidebar::section_lines(
        info.subagents,
        row_width,
        theme,
    ));
    let content_rows = lines.len() as u16;
    render_sidebar_block(f, area, lines, theme);

    if show_box {
        let y = area.y + content_rows + GAP;
        let h = box_h.min(area.height.saturating_sub(content_rows + GAP));
        if h >= 2 {
            // Inset by one column so the sidebar's left rail (drawn by the top
            // block) stays continuous through the box, and the box content
            // lines up with the bordered sections above it.
            let b = Rect::new(area.x + 1, y, area.width.saturating_sub(1), h);
            crate::ui::todo::render_todo_box(f, b, info.todos, info.busy, theme);
        }
    }
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
    let start = tab_window_start(active, tabs.len(), remaining_rows);
    for (i, tab) in tabs.iter().enumerate().skip(start).take(remaining_rows) {
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

fn tab_window_start(active: usize, total: usize, visible_rows: usize) -> usize {
    if total <= visible_rows || visible_rows == 0 {
        return 0;
    }
    let active = active.min(total - 1);
    active
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(total - visible_rows)
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

/// Returns `(is_header, text)` rows: header rows get the accent color, others
/// are values/blanks. Marking headers explicitly (rather than matching English
/// text) lets the header labels be translated without losing their styling.
fn sidebar_summary_lines(info: &SidebarInfo<'_>, width: usize) -> Vec<(bool, String)> {
    use crate::tr;
    let flags = match (info.yolo, info.sandbox) {
        (true, true) => format!("yolo · {}", tr("sandbox")),
        (true, false) => "yolo".to_string(),
        (false, true) => tr("sandbox").to_string(),
        (false, false) => tr("standard").to_string(),
    };
    let hdr = |s: &str| (true, sidebar_line(s, width));
    let val = |s: &str| (false, sidebar_line(s, width));
    let blank = (false, String::new());
    let mut lines = vec![
        hdr(tr("session")),
        val(info.session_title),
        blank.clone(),
        hdr(tr("context")),
        val(&format!(
            "↑{} ↓{} {}",
            info.input_tokens,
            info.output_tokens,
            tr("tokens")
        )),
        val(&format!("{} {}", tr("cost"), info.cost_label)),
        val(&format!("{} · {}", info.mode, flags)),
        blank.clone(),
    ];
    // Goal auto-loop: the objective + how long it's been running.
    if let Some(goal) = info.goal {
        lines.push(hdr(tr("goal")));
        lines.push(val(goal));
        if let Some(elapsed) = &info.goal_elapsed {
            lines.push(val(&format!("{} · {}", tr("looping"), elapsed)));
        }
        lines.push(blank.clone());
    }
    lines.extend([
        hdr(tr("model")),
        val(info.model),
        val(&format!("{} {}", tr("theme"), info.theme_name)),
        blank.clone(),
        hdr(tr("workspace")),
        val(&compact_path(info.cwd)),
        blank,
    ]);
    lines
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

fn styled_sidebar_line(is_header: bool, line: &str, width: usize, theme: &Theme) -> Line<'static> {
    let text = pad_to_width(line, width);
    let style = if is_header {
        Style::default()
            .fg(theme.accent)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD)
    } else if text.trim().is_empty() {
        Style::default().bg(theme.bg_secondary)
    } else {
        Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
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
            todos: &[],
            busy: false,
            subagents: &[],
            goal: None,
            goal_elapsed: None,
        };
        let lines = sidebar_summary_lines(&info, 34);
        let joined = lines
            .into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>()
            .join("\n");
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

    #[test]
    fn sidebar_renders_todo_box_with_items() {
        use zode_core::{TodoItem, TodoStatus};
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let todos = vec![
            TodoItem {
                subject: "read tabs.rs".into(),
                description: None,
                status: TodoStatus::Completed,
                id: None,
            },
            TodoItem {
                subject: "wire snapshot".into(),
                description: None,
                status: TodoStatus::InProgress,
                id: None,
            },
        ];
        let info = SidebarInfo {
            session_title: "s",
            theme_name: "Minimal",
            model: "m",
            cwd: std::path::Path::new("/tmp"),
            mode: "ready",
            input_tokens: 0,
            output_tokens: 0,
            cost_label: "$0.00",
            yolo: false,
            sandbox: false,
            todos: &todos,
            busy: true,
            subagents: &[],
            goal: None,
            goal_elapsed: None,
        };
        let backend = ratatui::backend::TestBackend::new(34, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_sidebar(f, f.area(), &[], 0, info, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Todo"));
        assert!(content.contains("running…"));
        assert!(content.contains("wire snapshot"));
        assert!(content.contains("1/2"));

        // The box must sit just beneath the sessions list, NOT pinned to the
        // foot. With 0 tabs the summary+header is 16 rows, so the box opens
        // around row 17 — assert its content lands in the upper half and the
        // bottom rows stay empty.
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(34)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        let box_row = rows
            .iter()
            .position(|r| r.contains("wire snapshot"))
            .expect("todo item should be rendered");
        assert!(
            box_row < 20,
            "todo box should sit under sessions, not at the foot (row {box_row})"
        );
        assert!(
            rows[30..].iter().all(|r| !r.contains("wire snapshot")),
            "todo box must not render in the foot rows"
        );
    }

    #[test]
    fn sidebar_renders_subagents_in_their_own_section_below_sessions() {
        use zode_core::{SubAgent, SubAgentStatus};
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let subagents = vec![SubAgent {
            id: 1,
            agent_type: "researcher".into(),
            description: Some("dig".into()),
            depth: 0,
            status: SubAgentStatus::Running,
            started_at: 0,
            finished_at: None,
            input_tokens: 0,
            output_tokens: 0,
            transcript: Vec::new(),
        }];
        let info = SidebarInfo {
            session_title: "s",
            theme_name: "Minimal",
            model: "m",
            cwd: std::path::Path::new("/tmp"),
            mode: "ready",
            input_tokens: 0,
            output_tokens: 0,
            cost_label: "$0.00",
            yolo: false,
            sandbox: false,
            todos: &[],
            busy: false,
            subagents: &subagents,
            goal: None,
            goal_elapsed: None,
        };
        let backend = ratatui::backend::TestBackend::new(34, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_sidebar(f, f.area(), &[], 0, info, &theme))
            .unwrap();
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(34)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        let sessions_row = rows.iter().position(|r| r.contains("sessions"));
        let subagents_row = rows.iter().position(|r| r.contains("subagents"));
        assert!(sessions_row.is_some(), "sessions header should render");
        assert!(subagents_row.is_some(), "subagents header should render");
        // The sub-agent section is its OWN header, placed below sessions.
        assert!(subagents_row > sessions_row);
        assert!(rows.iter().any(|r| r.contains("researcher")));
    }

    #[test]
    fn tab_window_start_keeps_active_tab_visible() {
        assert_eq!(tab_window_start(0, 8, 4), 0);
        assert_eq!(tab_window_start(3, 8, 4), 0);
        assert_eq!(tab_window_start(4, 8, 4), 1);
        assert_eq!(tab_window_start(7, 8, 4), 4);
        assert_eq!(tab_window_start(7, 8, 12), 0);
    }
}
