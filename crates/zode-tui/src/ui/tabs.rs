//! Right session sidebar: current session metadata plus one row per open tab.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
    /// Active tab's cached todo snapshot; empty hides the todo section.
    pub todos: &'a [TodoItem],
    /// Whether the active tab has a turn in flight (drives the todo header).
    pub busy: bool,
    /// Fold state of the todo section.
    pub todos_collapsed: bool,
    /// Active tab's Task-spawned sub-agents (newest first), rendered as their
    /// own section beneath the sessions list.
    pub subagents: &'a [zode_core::SubAgent],
    /// The active goal (`/goal`) while its auto-loop runs — shown as its own
    /// `goal` section. `None` hides the section.
    pub goal: Option<&'a str>,
    /// How long the goal loop has been running (pre-formatted, e.g. `2m 05s`).
    pub goal_elapsed: Option<String>,
    /// Configured MCP servers as `(name, connected)`; empty hides the section.
    pub mcp_servers: &'a [(String, bool)],
    /// Fold state of the MCP section (header stays visible when folded).
    pub mcp_collapsed: bool,
    /// Enabled LSP languages as `(language, running)`; empty hides the section.
    pub lsp_servers: &'a [(String, bool)],
    /// Fold state of the LSP section (header stays visible when folded).
    pub lsp_collapsed: bool,
    /// Git working-tree modifications; empty hides the section.
    pub git_files: &'a [zode_core::GitFileStat],
    /// Fold state of the modified-files section.
    pub files_collapsed: bool,
    /// App version for the pinned footer row (e.g. `0.1.0-beta.3`).
    pub version: &'a str,
}

/// Absolute terminal rows of the sidebar's click targets, rebuilt each frame:
/// the collapsible section headers (fold toggles) and the modified-files
/// "…+k more" row (opens the full-list overlay).
#[derive(Debug, Default, Clone, Copy)]
pub struct SidebarHits {
    pub mcp_header_row: Option<u16>,
    pub lsp_header_row: Option<u16>,
    pub files_header_row: Option<u16>,
    pub files_more_row: Option<u16>,
    pub todo_header_row: Option<u16>,
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
) -> SidebarHits {
    let row_width = area.width.saturating_sub(1) as usize;
    // Sections flowing BELOW the tab list (empty → no-op) plus the pinned
    // version row: reserve room so the tab list yields.
    let sub_h = crate::ui::subagents_sidebar::section_height(info.subagents.len());
    let files_h =
        crate::ui::modified_files::section_height(info.git_files.len(), info.files_collapsed);
    let todo_h = crate::ui::todo::section_height(info.todos.len(), info.todos_collapsed);
    let version_foot = 2; // blank gap + the pinned `● zode <version>` row

    let mut hits = SidebarHits::default();
    let header_row = |lines: &Vec<Line<'_>>| {
        // The section's header renders after its leading blank separator.
        let row = area.y + lines.len() as u16 + 1;
        (row < area.y + area.height).then_some(row)
    };

    let mut lines = sidebar_summary_lines(&info, row_width)
        .into_iter()
        .map(|(is_header, line)| styled_sidebar_line(is_header, &line, row_width, theme))
        .collect::<Vec<_>>();
    // The MCP and LSP sections flow between the summary and the sessions
    // list; each renders only while it has items.
    let section_gap = |lines: &mut Vec<Line<'static>>| {
        lines.push(Line::from(Span::styled(
            " ".repeat(row_width),
            Style::default().bg(theme.bg_secondary),
        )));
    };
    if !info.mcp_servers.is_empty() {
        hits.mcp_header_row = header_row(&lines);
        lines.extend(crate::ui::mcp_sidebar::section_lines(
            "MCP",
            info.mcp_servers,
            ("connected", "disconnected"),
            info.mcp_collapsed,
            row_width,
            theme,
        ));
        section_gap(&mut lines);
    }
    if !info.lsp_servers.is_empty() {
        hits.lsp_header_row = header_row(&lines);
        lines.extend(crate::ui::mcp_sidebar::section_lines(
            "LSP",
            info.lsp_servers,
            ("running", "idle"),
            info.lsp_collapsed,
            row_width,
            theme,
        ));
        section_gap(&mut lines);
    }
    lines.push(header_line(row_width, theme));
    // Cap the tab list so everything below it fits.
    let tabs_budget =
        (area.height as usize).saturating_sub(sub_h + files_h + todo_h + version_foot);
    append_tab_rows(&mut lines, row_width, tabs_budget, tabs, active, theme);
    // Flow the subagents section right after the tabs (no-op when empty).
    lines.extend(crate::ui::subagents_sidebar::section_lines(
        info.subagents,
        row_width,
        theme,
    ));
    // Modified files flow beneath subagents; the todo section follows. Both
    // appear only while they have content — nothing is pinned.
    if !info.git_files.is_empty() {
        hits.files_header_row = header_row(&lines);
        let start = lines.len();
        lines.extend(crate::ui::modified_files::section_lines(
            info.git_files,
            info.files_collapsed,
            row_width,
            theme,
        ));
        if let Some(idx) = crate::ui::modified_files::overflow_row_index(
            info.git_files.len(),
            info.files_collapsed,
        ) {
            let row = area.y + (start + idx) as u16;
            hits.files_more_row = (row < area.y + area.height).then_some(row);
        }
    }
    if !info.todos.is_empty() {
        hits.todo_header_row = header_row(&lines);
        lines.extend(crate::ui::todo::section_lines(
            info.todos,
            info.busy,
            info.todos_collapsed,
            row_width,
            theme,
        ));
    }
    let content_rows = lines.len() as u16;
    render_sidebar_block(f, area, lines, theme);
    render_version_row(f, area, content_rows, info.version, theme);
    hits
}

/// The pinned `● zode <version>` footer row, on the sidebar's last line.
/// Skipped when the flowing content already reaches it.
fn render_version_row(f: &mut Frame, area: Rect, occupied: u16, version: &str, theme: &Theme) {
    if area.height < 4 || area.width < 4 {
        return;
    }
    let y = area.y + area.height - 1;
    if y <= area.y + occupied {
        return;
    }
    let row_width = area.width.saturating_sub(1) as usize;
    let text = truncate_to_width(&format!("zode {version}"), row_width.saturating_sub(3));
    let bg = Style::default().bg(theme.bg_secondary);
    let line = Line::from(vec![
        Span::styled(" ● ", bg.fg(Color::Green)),
        Span::styled(text, bg.fg(theme.fg_subtle)),
    ]);
    let strip = Rect::new(area.x + 1, y, area.width.saturating_sub(1), 1);
    f.render_widget(Paragraph::new(line).style(bg), strip);
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
    let title = " Sessions";
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
            todos_collapsed: false,
            subagents: &[],
            goal: None,
            goal_elapsed: None,
            mcp_servers: &[],
            mcp_collapsed: false,
            lsp_servers: &[],
            lsp_collapsed: false,
            git_files: &[],
            files_collapsed: false,
            version: "0.0.0-test",
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
    fn sidebar_renders_todos_as_flowing_collapsible_section() {
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
            todos_collapsed: false,
            subagents: &[],
            goal: None,
            goal_elapsed: None,
            mcp_servers: &[],
            mcp_collapsed: false,
            lsp_servers: &[],
            lsp_collapsed: false,
            git_files: &[],
            files_collapsed: false,
            version: "0.0.0-test",
        };
        let backend = ratatui::backend::TestBackend::new(34, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut hits = SidebarHits::default();
        terminal
            .draw(|f| {
                hits = render_sidebar(f, f.area(), &[], 0, info, &theme);
            })
            .unwrap();
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(34)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        // Flows right beneath the sessions list like the other sections.
        let sessions_row = rows.iter().position(|r| r.contains("Sessions")).unwrap();
        let todo_row = rows
            .iter()
            .position(|r| r.contains("▼ Todo · running…"))
            .expect("todo header should render");
        assert!(todo_row > sessions_row);
        assert!(rows[todo_row].contains("1/2"));
        assert!(rows.iter().any(|r| r.contains("wire snapshot")));
        // The header is a click target for the fold toggle.
        assert_eq!(hits.todo_header_row, Some(todo_row as u16));
    }

    #[test]
    fn sidebar_hides_todo_section_when_empty() {
        let (rows, hits) = draw_rows(info_with_sections(&[], &[], false, false));
        assert!(!rows.iter().any(|r| r.contains("Todo")));
        assert_eq!(hits.todo_header_row, None);
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
            todos_collapsed: false,
            subagents: &subagents,
            goal: None,
            goal_elapsed: None,
            mcp_servers: &[],
            mcp_collapsed: false,
            lsp_servers: &[],
            lsp_collapsed: false,
            git_files: &[],
            files_collapsed: false,
            version: "0.0.0-test",
        };
        let backend = ratatui::backend::TestBackend::new(34, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_sidebar(f, f.area(), &[], 0, info, &theme);
            })
            .unwrap();
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(34)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        let sessions_row = rows.iter().position(|r| r.contains("Sessions"));
        let subagents_row = rows.iter().position(|r| r.contains("subagents"));
        assert!(sessions_row.is_some(), "Sessions header should render");
        assert!(subagents_row.is_some(), "subagents header should render");
        // The sub-agent section is its OWN header, placed below sessions.
        assert!(subagents_row > sessions_row);
        assert!(rows.iter().any(|r| r.contains("researcher")));
    }

    fn info_with_sections<'a>(
        mcp_servers: &'a [(String, bool)],
        git_files: &'a [zode_core::GitFileStat],
        mcp_collapsed: bool,
        files_collapsed: bool,
    ) -> SidebarInfo<'a> {
        SidebarInfo {
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
            todos_collapsed: false,
            subagents: &[],
            goal: None,
            goal_elapsed: None,
            mcp_servers,
            mcp_collapsed,
            lsp_servers: &[],
            lsp_collapsed: false,
            git_files,
            files_collapsed,
            version: "0.1.0-test",
        }
    }

    fn draw_rows(info: SidebarInfo<'_>) -> (Vec<String>, SidebarHits) {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(34, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut hits = SidebarHits::default();
        terminal
            .draw(|f| {
                hits = render_sidebar(f, f.area(), &[], 0, info, &theme);
            })
            .unwrap();
        let rows = terminal
            .backend()
            .buffer()
            .content()
            .chunks(34)
            .map(|r| r.iter().map(|c| c.symbol()).collect())
            .collect();
        (rows, hits)
    }

    #[test]
    fn sidebar_renders_mcp_files_sections_and_version_row() {
        let servers = vec![("chrome-devtools".to_string(), true)];
        let files = vec![zode_core::GitFileStat {
            path: "crates/zode-tui/src/app.rs".into(),
            added: Some(4),
            removed: Some(1),
        }];
        let (rows, hits) = draw_rows(info_with_sections(&servers, &files, false, false));

        let mcp_row = rows.iter().position(|r| r.contains("▼ MCP"));
        let sessions_row = rows.iter().position(|r| r.contains("Sessions"));
        let files_row = rows.iter().position(|r| r.contains("modified files"));
        assert!(mcp_row.is_some(), "MCP header should render");
        assert!(files_row.is_some(), "modified files header should render");
        // MCP flows above the sessions list; modified files below it.
        assert!(mcp_row < sessions_row);
        assert!(files_row > sessions_row);
        assert!(rows.iter().any(|r| r.contains("chrome-devtools")));
        assert!(rows
            .iter()
            .any(|r| r.contains("app.rs") && r.contains("+4")));
        // Version row pinned to the sidebar foot.
        assert!(rows[39].contains("zode 0.1.0-test"));
        // Click hitboxes point at the header rows just rendered.
        assert_eq!(hits.mcp_header_row, mcp_row.map(|r| r as u16));
        assert_eq!(hits.files_header_row, files_row.map(|r| r as u16));
    }

    #[test]
    fn sidebar_renders_lsp_section_only_when_servers_exist() {
        // Present: renders between MCP and the Sessions list with running
        // state, and the header is a fold click target.
        let langs = vec![("rust".to_string(), true), ("python".to_string(), false)];
        let mut info = info_with_sections(&[], &[], false, false);
        info.lsp_servers = &langs;
        let (rows, hits) = draw_rows(info);
        let lsp_row = rows.iter().position(|r| r.contains("▼ LSP"));
        let sessions_row = rows.iter().position(|r| r.contains("Sessions"));
        assert!(lsp_row.is_some(), "LSP header should render");
        assert!(lsp_row < sessions_row);
        assert!(rows.iter().any(|r| r.contains("rust")));
        assert_eq!(hits.lsp_header_row, lsp_row.map(|r| r as u16));

        // Absent: no header, no hitbox.
        let (rows, hits) = draw_rows(info_with_sections(&[], &[], false, false));
        assert!(!rows.iter().any(|r| r.contains("LSP")));
        assert_eq!(hits.lsp_header_row, None);
    }

    #[test]
    fn collapsed_sections_hide_their_rows_but_keep_headers() {
        let servers = vec![("chrome-devtools".to_string(), true)];
        let files = vec![zode_core::GitFileStat {
            path: "crates/zode-tui/src/app.rs".into(),
            added: Some(4),
            removed: Some(1),
        }];
        let (rows, hits) = draw_rows(info_with_sections(&servers, &files, true, true));
        assert!(rows.iter().any(|r| r.contains("▶ MCP")));
        assert!(rows.iter().any(|r| r.contains("▶ modified files")));
        assert!(!rows.iter().any(|r| r.contains("chrome-devtools")));
        assert!(!rows.iter().any(|r| r.contains("app.rs")));
        assert!(hits.mcp_header_row.is_some());
        assert!(hits.files_header_row.is_some());
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
