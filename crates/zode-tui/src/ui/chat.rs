//! Conversation view: holds the message list, the streaming delta buffer,
//! and scroll state. Renders into a ratatui Frame.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;
use crate::ui::markdown::render_markdown;

/// Approximate the number of wrapped rows a line occupies at `width`
/// columns (ratatui wraps on words, so this char-width estimate is close
/// enough for scroll math and, crucially, never overflows).
#[cfg(test)]
fn wrapped_rows(line: &Line, width: u16) -> usize {
    if width == 0 {
        return 1;
    }
    let w = line_disp_width(line);
    w.div_ceil(width as usize).max(1)
}

#[cfg(test)]
fn line_disp_width(line: &Line) -> usize {
    line.spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    pub images: Vec<ImagePreview>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePreview {
    pub display_name: String,
    pub media_type: String,
    pub size_bytes: u64,
}

const THINKING_PREFIX: &str = "Thinking: ";
const ASSISTANT_BODY_INDENT: &str = "  ";

#[derive(Debug, Clone, Copy)]
pub struct ChatRenderMeta<'a> {
    pub theme_name: &'a str,
    pub model: &'a str,
    pub cwd: &'a Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatSelectionPoint {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatSelection {
    pub anchor: ChatSelectionPoint,
    pub focus: ChatSelectionPoint,
}

impl ChatSelection {
    pub fn new(anchor: ChatSelectionPoint, focus: ChatSelectionPoint) -> Self {
        Self { anchor, focus }
    }

    fn normalized(self) -> (ChatSelectionPoint, ChatSelectionPoint) {
        if (self.focus.line, self.focus.column) < (self.anchor.line, self.anchor.column) {
            (self.focus, self.anchor)
        } else {
            (self.anchor, self.focus)
        }
    }
}

#[derive(Debug, Default)]
pub struct ChatView {
    messages: Vec<ChatMessage>,
    streaming: bool,
    active_assistant_index: Option<usize>,
    /// Lines scrolled up from the bottom (0 = following the tail).
    scroll_back: usize,
    last_render_total_rows: usize,
    /// Display prefs (`/thinking`, `/tool-details`). Stored as "hide" so the
    /// `Default` (false) shows everything; messages stay in the log and toggle
    /// live at render time.
    hide_thinking: bool,
    hide_tool_details: bool,
    revision: u64,
    render_cache: Option<RenderCache>,
    /// Per-message rendered-line cache, keyed by a hash of (role, text, width,
    /// theme). Lets a rebuild reuse unchanged messages' lines instead of
    /// re-parsing markdown for the whole transcript on every content change —
    /// the key invalidates automatically on content/width/theme change.
    line_cache: RefCell<HashMap<u64, Vec<Line<'static>>>>,
    #[cfg(test)]
    render_misses: std::cell::Cell<usize>,
    #[cfg(test)]
    cache_builds: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderCacheKey {
    revision: u64,
    width: u16,
    theme_name: String,
    hide_thinking: bool,
    hide_tool_details: bool,
}

#[derive(Debug, Clone)]
struct RenderCache {
    key: RenderCacheKey,
    lines: Vec<Line<'static>>,
}

impl ChatView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the show-thinking / show-tool-detail display preferences. Called
    /// each frame from the app so a `/thinking` or `/tool-details` toggle takes
    /// effect immediately without losing scrollback.
    pub fn set_display_prefs(&mut self, show_thinking: bool, show_tool_details: bool) {
        self.hide_thinking = !show_thinking;
        self.hide_tool_details = !show_tool_details;
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn push_user(&mut self, text: &str) {
        self.push_user_with_images(text, Vec::new());
    }

    pub fn push_user_with_images(&mut self, text: &str, images: Vec<ImagePreview>) {
        self.streaming = false;
        self.active_assistant_index = None;
        self.messages.push(ChatMessage {
            role: Role::User,
            text: text.to_string(),
            images,
        });
        self.scroll_back = 0;
        self.bump_revision();
    }

    pub fn push_system(&mut self, text: &str) {
        self.messages.push(ChatMessage {
            role: Role::System,
            text: text.to_string(),
            images: Vec::new(),
        });
        self.bump_revision();
    }

    pub fn push_tool(&mut self, text: &str) {
        self.push_process_message(ChatMessage {
            role: Role::Tool,
            text: text.to_string(),
            images: Vec::new(),
        });
        self.bump_revision();
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
                images: Vec::new(),
            })
        };
        if let Some(msg) = self.messages.get_mut(thinking_idx) {
            msg.text.push_str(delta);
        }
        self.bump_revision();
    }

    pub fn begin_assistant(&mut self) {
        self.messages.push(ChatMessage {
            role: Role::Assistant,
            text: String::new(),
            images: Vec::new(),
        });
        self.active_assistant_index = self.messages.len().checked_sub(1);
        self.streaming = true;
        self.bump_revision();
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
                    images: Vec::new(),
                });
                self.messages.len() - 1
            }
        };
        self.active_assistant_index = Some(idx);
        self.streaming = true;
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.text.push_str(delta);
        }
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.render_cache = None;
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

    /// Jump to the latest output (follow the tail). End key / new turns.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_back = 0;
    }

    /// Jump to the top of the conversation. Clamped to the real maximum on the
    /// next render (we don't know the row count here). Home key.
    pub fn scroll_to_top(&mut self) {
        self.scroll_back = usize::MAX;
    }

    fn scroll_offset_for_render(&mut self, total_rows: usize, viewport_rows: usize) -> usize {
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
        max_scroll.saturating_sub(back)
    }

    /// Build all lines, then render the visible window into `area`.
    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme, meta: ChatRenderMeta<'_>) {
        self.render_with_selection(f, area, theme, meta, None);
    }

    pub fn render_with_selection(
        &mut self,
        f: &mut Frame,
        area: Rect,
        theme: &Theme,
        meta: ChatRenderMeta<'_>,
        selection: Option<ChatSelection>,
    ) {
        self.ensure_render_cache(theme, meta, area.width);
        let total = self
            .render_cache
            .as_ref()
            .expect("cache populated")
            .lines
            .len();
        let viewport = area.height as usize;
        let offset = self.scroll_offset_for_render(total, viewport);
        let range = visible_line_window(total, viewport, offset);
        let cache = self.render_cache.as_ref().expect("cache populated");
        let mut visible_lines: Vec<Line<'static>> = cache.lines[range.clone()]
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let line_idx = range.start + idx;
                match selection {
                    Some(selection) => {
                        apply_selection_to_line(line.clone(), line_idx, selection, theme)
                    }
                    None => line.clone(),
                }
            })
            .collect();
        if visible_lines.is_empty() {
            visible_lines.push(Line::from(""));
        }

        let para = Paragraph::new(visible_lines)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().bg(theme.bg_primary).fg(theme.fg_text));
        f.render_widget(para, area);
    }

    pub fn selection_point_at(
        &mut self,
        theme: &Theme,
        meta: ChatRenderMeta<'_>,
        area: Rect,
        column: u16,
        row: u16,
    ) -> Option<ChatSelectionPoint> {
        if area.width == 0 || area.height == 0 {
            return None;
        }
        self.ensure_render_cache(theme, meta, area.width);
        let total = self
            .render_cache
            .as_ref()
            .expect("cache populated")
            .lines
            .len();
        if total == 0 {
            return None;
        }
        let line = self.visible_start(area.height as usize)
            + usize::from(
                row.saturating_sub(area.y)
                    .min(area.height.saturating_sub(1)),
            );
        let column = usize::from(
            column
                .saturating_sub(area.x)
                .min(area.width.saturating_sub(1)),
        );
        Some(ChatSelectionPoint {
            line: line.min(total.saturating_sub(1)),
            column,
        })
    }

    pub fn selected_text(
        &mut self,
        selection: ChatSelection,
        theme: &Theme,
        meta: ChatRenderMeta<'_>,
        area: Rect,
    ) -> String {
        self.ensure_render_cache(theme, meta, area.width);
        let Some(cache) = &self.render_cache else {
            return String::new();
        };
        let (start, end) = selection.normalized();
        if start == end {
            return String::new();
        }
        let last_line = cache.lines.len().saturating_sub(1);
        let start_line = start.line.min(last_line);
        let end_line = end.line.min(last_line);
        let mut out = Vec::new();
        for line_idx in start_line..=end_line {
            let text = plain_line_text(&cache.lines[line_idx]);
            let row_text = if start_line == end_line {
                slice_display_cols(&text, start.column, end.column)
            } else if line_idx == start_line {
                slice_display_cols(&text, start.column, usize::MAX)
            } else if line_idx == end_line {
                slice_display_cols(&text, 0, end.column)
            } else {
                text
            };
            out.push(row_text.trim_end().to_string());
        }
        out.join("\n")
    }

    fn ensure_render_cache(&mut self, theme: &Theme, meta: ChatRenderMeta<'_>, width: u16) {
        let cache_key = RenderCacheKey {
            revision: self.revision,
            width,
            theme_name: meta.theme_name.to_string(),
            hide_thinking: self.hide_thinking,
            hide_tool_details: self.hide_tool_details,
        };
        if self
            .render_cache
            .as_ref()
            .map_or(true, |cache| cache.key != cache_key)
        {
            let lines = self.build_lines(theme, meta, width);
            self.render_cache = Some(RenderCache {
                key: cache_key,
                lines,
            });
            #[cfg(test)]
            {
                self.cache_builds += 1;
            }
        }
    }

    fn visible_start(&self, viewport_rows: usize) -> usize {
        let total = self
            .render_cache
            .as_ref()
            .map(|cache| cache.lines.len())
            .unwrap_or_default();
        let max_scroll = total.saturating_sub(viewport_rows);
        max_scroll.saturating_sub(self.scroll_back.min(max_scroll))
    }

    fn build_lines(
        &self,
        theme: &Theme,
        meta: ChatRenderMeta<'_>,
        width: u16,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = if self.messages.is_empty() {
            self.render_empty(theme, meta)
        } else {
            let mut out = vec![Line::from("")];
            let mut prev_role: Option<&Role> = None;
            let last = self.messages.len().saturating_sub(1);
            for (i, msg) in self.messages.iter().enumerate() {
                // Display-preference filters: thinking lines and tool-detail
                // lines are both Role::Tool, told apart by the THINKING_PREFIX.
                if msg.role == Role::Tool {
                    let is_thinking = msg.text.starts_with(THINKING_PREFIX);
                    if (is_thinking && self.hide_thinking)
                        || (!is_thinking && self.hide_tool_details)
                    {
                        continue;
                    }
                }
                if let Some(prev) = prev_role {
                    if should_insert_message_gap(prev, &msg.role) {
                        out.push(Line::from(""));
                    }
                }
                // The actively-streaming message (the assistant being filled, or
                // the growing tail during a stream) changes every delta — never
                // cache it; everything else reuses its cached lines.
                let skip_cache =
                    Some(i) == self.active_assistant_index || (self.streaming && i == last);
                out.extend(self.render_message_cached(
                    msg,
                    theme,
                    width,
                    meta.theme_name,
                    skip_cache,
                ));
                prev_role = Some(&msg.role);
            }
            out
        };

        if lines.is_empty() {
            lines.push(Line::from(""));
        }

        // If the conversation is non-empty but the display filters
        // (`/thinking`, `/tool-details`) hid every visible message, the screen
        // would otherwise go completely blank — show why instead.
        if !self.messages.is_empty()
            && !lines
                .iter()
                .any(|l| l.spans.iter().any(|s| !s.content.trim().is_empty()))
        {
            lines.push(Line::styled(
                "  (thinking & tool details are hidden — /thinking or /tool-details to show them)",
                Style::default().fg(theme.fg_subtle),
            ));
        }
        if !self.messages.is_empty() {
            lines.push(Line::from(""));
        }
        lines
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
                Span::styled("   ▗▄▄▄▄▖   ", rail),
                Span::styled("zode", title),
                Span::styled(" workbench ", muted),
                Span::styled(":: terminal coding agent", accent2),
            ]),
            Line::from(vec![
                Span::styled("  ▐  ◕ ◕ ▌  ", rail),
                Span::styled("ready for code, shells, and agents", muted),
            ]),
            Line::from(vec![
                Span::styled("  ▐   ▿  ▌  ", rail),
                Span::styled("small mascot, serious tools", accent2),
            ]),
            Line::from(vec![
                Span::styled("   ▝▜██▛▘   ", rail),
                Span::styled("command deck online", muted),
            ]),
            Line::from(vec![Span::styled("    ▝▘▝▘    ", rail)]),
            Line::from(vec![Span::styled("          ", panel)]),
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

    /// `render_message` with a content-keyed cache. The key (role, text, width,
    /// theme) invalidates automatically on any of those changing, so a rebuild
    /// reuses unchanged messages instead of re-parsing the whole transcript.
    /// The actively-streaming message and image messages are never cached (the
    /// former grows each delta; the latter's preview isn't keyed by content).
    fn render_message_cached(
        &self,
        msg: &ChatMessage,
        theme: &Theme,
        width: u16,
        theme_name: &str,
        skip_cache: bool,
    ) -> Vec<Line<'static>> {
        if skip_cache || !msg.images.is_empty() {
            return self.render_message(msg, theme, width);
        }
        let mut h = DefaultHasher::new();
        msg.role.hash(&mut h);
        msg.text.hash(&mut h);
        width.hash(&mut h);
        theme_name.hash(&mut h);
        let key = h.finish();
        if let Some(lines) = self.line_cache.borrow().get(&key) {
            return lines.clone();
        }
        #[cfg(test)]
        self.render_misses.set(self.render_misses.get() + 1);
        let lines = self.render_message(msg, theme, width);
        let mut cache = self.line_cache.borrow_mut();
        if cache.len() > 4096 {
            cache.clear(); // backstop against unbounded growth
        }
        cache.insert(key, lines.clone());
        lines
    }

    fn render_message(&self, msg: &ChatMessage, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        match msg.role {
            Role::User => render_user_bar(&msg.text, &msg.images, theme, width),
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

fn should_insert_message_gap(_prev: &Role, _next: &Role) -> bool {
    true
}

fn visible_line_window(total_rows: usize, viewport_rows: usize, offset: usize) -> Range<usize> {
    if total_rows == 0 || viewport_rows == 0 {
        return 0..0;
    }
    let max_start = total_rows.saturating_sub(viewport_rows);
    let start = offset.min(max_start);
    let end = total_rows.min(start.saturating_add(viewport_rows));
    start..end
}

fn apply_selection_to_line(
    line: Line<'static>,
    line_idx: usize,
    selection: ChatSelection,
    theme: &Theme,
) -> Line<'static> {
    let Some((start_col, end_col)) = selection_cols_for_line(line_idx, selection) else {
        return line;
    };
    if start_col == end_col {
        return line;
    }
    let selected_style = Style::default()
        .bg(theme.accent)
        .fg(theme.bg_primary)
        .add_modifier(Modifier::BOLD);
    let mut out = Vec::new();
    let mut display_col = 0usize;
    for span in line.spans {
        for ch in span.content.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
            let next_col = display_col.saturating_add(width);
            let selected = display_col < end_col && next_col > start_col;
            out.push(Span::styled(
                ch.to_string(),
                if selected { selected_style } else { span.style },
            ));
            display_col = next_col;
        }
    }
    Line::from(out)
}

fn selection_cols_for_line(line_idx: usize, selection: ChatSelection) -> Option<(usize, usize)> {
    let (start, end) = selection.normalized();
    if start == end || line_idx < start.line || line_idx > end.line {
        return None;
    }
    if start.line == end.line {
        return Some((start.column, end.column));
    }
    if line_idx == start.line {
        Some((start.column, usize::MAX))
    } else if line_idx == end.line {
        Some((0, end.column))
    } else {
        Some((0, usize::MAX))
    }
}

fn plain_line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn slice_display_cols(text: &str, start_col: usize, end_col: usize) -> String {
    let mut out = String::new();
    let mut display_col = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        let next_col = display_col.saturating_add(width);
        if display_col >= end_col {
            break;
        }
        if next_col > start_col && display_col < end_col {
            out.push(ch);
        }
        display_col = next_col;
    }
    out
}

fn render_user_bar(
    text: &str,
    images: &[ImagePreview],
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
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
    if !images.is_empty() {
        out.push(blank_user_bar_line(theme, width));
        for image in images {
            out.extend(render_image_preview(image, theme, width));
        }
    }
    out.push(blank_user_bar_line(theme, width));
    out
}

fn render_image_preview(image: &ImagePreview, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let style = Style::default().bg(theme.bg_secondary).fg(theme.fg_subtle);
    let name = Style::default()
        .bg(theme.bg_secondary)
        .fg(theme.fg_white)
        .add_modifier(Modifier::BOLD);
    let accent = Style::default().bg(theme.bg_secondary).fg(theme.accent);
    let rail = Span::styled("▌ ", Style::default().bg(theme.bg_secondary).fg(theme.user));
    let spans = vec![
        Span::styled("▣ ", accent),
        Span::styled(image.display_name.clone(), name),
        Span::styled(
            format!(
                "  {}  {}",
                image.media_type,
                format_size_bytes(image.size_bytes)
            ),
            style,
        ),
    ];
    wrap_spans_with_prefix(spans, rail, Span::styled("  ", style), width)
        .into_iter()
        .map(|line| pad_line_to_width(line, width, style))
        .collect()
}

fn format_size_bytes(size: u64) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
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
            // A literal newline is a HARD break: finalize this row and start a
            // fresh one. Never embed `\n` in a Line's span — ratatui renders a
            // `Line` as a single row and mis-measures embedded newlines, which
            // desyncs our scroll-row count from ratatui's and scrolls the view
            // past the content (the "long session renders blank" bug). Callers
            // like tool/thinking/process lines pass raw multi-line text here.
            if ch == '\n' {
                out.push(Line::from(std::mem::take(&mut current)));
                prefix = continuation_prefix.clone();
                current = vec![prefix.clone()];
                current_width = continuation_width;
                saw_content = true;
                continue;
            }
            if ch == '\r' {
                continue; // drop CR from CRLF; the \n above did the break
            }
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            let has_body = current.len() > 1;
            if has_body && current_width + ch_width > max_width {
                out.push(Line::from(std::mem::take(&mut current)));
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
    fn per_message_cache_skips_re_render_of_unchanged_messages() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let meta = ChatRenderMeta {
            theme_name: "minimal",
            model: "m",
            cwd: std::path::Path::new("."),
        };
        let mut chat = ChatView::new();
        chat.push_user("hello");
        chat.push_system("world");
        chat.push_tool("a tool line");
        let width = 80u16;

        // First build renders every message.
        chat.render_misses.set(0);
        let _ = chat.build_lines(&theme, meta, width);
        assert!(chat.render_misses.get() >= 3, "first build renders all");

        // Rebuilding with no change serves everything from the cache.
        chat.render_misses.set(0);
        let _ = chat.build_lines(&theme, meta, width);
        assert_eq!(chat.render_misses.get(), 0, "unchanged rebuild re-rendered");

        // Adding one message re-renders only that message.
        chat.push_system("again");
        chat.render_misses.set(0);
        let _ = chat.build_lines(&theme, meta, width);
        assert_eq!(chat.render_misses.get(), 1, "only the new message renders");

        // A width change re-renders (key includes width).
        chat.render_misses.set(0);
        let _ = chat.build_lines(&theme, meta, 60);
        assert!(
            chat.render_misses.get() >= 4,
            "width change invalidates cache"
        );
    }

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
    fn push_user_with_images_keeps_previews() {
        let mut view = ChatView::new();
        view.push_user_with_images(
            "describe this",
            vec![ImagePreview {
                display_name: "screen.png".into(),
                media_type: "image/png".into(),
                size_bytes: 2048,
            }],
        );

        assert_eq!(view.messages().len(), 1);
        assert_eq!(view.messages()[0].images[0].display_name, "screen.png");
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
    fn streaming_keeps_the_newest_line_at_the_bottom() {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user("design something");
        view.begin_assistant();
        // Stream well past the viewport, rendering each step — the freshest
        // line must always be visible (no manual scroll → follow the tail).
        for i in 0..200 {
            view.push_delta(&format!("streamed reasoning line number {i}\n"));
            let backend = TestBackend::new(60, 12);
            let mut term = Terminal::new(backend).unwrap();
            let meta = ChatRenderMeta {
                theme_name: &theme.name,
                model: "m",
                cwd: std::path::Path::new("/tmp"),
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
            assert!(
                content.contains(&format!("number {i}")),
                "newest streamed line {i} must stay visible at the bottom"
            );
        }
    }

    #[test]
    fn wrap_breaks_on_embedded_newlines() {
        // Multi-line text (e.g. a thinking/tool message) must become several
        // Lines, NONE embedding a literal '\n' and each within the width — so
        // our scroll-row count matches ratatui's and the view can't go blank.
        let spans = vec![Span::raw(
            "line one\nline two is a bit longer\nthree".to_string(),
        )];
        let lines = wrap_spans_with_prefix(spans, Span::raw(""), Span::raw(""), 40);
        assert!(
            lines.len() >= 3,
            "three source lines → ≥3 rows: {}",
            lines.len()
        );
        for l in &lines {
            for s in &l.spans {
                assert!(!s.content.contains('\n'), "a Line must not embed a newline");
            }
            assert!(line_disp_width(l) <= 40, "each row must fit the width");
        }
    }

    #[test]
    fn multiline_tool_message_total_rows_match_line_count() {
        // The exact invariant that broke "long session renders blank": after
        // building, every Line is one visual row, so summed wrapped_rows equals
        // the line count (no overcount → scroll offset can't overshoot).
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user("hi");
        view.push_thinking_delta("first paragraph of reasoning\nsecond line\nthird line here");
        view.push_tool("Tool Bash\nmulti-line\ntool output");
        let mut lines = 0usize;
        let mut rows = 0usize;
        for msg in view.messages() {
            for line in view.render_message(msg, &theme, 60) {
                assert!(line_disp_width(&line) <= 60, "every line fits the width");
                assert!(
                    !line.spans.iter().any(|s| s.content.contains('\n')),
                    "no rendered line embeds a newline"
                );
                lines += 1;
                rows += wrapped_rows(&line, 60);
            }
        }
        assert!(lines > 0);
        assert_eq!(rows, lines, "each line must be exactly one visual row");
    }

    #[test]
    fn long_conversation_shows_the_tail_not_blank() {
        // Reproduces the "chat goes blank after a while" report: with far more
        // content than the viewport and following the tail (scroll_back == 0),
        // the LAST message must be on screen — never scrolled past into blank.
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        for i in 0..60 {
            view.push_user(&format!("question number {i} about the codebase"));
            view.begin_assistant();
            view.push_delta(&format!(
                "answer {i}: here is a fairly long reply that will wrap across \
                 multiple terminal rows so the wrapped-row math is exercised, \
                 including some 中文字符 for width handling"
            ));
            view.end_turn();
        }
        view.push_user("FINAL_TAIL_MARKER question");
        let backend = TestBackend::new(40, 12);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "m",
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
        let non_space = content.chars().filter(|c| !c.is_whitespace()).count();
        assert!(non_space > 0, "chat rendered completely blank");
        assert!(
            content.contains("FINAL_TAIL_MARKER"),
            "tail message must be visible when following the bottom; got:\n{content}"
        );
    }

    #[test]
    fn renders_user_image_preview_without_panic() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut view = ChatView::new();
        view.push_user_with_images(
            "see attached",
            vec![ImagePreview {
                display_name: "diagram.webp".into(),
                media_type: "image/webp".into(),
                size_bytes: 1_500_000,
            }],
        );
        let backend = TestBackend::new(80, 12);
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
        assert!(content.contains("diagram.webp"));
        assert!(content.contains("image/webp"));
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
        assert!(content.contains("◕ ◕"));
        assert!(content.contains("▿"));
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
    fn rendered_chat_has_bottom_padding_after_last_message() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        view.push_delta("answer");
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        let lines = view.build_lines(&theme, meta, 40);
        let last = lines.last().expect("chat should render lines");

        assert!(
            last.spans.iter().all(|span| span.content.trim().is_empty()),
            "last chat line should be internal bottom padding: {last:?}"
        );
        assert!(
            lines.iter().rev().skip(1).any(|line| line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("answer")),
            "assistant answer should render above bottom padding"
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
    fn assistant_answer_has_breathing_room_after_process_tail() {
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
                .is_some_and(|row| row.trim().is_empty()),
            "assistant answer should leave one blank row after process tail: {rows:?}"
        );
        assert!(
            rows.get(usage_row + 2)
                .is_some_and(|row| row.contains("DeepSeek")),
            "assistant row should follow the process gap: {rows:?}"
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
    fn visible_line_window_slices_to_the_viewport() {
        assert_eq!(visible_line_window(100, 10, 0), 0..10);
        assert_eq!(visible_line_window(100, 10, 90), 90..100);
        assert_eq!(visible_line_window(5, 10, 0), 0..5);
        assert_eq!(visible_line_window(100, 10, 250), 90..100);
    }

    #[test]
    fn render_cache_reuses_built_lines_when_only_scroll_changes() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut view = ChatView::new();
        for i in 0..80 {
            view.push_user(&format!("question {i}"));
            view.push_delta(&format!(
                "answer {i} with enough content to wrap over several rows and make rendering non-trivial"
            ));
            view.end_turn();
        }
        let backend = TestBackend::new(60, 12);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "deepseek-v4-pro",
            cwd: std::path::Path::new("/tmp/zode"),
        };

        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        assert_eq!(view.cache_builds, 1);

        view.scroll_up(1);
        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        assert_eq!(
            view.cache_builds, 1,
            "scrolling should only slice cached lines, not rebuild full history"
        );

        view.push_delta("new text invalidates the cache");
        term.draw(|f| view.render(f, f.area(), &theme, meta))
            .unwrap();
        assert_eq!(view.cache_builds, 2);
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
                images: Vec::new(),
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
                images: Vec::new(),
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
                images: Vec::new(),
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
    fn assistant_markdown_table_renders_as_terminal_table() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Assistant,
                text: "| Type | 示例 |\n|---|---|\n| Fact | API 地址 |\n".into(),
                images: Vec::new(),
            },
            &theme,
            80,
        );

        let rows: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(rows.iter().any(|row| row.contains("│ Type")));
        assert!(rows
            .iter()
            .any(|row| row.contains("├") && row.contains("─")));
        assert!(rows
            .iter()
            .any(|row| row.contains("Fact") && row.contains("API 地址")));
    }

    #[test]
    fn selected_text_extracts_visible_chat_range() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let cwd = std::path::Path::new("/tmp");
        let mut chat = ChatView::new();
        chat.push_delta("alpha\nbeta\ngamma");
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "test-model",
            cwd,
        };
        let area = Rect::new(0, 0, 40, 6);
        let selection = ChatSelection::new(
            ChatSelectionPoint { line: 1, column: 2 },
            ChatSelectionPoint { line: 2, column: 6 },
        );

        let text = chat.selected_text(selection, &theme, meta, area);

        assert_eq!(text, "alpha\n  beta");
    }

    #[test]
    fn selection_highlights_visible_chat_cells() {
        let theme = ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let cwd = std::path::Path::new("/tmp");
        let mut chat = ChatView::new();
        chat.push_delta("alpha");
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "test-model",
            cwd,
        };
        let selection = ChatSelection::new(
            ChatSelectionPoint { line: 1, column: 2 },
            ChatSelectionPoint { line: 1, column: 7 },
        );
        let backend = TestBackend::new(40, 6);
        let mut term = Terminal::new(backend).unwrap();

        term.draw(|f| chat.render_with_selection(f, f.area(), &theme, meta, Some(selection)))
            .unwrap();

        let buf = term.backend().buffer();
        assert_eq!(buf[(2, 1)].bg, theme.accent);
        assert_eq!(buf[(6, 1)].bg, theme.accent);
    }

    #[test]
    fn process_lines_are_muted_not_role_blocks() {
        let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
        let view = ChatView::new();
        let lines = view.render_message(
            &ChatMessage {
                role: Role::Tool,
                text: "Bash cargo build".into(),
                images: Vec::new(),
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
                images: Vec::new(),
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
