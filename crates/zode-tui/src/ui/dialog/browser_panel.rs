//! /browser status panel: status header + four action rows.
//! Opened by the bare `/browser` command (see `zode_core::commands::browser`).
//! Single-level list — simpler than `SettingsDialog` (no `Level` concept) —
//! following the same `ListState` + `render_stateful_widget` + `modal_area`
//! shape as `settings.rs`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::theme::Theme;

pub const PANEL_BLURB: &str =
    "Built-in browser control lets the agent navigate pages, fill forms, \
     capture screenshots, and debug with console/network logs.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPanelAction {
    SelectTarget,
    ManagePermissions,
    Reconnect,
    ToggleDefault,
}

#[derive(Debug, Clone)]
pub struct BrowserPanelStatus {
    /// Whether the `tools:browser` group is enabled for this session.
    pub group_enabled: bool,
    /// "managed" | "bridge".
    pub target: String,
    /// Whether the extension bridge currently has an authenticated connection.
    pub paired: bool,
    /// Reserved for a future "is the managed browser currently up" indicator.
    pub running: bool,
}

pub struct BrowserPanel {
    state: ListState,
    status: BrowserPanelStatus,
}

impl BrowserPanel {
    pub fn new(status: BrowserPanelStatus) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self { state, status }
    }

    /// Header lines: status/target/extension, shown above the action list.
    pub fn status_lines(&self) -> Vec<String> {
        vec![
            format!(
                "Status:    {}",
                if self.status.group_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                }
            ),
            format!("Target:    {}", self.status.target),
            format!(
                "Extension: {}",
                if self.status.paired {
                    "Paired"
                } else {
                    "Not paired"
                }
            ),
        ]
    }

    pub fn items(&self) -> Vec<String> {
        vec![
            format!("Select target… (current: {})", self.status.target),
            "Manage permissions (reset always-allow)".to_string(),
            "Reconnect extension".to_string(),
            format!(
                "Enabled by default: {}",
                if self.status.group_enabled {
                    "Yes"
                } else {
                    "No"
                }
            ),
        ]
    }

    pub fn next(&mut self) {
        let len = self.items().len().max(1);
        let i = (self.state.selected().unwrap_or(0) + 1) % len;
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        let len = self.items().len().max(1);
        let i = self
            .state
            .selected()
            .unwrap_or(0)
            .checked_sub(1)
            .unwrap_or(len - 1);
        self.state.select(Some(i));
    }

    pub fn confirm(&self) -> Option<BrowserPanelAction> {
        Some(match self.state.selected()? {
            0 => BrowserPanelAction::SelectTarget,
            1 => BrowserPanelAction::ManagePermissions,
            2 => BrowserPanelAction::Reconnect,
            _ => BrowserPanelAction::ToggleDefault,
        })
    }

    pub fn set_status(&mut self, s: BrowserPanelStatus) {
        self.status = s;
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line(crate::tr("Browser"), inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        f.render_widget(
            Paragraph::new(PANEL_BLURB)
                .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary))
                .wrap(Wrap { trim: true }),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 3),
        );

        let status_y = inner.y.saturating_add(6);
        for (i, line) in self.status_lines().into_iter().enumerate() {
            f.render_widget(
                Paragraph::new(line)
                    .style(Style::default().fg(theme.fg_text).bg(theme.bg_secondary)),
                Rect::new(inner.x, status_y.saturating_add(i as u16), inner.width, 1),
            );
        }

        let list_y = status_y.saturating_add(4);
        let items: Vec<ListItem> = self
            .items()
            .into_iter()
            .map(|s| {
                ListItem::new(Line::styled(
                    format!("  {s}"),
                    Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
                ))
            })
            .collect();
        let list = List::new(items)
            .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text))
            .highlight_style(
                Style::default()
                    .bg(theme.system)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        let list_height = inner
            .height
            .saturating_sub(list_y.saturating_sub(inner.y))
            .saturating_sub(1);
        f.render_stateful_widget(
            list,
            Rect::new(inner.x, list_y, inner.width, list_height),
            &mut self.state,
        );

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
}

fn modal_area(area: Rect) -> Rect {
    let max_w = area.width.saturating_sub(4);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(64).max(max_w.min(40));
    let height = max_h.min(18).max(max_h.min(14));
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

fn footer_line(theme: &Theme) -> Paragraph<'static> {
    let key = Style::default()
        .fg(theme.fg_white)
        .bg(theme.bg_secondary)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary);
    Paragraph::new(Line::from(vec![
        Span::styled("Enter", key),
        Span::styled(format!(" {}  ", crate::tr("select")), lbl),
        Span::styled("Esc", key),
        Span::styled(format!(" {}  ", crate::tr("close")), lbl),
        Span::styled(
            "Usage: zode --browser | --no-browser",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> BrowserPanelStatus {
        BrowserPanelStatus {
            group_enabled: true,
            target: "managed".into(),
            paired: false,
            running: false,
        }
    }

    #[test]
    fn four_rows_and_wrapping_navigation() {
        let mut p = BrowserPanel::new(status());
        assert_eq!(p.items().len(), 4);
        assert_eq!(p.confirm(), Some(BrowserPanelAction::SelectTarget));
        p.prev(); // wraps to last row
        assert_eq!(p.confirm(), Some(BrowserPanelAction::ToggleDefault));
        p.next();
        assert_eq!(p.confirm(), Some(BrowserPanelAction::SelectTarget));
    }

    #[test]
    fn rows_reflect_status() {
        let mut s = status();
        s.paired = false;
        let p = BrowserPanel::new(s);
        let items = p.items();
        assert!(items[2].contains("Reconnect"), "{items:?}");
        // header text is rendered separately; status strings exposed for it:
        assert_eq!(p.status_lines()[0], "Status:    Enabled");
        assert_eq!(p.status_lines()[1], "Target:    managed");
        assert_eq!(p.status_lines()[2], "Extension: Not paired");
    }

    #[test]
    fn set_status_updates_rows() {
        let mut p = BrowserPanel::new(status());
        p.set_status(BrowserPanelStatus {
            group_enabled: false,
            target: "bridge".into(),
            paired: true,
            running: false,
        });
        assert_eq!(p.status_lines()[0], "Status:    Disabled");
        assert_eq!(p.status_lines()[1], "Target:    bridge");
        assert_eq!(p.status_lines()[2], "Extension: Paired");
        assert!(p.items()[3].contains("No"));
    }

    #[test]
    fn renders_without_panicking() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut panel = BrowserPanel::new(status());
        terminal
            .draw(|f| panel.render(f, f.area(), &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Browser"));
        assert!(content.contains("Status:"));
        assert!(content.contains("Reconnect"));
    }
}
