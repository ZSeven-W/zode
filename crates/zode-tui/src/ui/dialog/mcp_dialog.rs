//! MCP server manager opened by `/mcp`: lists every discovered MCP server with
//! its live connection state + tool count, and lets the user enable/disable
//! ("close") each. Toggles are staged in-memory and applied on close — the app
//! persists the disabled set (`mcp:<name>`) and reassembles, so a disabled
//! server disconnects and an enabled one (re)connects.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::plugin::{Plugin, PluginKind};

use crate::theme::Theme;

pub struct McpDialog {
    servers: Vec<Plugin>,
    original_disabled: Vec<String>,
    selected: usize,
}

impl McpDialog {
    /// Build from the engine's plugin list; keeps only the MCP entries.
    pub fn new(plugins: Vec<Plugin>) -> Self {
        let servers: Vec<Plugin> = plugins
            .into_iter()
            .filter(|p| p.kind == PluginKind::Mcp)
            .collect();
        Self {
            original_disabled: disabled_of(&servers),
            servers,
            selected: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn next(&mut self) {
        if !self.servers.is_empty() {
            self.selected = (self.selected + 1) % self.servers.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.servers.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.servers.len() - 1);
        }
    }

    /// Flip the selected server's enabled flag. Returns (name, new_state).
    pub fn toggle_selected(&mut self) -> Option<(String, bool)> {
        let p = self.servers.get_mut(self.selected)?;
        p.enabled = !p.enabled;
        Some((p.name.clone(), p.enabled))
    }

    /// All MCP plugin ids this dialog owns (so non-MCP disabled ids are kept).
    pub fn all_ids(&self) -> Vec<String> {
        self.servers.iter().map(|p| p.id.clone()).collect()
    }

    /// Sorted ids of servers currently OFF — persisted to `plugins.disabled`.
    pub fn disabled_ids(&self) -> Vec<String> {
        disabled_of(&self.servers)
    }

    pub fn is_dirty(&self) -> bool {
        self.disabled_ids() != self.original_disabled
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 70, 22);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );
        let inner = inner_area(popup);
        f.render_widget(
            header_line(crate::tr("MCP servers"), inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        let list = Rect::new(
            inner.x,
            inner.y.saturating_add(2),
            inner.width,
            inner.height.saturating_sub(4),
        );
        if self.servers.is_empty() {
            f.render_widget(
                Paragraph::new(crate::tr("no MCP servers configured").to_string())
                    .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
                list,
            );
        } else {
            let height = list.height as usize;
            let offset = self.selected.saturating_sub(height.saturating_sub(1));
            let mut y = list.y;
            for (i, p) in self.servers.iter().enumerate().skip(offset).take(height) {
                self.render_row(
                    f,
                    Rect::new(list.x, y, list.width, 1),
                    p,
                    i == self.selected,
                    theme,
                );
                y = y.saturating_add(1);
            }
        }

        let footer = format!("space {}   esc {}", crate::tr("toggle"), crate::tr("apply"));
        f.render_widget(
            Paragraph::new(footer)
                .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }

    fn render_row(&self, f: &mut Frame, area: Rect, p: &Plugin, selected: bool, theme: &Theme) {
        let mark = if selected { "> " } else { "  " };
        let check = if p.enabled { "[x] " } else { "[ ] " };
        let row = format!("{mark}{check}{:<20} {}", p.name, p.detail);
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

fn disabled_of(servers: &[Plugin]) -> Vec<String> {
    let mut v: Vec<String> = servers
        .iter()
        .filter(|p| !p.enabled)
        .map(|p| p.id.clone())
        .collect();
    v.sort();
    v
}

fn modal_area(area: Rect, target_w: u16, target_h: u16) -> Rect {
    let max_w = area.width.saturating_sub(6);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(target_w).max(max_w.min(40));
    let height = max_h.min(target_h).max(max_h.min(8));
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

fn header_line(title: &str, width: u16, theme: &Theme) -> Paragraph<'static> {
    let title_width = title.chars().count() as u16;
    let gap = width.saturating_sub(title_width.saturating_add(3)) as usize;
    Paragraph::new(Line::from(vec![
        Span::styled(
            title.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::plugin::PluginManager;

    fn sample() -> Vec<Plugin> {
        PluginManager::default().list(
            &[("deepwiki".into(), true), ("ctx7".into(), false)],
            &[("review".into(), "review".into())],
            &["rust".into()],
        )
    }

    #[test]
    fn keeps_only_mcp_and_toggles() {
        let mut d = McpDialog::new(sample());
        assert_eq!(d.servers.len(), 2); // deepwiki + ctx7, not the skill/lsp
        assert!(!d.is_dirty());
        let (name, enabled) = d.toggle_selected().expect("a server");
        assert!(!enabled);
        assert!(d.is_dirty());
        assert!(d.disabled_ids().contains(&format!("mcp:{name}")));
        d.toggle_selected();
        assert!(!d.is_dirty());
    }
}
