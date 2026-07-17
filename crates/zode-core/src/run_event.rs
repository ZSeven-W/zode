//! Stable, product-level run events shared by the headless (`-p`), REPL, TUI,
//! and ACP surfaces via [`TurnRecorder`], plus session journals and telemetry.
//! (The app-server crate keeps its own protocol accumulator.) The vendored
//! runtime's `Event` is an implementation detail; this envelope is the Zode
//! compatibility boundary.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::stream::{Event, ResultData};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RUN_EVENT_SCHEMA: &str = "zode.run-event.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventEnvelope {
    pub schema: String,
    pub seq: u64,
    pub timestamp_ms: u64,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub event: RunEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum RunEvent {
    #[serde(rename = "run.started")]
    RunStarted,
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "message.delta")]
    MessageDelta { delta: String },
    #[serde(rename = "thinking.delta")]
    ThinkingDelta { delta: String },
    #[serde(rename = "tool.started")]
    ToolStarted {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool.completed")]
    ToolCompleted { id: String, ok: bool, output: Value },
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    },
    #[serde(rename = "result")]
    Result {
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        metadata: HashMap<String, Value>,
    },
    #[serde(rename = "error")]
    Error { code: String, message: String },
    #[serde(rename = "notice")]
    Notice { code: String, message: String },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        status: RunStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        partial: bool,
    },
    #[serde(rename = "run.completed")]
    RunCompleted {
        status: RunStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        partial: bool,
    },
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Completed,
    Failed,
    Interrupted,
    LimitReached,
}

impl RunEvent {
    pub fn from_agent(event: &Event) -> Self {
        match event {
            Event::TextDelta { delta } => Self::MessageDelta {
                delta: delta.clone(),
            },
            Event::Thinking { delta } => Self::ThinkingDelta {
                delta: delta.clone(),
            },
            Event::ToolUse { id, name, input } => Self::ToolStarted {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            Event::ToolResult { id, ok, output } => Self::ToolCompleted {
                id: id.clone(),
                ok: *ok,
                output: output.clone(),
            },
            Event::Usage {
                input_tokens,
                output_tokens,
                cache_read,
                cache_create,
            } => Self::Usage {
                input_tokens: u64::from(*input_tokens),
                output_tokens: u64::from(*output_tokens),
                cache_read_tokens: u64::from(*cache_read),
                cache_write_tokens: u64::from(*cache_create),
            },
            Event::Result { data } => Self::from_result(data),
            Event::Error { code, message } => Self::Error {
                code: code.clone(),
                message: message.clone(),
            },
            Event::Notice { code, message } => Self::Notice {
                code: code.clone(),
                message: message.clone(),
            },
            Event::Unknown => Self::Unknown,
            _ => Self::Unknown,
        }
    }

    fn from_result(data: &ResultData) -> Self {
        Self::Result {
            stop_reason: data.stop_reason.clone(),
            model: data.model.clone(),
            metadata: data.metadata.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunEventContext {
    session_id: String,
    turn_id: Option<String>,
    request_id: Option<String>,
    next_seq: u64,
}

impl RunEventContext {
    pub fn new(
        session_id: impl Into<String>,
        turn_id: Option<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id,
            request_id,
            next_seq: 0,
        }
    }

    pub fn envelope(&mut self, event: RunEvent) -> RunEventEnvelope {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let envelope = RunEventEnvelope {
            schema: RUN_EVENT_SCHEMA.to_string(),
            seq,
            timestamp_ms: now_ms(),
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            request_id: self.request_id.clone(),
            event,
        };
        crate::telemetry::emit(&envelope);
        envelope
    }
}

impl RunStatus {
    /// Truthful status for a finished turn, shared by every surface so the
    /// same failure never journals differently per entrypoint. Precedence:
    /// interrupt > limit (the runtime pairs a limit Result with a stream
    /// error, which must not demote it to a generic failure) > failure.
    pub fn derive(interrupted: bool, failed: bool, stop_reason: Option<&str>) -> Self {
        if interrupted {
            Self::Interrupted
        } else if matches!(stop_reason, Some("max_iterations" | "max_turn_requests")) {
            Self::LimitReached
        } else if failed {
            Self::Failed
        } else {
            Self::Completed
        }
    }
}

/// Final disposition of a turn, emitted as the TurnCompleted/RunCompleted pair.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub status: RunStatus,
    pub stop_reason: Option<String>,
    pub partial: bool,
}

impl TurnOutcome {
    pub fn completed() -> Self {
        Self {
            status: RunStatus::Completed,
            stop_reason: None,
            partial: false,
        }
    }
}

/// One turn's lifecycle recorder, shared by the TUI, headless `-p`, the REPL,
/// and ACP: envelopes + journals every run event, pairs the turn with its
/// checkpoint (finish when the turn ran, cancel when it never started — a
/// turn that failed to start must not persist a phantom completed
/// checkpoint), and emits the completion pair exactly once.
pub struct TurnRecorder {
    store: Option<crate::sessions::SessionStore>,
    context: RunEventContext,
    completed: bool,
}

impl TurnRecorder {
    pub fn new(store: Option<crate::sessions::SessionStore>, context: RunEventContext) -> Self {
        Self {
            store,
            context,
            completed: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.context.session_id
    }

    /// Emit the RunStarted/TurnStarted pair.
    pub fn start(&mut self) -> Vec<RunEventEnvelope> {
        vec![
            self.record(RunEvent::RunStarted),
            self.record(RunEvent::TurnStarted),
        ]
    }

    /// Envelope + journal one event (the store skips per-token deltas).
    pub fn record(&mut self, event: RunEvent) -> RunEventEnvelope {
        let envelope = self.context.envelope(event);
        if let Some(store) = &self.store {
            if let Err(error) = store.append_run_event(&envelope.session_id, &envelope) {
                tracing::warn!("session event journal failed: {error}");
            }
        }
        envelope
    }

    pub fn record_agent(&mut self, event: &Event) -> RunEventEnvelope {
        self.record(RunEvent::from_agent(event))
    }

    /// Open the turn's checkpoint. No-op without a session store.
    pub fn begin_checkpoint(
        &self,
        tracker: &crate::sessions::checkpoint::CheckpointTracker,
        cwd: std::path::PathBuf,
        message_count: usize,
    ) -> Result<(), crate::CoreError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        tracker.begin(
            store.session_dir(&self.context.session_id)?,
            self.context.session_id.clone(),
            self.context.turn_id.clone().unwrap_or_default(),
            cwd,
            message_count,
        )?;
        Ok(())
    }

    /// Close the turn: finish the checkpoint when the turn actually ran,
    /// cancel it otherwise, then emit TurnCompleted/RunCompleted. Idempotent —
    /// only the first call emits.
    pub fn complete(
        &mut self,
        tracker: Option<&crate::sessions::checkpoint::CheckpointTracker>,
        turn_ran: bool,
        outcome: &TurnOutcome,
    ) -> Vec<RunEventEnvelope> {
        if self.completed {
            return Vec::new();
        }
        self.completed = true;
        if let Some(tracker) = tracker {
            if turn_ran {
                if let Err(error) = tracker.finish() {
                    tracing::warn!("checkpoint finish failed: {error}");
                }
            } else {
                tracker.cancel();
            }
        }
        vec![
            self.record(RunEvent::TurnCompleted {
                status: outcome.status,
                stop_reason: outcome.stop_reason.clone(),
                partial: outcome.partial,
            }),
            self.record(RunEvent::RunCompleted {
                status: outcome.status,
                stop_reason: outcome.stop_reason.clone(),
                partial: outcome.partial,
            }),
        ]
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_and_serializes_runtime_events_with_stable_wire_names() {
        let mut context = RunEventContext::new("s1", Some("t1".into()), Some("r1".into()));
        let envelope = context.envelope(RunEvent::from_agent(&Event::ToolUse {
            id: "tool-1".into(),
            name: "FileRead".into(),
            input: serde_json::json!({"path": "README.md"}),
        }));
        let json = serde_json::to_value(envelope).unwrap();
        assert_eq!(json["schema"], RUN_EVENT_SCHEMA);
        assert_eq!(json["seq"], 0);
        assert_eq!(json["sessionId"], "s1");
        assert_eq!(json["turnId"], "t1");
        assert_eq!(json["requestId"], "r1");
        assert_eq!(json["type"], "tool.started");
        assert_eq!(json["name"], "FileRead");
        assert_eq!(json["input"]["path"], "README.md");
    }

    #[test]
    fn sequence_is_monotonic_within_a_context() {
        let mut context = RunEventContext::new("s", None, None);
        let first = context.envelope(RunEvent::RunStarted);
        let second = context.envelope(RunEvent::TurnStarted);
        assert_eq!((first.seq, second.seq), (0, 1));
        assert!(first.timestamp_ms > 0);
    }

    #[test]
    fn result_preserves_stop_reason_model_and_metadata() {
        let event = Event::Result {
            data: ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("model-a".into()),
                metadata: HashMap::from([("turns".into(), serde_json::json!(3))]),
            },
        };
        assert_eq!(
            RunEvent::from_agent(&event),
            RunEvent::Result {
                stop_reason: Some("end_turn".into()),
                model: Some("model-a".into()),
                metadata: HashMap::from([("turns".into(), serde_json::json!(3))]),
            }
        );
    }

    fn recorder_fixture(config: &std::path::Path) -> TurnRecorder {
        let store = crate::sessions::SessionStore::at(config.join("sessions"));
        store
            .create(crate::sessions::DurableSessionMeta::new(
                crate::session_meta::SessionMeta {
                    id: "rec".into(),
                    title: "t".into(),
                    cwd: config.display().to_string(),
                    model: "m".into(),
                    updated_at: 1,
                },
            ))
            .unwrap();
        TurnRecorder::new(
            Some(store),
            RunEventContext::new("rec", Some("turn-1".into()), None),
        )
    }

    #[test]
    #[serial_test::serial]
    fn recorder_cancels_the_checkpoint_when_the_turn_never_ran() {
        let config = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", config.path());
        let mut recorder = recorder_fixture(config.path());
        let tracker = crate::sessions::checkpoint::CheckpointTracker::default();
        recorder
            .begin_checkpoint(&tracker, config.path().to_path_buf(), 0)
            .unwrap();
        recorder.complete(
            Some(&tracker),
            false,
            &TurnOutcome {
                status: RunStatus::Failed,
                stop_reason: None,
                partial: false,
            },
        );
        let store = crate::sessions::SessionStore::at(config.path().join("sessions"));
        assert!(
            store.checkpoints("rec").unwrap().is_empty(),
            "a turn that never started must not persist a phantom checkpoint"
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn recorder_completes_once_and_journals_the_lifecycle() {
        let config = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", config.path());
        let mut recorder = recorder_fixture(config.path());
        let tracker = crate::sessions::checkpoint::CheckpointTracker::default();
        recorder
            .begin_checkpoint(&tracker, config.path().to_path_buf(), 0)
            .unwrap();
        recorder.start();
        recorder.record(RunEvent::MessageDelta { delta: "x".into() });
        let first = recorder.complete(Some(&tracker), true, &TurnOutcome::completed());
        assert_eq!(first.len(), 2);
        assert!(recorder
            .complete(Some(&tracker), true, &TurnOutcome::completed())
            .is_empty());
        let store = crate::sessions::SessionStore::at(config.path().join("sessions"));
        assert_eq!(store.checkpoints("rec").unwrap().len(), 1);
        let kinds: Vec<String> = store
            .journal("rec")
            .unwrap()
            .read_all()
            .unwrap()
            .iter()
            .filter(|entry| entry.kind == "run.event")
            .filter_map(|entry| entry.payload.get("type")?.as_str().map(str::to_string))
            .collect();
        assert_eq!(
            kinds,
            [
                "run.started",
                "turn.started",
                "turn.completed",
                "run.completed"
            ]
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    fn event_payload_fields_are_camel_case() {
        let event = RunEvent::Usage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 4,
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["inputTokens"], 1);
        assert_eq!(json["cacheWriteTokens"], 4);
        assert!(json.get("input_tokens").is_none());
    }
}
