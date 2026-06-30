//! Modal opened by the `AskUserQuestion` tool: a multi-question panel. Each
//! question is single-choice over its preset options PLUS a free-text "Other"
//! row, and a request may carry several questions answered together. Navigation
//! is arrow-keys + Enter (mirroring the other pickers); a final "Submit" row
//! sends one answer per question back to the waiting tool, Esc dismisses.

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::question::{QuestionRequest, QuestionSpec};

use crate::theme::Theme;

/// What the user picked for one question.
#[derive(Debug, Clone, PartialEq)]
enum Sel {
    /// A preset option, by index.
    Opt(usize),
    /// The free-text "Other" answer (text lives in `customs[q]`).
    Other,
}

/// A focusable row in the flat navigation list.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Item {
    Option { q: usize, opt: usize },
    Other { q: usize },
    Submit,
}

pub struct QuestionDialog {
    request: Option<QuestionRequest>,
    specs: Vec<QuestionSpec>,
    items: Vec<Item>,
    cursor: usize,
    selections: Vec<Option<Sel>>,
    customs: Vec<String>,
    /// Editing the focused question's "Other" text.
    editing: bool,
}

impl QuestionDialog {
    pub fn new(request: QuestionRequest) -> Self {
        let specs = request.specs.clone();
        let mut items = Vec::new();
        for (q, spec) in specs.iter().enumerate() {
            for opt in 0..spec.options.len() {
                items.push(Item::Option { q, opt });
            }
            items.push(Item::Other { q });
        }
        items.push(Item::Submit);
        let n = specs.len();
        Self {
            request: Some(request),
            specs,
            items,
            cursor: 0,
            selections: vec![None; n],
            customs: vec![String::new(); n],
            editing: false,
        }
    }

    /// The asking tab's id, so the app can focus that conversation.
    pub fn source(&self) -> Option<String> {
        self.request.as_ref().and_then(|r| r.source.clone())
    }

    /// The question that the focused row belongs to (None on the Submit row).
    fn focused_question(&self) -> Option<usize> {
        match self.items.get(self.cursor) {
            Some(Item::Option { q, .. }) | Some(Item::Other { q }) => Some(*q),
            _ => None,
        }
    }

    fn all_answered(&self) -> bool {
        self.selections.iter().all(Option::is_some)
    }

    /// One answer per question: the chosen option label or the custom text.
    fn answers(&self) -> Vec<String> {
        self.specs
            .iter()
            .enumerate()
            .map(|(q, spec)| match &self.selections[q] {
                Some(Sel::Opt(i)) => spec.options.get(*i).cloned().unwrap_or_default(),
                Some(Sel::Other) => self.customs[q].clone(),
                None => String::new(),
            })
            .collect()
    }

    fn submit_index(&self) -> usize {
        self.items.len().saturating_sub(1)
    }

    fn first_unanswered_item(&self) -> Option<usize> {
        let q = self.selections.iter().position(Option::is_none)?;
        self.items
            .iter()
            .position(|it| matches!(it, Item::Option { q: iq, opt: 0 } if *iq == q))
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.items.len() as isize;
        if len == 0 {
            return;
        }
        self.cursor = (((self.cursor as isize + delta) % len + len) % len) as usize;
    }

    /// After picking an answer for `q`: jump to Submit once everything's
    /// answered, otherwise step to the next row.
    fn advance_after_select(&mut self) {
        if self.all_answered() {
            self.cursor = self.submit_index();
        } else {
            self.move_cursor(1);
        }
    }

    /// Handle a key. Returns true once the dialog is done (submitted/dismissed),
    /// at which point the response has been sent back to the waiting tool.
    pub fn on_key(&mut self, code: KeyCode) -> bool {
        if self.editing {
            return self.on_edit_key(code);
        }
        match code {
            KeyCode::Up | KeyCode::Left => {
                self.move_cursor(-1);
                false
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => {
                self.move_cursor(1);
                false
            }
            // A digit picks that option within the focused question.
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                if let Some(q) = self.focused_question() {
                    let n = (c as u8 - b'1') as usize;
                    if n < self.specs[q].options.len() {
                        self.selections[q] = Some(Sel::Opt(n));
                        self.advance_after_select();
                    }
                }
                false
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            KeyCode::Esc => {
                if let Some(req) = self.request.take() {
                    let _ = req.respond(None);
                }
                true
            }
            _ => false,
        }
    }

    fn activate(&mut self) -> bool {
        match self.items[self.cursor] {
            Item::Option { q, opt } => {
                self.selections[q] = Some(Sel::Opt(opt));
                self.advance_after_select();
                false
            }
            Item::Other { .. } => {
                self.editing = true;
                false
            }
            Item::Submit => {
                if self.all_answered() {
                    if let Some(req) = self.request.take() {
                        let _ = req.respond(Some(self.answers()));
                    }
                    return true;
                }
                if let Some(i) = self.first_unanswered_item() {
                    self.cursor = i;
                }
                false
            }
        }
    }

    fn on_edit_key(&mut self, code: KeyCode) -> bool {
        let Some(q) = self.focused_question() else {
            self.editing = false;
            return false;
        };
        match code {
            KeyCode::Char(c) => self.customs[q].push(c),
            KeyCode::Backspace => {
                self.customs[q].pop();
            }
            KeyCode::Enter => {
                self.selections[q] = Some(Sel::Other);
                self.editing = false;
                self.advance_after_select();
            }
            // Esc just leaves edit mode (keeps any text typed so far).
            KeyCode::Esc => self.editing = false,
            _ => {}
        }
        false
    }

    /// Flatten the panel into display rows, tagging each focusable row with its
    /// `Item` index so the renderer can highlight the cursor and window scroll.
    fn display_rows(&self, theme: &Theme) -> Vec<(Line<'static>, Option<usize>)> {
        let mut rows: Vec<(Line<'static>, Option<usize>)> = Vec::new();
        let mut item_idx = 0usize;
        for (q, spec) in self.specs.iter().enumerate() {
            if q > 0 {
                rows.push((Line::from(""), None));
            }
            // Question text (+ optional header chip).
            let mut head: Vec<Span<'static>> = Vec::new();
            if let Some(h) = &spec.header {
                head.push(Span::styled(
                    format!(" {h} "),
                    Style::default()
                        .fg(theme.bg_primary)
                        .bg(theme.accent_secondary),
                ));
                head.push(Span::styled(" ", Style::default().bg(theme.bg_secondary)));
            }
            head.push(Span::styled(
                spec.question.clone(),
                Style::default()
                    .fg(theme.fg_white)
                    .bg(theme.bg_secondary)
                    .add_modifier(Modifier::BOLD),
            ));
            rows.push((Line::from(head), None));

            for (opt, label) in spec.options.iter().enumerate() {
                let chosen = self.selections[q] == Some(Sel::Opt(opt));
                let marker = if chosen { "◉" } else { "○" };
                rows.push((Line::from(format!("  {marker} {label}")), Some(item_idx)));
                item_idx += 1;
            }
            // Other row.
            let other_chosen = self.selections[q] == Some(Sel::Other);
            let marker = if other_chosen { "◉" } else { "○" };
            let editing_here = self.editing && self.focused_question() == Some(q);
            let other_label = if !self.customs[q].is_empty() {
                self.customs[q].clone()
            } else {
                "Other (type a custom answer)".to_string()
            };
            let cursor_glyph = if editing_here { "▏" } else { "" };
            rows.push((
                Line::from(format!("  {marker} {other_label}{cursor_glyph}")),
                Some(item_idx),
            ));
            item_idx += 1;
        }
        // Submit row.
        let ready = self.all_answered();
        let submit = if ready {
            "✓ Submit"
        } else {
            "✓ Submit (answer all questions first)"
        };
        rows.push((Line::from(format!("  {submit}")), Some(item_idx)));
        rows
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        if self.request.is_none() {
            return;
        }
        let rows = self.display_rows(theme);
        let inner_w = 76u16;
        let total = rows.len() as u16;
        // header + body(capped) + footer.
        let body_cap = total.min(area.height.saturating_sub(6).max(3));
        let popup = modal_area(area, inner_w, body_cap + 5);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );
        let inner = inner_area(popup);

        // Header.
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "The agent is asking",
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg_secondary)
                    .add_modifier(Modifier::BOLD),
            )))
            .style(Style::default().bg(theme.bg_secondary)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        // Body window: keep the cursor's row visible.
        let body_top = inner.y.saturating_add(2);
        let body_h = inner.height.saturating_sub(3) as usize;
        let cursor_row = rows
            .iter()
            .position(|(_, item)| *item == Some(self.cursor))
            .unwrap_or(0);
        let start = scroll_start(cursor_row, rows.len(), body_h);
        let mut y = body_top;
        for (line, item) in rows.iter().skip(start).take(body_h) {
            let focused = *item == Some(self.cursor);
            let style = if focused {
                Style::default()
                    .fg(theme.bg_primary)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
            };
            let row = if item.is_some() {
                Paragraph::new(pad_to_width(plain(line), inner.width)).style(style)
            } else {
                Paragraph::new(line.clone()).style(Style::default().bg(theme.bg_secondary))
            };
            f.render_widget(row, Rect::new(inner.x, y, inner.width, 1));
            y = y.saturating_add(1);
        }

        // Footer.
        let key = Style::default()
            .fg(theme.fg_white)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD);
        let lbl = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
        let hint = if self.editing {
            Line::from(vec![
                Span::styled("type", key),
                Span::styled(" custom answer   ", lbl),
                Span::styled("enter", key),
                Span::styled(" confirm   ", lbl),
                Span::styled("esc", key),
                Span::styled(" stop editing", lbl),
            ])
        } else {
            Line::from(vec![
                Span::styled("↑↓/1-9", key),
                Span::styled(" move   ", lbl),
                Span::styled("enter", key),
                Span::styled(" select   ", lbl),
                Span::styled("esc", key),
                Span::styled(" skip", lbl),
            ])
        };
        f.render_widget(
            Paragraph::new(hint).style(Style::default().bg(theme.bg_secondary)),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }
}

fn plain(line: &Line<'static>) -> String {
    line.spans.iter().map(|s| s.content.to_string()).collect()
}

fn scroll_start(cursor_row: usize, total: usize, height: usize) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    let max_start = total - height;
    // Keep the cursor row within the window, biased so it isn't on the last line.
    cursor_row.saturating_sub(height / 2).min(max_start)
}

fn modal_area(area: Rect, target_width: u16, target_height: u16) -> Rect {
    let max_w = area.width.saturating_sub(6);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(target_width).max(max_w.min(40));
    let height = max_h.min(target_height).max(max_h.min(8));
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

fn pad_to_width(value: String, width: u16) -> String {
    let width = width as usize;
    let mut out: String = value.chars().take(width).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::question::{question_queue, QuestionSpec};

    fn spec(q: &str, opts: &[&str]) -> QuestionSpec {
        QuestionSpec {
            question: q.to_string(),
            header: None,
            options: opts.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn single_question_select_then_submit() {
        let (queue, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("pick", &["a", "b", "c"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        // Move to "b" and select it → all answered → cursor jumps to Submit.
        assert!(!d.on_key(KeyCode::Down)); // -> b
        assert!(!d.on_key(KeyCode::Enter)); // select b, jump to submit
        assert!(d.on_key(KeyCode::Enter)); // submit
        assert_eq!(asker.await.unwrap(), Some(vec!["b".to_string()]));
    }

    #[tokio::test]
    async fn other_free_text_is_returned() {
        let (queue, mut rx) = question_queue();
        let asker =
            tokio::spawn(
                async move { queue.ask_specs(vec![spec("pick", &["a", "b"])], None).await },
            );
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        // Navigate to the Other row (after a, b) and type a custom answer.
        d.on_key(KeyCode::Down); // b
        d.on_key(KeyCode::Down); // Other
        assert!(!d.on_key(KeyCode::Enter)); // start editing
        for c in "zig".chars() {
            d.on_key(KeyCode::Char(c));
        }
        assert!(!d.on_key(KeyCode::Enter)); // confirm Other → jump to submit
        assert!(d.on_key(KeyCode::Enter)); // submit
        assert_eq!(asker.await.unwrap(), Some(vec!["zig".to_string()]));
    }

    #[tokio::test]
    async fn multi_question_collects_all_answers() {
        let (queue, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("q1", &["a", "b"]), spec("q2", &["x", "y"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        // items: [a(0) b(1) Other0(2) x(3) y(4) Other1(5) Submit(6)]
        assert!(!d.on_key(KeyCode::Enter)); // q1 = a (cursor 0), advance → 1
        d.on_key(KeyCode::Down); // -> Other0 (2)
        d.on_key(KeyCode::Down); // -> x (3, q2 opt0)
        d.on_key(KeyCode::Down); // -> y (4, q2 opt1)
        assert!(!d.on_key(KeyCode::Enter)); // q2 = y → all answered → submit
        assert!(d.on_key(KeyCode::Enter)); // submit
        assert_eq!(
            asker.await.unwrap(),
            Some(vec!["a".to_string(), "y".to_string()])
        );
    }

    #[tokio::test]
    async fn esc_dismisses_with_none() {
        let (queue, mut rx) = question_queue();
        let asker =
            tokio::spawn(
                async move { queue.ask_specs(vec![spec("pick", &["a", "b"])], None).await },
            );
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(d.on_key(KeyCode::Esc));
        assert_eq!(asker.await.unwrap(), None);
    }

    #[tokio::test]
    async fn submit_blocked_until_all_answered() {
        let (queue, mut rx) = question_queue();
        let _asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("q1", &["a", "b"]), spec("q2", &["x", "y"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        // Jump straight to submit without answering → not done, cursor moves to
        // the first unanswered question.
        let submit = d.submit_index();
        d.cursor = submit;
        assert!(!d.on_key(KeyCode::Enter));
        assert!(matches!(d.items[d.cursor], Item::Option { q: 0, opt: 0 }));
    }
}
