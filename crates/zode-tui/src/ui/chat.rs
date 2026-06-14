//! Conversation view: holds the message list, the streaming delta buffer,
//! and scroll state. Renders into a ratatui Frame.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
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

const THINKING_PREFIX: &str = "Thinking: ";
const ASSISTANT_BODY_INDENT: &str = "  ";

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
    active_assistant_index: Option<usize>,
    /// Lines scrolled up from the bottom (0 = following the tail).
    scroll_back: usize,
    last_render_total_rows: usize,
}

impl ChatView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn push_user(&mut self, text: &str) {
        self.streaming = false;
        self.active_assistant_index = None;
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
        self.push_process_message(ChatMessage {
            role: Role::Tool,
            text: text.to_string(),
        });
    }

    pub fn push_thinking_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let tail_is_thinking = matches!(
            self.process_tail(),
            Some(m) if m.role == Role::Tool && m.text.starts_with(THINKING_PREFIX)
        );
        let thinking_idx = if tail_is_thinking {
            self.process_tail_index().expect("process tail exists")
        } else {
            self.push_process_message(ChatMessage {
                role: Role::Tool,
                text: THINKING_PREFIX.to_string(),
            })
        };
        if let Some(msg) = self.messages.get_mut(thinking_idx) {
            msg.text.push_str(delta);
        }
    }

    pub fn begin_assistant(&mut self) {
        self.messages.push(ChatMessage {
            role: Role::Assistant,
            text: String::new(),
        });
        self.active_assistant_index = self.messages.len().checked_sub(1);
        self.streaming = true;
    }

    pub fn push_delta(&mut self, delta: &str) {
        let idx = match self.active_assistant_index {
            Some(idx) if matches!(self.messages.get(idx), Some(m) if m.role == Role::Assistant) => {
                idx
            }
            _ => {
                self.messages.push(ChatMessage {
                    role: Role::Assistant,
                    text: String::new(),
                });
                self.messages.len() - 1
            }
        };
        self.active_assistant_index = Some(idx);
        self.streaming = true;
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.text.push_str(delta);
        }
    }

    fn push_process_message(&mut self, msg: ChatMessage) -> usize {
        if self.streaming {
            if let Some(idx) = self.active_assistant_index {
                self.messages.insert(idx, msg);
                let shifted_idx = idx + 1;
                self.active_assistant_index = Some(shifted_idx);
                return idx;
            }
        }

        self.messages.push(msg);
        self.messages.len() - 1
    }

    fn process_tail_index(&self) -> Option<usize> {
        match self.active_assistant_index {
            Some(idx) if self.streaming && idx > 0 => Some(idx - 1),
            _ => self.messages.len().checked_sub(1),
        }
    }

    fn process_tail(&self) -> Option<&ChatMessage> {
        self.process_tail_index()
            .and_then(|idx| self.messages.get(idx))
    }

    pub fn end_turn(&mut self) {
        self.streaming = false;
        self.active_assistant_index = None;
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_back = self.scroll_back.saturating_add(n as usize);
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_back = self.scroll_back.saturating_sub(n as usize);
    }

    fn scroll_offset_for_render(&mut self, total_rows: usize, viewport_rows: usize) -> u16 {
        if self.scroll_back > 0
            && self.last_render_total_rows > 0
            && total_rows > self.last_render_total_rows
        {
            self.scroll_back = self
                .scroll_back
                .saturating_add(total_rows - self.last_render_total_rows);
        }

        let max_scroll = total_rows.saturating_sub(viewport_rows);
        let back = self.scroll_back.min(max_scroll);
        self.scroll_back = back;
        self.last_render_total_rows = total_rows;
        u16::try_from(max_scroll.saturating_sub(back)).unwrap_or(u16::MAX)
    }

    /// Build all lines, then render the visible window into `area`.
    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme, meta: ChatRenderMeta<'_>) {
        let mut lines: Vec<Line<'static>> = if self.messages.is_empty() {
            self.render_empty(theme, meta)
        } else {
            let mut out = vec![Line::from("")];
            for (idx, msg) in self.messages.iter().enumerate() {
                if idx > 0 && should_insert_message_gap(&self.messages[idx - 1].role, &msg.role) {
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
        let offset = self.scroll_offset_for_render(total, viewport);

        let para = Paragraph::new(lines)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().bg(theme.bg_primary).fg(theme.fg_text))
            .wrap(Wrap { trim: false })
            .scroll((offset, 0));
        f.render_widget(para, area);
    }

    fn render_empty(&self, theme: &Theme, _meta: ChatRenderMeta<'_>) -> Vec<Line<'static>> {
        let panel = Style::default().bg(theme.bg_secondary);
        let rail = Style::default().bg(theme.bg_secondary).fg(theme.accent);
        let title = Style::default()
            .bg(theme.bg_secondary)
            .fg(theme.fg_white)
            .add_modifier(Modifier::BOLD);
        let muted = Style::default().bg(theme.bg_secondary).fg(theme.fg_subtle);
        let accent = Style::default()
            .bg(theme.bg_secondary)
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD);
        let accent2 = Style::default()
            .bg(theme.bg_secondary)
            .fg(theme.accent_secondary);
        vec![
            Line::from(vec![
                Span::styled(
                    "▌ ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{} ", theme.icon_logo), rail),
                Span::styled("zode", title),
                Span::styled(" workbench ", muted),
                Span::styled(":: command deck", accent2),
            ]),
            Line::from(vec![
                Span::styled("  ", panel),
                Span::styled(" /help ", accent),
                Span::styled("commands", muted),
                Span::styled("  ╱  ", accent2),
                Span::styled(" /model ", accent),
                Span::styled("engines", muted),
                Span::styled("  ╱  ", accent2),
                Span::styled(" /theme ", accent),
                Span::styled("palettes", muted),
                Span::styled("  ╱  ", accent2),
                Span::styled(" /sessions ", accent),
                Span::styled("timeline", muted),
                Span::styled("  ╱  ", accent2),
                Span::styled(" /tasks ", accent),
                Span::styled("jobs", muted),
            ]),
            Line::from(vec![
                Span::styled("  ", panel),
                Span::styled("neon rails online", accent2),
                Span::styled("  •  ", muted),
                Span::styled("slash commands are hot", muted),
            ]),
        ]
    }

    fn render_message(&self, msg: &ChatMessage, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        match msg.role {
            Role::User => render_user_bar(&msg.text, theme, width),
            Role::Assistant => render_plain_markdown(&msg.text, theme, width),
            Role::System => render_process_line(
                "⚡ ",
                &msg.text,
                Style::default()
                    .fg(theme.system)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.fg_subtle),
                width,
            ),
            Role::Tool => render_tool_line(&msg.text, theme, width),
        }
    }
}

fn should_insert_message_gap(prev: &Role, next: &Role) -> bool {
    !matches!((prev, next), (Role::Tool, Role::Assistant))
}

fn render_user_bar(text: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let style = Style::default().bg(theme.bg_secondary).fg(theme.fg_white);
    let rail = Span::styled("▌ ", Style::default().bg(theme.bg_secondary).fg(theme.user));
    let mut out = vec![blank_user_bar_line(theme, width)];
    out.extend(
        text.lines()
            .chain((text.is_empty()).then_some(""))
            .flat_map(|line| {
                wrap_spans_with_prefix(
                    vec![Span::styled(line.to_string(), style)],
                    rail.clone(),
                    Span::styled("  ", style),
                    width,
                )
            })
            .map(|line| pad_line_to_width(line, width, style)),
    );
    out.push(blank_user_bar_line(theme, width));
    out
}

fn blank_user_bar_line(theme: &Theme, width: u16) -> Line<'static> {
    let style = Style::default().bg(theme.bg_secondary).fg(theme.fg_white);
    let rail = Span::styled("▌ ", Style::default().bg(theme.bg_secondary).fg(theme.user));
    pad_line_to_width(Line::from(vec![rail]), width, style)
}

fn render_plain_markdown(src: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let rendered = render_markdown(src, theme);
    if rendered.is_empty() {
        return vec![Line::from("")];
    }
    rendered
        .into_iter()
        .filter(line_has_content)
        .flat_map(|line| {
            wrap_spans_with_prefix(
                line.spans,
                Span::raw(ASSISTANT_BODY_INDENT),
                Span::raw(ASSISTANT_BODY_INDENT),
                width,
            )
        })
        .collect()
}

fn line_has_content(line: &Line<'static>) -> bool {
    line.spans
        .iter()
        .any(|span| !span.content.as_ref().is_empty())
}

fn render_tool_line(text: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    if text == "Thinking…" {
        return render_tool_process_line(
            "Thinking: ",
            "",
            Style::default()
                .fg(theme.system)
                .add_modifier(Modifier::ITALIC),
            Style::default().fg(theme.fg_subtle),
            width,
        );
    }
    if let Some(thinking) = text.strip_prefix(THINKING_PREFIX) {
        return render_tool_process_line(
            "Thinking: ",
            thinking,
            Style::default()
                .fg(theme.system)
                .add_modifier(Modifier::ITALIC),
            Style::default().fg(theme.fg_subtle),
            width,
        );
    }
    render_tool_process_line(
        "▪ ",
        text,
        Style::default().fg(theme.accent_secondary),
        Style::default().fg(theme.fg_subtle),
        width,
    )
}

fn render_tool_process_line(
    prefix: &str,
    text: &str,
    prefix_style: Style,
    body_style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let first_prefix = format!("  {prefix}");
    let continuation = format!("  {}", " ".repeat(UnicodeWidthStr::width(prefix)));
    wrap_spans_with_prefix(
        vec![Span::styled(text.to_string(), body_style)],
        Span::styled(first_prefix, prefix_style),
        Span::styled(continuation, prefix_style),
        width,
    )
}

fn render_process_line(
    prefix: &str,
    text: &str,
    prefix_style: Style,
    body_style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    let first_prefix = format!("│  {prefix}");
    let continuation = format!("│  {}", " ".repeat(UnicodeWidthStr::width(prefix)));
    wrap_spans_with_prefix(
        vec![Span::styled(text.to_string(), body_style)],
        Span::styled(first_prefix, prefix_style),
        Span::styled(continuation, prefix_style),
        width,
    )
}

fn pad_line_to_width(mut line: Line<'static>, width: u16, style: Style) -> Line<'static> {
    let current_width: usize = line
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    let target = width as usize;
    if current_width < target {
        line.spans
            .push(Span::styled(" ".repeat(target - current_width), style));
    }
    line
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
    fn delta_after_tool_stays_in_active_assistant_answer() {
        // Process rows belong above the current answer, but they must not
        // become the append target for subsequent assistant text.
        let mut view = ChatView::new();
        view.push_user("hi");
        view.push_delta("part1 ");
        view.push_tool("Bash");
        view.push_delta("part2");
        let msgs = view.messages();
        assert_eq!(msgs.len(), 3); // user, tool, assistant(part1+part2)
        assert_eq!(msgs[1].role, Role::Tool);
        assert_eq!(msgs[1].text, "Bash");
        assert_eq!(msgs[2].role, Role::Assistant);
        assert_eq!(msgs[2].text, "part1 part2");
    }

    #[test]
    fn thinking_deltas_append_to_one_process_message() {
        let mut view = ChatView::new();
        view.push_thinking_delta("The user asked ");
        view.push_thinking_delta("for a file.");

        let msgs = view.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::Tool);
        assert_eq!(msgs[0].text, "Thinking: The user asked for a file.");
    }

    #[test]
    fn process_events_stay_above_active_assistant_answer() {
        let mut view = ChatView::new();
        view.push_user("why did clone timeout?");
        view.push_delta("It timed out because ");
        view.push_thinking_delta("Checking clone duration.");
        view.push_tool("Tool Bash git status");
        view.push_delta("the first attempt hit the shorter timeout.");

        let msgs = view.messages();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[1].role, Role::Tool);
        assert_eq!(msgs[1].text, "Thinking: Checking clone duration.");
        assert_eq!(msgs[2].role, Role::Tool);
        assert_eq!(msgs[2].text, "Tool Bash git status");
        assert_eq!(msgs[3].role, Role::Assistant);
        assert_eq!(
            msgs[3].text,
            "It timed out because the first attempt hit the shorter timeout."
        );
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
        let mut view = ChatView::new();
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
        assert!(content.contains("command deck"));
        assert!(content.contains("engines"));
        assert!(content.contains("palettes"));
    }

    #[test]
    fn messages_render_as_transcript_rows() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user("search opencode");
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
        assert!(!content.contains("You"));
        assert!(!content.contains("Assistant"));
        assert!(content.contains("search opencode"));
        assert!(content.contains("bold"));
        assert!(content.contains("Bash"));
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
        let row2: String = (0..buf.area.width).map(|x| buf[(x, 2)].symbol()).collect();
        assert!(row0.trim().is_empty(), "first row should breathe: {row0:?}");
        assert!(
            row1.starts_with("▌") && !row1.contains("hello"),
            "user block should have rail-only top padding: {row1:?}"
        );
        assert!(
            row2.contains("hello"),
            "third row should contain the user message after padding: {row2:?}"
        );
    }

    #[test]
    fn user_messages_render_with_vertical_padding() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        view.push_user("hello");
        view.push_thinking_delta("thinking");
        let backend = TestBackend::new(44, 9);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();

        let buf = term.backend().buffer();
        let user_row = (0..buf.area.height)
            .find(|&y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("hello")
            })
            .expect("user row should be rendered");
        let top = user_row.saturating_sub(1);
        let bottom = user_row.saturating_add(1);
        assert!(
            (0..buf.area.width).all(|x| buf[(x, top)].bg == theme.bg_secondary),
            "user top padding should use panel background"
        );
        assert!(
            (0..buf.area.width).all(|x| buf[(x, bottom)].bg == theme.bg_secondary),
            "user bottom padding should use panel background"
        );
        assert_eq!(buf[(0, top)].symbol(), "▌");
        assert_eq!(buf[(0, bottom)].symbol(), "▌");
    }

    #[test]
    fn assistant_answer_sits_right_under_process_tail() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        view.push_tool("Usage ↑378 ↓46");
        view.push_delta("我是 DeepSeek-V4-Pro 模型。");
        let backend = TestBackend::new(50, 6);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();

        let buf = term.backend().buffer();
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let usage_row = rows
            .iter()
            .position(|row| row.contains("Usage"))
            .expect("usage row");
        assert!(
            rows.get(usage_row + 1)
                .is_some_and(|row| row.contains("DeepSeek")),
            "assistant row should immediately follow usage row: {rows:?}"
        );
    }

    #[test]
    fn scroll_position_stays_stable_while_streaming_content_grows() {
        let mut view = ChatView::new();
        view.scroll_back = 4;

        let first_offset = view.scroll_offset_for_render(40, 10);
        let second_offset = view.scroll_offset_for_render(45, 10);

        assert_eq!(first_offset, 26);
        assert_eq!(second_offset, first_offset);
        assert_eq!(view.scroll_back, 9);
    }

    #[test]
    fn tail_following_stays_at_bottom_while_streaming_content_grows() {
        let mut view = ChatView::new();

        let first_offset = view.scroll_offset_for_render(40, 10);
        let second_offset = view.scroll_offset_for_render(45, 10);

        assert_eq!(first_offset, 30);
        assert_eq!(second_offset, 35);
        assert_eq!(view.scroll_back, 0);
    }

    #[test]
    fn user_messages_render_with_left_agent_rail() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        view.push_user("search opencode");
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
        let user_row = (0..buf.area.height)
            .find(|&y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("search opencode")
            })
            .expect("user row should be rendered");
        assert!(
            (0..buf.area.width).all(|x| buf[(x, user_row)].bg == theme.bg_secondary),
            "user row should use a full-width background"
        );
        assert_eq!(buf[(0, user_row)].symbol(), "▌");
        assert_eq!(buf[(0, user_row)].fg, theme.user);
    }

    #[test]
    fn assistant_messages_render_without_role_rails() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "hello".into(),
            },
            &theme,
            80,
        );

        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert_eq!(joined.trim(), "hello");
        assert!(!joined.contains("Assistant"));
        assert!(!joined.contains("│"));
    }

    #[test]
    fn assistant_messages_render_with_left_padding() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "hello".into(),
            },
            &theme,
            80,
        );

        let first_row: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(first_row.starts_with("  hello"));
    }

    #[test]
    fn assistant_markdown_compacts_empty_paragraph_gap() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "第一段\n\n第二段".into(),
            },
            &theme,
            80,
        );

        let rows: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect();
        assert_eq!(rows, vec!["  第一段".to_string(), "  第二段".to_string()]);
    }

    #[test]
    fn process_lines_are_muted_not_role_blocks() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Tool,
                text: "Bash cargo build".into(),
            },
            &theme,
            80,
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.accent_secondary));
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.fg_subtle));
        let joined: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(!joined.contains("│"));
    }

    #[test]
    fn thinking_lines_render_without_left_process_rail() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Tool,
                text: "Thinking: checking context".into(),
            },
            &theme,
            80,
        );

        let joined: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("Thinking: "));
        assert!(!joined.contains("│"));
    }

    #[test]
    fn minimal_theme_distinguishes_user_and_assistant_colors() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));

        assert_ne!(theme.user, theme.assistant);
    }

    #[test]
    fn wrapped_assistant_cjk_lines_keep_body_indent() {
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
            body_rows.iter().all(|row| !row.starts_with("│ ")),
            "wrapped body rows should not use role rails: {body_rows:?}"
        );
    }
}
