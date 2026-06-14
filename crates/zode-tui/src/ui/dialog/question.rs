//! Modal opened by the `AskUserQuestion` tool: a single-choice picker over the
//! agent's question. Mirrors [`super::permission::PermissionDialog`]'s
//! request-ownership (the oneshot lives inside the `QuestionRequest`, answered
//! on key) and the plugin picker's option rendering. Closing the dialog
//! responds with the chosen index (Enter) or `None` (Esc dismisses).

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;
use zode_core::question::QuestionRequest;

use crate::theme::Theme;

pub struct QuestionDialog {
    request: Option<QuestionRequest>,
    selected: usize,
}

impl QuestionDialog {
    pub fn new(request: QuestionRequest) -> Self {
        Self {
            request: Some(request),
            selected: 0,
        }
    }

    /// The asking tab's id, so the app can focus that conversation.
    pub fn source(&self) -> Option<String> {
        self.request.as_ref().and_then(|r| r.source.clone())
    }

    fn options_len(&self) -> usize {
        self.request.as_ref().map(|r| r.options.len()).unwrap_or(0)
    }

    /// Handle a key. Returns true once the dialog is done (answered/dismissed),
    /// at which point the response has been sent back to the waiting tool.
    pub fn on_key(&mut self, code: KeyCode) -> bool {
        let len = self.options_len();
        match code {
            KeyCode::Up => {
                if len > 0 {
                    self.selected = self.selected.checked_sub(1).unwrap_or(len - 1);
                }
                false
            }
            KeyCode::Down => {
                if len > 0 {
                    self.selected = (self.selected + 1) % len;
                }
                false
            }
            // Number keys 1-9 pick directly.
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = (c as u8 - b'1') as usize;
                if idx < len {
                    if let Some(req) = self.request.take() {
                        let _ = req.respond(Some(idx));
                    }
                    return true;
                }
                false
            }
            KeyCode::Enter => {
                if let Some(req) = self.request.take() {
                    let _ = req.respond(Some(self.selected));
                }
                true
            }
            KeyCode::Esc => {
                if let Some(req) = self.request.take() {
                    let _ = req.respond(None);
                }
                true
            }
            _ => false,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let Some(req) = &self.request else {
            return;
        };
        let inner_w = 72u16;
        // Height: header + question(up to ~3 wrapped lines) + blank + options + footer.
        let rows = (req.options.len() as u16) + 7;
        let popup = modal_area(area, inner_w, rows);
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
        // Question text (wrapped, up to 3 lines).
        f.render_widget(
            Paragraph::new(req.question.clone())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.fg_white).bg(theme.bg_secondary)),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 3),
        );

        // Options.
        let mut y = inner.y.saturating_add(5);
        let bottom = inner.y.saturating_add(inner.height);
        for (i, opt) in req.options.iter().enumerate() {
            if y >= bottom.saturating_sub(1) {
                break;
            }
            let selected = i == self.selected;
            let prefix = if selected { "> " } else { "  " };
            let row = pad_to_width(format!("{prefix}{}. {opt}", i + 1), inner.width);
            let style = if selected {
                Style::default()
                    .fg(theme.bg_primary)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
            };
            f.render_widget(
                Paragraph::new(row).style(style),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y = y.saturating_add(1);
        }

        // Footer.
        let key = Style::default()
            .fg(theme.fg_white)
            .bg(theme.bg_secondary)
            .add_modifier(Modifier::BOLD);
        let lbl = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("↑↓/1-9", key),
                Span::styled(" choose   ", lbl),
                Span::styled("enter", key),
                Span::styled(" select   ", lbl),
                Span::styled("esc", key),
                Span::styled(" skip", lbl),
            ]))
            .style(Style::default().bg(theme.bg_secondary)),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }
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
    use zode_core::question::question_queue;

    #[tokio::test]
    async fn enter_responds_with_selected_index() {
        let (q, mut rx) = question_queue();
        // The tool side awaits the answer.
        let asker = tokio::spawn(async move {
            q.ask("pick", &["a".into(), "b".into(), "c".into()], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(!d.on_key(KeyCode::Down)); // -> index 1
        assert!(d.on_key(KeyCode::Enter)); // answer
        assert_eq!(asker.await.unwrap(), Some(1));
    }

    #[tokio::test]
    async fn esc_dismisses_with_none() {
        let (q, mut rx) = question_queue();
        let asker =
            tokio::spawn(async move { q.ask("pick", &["a".into(), "b".into()], None).await });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(d.on_key(KeyCode::Esc));
        assert_eq!(asker.await.unwrap(), None);
    }

    #[tokio::test]
    async fn number_key_picks_directly() {
        let (q, mut rx) = question_queue();
        let asker = tokio::spawn(async move {
            q.ask("pick", &["a".into(), "b".into(), "c".into()], None)
                .await
        });
        let req = rx.next().await.unwrap();
        let mut d = QuestionDialog::new(req);
        assert!(d.on_key(KeyCode::Char('3')));
        assert_eq!(asker.await.unwrap(), Some(2));
    }
}
