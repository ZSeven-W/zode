//! Conversation view: holds the message list, the streaming delta buffer,
//! and scroll state. Renders into a ratatui Frame.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

#[derive(Debug, Clone, Copy)]
pub struct ChatRenderMeta<'a> {
    pub theme_name: &'a str,
    pub model: &'a str,
    pub cwd: &'a Path,
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
    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme, meta: ChatRenderMeta<'_>) {
        let mut lines: Vec<Line<'static>> = if self.messages.is_empty() {
            self.render_empty(theme, meta)
        } else {
            let mut out = vec![Line::from("")];
            for (idx, msg) in self.messages.iter().enumerate() {
                if idx > 0 {
                    out.push(Line::from(""));
                }
                out.extend(self.render_message(msg, theme, area.width));
            }
            out
        };

        if lines.is_empty() {
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

    fn render_empty(&self, theme: &Theme, _meta: ChatRenderMeta<'_>) -> Vec<Line<'static>> {
        vec![
            Line::from(vec![
                Span::styled(
                    format!("{} ", theme.icon_logo),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "zode",
                    Style::default()
                        .fg(theme.fg_white)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" workbench", Style::default().fg(theme.fg_subtle)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("/help", Style::default().fg(theme.accent)),
                Span::styled(" commands   ", Style::default().fg(theme.fg_subtle)),
                Span::styled("/model", Style::default().fg(theme.accent)),
                Span::styled(" switch   ", Style::default().fg(theme.fg_subtle)),
                Span::styled("/theme", Style::default().fg(theme.accent)),
                Span::styled(" switch   ", Style::default().fg(theme.fg_subtle)),
                Span::styled("/sessions", Style::default().fg(theme.accent)),
                Span::styled(" resume   ", Style::default().fg(theme.fg_subtle)),
                Span::styled("/tasks", Style::default().fg(theme.accent)),
                Span::styled(" shells", Style::default().fg(theme.fg_subtle)),
            ]),
        ]
    }

    fn render_message(&self, msg: &ChatMessage, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        match msg.role {
            Role::User => self.render_role_block(
                &theme.icon_user,
                "You",
                &msg.text,
                theme.user,
                Style::default().fg(theme.fg_text),
                width,
            ),
            Role::Assistant => {
                let mut out =
                    vec![self.role_header(&theme.icon_assistant, "Assistant", theme.assistant)];
                out.extend(rail_markdown(&msg.text, theme, theme.assistant, width));
                out
            }
            Role::System => self.render_role_block(
                &theme.icon_system,
                "System",
                &msg.text,
                theme.system,
                Style::default().fg(theme.fg_subtle),
                width,
            ),
            Role::Tool => vec![Line::from(vec![
                Span::styled("  · ", Style::default().fg(theme.accent_secondary)),
                Span::styled(msg.text.clone(), Style::default().fg(theme.fg_subtle)),
            ])],
        }
    }

    fn render_role_block(
        &self,
        icon: &str,
        label: &str,
        text: &str,
        color: Color,
        body_style: Style,
        width: u16,
    ) -> Vec<Line<'static>> {
        let mut out = vec![self.role_header(icon, label, color)];
        for line in text.lines().chain((text.is_empty()).then_some("")) {
            out.extend(rail_line(
                vec![Span::styled(line.to_string(), body_style)],
                color,
                width,
            ));
        }
        out
    }

    fn role_header(&self, icon: &str, label: &str, color: Color) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("{icon} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                label.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])
    }
}

fn rail_markdown(src: &str, theme: &Theme, rail_color: Color, width: u16) -> Vec<Line<'static>> {
    let rendered = render_markdown(src, theme);
    if rendered.is_empty() {
        return rail_line(Vec::new(), rail_color, width);
    }
    rendered
        .into_iter()
        .flat_map(|line| rail_line(line.spans, rail_color, width))
        .collect()
}

fn rail_line(spans: Vec<Span<'static>>, rail_color: Color, width: u16) -> Vec<Line<'static>> {
    let rail = Span::styled("│ ", Style::default().fg(rail_color));
    wrap_spans_with_prefix(spans, rail.clone(), rail, width)
}

fn wrap_spans_with_prefix(
    spans: Vec<Span<'static>>,
    first_prefix: Span<'static>,
    continuation_prefix: Span<'static>,
    width: u16,
) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let max_width = width as usize;
    let continuation_width = UnicodeWidthStr::width(continuation_prefix.content.as_ref());
    let mut prefix = first_prefix;
    let mut current_width = UnicodeWidthStr::width(prefix.content.as_ref());
    let mut current = vec![prefix.clone()];
    let mut out = Vec::new();
    let mut saw_content = false;

    for span in spans {
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            let has_body = current.len() > 1;
            if has_body && current_width + ch_width > max_width {
                out.push(Line::from(current));
                prefix = continuation_prefix.clone();
                current_width = continuation_width;
                current = vec![prefix.clone()];
            }
            current.push(Span::styled(ch.to_string(), span.style));
            current_width += ch_width;
            saw_content = true;
        }
    }

    if saw_content || out.is_empty() {
        out.push(Line::from(current));
    }
    out
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
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "MiniMax-M1",
            cwd: std::path::Path::new("/tmp/zode"),
        };
        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("hello"));
    }

    #[test]
    fn empty_state_renders_minimal_workbench_shortcuts() {
        let theme = ThemeStore::with_builtins().resolve(Some("hacker"));
        let view = ChatView::new();
        let backend = TestBackend::new(80, 10);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "MiniMax-M1",
            cwd: std::path::Path::new("/Users/kayshen/Workspace/ZSeven-W/zode"),
        };
        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("zode"));
        assert!(!content.contains("Hacker"));
        assert!(!content.contains("MiniMax-M1"));
        assert!(content.contains("/help"));
        assert!(content.contains("/model"));
        assert!(content.contains("/theme"));
    }

    #[test]
    fn messages_render_role_headers_and_rails() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user("hello");
        view.push_delta("**bold** reply");
        view.push_tool("Bash");
        view.push_system("done");
        let backend = TestBackend::new(80, 14);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "MiniMax-M1",
            cwd: std::path::Path::new("/tmp/zode"),
        };
        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("You"));
        assert!(content.contains("Assistant"));
        assert!(content.contains("System"));
        assert!(content.contains("│ hello"));
        assert!(content.contains("· Bash"));
    }

    #[test]
    fn rendered_chat_has_top_padding_before_first_message() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        view.push_user("hello");
        let backend = TestBackend::new(40, 6);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();

        let buf = term.backend().buffer();
        let row0: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        let row1: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol()).collect();
        assert!(row0.trim().is_empty(), "first row should breathe: {row0:?}");
        assert!(
            row1.contains("You"),
            "second row should start the message: {row1:?}"
        );
    }

    #[test]
    fn user_messages_use_same_rail_layout_as_assistant() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let user = view.render_message(
            &ChatMessage {
                role: Role::User,
                text: "hello".into(),
            },
            &theme,
            80,
        );
        let assistant = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "hello".into(),
            },
            &theme,
            80,
        );

        assert_eq!(user[1].spans[0].content, "│ ");
        assert_eq!(assistant[1].spans[0].content, "│ ");
        assert_eq!(user[1].spans[0].style.fg, Some(theme.user));
        assert_eq!(assistant[1].spans[0].style.fg, Some(theme.assistant));
    }

    #[test]
    fn user_and_assistant_headers_use_matching_role_styles() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();

        let user = view.render_message(
            &ChatMessage {
                role: Role::User,
                text: "hello".into(),
            },
            &theme,
            80,
        );
        let assistant = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "hello".into(),
            },
            &theme,
            80,
        );

        assert_eq!(user[0].spans[1].style.fg, Some(theme.user));
        assert_eq!(assistant[0].spans[1].style.fg, Some(theme.assistant));
    }

    #[test]
    fn minimal_theme_distinguishes_user_and_assistant_colors() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));

        assert_ne!(theme.user, theme.assistant);
    }

    #[test]
    fn wrapped_assistant_cjk_lines_keep_the_body_rail() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let mut view = ChatView::new();
        view.push_delta(
            "我是 Zode，一个运行在终端中的 AI 原生编程助手。我是 ZSeven-W/zode 项目的一部分，专门帮助你处理软件工程任务。",
        );
        let backend = TestBackend::new(36, 8);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "MiniMax-M1",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();

        let buf = term.backend().buffer();
        let body_rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .filter(|row| {
                let trimmed = row.trim();
                !trimmed.is_empty() && !trimmed.contains("Assistant")
            })
            .collect();

        assert!(body_rows.len() > 1, "expected wrapped body rows");
        assert!(
            body_rows.iter().all(|row| row.starts_with("│ ")),
            "all wrapped body rows should keep the rail prefix: {body_rows:?}"
        );
    }
}
