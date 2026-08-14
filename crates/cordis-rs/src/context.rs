//! Contexts: scoped dependency containers with extend/isolate/intercept.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tokio::sync::watch;

use crate::error::CordisError;
use crate::events::{EventBus, HistoryRecord};
use crate::fiber::{current_fiber, Fiber, FiberId, FiberInner, FiberState};
use crate::logger::Logger;
use crate::memory::{MemoryBudget, MemoryStats};
use crate::plugin::Plugin;
use crate::registry::Registry;
use crate::service::{ScopeKey, ServiceEntry};
use crate::types::{Cleanup, Disposer};

pub use crate::types::ContextId;

/// A scoped dependency container, the Rust analogue of a Cordis context.
///
/// Contexts form a chain: `extend()` adds an empty child scope,
/// `isolate(name)` gives one service name an independent scope, and
/// `intercept(name, config)` adds service-specific config. Service and
/// listener registrations attach to the current fiber and are removed when
/// it unloads; dropping the root context drops every fiber it owns.
#[derive(Clone)]
pub struct Context {
    pub(crate) inner: Arc<ContextInner>,
}

pub struct ContextInner {
    pub id: ContextId,
    /// Human-readable scope label (`isolate:x`, `intercept:x`, a plugin
    /// name, or `None`).
    pub label: Option<Arc<str>>,
    pub parent: Option<Weak<ContextInner>>,
    /// Ids of this context and all its ancestors (dispatch filtering).
    pub ancestor_ids: Vec<ContextId>,
    pub root: Arc<RootInner>,
    /// Service name → isolation scope label (own entries only).
    pub isolates: RwLock<HashMap<Arc<str>, Arc<str>>>,
    /// (name, config) intercept entries (own entries only).
    pub intercepts: RwLock<Vec<(Arc<str>, Value)>>,
    pub disposed: AtomicBool,
    /// Live fibers; populated on the root context only.
    pub fibers: Mutex<Vec<Arc<FiberInner>>>,
}

/// An async hook run at root disposal.
pub type DisposeHook = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// Shared state of one harness (bus, service store, registry, budget).
pub struct RootInner {
    pub id_counter: AtomicU64,
    pub fiber_counter: AtomicU64,
    /// Live contexts (incremented on extend, decremented on drop).
    pub live_ctx: AtomicUsize,
    pub lazy_uninitialized: AtomicUsize,
    pub bus: EventBus,
    pub services: RwLock<HashMap<ScopeKey, Arc<ServiceEntry>>>,
    pub registry: Registry,
    pub budget: RwLock<MemoryBudget>,
    pub root_ctx: OnceLock<Weak<ContextInner>>,
    pub root_fiber: OnceLock<Weak<FiberInner>>,
    pub dispose_hooks: Mutex<Vec<DisposeHook>>,
    pub disposed: AtomicBool,
}

impl Drop for ContextInner {
    fn drop(&mut self) {
        // Fields drop after this impl runs, so the root state is still
        // reachable here.
        self.root.live_ctx.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A weak handle to a context (for leak checks and observers).
#[derive(Clone)]
pub struct WeakContext(Weak<ContextInner>);

impl WeakContext {
    pub fn upgrade(&self) -> Option<Context> {
        self.0.upgrade().map(|inner| Context { inner })
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("id", &self.inner.id)
            .field("label", &self.inner.label)
            .finish()
    }
}

fn shallow_merge(mut base: Value, head: Value) -> Value {
    match (&mut base, head) {
        (Value::Object(base_map), Value::Object(head_map)) => {
            for (key, value) in head_map {
                base_map.insert(key, value);
            }
            base
        }
        (_, head) => head,
    }
}

impl Context {
    /// Create a root context: a fresh harness with its own event bus,
    /// service store, plugin registry, and memory budget.
    pub fn root() -> Self {
        let root_inner = Arc::new(RootInner {
            id_counter: AtomicU64::new(1),
            fiber_counter: AtomicU64::new(0),
            live_ctx: AtomicUsize::new(1),
            lazy_uninitialized: AtomicUsize::new(0),
            bus: EventBus::new(),
            services: RwLock::new(HashMap::new()),
            registry: Registry::new(),
            budget: RwLock::new(MemoryBudget::default()),
            root_ctx: OnceLock::new(),
            root_fiber: OnceLock::new(),
            dispose_hooks: Mutex::new(Vec::new()),
            disposed: AtomicBool::new(false),
        });
        let ctx_inner = Arc::new(ContextInner {
            id: ContextId(1),
            label: None,
            parent: None,
            ancestor_ids: vec![ContextId(1)],
            root: root_inner.clone(),
            isolates: RwLock::new(HashMap::new()),
            intercepts: RwLock::new(Vec::new()),
            disposed: AtomicBool::new(false),
            fibers: Mutex::new(Vec::new()),
        });
        let _ = root_inner.root_ctx.set(Arc::downgrade(&ctx_inner));
        let root = Context { inner: ctx_inner };

        // The root fiber owns registration done outside any plugin. Its
        // context is a dedicated child so the root context and the fiber
        // never form an Arc cycle (root ctx → fibers → fiber ctx →
        // RootInner, all forward edges).
        let fiber_ctx = root
            .extend_inner(Some(Arc::from("root")))
            .expect("root fiber context");
        let (state, _) = watch::channel(FiberState::Active);
        let root_fiber = Arc::new(FiberInner {
            id: FiberId(0),
            name: Arc::from("root"),
            ctx: fiber_ctx,
            runtime_type: std::any::TypeId::of::<()>(),
            plugin: None,
            config: RwLock::new(Arc::new(Value::Null)),
            inject: Vec::new(),
            state,
            error: RwLock::new(None),
            disposers: Mutex::new(Vec::new()),
        });
        let _ = root_inner.root_fiber.set(Arc::downgrade(&root_fiber));
        root.inner.fibers.lock().unwrap().push(root_fiber);
        root
    }

    pub fn id(&self) -> ContextId {
        self.inner.id
    }

    /// Scope label (`None` for the root).
    pub fn label(&self) -> Option<&str> {
        self.inner.label.as_deref()
    }

    pub fn is_root(&self) -> bool {
        self.inner.parent.is_none()
    }

    pub fn downgrade(&self) -> WeakContext {
        WeakContext(Arc::downgrade(&self.inner))
    }

    pub(crate) fn root_state(&self) -> Arc<RootInner> {
        self.inner.root.clone()
    }

    pub(crate) fn check_alive(&self) -> Result<(), CordisError> {
        if self.root_state().disposed.load(Ordering::Acquire) {
            return Err(CordisError::ContextDisposed);
        }
        Ok(())
    }

    /// Create a child scope inheriting every ancestor property.
    pub(crate) fn extend_inner(&self, label: Option<Arc<str>>) -> Result<Self, CordisError> {
        self.check_alive()?;
        let root = self.root_state();
        let budget = *root.budget.read().unwrap();
        let count = root.live_ctx.fetch_add(1, Ordering::SeqCst) + 1;
        if count > budget.max_contexts {
            root.live_ctx.fetch_sub(1, Ordering::SeqCst);
            return Err(CordisError::BudgetExceeded("max_contexts"));
        }
        let id = ContextId(root.id_counter.fetch_add(1, Ordering::SeqCst) + 1);
        let mut ancestor_ids = self.inner.ancestor_ids.clone();
        ancestor_ids.push(id);
        Ok(Context {
            inner: Arc::new(ContextInner {
                id,
                label,
                parent: Some(Arc::downgrade(&self.inner)),
                ancestor_ids,
                root,
                isolates: RwLock::new(HashMap::new()),
                intercepts: RwLock::new(Vec::new()),
                disposed: AtomicBool::new(false),
                fibers: Mutex::new(Vec::new()),
            }),
        })
    }

    /// Create a child scope with extra metadata on top of the current one.
    pub fn extend(&self) -> Result<Self, CordisError> {
        self.extend_inner(None)
    }

    /// Create a child context with an independent service scope for `name`.
    /// A different implementation can be provided below it without affecting
    /// the parent scope.
    pub fn isolate(&self, name: &str) -> Result<Self, CordisError> {
        let child = self.extend_inner(Some(Arc::from(format!("isolate:{name}"))))?;
        let label: Arc<str> = Arc::from(format!(
            "isolate:{name}#{}",
            self.root_state().id_counter.fetch_add(1, Ordering::SeqCst)
        ));
        child
            .inner
            .isolates
            .write()
            .unwrap()
            .insert(Arc::from(name), label);
        Ok(child)
    }

    /// Like `isolate`, but with an explicit scope label; two isolates with
    /// the same label join their scopes.
    pub fn isolate_with(&self, name: &str, label: &str) -> Result<Self, CordisError> {
        let child = self.extend_inner(Some(Arc::from(format!("isolate:{name}"))))?;
        child
            .inner
            .isolates
            .write()
            .unwrap()
            .insert(Arc::from(name), Arc::from(label));
        Ok(child)
    }

    /// Create a child context that merges extra config into a service's
    /// resolved intercept config (ancestor entries first, child overrides).
    pub fn intercept(&self, name: &str, config: Value) -> Result<Self, CordisError> {
        let child = self.extend_inner(Some(Arc::from(format!("intercept:{name}"))))?;
        child
            .inner
            .intercepts
            .write()
            .unwrap()
            .push((Arc::from(name), config));
        Ok(child)
    }

    /// The isolation scope label governing lookups of `name` from this
    /// context (walking up to the nearest isolate).
    pub(crate) fn isolation_label(&self, name: &str) -> Option<Arc<str>> {
        let mut current = Some(self.inner.clone());
        while let Some(ctx) = current {
            if let Some(label) = ctx.isolates.read().unwrap().get(name) {
                return Some(label.clone());
            }
            current = ctx.parent.as_ref().and_then(|weak| weak.upgrade());
        }
        None
    }

    /// Merge intercept config for a service name (ancestor entries first,
    /// later entries override; `extra` is applied last).
    pub fn resolve_intercept(&self, name: &str, extra: Option<Value>) -> Value {
        let mut entries: Vec<Value> = Vec::new();
        let mut current = Some(self.inner.clone());
        while let Some(ctx) = current {
            for (entry_name, config) in ctx.intercepts.read().unwrap().iter() {
                if entry_name.as_ref() == name {
                    entries.push(config.clone());
                }
            }
            current = ctx.parent.as_ref().and_then(|weak| weak.upgrade());
        }
        entries.reverse();
        if let Some(extra) = extra {
            entries.push(extra);
        }
        let mut merged = Value::Null;
        for entry in entries {
            merged = shallow_merge(merged, entry);
        }
        merged
    }

    /// A named logger (`cordis::<name>` tracing target).
    pub fn logger(&self) -> Logger {
        let name = current_fiber()
            .map(|fiber| fiber.name.clone())
            .or_else(|| self.inner.label.clone())
            .unwrap_or_else(|| Arc::from("cordis"));
        Logger { name }
    }

    /// The fiber whose plugin is running on the current task, if any.
    pub fn current_fiber(&self) -> Option<Fiber> {
        current_fiber().map(|inner| Fiber { inner })
    }

    // ---- lifecycle -------------------------------------------------------

    /// Dispose the whole harness (root context only): unloads every fiber
    /// in reverse order, runs dispose hooks, and drops residual state.
    pub async fn dispose(&self) -> Result<(), CordisError> {
        if !self.is_root() {
            return Err(CordisError::Unsupported(
                "dispose a non-root context — dispose its fiber instead",
            ));
        }
        let root = self.root_state();
        if root.disposed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.inner.disposed.store(true, Ordering::Release);
        let fibers = std::mem::take(&mut *self.inner.fibers.lock().unwrap());
        for fiber in fibers.iter().rev() {
            if fiber.id != FiberId(0) {
                crate::registry::dispose_fiber(fiber).await;
            }
        }
        let hooks = std::mem::take(&mut *root.dispose_hooks.lock().unwrap());
        for hook in hooks.into_iter().rev() {
            hook().await;
        }
        if let Some(root_fiber) = root.root_fiber.get().and_then(|weak| weak.upgrade()) {
            crate::registry::dispose_fiber(&root_fiber).await;
        }
        // Defensive: drop any residual state (should already be empty).
        root.services.write().unwrap().clear();
        root.bus.clear();
        root.registry.clear();
        Ok(())
    }

    /// Register an async hook that runs (in reverse order) when the root
    /// context disposes.
    pub fn on_dispose<F, Fut>(&self, f: F) -> Result<(), CordisError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.check_alive()?;
        self.root_state()
            .dispose_hooks
            .lock()
            .unwrap()
            .push(Box::new(move || Box::pin(f())));
        Ok(())
    }

    // ---- memory ----------------------------------------------------------

    pub fn memory_stats(&self) -> MemoryStats {
        let root = self.root_state();
        let (listeners, events, history_records, history_bytes) = root.bus.stats();
        let services = root.services.read().unwrap().len();
        let pending = root.registry.pending_len();
        let fibers = root
            .root_ctx
            .get()
            .and_then(|weak| weak.upgrade())
            .map(|root_ctx| root_ctx.fibers.lock().unwrap().len())
            .unwrap_or(0);
        let contexts = root.live_ctx.load(Ordering::Acquire);
        let lazy = root.lazy_uninitialized.load(Ordering::Acquire);
        let estimated_bytes = fibers * 512
            + pending * 256
            + services * 256
            + listeners * 512
            + contexts * 160
            + history_bytes;
        MemoryStats {
            fibers,
            pending_fibers: pending,
            services,
            listeners,
            events,
            contexts,
            lazy_uninitialized: lazy,
            history_records,
            history_bytes,
            estimated_bytes,
        }
    }

    pub fn budget(&self) -> MemoryBudget {
        *self.root_state().budget.read().unwrap()
    }

    /// Replace the harness memory budget (applies immediately).
    pub fn set_budget(&self, budget: MemoryBudget) {
        self.root_state()
            .bus
            .set_max_history(budget.max_event_history);
        *self.root_state().budget.write().unwrap() = budget;
    }

    /// Recent events retained for diagnostics (bounded by the budget).
    pub fn event_history(&self) -> Vec<HistoryRecord> {
        self.root_state().bus.history_snapshot()
    }

    pub(crate) fn mark_lazy_initialized(&self) {
        self.root_state()
            .lazy_uninitialized
            .fetch_sub(1, Ordering::SeqCst);
    }

    // ---- effects ---------------------------------------------------------

    /// Run `body` immediately and register the cleanup it produces on the
    /// current fiber (removed early by disposing the returned handle).
    pub async fn effect<B, Fut>(
        &self,
        label: &'static str,
        body: B,
    ) -> Result<Disposer, CordisError>
    where
        B: FnOnce(Context) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Cleanup, CordisError>> + Send + 'static,
    {
        self.check_alive()?;
        let root = self.root_state();
        let fiber = crate::fiber::current_fiber()
            .or_else(|| root.root_fiber.get().and_then(|weak| weak.upgrade()))
            .ok_or(CordisError::InactiveEffect)?;
        let cleanup = body(self.clone()).await?;
        let disposer = Disposer::new(cleanup);
        fiber.register_disposer(Arc::from(label), disposer.clone());
        Ok(disposer)
    }

    /// Register a cleanup directly on the current fiber.
    pub fn effect_fn(
        &self,
        label: &'static str,
        cleanup: Cleanup,
    ) -> Result<Disposer, CordisError> {
        self.check_alive()?;
        let root = self.root_state();
        let fiber = crate::fiber::current_fiber()
            .or_else(|| root.root_fiber.get().and_then(|weak| weak.upgrade()))
            .ok_or(CordisError::InactiveEffect)?;
        let disposer = Disposer::new(cleanup);
        fiber.register_disposer(Arc::from(label), disposer.clone());
        Ok(disposer)
    }

    // ---- plugins ---------------------------------------------------------

    /// Load a plugin in this context and return its fiber (loads once all
    /// declared dependencies are available).
    pub fn plugin<P: Plugin>(&self, plugin: P, config: Value) -> Result<Fiber, CordisError> {
        self.plugin_dyn(Arc::new(plugin), config)
    }

    /// Like `plugin`, for an already-erased plugin.
    pub fn plugin_dyn(&self, plugin: Arc<dyn Plugin>, config: Value) -> Result<Fiber, CordisError> {
        crate::registry::start_plugin(&self.root_state(), self, plugin, config)
    }

    /// Run a callback once the requested services are available; it is
    /// unloaded and re-run whenever a required service changes.
    pub fn inject<F, Fut>(&self, deps: &'static [&'static str], f: F) -> Result<Fiber, CordisError>
    where
        F: Fn(Context, Arc<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), CordisError>> + Send + 'static,
    {
        let plugin = crate::plugin::plugin_fn("inject", f).with_inject(deps);
        self.plugin(plugin, Value::Null)
    }

    /// Whether a plugin runtime of type `P` is registered.
    pub fn has_plugin_type<P: Plugin>(&self) -> bool {
        self.root_state()
            .registry
            .has_type(std::any::TypeId::of::<P>())
    }

    /// Dispose every fiber of plugin type `P` and remove its runtime.
    pub async fn unload_plugin<P: Plugin>(&self) -> Result<usize, CordisError> {
        self.check_alive()?;
        let fibers = self
            .root_state()
            .registry
            .delete_type(std::any::TypeId::of::<P>());
        for fiber in &fibers {
            crate::registry::dispose_fiber(fiber).await;
        }
        Ok(fibers.len())
    }

    /// Number of registered plugin runtimes.
    pub fn plugin_count(&self) -> usize {
        self.root_state().registry.runtime_count()
    }

    /// Visit every plugin runtime with its active fiber count.
    pub fn for_each_plugin(&self, f: impl FnMut(&str, usize)) {
        self.root_state().registry.for_each(f);
    }
}
