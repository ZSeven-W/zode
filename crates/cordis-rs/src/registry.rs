//! Plugin registry: runtime records, dependency scheduling, and loading.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock, Weak};

use futures_util::FutureExt;
use serde_json::json;
use tokio::sync::watch;

use crate::context::{Context, RootInner};
use crate::error::CordisError;
use crate::fiber::{scope_fiber, Fiber, FiberInner, FiberState};
use crate::plugin::Plugin;
use crate::service::ScopeKey;
use crate::types::FiberId;

pub(crate) struct PluginRuntime {
    pub name: Arc<str>,
    pub fibers: Mutex<Vec<Weak<FiberInner>>>,
}

pub(crate) struct Registry {
    runtimes: RwLock<HashMap<TypeId, Arc<PluginRuntime>>>,
    pending: Mutex<Vec<Weak<FiberInner>>>,
}

impl Registry {
    pub(crate) fn new() -> Self {
        Registry {
            runtimes: RwLock::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn register_runtime(&self, plugin: &Arc<dyn Plugin>, fiber: &Arc<FiberInner>) {
        let mut runtimes = self.runtimes.write().unwrap();
        let runtime = runtimes.entry(plugin.runtime_type()).or_insert_with(|| {
            Arc::new(PluginRuntime {
                name: Arc::from(plugin.name()),
                fibers: Mutex::new(Vec::new()),
            })
        });
        runtime.fibers.lock().unwrap().push(Arc::downgrade(fiber));
    }

    pub(crate) fn runtime(&self, key: TypeId) -> Option<Arc<PluginRuntime>> {
        self.runtimes.read().unwrap().get(&key).cloned()
    }

    pub(crate) fn runtime_count(&self) -> usize {
        self.runtimes.read().unwrap().len()
    }

    pub(crate) fn has_type(&self, key: TypeId) -> bool {
        self.runtimes.read().unwrap().contains_key(&key)
    }

    /// Remove a plugin runtime and hand back its live fibers for disposal.
    pub(crate) fn delete_type(&self, key: TypeId) -> Vec<Arc<FiberInner>> {
        self.runtimes
            .write()
            .unwrap()
            .remove(&key)
            .map(|runtime| {
                runtime
                    .fibers
                    .lock()
                    .unwrap()
                    .iter()
                    .filter_map(|weak| weak.upgrade())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn remove_fiber(&self, fiber: &Arc<FiberInner>) {
        if let Some(runtime) = self.runtime(fiber.runtime_type) {
            runtime
                .fibers
                .lock()
                .unwrap()
                .retain(|weak| weak.upgrade().map(|f| f.id) != Some(fiber.id));
        }
        self.pending
            .lock()
            .unwrap()
            .retain(|weak| weak.upgrade().map(|f| f.id) != Some(fiber.id));
    }

    pub(crate) fn pending_len(&self) -> usize {
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|weak| weak.strong_count() > 0);
        pending.len()
    }

    pub(crate) fn clear(&self) {
        self.runtimes.write().unwrap().clear();
        self.pending.lock().unwrap().clear();
    }

    pub(crate) fn for_each(&self, mut f: impl FnMut(&str, usize)) {
        for runtime in self.runtimes.read().unwrap().values() {
            let active = runtime
                .fibers
                .lock()
                .unwrap()
                .iter()
                .filter_map(|weak| weak.upgrade())
                .filter(|fiber| {
                    matches!(
                        fiber.state(),
                        FiberState::Active | FiberState::Loading | FiberState::Unloading
                    )
                })
                .count();
            f(&runtime.name, active);
        }
    }
}

/// Emit an internal event from the root context (silently skipped once the
/// root context is gone).
pub(crate) fn emit_internal(root: &Arc<RootInner>, name: &'static str, payload: serde_json::Value) {
    if let Some(root_ctx) = root.root_ctx.get().and_then(|weak| weak.upgrade()) {
        let ctx = Context { inner: root_ctx };
        root.bus.emit(&ctx, name, Arc::new(payload));
    }
}

pub(crate) fn emit_status(root: &Arc<RootInner>, fiber: &Arc<FiberInner>, previous: FiberState) {
    emit_internal(
        root,
        "internal/status",
        json!({
            "fiber": fiber.id.0,
            "name": fiber.name.as_ref(),
            "state": fiber.state().as_str(),
            "previous": previous.as_str(),
        }),
    );
}

/// Start a plugin under `parent` and return its fiber (loads once all
/// declared dependencies are available).
pub(crate) fn start_plugin(
    root: &Arc<RootInner>,
    parent: &Context,
    plugin: Arc<dyn Plugin>,
    config: serde_json::Value,
) -> Result<Fiber, CordisError> {
    parent.check_alive()?;
    let root_ctx = root
        .root_ctx
        .get()
        .and_then(|weak| weak.upgrade())
        .ok_or(CordisError::ContextDisposed)?;
    plugin
        .validate(&config)
        .map_err(|e| CordisError::ConfigInvalid(plugin.name().to_string(), e.to_string()))?;
    let budget = *root.budget.read().unwrap();
    if root_ctx.fibers.lock().unwrap().len() >= budget.max_fibers {
        return Err(CordisError::BudgetExceeded("max_fibers"));
    }
    if root.registry.pending_len() >= budget.max_pending {
        return Err(CordisError::BudgetExceeded("max_pending"));
    }

    let name: Arc<str> = Arc::from(plugin.name());
    let ctx = parent.extend_inner(Some(name.clone()))?;
    let id = FiberId(root.fiber_counter.fetch_add(1, Ordering::SeqCst) + 1);
    let (state, _) = watch::channel(FiberState::Pending);
    let fiber = Arc::new(FiberInner {
        id,
        name: name.clone(),
        ctx,
        runtime_type: plugin.runtime_type(),
        plugin: Some(plugin.clone()),
        config: RwLock::new(Arc::new(config)),
        inject: plugin
            .inject()
            .iter()
            .map(|name| Arc::from(*name))
            .collect(),
        state,
        error: RwLock::new(None),
        disposers: Mutex::new(Vec::new()),
    });

    root.registry.register_runtime(&plugin, &fiber);
    root_ctx.fibers.lock().unwrap().push(fiber.clone());
    emit_internal(
        root,
        "internal/plugin",
        json!({ "fiber": id.0, "name": name.as_ref(), "phase": "created" }),
    );

    if deps_ready(root, &fiber) {
        spawn_load(root, &fiber)?;
    } else {
        root.registry
            .pending
            .lock()
            .unwrap()
            .push(Arc::downgrade(&fiber));
    }

    Ok(Fiber { inner: fiber })
}

pub(crate) fn spawn_load(
    root: &Arc<RootInner>,
    fiber: &Arc<FiberInner>,
) -> Result<(), CordisError> {
    let handle = tokio::runtime::Handle::try_current().map_err(|_| CordisError::NoRuntime)?;
    // Load THIS fiber's own plugin instance — never the shared runtime
    // record, which holds only the first instance of the type.
    let plugin = fiber
        .plugin
        .clone()
        .ok_or_else(|| CordisError::FiberDisposed(fiber.name.to_string()))?;
    let root = root.clone();
    let fiber = fiber.clone();
    handle.spawn(async move { run_load(&root, &fiber, &plugin).await });
    Ok(())
}

async fn run_load(root: &Arc<RootInner>, fiber: &Arc<FiberInner>, plugin: &Arc<dyn Plugin>) {
    let previous = fiber.set_state(FiberState::Loading);
    emit_status(root, fiber, previous);

    let outcome = scope_fiber(fiber, async {
        let future = plugin.apply(fiber.ctx.clone(), fiber.config.read().unwrap().clone());
        std::panic::AssertUnwindSafe(future).catch_unwind().await
    })
    .await;

    let (state, error) = match outcome {
        Ok(Ok(())) => (FiberState::Active, None),
        Ok(Err(err)) => (FiberState::Failed, Some(Arc::from(err.to_string()))),
        Err(_) => (
            FiberState::Failed,
            Some(Arc::from("plugin panicked during startup")),
        ),
    };
    if let Some(error) = error {
        *fiber.error.write().unwrap() = Some(error);
    }

    if matches!(fiber.state(), FiberState::Disposed | FiberState::Unloading) {
        // The fiber was disposed while the plugin was still starting: sweep
        // the effects it registered and finish disposal.
        for (_, disposer) in fiber.take_disposers().into_iter().rev() {
            disposer.dispose().await;
        }
        let previous = fiber.set_state(FiberState::Disposed);
        if previous != FiberState::Disposed {
            emit_status(root, fiber, previous);
        }
        return;
    }

    let previous = fiber.set_state(state);
    emit_status(root, fiber, previous);
}

/// Whether every injected service of `fiber` is available in its context.
pub(crate) fn deps_ready(root: &Arc<RootInner>, fiber: &Arc<FiberInner>) -> bool {
    fiber
        .inject
        .iter()
        .all(|name| service_available(root, &fiber.ctx, name))
}

pub(crate) fn service_available(root: &Arc<RootInner>, ctx: &Context, name: &str) -> bool {
    let key: ScopeKey = (ctx.isolation_label(name), Arc::from(name));
    match root.services.read().unwrap().get(&key) {
        Some(entry) => entry.check.is_none_or(|check| check(ctx)),
        None => false,
    }
}

/// Load pending fibers whose dependencies are now satisfied.
pub(crate) fn recheck_pending(root: &Arc<RootInner>) {
    let pending: Vec<Weak<FiberInner>> = root.registry.pending.lock().unwrap().clone();
    for weak in pending {
        let Some(fiber) = weak.upgrade() else {
            continue;
        };
        if deps_ready(root, &fiber) {
            root.registry
                .pending
                .lock()
                .unwrap()
                .retain(|w| w.upgrade().map(|f| f.id) != Some(fiber.id));
            let _ = spawn_load(root, &fiber);
        }
    }
}

/// A service was provided, replaced, or removed: unload fibers that depend
/// on it, then reload whatever became runnable.
pub(crate) fn on_service_changed(root: &Arc<RootInner>, name: &str) {
    let Some(root_ctx) = root.root_ctx.get().and_then(|weak| weak.upgrade()) else {
        return;
    };
    let dependents: Vec<Arc<FiberInner>> = root_ctx
        .fibers
        .lock()
        .unwrap()
        .iter()
        .filter(|fiber| fiber.inject.iter().any(|dep| dep.as_ref() == name))
        .cloned()
        .collect();
    drop(root_ctx);
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        for fiber in dependents {
            if matches!(fiber.state(), FiberState::Active | FiberState::Loading) {
                let root = root.clone();
                let fiber = fiber.clone();
                handle.spawn(async move { unload_to_pending(&root, &fiber).await });
            }
        }
    }
    recheck_pending(root);
}

/// Unload a dependent fiber back to `Pending` (it reloads when its
/// dependencies return).
pub(crate) async fn unload_to_pending(root: &Arc<RootInner>, fiber: &Arc<FiberInner>) {
    if !matches!(fiber.state(), FiberState::Active | FiberState::Loading) {
        return;
    }
    let previous = fiber.set_state(FiberState::Unloading);
    emit_status(root, fiber, previous);
    for (_, disposer) in fiber.take_disposers().into_iter().rev() {
        disposer.dispose().await;
    }
    let previous = fiber.set_state(FiberState::Pending);
    emit_status(root, fiber, previous);
    // Requeue the fiber so it reloads as soon as its dependencies return.
    root.registry
        .pending
        .lock()
        .unwrap()
        .push(Arc::downgrade(fiber));
    recheck_pending(root);
}

/// Permanently dispose a fiber: cleanup in reverse order, then detach from
/// the registry.
pub(crate) async fn dispose_fiber(fiber: &Arc<FiberInner>) {
    if fiber.state() == FiberState::Disposed {
        return;
    }
    let root = fiber.ctx.root_state();
    let previous = fiber.set_state(FiberState::Unloading);
    emit_status(&root, fiber, previous);
    for (_, disposer) in fiber.take_disposers().into_iter().rev() {
        disposer.dispose().await;
    }
    let previous = fiber.set_state(FiberState::Disposed);
    emit_status(&root, fiber, previous);
    emit_internal(
        &root,
        "internal/plugin",
        json!({ "fiber": fiber.id.0, "name": fiber.name.as_ref(), "phase": "disposed" }),
    );
    root.registry.remove_fiber(fiber);
    if let Some(root_ctx) = root.root_ctx.get().and_then(|weak| weak.upgrade()) {
        root_ctx.fibers.lock().unwrap().retain(|f| f.id != fiber.id);
    }
}

/// Unload and immediately reload a fiber with its current config.
pub(crate) async fn reload_fiber(fiber: &Arc<FiberInner>) -> Result<(), CordisError> {
    if fiber.state() == FiberState::Disposed {
        return Err(CordisError::FiberDisposed(fiber.name.to_string()));
    }
    let root = fiber.ctx.root_state();
    if root.disposed.load(Ordering::Acquire) {
        return Err(CordisError::ContextDisposed);
    }
    let previous = fiber.set_state(FiberState::Unloading);
    emit_status(&root, fiber, previous);
    for (_, disposer) in fiber.take_disposers().into_iter().rev() {
        disposer.dispose().await;
    }
    let previous = fiber.set_state(FiberState::Pending);
    emit_status(&root, fiber, previous);
    if deps_ready(&root, fiber) {
        spawn_load(&root, fiber)?;
    } else {
        root.registry
            .pending
            .lock()
            .unwrap()
            .push(Arc::downgrade(fiber));
    }
    Ok(())
}
