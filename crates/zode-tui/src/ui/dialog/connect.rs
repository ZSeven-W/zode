//! Provider connection dialog opened by `/connect`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::config::{ProviderConfig, ProviderKind};

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectStage {
    Provider,
    ApiKey,
}

#[derive(Debug, Clone)]
pub struct ConnectAction {
    pub name: String,
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderSection {
    Popular,
    Providers,
}

#[derive(Debug, Clone)]
struct ProviderTemplate {
    name: &'static str,
    description: &'static str,
    section: ProviderSection,
    requires_api_key: bool,
    provider: ProviderConfig,
}

pub struct ConnectDialog {
    stage: ConnectStage,
    providers: Vec<ProviderTemplate>,
    selected: usize,
    selected_provider: Option<usize>,
    filter: String,
    api_key: String,
}

impl Default for ConnectDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectDialog {
    pub fn new() -> Self {
        Self {
            stage: ConnectStage::Provider,
            providers: provider_templates(),
            selected: 0,
            selected_provider: None,
            filter: String::new(),
            api_key: String::new(),
        }
    }

    pub fn stage(&self) -> ConnectStage {
        self.stage
    }

    pub fn next(&mut self) {
        let len = self.visible_provider_indices().len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub fn prev(&mut self) {
        let len = self.visible_provider_indices().len();
        if len > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(len - 1);
        }
    }

    pub fn push_filter_char(&mut self, c: char) {
        if self.stage == ConnectStage::Provider {
            self.filter.push(c);
            self.selected = 0;
        }
    }

    pub fn pop_filter_char(&mut self) {
        if self.stage == ConnectStage::Provider {
            self.filter.pop();
            self.selected = 0;
        }
    }

    pub fn push_api_key_char(&mut self, c: char) {
        if self.stage == ConnectStage::ApiKey {
            self.api_key.push(c);
        }
    }

    pub fn pop_api_key_char(&mut self) {
        if self.stage == ConnectStage::ApiKey {
            self.api_key.pop();
        }
    }

    pub fn confirm(&mut self) -> Option<ConnectAction> {
        match self.stage {
            ConnectStage::Provider => {
                let provider_idx = self.selected_provider_index()?;
                if self.providers[provider_idx].requires_api_key {
                    self.selected_provider = Some(provider_idx);
                    self.api_key.clear();
                    self.stage = ConnectStage::ApiKey;
                    None
                } else {
                    Some(self.action_for(provider_idx, None))
                }
            }
            ConnectStage::ApiKey => {
                if self.api_key.trim().is_empty() {
                    return None;
                }
                let provider_idx = self.selected_provider?;
                Some(self.action_for(provider_idx, Some(self.api_key.clone())))
            }
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        match self.stage {
            ConnectStage::Provider => self.render_provider_stage(f, area, theme),
            ConnectStage::ApiKey => self.render_api_key_stage(f, area, theme),
        }
    }

    fn selected_provider_index(&self) -> Option<usize> {
        let visible = self.visible_provider_indices();
        visible.get(self.selected).copied()
    }

    fn visible_provider_indices(&self) -> Vec<usize> {
        let query = self.filter.trim().to_ascii_lowercase();
        self.providers
            .iter()
            .enumerate()
            .filter_map(|(idx, provider)| {
                if query.is_empty()
                    || provider.name.to_ascii_lowercase().contains(&query)
                    || provider.description.to_ascii_lowercase().contains(&query)
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    fn action_for(&self, idx: usize, api_key: Option<String>) -> ConnectAction {
        let template = &self.providers[idx];
        let mut provider = template.provider.clone();
        provider.api_key = api_key;
        ConnectAction {
            name: template.name.to_string(),
            provider,
        }
    }

    fn render_provider_stage(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 76, 28);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line("Connect a provider", inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        f.render_widget(
            search_line(&self.filter, theme),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
        );
        self.render_provider_rows(
            f,
            Rect::new(
                inner.x,
                inner.y.saturating_add(4),
                inner.width,
                inner.height.saturating_sub(6),
            ),
            theme,
        );
        f.render_widget(
            footer_line("enter", "select", theme),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }

    fn render_provider_rows(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let visible = self.visible_provider_indices();
        if visible.is_empty() {
            f.render_widget(
                Paragraph::new("No providers")
                    .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
                Rect::new(area.x, area.y, area.width, 1),
            );
            return;
        }

        let selected_provider = self.selected_provider_index();
        let mut y = area.y;
        for section in [ProviderSection::Popular, ProviderSection::Providers] {
            let section_indices: Vec<usize> = visible
                .iter()
                .copied()
                .filter(|idx| self.providers[*idx].section == section)
                .collect();
            if section_indices.is_empty() || y >= area.y.saturating_add(area.height) {
                continue;
            }

            f.render_widget(
                Paragraph::new(section_title(section)).style(
                    Style::default()
                        .fg(theme.accent)
                        .bg(theme.bg_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Rect::new(area.x, y, area.width, 1),
            );
            y = y.saturating_add(1);

            for idx in section_indices {
                if y >= area.y.saturating_add(area.height) {
                    return;
                }
                let provider = &self.providers[idx];
                let selected = Some(idx) == selected_provider;
                let prefix = if selected { "> " } else { "  " };
                let label = format!("{prefix}{}", provider.name);
                let label = fixed_width(&label, 28);
                let row = format!("{label}{}", provider.description);
                let row = pad_to_width(row, area.width);
                let style = if selected {
                    Style::default()
                        .fg(theme.bg_primary)
                        .bg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
                };
                f.render_widget(
                    Paragraph::new(row).style(style),
                    Rect::new(area.x, y, area.width, 1),
                );
                y = y.saturating_add(1);
            }
            y = y.saturating_add(1);
        }
    }

    fn render_api_key_stage(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        let popup = modal_area(area, 62, 10);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line("API key", inner.width, theme),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        let provider = self
            .selected_provider
            .and_then(|idx| self.providers.get(idx))
            .map(|p| p.name)
            .unwrap_or("Provider");
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Provider ",
                    Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
                ),
                Span::styled(
                    provider,
                    Style::default()
                        .fg(theme.fg_white)
                        .bg(theme.bg_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .style(Style::default().bg(theme.bg_secondary)),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
        );

        f.render_widget(
            Paragraph::new(field_line(
                "API key",
                &mask_secret(&self.api_key),
                inner.width,
                theme,
            ))
            .style(Style::default().bg(theme.bg_secondary)),
            Rect::new(inner.x, inner.y.saturating_add(4), inner.width, 1),
        );
        f.render_widget(
            footer_line("enter", "submit", theme),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }
}

fn provider_templates() -> Vec<ProviderTemplate> {
    vec![
        ProviderTemplate {
            name: "MiniMax Token Plan",
            description: "MiniMax M1 long-context AI",
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Anthropic,
                Some("https://api.minimaxi.com/anthropic/v1"),
                Some("MiniMax-M1"),
                None,
            ),
        },
        ProviderTemplate {
            name: "DeepSeek",
            description: "Fast coding and reasoning models",
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.deepseek.com/v1"),
                Some("deepseek-v4-pro"),
                Some("deepseek"),
            ),
        },
        ProviderTemplate {
            name: "OpenAI",
            description: "OpenAI API and ChatGPT models",
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.openai.com/v1"),
                Some("gpt-4.1"),
                Some("standard"),
            ),
        },
        ProviderTemplate {
            name: "Anthropic",
            description: "Claude models via Anthropic API",
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Anthropic,
                None,
                Some("claude-sonnet-4-6"),
                None,
            ),
        },
        ProviderTemplate {
            name: "Moonshot AI",
            description: "Kimi and K2 models",
            section: ProviderSection::Providers,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.moonshot.ai/v1"),
                Some("kimi-k2-0711-preview"),
                Some("moonshot"),
            ),
        },
        ProviderTemplate {
            name: "OpenRouter",
            description: "Multi-provider model router",
            section: ProviderSection::Providers,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://openrouter.ai/api/v1"),
                Some("openai/gpt-4.1"),
                Some("openrouter"),
            ),
        },
        ProviderTemplate {
            name: "Ollama",
            description: "Local models on your machine",
            section: ProviderSection::Providers,
            requires_api_key: false,
            provider: provider(
                ProviderKind::Ollama,
                Some("http://localhost:11434"),
                Some("llama3.1"),
                None,
            ),
        },
    ]
}

fn provider(
    kind: ProviderKind,
    base_url: Option<&str>,
    model: Option<&str>,
    dialect: Option<&str>,
) -> ProviderConfig {
    ProviderConfig {
        r#type: Some(kind),
        api_key: None,
        base_url: base_url.map(str::to_string),
        model: model.map(str::to_string),
        dialect: dialect.map(str::to_string),
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
    let text = if filter.is_empty() {
        "earch".to_string()
    } else {
        format!("earch {filter}")
    };
    Paragraph::new(Line::from(vec![
        Span::styled(
            "S",
            Style::default()
                .fg(theme.accent_secondary)
                .bg(theme.bg_secondary),
        ),
        Span::styled(
            text,
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

fn field_line(label: &'static str, value: &str, width: u16, theme: &Theme) -> Line<'static> {
    let label = format!("{label} ");
    let value = if value.is_empty() {
        " ".to_string()
    } else {
        value.to_string()
    };
    let rest = width.saturating_sub(label.chars().count() as u16) as usize;
    let value = pad_to_width(value, rest as u16);
    Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            value,
            Style::default().fg(theme.fg_text).bg(theme.bg_secondary),
        ),
    ])
}

fn footer_line(key: &'static str, label: &'static str, theme: &Theme) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            key,
            Style::default()
                .fg(theme.fg_white)
                .bg(theme.bg_secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
        ),
    ]))
    .style(Style::default().bg(theme.bg_secondary))
}

fn section_title(section: ProviderSection) -> &'static str {
    match section {
        ProviderSection::Popular => "Popular",
        ProviderSection::Providers => "Providers",
    }
}

fn fixed_width(value: &str, width: usize) -> String {
    let mut out: String = value.chars().take(width).collect();
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

fn mask_secret(secret: &str) -> String {
    "*".repeat(secret.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_action_sets_openai_dialect() {
        let mut d = ConnectDialog::new();
        d.push_filter_char('d');
        d.push_filter_char('e');
        assert_eq!(d.confirm().map(|_| ()), None);
        assert_eq!(d.stage(), ConnectStage::ApiKey);
        d.push_api_key_char('s');
        d.push_api_key_char('k');

        let action = d.confirm().expect("api key submits provider");
        assert_eq!(action.name, "DeepSeek");
        assert_eq!(action.provider.kind(), ProviderKind::Openai);
        assert_eq!(
            action.provider.base_url.as_deref(),
            Some("https://api.deepseek.com/v1")
        );
        assert_eq!(action.provider.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(action.provider.dialect.as_deref(), Some("deepseek"));
        assert_eq!(action.provider.api_key.as_deref(), Some("sk"));
    }

    #[test]
    fn ollama_submits_without_api_key() {
        let mut d = ConnectDialog::new();
        for c in "ollama".chars() {
            d.push_filter_char(c);
        }
        let action = d.confirm().expect("ollama does not need an API key");

        assert_eq!(action.name, "Ollama");
        assert_eq!(action.provider.kind(), ProviderKind::Ollama);
        assert_eq!(
            action.provider.base_url.as_deref(),
            Some("http://localhost:11434")
        );
        assert!(action.provider.api_key.is_none());
    }

    #[test]
    fn provider_modal_uses_opencode_style() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let dialog = ConnectDialog::new();

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
        assert!(content.contains("Connect a provider"));
        assert!(content.contains("Search"));
        assert!(content.contains("Popular"));
        assert!(content.contains("MiniMax Token Plan"));
        assert!(content.contains("MiniMax M1 long-context AI"));
        assert!(content.contains("DeepSeek"));
        assert!(!content.contains("Anthropic-compatible endpoint"));
        assert!(!content.contains("┌"));
    }

    #[test]
    fn provider_modal_highlights_selection_with_theme_accent() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let dialog = ConnectDialog::new();

        terminal
            .draw(|f| dialog.render(f, f.area(), &theme))
            .unwrap();

        let popup = modal_area(Rect::new(0, 0, 100, 34), 76, 28);
        let inner = inner_area(popup);
        let first_provider_y = inner.y + 5;
        let selected_cell = terminal
            .backend()
            .buffer()
            .cell((inner.x, first_provider_y))
            .unwrap();
        assert_eq!(selected_cell.bg, theme.accent);
    }

    #[test]
    fn provider_modal_preserves_screen_behind_dialog() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let dialog = ConnectDialog::new();

        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new("CHAT_FRAME_VISIBLE").style(Style::default().fg(theme.fg_text)),
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
        assert!(content.contains("CHAT_FRAME_VISIBLE"));
        assert!(content.contains("Connect a provider"));
    }

    #[test]
    fn api_key_modal_masks_secret() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(90, 22);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut dialog = ConnectDialog::new();
        dialog.confirm();
        for c in "sk-secret".chars() {
            dialog.push_api_key_char(c);
        }

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
        assert!(content.contains("API key"));
        assert!(content.contains("enter submit"));
        assert!(content.contains("*********"));
        assert!(!content.contains("sk-secret"));
        assert!(!content.contains("┌"));
    }
}
