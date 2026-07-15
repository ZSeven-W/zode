//! Read-only models.dev catalog: provider/model metadata (context + output
//! limits, pricing) used to pre-fill the connect dialog. 3-tier source:
//! bundled snapshot (always present) -> disk cache (~/.zode, ~24h TTL) ->
//! live fetch (https://models.dev/api.json). Parsing tolerates schema drift
//! and never panics: a bad tier falls back to the previous one.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::config::{ConfigManager, ProviderKind};

/// URL of the live models.dev API.
const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Disk-cache TTL: refresh once per day.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Committed snapshot — guarantees offline / first-run availability.
const BUNDLED: &str = include_str!("../assets/models-dev.json");

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Full provider + model catalog loaded from models.dev.
#[derive(Debug, Clone)]
pub struct Catalog {
    providers: Vec<CatalogProvider>,
}

/// A single provider entry (e.g. "anthropic", "openai", "deepseek").
#[derive(Debug, Clone)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    /// Base URL for OpenAI-compatible endpoints (from the `api` field).
    pub base_url: Option<String>,
    /// Inferred provider type for mapping to `ProviderKind`.
    pub kind: ProviderKind,
    /// Inferred dialect (e.g. "deepseek") for OpenAI-compat providers.
    pub dialect: Option<String>,
    pub models: Vec<CatalogModel>,
}

/// A single model entry within a provider.
#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    /// Whether the model accepts image input. `None` means the catalog did not
    /// publish enough modality metadata to decide, so callers should retain
    /// their provider default (or an explicit user override).
    pub supports_images: Option<bool>,
    /// Context window size in tokens. None if the catalog doesn't publish it.
    pub context: Option<u32>,
    /// Max output tokens. None if the catalog doesn't publish it.
    pub max_output: Option<u32>,
    /// Input price in USD per million tokens ($/MTok).
    pub input_price: Option<f64>,
    /// Output price in USD per million tokens ($/MTok).
    pub output_price: Option<f64>,
    /// Cache-read price in USD/MTok, if the model supports caching.
    pub cache_read_price: Option<f64>,
    /// Cache-write price in USD/MTok, if the model supports caching.
    pub cache_write_price: Option<f64>,
}

// ---------------------------------------------------------------------------
// Raw wire types (pinned to the real models.dev schema)
// ---------------------------------------------------------------------------

/// Top-level map: { <providerId> -> RawProvider }
type RawCatalog = BTreeMap<String, RawProvider>;

#[derive(Debug, Deserialize)]
struct RawProvider {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    /// Base URL for API calls (e.g. "https://api.deepseek.com").
    #[serde(default)]
    api: Option<String>,
    /// Model map: { <modelId> -> RawModel }
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
}

#[derive(Debug, Deserialize, Default)]
struct RawModel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    /// Older models.dev records expose only this coarse attachment bit, while
    /// newer records also publish explicit input modalities below.
    #[serde(default)]
    attachment: Option<bool>,
    #[serde(default)]
    modalities: Option<RawModalities>,
    /// { context: u32, output: u32 } — both integers in the live API.
    #[serde(default)]
    limit: Option<RawLimit>,
    /// { input, output, cache_read?, cache_write? } — f64 $/MTok.
    #[serde(default)]
    cost: Option<RawCost>,
}

#[derive(Debug, Deserialize, Default)]
struct RawModalities {
    #[serde(default)]
    input: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLimit {
    /// Context window tokens.
    #[serde(default)]
    context: Option<u32>,
    /// Max output tokens.
    #[serde(default)]
    output: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct RawCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
}

// ---------------------------------------------------------------------------
// Catalog implementation
// ---------------------------------------------------------------------------

impl Catalog {
    /// Parse a models.dev `api.json` body. Tolerates unknown fields and
    /// missing limit/cost blocks; returns an error only for invalid JSON.
    pub fn from_json(body: &str) -> Result<Self, serde_json::Error> {
        let raw: RawCatalog = serde_json::from_str(body)?;
        let providers = raw
            .into_iter()
            .map(|(key, rp)| {
                let id = if rp.id.is_empty() { key } else { rp.id };
                let api = rp.api.as_deref();
                let (kind, dialect) = infer_kind(&id, api);
                let base_url = rp.api.filter(|s| !s.is_empty());
                let models = rp.models.into_values().map(raw_model_to_catalog).collect();
                CatalogProvider {
                    id,
                    name: rp.name,
                    base_url,
                    kind,
                    dialect,
                    models,
                }
            })
            .collect();
        Ok(Self { providers })
    }

    /// Synchronous, network-free load. Source priority:
    ///
    ///   1. Fresh disk cache (if it exists and is within CACHE_TTL).
    ///   2. Bundled `include_str!` snapshot (always present).
    ///   3. Empty catalog (defensive fallback — should never be reached in
    ///      practice because the bundled snapshot is tested at CI time).
    ///
    /// Never panics; never touches the network.
    pub fn load_blocking() -> Self {
        if let Some(mut cached) = Self::from_disk_cache() {
            // Caches written by Zode versions before image-capability parsing
            // deliberately kept only limits/prices. Detect that legacy shape
            // without parsing the large bundled snapshot on every normal
            // launch; new/raw caches have at least one known capability.
            let has_capability_metadata = cached
                .providers
                .iter()
                .flat_map(|provider| provider.models.iter())
                .any(|model| model.supports_images.is_some());
            if !has_capability_metadata {
                // Enrich a still-fresh legacy cache so an upgrade fixes vision
                // routing immediately instead of waiting up to CACHE_TTL for
                // the next network refresh.
                if let Ok(bundled) = Self::from_json(BUNDLED) {
                    cached.fill_missing_image_capabilities(&bundled);
                }
            }
            return cached;
        }

        Self::from_json(BUNDLED).unwrap_or_else(|_| Self {
            providers: Vec::new(),
        })
    }

    /// Best-effort live refresh: fetches `https://models.dev/api.json`,
    /// writes the result to the disk cache, and returns `Some(Catalog)`.
    /// Returns `None` on any error (network, parse, or I/O); never panics.
    ///
    /// Safe to call from both async and sync contexts:
    /// - Inside a tokio multi-thread runtime: uses `block_in_place`.
    /// - Outside any runtime: builds a temporary single-thread runtime.
    pub fn refresh_blocking() -> Option<Self> {
        let body = Self::block_on_fetch().ok()?;
        let catalog = Self::from_json(&body).ok()?;
        // Best-effort cache write; ignore errors.
        let _ = Self::write_disk_cache(&catalog);
        Some(catalog)
    }

    /// Ordered list of providers.
    pub fn providers(&self) -> &[CatalogProvider] {
        &self.providers
    }

    /// Look up a specific model by provider id and model id.
    pub fn find_model(&self, provider_id: &str, model_id: &str) -> Option<&CatalogModel> {
        self.providers
            .iter()
            .find(|p| p.id == provider_id)?
            .models
            .iter()
            .find(|m| m.id == model_id)
    }

    /// The published context window for `model_id`, searching across all
    /// providers. A fallback for the effective context window when the config
    /// doesn't pin a per-model `contextWindow` — so each model's real max
    /// context (which differs per model) drives compaction + the status %.
    pub fn context_for_model(&self, model_id: &str) -> Option<u32> {
        self.providers
            .iter()
            .flat_map(|p| p.models.iter())
            .find(|m| m.id == model_id)
            .and_then(|m| m.context)
    }

    /// The published context window for `model_id`, preferring the entry under
    /// `provider_id` when given. A model id can appear under several providers
    /// with differing windows (e.g. a direct provider's 200K vs an aggregator's
    /// 1M); scoping to the active provider picks the right one. Falls back to
    /// the first global match when the provider is unknown or no hint is given.
    pub fn context_for_model_scoped(
        &self,
        provider_id: Option<&str>,
        model_id: &str,
    ) -> Option<u32> {
        provider_id
            .and_then(|pid| self.find_model(pid, model_id))
            .and_then(|m| m.context)
            .or_else(|| self.context_for_model(model_id))
    }

    /// The published max output tokens for `model_id`, searching all providers.
    /// Lets the engine auto-resolve a model's real output cap instead of making
    /// the user pin `maxOutputTokens` by hand.
    pub fn max_output_for_model(&self, model_id: &str) -> Option<u32> {
        self.providers
            .iter()
            .flat_map(|p| p.models.iter())
            .find(|m| m.id == model_id)
            .and_then(|m| m.max_output)
    }

    /// The published max output tokens for `model_id`, preferring the entry
    /// under `provider_id` when given (same provider-scoping rationale as
    /// [`Self::context_for_model_scoped`]).
    pub fn max_output_for_model_scoped(
        &self,
        provider_id: Option<&str>,
        model_id: &str,
    ) -> Option<u32> {
        provider_id
            .and_then(|pid| self.find_model(pid, model_id))
            .and_then(|m| m.max_output)
            .or_else(|| self.max_output_for_model(model_id))
    }

    /// Published image-input support for `model_id`, searching all providers.
    /// Unknown metadata remains `None` so an explicit provider default is not
    /// accidentally turned into a denial.
    pub fn supports_images_for_model(&self, model_id: &str) -> Option<bool> {
        let mut matches = self
            .providers
            .iter()
            .flat_map(|p| p.models.iter())
            .filter(|m| m.id == model_id);
        let first = matches.next()?.supports_images?;
        // A model id can be reused by aggregators whose endpoint support
        // differs. Global fallback is safe only when every catalog match both
        // publishes a capability and agrees; otherwise require an explicit
        // supportsImages override.
        matches
            .all(|model| model.supports_images == Some(first))
            .then_some(first)
    }

    /// Provider-scoped form of [`Self::supports_images_for_model`]. A custom
    /// configured provider name may not equal a models.dev id, so a missing
    /// scoped match falls back to the globally matching model id.
    pub fn supports_images_for_model_scoped(
        &self,
        provider_id: Option<&str>,
        model_id: &str,
    ) -> Option<bool> {
        if let Some(model) = provider_id.and_then(|pid| self.find_model(pid, model_id)) {
            // A known endpoint with unknown metadata must stay unknown. A
            // same-named model on another endpoint is not evidence that this
            // endpoint accepts images.
            return model.supports_images;
        }
        self.supports_images_for_model(model_id)
    }

    fn fill_missing_image_capabilities(&mut self, fallback: &Self) {
        for provider in &mut self.providers {
            for model in &mut provider.models {
                if model.supports_images.is_none() {
                    model.supports_images = match fallback.find_model(&provider.id, &model.id) {
                        // Preserve an exact endpoint's unknown capability;
                        // do not borrow a claim from a different endpoint.
                        Some(exact) => exact.supports_images,
                        None => fallback.supports_images_for_model(&model.id),
                    };
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Disk cache helpers
    // -----------------------------------------------------------------------

    fn cache_path() -> Option<PathBuf> {
        ConfigManager::config_dir()
            .ok()
            .map(|d| d.join("models-dev-cache.json"))
    }

    /// Load the disk cache if it exists and is younger than CACHE_TTL.
    fn from_disk_cache() -> Option<Self> {
        Self::from_disk_cache_inner().ok()
    }

    fn from_disk_cache_inner() -> Result<Self, Box<dyn std::error::Error>> {
        let path = Self::cache_path().ok_or("no cache path")?;
        let meta = std::fs::metadata(&path)?;
        let age = SystemTime::now().duration_since(meta.modified()?)?;
        if age > CACHE_TTL {
            return Err("cache expired".into());
        }
        let body = std::fs::read_to_string(&path)?;
        Ok(Self::from_json(&body)?)
    }

    /// Write a freshly-fetched catalog body to the disk cache.
    fn write_disk_cache(catalog: &Self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::cache_path().ok_or("no cache path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Re-serialize via the raw structs is awkward; just re-fetch the
        // JSON we already have. Instead, store the parsed catalog as JSON.
        // Actually — we don't keep the raw bytes. Serialize CatalogProvider
        // back to a minimal JSON shape.
        let body = serialize_catalog_to_json(catalog)?;
        std::fs::write(path, body)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Async-safe bridge for live fetch
    // -----------------------------------------------------------------------

    /// Bridge async→sync in a way that is safe both inside and outside a
    /// tokio runtime:
    /// - Inside a multi-thread runtime: uses `block_in_place` (does NOT try
    ///   to create a new runtime, which would panic).
    /// - Outside any runtime: builds a temporary single-thread runtime.
    fn block_on_fetch() -> Result<String, Box<dyn std::error::Error>> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fetch_body_async())),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(fetch_body_async())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn raw_model_to_catalog(m: RawModel) -> CatalogModel {
    // Explicit modalities are more precise than the legacy attachment bit:
    // an attachment-capable text model can accept files without accepting
    // image content. Fall back to `attachment` only when modalities are absent.
    let supports_images = m.modalities.map_or(m.attachment, |modalities| {
        Some(
            modalities
                .input
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case("image")),
        )
    });
    let (context, max_output) = m
        .limit
        .map(|l| (l.context, l.output))
        .unwrap_or((None, None));
    let (input_price, output_price, cache_read_price, cache_write_price) = m
        .cost
        .map(|c| (c.input, c.output, c.cache_read, c.cache_write))
        .unwrap_or((None, None, None, None));
    CatalogModel {
        id: m.id,
        name: m.name,
        supports_images,
        context,
        max_output,
        input_price,
        output_price,
        cache_read_price,
        cache_write_price,
    }
}

/// Infer the `ProviderKind` (and optional dialect) from the provider id
/// and `api` URL.  Called during `from_json` so the inference is stable.
fn infer_kind(id: &str, api: Option<&str>) -> (ProviderKind, Option<String>) {
    let haystack = format!("{} {}", id, api.unwrap_or("")).to_lowercase();
    if id == "anthropic" || haystack.contains("/anthropic/") || haystack.contains("anthropic.com") {
        (ProviderKind::Anthropic, None)
    } else if id == "ollama" || haystack.contains("ollama") {
        (ProviderKind::Ollama, None)
    } else {
        // OpenAI-compatible: pick a dialect from known provider ids.
        let dialect = match id {
            "deepseek" => Some("deepseek".into()),
            "moonshot" => Some("moonshot".into()),
            "openrouter" => Some("openrouter".into()),
            _ => None,
        };
        (ProviderKind::Openai, dialect)
    }
}

/// Minimal JSON serialisation of a `Catalog` for the disk cache.
/// Uses serde_json::Value to avoid a separate Serialize derive on the public
/// structs (which would expose internal layout to callers).
fn serialize_catalog_to_json(catalog: &Catalog) -> Result<String, serde_json::Error> {
    use serde_json::{Map, Value};

    let mut top = Map::new();
    for p in &catalog.providers {
        let mut models_map = Map::new();
        for m in &p.models {
            let mut model_obj = Map::new();
            model_obj.insert("id".into(), Value::String(m.id.clone()));
            model_obj.insert("name".into(), Value::String(m.name.clone()));
            if let Some(supports_images) = m.supports_images {
                // The cache only needs the normalized capability. Store it in
                // a models.dev-compatible field so the regular parser can read
                // it back without a second cache schema.
                model_obj.insert("attachment".into(), Value::Bool(supports_images));
            }
            if m.context.is_some() || m.max_output.is_some() {
                let mut limit = Map::new();
                if let Some(c) = m.context {
                    limit.insert("context".into(), Value::Number(c.into()));
                }
                if let Some(o) = m.max_output {
                    limit.insert("output".into(), Value::Number(o.into()));
                }
                model_obj.insert("limit".into(), Value::Object(limit));
            }
            if m.input_price.is_some()
                || m.output_price.is_some()
                || m.cache_read_price.is_some()
                || m.cache_write_price.is_some()
            {
                let mut cost = Map::new();
                let push_f64 = |map: &mut Map<String, Value>, key: &str, v: Option<f64>| {
                    if let Some(f) = v {
                        if let Some(n) = serde_json::Number::from_f64(f) {
                            map.insert(key.into(), Value::Number(n));
                        }
                    }
                };
                push_f64(&mut cost, "input", m.input_price);
                push_f64(&mut cost, "output", m.output_price);
                push_f64(&mut cost, "cache_read", m.cache_read_price);
                push_f64(&mut cost, "cache_write", m.cache_write_price);
                model_obj.insert("cost".into(), Value::Object(cost));
            }
            models_map.insert(m.id.clone(), Value::Object(model_obj));
        }
        let mut provider_obj = Map::new();
        provider_obj.insert("id".into(), Value::String(p.id.clone()));
        provider_obj.insert("name".into(), Value::String(p.name.clone()));
        if let Some(ref u) = p.base_url {
            provider_obj.insert("api".into(), Value::String(u.clone()));
        }
        provider_obj.insert("models".into(), Value::Object(models_map));
        top.insert(p.id.clone(), Value::Object(provider_obj));
    }
    serde_json::to_string(&Value::Object(top))
}

/// Async helper: fetch the raw `api.json` body from models.dev (5s timeout).
async fn fetch_body_async() -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let body = client.get(MODELS_DEV_URL).send().await?.text().await?;
    Ok(body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal fixture matching the real models.dev schema shape.
    const FIXTURE: &str = r#"{
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
          }
        }
      }
    }"#;

    #[test]
    fn parses_provider_and_model() {
        let cat = Catalog::from_json(FIXTURE).expect("parse");
        let p = cat.providers().iter().find(|p| p.id == "deepseek").unwrap();
        assert_eq!(p.name, "DeepSeek");
        let m = cat.find_model("deepseek", "deepseek-v4-pro").unwrap();
        assert_eq!(m.context, Some(1_000_000));
        assert_eq!(m.max_output, Some(8192));
        assert_eq!(m.input_price, Some(0.28));
        assert_eq!(m.output_price, Some(0.42));
    }

    #[test]
    fn parses_image_support_from_modalities_and_legacy_attachment() {
        let json = r#"{
          "p": { "id":"p", "name":"P", "models": {
            "vision": {
              "id":"vision", "name":"Vision", "attachment":false,
              "modalities":{"input":["text","image"],"output":["text"]}
            },
            "files-only": {
              "id":"files-only", "name":"Files", "attachment":true,
              "modalities":{"input":["text"],"output":["text"]}
            },
            "legacy-vision": {
              "id":"legacy-vision", "name":"Legacy", "attachment":true
            },
            "unknown": { "id":"unknown", "name":"Unknown" }
          } }
        }"#;
        let cat = Catalog::from_json(json).expect("parse image metadata");

        assert_eq!(
            cat.find_model("p", "vision").unwrap().supports_images,
            Some(true)
        );
        assert_eq!(
            cat.find_model("p", "files-only").unwrap().supports_images,
            Some(false),
            "explicit modalities must win over the coarse attachment bit"
        );
        assert_eq!(
            cat.find_model("p", "legacy-vision")
                .unwrap()
                .supports_images,
            Some(true)
        );
        assert_eq!(
            cat.find_model("p", "unknown").unwrap().supports_images,
            None
        );
    }

    #[test]
    fn image_support_lookup_scopes_then_falls_back_by_model() {
        let cat = Catalog::from_json(
            r#"{
              "alpha": { "id":"alpha", "name":"Alpha", "models": {
                "shared": { "id":"shared", "name":"Shared", "attachment":false }
              } },
              "beta": { "id":"beta", "name":"Beta", "models": {
                "shared": { "id":"shared", "name":"Shared", "attachment":true },
                "vision": { "id":"vision", "name":"Vision", "attachment":true }
              } }
            }"#,
        )
        .expect("parse fixture");

        assert_eq!(
            cat.supports_images_for_model_scoped(Some("alpha"), "shared"),
            Some(false)
        );
        assert_eq!(
            cat.supports_images_for_model_scoped(Some("beta"), "shared"),
            Some(true)
        );
        assert_eq!(
            cat.supports_images_for_model_scoped(Some("custom-name"), "vision"),
            Some(true),
            "custom provider names should still benefit from model metadata"
        );
        assert_eq!(
            cat.supports_images_for_model_scoped(Some("custom-name"), "shared"),
            None,
            "a conflicting global model id must not create a false positive"
        );
    }

    #[test]
    fn scoped_unknown_image_support_does_not_borrow_from_another_endpoint() {
        let cat = Catalog::from_json(
            r#"{
              "alpha": { "id":"alpha", "name":"Alpha", "models": {
                "shared": { "id":"shared", "name":"Shared" }
              } },
              "beta": { "id":"beta", "name":"Beta", "models": {
                "shared": { "id":"shared", "name":"Shared", "attachment":true }
              } }
            }"#,
        )
        .expect("parse fixture");

        assert_eq!(
            cat.supports_images_for_model_scoped(Some("alpha"), "shared"),
            None,
            "an exact endpoint match with unknown metadata must remain unknown"
        );
        assert_eq!(
            cat.supports_images_for_model_scoped(Some("custom-name"), "shared"),
            None,
            "a provider alias must not borrow capability when any same-id endpoint is unknown"
        );
    }

    #[test]
    fn cache_roundtrip_preserves_normalized_image_capability() {
        let cat = Catalog::from_json(
            r#"{ "p": { "id":"p", "name":"P", "models": {
              "v": { "id":"v", "name":"V",
                "modalities":{"input":["text","image"],"output":["text"]} }
            } } }"#,
        )
        .expect("parse fixture");
        let cached = serialize_catalog_to_json(&cat).expect("serialize cache");
        let reparsed = Catalog::from_json(&cached).expect("parse cache");
        assert_eq!(
            reparsed.find_model("p", "v").unwrap().supports_images,
            Some(true)
        );
    }

    #[test]
    fn legacy_cache_can_be_enriched_from_bundled_metadata() {
        let mut legacy = Catalog::from_json(
            r#"{ "p": { "id":"p", "name":"P", "models": {
              "v": { "id":"v", "name":"V" }
            } } }"#,
        )
        .expect("parse legacy cache");
        let fallback = Catalog::from_json(
            r#"{ "p": { "id":"p", "name":"P", "models": {
              "v": { "id":"v", "name":"V", "attachment":true }
            } } }"#,
        )
        .expect("parse fallback");

        legacy.fill_missing_image_capabilities(&fallback);
        assert_eq!(
            legacy.find_model("p", "v").unwrap().supports_images,
            Some(true)
        );
    }

    #[test]
    fn legacy_cache_enrichment_preserves_exact_endpoint_unknown() {
        let mut legacy = Catalog::from_json(
            r#"{ "alpha": { "id":"alpha", "name":"Alpha", "models": {
              "shared": { "id":"shared", "name":"Shared" }
            } } }"#,
        )
        .expect("parse legacy cache");
        let fallback = Catalog::from_json(
            r#"{
              "alpha": { "id":"alpha", "name":"Alpha", "models": {
                "shared": { "id":"shared", "name":"Shared" }
              } },
              "beta": { "id":"beta", "name":"Beta", "models": {
                "shared": { "id":"shared", "name":"Shared", "attachment":true }
              } }
            }"#,
        )
        .expect("parse fallback");

        legacy.fill_missing_image_capabilities(&fallback);
        assert_eq!(
            legacy
                .find_model("alpha", "shared")
                .unwrap()
                .supports_images,
            None,
            "cache enrichment must not borrow image support from another endpoint"
        );
    }

    #[test]
    fn context_for_model_scoped_prefers_active_provider() {
        // The same model id under two providers with DIFFERENT context windows
        // (e.g. a direct provider vs an aggregator), plus a unique model on one.
        let json = r#"{
          "alpha": { "id":"alpha","name":"Alpha","models": {
            "dup": { "id":"dup","name":"Dup","limit": { "context": 200000 } } } },
          "beta": { "id":"beta","name":"Beta","models": {
            "dup":  { "id":"dup","name":"Dup","limit": { "context": 1000000 } },
            "solo": { "id":"solo","name":"Solo","limit": { "context": 500000 } } } }
        }"#;
        let cat = Catalog::from_json(json).expect("parse");
        // A provider hint scopes to that provider's value for the shared id.
        assert_eq!(
            cat.context_for_model_scoped(Some("beta"), "dup"),
            Some(1_000_000)
        );
        assert_eq!(
            cat.context_for_model_scoped(Some("alpha"), "dup"),
            Some(200_000)
        );
        // No hint → falls back to the global first match (order-independent here
        // only in that it must return one of the two configured windows).
        assert!(cat.context_for_model_scoped(None, "dup").is_some());
        // An unknown provider hint also falls back to the global search, which
        // still resolves a uniquely-named model deterministically.
        assert_eq!(
            cat.context_for_model_scoped(Some("nope"), "solo"),
            Some(500_000)
        );
        assert_eq!(cat.context_for_model_scoped(None, "solo"), Some(500_000));
        // Unknown model → None regardless of hint.
        assert_eq!(cat.context_for_model_scoped(Some("alpha"), "ghost"), None);
    }

    #[test]
    fn unknown_fields_and_missing_limits_tolerated() {
        let json = r#"{ "x": { "id":"x","name":"X","surprise":1,
          "models": { "m": { "id":"m","name":"M" } } } }"#;
        let cat = Catalog::from_json(json).expect("parse");
        let m = cat.find_model("x", "m").unwrap();
        assert_eq!(m.context, None); // no limit block -> None, no panic
        assert_eq!(m.input_price, None);
    }

    #[test]
    fn bundled_snapshot_parses() {
        // The committed snapshot must always parse.
        let cat = Catalog::from_json(BUNDLED).expect("bundled snapshot parses");
        assert!(!cat.providers().is_empty());
        assert_eq!(
            cat.supports_images_for_model("qwen3-vl-plus"),
            Some(true),
            "Qwen VL must be recognized from modalities even where attachment=false"
        );
    }

    #[test]
    fn infer_kind_anthropic() {
        let (kind, dialect) = infer_kind("anthropic", None);
        assert_eq!(kind, ProviderKind::Anthropic);
        assert!(dialect.is_none());
    }

    #[test]
    fn infer_kind_deepseek_gets_dialect() {
        let (kind, dialect) = infer_kind("deepseek", Some("https://api.deepseek.com"));
        assert_eq!(kind, ProviderKind::Openai);
        assert_eq!(dialect.as_deref(), Some("deepseek"));
    }

    #[test]
    fn infer_kind_unknown_defaults_to_openai() {
        let (kind, dialect) = infer_kind("some-router", Some("https://some.router/v1"));
        assert_eq!(kind, ProviderKind::Openai);
        assert!(dialect.is_none());
    }

    #[test]
    fn catalog_roundtrips_through_cache_serializer() {
        let cat = Catalog::from_json(FIXTURE).unwrap();
        let json = serialize_catalog_to_json(&cat).unwrap();
        let cat2 = Catalog::from_json(&json).unwrap();
        assert_eq!(cat2.providers().len(), cat.providers().len());
        let m2 = cat2.find_model("deepseek", "deepseek-v4-pro").unwrap();
        assert_eq!(m2.context, Some(1_000_000));
        assert_eq!(m2.input_price, Some(0.28));
    }

    #[test]
    fn find_model_returns_none_for_unknown() {
        let cat = Catalog::from_json(FIXTURE).unwrap();
        assert!(cat.find_model("deepseek", "no-such-model").is_none());
        assert!(cat.find_model("no-provider", "x").is_none());
    }

    #[test]
    fn load_blocking_returns_bundled_when_no_cache() {
        // load_blocking must always yield at least the bundled catalog (which
        // has providers). A fresh disk cache would also satisfy the assertion.
        // This test does NOT touch the network.
        let cat = Catalog::load_blocking();
        assert!(
            !cat.providers().is_empty(),
            "load_blocking should return the bundled catalog (non-empty providers)"
        );
        assert_eq!(
            cat.supports_images_for_model("qwen3-vl-plus"),
            Some(true),
            "a fresh legacy disk cache must be enriched before it can mask bundled vision metadata"
        );
    }
}
