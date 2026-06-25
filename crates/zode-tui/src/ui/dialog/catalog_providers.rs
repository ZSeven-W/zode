//! Helpers for building `ProviderTemplate` lists from the models.dev catalog.
//! Kept separate from `connect.rs` to stay within the 800-line file budget.

use zode_core::config::{ProviderConfig, ProviderKind};
use zode_core::{Catalog, CatalogProvider};

use super::connect::{ProviderSection, ProviderTemplate};

/// Build the full provider list from the catalog: the 7 hardcoded presets in
/// the Popular section, followed by catalog providers (deduped by id) in the
/// Providers section.
pub(crate) fn build_provider_templates(
    hardcoded: Vec<ProviderTemplate>,
    cat: &Catalog,
) -> Vec<ProviderTemplate> {
    // Derive the excluded-id set dynamically from the hardcoded templates so
    // that any preset with a catalog_provider_id is deduped — no static list
    // needed (which was the source of the moonshot/openrouter/ollama bug).
    // Collect as owned Strings so the borrow on `hardcoded` ends before we move it.
    let excluded: std::collections::HashSet<String> = hardcoded
        .iter()
        .filter_map(|t| t.catalog_provider_id.clone())
        .collect();

    let mut templates = hardcoded;

    for cp in cat.providers() {
        // Skip providers already represented in hardcoded templates.
        if excluded.contains(&cp.id) {
            continue;
        }
        // Skip providers without a name (degenerate catalog entry).
        if cp.name.is_empty() {
            continue;
        }
        templates.push(catalog_provider_to_template(cp));
    }

    templates
}

/// Convert a single `CatalogProvider` into a `ProviderTemplate` for the
/// Providers section. Uses the first model (alphabetically by id) as the
/// default model field pre-fill.
fn catalog_provider_to_template(cp: &CatalogProvider) -> ProviderTemplate {
    // Pick the first model as a sensible default.
    let first_model = cp.models.first().map(|m| m.id.as_str()).unwrap_or("");
    let provider = ProviderConfig {
        r#type: Some(cp.kind),
        api_key: None,
        base_url: cp.base_url.clone(),
        model: if first_model.is_empty() {
            None
        } else {
            Some(first_model.to_string())
        },
        dialect: cp.dialect.clone(),
        ..Default::default()
    };
    // Ollama-style providers (localhost) don't need an API key.
    let requires_api_key = cp.kind != ProviderKind::Ollama;
    ProviderTemplate {
        name: cp.name.clone(),
        description: cp.base_url.clone().unwrap_or_default(),
        section: ProviderSection::Providers,
        requires_api_key,
        provider,
        catalog_provider_id: Some(cp.id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::connect::ConnectDialog;

    /// Fixture with openrouter, moonshot, and ollama — all three ids that the
    /// old static POPULAR_IDS omitted, causing them to appear twice.
    const DEDUP_FIXTURE: &str = r#"{
      "openrouter": {
        "id": "openrouter",
        "name": "OpenRouter",
        "api": "https://openrouter.ai/api/v1",
        "models": {
          "openai/gpt-4.1": { "id": "openai/gpt-4.1", "name": "GPT-4.1" }
        }
      },
      "moonshot": {
        "id": "moonshot",
        "name": "Moonshot",
        "api": "https://api.moonshot.ai/v1",
        "models": {
          "kimi-k2-0711-preview": { "id": "kimi-k2-0711-preview", "name": "Kimi K2" }
        }
      },
      "ollama": {
        "id": "ollama",
        "name": "Ollama",
        "api": "http://localhost:11434",
        "models": {
          "llama3.1": { "id": "llama3.1", "name": "Llama 3.1" }
        }
      },
      "newco": {
        "id": "newco",
        "name": "NewCo",
        "api": "https://api.newco.ai/v1",
        "models": {
          "new-model": { "id": "new-model", "name": "New Model" }
        }
      }
    }"#;

    #[test]
    fn hardcoded_presets_are_not_duplicated_by_catalog() {
        let cat = zode_core::Catalog::from_json(DEDUP_FIXTURE).expect("fixture parses");
        let d = ConnectDialog::with_catalog(&cat);

        // Each of the hardcoded providers that has a catalog_provider_id must
        // appear EXACTLY ONCE across the whole provider list.
        for id in &["openrouter", "moonshot", "ollama"] {
            let count = d
                .providers_for_test()
                .iter()
                .filter(|p| p.catalog_provider_id.as_deref() == Some(id))
                .count();
            assert_eq!(
                count, 1,
                "provider id {id:?} should appear exactly once, found {count}"
            );
        }

        // "newco" is not in the hardcoded list, so it must appear once in Providers.
        let newco_count = d
            .providers_for_test()
            .iter()
            .filter(|p| p.catalog_provider_id.as_deref() == Some("newco"))
            .count();
        assert_eq!(newco_count, 1, "newco should appear exactly once");
    }
}
