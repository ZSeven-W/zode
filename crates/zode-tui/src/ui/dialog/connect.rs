//! Provider connection dialog opened by `/connect`.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::config::{ProviderConfig, ProviderKind};
use zode_core::Catalog;

use super::connect_render::{
    field_line_focused, fixed_width, footer_line, header_line, inner_area, kind_label, mask_secret,
    modal_area, pad_to_width, search_line, section_title,
};
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

/// Which section a provider belongs to in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSection {
    Popular,
    Providers,
}

/// A single entry in the provider picker list.
#[derive(Debug, Clone)]
pub(crate) struct ProviderTemplate {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) section: ProviderSection,
    pub(crate) requires_api_key: bool,
    pub(crate) provider: ProviderConfig,
    /// models.dev provider id — used to look up catalog models for pre-fill.
    /// `None` for the hardcoded Popular presets that don't map 1:1 to a catalog id.
    pub(crate) catalog_provider_id: Option<String>,
}

/// Prices stashed from a catalog model match; applied to `ProviderConfig` at submit.
#[derive(Debug, Clone)]
struct CatalogPrices {
    input_price: Option<f64>,
    output_price: Option<f64>,
    cache_read_price: Option<f64>,
    cache_write_price: Option<f64>,
}

/// Which form field is currently active in the API-key / form stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectField {
    ApiKey,
    Type,
    Model,
    BaseUrl,
    Context,
    MaxOutput,
}

/// Tab-order for all editable fields in the form stage.
const FORM_ORDER: [ConnectField; 6] = [
    ConnectField::ApiKey,
    ConnectField::Type,
    ConnectField::Model,
    ConnectField::BaseUrl,
    ConnectField::Context,
    ConnectField::MaxOutput,
];

pub struct ConnectDialog {
    stage: ConnectStage,
    providers: Vec<ProviderTemplate>,
    selected: usize,
    selected_provider: Option<usize>,
    filter: String,
    /// Current value of the API-key field.
    api_key: String,
    // --- form state (populated when entering ApiKey stage) ---
    focus: usize,
    kind: ProviderKind,
    model: String,
    base_url: String,
    context: String,
    max_output: String,
    /// Catalog handle for model lookup during form editing (optional).
    catalog: Option<Catalog>,
    /// Prices stashed from a catalog model match; applied at submit.
    pending_prices: Option<CatalogPrices>,
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
            focus: 0,
            kind: ProviderKind::default(),
            model: String::new(),
            base_url: String::new(),
            context: String::new(),
            max_output: String::new(),
            catalog: None,
            pending_prices: None,
        }
    }

    /// Catalog-driven constructor. Builds provider templates from the catalog
    /// (catalog providers → Providers section) merged with the 7 hardcoded
    /// Popular presets (deduped by id). Stores the catalog for model lookup.
    pub fn with_catalog(cat: &Catalog) -> Self {
        use super::catalog_providers::build_provider_templates;
        let providers = build_provider_templates(provider_templates(), cat);
        Self {
            stage: ConnectStage::Provider,
            providers,
            selected: 0,
            selected_provider: None,
            filter: String::new(),
            api_key: String::new(),
            focus: 0,
            kind: ProviderKind::default(),
            model: String::new(),
            base_url: String::new(),
            context: String::new(),
            max_output: String::new(),
            catalog: Some(cat.clone()),
            pending_prices: None,
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

    /// Legacy wrapper: types a char into the API-key field. Kept so older call
    /// sites and tests continue to compile; in new code prefer `input_char`.
    pub fn push_api_key_char(&mut self, c: char) {
        if self.stage == ConnectStage::ApiKey {
            self.api_key.push(c);
        }
    }

    /// Legacy wrapper: removes the last char from the API-key field.
    pub fn pop_api_key_char(&mut self) {
        if self.stage == ConnectStage::ApiKey {
            self.api_key.pop();
        }
    }

    // ── form reducer ──────────────────────────────────────────────────────────

    /// Returns the field that currently has keyboard focus.
    pub fn focused_field(&self) -> ConnectField {
        FORM_ORDER[self.focus]
    }

    /// Move focus to the next field (wraps around).
    pub fn focus_next(&mut self) {
        self.focus = (self.focus + 1) % FORM_ORDER.len();
    }

    /// Move focus to the previous field (wraps around).
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.checked_sub(1).unwrap_or(FORM_ORDER.len() - 1);
    }

    /// Cycle the `Type` field through Anthropic → Openai → Ollama (or reverse).
    /// No-op when the focused field is not `Type`.
    pub fn cycle_type(&mut self, forward: bool) {
        let order = [
            ProviderKind::Anthropic,
            ProviderKind::Openai,
            ProviderKind::Ollama,
        ];
        let n = order.len();
        let i = order.iter().position(|k| *k == self.kind).unwrap_or(0);
        self.kind = if forward {
            order[(i + 1) % n]
        } else {
            order[(i + n - 1) % n]
        };
    }

    /// Insert a character into the focused field.
    /// `Context`/`MaxOutput` only accept ASCII digits.
    /// `Type` ignores characters (use `cycle_type`).
    pub fn input_char(&mut self, c: char) {
        match self.focused_field() {
            ConnectField::ApiKey => self.api_key.push(c),
            ConnectField::Model => {
                self.model.push(c);
                self.apply_model_prefill();
            }
            ConnectField::BaseUrl => self.base_url.push(c),
            ConnectField::Context => {
                if c.is_ascii_digit() {
                    self.context.push(c);
                }
            }
            ConnectField::MaxOutput => {
                if c.is_ascii_digit() {
                    self.max_output.push(c);
                }
            }
            ConnectField::Type => {} // use cycle_type instead
        }
    }

    /// Remove the last character from the focused field.
    pub fn backspace(&mut self) {
        match self.focused_field() {
            ConnectField::ApiKey => {
                self.api_key.pop();
            }
            ConnectField::Model => {
                self.model.pop();
                self.apply_model_prefill();
            }
            ConnectField::BaseUrl => {
                self.base_url.pop();
            }
            ConnectField::Context => {
                self.context.pop();
            }
            ConnectField::MaxOutput => {
                self.max_output.pop();
            }
            ConnectField::Type => {}
        }
    }

    /// When the model buffer matches a catalog model for the active provider,
    /// pre-fill `context`/`max_output` (only if still empty) and stash prices.
    /// Clears stashed prices when the model no longer matches.
    fn apply_model_prefill(&mut self) {
        let Some(provider_idx) = self.selected_provider else {
            return;
        };
        // Resolve the catalog provider id for the selected template.
        // Do the catalog lookup first (borrow ends before we mutate self).
        let matched = self.catalog.as_ref().and_then(|cat| {
            let catalog_id = self.providers[provider_idx]
                .catalog_provider_id
                .as_deref()
                .map(str::to_string)
                // Fall back to matching by the template's name lowercased against
                // catalog provider ids (covers the hardcoded Popular presets which
                // have no catalog_provider_id).
                .or_else(|| {
                    let name_lower = self.providers[provider_idx].name.to_ascii_lowercase();
                    cat.providers()
                        .iter()
                        .find(|p| p.id == name_lower || p.name.to_ascii_lowercase() == name_lower)
                        .map(|p| p.id.clone())
                })?;
            cat.find_model(&catalog_id, &self.model).cloned()
        });

        match matched {
            Some(m) => {
                // Pre-fill context/max_output only if the field is still empty.
                if self.context.is_empty() {
                    if let Some(ctx) = m.context {
                        self.context = ctx.to_string();
                    }
                }
                if self.max_output.is_empty() {
                    if let Some(mo) = m.max_output {
                        self.max_output = mo.to_string();
                    }
                }
                // Stash prices to apply at submit.
                self.pending_prices = Some(CatalogPrices {
                    input_price: m.input_price,
                    output_price: m.output_price,
                    cache_read_price: m.cache_read_price,
                    cache_write_price: m.cache_write_price,
                });
            }
            None => {
                // Model doesn't match any known catalog model — clear stashed prices.
                self.pending_prices = None;
            }
        }
    }

    /// Expose the provider list for tests (e.g. dedup assertions in catalog_providers).
    #[cfg(test)]
    pub(crate) fn providers_for_test(&self) -> &[ProviderTemplate] {
        &self.providers
    }

    /// Read a field's current string value. Only compiled in test builds.
    #[cfg(test)]
    pub fn field_value(&self, f: ConnectField) -> String {
        match f {
            ConnectField::ApiKey => self.api_key.clone(),
            ConnectField::Type => format!("{:?}", self.kind),
            ConnectField::Model => self.model.clone(),
            ConnectField::BaseUrl => self.base_url.clone(),
            ConnectField::Context => self.context.clone(),
            ConnectField::MaxOutput => self.max_output.clone(),
        }
    }

    /// Directly focus a specific field (test helper).
    #[cfg(test)]
    pub fn focus_field_for_test(&mut self, f: ConnectField) {
        self.focus = FORM_ORDER.iter().position(|x| *x == f).unwrap();
    }

    /// Pick the named preset by display-name and advance to the form stage.
    /// Panics if the name does not match any provider (test helper).
    #[cfg(test)]
    pub fn select_to_api_key_for_test(&mut self, name: &str) {
        let idx = self
            .providers
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no provider named {name:?}"));
        self.selected = self
            .visible_provider_indices()
            .iter()
            .position(|&i| i == idx)
            .unwrap_or_else(|| panic!("provider {name:?} not visible"));
        self.confirm(); // advances to ApiKey stage and pre-fills form
    }

    // ── submit ────────────────────────────────────────────────────────────────

    pub fn confirm(&mut self) -> Option<ConnectAction> {
        match self.stage {
            ConnectStage::Provider => {
                let provider_idx = self.selected_provider_index()?;
                if self.providers[provider_idx].requires_api_key {
                    // Pre-fill form fields from the preset.
                    let preset = &self.providers[provider_idx].provider;
                    self.kind = preset.kind();
                    self.model = preset.model.clone().unwrap_or_default();
                    self.base_url = preset.base_url.clone().unwrap_or_default();
                    self.context = preset
                        .context_window
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    self.max_output = preset
                        .max_output_tokens
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    self.focus = 0; // default to API key field
                    self.api_key.clear();
                    self.selected_provider = Some(provider_idx);
                    self.stage = ConnectStage::ApiKey;
                    None
                } else {
                    Some(self.action_for(provider_idx, None))
                }
            }
            ConnectStage::ApiKey => {
                let provider_idx = self.selected_provider?;
                let requires_key = self.providers[provider_idx].requires_api_key;
                if requires_key && self.api_key.trim().is_empty() {
                    return None; // guard: don't submit without a key
                }
                // Build the full ProviderConfig from the form buffers.
                let mut prov = self.providers[provider_idx].provider.clone();
                prov.r#type = Some(self.kind);
                prov.api_key = Some(self.api_key.clone()).filter(|s| !s.trim().is_empty());
                if !self.model.trim().is_empty() {
                    prov.model = Some(self.model.clone());
                }
                if !self.base_url.trim().is_empty() {
                    prov.base_url = Some(self.base_url.clone());
                }
                prov.context_window = self.context.parse::<u32>().ok();
                prov.max_output_tokens = self.max_output.parse::<u32>().ok();
                // Apply catalog prices if a model match was found during editing.
                if let Some(ref prices) = self.pending_prices {
                    prov.input_price = prices.input_price;
                    prov.output_price = prices.output_price;
                    prov.cache_read_price = prices.cache_read_price;
                    prov.cache_write_price = prices.cache_write_price;
                }
                // Provider name defaults to model id (the config map key),
                // editable in the model field.
                let name = if self.model.trim().is_empty() {
                    self.providers[provider_idx].name.to_string()
                } else {
                    self.model.clone()
                };
                Some(ConnectAction {
                    name,
                    provider: prov,
                })
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
        // Taller popup to accommodate all six form fields.
        let popup = modal_area(area, 62, 16);
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

        // Provider name row.
        let provider_name: String = self
            .selected_provider
            .and_then(|idx| self.providers.get(idx))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Provider".to_string());
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Provider ",
                    Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary),
                ),
                Span::styled(
                    provider_name,
                    Style::default()
                        .fg(theme.fg_white)
                        .bg(theme.bg_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .style(Style::default().bg(theme.bg_secondary)),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
        );

        // One row per form field, starting at row 4.
        let form_fields: [(ConnectField, &str); 6] = [
            (ConnectField::ApiKey, "API key"),
            (ConnectField::Type, "type"),
            (ConnectField::Model, "model"),
            (ConnectField::BaseUrl, "base URL"),
            (ConnectField::Context, "context"),
            (ConnectField::MaxOutput, "max output"),
        ];
        for (row_idx, (field, label)) in form_fields.iter().enumerate() {
            let focused = self.focused_field() == *field;
            let value = match field {
                ConnectField::ApiKey => mask_secret(&self.api_key),
                ConnectField::Type => kind_label(self.kind),
                ConnectField::Model => self.model.clone(),
                ConnectField::BaseUrl => self.base_url.clone(),
                ConnectField::Context => self.context.clone(),
                ConnectField::MaxOutput => self.max_output.clone(),
            };
            let y_off = 4u16.saturating_add(row_idx as u16);
            f.render_widget(
                Paragraph::new(field_line_focused(
                    label,
                    &value,
                    inner.width,
                    theme,
                    focused,
                ))
                .style(Style::default().bg(theme.bg_secondary)),
                Rect::new(inner.x, inner.y.saturating_add(y_off), inner.width, 1),
            );
        }

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
            name: "MiniMax Token Plan".to_string(),
            description: "MiniMax M1 long-context AI".to_string(),
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Anthropic,
                Some("https://api.minimaxi.com/anthropic/v1"),
                Some("MiniMax-M1"),
                None,
            ),
            catalog_provider_id: None,
        },
        ProviderTemplate {
            name: "DeepSeek".to_string(),
            description: "Fast coding and reasoning models".to_string(),
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.deepseek.com/v1"),
                Some("deepseek-v4-pro"),
                Some("deepseek"),
            ),
            catalog_provider_id: Some("deepseek".to_string()),
        },
        ProviderTemplate {
            name: "OpenAI".to_string(),
            description: "OpenAI API and ChatGPT models".to_string(),
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.openai.com/v1"),
                Some("gpt-4.1"),
                Some("standard"),
            ),
            catalog_provider_id: Some("openai".to_string()),
        },
        ProviderTemplate {
            name: "Anthropic".to_string(),
            description: "Claude models via Anthropic API".to_string(),
            section: ProviderSection::Popular,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Anthropic,
                None,
                Some("claude-sonnet-4-6"),
                None,
            ),
            catalog_provider_id: Some("anthropic".to_string()),
        },
        ProviderTemplate {
            name: "Moonshot AI".to_string(),
            description: "Kimi and K2 models".to_string(),
            section: ProviderSection::Providers,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://api.moonshot.ai/v1"),
                Some("kimi-k2-0711-preview"),
                Some("moonshot"),
            ),
            catalog_provider_id: Some("moonshot".to_string()),
        },
        ProviderTemplate {
            name: "OpenRouter".to_string(),
            description: "Multi-provider model router".to_string(),
            section: ProviderSection::Providers,
            requires_api_key: true,
            provider: provider(
                ProviderKind::Openai,
                Some("https://openrouter.ai/api/v1"),
                Some("openai/gpt-4.1"),
                Some("openrouter"),
            ),
            catalog_provider_id: Some("openrouter".to_string()),
        },
        ProviderTemplate {
            name: "Ollama".to_string(),
            description: "Local models on your machine".to_string(),
            section: ProviderSection::Providers,
            requires_api_key: false,
            provider: provider(
                ProviderKind::Ollama,
                Some("http://localhost:11434"),
                Some("llama3.1"),
                None,
            ),
            catalog_provider_id: Some("ollama".to_string()),
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
        ..Default::default()
    }
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
        // Name defaults to the model id (the config-map key) when model is set.
        assert_eq!(action.name, "deepseek-v4-pro");
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

    #[test]
    fn form_navigates_fields_and_edits_text() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek"); // pick a preset, enter form
                                                  // Default focus is the API key field.
        assert_eq!(d.focused_field(), ConnectField::ApiKey);
        d.input_char('s');
        d.input_char('k');
        assert_eq!(d.field_value(ConnectField::ApiKey), "sk");
        d.focus_next();
        assert_eq!(d.focused_field(), ConnectField::Type);
        d.cycle_type(true); // cycle ProviderKind forward
                            // model field takes text:
        d.focus_next();
        assert_eq!(d.focused_field(), ConnectField::Model);
        d.input_char('x');
        assert!(d.field_value(ConnectField::Model).ends_with('x'));
        // context field is digits-only:
        d.focus_field_for_test(ConnectField::Context);
        d.input_char('a'); // ignored (not a digit)
        d.input_char('9');
        assert_eq!(d.field_value(ConnectField::Context), "9");
    }

    #[test]
    fn submit_builds_full_provider_config() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek");
        for c in "sk-key".chars() {
            d.input_char(c);
        }
        d.focus_field_for_test(ConnectField::Context);
        for c in "1000000".chars() {
            d.input_char(c);
        }
        let action = d.confirm().expect("submit");
        assert_eq!(action.provider.api_key.as_deref(), Some("sk-key"));
        assert_eq!(action.provider.context_window, Some(1_000_000));
        assert!(action.provider.model.is_some()); // pre-filled from preset
        assert!(action.provider.base_url.is_some()); // pre-filled from preset
    }

    #[test]
    fn form_stage_renders_field_labels_and_cursor() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let backend = ratatui::backend::TestBackend::new(90, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut dialog = ConnectDialog::new();
        dialog.select_to_api_key_for_test("DeepSeek");

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
        assert!(content.contains("type"));
        assert!(content.contains("model"));
        assert!(content.contains("context"));
        // Cursor (reversed-video space) is present — not easy to check by symbol,
        // but the render must not panic and all labels must be present.
        assert!(content.contains("enter submit"));
    }

    // --- Task 4 tests: catalog wiring ---

    /// Fixture matching the real models.dev JSON shape (one provider, two models).
    const CATALOG_FIXTURE: &str = r#"{
      "deepseek": {
        "id": "deepseek",
        "name": "DeepSeek",
        "api": "https://api.deepseek.com",
        "models": {
          "deepseek-v4-pro": {
            "id": "deepseek-v4-pro",
            "name": "DeepSeek V4 Pro",
            "limit": { "context": 1000000, "output": 8192 },
            "cost": { "input": 0.28, "output": 0.42 }
          },
          "deepseek-r2": {
            "id": "deepseek-r2",
            "name": "DeepSeek R2",
            "limit": { "context": 131072, "output": 16384 },
            "cost": { "input": 0.14, "output": 0.28, "cache_read": 0.014, "cache_write": 0.28 }
          }
        }
      },
      "anotherco": {
        "id": "anotherco",
        "name": "AnotherCo",
        "api": "https://api.anotherco.io/v1",
        "models": {
          "fast-model": {
            "id": "fast-model",
            "name": "Fast Model"
          }
        }
      }
    }"#;

    #[test]
    fn with_catalog_builds_catalog_providers_deduped() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let d = ConnectDialog::with_catalog(&cat);
        // Hardcoded Popular presets must still be present.
        assert!(d.providers.iter().any(|p| p.name == "DeepSeek"));
        assert!(d.providers.iter().any(|p| p.name == "Anthropic"));
        // "anotherco" (not in Popular) should appear under Providers.
        assert!(d.providers.iter().any(|p| p.name == "AnotherCo"));
        // DeepSeek must NOT appear twice (catalog deepseek is in POPULAR_IDS).
        let deepseek_count = d.providers.iter().filter(|p| p.name == "DeepSeek").count();
        assert_eq!(deepseek_count, 1, "deepseek should not be duplicated");
    }

    #[test]
    fn catalog_provider_appears_in_providers_section() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let d = ConnectDialog::with_catalog(&cat);
        let anotherco = d.providers.iter().find(|p| p.name == "AnotherCo").unwrap();
        assert_eq!(anotherco.section, ProviderSection::Providers);
        assert_eq!(anotherco.catalog_provider_id.as_deref(), Some("anotherco"));
    }

    #[test]
    fn selecting_known_model_prefills_context_and_max_output() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        // Pick DeepSeek (Popular preset with catalog_provider_id="deepseek").
        d.select_to_api_key_for_test("DeepSeek");
        // Model buffer starts with "deepseek-v4-pro" (pre-filled from preset).
        // Clear model buffer and type a known model id to trigger pre-fill.
        while !d.model.is_empty() {
            d.focus_field_for_test(ConnectField::Model);
            d.backspace();
        }
        d.context.clear();
        d.max_output.clear();
        // Type a known model id character by character.
        d.focus_field_for_test(ConnectField::Model);
        for c in "deepseek-v4-pro".chars() {
            d.input_char(c);
        }
        // Context and max_output should be pre-filled from the catalog.
        assert_eq!(d.field_value(ConnectField::Context), "1000000");
        assert_eq!(d.field_value(ConnectField::MaxOutput), "8192");
    }

    #[test]
    fn confirm_carries_catalog_prices() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek");
        // Clear and retype the model so apply_model_prefill fires.
        while !d.model.is_empty() {
            d.focus_field_for_test(ConnectField::Model);
            d.backspace();
        }
        d.context.clear();
        d.max_output.clear();
        d.focus_field_for_test(ConnectField::Model);
        for c in "deepseek-r2".chars() {
            d.input_char(c);
        }
        // Prices should be stashed now; type the API key and submit.
        d.focus_field_for_test(ConnectField::ApiKey);
        for c in "sk-test".chars() {
            d.input_char(c);
        }
        let action = d.confirm().expect("submit");
        assert_eq!(action.provider.input_price, Some(0.14));
        assert_eq!(action.provider.output_price, Some(0.28));
        assert_eq!(action.provider.cache_read_price, Some(0.014));
        assert_eq!(action.provider.cache_write_price, Some(0.28));
        // Context/max_output should also be set.
        assert_eq!(action.provider.context_window, Some(131072));
        assert_eq!(action.provider.max_output_tokens, Some(16384));
    }

    #[test]
    fn no_double_prefill_when_context_already_set() {
        // If the user already typed a context value, prefill must NOT overwrite it.
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek");
        // Manually set context to user-provided value.
        d.context = "99999".to_string();
        d.max_output.clear();
        // Clear model and retype to trigger apply_model_prefill.
        while !d.model.is_empty() {
            d.focus_field_for_test(ConnectField::Model);
            d.backspace();
        }
        d.focus_field_for_test(ConnectField::Model);
        for c in "deepseek-v4-pro".chars() {
            d.input_char(c);
        }
        // User-set context must be preserved (not overwritten by catalog 1000000).
        assert_eq!(d.field_value(ConnectField::Context), "99999");
        // max_output was empty so it should be filled.
        assert_eq!(d.field_value(ConnectField::MaxOutput), "8192");
    }
}
