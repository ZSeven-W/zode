//! Plugin fibers: per-plugin runtime instances owning effects and cleanup.

use std::any::TypeId;
use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock, Weak};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tokio::sync::watch;

use crate::context::Context;
use crate::error::CordisError;
use crate::plugin::Plugin;
use crate::types::Disposer;

pub use crate::types::FiberId;

/// Lifecycle state of one plugin fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    /// Waiting for required services.
    Pending,
    /// The plugin callback is running.
    Loading,
    /// Loaded and providing.
    Active,
    /// The callback or its config validation failed.
    Failed,
    /// Cleanup is running.
    Unloading,
    /// The fiber was removed and cannot restart.
    Disposed,
}

impl FiberState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FiberState::Pending => "pending",
            FiberState::Loading => "loading",
            FiberState::Active => "active",
            FiberState::Failed => "failed",
            FiberState::Unloading => "unloading",
            FiberState::Disposed => "disposed",
        }
    }
}

/// Runtime instance of one plugin application.
#[derive(Clone)]
pub struct Fiber {
    pub(crate) inner: Arc<FiberInner>,
}

pub struct FiberInner {
    pub id: FiberId,
    pub name: Arc<str>,
    /// The context this fiber's plugin runs in (extends the parent context).
    pub ctx: Context,
    /// Identity of the plugin runtime this fiber belongs to (for grouping).
    pub runtime_type: TypeId,
    /// The exact plugin instance this fiber loads — two fibers of the same
    /// plugin type may carry different instances (different closures or
    /// state), so loading must never fall back to a shared runtime record.
    pub plugin: Option<Arc<dyn Plugin>>,
    /// The validated plugin config.
    pub config: RwLock<Arc<Value>>,
    /// Required service names.
    pub inject: Vec<Arc<str>>,
    pub state: watch::Sender<FiberState>,
    pub error: RwLock<Option<Arc<str>>>,
    pub disposers: Mutex<Vec<(Arc<str>, Disposer)>>,
}

impl FiberInner {
    pub fn state(&self) -> FiberState {
        *self.state.borrow()
    }

    pub fn set_state(&self, state: FiberState) -> FiberState {
        self.state.send_replace(state)
    }

    pub fn register_disposer(&self, label: Arc<str>, disposer: Disposer) {
        self.disposers.lock().unwrap().push((label, disposer));
    }

    pub fn take_disposers(&self) -> Vec<(Arc<str>, Disposer)> {
        std::mem::take(&mut *self.disposers.lock().unwrap())
    }

    pub fn effects(&self) -> Vec<Arc<str>> {
        self.disposers
            .lock()
            .unwrap()
            .iter()
            .map(|(label, _)| label.clone())
            .collect()
    }
}

impl Drop for FiberInner {
    fn drop(&mut self) {
        // Best-effort teardown without a runtime: sync cleanups (service and
        // listener removals) always run, so dropping a fiber can never leak
        // registry state. Async cleanups need an explicit `Fiber::dispose()`
        // to be guaranteed.
        for (_, disposer) in self.take_disposers().into_iter().rev() {
            disposer.dispose_bg();
        }
    }
}

impl fmt::Debug for Fiber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fiber")
            .field("id", &self.inner.id)
            .field("name", &self.inner.name)
            .field("state", &self.inner.state())
            .finish()
    }
}

impl Fiber {
    pub fn id(&self) -> FiberId {
        self.inner.id
    }

    pub fn name(&self) -> Arc<str> {
        self.inner.name.clone()
    }

    pub fn state(&self) -> FiberState {
        self.inner.state()
    }

    pub fn ctx(&self) -> &Context {
        &self.inner.ctx
    }

    pub fn config(&self) -> Arc<Value> {
        self.inner.config.read().unwrap().clone()
    }

    /// Labels of the effects currently registered by this fiber.
    pub fn effects(&self) -> Vec<Arc<str>> {
        self.inner.effects()
    }

    pub fn error(&self) -> Option<Arc<str>> {
        self.inner.error.read().unwrap().clone()
    }

    /// Wait until the fiber settles: `Ok` once `Active`, or `Err` once the
    /// plugin failed or the fiber was disposed. A pending fiber resolves when
    /// its dependencies arrive.
    pub async fn await_ready(&self) -> Result<(), CordisError> {
        let mut rx = self.inner.state.subscribe();
        loop {
            match *rx.borrow() {
                FiberState::Active => return Ok(()),
                FiberState::Failed => {
                    let message = self
                        .error()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "plugin failed".to_string());
                    return Err(CordisError::PluginStartup(self.name().to_string(), message));
                }
                FiberState::Disposed => {
                    return Err(CordisError::FiberDisposed(self.name().to_string()));
                }
                _ => {}
            }
            if rx.changed().await.is_err() {
                return Err(CordisError::FiberDisposed(self.name().to_string()));
            }
        }
    }

    /// Dispose this fiber permanently: unload the plugin, run its cleanup in
    /// reverse registration order, and remove it from the registry.
    pub async fn dispose(&self) {
        crate::registry::dispose_fiber(&self.inner).await;
    }

    /// Register a listener owned by this fiber, regardless of the
    /// task-local current fiber (usable from spawned helper tasks). The
    /// listener is removed when the fiber unloads. Context filtering
    /// applies: it fires for dispatches on the fiber context or below.
    pub fn on_dyn<F, Fut>(&self, name: &str, f: F) -> Result<crate::types::Disposer, CordisError>
    where
        F: Fn(&crate::events::Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::events::Flow> + Send + 'static,
    {
        self.on_dyn_with(name, f, false)
    }

    /// Like \`on_dyn\`, but the listener fires for dispatches on any
    /// context (useful for process-wide observers such as subprocess
    /// plugins).
    pub fn on_dyn_global<F, Fut>(
        &self,
        name: &str,
        f: F,
    ) -> Result<crate::types::Disposer, CordisError>
    where
        F: Fn(&crate::events::Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::events::Flow> + Send + 'static,
    {
        self.on_dyn_with(name, f, true)
    }

    fn on_dyn_with<F, Fut>(
        &self,
        name: &str,
        f: F,
        global: bool,
    ) -> Result<crate::types::Disposer, CordisError>
    where
        F: Fn(&crate::events::Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::events::Flow> + Send + 'static,
    {
        let root = self.inner.ctx.root_state();
        let disposer = root.bus.register(
            &root,
            &self.inner.ctx,
            name,
            Box::new(move |event| Box::pin(f(event)) as BoxFuture<'static, crate::events::Flow>),
            global,
        )?;
        self.inner
            .register_disposer(Arc::from(format!("on:{name}")), disposer.clone());
        Ok(disposer)
    }

    /// Unload and immediately reload the plugin with its current config.
    pub async fn restart(&self) -> Result<(), CordisError> {
        crate::registry::reload_fiber(&self.inner).await
    }

    /// Validate and apply new config, then restart the plugin.
    pub async fn update(&self, config: Value) -> Result<(), CordisError> {
        if let Some(plugin) = &self.inner.plugin {
            plugin
                .validate(&config)
                .map_err(|e| CordisError::ConfigInvalid(self.name().to_string(), e.to_string()))?;
        }
        *self.inner.config.write().unwrap() = Arc::new(config);
        crate::registry::reload_fiber(&self.inner).await
    }
}

// The fiber whose plugin is currently running on this task. Effect,
// service, and listener registration attaches to it (falling back to the
// root fiber outside any plugin).
tokio::task_local! {
    static CURRENT_FIBER: RefCell<Option<Weak<FiberInner>>>;
}

pub(crate) async fn scope_fiber<T>(fiber: &Arc<FiberInner>, fut: impl Future<Output = T>) -> T {
    CURRENT_FIBER
        .scope(RefCell::new(Some(Arc::downgrade(fiber))), fut)
        .await
}

pub(crate) fn current_fiber() -> Option<Arc<FiberInner>> {
    CURRENT_FIBER
        .try_with(|cell| cell.borrow().as_ref().and_then(|weak| weak.upgrade()))
        .ok()
        .flatten()
}

pub(crate) fn current_fiber_or_root(ctx: &Context) -> Option<Arc<FiberInner>> {
    current_fiber().or_else(|| {
        ctx.root_state()
            .root_fiber
            .get()
            .and_then(|weak| weak.upgrade())
    })
}
