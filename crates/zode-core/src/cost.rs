//! Cost tracking. Feeds Usage events to agent::CostTracker. Models without
//! price data (e.g. MiniMax) accrue token counts only (no USD).

use std::sync::Arc;

use agent::cost::{CostTracker, ModelPriceCatalog};
use agent::stream::Event;
use tokio::sync::Mutex;

use crate::currency::Currency;

pub struct CostState {
    model: String,
    tracker: Mutex<CostTracker>,
    /// Display currency; the USD total is converted for `/cost` + the sidebar.
    /// Behind a lock so `/currency` can switch it in place (no engine rebuild).
    currency: std::sync::RwLock<Currency>,
}

impl CostState {
    /// Default catalog (Anthropic/OpenAI built-ins), USD display.
    pub fn new(model: String) -> Self {
        Self::new_with(model, ModelPriceCatalog::with_defaults(), "USD")
    }

    /// Build with a (possibly price-overridden) catalog and a display currency.
    /// The engine seeds the catalog with the active provider's configured
    /// prices so models the built-ins don't know (e.g. DeepSeek) still get a
    /// cost instead of "n/a".
    pub fn new_with(model: String, catalog: ModelPriceCatalog, currency_code: &str) -> Self {
        Self {
            model,
            tracker: Mutex::new(CostTracker::new(Arc::new(catalog))),
            currency: std::sync::RwLock::new(Currency::from_code(currency_code)),
        }
    }

    /// Switch the display currency at runtime (`/currency`). Returns the applied
    /// ISO code (falls back to USD for an unknown code). Cost is re-converted on
    /// the next `report`/`sidebar_label` — no engine rebuild needed.
    pub fn set_currency(&self, code: &str) -> &'static str {
        let c = Currency::from_code(code);
        *self.currency.write().expect("currency lock") = c;
        c.code
    }

    /// The current display-currency ISO code.
    pub fn currency_code(&self) -> &'static str {
        self.currency.read().expect("currency lock").code
    }

    fn currency(&self) -> Currency {
        *self.currency.read().expect("currency lock")
    }

    /// Feed one event. Usage events are counted by the tracker directly; Task
    /// tool results carry the sub-agent's token usage (`usage_input_tokens` /
    /// `usage_output_tokens`, alongside `agent_type`) which the child consumed
    /// internally and never emitted to the parent stream — fold those in too so
    /// `/cost` reflects sub-agent calls.
    pub async fn observe(&self, event: &Event) {
        let mut tracker = self.tracker.lock().await;
        tracker.observe_event(&self.model, event);
        if let Event::ToolResult { output, .. } = event {
            // `agent_type` is unique to the Task tool's result shape — gate on
            // it so an MCP tool that happens to use those field names can't be
            // mistaken for a sub-agent.
            if output.get("agent_type").is_some() {
                let ci = output
                    .get("usage_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let co = output
                    .get("usage_output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if ci > 0 || co > 0 {
                    // The Task result already carries the child turn's TOTAL
                    // tokens (not cumulative frames), so add them directly via
                    // `observe` — NOT `observe_event`, whose cumulative-delta
                    // baseline is keyed by model and would be corrupted by an
                    // out-of-band absolute count for the parent's model.
                    tracker.observe(&self.model, ci, co, 0, 0);
                }
            }
        }
    }

    /// Human-readable report for `/cost`. Includes prompt-cache hits when the
    /// provider reports them (cache_read), and a hit-rate over input tokens.
    pub async fn report(&self) -> String {
        let snap = self.tracker.lock().await.snapshot();
        let (mut input, mut output, mut cache) = (0u64, 0u64, 0u64);
        for t in snap.unknown_models.values() {
            input += t.input_tokens;
            output += t.output_tokens;
            cache += t.cache_read_tokens;
        }
        for t in snap.per_model.values() {
            input += t.input_tokens;
            output += t.output_tokens;
            cache += t.cache_read_tokens;
        }
        // input_tokens is the non-cached (full-rate) prompt; cache is the
        // cached read. Hit rate is over the TOTAL prompt = input + cache.
        let hit = match (100 * cache).checked_div(input + cache) {
            Some(pct) => format!(" cache:{cache} ({pct}%)"),
            None => String::new(),
        };
        if snap.has_unknown_models() {
            format!(
                "model: {} (no price data)\ntokens: ↑{input} ↓{output}{hit}\n(cost estimate unavailable for this model)",
                self.model
            )
        } else {
            format!(
                "model: {}\ntokens: ↑{input} ↓{output}{hit}\ncost: {}",
                self.model,
                self.currency().format(snap.total_usd())
            )
        }
    }

    /// Short cost label for compact UI surfaces. Unknown priced models return
    /// `n/a` once they have usage so the UI does not imply a zero bill.
    pub async fn sidebar_label(&self) -> String {
        let snap = self.tracker.lock().await.snapshot();
        if snap.has_unknown_models() {
            "n/a".to_string()
        } else {
            self.currency().format(snap.total_usd())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::stream::Event;

    fn usage(i: u32, o: u32) -> Event {
        Event::Usage {
            input_tokens: i,
            output_tokens: o,
            cache_read: 0,
            cache_create: 0,
        }
    }

    #[tokio::test]
    async fn unknown_model_reports_tokens_not_usd() {
        // MiniMax is not in the default catalog -> token-only report.
        let cost = CostState::new("MiniMax-M1".into());
        cost.observe(&usage(100, 50)).await;
        let report = cost.report().await;
        assert!(report.to_lowercase().contains("token"), "{report}");
        assert!(report.contains("100"));
        assert!(report.contains("50"));
    }

    #[tokio::test]
    async fn priced_override_yields_real_cost_not_na() {
        use agent::cost::{ModelPriceCatalog, ModelPrices};
        // A model the built-ins don't know, with config-supplied prices.
        let mut catalog = ModelPriceCatalog::with_defaults();
        catalog.insert(
            "deepseek-v4-pro",
            ModelPrices::from_usd_per_mtok(0.3, 1.2, 0.0, 0.0),
        );
        let cost = CostState::new_with("deepseek-v4-pro".into(), catalog, "USD");
        cost.observe(&usage(1_000_000, 1_000_000)).await; // 1M in + 1M out
        let report = cost.report().await;
        // 1M*$0.30/MTok + 1M*$1.20/MTok = $1.50.
        assert!(report.contains("cost: $1.50"), "{report}");
        assert!(!report.contains("n/a"), "{report}");
        assert!(!report.contains("no price data"), "{report}");
    }

    #[tokio::test]
    async fn cost_converts_to_configured_currency() {
        use agent::cost::{ModelPriceCatalog, ModelPrices};
        let mut catalog = ModelPriceCatalog::with_defaults();
        catalog.insert(
            "deepseek-v4-pro",
            ModelPrices::from_usd_per_mtok(0.3, 1.2, 0.0, 0.0),
        );
        let cost = CostState::new_with("deepseek-v4-pro".into(), catalog, "CNY");
        cost.observe(&usage(1_000_000, 1_000_000)).await;
        let report = cost.report().await;
        // $1.50 * 7.2 = ¥10.80.
        assert!(report.contains("¥10.80"), "{report}");
    }

    #[tokio::test]
    async fn set_currency_switches_display_in_place() {
        use agent::cost::{ModelPriceCatalog, ModelPrices};
        let mut catalog = ModelPriceCatalog::with_defaults();
        catalog.insert(
            "deepseek-v4-pro",
            ModelPrices::from_usd_per_mtok(0.3, 1.2, 0.0, 0.0),
        );
        let cost = CostState::new_with("deepseek-v4-pro".into(), catalog, "USD");
        cost.observe(&usage(1_000_000, 1_000_000)).await; // $1.50
        assert!(cost.report().await.contains("$1.50"));

        // Switch to CNY at runtime → re-converted, no rebuild.
        assert_eq!(cost.set_currency("cny"), "CNY");
        assert_eq!(cost.currency_code(), "CNY");
        let report = cost.report().await;
        assert!(report.contains("¥10.80"), "{report}"); // 1.50 * 7.2

        // Unknown code falls back to USD.
        assert_eq!(cost.set_currency("XYZ"), "USD");
        assert!(cost.report().await.contains("$1.50"));
    }

    #[tokio::test]
    async fn folds_subagent_usage_from_task_result() {
        let cost = CostState::new("MiniMax-M1".into());
        // A Task tool result carries the child's usage; it must be counted.
        let task_result = Event::ToolResult {
            id: "tu_1".into(),
            ok: true,
            output: serde_json::json!({
                "output": "done",
                "agent_type": "researcher",
                "usage_input_tokens": 200,
                "usage_output_tokens": 80,
            }),
        };
        cost.observe(&task_result).await;
        let report = cost.report().await;
        assert!(report.contains("200"), "{report}");
        assert!(report.contains("80"), "{report}");
    }

    #[tokio::test]
    async fn non_task_tool_result_is_not_counted() {
        let cost = CostState::new("MiniMax-M1".into());
        // A normal tool result (no agent_type) must not add tokens.
        let other = Event::ToolResult {
            id: "tu_2".into(),
            ok: true,
            output: serde_json::json!({"text": "ok", "usage_input_tokens": 999}),
        };
        cost.observe(&other).await;
        let report = cost.report().await;
        assert!(!report.contains("999"), "{report}");
    }

    #[tokio::test]
    async fn known_model_reports_usd() {
        // A model in the default catalog (Anthropic defaults) -> USD report.
        let cost = CostState::new("claude-3-5-sonnet-20241022".into());
        cost.observe(&usage(1000, 1000)).await;
        let report = cost.report().await;
        // Either a USD figure or — if this model id isn't in the catalog —
        // the token fallback; both are valid. Assert it doesn't panic and
        // names the model.
        assert!(report.contains("claude-3-5-sonnet-20241022"));
    }

    #[tokio::test]
    async fn sidebar_label_formats_known_model_usd() {
        let cost = CostState::new("gpt-4o-mini".into());
        cost.observe(&usage(1000, 1000)).await;
        assert_eq!(cost.sidebar_label().await, "$0.0008");
    }

    #[tokio::test]
    async fn sidebar_label_marks_unknown_model_unavailable() {
        let cost = CostState::new("model-not-in-catalog".into());
        cost.observe(&usage(1000, 1000)).await;
        assert_eq!(cost.sidebar_label().await, "n/a");
    }
}
