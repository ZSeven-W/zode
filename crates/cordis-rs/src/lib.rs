//! # cordis-rs
//!
//! A [Cordis](https://github.com/cordiverse/cordis)-inspired plugin harness
//! for Rust: scoped dependency containers, lifecycle-managed cleanup, and a
//! bounded memory budget. Everything — tools, services, event listeners,
//! forks — is a plugin, and everything a plugin acquires is freed when its
//! fiber disposes.
//!
//! ## Concepts (mirroring Cordis)
//!
//! | Cordis | cordis-rs |
//! |---|---|
//! | `new Context()` | `Context::root()` |
//! | `ctx.extend()` / `isolate()` / `intercept()` | same names |
//! | `ctx.plugin(p, config)` → thenable fiber | `ctx.plugin(p, config)` → `Fiber` (`await_ready()`) |
//! | `Service` class + `ctx.provide` | `ctx.provide` / `provide_lazy` + `use_service::<T>` |
//! | `ctx.on/emit/parallel/serial/bail/waterfall` | `on_dyn/emit_dyn/parallel_dyn/serial_dyn/bail_dyn/waterfall_dyn` |
//! | `ctx.effect()` | `ctx.effect()` / `effect_fn()` |
//! | `fiber.dispose()` | `fiber.dispose()` (async) |
//! | `inject` dependency scheduling | `ctx.inject(&["db"], cb)` |
//!
//! ## Memory control
//!
//! The Rust port adds explicit memory accounting on top of Cordis's dispose
//! semantics:
//!
//! - **Deterministic disposal**: every service, listener, and effect is owned
//!   by a fiber; `dispose()` (or root `dispose()`) frees them in reverse
//!   registration order. Dropping the root context drops every fiber, and
//!   `FiberInner::drop` runs sync cleanups, so registry state can never
//!   leak even without an explicit dispose.
//! - **Lazy services**: `provide_lazy` builds the value on first access —
//!   never-used dependencies are never allocated.
//! - **Budget caps**: `MemoryBudget` bounds fibers, pending fibers,
//!   services, listeners, and contexts; exceeding a cap fails with
//!   `BUDGET_EXCEEDED` instead of growing unbounded.
//! - **Bounded event history**: a ring buffer of recent events for
//!   diagnostics, truncated to `max_event_history`.
//! - **Observability**: `memory_stats()` reports live counts and a byte
//!   estimate.
//!
//! ## Example
//!
//! ```no_run
//! use cordis_rs::prelude::*;
//! use serde_json::json;
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), CordisError> {
//!     let root = Context::root();
//!     root.provide_lazy("counter", |_ctx| Ok(Arc::new(AtomicUsize::new(0))))?;
//!
//!     let fiber = root.plugin(
//!         plugin_fn("incrementer", |ctx, _config| async move {
//!             let counter = ctx.use_service::<AtomicUsize>("counter")?;
//!             ctx.on_dyn("app/ready", move |_event| {
//!                 counter.fetch_add(1, Ordering::SeqCst);
//!                 async { Flow::Continue }
//!             })?;
//!             Ok(())
//!         }),
//!         json!({}),
//!     )?;
//!     fiber.await_ready().await?;
//!
//!     root.emit_dyn("app/ready", &json!("started"))?;
//!     println!("{:?}", root.memory_stats());
//!
//!     root.dispose().await?; // frees every listener/service/fiber
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]

mod context;
mod error;
mod events;
mod evolution;
mod fiber;
mod logger;
mod memory;
mod plugin;
mod process;
mod registry;
mod service;
mod types;

pub use context::{Context, ContextId, WeakContext};
pub use error::CordisError;
pub use events::{Event, EventDef, EventName, Flow, HistoryRecord, Next, Payload};
pub use evolution::{Evolution, EvolutionConfig, Fitness, GeneRecord, Provenance};
pub use fiber::{Fiber, FiberId, FiberState};
pub use logger::Logger;
pub use memory::{MemoryBudget, MemoryStats};
pub use plugin::{plugin_fn, FunctionPlugin, Plugin, PluginResult};
pub use process::ProcessPlugin;
pub use types::{Cleanup, Disposer};

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        plugin_fn, Cleanup, Context, CordisError, Disposer, Event, EventDef, Evolution,
        EvolutionConfig, Fiber, FiberState, Fitness, Flow, GeneRecord, MemoryBudget, MemoryStats,
        Plugin, ProcessPlugin, Provenance,
    };
}
