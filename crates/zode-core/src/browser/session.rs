//! Process-wide browser session: one shared backend slot, all backend
//! operations serialized by the slot mutex (agent dispatches tools
//! concurrently; "current tab" is session state — see spec).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use crate::config::BrowserConfig;

use super::backend::{BrowserBackend, BrowserError, BrowserTarget};
use super::bridge::{BridgeBackend, BridgeServer, PairingHandle};

#[async_trait]
pub trait BackendFactory: Send + Sync + std::fmt::Debug {
    async fn create(&self, cfg: &BrowserConfig) -> Result<Arc<dyn BrowserBackend>, BrowserError>;
}

#[derive(Debug)]
pub struct BrowserSession {
    cfg: BrowserConfig,
    factory: Arc<dyn BackendFactory>,
    bridge: Arc<BridgeServer>,
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
        let target = match cfg.default_target() {
            "bridge" => BrowserTarget::Bridge,
            _ => BrowserTarget::Managed,
        };
        Arc::new(Self {
            cfg,
            factory,
            bridge: BridgeServer::new(),
            target: StdMutex::new(target),
            slot: tokio::sync::Mutex::new(None),
            perm_flags: StdMutex::new(Vec::new()),
        })
    }

    pub async fn lease(&self) -> Result<BackendLease<'_>, BrowserError> {
        let want_bridge = matches!(self.target(), BrowserTarget::Bridge);
        let mut guard = self.slot.lock().await;
        let needs_new = match guard.as_ref() {
            Some(b) => !b.is_alive().await || b.is_bridge() != want_bridge,
            None => true,
        };
        if needs_new {
            *guard = Some(if want_bridge {
                if !self.bridge.is_connected() {
                    return Err(BrowserError::Dead(
                        "bridge not connected; run /browser pair and click the extension".into(),
                    ));
                }
                let backend: Arc<dyn BrowserBackend> = BridgeBackend::new(self.bridge.clone());
                backend
            } else {
                self.factory.create(&self.cfg).await?
            });
        }
        Ok(BackendLease { guard })
    }

    pub fn target(&self) -> BrowserTarget {
        self.target.lock().unwrap().clone()
    }

    /// Whether browser control is enabled by config (`browser.enabled`,
    /// including the session-only `--no-browser` CLI override folded into
    /// `cfg` before the engine assembles). Distinct from the `tools:browser`
    /// plugin-group toggle — callers that need "are the tools actually live"
    /// must check both (see `app.rs` `browser_panel_status`).
    pub fn enabled(&self) -> bool {
        self.cfg.enabled()
    }

    pub fn set_target(&self, t: BrowserTarget) -> Result<(), BrowserError> {
        *self.target.lock().unwrap() = t;
        Ok(())
    }

    pub async fn start_pairing(&self) -> Result<PairingHandle, BrowserError> {
        self.bridge.start_pairing().await
    }

    pub fn bridge_connected(&self) -> bool {
        self.bridge.is_connected()
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
    async fn set_target_bridge_now_succeeds() {
        let s = BrowserSession::new(BrowserConfig::default(), MockFactory::new());
        s.set_target(BrowserTarget::Bridge).unwrap();
        assert!(matches!(s.target(), BrowserTarget::Bridge));
    }

    #[tokio::test]
    async fn default_target_bridge_selects_bridge() {
        let cfg = BrowserConfig {
            default_target: Some("bridge".into()),
            ..BrowserConfig::default()
        };
        let s = BrowserSession::new(cfg, MockFactory::new());
        assert!(matches!(s.target(), BrowserTarget::Bridge));
    }

    #[tokio::test]
    async fn lease_bridge_without_pairing_is_dead_with_hint() {
        let s = BrowserSession::new(BrowserConfig::default(), MockFactory::new());
        s.set_target(BrowserTarget::Bridge).unwrap();
        let err = match s.lease().await {
            Ok(_) => panic!("bridge lease should require pairing first"),
            Err(err) => err,
        };
        assert!(matches!(err, BrowserError::Dead(_)));
        assert!(err.to_string().contains("pair"));
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
