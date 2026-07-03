//! Process-wide browser session: one shared backend slot, all backend
//! operations serialized by the slot mutex (agent dispatches tools
//! concurrently; "current tab" is session state — see spec).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use crate::config::BrowserConfig;

use super::backend::{BrowserBackend, BrowserError, BrowserTarget};

#[async_trait]
pub trait BackendFactory: Send + Sync + std::fmt::Debug {
    async fn create(&self, cfg: &BrowserConfig) -> Result<Arc<dyn BrowserBackend>, BrowserError>;
}

#[derive(Debug)]
pub struct BrowserSession {
    cfg: BrowserConfig,
    factory: Arc<dyn BackendFactory>,
    target: StdMutex<BrowserTarget>,
    slot: tokio::sync::Mutex<Option<Arc<dyn BrowserBackend>>>,
    perm_flags: StdMutex<Vec<(String, Arc<AtomicBool>)>>,
}

/// Holding a lease holds the slot mutex: backend ops performed through
/// a lease are serialized across all zode tabs and concurrent tools.
pub struct BackendLease<'a> {
    guard: tokio::sync::MutexGuard<'a, Option<Arc<dyn BrowserBackend>>>,
}

impl BackendLease<'_> {
    pub fn backend(&self) -> Arc<dyn BrowserBackend> {
        self.guard
            .as_ref()
            .expect("lease always holds a live backend")
            .clone()
    }
}

impl BrowserSession {
    pub fn new(cfg: BrowserConfig, factory: Arc<dyn BackendFactory>) -> Arc<Self> {
        let target = if cfg.default_target() == "bridge" {
            // Bridge ships in M2; fall back rather than fail startup.
            BrowserTarget::Managed
        } else {
            BrowserTarget::Managed
        };
        Arc::new(Self {
            cfg,
            factory,
            target: StdMutex::new(target),
            slot: tokio::sync::Mutex::new(None),
            perm_flags: StdMutex::new(Vec::new()),
        })
    }

    pub async fn lease(&self) -> Result<BackendLease<'_>, BrowserError> {
        if matches!(self.target(), BrowserTarget::Bridge) {
            return Err(BrowserError::Protocol(
                "bridge target ships in M2; switch back with /browser target managed".into(),
            ));
        }
        let mut guard = self.slot.lock().await;
        let dead = match guard.as_ref() {
            Some(b) => !b.is_alive().await,
            None => true,
        };
        if dead {
            *guard = Some(self.factory.create(&self.cfg).await?);
        }
        Ok(BackendLease { guard })
    }

    pub fn target(&self) -> BrowserTarget {
        self.target.lock().unwrap().clone()
    }

    pub fn set_target(&self, t: BrowserTarget) -> Result<(), BrowserError> {
        if matches!(t, BrowserTarget::Bridge) {
            return Err(BrowserError::Protocol(
                "bridge target ships in M2 (extension pairing)".into(),
            ));
        }
        *self.target.lock().unwrap() = t;
        Ok(())
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

    /// Best-effort URL for approval prompts: never blocks behind a
    /// running operation (try_lock), never launches a browser.
    pub async fn current_url_hint(&self) -> Option<String> {
        let guard = self.slot.try_lock().ok()?;
        let backend = guard.as_ref()?.clone();
        drop(guard);
        backend.current_url().await.ok()
    }

    pub async fn status(&self) -> String {
        let running = self.slot.try_lock().map(|g| g.is_some()).unwrap_or(true); // busy slot implies a live backend
        format!(
            "browser: target={} running={} headless={}",
            match self.target() {
                BrowserTarget::Managed => "managed",
                BrowserTarget::Bridge => "bridge",
            },
            running,
            self.cfg.headless(),
        )
    }

    pub async fn close(&self) {
        let mut guard = self.slot.lock().await;
        if let Some(b) = guard.take() {
            let _ = b.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::backend::mock::MockFactory;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn lease_serializes_concurrent_ops() {
        let f = MockFactory::new();
        let s = BrowserSession::new(BrowserConfig::default(), f.clone());
        let mut joins = Vec::new();
        for _ in 0..8 {
            let s = s.clone();
            joins.push(tokio::spawn(async move {
                let lease = s.lease().await.unwrap();
                lease.backend().snapshot().await.unwrap();
            }));
        }
        for j in joins {
            j.await.unwrap();
        }
        let b = f.current.lock().unwrap().clone().unwrap();
        assert_eq!(b.calls.load(Ordering::SeqCst), 8);
        assert!(!b.overlap_seen.load(Ordering::SeqCst), "ops overlapped");
        assert_eq!(f.made.load(Ordering::SeqCst), 1, "one backend reused");
    }

    #[tokio::test]
    async fn dead_backend_is_relaunched_once() {
        let f = MockFactory::new();
        let s = BrowserSession::new(BrowserConfig::default(), f.clone());
        {
            s.lease().await.unwrap();
        }
        f.current
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .dead
            .store(true, Ordering::SeqCst);
        {
            s.lease().await.unwrap();
        } // must recreate
        assert_eq!(f.made.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn bridge_target_is_rejected_in_m1() {
        let s = BrowserSession::new(BrowserConfig::default(), MockFactory::new());
        let err = s.set_target(BrowserTarget::Bridge).unwrap_err();
        assert!(err.to_string().contains("M2"));
        assert!(matches!(s.target(), BrowserTarget::Managed));
    }

    #[tokio::test]
    async fn url_hint_is_none_before_first_lease() {
        let s = BrowserSession::new(BrowserConfig::default(), MockFactory::new());
        assert!(s.current_url_hint().await.is_none());
        {
            s.lease().await.unwrap();
        }
        assert_eq!(
            s.current_url_hint().await.as_deref(),
            Some("https://example.test/")
        );
    }
}
