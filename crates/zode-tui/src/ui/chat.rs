//! Conversation view: holds the message list, the streaming delta buffer,
//! and scroll state. Renders into a ratatui Frame.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::theme::Theme;
use crate::ui::markdown::render_markdown;

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
        if !self.streaming {
            self.begin_assistant();
        }
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
        let total = lines.len() as u16;
        let viewport = area.height;
        let max_scroll = total.saturating_sub(viewport);
        let offset = max_scroll.saturating_sub(self.scroll_back.min(max_scroll));

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
