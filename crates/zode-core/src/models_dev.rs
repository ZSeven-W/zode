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
    /// { context: u32, output: u32 } — both integers in the live API.
    #[serde(default)]
    limit: Option<RawLimit>,
    /// { input, output, cache_read?, cache_write? } — f64 $/MTok.
    #[serde(default)]
    cost: Option<RawCost>,
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
        Self::from_disk_cache()
            .or_else(|| Self::from_json(BUNDLED).ok())
            .unwrap_or_else(|| Self {
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
    }
}
