use std::collections::BTreeMap;

use agent::abort::AbortController;
use uuid::Uuid;
use zode_app_server_protocol::types::{Turn, TurnStatus};

#[derive(Default)]
pub struct TurnRegistry {
    active: BTreeMap<String, ActiveTurn>,
}

pub struct ActiveTurn {
    pub turn: Turn,
    pub abort: AbortController,
}

impl TurnRegistry {
    pub fn start(&mut self, thread_id: &str) -> Result<(Turn, AbortController), String> {
        if self.active.contains_key(thread_id) {
            return Err(format!("turn already running for thread: {thread_id}"));
        }
        let abort = AbortController::new();
        let turn = Turn {
            id: Uuid::new_v4().simple().to_string(),
            thread_id: thread_id.to_string(),
            status: TurnStatus::Running,
        };
        self.active.insert(
            thread_id.to_string(),
            ActiveTurn {
                turn: turn.clone(),
                abort: abort.clone(),
            },
        );
        Ok((turn, abort))
    }

    pub fn interrupt(&mut self, thread_id: &str, turn_id: &str) -> bool {
        let Some(active) = self.active.get(thread_id) else {
            return false;
        };
        if active.turn.id != turn_id {
            return false;
        }
        active.abort.abort();
        true
    }

    pub fn finish(&mut self, thread_id: &str, turn_id: &str) -> bool {
        let Some(active) = self.active.get(thread_id) else {
            return false;
        };
        if active.turn.id != turn_id {
            return false;
        }
        self.active.remove(thread_id);
        true
    }
}
