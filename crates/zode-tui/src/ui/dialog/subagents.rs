//! Full-screen sub-agent activity overlay (`/subagents`, F2): a left list of
//! the active tab's sub-agents and a right transcript of the selected one.
//! Row/transcript formatting is pure so it can be unit-tested without a Frame.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use zode_core::{SubAgent, SubAgentLine, SubAgentStatus};

use crate::theme::Theme;
use crate::ui::centered;

fn status_glyph(s: SubAgentStatus) -> char {
    match s {
        SubAgentStatus::Running => '◐',
        SubAgentStatus::Done => '✓',
        SubAgentStatus::Failed => '✗',
    }
}

/// One left-list row: glyph + depth indent + `type · desc` + elapsed + tokens.
pub(crate) fn list_row(a: &SubAgent, now: u64) -> String {
    let indent = "  ".repeat(a.depth);
    let desc = a
        .description
        .as_deref()
        .map(|d| format!(" · {d}"))
        .unwrap_or_default();
    let end = a.finished_at.unwrap_or(now);
    let elapsed = end.saturating_sub(a.started_at);
    format!(
        "{}{indent}{}{desc} · {}s  ↑{} ↓{}",
        status_glyph(a.status),
        a.agent_type,
        elapsed,
        a.input_tokens,
        a.output_tokens
    )
}

/// Text form of one transcript line. Shared by the render and the tests so the
/// formatting has a single source of truth (and no test-only dead code).
pub(crate) fn line_text(l: &SubAgentLine) -> String {
    match l {
        SubAgentLine::Text(t) => t.clone(),
        SubAgentLine::Thinking(t) => format!("💭 {t}"),
        SubAgentLine::ToolUse { name, input } => format!("🔧 {name}({input})"),
        SubAgentLine::ToolResult { ok, summary } => {
            format!("{} {summary}", if *ok { '✓' } else { '✗' })
        }
        SubAgentLine::Error(e) => format!("✗ {e}"),
        SubAgentLine::Notice(n) => format!("• {n}"),
    }
}

fn line_color(l: &SubAgentLine, theme: &Theme) -> Color {
    match l {
        SubAgentLine::Text(_) => theme.fg_text,
        SubAgentLine::Thinking(_) => theme.fg_subtle,
        SubAgentLine::ToolUse { .. } => theme.accent,
        SubAgentLine::ToolResult { ok, .. } => {
            if *ok {
                theme.fg_subtle
            } else {
                Color::Red
            }
        }
        SubAgentLine::Error(_) => Color::Red,
        SubAgentLine::Notice(_) => theme.fg_subtle,
    }
}

fn transcript_lines(a: &SubAgent, theme: &Theme) -> Vec<Line<'static>> {
    a.transcript
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                line_text(l),
                Style::default().fg(line_color(l, theme)),
            ))
        })
        .collect()
}

pub struct SubAgentsPanel {
    selected_id: Option<u64>,
    scroll: u16,
}

impl Default for SubAgentsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SubAgentsPanel {
    pub fn new() -> Self {
        Self {
            selected_id: None,
            scroll: 0,
        }
    }

    /// Resolve the selected id to an index, defaulting to the first row.
    pub fn selected_index(&self, agents: &[SubAgent]) -> usize {
        self.selected_id
            .and_then(|id| agents.iter().position(|a| a.id == id))
            .unwrap_or(0)
    }

    pub fn select_next(&mut self, agents: &[SubAgent]) {
        if agents.is_empty() {
            return;
        }
        let i = (self.selected_index(agents) + 1).min(agents.len() - 1);
        self.selected_id = Some(agents[i].id);
        self.scroll = 0;
    }

    pub fn select_prev(&mut self, agents: &[SubAgent]) {
        if agents.is_empty() {
            return;
        }
        let i = self.selected_index(agents).saturating_sub(1);
        self.selected_id = Some(agents[i].id);
        self.scroll = 0;
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(3);
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(3);
    }

    pub fn render(
        &mut self,
        f: &mut Frame,
        area: Rect,
        agents: &[SubAgent],
        now: u64,
        theme: &Theme,
    ) {
        let outer = centered(area, 90, 85);
        f.render_widget(Clear, outer);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.separator))
            .title(format!(
                " {}  [↑/↓] {}  [PgUp/PgDn] {}  [Esc] {} ",
                crate::tr("Sub-agents"),
                crate::tr("select"),
                crate::tr("scroll"),
                crate::tr("close")
            ))
            .style(Style::default().bg(theme.bg_secondary));
        // In-tree idiom (see dialog/permission.rs:151): compute the inner rect
        // from the block BEFORE rendering (render_widget moves the block).
        let inner = block.inner(outer);
        f.render_widget(block, outer);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
            .split(inner);

        if agents.is_empty() {
            f.render_widget(
                Paragraph::new(crate::tr("no sub-agents yet"))
                    .style(Style::default().fg(theme.fg_subtle)),
                cols[0],
            );
            return;
        }

        let sel = self.selected_index(agents);
        let items: Vec<ListItem> = agents
            .iter()
            .map(|a| {
                let color = match a.status {
                    SubAgentStatus::Running => theme.accent,
                    SubAgentStatus::Done => theme.fg_subtle,
                    SubAgentStatus::Failed => Color::Red,
                };
                ListItem::new(list_row(a, now)).style(Style::default().fg(color))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(sel));
        f.render_stateful_widget(
            List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
            cols[0],
            &mut state,
        );

        f.render_widget(
            Paragraph::new(transcript_lines(&agents[sel], theme))
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            cols[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::{SubAgent, SubAgentLine, SubAgentStatus};

    fn agent(id: u64, ty: &str, status: SubAgentStatus) -> SubAgent {
        SubAgent {
            id,
            agent_type: ty.into(),
            description: Some("desc".into()),
            depth: 0,
            status,
            started_at: 100,
            finished_at: None,
            input_tokens: 12,
            output_tokens: 7,
            committed_input: 0,
            committed_output: 0,
            turn_input: 0,
            turn_output: 0,
            transcript: vec![
                SubAgentLine::Text("hello".into()),
                SubAgentLine::ToolUse {
                    name: "Read".into(),
                    input: "a.rs".into(),
                },
                SubAgentLine::ToolResult {
                    ok: true,
                    summary: "ok".into(),
                },
            ],
        }
    }

    #[test]
    fn list_row_shows_glyph_type_and_tokens() {
        let a = agent(1, "researcher", SubAgentStatus::Running);
        let row = list_row(&a, 130); // now=130, started=100 -> 30s
        assert!(row.contains("researcher"));
        assert!(row.contains("desc"));
        assert!(row.contains("30s"));
        assert!(row.contains("↑12"));
        assert!(row.contains("↓7"));
        assert!(row.starts_with('◐')); // running glyph
    }

    #[test]
    fn line_text_renders_each_kind() {
        let a = agent(1, "r", SubAgentStatus::Done);
        let lines: Vec<String> = a.transcript.iter().map(line_text).collect();
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "🔧 Read(a.rs)");
        assert_eq!(lines[2], "✓ ok");
        // Cover the remaining variants directly.
        assert_eq!(line_text(&SubAgentLine::Thinking("t".into())), "💭 t");
        assert_eq!(
            line_text(&SubAgentLine::ToolResult {
                ok: false,
                summary: "boom".into()
            }),
            "✗ boom"
        );
        assert_eq!(line_text(&SubAgentLine::Error("e".into())), "✗ e");
        assert_eq!(line_text(&SubAgentLine::Notice("n".into())), "• n");
    }

    #[test]
    fn selection_is_stable_by_id_when_new_agent_arrives() {
        let mut panel = SubAgentsPanel::new();
        let mut agents = vec![agent(2, "b", SubAgentStatus::Running)];
        panel.select_next(&agents); // selects id 2 (only one)
                                    // A newer agent is prepended (newest-first ordering is the caller's).
        agents.insert(0, agent(3, "c", SubAgentStatus::Running));
        // The previously-selected id (2) must still resolve to its row.
        let idx = panel.selected_index(&agents);
        assert_eq!(agents[idx].id, 2);
    }

    #[test]
    fn render_draws_list_and_transcript() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let agents = vec![
            agent(1, "researcher", SubAgentStatus::Running),
            agent(2, "planner", SubAgentStatus::Done),
        ];
        let mut panel = SubAgentsPanel::new();
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| panel.render(f, f.area(), &agents, 130, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("researcher"));
        assert!(content.contains("planner"));
        assert!(content.contains("Read"));
    }
}
