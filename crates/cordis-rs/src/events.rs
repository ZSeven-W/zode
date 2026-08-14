//! Event bus: fiber-owned listeners with five dispatch modes.
//!
//! Mirrors Cordis's event service: listeners registered through a context
//! are removed automatically when their owning fiber unloads, dispatch is
//! context-filtered (a listener fires for dispatches on its own context or
//! any descendant), and a bounded ring buffer retains recent events for
//! diagnostics without growing memory unboundedly.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::context::{Context, ContextId, RootInner};
use crate::error::CordisError;
use crate::fiber::current_fiber_or_root;
use crate::memory::MemoryBudget;
use crate::types::{Cleanup, Disposer};

/// Interned event name.
pub type EventName = Arc<str>;
/// Shared, serializable event payload.
pub type Payload = Arc<serde_json::Value>;

/// A dispatched event as seen by listeners.
#[derive(Debug, Clone)]
pub struct Event {
    /// Monotonic dispatch sequence number.
    pub seq: u64,
    pub name: EventName,
    pub payload: Payload,
}

/// Listener control flow.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow {
    Continue,
    Bail(serde_json::Value),
}

impl Flow {
    pub fn is_bail(&self) -> bool {
        matches!(self, Flow::Bail(_))
    }

    pub fn into_bail(self) -> Option<serde_json::Value> {
        match self {
            Flow::Bail(value) => Some(value),
            Flow::Continue => None,
        }
    }

    /// Build a bail value from any serializable type.
    pub fn bail_typed(value: impl Serialize) -> Flow {
        Flow::Bail(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))
    }
}

/// Typed event definition: a stable name plus payload and return types.
///
/// Example:
///
/// ```ignore
/// struct AppReady;
/// impl EventDef for AppReady {
///     const NAME: &'static str = "app/ready";
///     type Payload = String;
///     type Return = String;
/// }
/// ```
pub trait EventDef: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Serialize + DeserializeOwned;
    type Return: Serialize + DeserializeOwned;
}

/// The continuation of a waterfall chain.
#[derive(Clone)]
pub struct Next(Arc<dyn Fn() -> BoxFuture<'static, Flow> + Send + Sync>);

impl Next {
    /// Invoke the rest of the chain (subsequent listeners, then the built-in
    /// behavior). A listener that does not call this vetoes the rest.
    pub async fn call(&self) -> Flow {
        (self.0)().await
    }
}

/// One entry of the bounded event history.
#[derive(Debug, Clone)]
pub struct HistoryRecord {
    pub seq: u64,
    pub name: EventName,
    pub payload: Payload,
}

type ListenerFn = Box<dyn Fn(&Event) -> BoxFuture<'static, Flow> + Send + Sync>;
type WaterfallFn = Box<dyn Fn(Event, Next) -> BoxFuture<'static, Flow> + Send + Sync>;

pub(crate) struct Hook {
    pub ctx_id: ContextId,
    pub global: bool,
    pub callback: Arc<ListenerFn>,
}

pub(crate) struct WaterfallHook {
    pub ctx_id: ContextId,
    pub global: bool,
    pub callback: Arc<WaterfallFn>,
}

pub(crate) struct EventBus {
    hooks: RwLock<HashMap<EventName, Vec<Arc<Hook>>>>,
    waterfall_hooks: RwLock<HashMap<EventName, Vec<Arc<WaterfallHook>>>>,
    history: RwLock<VecDeque<HistoryRecord>>,
    history_bytes: AtomicUsize,
    max_history: AtomicUsize,
    seq: AtomicU64,
}

impl EventBus {
    pub(crate) fn new() -> Self {
        EventBus {
            hooks: RwLock::new(HashMap::new()),
            waterfall_hooks: RwLock::new(HashMap::new()),
            history: RwLock::new(VecDeque::new()),
            history_bytes: AtomicUsize::new(0),
            max_history: AtomicUsize::new(MemoryBudget::default().max_event_history),
            seq: AtomicU64::new(1),
        }
    }

    pub(crate) fn set_max_history(&self, n: usize) {
        self.max_history.store(n, Ordering::Release);
        self.truncate_history();
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn record(&self, seq: u64, name: EventName, payload: Payload) {
        let bytes = name.len() + payload.to_string().len();
        self.history
            .write()
            .unwrap()
            .push_back(HistoryRecord { seq, name, payload });
        self.history_bytes.fetch_add(bytes, Ordering::SeqCst);
        self.truncate_history();
    }

    fn truncate_history(&self) {
        let mut history = self.history.write().unwrap();
        while history.len() > self.max_history.load(Ordering::Acquire) {
            if let Some(old) = history.pop_front() {
                let bytes = old.name.len() + old.payload.to_string().len();
                self.history_bytes.fetch_sub(bytes, Ordering::SeqCst);
            }
        }
    }

    fn matches(ctx_id: ContextId, global: bool, ctx: &Context) -> bool {
        global || ctx.inner.ancestor_ids.contains(&ctx_id)
    }

    pub(crate) fn register(
        &self,
        root: &Arc<RootInner>,
        ctx: &Context,
        name: &str,
        callback: ListenerFn,
        global: bool,
    ) -> Result<Disposer, CordisError> {
        let budget = *root.budget.read().unwrap();
        if self.listener_count() >= budget.max_listeners {
            return Err(CordisError::BudgetExceeded("max_listeners"));
        }
        let hook = Arc::new(Hook {
            ctx_id: ctx.id(),
            global,
            callback: Arc::new(callback),
        });
        let name: EventName = Arc::from(name);
        self.hooks
            .write()
            .unwrap()
            .entry(name.clone())
            .or_default()
            .push(hook.clone());
        let root = Arc::downgrade(root);
        Ok(Disposer::new(Cleanup::sync(move || {
            if let Some(root) = root.upgrade() {
                let mut map = root.bus.hooks.write().unwrap();
                if let Some(list) = map.get_mut(&name) {
                    list.retain(|h| !Arc::ptr_eq(h, &hook));
                    if list.is_empty() {
                        map.remove(&name);
                    }
                }
            }
        })))
    }

    pub(crate) fn register_waterfall(
        &self,
        root: &Arc<RootInner>,
        ctx: &Context,
        name: &str,
        callback: WaterfallFn,
        global: bool,
    ) -> Result<Disposer, CordisError> {
        let budget = *root.budget.read().unwrap();
        if self.listener_count() >= budget.max_listeners {
            return Err(CordisError::BudgetExceeded("max_listeners"));
        }
        let hook = Arc::new(WaterfallHook {
            ctx_id: ctx.id(),
            global,
            callback: Arc::new(callback),
        });
        let name: EventName = Arc::from(name);
        self.waterfall_hooks
            .write()
            .unwrap()
            .entry(name.clone())
            .or_default()
            .push(hook.clone());
        let root = Arc::downgrade(root);
        Ok(Disposer::new(Cleanup::sync(move || {
            if let Some(root) = root.upgrade() {
                let mut map = root.bus.waterfall_hooks.write().unwrap();
                if let Some(list) = map.get_mut(&name) {
                    list.retain(|h| !Arc::ptr_eq(h, &hook));
                    if list.is_empty() {
                        map.remove(&name);
                    }
                }
            }
        })))
    }

    fn resolve(&self, ctx: &Context, name: &EventName) -> Vec<Arc<Hook>> {
        self.hooks
            .read()
            .unwrap()
            .get(name)
            .map(|list| {
                list.iter()
                    .filter(|hook| Self::matches(hook.ctx_id, hook.global, ctx))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn resolve_waterfall(&self, ctx: &Context, name: &EventName) -> Vec<Arc<WaterfallHook>> {
        self.waterfall_hooks
            .read()
            .unwrap()
            .get(name)
            .map(|list| {
                list.iter()
                    .filter(|hook| Self::matches(hook.ctx_id, hook.global, ctx))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Fire-and-forget dispatch: listeners run on spawned tasks, results are
    /// ignored (mirrors Cordis `emit`).
    pub(crate) fn emit(&self, ctx: &Context, name: &str, payload: Payload) {
        let name: EventName = Arc::from(name);
        let seq = self.next_seq();
        self.record(seq, name.clone(), payload.clone());
        let event = Event { seq, name, payload };
        for hook in self.resolve(ctx, &event.name) {
            let fut = (hook.callback)(&event);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(fut);
            }
        }
    }

    /// Run all listeners concurrently and await them (results ignored).
    pub(crate) async fn parallel(&self, ctx: &Context, name: &str, payload: Payload) {
        let name: EventName = Arc::from(name);
        let seq = self.next_seq();
        self.record(seq, name.clone(), payload.clone());
        let event = Event { seq, name, payload };
        let futures: Vec<_> = self
            .resolve(ctx, &event.name)
            .iter()
            .map(|hook| (hook.callback)(&event))
            .collect();
        let _ = futures_util::future::join_all(futures).await;
    }

    /// Await listeners in order, running all of them, and return the first
    /// bail value (mirrors Cordis `serial`).
    pub(crate) async fn serial(
        &self,
        ctx: &Context,
        name: &str,
        payload: Payload,
    ) -> Option<serde_json::Value> {
        let name: EventName = Arc::from(name);
        let seq = self.next_seq();
        self.record(seq, name.clone(), payload.clone());
        let event = Event { seq, name, payload };
        let mut result = None;
        for hook in self.resolve(ctx, &event.name) {
            if let Flow::Bail(value) = (hook.callback)(&event).await {
                if result.is_none() {
                    result = Some(value);
                }
            }
        }
        result
    }

    /// Await listeners in order and stop at the first bail (mirrors Cordis
    /// `bail`).
    pub(crate) async fn bail(
        &self,
        ctx: &Context,
        name: &str,
        payload: Payload,
    ) -> Option<serde_json::Value> {
        let name: EventName = Arc::from(name);
        let seq = self.next_seq();
        self.record(seq, name.clone(), payload.clone());
        let event = Event { seq, name, payload };
        for hook in self.resolve(ctx, &event.name) {
            if let Flow::Bail(value) = (hook.callback)(&event).await {
                return Some(value);
            }
        }
        None
    }

    /// Compose waterfall listeners around the built-in behavior (mirrors
    /// Cordis `waterfall`).
    pub(crate) async fn waterfall<F>(
        &self,
        ctx: &Context,
        name: &str,
        payload: Payload,
        inner: F,
    ) -> Flow
    where
        F: FnOnce(&Event) -> Flow + Send + 'static,
    {
        let name: EventName = Arc::from(name);
        let seq = self.next_seq();
        self.record(seq, name.clone(), payload.clone());
        let event = Arc::new(Event { seq, name, payload });
        let hooks = self.resolve_waterfall(ctx, &event.name);

        let inner = Arc::new(std::sync::Mutex::new(Some(inner)));
        let first_event = event.clone();
        let mut next: Box<dyn Fn() -> BoxFuture<'static, Flow> + Send + Sync> =
            Box::new(move || {
                let inner = inner.clone();
                let event = first_event.clone();
                Box::pin(async move {
                    match inner.lock().unwrap().take() {
                        Some(f) => f(&event),
                        None => Flow::Continue,
                    }
                })
            });

        for hook in hooks.iter().rev() {
            let hook = hook.clone();
            let event = event.clone();
            let previous =
                std::mem::replace(&mut next, Box::new(|| Box::pin(async { Flow::Continue })));
            let previous = Arc::new(std::sync::Mutex::new(Some(previous)));
            next = Box::new(move || {
                let event = event.clone();
                let previous = previous.clone();
                let callback = hook.callback.clone();
                Box::pin(async move {
                    let inner_next = Next(Arc::new(move || {
                        let previous = previous.clone();
                        Box::pin(async move {
                            // Drop the lock guard before awaiting (Send).
                            let taken = previous.lock().unwrap().take();
                            match taken {
                                Some(f) => f().await,
                                None => Flow::Continue,
                            }
                        })
                    }));
                    (callback)((*event).clone(), inner_next).await
                })
            });
        }

        next().await
    }

    pub(crate) fn stats(&self) -> (usize, usize, usize, usize) {
        let hooks = self.hooks.read().unwrap();
        let waterfall = self.waterfall_hooks.read().unwrap();
        let listeners = hooks.values().map(|v| v.len()).sum::<usize>()
            + waterfall.values().map(|v| v.len()).sum::<usize>();
        let mut names: HashSet<EventName> = HashSet::new();
        names.extend(hooks.keys().cloned());
        names.extend(waterfall.keys().cloned());
        let history = self.history.read().unwrap();
        (
            listeners,
            names.len(),
            history.len(),
            self.history_bytes.load(Ordering::Acquire),
        )
    }

    pub(crate) fn history_snapshot(&self) -> Vec<HistoryRecord> {
        self.history.read().unwrap().iter().cloned().collect()
    }

    pub(crate) fn listener_count(&self) -> usize {
        self.hooks
            .read()
            .unwrap()
            .values()
            .map(|v| v.len())
            .sum::<usize>()
            + self
                .waterfall_hooks
                .read()
                .unwrap()
                .values()
                .map(|v| v.len())
                .sum::<usize>()
    }

    pub(crate) fn clear(&self) {
        self.hooks.write().unwrap().clear();
        self.waterfall_hooks.write().unwrap().clear();
        self.history.write().unwrap().clear();
        self.history_bytes.store(0, Ordering::SeqCst);
    }
}

impl Context {
    /// Register a dynamic listener (removed automatically with its fiber).
    pub fn on_dyn<F, Fut>(&self, name: &str, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.check_alive()?;
        let root = self.root_state();
        let fiber = current_fiber_or_root(self).ok_or(CordisError::InactiveEffect)?;
        let disposer = root.bus.register(
            &root,
            self,
            name,
            Box::new(move |event| Box::pin(f(event)) as BoxFuture<'static, Flow>),
            false,
        )?;
        fiber.register_disposer(Arc::from(format!("on:{name}")), disposer.clone());
        Ok(disposer)
    }

    /// Like `on_dyn`, but the listener fires regardless of context filters.
    pub fn on_dyn_global<F, Fut>(&self, name: &str, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.check_alive()?;
        let root = self.root_state();
        let fiber = current_fiber_or_root(self).ok_or(CordisError::InactiveEffect)?;
        let disposer = root.bus.register(
            &root,
            self,
            name,
            Box::new(move |event| Box::pin(f(event)) as BoxFuture<'static, Flow>),
            true,
        )?;
        fiber.register_disposer(Arc::from(format!("on:{name}")), disposer.clone());
        Ok(disposer)
    }

    /// Register a dynamic waterfall listener.
    pub fn on_waterfall_dyn<F, Fut>(&self, name: &str, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(Event, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.check_alive()?;
        let root = self.root_state();
        let fiber = current_fiber_or_root(self).ok_or(CordisError::InactiveEffect)?;
        let disposer = root.bus.register_waterfall(
            &root,
            self,
            name,
            Box::new(move |event, next| Box::pin(f(event, next)) as BoxFuture<'static, Flow>),
            false,
        )?;
        fiber.register_disposer(Arc::from(format!("on:{name}")), disposer.clone());
        Ok(disposer)
    }

    /// Like `on_dyn`, but the listener removes itself after its first call.
    pub fn once_dyn<F, Fut>(&self, name: &str, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&Event) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        let slot: Arc<std::sync::Mutex<Option<F>>> = Arc::new(std::sync::Mutex::new(Some(f)));
        let remover: Arc<std::sync::Mutex<Option<Disposer>>> =
            Arc::new(std::sync::Mutex::new(None));
        let disposer = self.on_dyn(name, {
            let slot = slot.clone();
            let remover = remover.clone();
            move |event: &Event| {
                let callback = slot.lock().unwrap().take();
                match callback {
                    Some(callback) => {
                        if let Some(remover) = remover.lock().unwrap().take() {
                            remover.dispose_bg();
                        }
                        Box::pin(callback(event)) as BoxFuture<'static, Flow>
                    }
                    None => Box::pin(async { Flow::Continue }),
                }
            }
        })?;
        *remover.lock().unwrap() = Some(disposer.clone());
        Ok(disposer)
    }

    /// Fire-and-forget dynamic dispatch (mirrors Cordis `emit`).
    pub fn emit_dyn(&self, name: &str, payload: &impl Serialize) -> Result<(), CordisError> {
        self.check_alive()?;
        let payload =
            serde_json::to_value(payload).map_err(|e| CordisError::Payload(e.to_string()))?;
        self.root_state().bus.emit(self, name, Arc::new(payload));
        Ok(())
    }

    /// Concurrent dynamic dispatch; awaits every listener (mirrors
    /// `parallel`).
    pub async fn parallel_dyn(
        &self,
        name: &str,
        payload: &impl Serialize,
    ) -> Result<(), CordisError> {
        self.check_alive()?;
        let payload =
            serde_json::to_value(payload).map_err(|e| CordisError::Payload(e.to_string()))?;
        self.root_state()
            .bus
            .parallel(self, name, Arc::new(payload))
            .await;
        Ok(())
    }

    /// Sequential dynamic dispatch; runs every listener, returns the first
    /// bail value.
    pub async fn serial_dyn(
        &self,
        name: &str,
        payload: &impl Serialize,
    ) -> Result<Option<serde_json::Value>, CordisError> {
        self.check_alive()?;
        let payload =
            serde_json::to_value(payload).map_err(|e| CordisError::Payload(e.to_string()))?;
        Ok(self
            .root_state()
            .bus
            .serial(self, name, Arc::new(payload))
            .await)
    }

    /// Sequential dynamic dispatch; stops at the first bail value.
    pub async fn bail_dyn(
        &self,
        name: &str,
        payload: &impl Serialize,
    ) -> Result<Option<serde_json::Value>, CordisError> {
        self.check_alive()?;
        let payload =
            serde_json::to_value(payload).map_err(|e| CordisError::Payload(e.to_string()))?;
        Ok(self
            .root_state()
            .bus
            .bail(self, name, Arc::new(payload))
            .await)
    }

    /// Dynamic waterfall dispatch around a built-in behavior.
    pub async fn waterfall_dyn<F>(
        &self,
        name: &str,
        payload: &impl Serialize,
        inner: F,
    ) -> Result<Flow, CordisError>
    where
        F: FnOnce(&Event) -> Flow + Send + 'static,
    {
        self.check_alive()?;
        let payload =
            serde_json::to_value(payload).map_err(|e| CordisError::Payload(e.to_string()))?;
        Ok(self
            .root_state()
            .bus
            .waterfall(self, name, Arc::new(payload), inner)
            .await)
    }

    /// Register a typed listener (payload deserialized per dispatch).
    pub fn on_t<E: EventDef, F, Fut>(&self, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&E::Payload) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.on_dyn(E::NAME, move |event| {
            match serde_json::from_value::<E::Payload>((*event.payload).clone()) {
                Ok(payload) => Box::pin(f(&payload)) as BoxFuture<'static, Flow>,
                Err(error) => {
                    tracing::warn!(
                        target: "cordis",
                        "dropping listener for typed event `{}`: {}",
                        E::NAME,
                        error
                    );
                    Box::pin(async { Flow::Continue })
                }
            }
        })
    }

    /// Typed one-shot listener.
    pub fn once_t<E: EventDef, F, Fut>(&self, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&E::Payload) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.once_dyn(E::NAME, move |event| {
            match serde_json::from_value::<E::Payload>((*event.payload).clone()) {
                Ok(payload) => Box::pin(f(&payload)) as BoxFuture<'static, Flow>,
                Err(error) => {
                    tracing::warn!(
                        target: "cordis",
                        "dropping listener for typed event `{}`: {}",
                        E::NAME,
                        error
                    );
                    Box::pin(async { Flow::Continue })
                }
            }
        })
    }

    /// Register a typed waterfall listener.
    pub fn on_waterfall_t<E: EventDef, F, Fut>(&self, f: F) -> Result<Disposer, CordisError>
    where
        F: Fn(&E::Payload, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Flow> + Send + 'static,
    {
        self.on_waterfall_dyn(E::NAME, move |event, next| {
            match serde_json::from_value::<E::Payload>((*event.payload).clone()) {
                Ok(payload) => Box::pin(f(&payload, next)) as BoxFuture<'static, Flow>,
                Err(error) => {
                    tracing::warn!(
                        target: "cordis",
                        "dropping listener for typed event `{}`: {}",
                        E::NAME,
                        error
                    );
                    Box::pin(async { Flow::Continue })
                }
            }
        })
    }

    /// Typed fire-and-forget dispatch.
    pub fn emit_t<E: EventDef>(&self, payload: &E::Payload) -> Result<(), CordisError> {
        self.emit_dyn(E::NAME, payload)
    }

    /// Typed concurrent dispatch.
    pub async fn parallel_t<E: EventDef>(&self, payload: &E::Payload) -> Result<(), CordisError> {
        self.parallel_dyn(E::NAME, payload).await
    }

    /// Typed sequential dispatch; returns the first typed bail value.
    pub async fn serial_t<E: EventDef>(
        &self,
        payload: &E::Payload,
    ) -> Result<Option<E::Return>, CordisError> {
        self.serial_dyn(E::NAME, payload)
            .await?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| CordisError::Payload(e.to_string()))
    }

    /// Typed dispatch stopping at the first typed bail value.
    pub async fn bail_t<E: EventDef>(
        &self,
        payload: &E::Payload,
    ) -> Result<Option<E::Return>, CordisError> {
        self.bail_dyn(E::NAME, payload)
            .await?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| CordisError::Payload(e.to_string()))
    }

    /// Typed waterfall dispatch.
    pub async fn waterfall_t<E: EventDef, F>(
        &self,
        payload: &E::Payload,
        inner: F,
    ) -> Result<Flow, CordisError>
    where
        F: FnOnce(&E::Payload) -> Flow + Send + 'static,
    {
        self.waterfall_dyn(
            E::NAME,
            payload,
            move |event| match serde_json::from_value::<E::Payload>((*event.payload).clone()) {
                Ok(payload) => inner(&payload),
                Err(_) => Flow::Continue,
            },
        )
        .await
    }
}
