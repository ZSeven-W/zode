//! Shared harness types: ids, cleanup closures, and disposers.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;

/// Unique identifier of a context (scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextId(pub u64);

/// Unique identifier of a fiber (plugin runtime instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FiberId(pub u64);

/// A cleanup action that releases resources acquired by an effect.
///
/// Sync cleanups run inline — also during the best-effort teardown of a
/// dropped fiber — so service and listener removals can never leak. Async
/// cleanups are awaited by an explicit `dispose()`; on plain `Drop` they
/// are only spawned best-effort (they need a live tokio runtime).
pub enum Cleanup {
    /// Runs immediately when the owning disposer fires; safe to run from
    /// `Drop` (no await involved).
    Sync(Box<dyn FnOnce() + Send + 'static>),
    /// An async teardown; awaited by `dispose()`.
    Async(BoxFuture<'static, ()>),
}

impl Cleanup {
    /// Build a cleanup from a synchronous closure.
    pub fn sync<F: FnOnce() + Send + 'static>(f: F) -> Self {
        Cleanup::Sync(Box::new(f))
    }

    /// Build a cleanup from a boxed future.
    pub fn async_boxed(fut: BoxFuture<'static, ()>) -> Self {
        Cleanup::Async(fut)
    }
}

impl<F: Future<Output = ()> + Send + 'static> From<F> for Cleanup {
    fn from(fut: F) -> Self {
        Cleanup::Async(Box::pin(fut))
    }
}

impl fmt::Debug for Cleanup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cleanup::Sync(_) => f.write_str("Cleanup::Sync(..)"),
            Cleanup::Async(_) => f.write_str("Cleanup::Async(..)"),
        }
    }
}

struct DisposerInner {
    cleanup: Mutex<Option<Cleanup>>,
    settled: AtomicBool,
}

/// A handle that tears an effect down at most once.
///
/// Calling `dispose()` twice is a no-op; concurrent calls are safe. When
/// the owning fiber unloads, every disposer it registered runs in reverse
/// registration order.
#[derive(Clone)]
pub struct Disposer {
    inner: Arc<DisposerInner>,
}

impl Disposer {
    /// Wrap a cleanup into a disposer.
    pub fn new(cleanup: Cleanup) -> Self {
        Disposer {
            inner: Arc::new(DisposerInner {
                cleanup: Mutex::new(Some(cleanup)),
                settled: AtomicBool::new(false),
            }),
        }
    }

    /// A no-op disposer (already settled).
    pub fn settled() -> Self {
        Disposer {
            inner: Arc::new(DisposerInner {
                cleanup: Mutex::new(None),
                settled: AtomicBool::new(true),
            }),
        }
    }

    /// Whether the cleanup already ran.
    pub fn is_settled(&self) -> bool {
        self.inner.settled.load(Ordering::Acquire)
    }

    /// Run the cleanup and await async teardown.
    pub async fn dispose(&self) {
        if self.inner.settled.swap(true, Ordering::AcqRel) {
            return;
        }
        // Take the cleanup out of the lock scope so the guard is dropped
        // before any await (the future must stay `Send`).
        let cleanup = self.inner.cleanup.lock().unwrap().take();
        if let Some(cleanup) = cleanup {
            match cleanup {
                Cleanup::Sync(f) => f(),
                Cleanup::Async(fut) => fut.await,
            }
        }
    }

    /// Best-effort teardown without awaiting (used by `Drop`).
    pub(crate) fn dispose_bg(&self) {
        if self.inner.settled.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(cleanup) = self.inner.cleanup.lock().unwrap().take() {
            match cleanup {
                Cleanup::Sync(f) => f(),
                Cleanup::Async(fut) => {
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move {
                            fut.await;
                        });
                    }
                    // Without a runtime the async teardown is dropped:
                    // async cleanups require an explicit dispose() to be
                    // guaranteed to run.
                }
            }
        }
    }
}

impl fmt::Debug for Disposer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disposer")
            .field("settled", &self.is_settled())
            .finish()
    }
}
