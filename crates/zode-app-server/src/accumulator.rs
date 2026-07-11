//! Accumulates engine events into turn notifications and a final outcome.

use std::collections::HashMap;

use agent::stream::Event;
use zode_app_server_protocol::{notify, JsonRpcNotification};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEndState {
    Completed,
    Interrupted,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub state: TurnEndState,
    pub final_text: String,
    pub usage: notify::TurnUsage,
}

pub struct TurnAccumulator {
    thread_id: String,
    turn_id: String,
    seq: u64,
    item_ids: HashMap<String, String>,
    current_text: String,
    segment_open: bool,
    usage: notify::TurnUsage,
}

impl TurnAccumulator {
    pub fn new(thread_id: &str, turn_id: &str) -> Self {
        Self {
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            seq: 0,
            item_ids: HashMap::new(),
            current_text: String::new(),
            segment_open: false,
            usage: notify::TurnUsage::default(),
        }
    }

    /// Maps one engine event to zero or more non-terminal notifications.
    pub fn on_event(&mut self, event: &Event) -> Vec<JsonRpcNotification> {
        match event {
            Event::TextDelta { delta } => {
                if !self.segment_open {
                    self.current_text.clear();
                    self.segment_open = true;
                }
                self.current_text.push_str(delta);
                vec![notify::agent_message_delta(
                    &self.thread_id,
                    &self.turn_id,
                    delta,
                )]
            }
            Event::ToolUse { id, name, input } => {
                self.segment_open = false;
                let item_id = format!("{}-item-{}", self.turn_id, self.seq);
                self.seq += 1;
                self.item_ids.insert(id.clone(), item_id.clone());
                vec![notify::item_started(
                    &self.thread_id,
                    &self.turn_id,
                    &item_id,
                    name,
                    input,
                )]
            }
            Event::ToolResult { id, ok, output } => {
                let item_id = self.item_ids.get(id).map_or(id.as_str(), String::as_str);
                vec![notify::item_completed(
                    &self.thread_id,
                    &self.turn_id,
                    item_id,
                    *ok,
                    output,
                )]
            }
            Event::Usage {
                input_tokens,
                output_tokens,
                cache_read,
                cache_create,
            } => {
                self.usage.input_tokens += u64::from(*input_tokens);
                self.usage.output_tokens += u64::from(*output_tokens);
                self.usage.cache_read += u64::from(*cache_read);
                self.usage.cache_create += u64::from(*cache_create);
                vec![]
            }
            Event::Error { code, message } => vec![notify::turn_error(
                &self.thread_id,
                &self.turn_id,
                code,
                message,
            )],
            _ => vec![],
        }
    }

    pub fn finish(self, state: TurnEndState) -> TurnOutcome {
        TurnOutcome {
            state,
            final_text: self.current_text,
            usage: self.usage,
        }
    }
}
