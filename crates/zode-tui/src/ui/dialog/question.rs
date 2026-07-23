//! Modal opened by the `AskUserQuestion` tool: a TABBED multi-question panel.
//! One question shows at a time; the tab strip on top tracks progress (✓ per
//! answered question) and ends in a Submit tab. Each question is single-choice
//! over its preset options PLUS a free-text "Other" row (preset options that
//! duplicate "Other" — models sometimes add their own — are filtered out).
//! Answering a question advances straight to the next unanswered one;
//! Tab/←→ switch tabs manually; Esc dismisses.

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
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

/// A preset option that duplicates the built-in free-text "Other" row (the
/// model added its own despite the tool description). Match on the label's
/// LEADING word only, so options merely mentioning these words survive.
fn is_custom_like(label: &str) -> bool {
    let l = label.trim().to_lowercase();
    [
        "other",
        "custom",
        "其他",
        "其它",
        "自定义",
        "自訂",
        "手动输入",
    ]
    .iter()
    .any(|p| l.starts_with(p))
}

/// Screen geometry recorded on render so mouse clicks can hit-test: the popup
/// rect, the tab strip's chips, the focusable body rows, and the submit row.
#[derive(Default)]
struct QuestionHits {
    popup: Rect,
    strip_row: u16,
    /// `(x_start, x_end, tab)` per chip — `tab == specs.len()` is Submit.
    chips: Vec<(u16, u16, usize)>,
    /// `(screen_row, focusable option index)` for the visible body rows.
    rows: Vec<(u16, Option<usize>)>,
    /// The `➤ Submit` action row on the Submit tab.
    submit_row: Option<u16>,
}

pub struct QuestionDialog {
    request: Option<QuestionRequest>,
    /// Specs with custom-like preset options already filtered out.
    specs: Vec<QuestionSpec>,
    /// Active tab: `0..specs.len()` is a question; `specs.len()` is Submit.
    tab: usize,
    /// Focused row inside the active question: `0..options.len()` is a preset
    /// option, `options.len()` is the "Other" row. Unused on the Submit tab.
    cursor: usize,
    selections: Vec<Option<Sel>>,
    customs: Vec<String>,
    /// Editing the active question's "Other" text.
    editing: bool,
    hits: QuestionHits,
}

impl QuestionDialog {
    pub fn new(request: QuestionRequest) -> Self {
        let specs: Vec<QuestionSpec> = request
            .specs
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.options.retain(|o| !is_custom_like(o));
                s
            })
            .collect();
        let n = specs.len();
        Self {
            request: Some(request),
            specs,
            tab: 0,
            cursor: 0,
            selections: vec![None; n],
            customs: vec![String::new(); n],
            editing: false,
            hits: QuestionHits::default(),
        }
    }

    /// Handle a left click at a screen position. Same done-semantics as
    /// [`Self::on_key`]: true once submitted. Geometry comes from the hits
    /// recorded on the previous render; clicks outside the popup are ignored
    /// (Esc remains the only way to dismiss).
    pub fn on_mouse(&mut self, column: u16, row: u16) -> bool {
        if self.request.is_none() {
            return false;
        }
        let popup = self.hits.popup;
        if popup.width == 0
            || column < popup.x
            || column >= popup.x + popup.width
            || row < popup.y
            || row >= popup.y + popup.height
        {
            return false;
        }
        if row == self.hits.strip_row {
            let chip = self
                .hits
                .chips
                .iter()
                .find(|(x0, x1, _)| column >= *x0 && column < *x1)
                .map(|&(_, _, tab)| tab);
            if let Some(tab) = chip {
                self.editing = false;
                self.goto_tab(tab);
            }
            return false;
        }
        if self.hits.submit_row == Some(row) {
            return self.activate();
        }
        let opt = self
            .hits
            .rows
            .iter()
            .find(|(y, tag)| *y == row && tag.is_some())
            .and_then(|(_, tag)| *tag);
        if let Some(opt) = opt {
            self.editing = false;
            self.cursor = opt;
            return self.activate();
        }
        false
    }

    #[cfg(test)]
    fn test_option_point(&self, opt: usize) -> Option<(u16, u16)> {
        self.hits
            .rows
            .iter()
            .find(|(_, tag)| *tag == Some(opt))
            .map(|(y, _)| (self.hits.popup.x + 3, *y))
    }

    #[cfg(test)]
    fn test_chip_point(&self, tab: usize) -> Option<(u16, u16)> {
        self.hits
            .chips
            .iter()
            .find(|(_, _, t)| *t == tab)
            .map(|(x0, _, _)| (*x0, self.hits.strip_row))
    }

    #[cfg(test)]
    fn test_submit_point(&self) -> Option<(u16, u16)> {
        self.hits.submit_row.map(|y| (self.hits.popup.x + 3, y))
    }

    /// The asking tab's id, so the app can focus that conversation.
    pub fn source(&self) -> Option<String> {
        self.request.as_ref().and_then(|r| r.source.clone())
    }

    /// Dismiss the dialog and release the waiting tool with no answer.
    pub fn dismiss(&mut self) {
        if let Some(request) = self.request.take() {
            let _ = request.respond(None);
        }
    }

    fn submit_tab(&self) -> usize {
        self.specs.len()
    }

    fn on_submit_tab(&self) -> bool {
        self.tab == self.submit_tab()
    }

    /// Rows in the active question (presets + the "Other" row).
    fn rows(&self) -> usize {
        self.specs
            .get(self.tab)
            .map(|s| s.options.len() + 1)
            .unwrap_or(0)
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

    /// Put the row cursor on the question's current answer (or the top).
    fn sync_cursor(&mut self) {
        self.cursor = match self.specs.get(self.tab).map(|s| s.options.len()) {
            Some(len) => match self.selections.get(self.tab).and_then(|s| s.as_ref()) {
                Some(Sel::Opt(i)) => (*i).min(len),
                Some(Sel::Other) => len,
                None => 0,
            },
            None => 0,
        };
    }

    fn goto_tab(&mut self, tab: usize) {
        self.tab = tab.min(self.submit_tab());
        self.sync_cursor();
    }

    /// After answering: the next unanswered question, else the Submit tab.
    fn advance_after_select(&mut self) {
        let next = (self.tab + 1..self.specs.len())
            .chain(0..self.tab)
            .find(|&q| self.selections[q].is_none());
        match next {
            Some(q) => self.goto_tab(q),
            None => self.goto_tab(self.submit_tab()),
        }
    }

    /// Handle a key. Returns true once the dialog is done (submitted/dismissed),
    /// at which point the response has been sent back to the waiting tool.
    pub fn on_key(&mut self, code: KeyCode) -> bool {
        if self.editing {
            return self.on_edit_key(code);
        }
        let tabs = self.submit_tab() + 1;
        match code {
            KeyCode::Tab | KeyCode::Right => {
                self.goto_tab((self.tab + 1) % tabs);
                false
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.goto_tab((self.tab + tabs - 1) % tabs);
                false
            }
            KeyCode::Up => {
                let rows = self.rows();
                if rows > 0 {
                    self.cursor = (self.cursor + rows - 1) % rows;
                }
                false
            }
            KeyCode::Down => {
                let rows = self.rows();
                if rows > 0 {
                    self.cursor = (self.cursor + 1) % rows;
                }
                false
            }
            // A digit picks that option within the active question.
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                if let Some(spec) = self.specs.get(self.tab) {
                    let n = (c as u8 - b'1') as usize;
                    if n < spec.options.len() {
                        self.selections[self.tab] = Some(Sel::Opt(n));
                        self.advance_after_select();
                    }
                }
                false
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            KeyCode::Esc => {
                self.dismiss();
                true
            }
            _ => false,
        }
    }

    fn activate(&mut self) -> bool {
        if self.on_submit_tab() {
            if self.all_answered() {
                if let Some(req) = self.request.take() {
                    let _ = req.respond(Some(self.answers()));
                }
                return true;
            }
            if let Some(q) = self.selections.iter().position(Option::is_none) {
                self.goto_tab(q);
            }
            return false;
        }
        let opts = self.specs[self.tab].options.len();
        if self.cursor < opts {
            self.selections[self.tab] = Some(Sel::Opt(self.cursor));
            self.advance_after_select();
        } else {
            self.editing = true;
        }
        false
    }

    fn on_edit_key(&mut self, code: KeyCode) -> bool {
        let q = self.tab;
        if q >= self.specs.len() {
            self.editing = false;
            return false;
        }
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

    /// The tab strip: one chip per question (its header, or `Q<n>`) plus the
    /// Submit chip. Active chip inverts; answered chips get a ✓. Also returns
    /// each chip's `(x_start, x_end, tab)` RELATIVE to the line start, so the
    /// render can anchor click hitboxes.
    fn tab_strip(&self, theme: &Theme) -> (Line<'static>, Vec<(u16, u16, usize)>) {
        use unicode_width::UnicodeWidthStr;
        let bg = Style::default().bg(theme.bg_secondary);
        let mut spans: Vec<Span<'static>> = vec![Span::styled(" ", bg)];
        let mut chips = Vec::new();
        let mut x = 1u16;
        for (q, spec) in self.specs.iter().enumerate() {
            let label = spec.header.clone().unwrap_or_else(|| format!("Q{}", q + 1));
            let mark = if self.selections[q].is_some() {
                "✓ "
            } else {
                "○ "
            };
            let style = if q == self.tab {
                Style::default()
                    .fg(theme.bg_primary)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if self.selections[q].is_some() {
                bg.fg(Color::Green)
            } else {
                bg.fg(theme.fg_subtle)
            };
            let chip = format!(" {mark}{label} ");
            let w = UnicodeWidthStr::width(chip.as_str()) as u16;
            chips.push((x, x + w, q));
            x += w + 1; // the 1-col separator span below
            spans.push(Span::styled(chip, style));
            spans.push(Span::styled(" ", bg));
        }
        let submit_label = crate::tr("Submit");
        let submit_style = if self.on_submit_tab() {
            Style::default()
                .fg(theme.bg_primary)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else if self.all_answered() {
            bg.fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            bg.fg(theme.fg_subtle)
        };
        let chip = format!(" ➤ {submit_label} ");
        let w = UnicodeWidthStr::width(chip.as_str()) as u16;
        chips.push((x, x + w, self.submit_tab()));
        spans.push(Span::styled(chip, submit_style));
        (Line::from(spans), chips)
    }

    /// Body lines for the active tab. `Some(row)` tags focusable rows.
    fn body_rows(&self, theme: &Theme) -> Vec<(Line<'static>, Option<usize>)> {
        let bg = Style::default().bg(theme.bg_secondary);
        let mut rows: Vec<(Line<'static>, Option<usize>)> = Vec::new();
        if let Some(spec) = self.specs.get(self.tab) {
            rows.push((
                Line::from(Span::styled(
                    format!(" {}", spec.question),
                    bg.fg(theme.fg_white).add_modifier(Modifier::BOLD),
                )),
                None,
            ));
            rows.push((Line::from(Span::styled(String::new(), bg)), None));
            for (opt, label) in spec.options.iter().enumerate() {
                let chosen = self.selections[self.tab] == Some(Sel::Opt(opt));
                rows.push((self.option_line(opt, label, chosen, theme), Some(opt)));
            }
            let other_row = spec.options.len();
            let other_chosen = self.selections[self.tab] == Some(Sel::Other);
            let editing_here = self.editing;
            let label = if !self.customs[self.tab].is_empty() || editing_here {
                format!(
                    "{}{}",
                    self.customs[self.tab],
                    if editing_here { "▏" } else { "" }
                )
            } else {
                crate::tr("Other (type a custom answer)").to_string()
            };
            rows.push((
                self.option_line(other_row, &label, other_chosen, theme),
                Some(other_row),
            ));
        } else {
            // Submit tab: a summary of every answer.
            for (q, spec) in self.specs.iter().enumerate() {
                let name = spec.header.clone().unwrap_or_else(|| spec.question.clone());
                let (mark, answer, style) = match &self.selections[q] {
                    Some(sel) => {
                        let a = match sel {
                            Sel::Opt(i) => spec.options.get(*i).cloned().unwrap_or_default(),
                            Sel::Other => self.customs[q].clone(),
                        };
                        ("✓", a, bg.fg(theme.fg_text))
                    }
                    None => ("○", "—".to_string(), bg.fg(theme.fg_subtle)),
                };
                rows.push((
                    Line::from(vec![
                        Span::styled(format!(" {mark} {name}  "), bg.fg(theme.fg_subtle)),
                        Span::styled(answer, style),
                    ]),
                    None,
                ));
            }
            rows.push((Line::from(Span::styled(String::new(), bg)), None));
            let ready = self.all_answered();
            let submit = if ready {
                format!(" ➤ {}", crate::tr("Submit"))
            } else {
                format!(" ➤ {}", crate::tr("Submit (answer all questions first)"))
            };
            let style = if ready {
                bg.fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                bg.fg(theme.fg_subtle)
            };
            rows.push((Line::from(Span::styled(submit, style)), None));
        }
        rows
    }

    /// One selectable row: focus bar + radio marker + label.
    fn option_line(&self, row: usize, label: &str, chosen: bool, theme: &Theme) -> Line<'static> {
        let focused = !self.on_submit_tab() && self.cursor == row;
        let row_bg = if focused {
            theme.bg_input
        } else {
            theme.bg_secondary
        };
        let bar = if focused { "▌" } else { " " };
        let marker = if chosen { "◉" } else { "○" };
        let marker_fg = if chosen {
            theme.accent
        } else {
            theme.fg_subtle
        };
        let label_style = if focused {
            Style::default()
                .bg(row_bg)
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(row_bg).fg(theme.fg_text)
        };
        Line::from(vec![
            Span::styled(
                bar.to_string(),
                Style::default().bg(row_bg).fg(theme.accent),
            ),
            Span::styled(" ".to_string(), Style::default().bg(row_bg)),
            Span::styled(
                marker.to_string(),
                Style::default().bg(row_bg).fg(marker_fg),
            ),
            Span::styled(format!(" {label}"), label_style),
        ])
    }

    fn footer(&self, theme: &Theme) -> Line<'static> {
        let key = Style::default()
            .fg(theme.fg_white)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD);
        let lbl = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
        if self.editing {
            Line::from(vec![
                Span::styled(" type", key),
                Span::styled(format!(" {}   ", crate::tr("custom answer")), lbl),
                Span::styled("enter", key),
                Span::styled(format!(" {}   ", crate::tr("confirm")), lbl),
                Span::styled("esc", key),
                Span::styled(format!(" {}", crate::tr("stop editing")), lbl),
            ])
        } else {
            Line::from(vec![
                Span::styled(" tab", key),
                Span::styled(format!(" {}   ", crate::tr("switch")), lbl),
                Span::styled("↑↓/1-9", key),
                Span::styled(format!(" {}   ", crate::tr("move")), lbl),
                Span::styled("enter", key),
                Span::styled(format!(" {}   ", crate::tr("select")), lbl),
                Span::styled("esc", key),
                Span::styled(format!(" {}", crate::tr("skip")), lbl),
            ])
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.hits = QuestionHits::default();
        if self.request.is_none() {
            return;
        }
        let body = self.body_rows(theme);
        // borders(2) + tabs(1) + blank(1) + body + blank(1) + footer(1).
        let want_h = (body.len() as u16).saturating_add(6);
        let popup = modal_area(area, 76, want_h);
        self.hits.popup = popup;
        f.render_widget(Clear, popup);

        let bg = Style::default().bg(theme.bg_secondary);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.separator).bg(theme.bg_secondary))
            .title(Span::styled(
                format!(" {} ", crate::tr("The agent is asking")),
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg_secondary)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(bg);
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        if inner.height < 4 || inner.width < 10 {
            return;
        }
        // One column of breathing room inside the border.
        let inner = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );

        // Tab strip (chips recorded for click hit-testing).
        let (strip, chips) = self.tab_strip(theme);
        self.hits.strip_row = inner.y;
        self.hits.chips = chips
            .into_iter()
            .filter(|(_, x1, _)| *x1 <= inner.width)
            .map(|(x0, x1, t)| (inner.x + x0, inner.x + x1, t))
            .collect();
        f.render_widget(
            Paragraph::new(strip).style(bg),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        // Body window (below tabs + blank, above blank + footer), scrolled to
        // keep the focused row visible.
        let body_top = inner.y + 2;
        let body_h = inner.height.saturating_sub(4) as usize;
        let cursor_row = body
            .iter()
            .position(|(_, row)| *row == Some(self.cursor))
            .unwrap_or(0);
        let start = scroll_start(cursor_row, body.len(), body_h);
        let mut y = body_top;
        for (idx, (line, tag)) in body.iter().enumerate().skip(start).take(body_h) {
            self.hits.rows.push((y, *tag));
            // On the Submit tab the action row is the last body row.
            if self.on_submit_tab() && idx == body.len() - 1 {
                self.hits.submit_row = Some(y);
            }
            f.render_widget(
                Paragraph::new(line.clone()).style(bg),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y = y.saturating_add(1);
        }

        // Footer.
        f.render_widget(
            Paragraph::new(self.footer(theme)).style(bg),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
        );
    }
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

    #[test]
    fn custom_like_labels_are_detected() {
        assert!(is_custom_like("其他(输入自定义答案)"));
        assert!(is_custom_like("自定义(手动输入)"));
        assert!(is_custom_like("Other (specify)"));
        assert!(is_custom_like("  custom answer"));
        assert!(!is_custom_like("use another library")); // mid-word "other"
        assert!(!is_custom_like("2-3 步"));
    }

    #[tokio::test]
    async fn custom_like_preset_options_are_deduped() {
        let (queue, mut rx) = question_queue();
        let _asker = tokio::spawn(async move {
            queue
                .ask_specs(
                    vec![spec("pick", &["a", "自定义(手动输入)", "Other (specify)"])],
                    None,
                )
                .await
        });
        let req = rx.next().await.unwrap();
        let d = QuestionDialog::new(req);
        assert_eq!(d.specs[0].options, vec!["a".to_string()]);
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
        // Move to "b" and select it → all answered → jumps to the Submit tab.
        assert!(!d.on_key(KeyCode::Down)); // -> b
        assert!(!d.on_key(KeyCode::Enter)); // select b, advance to submit
        assert!(d.on_submit_tab());
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
    async fn answering_advances_to_the_next_question_tab() {
        let (queue, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("q1", &["a", "b"]), spec("q2", &["x", "y"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(!d.on_key(KeyCode::Enter)); // q1 = a → auto-advance to q2's tab
        assert_eq!(d.tab, 1);
        assert_eq!(d.cursor, 0);
        d.on_key(KeyCode::Down); // -> y
        assert!(!d.on_key(KeyCode::Enter)); // q2 = y → all answered → submit
        assert!(d.on_submit_tab());
        assert!(d.on_key(KeyCode::Enter)); // submit
        assert_eq!(
            asker.await.unwrap(),
            Some(vec!["a".to_string(), "y".to_string()])
        );
    }

    #[tokio::test]
    async fn tab_key_switches_questions_and_wraps() {
        let (queue, mut rx) = question_queue();
        let _asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("q1", &["a", "b"]), spec("q2", &["x", "y"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        d.on_key(KeyCode::Tab);
        assert_eq!(d.tab, 1);
        d.on_key(KeyCode::Tab);
        assert!(d.on_submit_tab());
        d.on_key(KeyCode::Tab); // wraps back to the first question
        assert_eq!(d.tab, 0);
        d.on_key(KeyCode::BackTab);
        assert!(d.on_submit_tab());
    }

    #[tokio::test]
    async fn clicking_options_chips_and_submit_drives_the_dialog() {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};
        let (queue, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("q1", &["a", "b"]), spec("q2", &["x", "y"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();

        // Click option "b" in q1 → selected, auto-advances to q2.
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let (col, row) = d.test_option_point(1).expect("option row rendered");
        assert!(!d.on_mouse(col, row));
        assert_eq!(d.tab, 1);

        // A click outside the popup is ignored.
        assert!(!d.on_mouse(0, 0));
        assert_eq!(d.tab, 1);

        // Click the FIRST question's chip in the strip → back to q1.
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let (col, row) = d.test_chip_point(0).expect("chip rendered");
        assert!(!d.on_mouse(col, row));
        assert_eq!(d.tab, 0);

        // Answer q2 by clicking "y", landing on the Submit tab.
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let (col, row) = d.test_chip_point(1).unwrap();
        assert!(!d.on_mouse(col, row));
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let (col, row) = d.test_option_point(1).unwrap();
        assert!(!d.on_mouse(col, row));
        assert!(d.on_submit_tab());

        // Click the submit row → dialog done, answers delivered.
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let (col, row) = d.test_submit_point().expect("submit row rendered");
        assert!(d.on_mouse(col, row));
        assert_eq!(
            asker.await.unwrap(),
            Some(vec!["b".to_string(), "y".to_string()])
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
    async fn dismiss_releases_waiter_once_with_none() {
        let (queue, mut rx) = question_queue();
        let asker =
            tokio::spawn(
                async move { queue.ask_specs(vec![spec("pick", &["a", "b"])], None).await },
            );
        let req = rx.next().await.unwrap();
        let mut dialog = QuestionDialog::new(req);

        dialog.dismiss();
        dialog.dismiss();

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
        // Jump straight to Submit without answering → not done; focus moves to
        // the first unanswered question's tab.
        d.goto_tab(d.submit_tab());
        assert!(!d.on_key(KeyCode::Enter));
        assert_eq!(d.tab, 0);
        assert_eq!(d.cursor, 0);
    }
}
