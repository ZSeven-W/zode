//! Plugin management picker opened by `/plugin`. Lists every toggleable
//! capability — built-in tool groups, MCP servers, skills, and LSP servers —
//! grouped by kind, and lets the user flip each on/off with a checkbox.
//!
//! Toggles are staged in-memory (the checkbox flips immediately) and applied
//! once when the picker closes, so flipping several plugins reassembles the
//! engine — and reconnects MCP servers — exactly once instead of per keystroke.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::plugin::{Plugin, PluginKind};

use crate::theme::Theme;

/// Kinds in the order they're sectioned in the list.
const KIND_ORDER: [PluginKind; 4] = [
    PluginKind::Tools,
    PluginKind::Mcp,
    PluginKind::Skill,
    PluginKind::Lsp,
];

/// One rendered line: a section heading or a plugin row (index into `plugins`).
enum Row {
    Header(&'static str),
    Item(usize),
}

pub struct PluginPicker {
    plugins: Vec<Plugin>,
    /// Disabled-id set captured when the picker opened, so the app can tell
    /// whether the user actually changed anything before reassembling.
    original_disabled: Vec<String>,
    /// Index into `visible_indices()` (navigable items only, no headers).
    selected: usize,
    filter: String,
}

impl PluginPicker {
    pub fn new(plugins: Vec<Plugin>) -> Self {
        Self {
            original_disabled: disabled_of(&plugins),
            plugins,
            selected: 0,
            filter: String::new(),
        }
    }

    /// Every plugin id the picker presents — the set it can authoritatively set
    /// on/off. Ids in config but outside this set (e.g. a project-scoped MCP
    /// server, or an `lsp:*` for a server not available on this machine) must be
    /// preserved, not clobbered, on save.
    pub fn all_ids(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.id.clone()).collect()
    }

    pub fn next(&mut self) {
        let len = self.visible_indices().len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub fn prev(&mut self) {
        let len = self.visible_indices().len();
        if len > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(len - 1);
        }
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }

    /// Flip the selected plugin's enabled flag in place. Returns its name and
    /// the new state for a toast; `None` when the filter hides every row.
    pub fn toggle_selected(&mut self) -> Option<(String, bool)> {
        let idx = self.visible_indices().get(self.selected).copied()?;
        let p = &mut self.plugins[idx];
        p.enabled = !p.enabled;
        Some((p.name.clone(), p.enabled))
    }

    /// Sorted ids of plugins currently OFF — persisted to `plugins.disabled`.
    pub fn disabled_ids(&self) -> Vec<String> {
        disabled_of(&self.plugins)
    }

    /// Whether the user changed any enable/disable state since opening.
    pub fn is_dirty(&self) -> bool {
        self.disabled_ids() != self.original_disabled
    }

    /// Indices of plugins matching the current filter, in list order.
    fn visible_indices(&self) -> Vec<usize> {
        let q = self.filter.trim().to_ascii_lowercase();
        self.plugins
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.description.to_ascii_lowercase().contains(&q)
                    || p.id.to_ascii_lowercase().contains(&q)
                {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Flatten the visible plugins into section-headed rows in `KIND_ORDER`.
    fn rows(&self, visible: &[usize]) -> Vec<Row> {
        let mut out = Vec::new();
        for kind in KIND_ORDER {
            let mut any = false;
            for &idx in visible {
                if self.plugins[idx].kind == kind {
                    if !any {
                        out.push(Row::Header(section_title(kind)));
                        any = true;
                    }
                    out.push(Row::Item(idx));
                }
            }
        }
        out
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 78, 30);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line(crate::tr("Manage plugins"), inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        f.render_widget(
            search_line(&self.filter, theme),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
        );

        let list = Rect::new(
            inner.x,
            inner.y.saturating_add(4),
            inner.width,
            inner.height.saturating_sub(6),
        );
        self.render_rows(f, list, theme);

        f.render_widget(
            footer_line(theme),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }

    fn render_rows(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            f.render_widget(
                Paragraph::new(crate::tr("No plugins match"))
                    .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
                Rect::new(area.x, area.y, area.width, 1),
            );
            return;
        }
        let rows = self.rows(&visible);
        let selected_idx = visible.get(self.selected).copied();

        // Scroll so the selected item's flat row stays on screen.
        let sel_row = rows
            .iter()
            .position(|r| matches!(r, Row::Item(i) if Some(*i) == selected_idx))
            .unwrap_or(0);
        let height = area.height as usize;
        let offset = sel_row.saturating_sub(height.saturating_sub(1));

        let mut y = area.y;
        for row in rows.iter().skip(offset).take(height) {
            match row {
                Row::Header(title) => {
                    f.render_widget(
                        Paragraph::new(*title).style(
                            Style::default()
                                .fg(theme.accent)
                                .bg(theme.bg_secondary)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Rect::new(area.x, y, area.width, 1),
                    );
                }
                Row::Item(idx) => {
                    let p = &self.plugins[*idx];
                    let selected = Some(*idx) == selected_idx;
                    self.render_item(f, Rect::new(area.x, y, area.width, 1), p, selected, theme);
                }
            }
            y = y.saturating_add(1);
        }
    }

    fn render_item(&self, f: &mut Frame, area: Rect, p: &Plugin, selected: bool, theme: &Theme) {
        let prefix = if selected { "> " } else { "  " };
        let check = if p.enabled { "[x] " } else { "[ ] " };
        let name = fixed_width(&p.name, 16);
        let detail = fixed_width(&p.detail, 14);
        let row = pad_to_width(
            format!("{prefix}{check}{name}{detail}{}", p.description),
            area.width,
        );
        let style = if selected {
            Style::default()
                .fg(theme.bg_primary)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else if p.enabled {
            Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
        } else {
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)
        };
        f.render_widget(Paragraph::new(row).style(style), area);
    }
}

fn disabled_of(plugins: &[Plugin]) -> Vec<String> {
    let mut v: Vec<String> = plugins
        .iter()
        .filter(|p| !p.enabled)
        .map(|p| p.id.clone())
        .collect();
    v.sort();
    v
}

fn section_title(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Tools => crate::tr("Tool groups"),
        PluginKind::Mcp => crate::tr("MCP servers"),
        PluginKind::Skill => crate::tr("Skills"),
        PluginKind::Lsp => crate::tr("LSP servers"),
    }
}

fn modal_area(area: Rect, target_width: u16, target_height: u16) -> Rect {
    let max_w = area.width.saturating_sub(6);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(target_width).max(max_w.min(44));
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

fn header_line(title: &'static str, width: u16, theme: &Theme) -> Paragraph<'static> {
    let title_width = title.chars().count() as u16;
    let gap = width.saturating_sub(title_width.saturating_add(3)) as usize;
    Paragraph::new(Line::from(vec![
        Span::styled(
            title,
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

fn search_line(filter: &str, theme: &Theme) -> Paragraph<'static> {
    // Localized placeholder; accent the first character (works for multi-byte
    // scripts like 搜索 / 検索, not just ASCII "Search").
    let label = zode_core::i18n::t("Search");
    let mut chars = label.chars();
    let first: String = chars.next().map(|c| c.to_string()).unwrap_or_default();
    let rest: String = chars.collect();
    let rest = if filter.is_empty() {
        rest
    } else {
        format!("{rest} {filter}")
    };
    Paragraph::new(Line::from(vec![
        Span::styled(
            first,
            Style::default()
                .fg(theme.accent_secondary)
                .bg(theme.bg_secondary),
        ),
        Span::styled(
            rest,
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

fn footer_line(theme: &Theme) -> Paragraph<'static> {
    let key = Style::default()
        .fg(theme.fg_white)
        .bg(theme.bg_secondary)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
    Paragraph::new(Line::from(vec![
        Span::styled("space", key),
        Span::styled(format!(" {}   ", crate::tr("toggle")), lbl),
        Span::styled("esc", key),
        Span::styled(format!(" {}", crate::tr("apply")), lbl),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

fn fixed_width(value: &str, width: usize) -> String {
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    let len = out.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
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
    use zode_core::plugin::PluginManager;

    fn sample() -> Vec<Plugin> {
        PluginManager::default().list(
            &[("deepwiki".into(), true)],
            &[("review".into(), "review code".into())],
            &["rust".into()],
        )
    }

    #[test]
    fn toggle_flips_and_tracks_dirty() {
        let mut p = PluginPicker::new(sample());
        assert!(!p.is_dirty());
        // Select the first item (a tool group) and toggle it off.
        let (_name, enabled) = p.toggle_selected().expect("has a selectable row");
        assert!(!enabled);
        assert!(p.is_dirty());
        assert!(p.disabled_ids().iter().any(|id| id.starts_with("tools:")));
        // Toggling back clears the dirty flag.
        p.toggle_selected();
        assert!(!p.is_dirty());
    }

    #[test]
    fn filter_narrows_to_mcp() {
        let mut p = PluginPicker::new(sample());
        for c in "deepwiki".chars() {
            p.push_filter_char(c);
        }
        let visible = p.visible_indices();
        assert_eq!(visible.len(), 1);
        let (name, _) = p.toggle_selected().expect("filtered row is selectable");
        assert_eq!(name, "deepwiki");
        assert_eq!(p.disabled_ids(), vec!["mcp:deepwiki".to_string()]);
    }

    #[test]
    fn lists_lsp_rows_alongside_other_kinds() {
        let p = PluginPicker::new(sample());
        // LSP servers are real plugins now: they appear and are owned (so a
        // toggle persists), next to tool groups, MCP, and skills.
        assert!(p.all_ids().iter().any(|id| id == "lsp:rust"));
        assert!(p.all_ids().iter().any(|id| id == "mcp:deepwiki"));
        assert!(p.all_ids().iter().any(|id| id.starts_with("tools:")));
    }

    #[test]
    fn renders_sections_and_checkboxes() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let picker = PluginPicker::new(sample());
        terminal
            .draw(|f| picker.render(f, f.area(), &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Manage plugins"));
        assert!(content.contains("Tool groups"));
        assert!(content.contains("MCP servers"));
        assert!(content.contains("Skills"));
        assert!(content.contains("[x]"));
        assert!(content.contains("space toggle"));
    }
}
