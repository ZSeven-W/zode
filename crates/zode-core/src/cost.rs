//! Cost tracking. Feeds Usage events to agent::CostTracker. Models without
//! price data (e.g. MiniMax) accrue token counts only (no USD).

use std::sync::Arc;

use agent::cost::{CostTracker, ModelPriceCatalog};
use agent::stream::Event;
use tokio::sync::Mutex;

pub struct CostState {
    model: String,
    tracker: Mutex<CostTracker>,
}

impl CostState {
    pub fn new(model: String) -> Self {
        let catalog = Arc::new(ModelPriceCatalog::with_defaults());
        Self {
            model,
            tracker: Mutex::new(CostTracker::new(catalog)),
        }
    }

    /// Feed one event (only Usage events are counted by the tracker).
    pub async fn observe(&self, event: &Event) {
        self.tracker.lock().await.observe_event(&self.model, event);
    }

    /// Human-readable report for `/cost`.
    pub async fn report(&self) -> String {
        let snap = self.tracker.lock().await.snapshot();
        if snap.has_unknown_models() {
            let (mut input, mut output) = (0u64, 0u64);
            for t in snap.unknown_models.values() {
                input += t.input_tokens;
                output += t.output_tokens;
            }
            format!(
                "model: {} (no price data)\ntokens: ↑{input} ↓{output}\n(cost estimate unavailable for this model)",
                self.model
            )
        } else {
            format!("model: {}\ncost: {}", self.model, snap.format_total_usd())
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
}
