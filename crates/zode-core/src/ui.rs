//! UI plugin seam — the Rust analogue of DSH's client UI: every frontend
//! (TUI, headless, readline, app-server, browser panel, ...) is a harness
//! plugin mounted on the app context. The fiber owns the UI's runtime, so
//! mounting/disposing fibers swaps the UI at runtime.
//!
//! Protocol between a UI and the host:
//!
//! - A UI runs until its 'serve()' returns (its fiber settles).
//! - Before returning it may dispatch 'ui/swap' with payload
//!   '{"to": "<ui-id>"}' (use 'parallel_dyn' so the host records the
//!   handover before the frontend exits); the host unmounts the old fiber
//!   and mounts the new one.
//! - Anything the UI needs arrives through the context: 'UiDeps' is provided
//!   as the 'ui/deps' service, and UIs can register their own services,
//!   listeners, and effects — all owned by their fiber.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cordis_rs::{Context, CordisError, Fiber, Flow, Plugin, PluginResult};
use serde_json::{json, Value};

use crate::config::ZodeConfig;

/// What the harness hands to every mounted UI.
#[derive(Debug, Clone)]
pub struct UiDeps {
    /// Working directory the session started in.
    pub cwd: PathBuf,
    /// The effective configuration.
    pub cfg: ZodeConfig,
}

/// A pluggable frontend.
#[async_trait]
pub trait Ui: Send + Sync + 'static {
    /// Stable UI id, e.g. "tui", "headless", "readline", "server".
    fn id(&self) -> &'static str;

    /// Run the frontend until it exits. Returning (or erroring) ends this
    /// UI session; the owning fiber is then disposed by the host.
    async fn serve(&self, ctx: Context, deps: Arc<UiDeps>) -> Result<(), CordisError>;
}

/// Adapter mounting a [Ui] as a harness plugin: 'serve' is the plugin body,
/// and the deps come from the context service registry.
struct UiPlugin {
    ui: Arc<dyn Ui>,
}

#[async_trait]
impl Plugin for UiPlugin {
    fn name(&self) -> &'static str {
        self.ui.id()
    }

    async fn apply(&self, ctx: Context, _config: Arc<Value>) -> PluginResult {
        let deps = ctx.use_service::<UiDeps>("ui/deps")?;
        self.ui.serve(ctx, deps).await
    }
}

/// The app-level UI host: register frontends, mount one at a time, and
/// follow 'ui/swap' events to replace the active UI at runtime.
pub struct UiHost {
    ctx: Context,
    registry: Mutex<HashMap<&'static str, Arc<dyn Ui>>>,
    active: Mutex<Option<Fiber>>,
    swap_target: Arc<Mutex<Option<String>>>,
}

impl UiHost {
    /// Create the host on an app context (also provides the 'ui/deps'
    /// service).
    pub fn new(ctx: &Context, deps: Arc<UiDeps>) -> Result<Arc<Self>, CordisError> {
        let deps = deps.as_ref().clone();
        ctx.provide("ui/deps", deps)?;

        let swap_target: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        {
            let target = swap_target.clone();
            ctx.on_dyn("ui/swap", move |event| {
                let to = event
                    .payload
                    .get("to")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                if let Some(to) = to {
                    *target.lock().unwrap() = Some(to);
                }
                async { Flow::Continue }
            })?;
        }

        Ok(Arc::new(UiHost {
            ctx: ctx.clone(),
            registry: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            swap_target,
        }))
    }

    /// The context the UIs are mounted on (dispatch events here to reach
    /// the active frontend).
    pub fn ctx(&self) -> Context {
        self.ctx.clone()
    }

    /// Register a frontend under its id.
    pub fn register(&self, ui: Arc<dyn Ui>) {
        self.registry.lock().unwrap().insert(ui.id(), ui);
    }

    /// Ids of every registered frontend (sorted, deterministic).
    pub fn registered(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.registry.lock().unwrap().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// The id of the active frontend, if any.
    pub fn active_id(&self) -> Option<&'static str> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|fiber| fiber.name().to_string().leak() as &'static str)
    }

    /// Dispose the active frontend (tearing down its runtime) and mount a
    /// new one. Returns the new fiber (settled once the UI exits).
    pub async fn mount(&self, id: &str) -> Result<Fiber, CordisError> {
        self.unmount().await;
        let ui = self
            .registry
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CordisError::ServiceNotFound(format!("ui '{id}' is not registered")))?;
        let fiber = self.ctx.plugin(UiPlugin { ui }, json!({}))?;
        *self.active.lock().unwrap() = Some(fiber.clone());
        Ok(fiber)
    }

    /// Dispose the active frontend if one is mounted.
    pub async fn unmount(&self) {
        let fiber = self.active.lock().unwrap().take();
        if let Some(fiber) = fiber {
            fiber.dispose().await;
        }
    }

    /// Run the UI loop: mount 'id', wait for it to exit, and follow
    /// 'ui/swap' handovers until a frontend exits without one.
    pub async fn run(&self, id: &str) -> Result<(), CordisError> {
        let mut current = id.to_string();
        loop {
            let fiber = self.mount(&current).await?;
            // The fiber settles when the UI's serve() returns: that is the
            // frontend session ending.
            fiber.await_ready().await?;
            self.unmount().await;
            let next = self.swap_target.lock().unwrap().take();
            match next {
                Some(next) => current = next,
                None => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A test frontend that runs, records itself, then exits — optionally
    /// handing over to another UI via 'ui/swap'.
    struct ProbeUi {
        id: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
        swap_to: Option<&'static str>,
    }

    #[async_trait]
    impl Ui for ProbeUi {
        fn id(&self) -> &'static str {
            self.id
        }

        async fn serve(&self, ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
            self.log.lock().unwrap().push(self.id);
            if let Some(next) = self.swap_to {
                // parallel_dyn awaits the host's swap listener, so the
                // handover is recorded before this frontend exits.
                let _ = ctx.parallel_dyn("ui/swap", &json!({ "to": next })).await;
            }
            Ok(())
        }
    }

    fn deps() -> Arc<UiDeps> {
        Arc::new(UiDeps {
            cwd: PathBuf::from("/tmp"),
            cfg: ZodeConfig::default(),
        })
    }

    #[tokio::test]
    async fn mount_runs_and_swaps_at_runtime() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(&root, deps())?;
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        host.register(Arc::new(ProbeUi {
            id: "headless",
            log: log.clone(),
            swap_to: Some("readline"),
        }));
        host.register(Arc::new(ProbeUi {
            id: "readline",
            log: log.clone(),
            swap_to: None,
        }));

        assert!(host.active_id().is_none());
        host.run("headless").await?;

        // headless ran, handed over to readline, which exited without a
        // swap: the loop ended and the host is clean.
        assert_eq!(*log.lock().unwrap(), vec!["headless", "readline"]);
        assert!(host.active_id().is_none());
        assert_eq!(host.registered(), vec!["headless", "readline"]);
        Ok(())
    }

    #[tokio::test]
    async fn unknown_ui_is_rejected() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(&root, deps())?;
        let err = host.mount("web").await.unwrap_err();
        assert_eq!(err.code(), "SERVICE_NOT_FOUND");
        Ok(())
    }

    #[tokio::test]
    async fn unmount_disposes_the_active_ui() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(&root, deps())?;
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        host.register(Arc::new(ProbeUi {
            id: "headless",
            log,
            swap_to: None,
        }));
        let fiber = host.mount("headless").await?;
        assert!(host.active_id().is_some());
        host.unmount().await;
        assert!(host.active_id().is_none());
        assert_eq!(fiber.state(), cordis_rs::FiberState::Disposed);
        Ok(())
    }

    #[tokio::test]
    async fn mounting_a_new_ui_replaces_the_old_one() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(&root, deps())?;
        host.register(Arc::new(ProbeUi {
            id: "headless",
            log: Arc::new(Mutex::new(Vec::new())),
            swap_to: None,
        }));
        host.register(Arc::new(ProbeUi {
            id: "readline",
            log: Arc::new(Mutex::new(Vec::new())),
            swap_to: None,
        }));
        let first = host.mount("headless").await?;
        let second = host.mount("readline").await?;
        assert_ne!(first.id(), second.id());
        assert!(host.active_id().is_some());
        host.unmount().await;
        Ok(())
    }
}
