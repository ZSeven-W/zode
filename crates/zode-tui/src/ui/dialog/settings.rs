//! Two-level settings dialog (Ctrl+,): Top → {Theme, Provider, Mode}.
//! Confirming a leaf returns a SettingsAction the app applies.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;
use crate::ui::centered;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsLevel {
    Top,
    Theme,
    Provider,
    Mode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SetTheme(String),
    SetProvider(String),
    SetMode(String),
}

pub struct SettingsDialog {
    level: SettingsLevel,
    root_level: SettingsLevel,
    state: ListState,
    theme_ids: Vec<String>,
    provider_names: Vec<String>,
    modes: Vec<String>,
}

const TOP_ITEMS: &[&str] = &["Theme", "Provider", "Permission mode"];

impl SettingsDialog {
    pub fn new(theme_ids: Vec<String>, provider_names: Vec<String>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Top,
            root_level: SettingsLevel::Top,
            state,
            theme_ids,
            provider_names,
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
        }
    }

    pub fn theme_picker(theme_ids: Vec<String>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Theme,
            root_level: SettingsLevel::Theme,
            state,
            theme_ids,
            provider_names: Vec::new(),
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
        }
    }

    pub fn level(&self) -> SettingsLevel {
        self.level
    }

    pub fn is_root_level(&self) -> bool {
        self.level == self.root_level
    }

    fn items(&self) -> Vec<String> {
        match self.level {
            SettingsLevel::Top => TOP_ITEMS.iter().map(|s| s.to_string()).collect(),
            SettingsLevel::Theme => self.theme_ids.clone(),
            SettingsLevel::Provider => self.provider_names.clone(),
            SettingsLevel::Mode => self.modes.clone(),
        }
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

    /// Enter the submenu (from Top). No-op on leaf levels.
    pub fn enter(&mut self) {
        if self.level == SettingsLevel::Top {
            self.level = match self.state.selected().unwrap_or(0) {
                0 => SettingsLevel::Theme,
                1 => SettingsLevel::Provider,
                _ => SettingsLevel::Mode,
            };
            self.state.select(Some(0));
        }
    }

    pub fn back(&mut self) {
        self.level = self.root_level;
        self.state.select(Some(0));
    }

    /// Confirm a leaf selection → SettingsAction. None on the Top level.
    pub fn confirm(&self) -> Option<SettingsAction> {
        let idx = self.state.selected().unwrap_or(0);
        match self.level {
            SettingsLevel::Top => None,
            SettingsLevel::Theme => self
                .theme_ids
                .get(idx)
                .map(|s| SettingsAction::SetTheme(s.clone())),
            SettingsLevel::Provider => self
                .provider_names
                .get(idx)
                .map(|s| SettingsAction::SetProvider(s.clone())),
            SettingsLevel::Mode => self
                .modes
                .get(idx)
                .map(|s| SettingsAction::SetMode(s.clone())),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_primary)),
            area,
        );

        let items: Vec<ListItem> = self
            .items()
            .into_iter()
            .map(|s| ListItem::new(Line::from(s)))
            .collect();
        let popup = centered(area, 50, 50);
        f.render_widget(Clear, popup);
        let title = match self.level {
            SettingsLevel::Top => " Settings ",
            SettingsLevel::Theme if self.root_level == SettingsLevel::Theme => " Theme ",
            SettingsLevel::Theme => " Settings › Theme ",
            SettingsLevel::Provider => " Settings › Provider ",
            SettingsLevel::Mode => " Settings › Permission mode ",
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text)),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.accent)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, popup, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_menu_enters_submenu_and_back() {
        let mut d = SettingsDialog::new(vec!["catppuccin-mocha".into(), "hacker".into()], vec![]);
        assert_eq!(d.level(), SettingsLevel::Top);
        d.enter(); // index 0 = Theme
        assert_eq!(d.level(), SettingsLevel::Theme);
        d.back();
        assert_eq!(d.level(), SettingsLevel::Top);
    }

    #[test]
    fn theme_submenu_confirm_yields_choice() {
        let mut d = SettingsDialog::new(vec!["catppuccin-mocha".into(), "hacker".into()], vec![]);
        d.enter(); // into Theme
        d.next(); // select "hacker"
        assert_eq!(d.confirm(), Some(SettingsAction::SetTheme("hacker".into())));
    }

    #[test]
    fn top_level_confirm_is_none() {
        let d = SettingsDialog::new(vec![], vec![]);
        assert_eq!(d.confirm(), None);
    }

    #[test]
    fn theme_picker_starts_at_theme_root() {
        let d = SettingsDialog::theme_picker(vec!["catppuccin-mocha".into(), "hacker".into()]);
        assert_eq!(d.level(), SettingsLevel::Theme);
        assert!(d.is_root_level());
        assert_eq!(
            d.confirm(),
            Some(SettingsAction::SetTheme("catppuccin-mocha".into()))
        );
    }

    #[test]
    fn render_clears_screen_behind_dialog() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
        let backend = ratatui::backend::TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut dialog =
            SettingsDialog::theme_picker(vec!["catppuccin-mocha".into(), "hacker".into()]);

        terminal
            .draw(|f| {
                f.render_widget(
                    ratatui::widgets::Paragraph::new("LEAK_OUTSIDE_SETTINGS"),
                    f.area(),
                );
                dialog.render(f, f.area(), &theme);
            })
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!content.contains("LEAK_OUTSIDE_SETTINGS"));
        assert!(content.contains("Theme"));
        assert!(content.contains("hacker"));
    }
}
