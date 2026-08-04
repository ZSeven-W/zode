//! Agent manager opened by `/agents`: lists user-defined and built-in
//! sub-agents, and lets the user create a new one (saved to ~/.zode/agents) or
//! delete a user-defined one. Mirrors Claude Code's `/agents` Library view.
//!
//! Two modes: a List (Create-new entry + grouped agents) and a Create form
//! (name / description / system prompt). The app drains a [`AgentsAction`] on
//! confirm and performs the filesystem write/delete + engine reload.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

/// Where an agent comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    User,
    BuiltIn,
}

/// One listed agent.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub name: String,
    pub description: String,
    pub kind: AgentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    /// Manual create form (name / description / system prompt).
    Create,
    /// AI-assisted: a one-field brief the main agent turns into a definition.
    AiBrief,
}

/// What the app should do when the dialog yields control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsAction {
    /// Manual create.
    Create {
        name: String,
        description: String,
        system: String,
    },
    /// AI-assisted create: submit a templated turn so the main agent builds the
    /// agent (via the `DefineAgent` tool) from this brief.
    AiCreate {
        brief: String,
    },
    Delete {
        name: String,
    },
    Close,
}

/// The three editable fields of the create form.
#[derive(Default)]
struct CreateForm {
    name: String,
    description: String,
    system: String,
    field: usize, // 0=name 1=description 2=system
}

pub struct AgentsDialog {
    mode: Mode,
    rows: Vec<AgentRow>,
    /// Selection index into the navigable list: 0 = "have the main agent build
    /// it", 1 = "create manually", then the agent rows.
    selected: usize,
    form: CreateForm,
    /// The AI-assisted brief (Mode::AiBrief).
    brief: String,
}

/// Nav rows before the agent list: the two create entries.
const CREATE_ENTRIES: usize = 2;

impl AgentsDialog {
    /// `rows` should list user agents first, then built-ins.
    pub fn new(rows: Vec<AgentRow>) -> Self {
        Self {
            mode: Mode::List,
            rows,
            selected: 0,
            form: CreateForm::default(),
            brief: String::new(),
        }
    }

    fn nav_len(&self) -> usize {
        self.rows.len() + CREATE_ENTRIES
    }

    pub fn next(&mut self) {
        if self.mode == Mode::List {
            self.selected = (self.selected + 1) % self.nav_len();
        }
    }

    pub fn prev(&mut self) {
        if self.mode == Mode::List {
            let len = self.nav_len();
            self.selected = self.selected.checked_sub(1).unwrap_or(len - 1);
        }
    }

    /// The agent row currently selected (None when on a create entry).
    fn selected_row(&self) -> Option<&AgentRow> {
        self.selected
            .checked_sub(CREATE_ENTRIES)
            .and_then(|i| self.rows.get(i))
    }

    /// Enter on a create entry → switch to that create flow; on an agent row →
    /// no-op (existing user agents are edited via their .md file).
    pub fn on_enter(&mut self) {
        if self.mode != Mode::List {
            return;
        }
        match self.selected {
            0 => {
                self.mode = Mode::AiBrief;
                self.brief.clear();
            }
            1 => {
                self.mode = Mode::Create;
                self.form = CreateForm::default();
            }
            _ => {}
        }
    }

    /// 'd' on a user agent → a Delete action; else None.
    pub fn on_delete(&self) -> Option<AgentsAction> {
        if self.mode != Mode::List {
            return None;
        }
        match self.selected_row() {
            Some(r) if r.kind == AgentKind::User => Some(AgentsAction::Delete {
                name: r.name.clone(),
            }),
            _ => None,
        }
    }

    /// Esc: in a create flow go back to the list; in List mode close.
    pub fn on_esc(&mut self) -> Option<AgentsAction> {
        match self.mode {
            Mode::Create | Mode::AiBrief => {
                self.mode = Mode::List;
                None
            }
            Mode::List => Some(AgentsAction::Close),
        }
    }

    // --- Create-flow input (manual form + AI brief) ---

    pub fn is_create_mode(&self) -> bool {
        self.mode == Mode::Create
    }

    /// Whether the dialog is in a text-input flow (manual form or AI brief).
    pub fn is_input_mode(&self) -> bool {
        matches!(self.mode, Mode::Create | Mode::AiBrief)
    }

    pub fn form_next_field(&mut self) {
        if self.mode == Mode::Create {
            self.form.field = (self.form.field + 1) % 3;
        }
    }

    pub fn form_push(&mut self, c: char) {
        if self.is_input_mode() {
            self.active_field_mut().push(c);
        }
    }

    pub fn form_backspace(&mut self) {
        if self.is_input_mode() {
            self.active_field_mut().pop();
        }
    }

    /// Enter: manual form advances fields then submits; AI brief submits.
    pub fn form_enter(&mut self) -> Option<AgentsAction> {
        match self.mode {
            Mode::Create if self.form.field < 2 => {
                self.form.field += 1;
                None
            }
            Mode::Create | Mode::AiBrief => self.submit(),
            Mode::List => None,
        }
    }

    /// Submit (e.g. Ctrl+S). AI brief → AiCreate (needs a non-empty brief);
    /// manual form → Create (needs a name + system prompt).
    pub fn submit(&mut self) -> Option<AgentsAction> {
        if self.mode == Mode::AiBrief {
            let brief = self.brief.trim().to_string();
            return (!brief.is_empty()).then_some(AgentsAction::AiCreate { brief });
        }
        let name = self.form.name.trim().to_string();
        let system = self.form.system.trim().to_string();
        if name.is_empty() || system.is_empty() {
            return None;
        }
        Some(AgentsAction::Create {
            name,
            description: self.form.description.trim().to_string(),
            system,
        })
    }

    fn active_field_mut(&mut self) -> &mut String {
        if self.mode == Mode::AiBrief {
            return &mut self.brief;
        }
        match self.form.field {
            0 => &mut self.form.name,
            1 => &mut self.form.description,
            _ => &mut self.form.system,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 76, 26);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );
        let inner = inner_area(popup);
        let title = if self.mode == Mode::List {
            crate::tr("Manage agents")
        } else {
            crate::tr("Create new agent")
        };
        f.render_widget(
            header_line(title, inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        let body = Rect::new(
            inner.x,
            inner.y.saturating_add(2),
            inner.width,
            inner.height.saturating_sub(4),
        );
        match self.mode {
            Mode::List => self.render_list(f, body, theme),
            Mode::Create => self.render_form(f, body, theme),
            Mode::AiBrief => self.render_ai_brief(f, body, theme),
        }
        let footer = match self.mode {
            Mode::Create => format!(
                "Tab {}   Enter {}   Esc {}",
                crate::tr("field"),
                crate::tr("save"),
                crate::tr("cancel")
            ),
            Mode::AiBrief => {
                format!("Enter {}   Esc {}", crate::tr("save"), crate::tr("cancel"))
            }
            Mode::List => format!(
                "↑↓   Enter {}   d {}   Esc {}",
                crate::tr("new"),
                crate::tr("delete"),
                crate::tr("cancel")
            ),
        };
        f.render_widget(
            Paragraph::new(footer)
                .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }

    fn render_list(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let mut lines: Vec<Line> = Vec::new();
        // Two create entries: AI-assisted (nav 0) then manual (nav 1).
        lines.push(row_line(
            &format!("› {}  ✦", crate::tr("Create new agent")),
            self.selected == 0,
            theme,
        ));
        lines.push(row_line(
            &format!("› {}  ✎", crate::tr("Create new agent")),
            self.selected == 1,
            theme,
        ));
        lines.push(Line::from(""));
        let mut pushed_user = false;
        let mut pushed_builtin = false;
        for (i, r) in self.rows.iter().enumerate() {
            match r.kind {
                AgentKind::User if !pushed_user => {
                    lines.push(section(crate::tr("User agents"), theme));
                    pushed_user = true;
                }
                AgentKind::BuiltIn if !pushed_builtin => {
                    lines.push(section(crate::tr("Built-in agents"), theme));
                    pushed_builtin = true;
                }
                _ => {}
            }
            let label = format!("  {} · {}", r.name, r.description);
            lines.push(row_line(&label, self.selected == i + CREATE_ENTRIES, theme));
        }
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary)),
            area,
        );
    }

    fn render_ai_brief(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let lines = vec![
            Line::styled(
                crate::tr("Describe what this agent should do; the main agent will create it.")
                    .to_string(),
                Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
            ),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default().fg(theme.accent).bg(theme.bg_secondary),
                ),
                Span::styled(
                    format!("{}▏", self.brief),
                    Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
                ),
            ]),
        ];
        f.render_widget(
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .style(Style::default().bg(theme.bg_secondary)),
            area,
        );
    }

    fn render_form(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let field = |idx: usize, label: &str, value: &str| -> Line<'static> {
            let active = self.form.field == idx;
            let marker = if active { "> " } else { "  " };
            let cursor = if active { "▏" } else { "" };
            Line::from(vec![
                Span::styled(
                    format!("{marker}{label}: "),
                    Style::default()
                        .fg(if active {
                            theme.accent
                        } else {
                            theme.fg_subtle
                        })
                        .bg(theme.bg_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{value}{cursor}"),
                    Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
                ),
            ])
        };
        let lines = vec![
            field(0, crate::tr("name"), &self.form.name),
            Line::from(""),
            field(1, crate::tr("description"), &self.form.description),
            Line::from(""),
            field(2, crate::tr("system prompt"), &self.form.system),
        ];
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary)),
            area,
        );
    }
}

fn row_line(text: &str, selected: bool, theme: &Theme) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(theme.bg_primary)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
    };
    Line::styled(text.to_string(), style)
}

fn section(title: &str, theme: &Theme) -> Line<'static> {
    Line::styled(
        title.to_string(),
        Style::default()
            .fg(theme.accent)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD),
    )
}

fn modal_area(area: Rect, target_w: u16, target_h: u16) -> Rect {
    let max_w = area.width.saturating_sub(6);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(target_w).max(max_w.min(40));
    let height = max_h.min(target_h).max(max_h.min(8));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn inner_area(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(3),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(6),
        height: area.height.saturating_sub(2),
    }
}

fn header_line(title: &str, width: u16, theme: &Theme) -> Paragraph<'static> {
    let title_width = title.chars().count() as u16;
    let gap = width.saturating_sub(title_width.saturating_add(3)) as usize;
    Paragraph::new(Line::from(vec![
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(gap), Style::default().bg(theme.bg_secondary)),
        Span::styled(
            "esc",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<AgentRow> {
        vec![
            AgentRow {
                name: "refactorer".into(),
                description: "Refactors".into(),
                kind: AgentKind::User,
            },
            AgentRow {
                name: "general".into(),
                description: "Focused".into(),
                kind: AgentKind::BuiltIn,
            },
        ]
    }

    #[test]
    fn nav0_is_ai_assisted_nav1_is_manual_form() {
        let mut d = AgentsDialog::new(rows());
        assert!(!d.is_input_mode());
        d.on_enter(); // nav 0 → AI brief
        assert!(d.is_input_mode());
        assert!(!d.is_create_mode());
        d.on_esc(); // back to list
        d.next(); // nav 1 → manual
        d.on_enter();
        assert!(d.is_create_mode());
    }

    #[test]
    fn ai_brief_yields_aicreate() {
        let mut d = AgentsDialog::new(rows());
        d.on_enter(); // nav 0 → AI brief
        for c in "an agent that writes tests".chars() {
            d.form_push(c);
        }
        assert_eq!(
            d.submit(),
            Some(AgentsAction::AiCreate {
                brief: "an agent that writes tests".into()
            })
        );
    }

    #[test]
    fn delete_only_on_user_agents() {
        let mut d = AgentsDialog::new(rows());
        d.next(); // nav 1 (manual entry)
        d.next(); // → user "refactorer" (nav idx 2)
        assert_eq!(
            d.on_delete(),
            Some(AgentsAction::Delete {
                name: "refactorer".into()
            })
        );
        d.next(); // → built-in "general"
        assert_eq!(d.on_delete(), None);
    }

    #[test]
    fn form_submit_requires_name_and_system() {
        let mut d = AgentsDialog::new(rows());
        d.next(); // nav 1 = manual
        d.on_enter();
        for c in "tester".chars() {
            d.form_push(c);
        }
        assert_eq!(d.submit(), None); // no system yet
        d.form_next_field(); // description
        d.form_next_field(); // system
        for c in "You test.".chars() {
            d.form_push(c);
        }
        assert_eq!(
            d.submit(),
            Some(AgentsAction::Create {
                name: "tester".into(),
                description: String::new(),
                system: "You test.".into()
            })
        );
    }

    #[test]
    fn esc_from_create_returns_to_list_then_closes() {
        let mut d = AgentsDialog::new(rows());
        d.on_enter();
        assert_eq!(d.on_esc(), None); // create → list
        assert!(!d.is_create_mode());
        assert_eq!(d.on_esc(), Some(AgentsAction::Close));
    }
}
