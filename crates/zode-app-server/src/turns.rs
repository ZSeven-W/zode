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
    pub interrupted: bool,
    pub has_model_override: bool,
}

impl TurnRegistry {
    pub fn start(
        &mut self,
        thread_id: &str,
        turn_id: String,
    ) -> Result<(Turn, AbortController), String> {
        if self.active.contains_key(thread_id) {
            return Err(format!("turn already running for thread: {thread_id}"));
        }
        let abort = AbortController::new();
        let turn = Turn {
            id: turn_id,
            thread_id: thread_id.to_string(),
            status: TurnStatus::Running,
        };
        self.active.insert(
            thread_id.to_string(),
            ActiveTurn {
                turn: turn.clone(),
                abort: abort.clone(),
                interrupted: false,
                has_model_override: false,
            },
        );
        Ok((turn, abort))
    }

    pub fn mark_model_override(&mut self, thread_id: &str) {
        if let Some(active) = self.active.get_mut(thread_id) {
            active.has_model_override = true;
        }
    }

    pub fn generate_id() -> String {
        Uuid::new_v4().simple().to_string()
    }
    pub fn get(&self, thread_id: &str) -> Option<&ActiveTurn> {
        self.active.get(thread_id)
    }
    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }
    pub fn abort_all(&mut self) {
        for active in self.active.values_mut() {
            active.interrupted = true;
            active.abort.abort();
        }
    }

    pub fn interrupt(&mut self, thread_id: &str, turn_id: &str) -> bool {
        let Some(active) = self.active.get_mut(thread_id) else {
            return false;
        };
        if active.turn.id != turn_id {
            return false;
        }
        active.interrupted = true;
        active.abort.abort();
        true
    }

    pub fn abort_thread(&mut self, thread_id: &str) -> bool {
        let Some(active) = self.active.get_mut(thread_id) else {
            return false;
        };
        active.interrupted = true;
        active.abort.abort();
        true
    }

    pub fn finish(&mut self, thread_id: &str, turn_id: &str) -> Option<ActiveTurn> {
        if self
            .active
            .get(thread_id)
            .is_none_or(|active| active.turn.id != turn_id)
        {
            return None;
        }
        self.active.remove(thread_id)
    }
}
