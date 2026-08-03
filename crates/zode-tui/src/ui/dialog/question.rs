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

use super::question_layout::{
    modal_height, modal_rect, modal_width, scroll_start, strip_scroll, text_width, wrap_text,
};
use crate::theme::Theme;

/// Columns taken by an option row's prefix (`▌ ◉ `): continuation lines indent
/// by the same amount so wrapped text stays flush under the label.
const OPT_INDENT: u16 = 4;

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

/// One rendered body line. `tag` marks the focusable option a line belongs to
/// — every visual line of a wrapped option carries the same tag, so focus and
/// click hit-testing cover the whole block. `submit` marks the Submit action.
struct BodyLine {
    line: Line<'static>,
    tag: Option<usize>,
    submit: bool,
}

impl BodyLine {
    fn plain(line: Line<'static>) -> Self {
        Self {
            line,
            tag: None,
            submit: false,
        }
    }

    fn tagged(line: Line<'static>, tag: usize) -> Self {
        Self {
            line,
            tag: Some(tag),
            submit: false,
        }
    }
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

    /// Question `q`'s free-text row label: the typed answer (with a cursor
    /// glyph while that row is being edited) or the placeholder hint.
    fn other_label(&self, q: usize) -> String {
        let editing = self.editing && q == self.tab;
        if !self.customs[q].is_empty() || editing {
            format!("{}{}", self.customs[q], if editing { "▏" } else { "" })
        } else {
            crate::tr("Other (type a custom answer)").to_string()
        }
    }

    /// The Submit tab's summary text for one question: `(marker, name, answer)`.
    fn summary_parts(&self, q: usize) -> (&'static str, String, String) {
        let spec = &self.specs[q];
        let name = spec.header.clone().unwrap_or_else(|| spec.question.clone());
        match &self.selections[q] {
            Some(Sel::Opt(i)) => ("✓", name, spec.options.get(*i).cloned().unwrap_or_default()),
            Some(Sel::Other) => ("✓", name, self.customs[q].clone()),
            None => ("○", name, "—".to_string()),
        }
    }

    fn submit_label(&self) -> String {
        if self.all_answered() {
            format!(" ➤ {}", crate::tr("Submit"))
        } else {
            format!(" ➤ {}", crate::tr("Submit (answer all questions first)"))
        }
    }

    /// Widest body line across ALL tabs, unwrapped, so the popup can be sized
    /// once and stay that size while the user moves between tabs.
    fn natural_width(&self) -> u16 {
        let mut w = 0u16;
        for (q, spec) in self.specs.iter().enumerate() {
            w = w.max(1 + text_width(&spec.question));
            for label in &spec.options {
                w = w.max(OPT_INDENT + text_width(label));
            }
            w = w.max(OPT_INDENT + text_width(&self.other_label(q)));
            let (mark, name, answer) = self.summary_parts(q);
            w = w.max(text_width(&format!(" {mark} {name}  {answer}")));
        }
        w.max(text_width(&self.submit_label()))
    }

    /// Body lines for the active tab, wrapped to `width` columns.
    fn body_rows(&self, width: u16, theme: &Theme) -> Vec<BodyLine> {
        let bg = Style::default().bg(theme.bg_secondary);
        let mut rows: Vec<BodyLine> = Vec::new();
        if let Some(spec) = self.specs.get(self.tab) {
            let head = bg.fg(theme.fg_white).add_modifier(Modifier::BOLD);
            for seg in wrap_text(&spec.question, width.saturating_sub(1)) {
                rows.push(BodyLine::plain(Line::from(Span::styled(
                    format!(" {seg}"),
                    head,
                ))));
            }
            rows.push(BodyLine::plain(Line::from(Span::styled(String::new(), bg))));
            for (opt, label) in spec.options.iter().enumerate() {
                let chosen = self.selections[self.tab] == Some(Sel::Opt(opt));
                for line in self.option_lines(opt, label, chosen, width, theme) {
                    rows.push(BodyLine::tagged(line, opt));
                }
            }
            let other_row = spec.options.len();
            let other_chosen = self.selections[self.tab] == Some(Sel::Other);
            let label = self.other_label(self.tab);
            for line in self.option_lines(other_row, &label, other_chosen, width, theme) {
                rows.push(BodyLine::tagged(line, other_row));
            }
        } else {
            // Submit tab: a summary of every answer.
            for q in 0..self.specs.len() {
                let (mark, name, answer) = self.summary_parts(q);
                let answered = self.selections[q].is_some();
                let style = if answered {
                    bg.fg(theme.fg_text)
                } else {
                    bg.fg(theme.fg_subtle)
                };
                rows.extend(
                    summary_lines(&format!(" {mark} {name}  "), &answer, width, bg, style)
                        .into_iter()
                        .map(BodyLine::plain),
                );
            }
            rows.push(BodyLine::plain(Line::from(Span::styled(String::new(), bg))));
            let ready = self.all_answered();
            let style = if ready {
                bg.fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                bg.fg(theme.fg_subtle)
            };
            // Wrapped at the indent width so continuation lines still fit.
            for (i, seg) in wrap_text(&self.submit_label(), width.saturating_sub(3))
                .into_iter()
                .enumerate()
            {
                let text = if i == 0 { seg } else { format!("   {seg}") };
                rows.push(BodyLine {
                    line: Line::from(Span::styled(text, style)),
                    tag: None,
                    submit: true,
                });
            }
        }
        rows
    }

    /// One selectable row: focus bar + radio marker + label, wrapped to
    /// `width`. Continuation lines keep the focus bar and indent under the
    /// label so a long option reads as one block.
    fn option_lines(
        &self,
        row: usize,
        label: &str,
        chosen: bool,
        width: u16,
        theme: &Theme,
    ) -> Vec<Line<'static>> {
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
        let bar_style = Style::default().bg(row_bg).fg(theme.accent);
        let text_w = width.saturating_sub(OPT_INDENT);
        wrap_text(label, text_w)
            .into_iter()
            .enumerate()
            .map(|(i, seg)| {
                let pad = if focused {
                    // Pad the focus background across the row so a wrapped
                    // block reads as a single highlighted selection.
                    " ".repeat((text_w as usize).saturating_sub(text_width(&seg) as usize))
                } else {
                    String::new()
                };
                let mut spans = vec![Span::styled(bar.to_string(), bar_style)];
                if i == 0 {
                    spans.push(Span::styled(" ".to_string(), Style::default().bg(row_bg)));
                    spans.push(Span::styled(
                        marker.to_string(),
                        Style::default().bg(row_bg).fg(marker_fg),
                    ));
                    spans.push(Span::styled(format!(" {seg}{pad}"), label_style));
                } else {
                    spans.push(Span::styled(format!("   {seg}{pad}"), label_style));
                }
                Line::from(spans)
            })
            .collect()
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
        // Width first: wrapping (and therefore the height) depends on it.
        // borders(2) + one column of padding per side(2).
        let (strip, chips) = self.tab_strip(theme);
        let strip_w = chips.last().map(|&(_, x1, _)| x1).unwrap_or(0);
        let width = modal_width(area, self.natural_width().max(strip_w).saturating_add(4));
        let text_w = width.saturating_sub(4);
        let body = self.body_rows(text_w, theme);
        // borders(2) + tabs(1) + blank(1) + body + blank(1) + footer(1).
        let want_h = (body.len() as u16).saturating_add(6);
        let popup = modal_rect(area, width, modal_height(area, want_h));
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

        // Tab strip, scrolled so the active chip stays visible; chips are
        // recorded (clipped to the visible span) for click hit-testing.
        let off = strip_scroll(&chips, self.tab, inner.width);
        self.hits.strip_row = inner.y;
        self.hits.chips = chips
            .into_iter()
            .filter_map(|(x0, x1, t)| {
                let (x0, x1) = (x0.max(off), x1.min(off + inner.width));
                (x1 > x0).then(|| (inner.x + x0 - off, inner.x + x1 - off, t))
            })
            .collect();
        f.render_widget(
            Paragraph::new(strip).style(bg).scroll((0, off)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        // Body window (below tabs + blank, above blank + footer), scrolled to
        // keep the focused row visible.
        let body_top = inner.y + 2;
        let body_h = inner.height.saturating_sub(4) as usize;
        // Scroll by the FIRST visual line of the focused option, so a wrapped
        // block is entered from its top.
        let cursor_row = body
            .iter()
            .position(|b| b.tag == Some(self.cursor))
            .unwrap_or(0);
        let start = scroll_start(cursor_row, body.len(), body_h);
        let mut y = body_top;
        for b in body.iter().skip(start).take(body_h) {
            self.hits.rows.push((y, b.tag));
            if b.submit {
                self.hits.submit_row = Some(y);
            }
            f.render_widget(
                Paragraph::new(b.line.clone()).style(bg),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y = y.saturating_add(1);
        }

        // Scroll hints go on the blank rows framing the body, so they never
        // overwrite content.
        let hint = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
        if start > 0 {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled("↑", hint))).style(bg),
                Rect::new(inner.x + inner.width.saturating_sub(1), inner.y + 1, 1, 1),
            );
        }
        if start + body_h < body.len() {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled("↓", hint))).style(bg),
                Rect::new(
                    inner.x + inner.width.saturating_sub(1),
                    inner.y + inner.height - 2,
                    1,
                    1,
                ),
            );
        }

        // Footer.
        f.render_widget(
            Paragraph::new(self.footer(theme)).style(bg),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
        );
    }
}

/// A Submit-tab summary row: `label` in subtle ink, then the answer. The
/// answer wraps under itself while the label leaves room; when the label alone
/// eats the width, it wraps first and the answer follows, indented.
fn summary_lines(
    label: &str,
    answer: &str,
    width: u16,
    label_style: Style,
    answer_style: Style,
) -> Vec<Line<'static>> {
    let lw = text_width(label);
    if width.saturating_sub(lw) >= 8 {
        let indent = " ".repeat(lw as usize);
        return wrap_text(answer, width - lw)
            .into_iter()
            .enumerate()
            .map(|(i, seg)| {
                let head = if i == 0 {
                    label.to_string()
                } else {
                    indent.clone()
                };
                Line::from(vec![
                    Span::styled(head, label_style),
                    Span::styled(seg, answer_style),
                ])
            })
            .collect();
    }
    let mut lines: Vec<Line<'static>> = wrap_text(label.trim_end(), width)
        .into_iter()
        .map(|seg| Line::from(Span::styled(seg, label_style)))
        .collect();
    lines.extend(
        wrap_text(answer, width.saturating_sub(3))
            .into_iter()
            .map(|seg| Line::from(Span::styled(format!("   {seg}"), answer_style))),
    );
    lines
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

    /// Columns a rendered line occupies.
    fn line_width(line: &Line<'_>) -> u16 {
        line.spans.iter().map(|s| text_width(&s.content)).sum()
    }

    /// Draw once on a `w`×`h` test terminal and hand back the dialog's body,
    /// re-wrapped at exactly the width the render chose.
    fn draw(d: &mut QuestionDialog, w: u16, h: u16) -> (Vec<BodyLine>, Rect) {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let popup = d.hits.popup;
        (d.body_rows(popup.width - 4, &theme), popup)
    }

    #[tokio::test]
    async fn long_cjk_content_wraps_inside_the_body_width() {
        let long = "请从下面的方案里挑选一个你希望我们采用的实现路径，选定之后我会立刻开始动手改造相关模块并补齐测试";
        let (queue, mut rx) = question_queue();
        let _asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec(long, &[long, "短选项"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        let (body, popup) = draw(&mut d, 90, 40);
        let text_w = popup.width - 4;

        // The question alone needs several lines, and nothing overflows.
        assert!(body.iter().filter(|b| b.tag.is_none()).count() > 2);
        for b in &body {
            assert!(
                line_width(&b.line) <= text_w,
                "line wider than {text_w}: {:?}",
                b.line
            );
        }
    }

    #[tokio::test]
    async fn clicking_a_wrapped_options_continuation_line_selects_it() {
        let long = "使用增量式的迁移方案，先把新的渲染管线挂在旧接口后面跑一段时间，确认没有回归之后再删掉旧实现";
        let (queue, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            queue
                .ask_specs(vec![spec("怎么做", &[long, "直接重写"])], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        draw(&mut d, 90, 40);

        // Every visual line of the wrapped option carries its tag.
        let rows: Vec<u16> = d
            .hits
            .rows
            .iter()
            .filter(|(_, tag)| *tag == Some(0))
            .map(|(y, _)| *y)
            .collect();
        assert!(rows.len() > 1, "option should wrap: {rows:?}");

        // Clicking a continuation line picks that option (single question →
        // straight to the Submit tab).
        d.cursor = 1;
        assert!(!d.on_mouse(d.hits.popup.x + 3, rows[1]));
        assert!(d.on_submit_tab());
        assert!(d.on_key(KeyCode::Enter));
        assert_eq!(asker.await.unwrap(), Some(vec![long.to_string()]));
    }

    #[tokio::test]
    async fn popup_widens_for_long_content_but_stays_inside_the_terminal() {
        let (queue, mut rx) = question_queue();
        let _asker =
            tokio::spawn(
                async move { queue.ask_specs(vec![spec("pick", &["a", "b"])], None).await },
            );
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        let (_, short) = draw(&mut d, 90, 40);
        assert_eq!(short.width, 76, "short content keeps the classic width");

        let long =
            "把整条渲染链路拆成可组合的小步骤，并为每一步补上独立的快照测试与回归基线".repeat(2);
        let (queue, mut rx) = question_queue();
        let _asker =
            tokio::spawn(
                async move { queue.ask_specs(vec![spec(&long, &["a", "b"])], None).await },
            );
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        let (body, wide) = draw(&mut d, 90, 40);
        assert!(wide.width > 76, "long content should widen: {}", wide.width);
        assert!(wide.width <= 90 - 6, "popup must fit the terminal");
        assert!(wide.x + wide.width <= 90);
        for b in &body {
            assert!(line_width(&b.line) <= wide.width - 4);
        }
    }

    #[tokio::test]
    async fn submit_summary_wraps_and_keeps_its_action_row_clickable() {
        let long = "先补齐单元测试，再逐步替换旧的实现，最后清理掉不再使用的兼容分支和相关配置项";
        let (queue, mut rx) = question_queue();
        let asker =
            tokio::spawn(async move { queue.ask_specs(vec![spec("怎么做", &[long])], None).await });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(!d.on_key(KeyCode::Enter)); // answer → Submit tab
        let (body, popup) = draw(&mut d, 60, 40);
        for b in &body {
            assert!(line_width(&b.line) <= popup.width - 4);
        }
        let row = d.hits.submit_row.expect("submit row rendered");
        assert!(d.on_mouse(popup.x + 3, row));
        assert_eq!(asker.await.unwrap(), Some(vec![long.to_string()]));
    }

    #[tokio::test]
    async fn active_tab_chip_stays_visible_when_the_strip_overflows() {
        let names = ["渲染管线的重构方式", "测试覆盖策略", "发布节奏", "回滚方案"];
        let (queue, mut rx) = question_queue();
        let _asker = tokio::spawn(async move {
            let specs = names
                .iter()
                .map(|n| QuestionSpec {
                    question: "pick".to_string(),
                    header: Some(n.to_string()),
                    options: vec!["a".to_string(), "b".to_string()],
                })
                .collect();
            queue.ask_specs(specs, None).await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        d.goto_tab(d.submit_tab());
        draw(&mut d, 60, 40);
        let popup = d.hits.popup;
        let chip = d
            .hits
            .chips
            .iter()
            .find(|(_, _, t)| *t == d.submit_tab())
            .copied()
            .expect("active chip visible");
        assert!(chip.0 >= popup.x && chip.1 <= popup.x + popup.width);
        // Clicking it still lands on the same tab.
        assert!(!d.on_mouse(chip.0, d.hits.strip_row));
        assert!(d.on_submit_tab());
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
