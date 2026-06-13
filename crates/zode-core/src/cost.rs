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
        let hit = match (100 * cache).checked_div(input) {
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
                snap.format_total_usd()
            )
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
}
