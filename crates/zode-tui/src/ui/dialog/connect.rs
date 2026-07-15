//! Provider connection dialog opened by `/connect`.

use indexmap::IndexMap;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use zode_core::config::{ModelOverride, ProviderConfig, ProviderKind};
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
    /// Provider-group key under which the model is stored in `providers`, so one
    /// provider can accumulate several models without duplicating credentials.
    pub provider_key: String,
    /// Per-model override (context/max-output/prices) stored under
    /// `providers[provider_key].models[model]`.
    pub model_override: ModelOverride,
}

/// The `providers` map key a connected template groups under: its catalog id
/// when known, else a slug of its display name.
fn provider_group_key(template: &ProviderTemplate) -> String {
    template
        .catalog_provider_id
        .clone()
        .unwrap_or_else(|| slugify(&template.name))
}

/// Lowercase alphanumeric slug: runs of non-alphanumeric characters collapse to
/// a single `-`, leading/trailing dashes are trimmed, and an empty result (e.g.
/// a punctuation-only name) falls back to `"custom"`. Unicode-aware so a non-
/// ASCII provider name still yields a usable, stable key.
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "custom".to_string()
    } else {
        slug
    }
}

/// Whether a pasted character should be kept. Drops control characters
/// (newlines, tabs) and invisible / default-ignorable Unicode format characters
/// — a zero-width space, soft hyphen, BOM, bidi mark, or variation selector
/// hiding in a copied API key or URL would otherwise cause auth failures or URL
/// parse errors that are very hard to debug. The connect fields are effectively
/// ASCII (API key, base URL, model id, digits), so stripping the whole invisible
/// class is safe. Ranges cover the common default-ignorable code points without
/// pulling in a Unicode-properties dependency.
fn is_pasteable(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    !matches!(c,
        '\u{00AD}'                  // soft hyphen
        | '\u{034F}'                // combining grapheme joiner
        | '\u{061C}'                // arabic letter mark
        | '\u{115F}'..='\u{1160}'   // Hangul choseong/jungseong fillers
        | '\u{17B4}'..='\u{17B5}'   // Khmer inherent vowels
        | '\u{180B}'..='\u{180F}'   // Mongolian variation/vowel separators
        | '\u{200B}'..='\u{200F}'   // zero-width space/joiners + LTR/RTL marks
        | '\u{202A}'..='\u{202E}'   // bidi embedding/override
        | '\u{2060}'..='\u{206F}'   // word joiner, invisible operators, deprecated
        | '\u{FE00}'..='\u{FE0F}'   // variation selectors
        | '\u{FEFF}'                // BOM / zero-width no-break space
        | '\u{FFF9}'..='\u{FFFB}'   // interlinear annotation anchors
    )
}

/// Append a character to a price ($/MTok) buffer: ASCII digits, plus a single
/// decimal point. Anything else (including a second dot) is ignored.
fn push_price_char(buf: &mut String, c: char) {
    if c.is_ascii_digit() || (c == '.' && !buf.contains('.')) {
        buf.push(c);
    }
}

/// Format a catalog price for the editable field. `f64::to_string` already
/// yields the shortest round-tripping decimal (e.g. 0.28, 0.014).
fn format_price(p: f64) -> String {
    p.to_string()
}

/// Adopt a models.dev `api` URL as a base URL: if it's a bare host (no protocol
/// path), append the suffix for `kind` (`/v1` for OpenAI, `/anthropic` for
/// Anthropic); if it already carries a `/v1` or `/anthropic` path, use it as
/// published (e.g. MiniMax's `.../anthropic/v1`, OpenRouter's `.../api/v1`).
fn ensure_protocol_suffix(api: &str, kind: ProviderKind) -> String {
    let trimmed = api.trim_end_matches('/');
    if trimmed.ends_with("/v1") || trimmed.ends_with("/anthropic") {
        return trimmed.to_string();
    }
    match kind {
        ProviderKind::Anthropic => format!("{trimmed}/anthropic"),
        ProviderKind::Openai => format!("{trimmed}/v1"),
        ProviderKind::Ollama => trimmed.to_string(),
    }
}

/// Rewrite a base URL's protocol suffix to match `kind` — providers that speak
/// both protocols expose `/v1` (OpenAI-compatible) and `/anthropic` endpoints
/// on the same host (e.g. DeepSeek). Returns `None` (leave the URL as-is) when
/// the suffix isn't recognized or the URL is already in the target shape, so a
/// non-conforming or empty base URL is never mangled.
fn base_url_for_kind(base: &str, kind: ProviderKind) -> Option<String> {
    let trimmed = base.trim_end_matches('/');
    match kind {
        ProviderKind::Anthropic => {
            let root = trimmed.strip_suffix("/v1")?;
            // Already an anthropic-style URL (e.g. .../anthropic/v1) → leave it.
            if root.ends_with("/anthropic") {
                return None;
            }
            Some(format!("{root}/anthropic"))
        }
        ProviderKind::Openai => {
            // `.../anthropic` → `.../v1`, and `.../anthropic/v1` → `.../v1`
            // (the latter would otherwise strand the user on an anthropic URL).
            if let Some(root) = trimmed.strip_suffix("/anthropic/v1") {
                Some(format!("{root}/v1"))
            } else {
                trimmed
                    .strip_suffix("/anthropic")
                    .map(|root| format!("{root}/v1"))
            }
        }
        ProviderKind::Ollama => None,
    }
}

/// Extract the per-model fields of a resolved provider into a `ModelOverride`.
fn model_override_from(p: &ProviderConfig) -> ModelOverride {
    ModelOverride {
        context_window: p.context_window,
        max_output_tokens: p.max_output_tokens,
        supports_images: p.supports_images,
        input_price: p.input_price,
        output_price: p.output_price,
        cache_read_price: p.cache_read_price,
        cache_write_price: p.cache_write_price,
    }
}

/// Which section a provider belongs to in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderSection {
    /// Providers already saved in the user's config (`providers` map). Shown
    /// first; selecting one jumps to the form with its credentials pre-filled to
    /// add or pick a model.
    Configured,
    /// Built-in presets + the models.dev catalog — for connecting a new provider.
    Providers,
}

/// One rendered line in the provider list: a section heading, a provider row
/// (index into `providers`), or a blank spacer between sections.
enum ProviderRow {
    Header(ProviderSection),
    Item(usize),
    Spacer,
}

/// Rows the Page Up / Page Down keys jump through the provider list.
const PROVIDER_PAGE: usize = 8;

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

/// Cache prices stashed from a catalog model match; applied to `ProviderConfig`
/// at submit. Input/output prices live in the editable `input_price`/
/// `output_price` fields instead; only the (unshown) cache prices carry here.
#[derive(Debug, Clone)]
struct CatalogPrices {
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
    InputPrice,
    OutputPrice,
}

/// Tab-order for all editable fields in the form stage.
const FORM_ORDER: [ConnectField; 8] = [
    ConnectField::ApiKey,
    ConnectField::Type,
    ConnectField::Model,
    ConnectField::BaseUrl,
    ConnectField::Context,
    ConnectField::MaxOutput,
    ConnectField::InputPrice,
    ConnectField::OutputPrice,
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
    /// Per-million-token prices ($/MTok), editable; pre-filled from the catalog.
    input_price: String,
    output_price: String,
    /// Catalog handle for model lookup during form editing (optional).
    catalog: Option<Catalog>,
    /// Prices stashed from a catalog model match; applied at submit.
    pending_prices: Option<CatalogPrices>,
    /// Image-input capability stashed from the matched catalog model. Kept
    /// separate from the editable form fields, like cache prices.
    pending_supports_images: Option<bool>,
    /// Catalog model ids offered for the selected provider (Left/Right cycle in
    /// the Model field). Populated on form entry; empty when no catalog match.
    available_models: Vec<String>,
    /// The user's configured providers (config `providers` map), keyed by group
    /// name. Drives the "Configured" section and its models on form entry.
    config_providers: IndexMap<String, ProviderConfig>,
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
            input_price: String::new(),
            output_price: String::new(),
            catalog: None,
            pending_prices: None,
            pending_supports_images: None,
            available_models: Vec::new(),
            config_providers: IndexMap::new(),
        }
    }

    /// Catalog-driven constructor. Builds provider templates from the catalog
    /// (catalog providers → Providers section) merged with the hardcoded presets
    /// (deduped by id). Stores the catalog for model lookup.
    pub fn with_catalog(cat: &Catalog) -> Self {
        Self::with_catalog_and_providers(cat, &IndexMap::new())
    }

    /// Catalog + the user's configured providers. The configured providers form
    /// a "Configured" section listed FIRST (selecting one jumps to the form with
    /// its credentials pre-filled to add/pick a model); the presets + catalog
    /// follow in the "Providers" section.
    pub fn with_catalog_and_providers(
        cat: &Catalog,
        config_providers: &IndexMap<String, ProviderConfig>,
    ) -> Self {
        use super::catalog_providers::build_provider_templates;
        let mut providers = configured_templates(config_providers);
        providers.extend(build_provider_templates(provider_templates(), cat));
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
            input_price: String::new(),
            output_price: String::new(),
            catalog: Some(cat.clone()),
            pending_prices: None,
            pending_supports_images: None,
            available_models: Vec::new(),
            config_providers: config_providers.clone(),
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

    /// Jump to the first / last visible provider (Home / End). Clamped.
    pub fn first(&mut self) {
        if !self.visible_provider_indices().is_empty() {
            self.selected = 0;
        }
    }

    pub fn last(&mut self) {
        let len = self.visible_provider_indices().len();
        if len > 0 {
            self.selected = len - 1;
        }
    }

    /// Move the selection a page up / down (Page Up / Page Down). Clamped — no
    /// wraparound, unlike single-step `next`/`prev`.
    pub fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(PROVIDER_PAGE);
    }

    pub fn page_down(&mut self) {
        let len = self.visible_provider_indices().len();
        if len > 0 {
            self.selected = self.selected.saturating_add(PROVIDER_PAGE).min(len - 1);
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
        // Only the two API protocols are toggleable in the form; Ollama is its
        // own (local) provider preset and isn't part of this switch.
        let order = [ProviderKind::Anthropic, ProviderKind::Openai];
        let n = order.len();
        let i = order.iter().position(|k| *k == self.kind).unwrap_or(0);
        self.kind = if forward {
            order[(i + 1) % n]
        } else {
            order[(i + n - 1) % n]
        };
        // Keep the base URL's protocol suffix in sync with the selected type
        // (e.g. DeepSeek: /v1 ↔ /anthropic). Best-effort: left untouched when the
        // URL doesn't carry a recognized suffix.
        if let Some(url) = base_url_for_kind(&self.base_url, self.kind) {
            self.base_url = url;
        }
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
            // Prices are decimals in $/MTok: digits and a single dot.
            ConnectField::InputPrice => push_price_char(&mut self.input_price, c),
            ConnectField::OutputPrice => push_price_char(&mut self.output_price, c),
            ConnectField::Type => {} // use cycle_type instead
        }
    }

    /// Insert pasted text. In the provider stage it extends the search filter;
    /// in the form stage it inserts into the focused field (routing through
    /// `input_char`, so digit-only fields still reject non-digits). Control
    /// characters (newlines, tabs) AND invisible Unicode format characters
    /// (zero-width space/joiner, word joiner, BOM) are stripped — a pasted API
    /// key or URL copied from a browser lands clean instead of silently failing
    /// auth or URL parsing.
    pub fn paste(&mut self, text: &str) {
        match self.stage {
            ConnectStage::Provider => {
                for c in text.chars().filter(|c| is_pasteable(*c)) {
                    self.filter.push(c);
                }
                self.selected = 0;
            }
            ConnectStage::ApiKey => {
                for c in text.chars().filter(|c| is_pasteable(*c)) {
                    self.input_char(c);
                }
            }
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
            ConnectField::InputPrice => {
                self.input_price.pop();
            }
            ConnectField::OutputPrice => {
                self.output_price.pop();
            }
            ConnectField::Type => {}
        }
    }

    /// Resolve the models.dev provider id for the template at `provider_idx`:
    /// its explicit `catalog_provider_id`, else a name match against the catalog
    /// (covers the hardcoded Popular presets). `None` if no catalog or no match.
    fn catalog_id_for(&self, provider_idx: usize) -> Option<String> {
        let cat = self.catalog.as_ref()?;
        self.providers[provider_idx]
            .catalog_provider_id
            .clone()
            .or_else(|| {
                let name_lower = self.providers[provider_idx].name.to_ascii_lowercase();
                cat.providers()
                    .iter()
                    .find(|p| p.id == name_lower || p.name.to_ascii_lowercase() == name_lower)
                    .map(|p| p.id.clone())
            })
    }

    /// The models.dev base URL (`api` field) for the provider at `provider_idx`,
    /// if the catalog knows it. The source of truth for the base URL field.
    fn catalog_base_url_for(&self, provider_idx: usize) -> Option<String> {
        let cat = self.catalog.as_ref()?;
        let id = self.catalog_id_for(provider_idx)?;
        cat.providers()
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.base_url.clone())
    }

    /// The catalog model ids offered by the provider at `provider_idx`
    /// (in catalog order). Empty when there's no catalog or no provider match.
    fn catalog_models_for(&self, provider_idx: usize) -> Vec<String> {
        let Some(cat) = self.catalog.as_ref() else {
            return Vec::new();
        };
        let Some(catalog_id) = self.catalog_id_for(provider_idx) else {
            return Vec::new();
        };
        cat.providers()
            .iter()
            .find(|p| p.id == catalog_id)
            .map(|p| p.models.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default()
    }

    /// The models already saved under this provider in the user's config (the
    /// "Configured" section), so the Model field can cycle them. Empty unless the
    /// template's name matches a configured-providers group key.
    fn config_models_for(&self, provider_idx: usize) -> Vec<String> {
        self.config_providers
            .get(&self.providers[provider_idx].name)
            .map(|e| e.models.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn configured_model_image_override(&self, provider_idx: usize) -> Option<bool> {
        self.config_providers
            .get(&self.providers[provider_idx].name)
            .and_then(|entry| entry.models.get(&self.model))
            .and_then(|model| model.supports_images)
    }

    /// Cycle the Model field through the provider's catalog models (Left/Right).
    /// Resets and re-fills `context`/`max_output`/prices for the newly selected
    /// model. No-op when the provider has no catalog models.
    pub fn cycle_model(&mut self, forward: bool) {
        if self.available_models.is_empty() {
            return;
        }
        let n = self.available_models.len();
        let next = match self.available_models.iter().position(|m| *m == self.model) {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            // Current value isn't a catalog model (custom-typed) → jump to the
            // first (or last, going backward) catalog model.
            None if forward => 0,
            None => n - 1,
        };
        let new_model = self.available_models[next].clone();
        // Single-model provider (or otherwise unchanged): don't wipe the user's
        // edited limits when the selection doesn't actually move.
        if new_model == self.model {
            return;
        }
        self.model = new_model;
        // A different model → drop the previous model's limits/prices and re-fill.
        self.context.clear();
        self.max_output.clear();
        self.input_price.clear();
        self.output_price.clear();
        self.pending_prices = None;
        self.pending_supports_images = None;
        self.apply_model_prefill();
    }

    /// When the model buffer matches a catalog model for the active provider,
    /// pre-fill `context`/`max_output` (only if still empty) and stash prices.
    /// Clears stashed prices when the model no longer matches.
    fn apply_model_prefill(&mut self) {
        let Some(provider_idx) = self.selected_provider else {
            return;
        };
        // Resolve the catalog provider id, then look up the model. Do the lookup
        // first so the borrow on `self.catalog` ends before we mutate `self`.
        let catalog_id = self.catalog_id_for(provider_idx);
        let configured_supports_images = self.configured_model_image_override(provider_idx);
        let matched = self
            .catalog
            .as_ref()
            .zip(catalog_id)
            .and_then(|(cat, id)| cat.find_model(&id, &self.model).cloned());

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
                // Pre-fill the editable input/output price fields (only if empty).
                if self.input_price.is_empty() {
                    if let Some(p) = m.input_price {
                        self.input_price = format_price(p);
                    }
                }
                if self.output_price.is_empty() {
                    if let Some(p) = m.output_price {
                        self.output_price = format_price(p);
                    }
                }
                // Stash cache prices to apply at submit (input/output live in
                // the editable fields above).
                self.pending_prices = Some(CatalogPrices {
                    cache_read_price: m.cache_read_price,
                    cache_write_price: m.cache_write_price,
                });
                // Persist a positive catalog capability so a just-refreshed
                // model works in this process. Keep catalog denials inferred:
                // writing `false` as an explicit model override would mask a
                // future catalog upgrade that adds image input support.
                self.pending_supports_images =
                    configured_supports_images.or(m.supports_images.filter(|supported| *supported));
            }
            None => {
                // Model doesn't match any known catalog model — clear stashed prices.
                self.pending_prices = None;
                self.pending_supports_images = configured_supports_images;
            }
        }
    }

    /// Expose the provider list for tests (e.g. dedup assertions in catalog_providers).
    #[cfg(test)]
    pub(crate) fn providers_for_test(&self) -> &[ProviderTemplate] {
        &self.providers
    }

    /// Catalog models offered for the active provider (test helper).
    #[cfg(test)]
    pub(crate) fn available_models_for_test(&self) -> &[String] {
        &self.available_models
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
            ConnectField::InputPrice => self.input_price.clone(),
            ConnectField::OutputPrice => self.output_price.clone(),
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
                                    // A configured provider carries a saved key — pre-fill it so
                                    // the user can add a model without re-entering it. Fresh
                                    // presets have no key → empty field.
                    self.api_key = preset.api_key.clone().unwrap_or_default();
                    self.selected_provider = Some(provider_idx);
                    // Fresh (non-configured) providers source the base URL from
                    // models.dev (the catalog `api` field) + protocol suffix;
                    // configured providers keep their saved base URL.
                    if self.providers[provider_idx].section != ProviderSection::Configured {
                        if let Some(api) = self.catalog_base_url_for(provider_idx) {
                            self.base_url = ensure_protocol_suffix(&api, self.kind);
                        }
                    }
                    // Offer this provider's configured models first, then any
                    // catalog models, for Left/Right selection; then pre-fill
                    // context/max_output/prices for the active model.
                    let mut models = self.config_models_for(provider_idx);
                    for m in self.catalog_models_for(provider_idx) {
                        if !models.contains(&m) {
                            models.push(m);
                        }
                    }
                    self.available_models = models;
                    self.apply_model_prefill();
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
                // Input/output prices come from the editable fields; cache prices
                // (rarely tuned, not shown) carry over from the catalog match.
                prov.input_price = self.input_price.trim().parse::<f64>().ok();
                prov.output_price = self.output_price.trim().parse::<f64>().ok();
                if let Some(ref prices) = self.pending_prices {
                    prov.cache_read_price = prices.cache_read_price;
                    prov.cache_write_price = prices.cache_write_price;
                }
                // Model-specific user config wins over the provider-level
                // default, which in turn wins over catalog inference. Persist
                // the result into the new per-model override so a just-refreshed
                // catalog model also works immediately.
                prov.supports_images = self
                    .configured_model_image_override(provider_idx)
                    .or(prov.supports_images)
                    .or(self.pending_supports_images);
                // Provider name defaults to model id (the config map key),
                // editable in the model field.
                let name = if self.model.trim().is_empty() {
                    self.providers[provider_idx].name.to_string()
                } else {
                    self.model.clone()
                };
                let provider_key = provider_group_key(&self.providers[provider_idx]);
                let model_override = model_override_from(&prov);
                Some(ConnectAction {
                    name,
                    provider: prov,
                    provider_key,
                    model_override,
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
        let provider_key = provider_group_key(template);
        let model_override = model_override_from(&provider);
        ConnectAction {
            name: template.name.to_string(),
            provider,
            provider_key,
            model_override,
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
            header_line(zode_core::i18n::t("Connect a provider"), inner.width, theme),
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
            footer_line("enter", zode_core::i18n::t("select"), theme),
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
                Paragraph::new(zode_core::i18n::t("No providers"))
                    .style(Style::default().fg(theme.fg_subtle).bg(theme.bg_secondary)),
                Rect::new(area.x, area.y, area.width, 1),
            );
            return;
        }

        let selected_provider = self.selected_provider_index();
        let rows = self.provider_rows(&visible);

        // Scroll so the selected provider's flat row stays on screen: once the
        // selection drops below the viewport, anchor it to the bottom edge.
        let sel_row = rows
            .iter()
            .position(|r| matches!(r, ProviderRow::Item(i) if Some(*i) == selected_provider))
            .unwrap_or(0);
        let height = area.height as usize;
        let offset = sel_row.saturating_sub(height.saturating_sub(1));

        let mut y = area.y;
        for row in rows.iter().skip(offset).take(height) {
            match row {
                ProviderRow::Header(section) => {
                    f.render_widget(
                        Paragraph::new(section_title(*section)).style(
                            Style::default()
                                .fg(theme.accent)
                                .bg(theme.bg_secondary)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Rect::new(area.x, y, area.width, 1),
                    );
                }
                ProviderRow::Spacer => {}
                ProviderRow::Item(idx) => {
                    let provider = &self.providers[*idx];
                    let selected = Some(*idx) == selected_provider;
                    let prefix = if selected { "> " } else { "  " };
                    let label = format!("{prefix}{}", provider.name);
                    let label = fixed_width(&label, 28);
                    let row_text = format!("{label}{}", provider.description);
                    let row_text = pad_to_width(row_text, area.width);
                    let style = if selected {
                        Style::default()
                            .fg(theme.bg_primary)
                            .bg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.fg_text).bg(theme.bg_secondary)
                    };
                    f.render_widget(
                        Paragraph::new(row_text).style(style),
                        Rect::new(area.x, y, area.width, 1),
                    );
                }
            }
            y = y.saturating_add(1);
        }
    }

    /// Flatten the visible providers into section-headed rows, with a blank
    /// spacer between sections. Indices point into `self.providers`.
    fn provider_rows(&self, visible: &[usize]) -> Vec<ProviderRow> {
        let mut out = Vec::new();
        for section in [ProviderSection::Configured, ProviderSection::Providers] {
            let mut any = false;
            for &idx in visible {
                if self.providers[idx].section == section {
                    if !any {
                        if !out.is_empty() {
                            out.push(ProviderRow::Spacer);
                        }
                        out.push(ProviderRow::Header(section));
                        any = true;
                    }
                    out.push(ProviderRow::Item(idx));
                }
            }
        }
        out
    }

    fn render_api_key_stage(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        // Taller popup to accommodate all eight form fields (incl. cost rows).
        let popup = modal_area(area, 62, 16);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.bg_secondary)),
            popup,
        );

        let inner = inner_area(popup);
        f.render_widget(
            header_line(zode_core::i18n::t("API key"), inner.width, theme),
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
                    format!("{} ", zode_core::i18n::t("Provider")),
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
        let form_fields: [(ConnectField, &str); 8] = [
            (ConnectField::ApiKey, "API key"),
            (ConnectField::Type, "type"),
            (ConnectField::Model, "model"),
            (ConnectField::BaseUrl, "base URL"),
            (ConnectField::Context, "context"),
            (ConnectField::MaxOutput, "max output"),
            (ConnectField::InputPrice, "input $/M"),
            (ConnectField::OutputPrice, "output $/M"),
        ];
        for (row_idx, (field, label)) in form_fields.iter().enumerate() {
            let focused = self.focused_field() == *field;
            let value = match field {
                ConnectField::ApiKey => mask_secret(&self.api_key),
                ConnectField::Type => kind_label(self.kind),
                // Wrap in ‹ › when the current model is one the provider's
                // catalog offers — a cue that Left/Right selects among them.
                ConnectField::Model if self.available_models.contains(&self.model) => {
                    format!("‹ {} ›", self.model)
                }
                ConnectField::Model => self.model.clone(),
                ConnectField::BaseUrl => self.base_url.clone(),
                ConnectField::Context => self.context.clone(),
                ConnectField::MaxOutput => self.max_output.clone(),
                ConnectField::InputPrice => self.input_price.clone(),
                ConnectField::OutputPrice => self.output_price.clone(),
            };
            let y_off = 4u16.saturating_add(row_idx as u16);
            f.render_widget(
                Paragraph::new(field_line_focused(
                    zode_core::i18n::t(label),
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
            footer_line("enter", zode_core::i18n::t("submit"), theme),
            Rect::new(
                inner.x,
                inner.y.saturating_add(inner.height.saturating_sub(1)),
                inner.width,
                1,
            ),
        );
    }
}

/// Build "Configured" section rows from the user's saved `providers` map. Each
/// row carries the provider's credentials so selecting it jumps straight to the
/// form (key/base URL pre-filled) to add or pick a model under that provider.
fn configured_templates(providers: &IndexMap<String, ProviderConfig>) -> Vec<ProviderTemplate> {
    providers
        .iter()
        .map(|(key, entry)| {
            let default_model = entry
                .model
                .clone()
                .or_else(|| entry.models.keys().next().cloned());
            let mut provider = entry.clone();
            provider.model = default_model;
            // The template's provider carries credentials only; the model list
            // lives in `config_providers` and is read on form entry.
            provider.models = IndexMap::new();
            let model_count = entry.models.len().max(usize::from(entry.model.is_some()));
            let description = entry
                .base_url
                .clone()
                .unwrap_or_else(|| format!("{model_count} model(s)"));
            ProviderTemplate {
                name: key.clone(),
                description,
                section: ProviderSection::Configured,
                requires_api_key: true,
                provider,
                catalog_provider_id: Some(key.clone()),
            }
        })
        .collect()
}

fn provider_templates() -> Vec<ProviderTemplate> {
    vec![
        ProviderTemplate {
            name: "MiniMax Token Plan".to_string(),
            description: "MiniMax M1 long-context AI".to_string(),
            section: ProviderSection::Providers,
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
            section: ProviderSection::Providers,
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
            section: ProviderSection::Providers,
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
            section: ProviderSection::Providers,
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
        // Presets now live under the "Providers" section (Popular was repurposed
        // to "Configured" for the user's saved providers).
        assert!(content.contains("Providers"));
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
    fn paste_inserts_into_focused_field_stripping_control_chars() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek"); // focus = API key
        d.paste("sk-abc123\n"); // trailing newline must be stripped
        assert_eq!(d.field_value(ConnectField::ApiKey), "sk-abc123");

        // Paste appends at the end of the (preset-prefilled) base URL field.
        d.focus_field_for_test(ConnectField::BaseUrl);
        let before = d.field_value(ConnectField::BaseUrl);
        d.paste("/extra");
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            format!("{before}/extra")
        );

        // Digit-only fields keep only digits from the pasted text.
        d.focus_field_for_test(ConnectField::Context);
        d.paste("1,000\n");
        assert_eq!(d.field_value(ConnectField::Context), "1000");
    }

    #[test]
    fn paste_strips_zero_width_and_bom_characters() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek");
        // A browser-copied key can carry a leading BOM and assorted invisible /
        // default-ignorable chars (zero-width, soft hyphen, variation selector,
        // word joiner); all must be stripped so auth doesn't fail mysteriously.
        d.paste("\u{FEFF}sk-\u{200B}a\u{00AD}b\u{200D}c\u{FE0F}d\u{2060}");
        assert_eq!(d.field_value(ConnectField::ApiKey), "sk-abcd");
    }

    #[test]
    fn paste_in_provider_stage_filters_list() {
        let mut d = ConnectDialog::new();
        d.paste("ollama"); // narrows the filter to the Ollama row
        let action = d.confirm().expect("ollama selected via pasted filter");
        assert_eq!(action.name, "Ollama");
    }

    #[test]
    fn connect_action_carries_provider_group_and_override() {
        // DeepSeek preset has catalog_provider_id = Some("deepseek").
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
        assert_eq!(action.provider_key, "deepseek");
        assert_eq!(action.model_override.context_window, Some(1_000_000));
    }

    #[test]
    fn slugify_collapses_separators_and_guards_empty() {
        assert_eq!(slugify("MiniMax Token Plan"), "minimax-token-plan");
        assert_eq!(slugify("My  Provider!!"), "my-provider");
        assert_eq!(slugify("a/b.c"), "a-b-c");
        assert_eq!(slugify("!!!"), "custom");
        assert_eq!(slugify(""), "custom");
    }

    #[test]
    fn connect_action_group_key_slugs_preset_without_catalog_id() {
        // "MiniMax Token Plan" has no catalog_provider_id → slugged name.
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("MiniMax Token Plan");
        for c in "sk".chars() {
            d.input_char(c);
        }
        let action = d.confirm().expect("submit");
        assert_eq!(action.provider_key, "minimax-token-plan");
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
        d.input_price.clear();
        d.output_price.clear();
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
    fn confirm_persists_catalog_image_capability_into_model_override() {
        let cat = zode_core::Catalog::from_json(
            r#"{ "visionco": {
              "id":"visionco", "name":"VisionCo", "api":"https://vision.example/v1",
              "models": { "qwen-vl": {
                "id":"qwen-vl", "name":"Qwen VL", "attachment":false,
                "modalities":{"input":["text","image"],"output":["text"]}
              } }
            } }"#,
        )
        .expect("fixture parses");
        let mut dialog = ConnectDialog::with_catalog(&cat);
        dialog.select_to_api_key_for_test("VisionCo");
        for c in "sk-test".chars() {
            dialog.input_char(c);
        }

        let action = dialog.confirm().expect("submit vision provider");
        assert_eq!(action.provider.supports_images, Some(true));
        assert_eq!(action.model_override.supports_images, Some(true));
    }

    #[test]
    fn confirm_does_not_freeze_catalog_image_denial_as_an_override() {
        let cat = zode_core::Catalog::from_json(
            r#"{ "textco": {
              "id":"textco", "name":"TextCo", "api":"https://text.example/v1",
              "models": { "qwen-text": {
                "id":"qwen-text", "name":"Qwen Text",
                "modalities":{"input":["text"],"output":["text"]}
              } }
            } }"#,
        )
        .expect("fixture parses");
        let mut dialog = ConnectDialog::with_catalog(&cat);
        dialog.select_to_api_key_for_test("TextCo");
        for c in "sk-test".chars() {
            dialog.input_char(c);
        }

        let action = dialog.confirm().expect("submit text provider");
        assert_eq!(action.provider.supports_images, None);
        assert_eq!(action.model_override.supports_images, None);
    }

    #[test]
    fn configured_model_image_override_wins_over_catalog() {
        let cat = zode_core::Catalog::from_json(
            r#"{ "visionco": {
              "id":"visionco", "name":"VisionCo", "api":"https://vision.example/v1",
              "models": { "qwen-vl": {
                "id":"qwen-vl", "name":"Qwen VL",
                "modalities":{"input":["text","image"],"output":["text"]}
              } }
            } }"#,
        )
        .expect("fixture parses");
        let mut models = IndexMap::new();
        models.insert(
            "qwen-vl".to_string(),
            ModelOverride {
                supports_images: Some(false),
                ..Default::default()
            },
        );
        let mut providers = IndexMap::new();
        providers.insert(
            "visionco".to_string(),
            ProviderConfig {
                r#type: Some(ProviderKind::Openai),
                api_key: Some("sk-saved".into()),
                base_url: Some("https://vision.example/v1".into()),
                supports_images: Some(true),
                models,
                ..Default::default()
            },
        );
        let mut dialog = ConnectDialog::with_catalog_and_providers(&cat, &providers);
        dialog.select_to_api_key_for_test("visionco");

        let action = dialog.confirm().expect("submit configured provider");
        assert_eq!(action.provider.supports_images, Some(false));
        assert_eq!(action.model_override.supports_images, Some(false));
    }

    /// Build a catalog JSON string with `n` distinct providers so the picker
    /// list overflows a normal-sized modal viewport.
    fn many_provider_catalog(n: usize) -> zode_core::Catalog {
        let entries: Vec<String> = (0..n)
            .map(|i| {
                format!(
                    r#""zprov{i:02}": {{ "id": "zprov{i:02}", "name": "ZProvider{i:02}", "api": "https://api.z{i:02}.example/v1", "models": {{ "m{i:02}": {{ "id": "m{i:02}", "name": "M{i:02}" }} }} }}"#
                )
            })
            .collect();
        let json = format!("{{ {} }}", entries.join(","));
        zode_core::Catalog::from_json(&json).expect("generated fixture parses")
    }

    #[test]
    fn provider_list_scrolls_selected_into_view() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let cat = many_provider_catalog(20);
        let mut d = ConnectDialog::with_catalog(&cat);
        // Select the LAST provider (prev() from index 0 wraps to the end).
        d.prev();
        let last_name = d.providers.last().unwrap().name.clone();
        assert!(last_name.starts_with("ZProvider"), "last is a catalog row");

        let backend = ratatui::backend::TestBackend::new(100, 34);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| d.render(f, f.area(), &theme)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        // The selected (last) provider must be scrolled into view…
        assert!(
            content.contains(&last_name),
            "selected provider {last_name:?} should scroll into view"
        );
        // …and the top of the list must have scrolled off-screen.
        assert!(
            !content.contains("MiniMax Token Plan"),
            "top of the list should scroll away when the last row is selected"
        );
    }

    #[test]
    fn focused_empty_field_shows_cursor_right_after_label() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek"); // focus defaults to API key (empty)

        let area = Rect::new(0, 0, 90, 30);
        let backend = ratatui::backend::TestBackend::new(90, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| d.render(f, area, &theme)).unwrap();

        let inner = inner_area(modal_area(area, 62, 16));
        let row_y = inner.y + 4; // API-key field row
                                 // Cursor block sits immediately after the "API key " label (8 cols).
        let cursor_x = inner.x + "API key ".len() as u16;
        let cur = terminal.backend().buffer().cell((cursor_x, row_y)).unwrap();
        assert_eq!(
            cur.bg, theme.accent,
            "empty focused field should show the cursor block right after the label"
        );
        // The cursor must NOT float at the far-right edge of the row.
        let far = terminal
            .backend()
            .buffer()
            .cell((inner.x + inner.width - 1, row_y))
            .unwrap();
        assert_ne!(far.bg, theme.accent, "cursor must not sit at the far right");
    }

    #[test]
    fn focused_field_cursor_follows_typed_text() {
        let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("minimal"));
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek");
        for c in "abc".chars() {
            d.input_char(c); // API key is masked → "***"
        }

        let area = Rect::new(0, 0, 90, 30);
        let backend = ratatui::backend::TestBackend::new(90, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| d.render(f, area, &theme)).unwrap();

        let inner = inner_area(modal_area(area, 62, 16));
        let row_y = inner.y + 4;
        // Cursor block sits right after "API key " (8) + "***" (3) = 11 cols.
        let cursor_x = inner.x + "API key ".len() as u16 + 3;
        let cur = terminal.backend().buffer().cell((cursor_x, row_y)).unwrap();
        assert_eq!(
            cur.bg, theme.accent,
            "cursor block should follow the typed (masked) text"
        );
    }

    #[test]
    fn ensure_protocol_suffix_appends_for_bare_host_keeps_suffixed() {
        assert_eq!(
            ensure_protocol_suffix("https://api.deepseek.com", ProviderKind::Openai),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            ensure_protocol_suffix("https://api.deepseek.com", ProviderKind::Anthropic),
            "https://api.deepseek.com/anthropic"
        );
        // URLs that already carry a protocol path are used as published.
        assert_eq!(
            ensure_protocol_suffix("https://api.minimax.io/anthropic/v1", ProviderKind::Openai),
            "https://api.minimax.io/anthropic/v1"
        );
        assert_eq!(
            ensure_protocol_suffix("https://openrouter.ai/api/v1", ProviderKind::Anthropic),
            "https://openrouter.ai/api/v1"
        );
    }

    #[test]
    fn entering_form_sources_base_url_from_models_dev() {
        // models.dev publishes the bare host for deepseek; the form should adopt
        // it (not the hardcoded preset URL) and append the protocol suffix.
        let json = r#"{ "deepseek": { "id": "deepseek", "name": "DeepSeek",
            "api": "https://gateway.example.com",
            "models": { "deepseek-v4-pro": { "id": "deepseek-v4-pro", "name": "x" } } } }"#;
        let cat = zode_core::Catalog::from_json(json).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek"); // preset kind = Openai
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            "https://gateway.example.com/v1"
        );
    }

    #[test]
    fn base_url_for_kind_unstrands_anthropic_v1_on_openai() {
        // A `.../anthropic/v1` URL must not be left stranded when toggling to
        // OpenAI — it collapses to `.../v1`.
        assert_eq!(
            base_url_for_kind("https://api.minimax.io/anthropic/v1", ProviderKind::Openai),
            Some("https://api.minimax.io/v1".to_string())
        );
        // Plain `.../anthropic` → `.../v1`; `.../v1` (already openai) → unchanged.
        assert_eq!(
            base_url_for_kind("https://api.deepseek.com/anthropic", ProviderKind::Openai),
            Some("https://api.deepseek.com/v1".to_string())
        );
        assert_eq!(
            base_url_for_kind("https://api.deepseek.com/v1", ProviderKind::Openai),
            None
        );
    }

    #[test]
    fn cycle_type_toggles_only_anthropic_and_openai() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek"); // preset kind = Openai
        d.focus_field_for_test(ConnectField::Type);
        let k0 = d.field_value(ConnectField::Type);
        d.cycle_type(true);
        let k1 = d.field_value(ConnectField::Type);
        d.cycle_type(true);
        let k2 = d.field_value(ConnectField::Type);
        // Never reaches Ollama, and toggles back after two steps.
        assert_ne!(k1, "Ollama");
        assert_ne!(k2, "Ollama");
        assert_ne!(k0, k1);
        assert_eq!(k0, k2);
    }

    #[test]
    fn cycle_type_rewrites_base_url_protocol_suffix() {
        let mut d = ConnectDialog::new();
        d.select_to_api_key_for_test("DeepSeek"); // base .../v1, kind Openai
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            "https://api.deepseek.com/v1"
        );
        d.focus_field_for_test(ConnectField::Type);
        d.cycle_type(true); // → Anthropic: base URL follows
        assert_eq!(d.field_value(ConnectField::Type), "Anthropic");
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            "https://api.deepseek.com/anthropic"
        );
        d.cycle_type(true); // → Openai: base URL follows back
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            "https://api.deepseek.com/v1"
        );
    }

    #[test]
    fn configured_provider_lists_under_configured_and_prefills_on_select() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut models = IndexMap::new();
        models.insert("deepseek-v4-pro".to_string(), ModelOverride::default());
        models.insert("my-custom".to_string(), ModelOverride::default());
        let mut providers = IndexMap::new();
        providers.insert(
            "deepseek".into(),
            ProviderConfig {
                r#type: Some(ProviderKind::Anthropic),
                api_key: Some("sk-saved".into()),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                models,
                ..Default::default()
            },
        );
        let mut d = ConnectDialog::with_catalog_and_providers(&cat, &providers);
        // The saved provider shows under the Configured section.
        let tpl = d
            .providers_for_test()
            .iter()
            .find(|p| p.name == "deepseek")
            .expect("configured provider present");
        assert_eq!(tpl.section, ProviderSection::Configured);

        // Selecting it pre-fills the saved key + base URL (no re-typing to add a
        // model) and offers the provider's configured models for selection.
        d.select_to_api_key_for_test("deepseek");
        assert_eq!(d.field_value(ConnectField::ApiKey), "sk-saved");
        assert_eq!(
            d.field_value(ConnectField::BaseUrl),
            "https://api.deepseek.com/anthropic"
        );
        let avail = d.available_models_for_test();
        assert!(avail.contains(&"deepseek-v4-pro".to_string()));
        assert!(avail.contains(&"my-custom".to_string()));
    }

    #[test]
    fn entering_form_prefills_cost_from_catalog() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek"); // deepseek-v4-pro: input 0.28, output 0.42
        assert_eq!(d.field_value(ConnectField::InputPrice), "0.28");
        assert_eq!(d.field_value(ConnectField::OutputPrice), "0.42");
        // Cycling to another model refills the cost from that model.
        d.focus_field_for_test(ConnectField::Model);
        d.cycle_model(true); // → deepseek-r2: input 0.14, output 0.28
        assert_eq!(d.field_value(ConnectField::Model), "deepseek-r2");
        assert_eq!(d.field_value(ConnectField::InputPrice), "0.14");
        assert_eq!(d.field_value(ConnectField::OutputPrice), "0.28");
    }

    #[test]
    fn entering_form_prefills_context_and_max_output_from_catalog() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek"); // preset model deepseek-v4-pro
                                                  // On form entry both limits are pre-filled from models.dev (not just one).
        assert_eq!(d.field_value(ConnectField::Context), "1000000");
        assert_eq!(d.field_value(ConnectField::MaxOutput), "8192");
        // The provider's catalog models are offered for selection.
        let models = d.available_models_for_test();
        assert!(models.contains(&"deepseek-r2".to_string()));
        assert!(models.contains(&"deepseek-v4-pro".to_string()));
    }

    #[test]
    fn cycle_model_keeps_user_edits_on_single_model_provider() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        // AnotherCo has exactly one catalog model ("fast-model").
        d.select_to_api_key_for_test("AnotherCo");
        d.focus_field_for_test(ConnectField::Context);
        for c in "55555".chars() {
            d.input_char(c);
        }
        // Cycling a single-model provider doesn't change the model, so it must
        // NOT wipe the user's typed context/max output.
        d.focus_field_for_test(ConnectField::Model);
        d.cycle_model(true);
        assert_eq!(d.field_value(ConnectField::Model), "fast-model");
        assert_eq!(d.field_value(ConnectField::Context), "55555");
    }

    #[test]
    fn cycle_model_picks_catalog_model_and_refills_limits() {
        let cat = zode_core::Catalog::from_json(CATALOG_FIXTURE).expect("fixture parses");
        let mut d = ConnectDialog::with_catalog(&cat);
        d.select_to_api_key_for_test("DeepSeek"); // model deepseek-v4-pro (index 1)
        d.focus_field_for_test(ConnectField::Model);
        // available_models is sorted ["deepseek-r2", "deepseek-v4-pro"]; forward
        // from index 1 wraps to index 0.
        d.cycle_model(true);
        assert_eq!(d.field_value(ConnectField::Model), "deepseek-r2");
        // Limits re-fill for the newly selected model.
        assert_eq!(d.field_value(ConnectField::Context), "131072");
        assert_eq!(d.field_value(ConnectField::MaxOutput), "16384");
        // Cycling back restores deepseek-v4-pro's limits.
        d.cycle_model(true);
        assert_eq!(d.field_value(ConnectField::Model), "deepseek-v4-pro");
        assert_eq!(d.field_value(ConnectField::Context), "1000000");
        assert_eq!(d.field_value(ConnectField::MaxOutput), "8192");
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
