use std::collections::BTreeMap;

use crate::error::{error, ServerResult};
use uuid::Uuid;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_app_server_protocol::types::{Thread, ThreadStartParams, ThreadStatus};

#[derive(Debug, Default)]
pub struct ThreadRegistry {
    threads: BTreeMap<String, Thread>,
}

impl ThreadRegistry {
    pub fn start_metadata_only(
        &mut self,
        params: ThreadStartParams,
        title: String,
    ) -> ServerResult<Thread> {
        let id = Uuid::new_v4().simple().to_string();
        let thread = Thread {
            id: id.clone(),
            name: title,
            cwd: params.cwd.unwrap_or_else(|| ".".to_string()),
            model: params.model.unwrap_or_else(|| "default".to_string()),
            status: ThreadStatus::Loaded,
        };
        self.threads.insert(id, thread.clone());
        Ok(thread)
    }

    pub fn list(&self) -> Vec<Thread> {
        self.threads.values().cloned().collect()
    }

    pub fn read(&self, id: &str) -> ServerResult<Thread> {
        self.threads
            .get(id)
            .cloned()
            .ok_or_else(|| error(INVALID_PARAMS, format!("thread not found: {id}")))
    }

    pub fn set_name(&mut self, id: &str, name: &str) -> ServerResult<()> {
        let thread = self
            .threads
            .get_mut(id)
            .ok_or_else(|| error(INVALID_PARAMS, format!("thread not found: {id}")))?;
        thread.name = name.to_string();
        Ok(())
    }

    pub fn set_model(&mut self, id: &str, model: &str) -> ServerResult<()> {
        let thread = self
            .threads
            .get_mut(id)
            .ok_or_else(|| error(INVALID_PARAMS, format!("thread not found: {id}")))?;
        thread.model = model.to_string();
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> ServerResult<()> {
        self.threads
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| error(INVALID_PARAMS, format!("thread not found: {id}")))
    }
}
