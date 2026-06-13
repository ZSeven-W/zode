//! Conversation view: holds the message list, the streaming delta buffer,
//! and scroll state. Renders into a ratatui Frame.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::ui::markdown::render_markdown;

/// Approximate the number of wrapped rows a line occupies at `width`
/// columns (ratatui wraps on words, so this char-width estimate is close
/// enough for scroll math and, crucially, never overflows).
fn wrapped_rows(line: &Line, width: u16) -> usize {
    if width == 0 {
        return 1;
    }
    let w: usize = line
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    w.div_ceil(width as usize).max(1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct ChatView {
    messages: Vec<ChatMessage>,
    streaming: bool,
    /// Lines scrolled up from the bottom (0 = following the tail).
    scroll_back: u16,
}

impl ChatView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn push_user(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            role: Role::User,
            text: text.to_string(),
        });
        self.scroll_back = 0;
    }

    pub fn push_system(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            role: Role::System,
            text: text.to_string(),
        });
    }

    pub fn push_tool(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            role: Role::Tool,
            text: text.to_string(),
        });
    }

    pub fn begin_assistant(&mut self) {
        self.messages.push(ChatMessage {
            role: Role::Assistant,
            text: String::new(),
        });
        self.streaming = true;
    }

    pub fn push_delta(&mut self, delta: &str) {
        // Only a trailing assistant message accepts deltas; if a tool/system
        // card was pushed mid-stream, start a fresh assistant segment so the
        // text doesn't append to the tool card.
        let tail_is_assistant =
            matches!(self.messages.last(), Some(m) if m.role == Role::Assistant);
        if !tail_is_assistant {
            self.messages.push(ChatMessage {
                role: Role::Assistant,
                text: String::new(),
            });
        }
        self.streaming = true;
        if let Some(last) = self.messages.last_mut() {
            last.text.push_str(delta);
        }
    }

    pub fn end_turn(&mut self) {
        self.streaming = false;
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_back = self.scroll_back.saturating_add(n);
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_back = self.scroll_back.saturating_sub(n);
    }

    /// Build all lines, then render the visible window into `area`.
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        for msg in &self.messages {
            lines.extend(self.render_message(msg, theme));
            lines.push(Line::from(""));
        }
        // Count POST-wrap rows so scrolling is correct for wrapped content,
        // and clamp into u16 so a huge history can't overflow. Computed
        // before `lines` moves into the Paragraph.
        let total: usize = lines.iter().map(|l| wrapped_rows(l, area.width)).sum();
        let viewport = area.height as usize;
        let max_scroll = total.saturating_sub(viewport);
        let back = (self.scroll_back as usize).min(max_scroll);
        let offset = u16::try_from(max_scroll - back).unwrap_or(u16::MAX);

        let para = Paragraph::new(lines)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().bg(theme.bg_primary).fg(theme.fg_text))
            .wrap(Wrap { trim: false })
            .scroll((offset, 0));
        f.render_widget(para, area);
    }

    fn render_message(&self, msg: &ChatMessage, theme: &Theme) -> Vec<Line<'static>> {
        match msg.role {
            Role::User => vec![Line::from(vec![
                Span::styled(
                    format!("{} ", theme.icon_user),
                    Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
                ),
                Span::styled(msg.text.clone(), Style::default().fg(theme.fg_white)),
            ])],
            Role::Assistant => {
                let mut out = vec![Line::from(Span::styled(
                    format!("{} ", theme.icon_assistant),
                    Style::default()
                        .fg(theme.assistant)
                        .add_modifier(Modifier::BOLD),
                ))];
                out.extend(render_markdown(&msg.text, theme));
                out
            }
            Role::System => vec![Line::from(Span::styled(
                msg.text.clone(),
                Style::default().fg(theme.system),
            ))],
            Role::Tool => vec![Line::from(Span::styled(
                format!("· {}", msg.text),
                Style::default().fg(theme.fg_subtle),
            ))],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn push_user_then_stream_assistant() {
        let mut view = ChatView::new();
        view.push_user("hello");
        view.begin_assistant();
        view.push_delta("hi ");
        view.push_delta("there");
        view.end_turn();
        assert_eq!(view.messages().len(), 2);
        assert_eq!(view.messages()[1].text, "hi there");
    }

    #[test]
    fn delta_after_tool_starts_new_assistant_segment() {
        // BLOCK regression: a tool card pushed mid-stream must not become the
        // append target for subsequent assistant text.
        let mut view = ChatView::new();
        view.push_user("hi");
        view.push_delta("part1 ");
        view.push_tool("Bash");
        view.push_delta("part2");
        let msgs = view.messages();
        assert_eq!(msgs.len(), 4); // user, assistant(part1), tool, assistant(part2)
        assert_eq!(msgs[2].role, Role::Tool);
        assert_eq!(msgs[2].text, "Bash");
        assert_eq!(msgs[3].role, Role::Assistant);
        assert_eq!(msgs[3].text, "part2");
    }

    #[test]
    fn renders_without_panic() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user("hello");
        view.begin_assistant();
        view.push_delta("**bold** reply");
        let backend = TestBackend::new(40, 10);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = term.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("hello"));
    }
}
