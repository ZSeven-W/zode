//! Thin holder around a [`ComputerBackend`]. Unlike `BrowserSession`, there
//! is no lazily-launched subprocess to relaunch and no "current tab" to
//! serialize — the backend talks directly to OS APIs in-process — so this
//! is just an `Arc<dyn ComputerBackend>` plus the "allow always" flag
//! registry that `computer_gated` needs (mirroring `BrowserSession`'s
//! `perm_flags`, see `browser/session.rs`).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use super::backend::ComputerBackend;

#[derive(Debug)]
pub struct ComputerSession {
    backend: Arc<dyn ComputerBackend>,
    perm_flags: StdMutex<Vec<(String, Arc<AtomicBool>)>>,
}

impl ComputerSession {
    pub fn new(backend: Arc<dyn ComputerBackend>) -> Arc<Self> {
        Arc::new(Self {
            backend,
            perm_flags: StdMutex::new(Vec::new()),
        })
    }

    pub fn backend(&self) -> Arc<dyn ComputerBackend> {
        self.backend.clone()
    }

    pub fn register_perm_flag(&self, tool: &str, flag: Arc<AtomicBool>) {
        self.perm_flags
            .lock()
            .unwrap()
            .push((tool.to_string(), flag));
    }

    pub fn perm_flags(&self) -> Vec<(String, Arc<AtomicBool>)> {
        self.perm_flags.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::backend::mock::MockBackend;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn backend_is_reachable_through_the_session() {
        let session = ComputerSession::new(Arc::new(MockBackend::default()));
        let state = session.backend().app_state(None).await.unwrap();
        assert_eq!(state.generation, 1);
    }

    #[test]
    fn perm_flags_round_trip() {
        let session = ComputerSession::new(Arc::new(MockBackend::default()));
        let flag = Arc::new(AtomicBool::new(true));
        session.register_perm_flag("computer_act", flag.clone());
        let flags = session.perm_flags();
        assert_eq!(flags[0].0, "computer_act");
        assert!(flags[0].1.load(Ordering::Relaxed));
    }
}
