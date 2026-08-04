//! Workflow manager opened by `/workflows`: lists user-defined workflows, lets
//! you delete one, and — per "let the main agent create workflows" — creates new
//! ones via a one-line brief that the main agent turns into a workflow with the
//! `DefineWorkflow` tool (rather than a manual step form). Sibling of
//! [`super::agents_dialog`].

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

/// One listed workflow (name + description).
#[derive(Debug, Clone)]
pub struct WorkflowRow {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    /// AI-assisted: a one-field brief the main agent turns into a workflow.
    AiBrief,
}

/// What the app should do when the dialog yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowsAction {
    /// Submit a templated turn so the main agent builds the workflow (via the
    /// `DefineWorkflow` tool) from this brief.
    AiCreate {
        brief: String,
    },
    /// Run a saved workflow: execute its steps in order (the app loads the def
    /// and submits a strict ordered-script turn).
    Run {
        name: String,
    },
    Delete {
        name: String,
    },
    Close,
}

pub struct WorkflowsDialog {
    mode: Mode,
    rows: Vec<WorkflowRow>,
    selected: usize, // 0 = "Create new", then rows
    brief: String,
}

/// Nav rows before the workflow list: the single create entry.
const CREATE_ENTRIES: usize = 1;

impl WorkflowsDialog {
    pub fn new(rows: Vec<WorkflowRow>) -> Self {
        Self {
            mode: Mode::List,
            rows,
            selected: 0,
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

    fn selected_row(&self) -> Option<&WorkflowRow> {
        self.selected
            .checked_sub(CREATE_ENTRIES)
            .and_then(|i| self.rows.get(i))
    }

    /// Enter: on the create entry → AI brief; on a workflow row → run it.
    pub fn on_enter(&mut self) -> Option<WorkflowsAction> {
        if self.mode != Mode::List {
            return None;
        }
        if self.selected == 0 {
            self.mode = Mode::AiBrief;
            self.brief.clear();
            None
        } else {
            self.selected_row().map(|r| WorkflowsAction::Run {
                name: r.name.clone(),
            })
        }
    }

    pub fn on_delete(&self) -> Option<WorkflowsAction> {
        if self.mode != Mode::List {
            return None;
        }
        self.selected_row().map(|r| WorkflowsAction::Delete {
            name: r.name.clone(),
        })
    }

    pub fn on_esc(&mut self) -> Option<WorkflowsAction> {
        match self.mode {
            Mode::AiBrief => {
                self.mode = Mode::List;
                None
            }
            Mode::List => Some(WorkflowsAction::Close),
        }
    }

    pub fn is_input_mode(&self) -> bool {
        self.mode == Mode::AiBrief
    }

    pub fn form_push(&mut self, c: char) {
        if self.mode == Mode::AiBrief {
            self.brief.push(c);
        }
    }

    pub fn form_backspace(&mut self) {
        if self.mode == Mode::AiBrief {
            self.brief.pop();
        }
    }

    /// Enter / Ctrl+S: submit the brief.
    pub fn submit(&mut self) -> Option<WorkflowsAction> {
        if self.mode != Mode::AiBrief {
            return None;
        }
        let brief = self.brief.trim().to_string();
        (!brief.is_empty()).then_some(WorkflowsAction::AiCreate { brief })
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 76, 26);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );
        let inner = inner_area(popup);
        let title = if self.mode == Mode::AiBrief {
            crate::tr("Create new workflow")
        } else {
            crate::tr("Manage workflows")
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
            Mode::AiBrief => self.render_ai_brief(f, body, theme),
        }
        let footer = match self.mode {
            Mode::AiBrief => {
                format!("Enter {}   Esc {}", crate::tr("save"), crate::tr("cancel"))
            }
            Mode::List => format!(
                "↑↓   Enter {} / {}   d {}   Esc {}",
                crate::tr("new"),
                crate::tr("run"),
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
        lines.push(row_line(
            &format!("› {}  ✦", crate::tr("Create new workflow")),
            self.selected == 0,
            theme,
        ));
        lines.push(Line::from(""));
        if self.rows.is_empty() {
            lines.push(Line::styled(
                crate::tr("No workflows yet").to_string(),
                Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
            ));
        } else {
            lines.push(section(crate::tr("User workflows"), theme));
            for (i, r) in self.rows.iter().enumerate() {
                let label = format!("  {} · {}", r.name, r.description);
                lines.push(row_line(&label, self.selected == i + CREATE_ENTRIES, theme));
            }
        }
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme.bg_secondary)),
            area,
        );
    }

    fn render_ai_brief(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let lines = vec![
            Line::styled(
                crate::tr("Describe the workflow; the main agent will build it as ordered steps.")
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

    fn rows() -> Vec<WorkflowRow> {
        vec![WorkflowRow {
            name: "review-and-fix".into(),
            description: "Review then fix".into(),
        }]
    }

    #[test]
    fn enter_on_create_switches_to_brief() {
        let mut d = WorkflowsDialog::new(rows());
        d.on_enter();
        assert!(d.is_input_mode());
    }

    #[test]
    fn brief_yields_aicreate() {
        let mut d = WorkflowsDialog::new(rows());
        d.on_enter();
        for c in "review the diff then fix".chars() {
            d.form_push(c);
        }
        assert_eq!(
            d.submit(),
            Some(WorkflowsAction::AiCreate {
                brief: "review the diff then fix".into()
            })
        );
    }

    #[test]
    fn delete_yields_action_for_a_workflow() {
        let mut d = WorkflowsDialog::new(rows());
        d.next(); // → "review-and-fix" (nav idx 1)
        assert_eq!(
            d.on_delete(),
            Some(WorkflowsAction::Delete {
                name: "review-and-fix".into()
            })
        );
    }
}
