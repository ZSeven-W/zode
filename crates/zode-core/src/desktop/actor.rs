//! ActorHandle owns the lifecycle of one platform backend: lazy creation,
//! generation replacement on repeated timeouts, and a circuit breaker that
//! fails fast (Dead) once replacements exceed a session cap, until a manual
//! reset (surfaced via `/desktop status`). See spec §线程与执行模型 recovery.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

use super::backend::{DesktopBackend, DesktopBackendFactory, DesktopError};

#[derive(Debug)]
pub struct ActorHandle {
    factory: Arc<dyn DesktopBackendFactory>,
    slot: Mutex<Option<Arc<dyn DesktopBackend>>>,
    generation: AtomicU64,
    replacements: AtomicU64,
    max_replacements: u64,
    tripped: AtomicBool,
}

impl ActorHandle {
    pub fn new(factory: Arc<dyn DesktopBackendFactory>, max_replacements: u64) -> Arc<Self> {
        Arc::new(Self {
            factory,
            slot: Mutex::new(None),
            generation: AtomicU64::new(0),
            replacements: AtomicU64::new(0),
            max_replacements,
            tripped: AtomicBool::new(false),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.tripped.store(false, Ordering::SeqCst);
        self.replacements.store(0, Ordering::SeqCst);
    }

    /// Drop the current backend and bump generation; trip the breaker once the
    /// session replacement cap is exceeded (defends against a bad provider that
    /// keeps wedging the freshly-spawned actor thread).
    pub async fn request_replacement(&self) {
        let mut slot = self.slot.lock().await;
        *slot = None;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let n = self.replacements.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= self.max_replacements {
            self.tripped.store(true, Ordering::SeqCst);
        }
    }

    pub async fn backend(&self) -> Result<Arc<dyn DesktopBackend>, DesktopError> {
        if self.is_tripped() {
            return Err(DesktopError::Dead(
                "desktop backend tripped after repeated timeouts (suspected bad accessibility \
                 provider); run `/desktop status` to reset"
                    .into(),
            ));
        }
        let mut slot = self.slot.lock().await;
        if slot.is_none() {
            *slot = Some(self.factory.create().await?);
        }
        Ok(slot.as_ref().unwrap().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::mock::{mock_factory, MockDesktopFactory};

    #[tokio::test]
    async fn backend_created_once_then_reused() {
        let f = Arc::new(MockDesktopFactory::default());
        let h = ActorHandle::new(f.clone(), 3);
        h.backend().await.unwrap();
        h.backend().await.unwrap();
        assert_eq!(f.made.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn generation_bumps_until_breaker_trips_then_dead() {
        let h = ActorHandle::new(mock_factory(), 3);
        h.backend().await.unwrap();
        let g0 = h.generation();
        for _ in 0..3 {
            h.request_replacement().await;
        }
        assert!(h.is_tripped());
        assert!(h.generation() > g0);
        let err = h.backend().await.unwrap_err();
        assert!(matches!(err, DesktopError::Dead(_)));
        h.reset();
        assert!(!h.is_tripped());
        assert!(h.backend().await.is_ok());
    }
}
