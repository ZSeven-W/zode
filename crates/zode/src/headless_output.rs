//! Versioned headless wire types and a pure accumulator for runtime events.

use std::collections::{BTreeMap, HashMap};

use agent::cost::CostSnapshot;
use agent::stream::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zode_core::currency::Currency;
use zode_core::run_event::RunStatus;

pub const HEADLESS_RESULT_SCHEMA: &str = "zode.headless.result.v1";

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_PROVIDER: i32 = 10;
pub const EXIT_PERMISSION: i32 = 11;
pub const EXIT_LIMIT: i32 = 12;
pub const EXIT_INTERRUPTED: i32 = 13;
pub const EXIT_PARTIAL: i32 = 14;
/// Session targeting failed (`--resume`/`--session-id`/`--fork-session`) —
/// distinct from EXIT_PERMISSION so automation can tell them apart.
pub const EXIT_SESSION: i32 = 15;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessResult {
    pub schema: String,
    pub status: RunStatus,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub num_turns: u64,
    pub usage: UsageSummary,
    pub cost: CostSummary,
    pub tool_calls: Vec<ToolCallSummary>,
    pub errors: Vec<HeadlessError>,
    pub partial: bool,
}

impl HeadlessResult {
    pub fn startup_failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema: HEADLESS_RESULT_SCHEMA.into(),
            status: RunStatus::Failed,
            text: String::new(),
            stop_reason: None,
            session_id: String::new(),
            turn_id: String::new(),
            request_id: String::new(),
            num_turns: 0,
            usage: UsageSummary::default(),
            cost: CostSummary {
                currency: "USD".into(),
                amount: None,
                total_usd: None,
                price_complete: false,
            },
            tool_calls: Vec::new(),
            errors: vec![HeadlessError {
                code: code.into(),
                message: message.into(),
            }],
            partial: false,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match (self.status, self.partial) {
            // A limit or interrupt is by nature a partial run; the specific
            // status is more actionable than the generic partial code.
            (RunStatus::Interrupted, _) => EXIT_INTERRUPTED,
            (RunStatus::LimitReached, _) => EXIT_LIMIT,
            (_, true) => EXIT_PARTIAL,
            (RunStatus::Completed, false) => EXIT_SUCCESS,
            (RunStatus::Failed, false) => {
                if self
                    .errors
                    .iter()
                    .any(|error| error.code.contains("permission") || error.code.contains("denied"))
                {
                    EXIT_PERMISSION
                } else {
                    EXIT_PROVIDER
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_model: BTreeMap<String, ModelUsage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_usd: Option<f64>,
    pub price_complete: bool,
}

impl CostSummary {
    pub fn from_snapshot(snapshot: &CostSnapshot, currency_code: &str) -> Self {
        let complete = !snapshot.has_unknown_models();
        let currency = Currency::from_code(currency_code);
        let total_usd = complete.then(|| snapshot.total_usd());
        Self {
            currency: currency.code.to_string(),
            amount: total_usd.map(|usd| usd * currency.per_usd),
            total_usd,
            price_complete: complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallSummary {
    pub id: String,
    pub name: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadlessError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct HeadlessAccumulator {
    text: String,
    stop_reason: Option<String>,
    result_model: Option<String>,
    num_turns: u64,
    usage: UsageAccumulator,
    tools: Vec<ToolCallSummary>,
    tool_indexes: HashMap<String, usize>,
    errors: Vec<HeadlessError>,
}

impl HeadlessAccumulator {
    pub fn on_event(&mut self, model: &str, event: &Event) {
        match event {
            Event::TextDelta { delta } => self.text.push_str(delta),
            Event::ToolUse { id, name, input } => {
                let index = self.tools.len();
                self.tool_indexes.insert(id.clone(), index);
                self.tools.push(ToolCallSummary {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    ok: None,
                    output: None,
                });
            }
            Event::ToolResult { id, ok, output } => {
                if let Some(index) = self.tool_indexes.get(id).copied() {
                    let tool = &mut self.tools[index];
                    tool.ok = Some(*ok);
                    tool.output = Some(output.clone());
                }
                self.usage.observe_subagent(model, output);
            }
            Event::Usage {
                input_tokens,
                output_tokens,
                cache_read,
                cache_create,
            } => self.usage.observe_cumulative(
                model,
                ModelUsage {
                    input_tokens: u64::from(*input_tokens),
                    output_tokens: u64::from(*output_tokens),
                    cache_read_tokens: u64::from(*cache_read),
                    cache_write_tokens: u64::from(*cache_create),
                },
            ),
            Event::Result { data } => {
                self.stop_reason = data.stop_reason.clone();
                self.result_model = data.model.clone();
                self.num_turns = data
                    .metadata
                    .get("num_turns")
                    .or_else(|| data.metadata.get("turns"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
            }
            Event::Error { code, message } => self.errors.push(HeadlessError {
                code: code.clone(),
                message: message.clone(),
            }),
            Event::Thinking { .. } | Event::Notice { .. } | Event::Unknown => {}
            _ => {}
        }
    }

    pub fn push_stream_error(&mut self, message: String) {
        self.push_error("stream_error", message);
    }

    pub fn push_error(&mut self, code: impl Into<String>, message: impl Into<String>) {
        self.errors.push(HeadlessError {
            code: code.into(),
            message: message.into(),
        });
    }

    pub fn finish(
        self,
        ids: HeadlessIds,
        cost: CostSummary,
        partial: bool,
        interrupted: bool,
    ) -> HeadlessResult {
        // Shared precedence (interrupt > limit > failure) so the headless
        // JSON status never diverges from the journal/TUI for the same turn.
        let status = RunStatus::derive(
            interrupted,
            !self.errors.is_empty(),
            self.stop_reason.as_deref(),
        );
        let model = self.result_model.unwrap_or(ids.model);
        HeadlessResult {
            schema: HEADLESS_RESULT_SCHEMA.into(),
            status,
            text: self.text,
            stop_reason: self.stop_reason,
            session_id: ids.session_id,
            turn_id: ids.turn_id,
            request_id: ids.request_id,
            num_turns: self.num_turns.max(1),
            usage: self.usage.finish(&model),
            cost,
            tool_calls: self.tools,
            errors: self.errors,
            partial,
        }
    }
}

pub struct HeadlessIds {
    pub session_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub model: String,
}

#[derive(Debug, Default)]
struct UsageAccumulator {
    completed: BTreeMap<String, ModelUsage>,
    current: BTreeMap<String, ModelUsage>,
    subagent: BTreeMap<String, ModelUsage>,
}

impl UsageAccumulator {
    fn observe_cumulative(&mut self, model: &str, next: ModelUsage) {
        if let Some(previous) = self.current.get(model) {
            if !usage_progressed(previous, &next) {
                add_usage(
                    self.completed.entry(model.to_string()).or_default(),
                    previous,
                );
            }
        }
        self.current.insert(model.to_string(), next);
    }

    fn observe_subagent(&mut self, model: &str, output: &Value) {
        if output.get("agent_type").is_none() {
            return;
        }
        let usage = self.subagent.entry(model.to_string()).or_default();
        usage.input_tokens = usage.input_tokens.saturating_add(
            output
                .get("usage_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        usage.output_tokens = usage.output_tokens.saturating_add(
            output
                .get("usage_output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
    }

    fn finish(mut self, fallback_model: &str) -> UsageSummary {
        for (model, usage) in self.current {
            add_usage(self.completed.entry(model).or_default(), &usage);
        }
        for (model, usage) in self.subagent {
            add_usage(self.completed.entry(model).or_default(), &usage);
        }
        if self.completed.is_empty() {
            self.completed
                .insert(fallback_model.to_string(), ModelUsage::default());
        }
        let mut total = UsageSummary {
            by_model: self.completed,
            ..UsageSummary::default()
        };
        for usage in total.by_model.values() {
            total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
            total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
            total.cache_read_tokens = total
                .cache_read_tokens
                .saturating_add(usage.cache_read_tokens);
            total.cache_write_tokens = total
                .cache_write_tokens
                .saturating_add(usage.cache_write_tokens);
        }
        total
    }
}

fn usage_progressed(previous: &ModelUsage, next: &ModelUsage) -> bool {
    next.input_tokens >= previous.input_tokens
        && next.output_tokens >= previous.output_tokens
        && next.cache_read_tokens >= previous.cache_read_tokens
        && next.cache_write_tokens >= previous.cache_write_tokens
}

fn add_usage(target: &mut ModelUsage, delta: &ModelUsage) {
    target.input_tokens = target.input_tokens.saturating_add(delta.input_tokens);
    target.output_tokens = target.output_tokens.saturating_add(delta.output_tokens);
    target.cache_read_tokens = target
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    target.cache_write_tokens = target
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
}

#[cfg(test)]
mod tests {
    use agent::stream::ResultData;

    use super::*;

    fn cost() -> CostSummary {
        CostSummary {
            currency: "USD".into(),
            amount: Some(0.25),
            total_usd: Some(0.25),
            price_complete: true,
        }
    }

    fn ids() -> HeadlessIds {
        HeadlessIds {
            session_id: "s".into(),
            turn_id: "t".into(),
            request_id: "r".into(),
            model: "m".into(),
        }
    }

    #[test]
    fn accumulates_text_tools_result_and_cumulative_usage() {
        let mut accumulator = HeadlessAccumulator::default();
        accumulator.on_event(
            "m",
            &Event::TextDelta {
                delta: "hello".into(),
            },
        );
        accumulator.on_event(
            "m",
            &Event::ToolUse {
                id: "1".into(),
                name: "FileRead".into(),
                input: serde_json::json!({"path": "README.md"}),
            },
        );
        accumulator.on_event(
            "m",
            &Event::ToolResult {
                id: "1".into(),
                ok: true,
                output: serde_json::json!({"text": "ok"}),
            },
        );
        accumulator.on_event(
            "m",
            &Event::Usage {
                input_tokens: 100,
                output_tokens: 10,
                cache_read: 50,
                cache_create: 5,
            },
        );
        accumulator.on_event(
            "m",
            &Event::Usage {
                input_tokens: 120,
                output_tokens: 20,
                cache_read: 50,
                cache_create: 5,
            },
        );
        accumulator.on_event(
            "m",
            &Event::Result {
                data: ResultData {
                    stop_reason: Some("end_turn".into()),
                    model: Some("m".into()),
                    metadata: HashMap::from([("num_turns".into(), serde_json::json!(2))]),
                },
            },
        );
        let result = accumulator.finish(ids(), cost(), false, false);
        assert_eq!(result.text, "hello");
        assert_eq!(result.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(result.num_turns, 2);
        assert_eq!(result.usage.input_tokens, 120);
        assert_eq!(result.usage.output_tokens, 20);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].ok, Some(true));
        assert_eq!(result.exit_code(), EXIT_SUCCESS);
    }

    #[test]
    fn max_iterations_maps_to_limit_reached_and_exit_12() {
        let mut accumulator = HeadlessAccumulator::default();
        accumulator.on_event(
            "m",
            &Event::Result {
                data: ResultData {
                    stop_reason: Some("max_iterations".into()),
                    model: Some("m".into()),
                    metadata: HashMap::new(),
                },
            },
        );
        // The runtime follows the capped Result with a stream error; that
        // must not demote the limit status to a generic partial failure.
        accumulator.push_stream_error("QueryLoop hit max_iterations (2)".into());
        let result = accumulator.finish(ids(), cost(), true, false);
        assert_eq!(result.status, RunStatus::LimitReached);
        assert_eq!(result.exit_code(), EXIT_LIMIT);
    }

    #[test]
    fn user_interrupt_maps_to_exit_13() {
        let mut accumulator = HeadlessAccumulator::default();
        accumulator.on_event(
            "m",
            &Event::TextDelta {
                delta: "partial".into(),
            },
        );
        let result = accumulator.finish(ids(), cost(), true, true);
        assert_eq!(result.status, RunStatus::Interrupted);
        assert_eq!(result.exit_code(), EXIT_INTERRUPTED);
    }

    #[test]
    fn stream_failure_is_partial_and_has_stable_exit_code() {
        let mut accumulator = HeadlessAccumulator::default();
        accumulator.on_event(
            "m",
            &Event::TextDelta {
                delta: "partial".into(),
            },
        );
        accumulator.push_stream_error("socket closed".into());
        let result = accumulator.finish(ids(), cost(), true, false);
        assert_eq!(result.status, RunStatus::Failed);
        assert!(result.partial);
        assert_eq!(result.exit_code(), EXIT_PARTIAL);
    }

    #[test]
    fn unknown_model_cost_is_null_not_zero() {
        let snapshot = CostSnapshot {
            total_nd: 0,
            per_model: BTreeMap::new(),
            unknown_models: BTreeMap::from([(
                "m".into(),
                agent::cost::UnknownModelTokens {
                    input_tokens: 1,
                    ..Default::default()
                },
            )]),
        };
        let summary = CostSummary::from_snapshot(&snapshot, "USD");
        assert_eq!(summary.amount, None);
        assert!(!summary.price_complete);
    }
}
