//! In-memory registry of `Task`-spawned sub-agents and their live activity.
//!
//! A `SubAgentRegistry` is held by a `ZodeEngine` (one per tab) and written by
//! `ZodeTaskObserver` (a `TaskObserver` the Task tool drives). The TUI reads a
//! `snapshot()` to render the sub-agent overlay. Cheap `Arc` clone, like
//! `TodoState`.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

use agent::stream::Event;
use agent_tools_code::task::TaskObserver;
use serde_json::Value;

const DEFAULT_CAP: usize = 50;
const SUMMARY_MAX: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubAgentStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentLine {
    Text(String),
    Thinking(String),
    ToolUse { name: String, input: String },
    ToolResult { ok: bool, summary: String },
    Error(String),
    Notice(String),
}

#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: u64,
    pub agent_type: String,
    pub description: Option<String>,
    pub depth: usize,
    pub status: SubAgentStatus,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub transcript: Vec<SubAgentLine>,
}

#[derive(Debug)]
struct State {
    agents: Vec<SubAgent>,
    cap: usize,
}

/// Shared sub-agent registry. Clone is cheap (`Arc`).
#[derive(Debug, Clone)]
pub struct SubAgentRegistry {
    inner: Arc<Mutex<State>>,
    next_id: Arc<AtomicU64>,
}

impl Default for SubAgentRegistry {
    fn default() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }
}

impl SubAgentRegistry {
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                agents: Vec::new(),
                cap: cap.max(1),
            })),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Snapshot for the sync TUI render. Cheap Vec clone.
    pub fn snapshot(&self) -> Vec<SubAgent> {
        self.inner
            .lock()
            .map(|s| s.agents.clone())
            .unwrap_or_default()
    }

    /// A `TaskObserver` that writes into this registry.
    pub fn observer(&self) -> Arc<dyn TaskObserver> {
        Arc::new(ZodeTaskObserver { reg: self.clone() })
    }

    fn start(&self, agent_type: &str, description: Option<&str>, depth: usize) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut s) = self.inner.lock() {
            s.agents.push(SubAgent {
                id,
                agent_type: agent_type.to_string(),
                description: description.map(str::to_string),
                depth,
                status: SubAgentStatus::Running,
                started_at: now_secs(),
                finished_at: None,
                input_tokens: 0,
                output_tokens: 0,
                transcript: Vec::new(),
            });
            let cap = s.cap;
            while s.agents.len() > cap {
                // Prefer evicting the oldest FINISHED agent so a running
                // sub-agent's live transcript is never dropped mid-flight.
                let victim = s
                    .agents
                    .iter()
                    .position(|a| a.status != SubAgentStatus::Running)
                    .unwrap_or(0);
                s.agents.remove(victim);
            }
        }
        id
    }

    fn push_event(&self, id: u64, event: &Event) {
        if let Ok(mut s) = self.inner.lock() {
            if let Some(a) = s.agents.iter_mut().find(|a| a.id == id) {
                apply_event(a, event);
            }
        }
    }

    fn finish(&self, id: u64, result: &str, error: Option<&str>) {
        if let Ok(mut s) = self.inner.lock() {
            if let Some(a) = s.agents.iter_mut().find(|a| a.id == id) {
                a.finished_at = Some(now_secs());
                if let Some(err) = error {
                    a.status = SubAgentStatus::Failed;
                    a.transcript.push(SubAgentLine::Error(err.to_string()));
                } else {
                    a.status = SubAgentStatus::Done;
                    if a.transcript.is_empty() && !result.is_empty() {
                        a.transcript.push(SubAgentLine::Text(result.to_string()));
                    }
                }
            }
        }
    }
}

/// Apply one streamed event to a sub-agent, coalescing consecutive text /
/// thinking deltas into a single trailing line.
fn apply_event(a: &mut SubAgent, event: &Event) {
    match event {
        Event::TextDelta { delta } => match a.transcript.last_mut() {
            Some(SubAgentLine::Text(t)) => t.push_str(delta),
            _ => a.transcript.push(SubAgentLine::Text(delta.clone())),
        },
        Event::Thinking { delta } => match a.transcript.last_mut() {
            Some(SubAgentLine::Thinking(t)) => t.push_str(delta),
            _ => a.transcript.push(SubAgentLine::Thinking(delta.clone())),
        },
        Event::ToolUse { name, input, .. } => a.transcript.push(SubAgentLine::ToolUse {
            name: name.clone(),
            input: summarize(input),
        }),
        Event::ToolResult { ok, output, .. } => a.transcript.push(SubAgentLine::ToolResult {
            ok: *ok,
            summary: summarize(output),
        }),
        Event::Usage {
            input_tokens,
            output_tokens,
            ..
        } => {
            // Cumulative frames — take the max, not the sum.
            a.input_tokens = a.input_tokens.max(*input_tokens);
            a.output_tokens = a.output_tokens.max(*output_tokens);
        }
        Event::Error { message, .. } => a.transcript.push(SubAgentLine::Error(message.clone())),
        Event::Notice { message, .. } => a.transcript.push(SubAgentLine::Notice(message.clone())),
        // Result totals are handled at finish; Unknown is forward-compat noise.
        _ => {}
    }
}

/// Compact a JSON value to a single truncated line for the transcript.
fn summarize(v: &Value) -> String {
    let raw = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let one_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > SUMMARY_MAX {
        let mut t: String = one_line.chars().take(SUMMARY_MAX - 1).collect();
        t.push('…');
        t
    } else {
        one_line
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug)]
struct ZodeTaskObserver {
    reg: SubAgentRegistry,
}

impl TaskObserver for ZodeTaskObserver {
    fn on_start(&self, agent_type: &str, description: Option<&str>, depth: usize) -> u64 {
        self.reg.start(agent_type, description, depth)
    }
    fn on_event(&self, id: u64, event: &Event) {
        self.reg.push_event(id, event);
    }
    fn on_finish(&self, id: u64, result: &str, error: Option<&str>) {
        self.reg.finish(id, result, error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::stream::Event;

    #[test]
    fn lifecycle_start_event_finish_builds_subagent() {
        let reg = SubAgentRegistry::new();
        let obs = reg.observer();
        let id = obs.on_start("researcher", Some("dig"), 0);
        obs.on_event(
            id,
            &Event::TextDelta {
                delta: "hello ".into(),
            },
        );
        obs.on_event(
            id,
            &Event::TextDelta {
                delta: "world".into(),
            },
        );
        obs.on_event(
            id,
            &Event::ToolUse {
                id: "t1".into(),
                name: "Read".into(),
                input: serde_json::json!({"path": "a.rs"}),
            },
        );
        obs.on_event(
            id,
            &Event::Usage {
                input_tokens: 30,
                output_tokens: 12,
                cache_read: 0,
                cache_create: 0,
            },
        );
        obs.on_finish(id, "done", None);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 1);
        let a = &snap[0];
        assert_eq!(a.agent_type, "researcher");
        assert_eq!(a.description.as_deref(), Some("dig"));
        assert_eq!(a.status, SubAgentStatus::Done);
        assert_eq!(a.input_tokens, 30);
        assert_eq!(a.output_tokens, 12);
        // Text deltas coalesce into ONE Text line.
        assert_eq!(a.transcript[0], SubAgentLine::Text("hello world".into()));
        assert!(matches!(&a.transcript[1], SubAgentLine::ToolUse { name, .. } if name == "Read"));
    }

    #[test]
    fn error_finish_marks_failed() {
        let reg = SubAgentRegistry::new();
        let obs = reg.observer();
        let id = obs.on_start("planner", None, 1);
        obs.on_finish(id, "", Some("boom"));
        let snap = reg.snapshot();
        assert_eq!(snap[0].status, SubAgentStatus::Failed);
        assert_eq!(snap[0].depth, 1);
        assert_eq!(
            snap[0].transcript.last(),
            Some(&SubAgentLine::Error("boom".into()))
        );
    }

    #[test]
    fn usage_takes_max_not_sum() {
        let reg = SubAgentRegistry::new();
        let obs = reg.observer();
        let id = obs.on_start("r", None, 0);
        obs.on_event(
            id,
            &Event::Usage {
                input_tokens: 10,
                output_tokens: 4,
                cache_read: 0,
                cache_create: 0,
            },
        );
        obs.on_event(
            id,
            &Event::Usage {
                input_tokens: 25,
                output_tokens: 9,
                cache_read: 0,
                cache_create: 0,
            },
        );
        let snap = reg.snapshot();
        assert_eq!((snap[0].input_tokens, snap[0].output_tokens), (25, 9));
    }

    #[test]
    fn registry_caps_retained_subagents() {
        let reg = SubAgentRegistry::with_cap(2);
        let obs = reg.observer();
        for i in 0..3 {
            let id = obs.on_start(&format!("a{i}"), None, 0);
            obs.on_finish(id, "ok", None);
        }
        let snap = reg.snapshot();
        assert_eq!(snap.len(), 2);
        // Oldest evicted; newest retained.
        assert_eq!(
            snap.iter()
                .map(|a| a.agent_type.clone())
                .collect::<Vec<_>>(),
            vec!["a1", "a2"]
        );
    }

    #[test]
    fn cap_prefers_evicting_finished_over_running() {
        let reg = SubAgentRegistry::with_cap(2);
        let obs = reg.observer();
        // Two running agents fill the cap.
        let r1 = obs.on_start("run1", None, 0);
        let _r2 = obs.on_start("run2", None, 0);
        // Mark r1 finished so it is the preferred eviction victim.
        obs.on_finish(r1, "done", None);
        // A third start must evict the oldest FINISHED agent (r1), not the
        // still-running run2.
        let _r3 = obs.on_start("run3", None, 0);
        let snap = reg.snapshot();
        assert_eq!(snap.len(), 2);
        // The finished run1 was evicted; the still-running run2 + new run3 remain.
        let names: Vec<String> = snap.iter().map(|a| a.agent_type.clone()).collect();
        assert_eq!(names, vec!["run2".to_string(), "run3".to_string()]);
    }
}
