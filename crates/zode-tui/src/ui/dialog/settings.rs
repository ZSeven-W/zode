//! Two-level settings dialog (Ctrl+,): Top → {Theme, Provider, Mode}.
//! Confirming a leaf returns a SettingsAction the app applies.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsLevel {
    Top,
    Theme,
    Model,
    Provider,
    Mode,
    Effort,
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SetTheme(String),
    SetModel(String),
    SetProvider(String),
    SetMode(String),
    SetEffort(String),
    SetSidebar(String),
}

pub struct SettingsDialog {
    level: SettingsLevel,
    root_level: SettingsLevel,
    state: ListState,
    theme_ids: Vec<String>,
    model_ids: Vec<String>,
    provider_names: Vec<String>,
    modes: Vec<String>,
    efforts: Vec<String>,
    sidebars: Vec<String>,
}

const TOP_ITEMS: &[&str] = &[
    "Theme",
    "Provider",
    "Permission mode",
    "Effort",
    "Sidebar",
];

fn default_efforts() -> Vec<String> {
    vec!["low".into(), "medium".into(), "high".into()]
}

fn default_sidebars() -> Vec<String> {
    vec!["auto".into(), "visible".into(), "hidden".into()]
}

impl SettingsDialog {
    pub fn new(theme_ids: Vec<String>, provider_names: Vec<String>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Top,
            root_level: SettingsLevel::Top,
            state,
            theme_ids,
            model_ids: Vec::new(),
            provider_names,
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
            efforts: default_efforts(),
            sidebars: default_sidebars(),
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
            model_ids: Vec::new(),
            provider_names: Vec::new(),
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
            efforts: default_efforts(),
            sidebars: default_sidebars(),
        }
    }

    pub fn model_picker(model_ids: Vec<String>) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Model,
            root_level: SettingsLevel::Model,
            state,
            theme_ids: Vec::new(),
            model_ids,
            provider_names: Vec::new(),
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
            efforts: default_efforts(),
            sidebars: default_sidebars(),
        }
    }

    pub fn effort_picker() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Effort,
            root_level: SettingsLevel::Effort,
            state,
            theme_ids: Vec::new(),
            model_ids: Vec::new(),
            provider_names: Vec::new(),
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
            efforts: default_efforts(),
            sidebars: default_sidebars(),
        }
    }

    pub fn sidebar_picker() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            level: SettingsLevel::Sidebar,
            root_level: SettingsLevel::Sidebar,
            state,
            theme_ids: Vec::new(),
            model_ids: Vec::new(),
            provider_names: Vec::new(),
            modes: vec!["default".into(), "acceptEdits".into(), "dontAsk".into()],
            efforts: default_efforts(),
            sidebars: default_sidebars(),
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
            SettingsLevel::Model => self.model_ids.clone(),
            SettingsLevel::Provider => self.provider_names.clone(),
            SettingsLevel::Mode => self.modes.clone(),
            SettingsLevel::Effort => self.efforts.clone(),
            SettingsLevel::Sidebar => self.sidebars.clone(),
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
                2 => SettingsLevel::Mode,
                3 => SettingsLevel::Effort,
                _ => SettingsLevel::Sidebar,
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
            SettingsLevel::Model => self
                .model_ids
                .get(idx)
                .map(|s| SettingsAction::SetModel(s.clone())),
            SettingsLevel::Provider => self
                .provider_names
                .get(idx)
                .map(|s| SettingsAction::SetProvider(s.clone())),
            SettingsLevel::Mode => self
                .modes
                .get(idx)
                .map(|s| SettingsAction::SetMode(s.clone())),
            SettingsLevel::Effort => self
                .efforts
                .get(idx)
                .map(|s| SettingsAction::SetEffort(s.clone())),
            SettingsLevel::Sidebar => self
                .sidebars
                .get(idx)
                .map(|s| SettingsAction::SetSidebar(s.clone())),
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
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
        let popup = modal_area(area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line(title_text(self.level, self.root_level), inner.width, theme),
            header_rect(inner),
        );
        f.render_widget(search_line(theme), search_rect(inner));
        f.render_widget(
            Paragraph::new(section_title(self.level, self.root_level)).style(
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.bg_secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            section_rect(inner),
        );

        let list = List::new(items)
            .style(Style::default().bg(theme.bg_secondary).fg(theme.fg_text))
            .highlight_style(
                Style::default()
                    .bg(theme.system)
                    .fg(theme.bg_primary)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_stateful_widget(list, list_rect(inner), &mut self.state);
        f.render_widget(footer_line(theme), footer_rect(inner));
    }
}

fn modal_area(area: Rect) -> Rect {
    let max_w = area.width.saturating_sub(4);
    let max_h = area.height.saturating_sub(4);
    let width = max_w.min(58).max(max_w.min(40));
    let height = max_h.min(22).max(max_h.min(14));
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

fn header_rect(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y, inner.width, 1)
}

fn search_rect(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1)
}

fn section_rect(inner: Rect) -> Rect {
    Rect::new(inner.x, inner.y.saturating_add(4), inner.width, 1)
}

fn list_rect(inner: Rect) -> Rect {
    Rect::new(
        inner.x,
        inner.y.saturating_add(5),
        inner.width,
        inner.height.saturating_sub(7),
    )
}

fn footer_rect(inner: Rect) -> Rect {
    Rect::new(
        inner.x,
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        inner.width,
        1,
    )
}

fn title_text(level: SettingsLevel, root: SettingsLevel) -> &'static str {
    match level {
        SettingsLevel::Top => "Settings",
        SettingsLevel::Theme if root == SettingsLevel::Theme => "Select theme",
        SettingsLevel::Model if root == SettingsLevel::Model => "Select model",
        SettingsLevel::Provider if root == SettingsLevel::Provider => "Select provider",
        SettingsLevel::Mode if root == SettingsLevel::Mode => "Permission mode",
        SettingsLevel::Theme => "Select theme",
        SettingsLevel::Model => "Select model",
        SettingsLevel::Provider => "Select provider",
        SettingsLevel::Mode => "Permission mode",
        SettingsLevel::Effort => "Effort level",
        SettingsLevel::Sidebar => "Sidebar",
    }
}

fn section_title(level: SettingsLevel, root: SettingsLevel) -> &'static str {
    match (level, root) {
        (SettingsLevel::Top, _) => "Suggested",
        (SettingsLevel::Theme, _) => "Themes",
        (SettingsLevel::Model, _) => "Models",
        (SettingsLevel::Provider, _) => "Providers",
        (SettingsLevel::Mode, _) => "Permission",
        (SettingsLevel::Effort, _) => "Effort",
        (SettingsLevel::Sidebar, _) => "Sidebar",
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

fn search_line(theme: &Theme) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            "S",
            Style::default()
                .fg(theme.accent_secondary)
                .bg(theme.bg_secondary),
        ),
        Span::styled(
            "earch",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

fn footer_line(theme: &Theme) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " select  ",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " close",
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
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
    fn model_picker_starts_at_model_root() {
        let d = SettingsDialog::model_picker(vec!["MiniMax-M1".into(), "deepseek-chat".into()]);
        assert_eq!(d.level(), SettingsLevel::Model);
        assert!(d.is_root_level());
        assert_eq!(
            d.confirm(),
            Some(SettingsAction::SetModel("MiniMax-M1".into()))
        );
    }

    #[test]
    fn render_preserves_screen_behind_dialog() {
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
        assert!(content.contains("LEAK_OUTSIDE_SETTINGS"));
        assert!(content.contains("Select theme"));
        assert!(content.contains("esc"));
        assert!(content.contains("Search"));
        assert!(content.contains("hacker"));
        assert!(!content.contains("┌"));
    }

    #[test]
    fn model_picker_uses_opencode_style_modal() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut dialog =
            SettingsDialog::model_picker(vec!["deepseek-v4-pro".into(), "MiniMax-M1".into()]);

        terminal
            .draw(|f| dialog.render(f, f.area(), &theme))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Select model"));
        assert!(content.contains("esc"));
        assert!(content.contains("Search"));
        assert!(content.contains("deepseek-v4-pro"));
        assert!(!content.contains("┌"));
        assert!(!content.contains("Settings ›"));
    }
}
