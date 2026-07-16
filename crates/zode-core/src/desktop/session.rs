//! DesktopSession: process-wide target registry, per-target leases, and the
//! permission ladder. Reads are NOT un-gated ReadOnly — the ladder is
//! subsystem-consent → app allowlist → per-family scoped grants (spec §权限).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::config::DesktopConfig;

use super::actor::ActorHandle;
use super::backend::{
    AppId, DesktopBackend, DesktopBackendFactory, DesktopError, WindowId,
};

/// Default per-session actor replacement cap before the breaker trips.
const DEFAULT_MAX_REPLACEMENTS: u64 = 3;

/// Mutating action families; each has an independent always-allow grant so
/// allowing one click never silently allows launch or raw keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionFamily {
    Element,
    RawInput,
    Launch,
}

impl ActionFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionFamily::Element => "element",
            ActionFamily::RawInput => "raw-input",
            ActionFamily::Launch => "launch",
        }
    }
}

/// The permission ladder state for one session. Consent, allowlist, and grants
/// are all session-scoped and never persist across sessions.
#[derive(Debug, Default)]
pub struct PermissionScopes {
    consented: AtomicBool,
    allowed_apps: StdMutex<HashSet<String>>,
    grants: StdMutex<HashSet<(String, ActionFamily)>>,
}

impl PermissionScopes {
    pub fn subsystem_consented(&self) -> bool {
        self.consented.load(Ordering::SeqCst)
    }
    pub fn grant_subsystem(&self) {
        self.consented.store(true, Ordering::SeqCst);
    }

    pub fn is_app_allowed(&self, exe: &str) -> bool {
        self.allowed_apps.lock().unwrap().contains(exe)
    }
    pub fn allow_app(&self, exe: &str) {
        self.allowed_apps.lock().unwrap().insert(exe.to_string());
    }

    pub fn is_granted(&self, exe: &str, family: ActionFamily) -> bool {
        self.grants
            .lock()
            .unwrap()
            .contains(&(exe.to_string(), family))
    }
    pub fn grant(&self, exe: &str, family: ActionFamily) {
        self.grants.lock().unwrap().insert((exe.to_string(), family));
    }
}

#[derive(Debug)]
pub struct DesktopSession {
    cfg: DesktopConfig,
    actor: Arc<ActorHandle>,
    scopes: PermissionScopes,
    slot: Arc<tokio::sync::Mutex<()>>,
    perm_flags: StdMutex<Vec<(String, Arc<AtomicBool>)>>,
    window_seq: AtomicU64,
}

/// Holding a lease holds the global input/slot mutex: backend ops through a
/// lease are serialized across all concurrent tool calls (M1 single actor).
pub struct TargetLease {
    backend: Arc<dyn DesktopBackend>,
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

impl TargetLease {
    pub fn backend(&self) -> Arc<dyn DesktopBackend> {
        self.backend.clone()
    }
}

impl DesktopSession {
    pub fn new(cfg: DesktopConfig, factory: Arc<dyn DesktopBackendFactory>) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            actor: ActorHandle::new(factory, DEFAULT_MAX_REPLACEMENTS),
            scopes: PermissionScopes::default(),
            slot: Arc::new(tokio::sync::Mutex::new(())),
            perm_flags: StdMutex::new(Vec::new()),
            window_seq: AtomicU64::new(0),
        })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled()
    }
    pub fn scopes(&self) -> &PermissionScopes {
        &self.scopes
    }
    pub fn snapshot_max_nodes(&self) -> usize {
        self.cfg.snapshot_max_nodes()
    }

    pub async fn lease(self: &Arc<Self>) -> Result<TargetLease, DesktopError> {
        let guard = self.slot.clone().lock_owned().await;
        let backend = self.actor.backend().await?;
        Ok(TargetLease {
            backend,
            _guard: guard,
        })
    }

    /// Build a generation-bound AppId for an executable identity. M1: pid and
    /// start-time are placeholders filled by the platform backend later; the
    /// actor generation is stamped so ids fail after an actor replacement.
    pub fn resolve_app(&self, exe: &str) -> AppId {
        AppId::new(0, 0, exe.to_string(), self.actor.generation())
    }

    /// Resolve a model-supplied window token into a generation-bound WindowId.
    /// M1: the mock/AX backend addresses windows by an opaque token; the id is
    /// stamped with the current actor generation so it fails as StaleRef after
    /// an actor replacement. Empty token is rejected (never a silent default).
    pub fn resolve_window(&self, app: AppId, token: &str) -> Result<WindowId, DesktopError> {
        if token.is_empty() {
            return Err(DesktopError::NotFound("window token required".into()));
        }
        let key = self.window_seq.fetch_add(1, Ordering::SeqCst);
        Ok(WindowId::new(app, key, 0, self.actor.generation()))
    }

    pub async fn actor_request_replacement(&self) {
        self.actor.request_replacement().await;
    }
    pub fn actor_reset(&self) {
        self.actor.reset();
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

    pub async fn status_lines(&self) -> Vec<String> {
        let flags = self.perm_flags();
        let grants: Vec<String> = flags
            .iter()
            .filter(|(_, f)| f.load(Ordering::Relaxed))
            .map(|(n, _)| n.clone())
            .collect();
        vec![
            format!("desktop: enabled={}", self.enabled()),
            format!("consented={}", self.scopes.subsystem_consented()),
            format!(
                "actor_generation={} tripped={}",
                self.actor.generation(),
                self.actor.is_tripped()
            ),
            format!("always_allow={}", if grants.is_empty() { "none".into() } else { grants.join(",") }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::mock::{mock_factory, MockDesktopFactory};

    #[test]
    fn scopes_ladder_is_independent_per_family() {
        let s = PermissionScopes::default();
        assert!(!s.subsystem_consented());
        s.grant_subsystem();
        assert!(s.subsystem_consented());

        assert!(!s.is_app_allowed("com.apple.TextEdit"));
        s.allow_app("com.apple.TextEdit");
        assert!(s.is_app_allowed("com.apple.TextEdit"));
        assert!(!s.is_app_allowed("com.other"));

        // granting Element must NOT grant RawInput or Launch
        s.grant("com.apple.TextEdit", ActionFamily::Element);
        assert!(s.is_granted("com.apple.TextEdit", ActionFamily::Element));
        assert!(!s.is_granted("com.apple.TextEdit", ActionFamily::RawInput));
        assert!(!s.is_granted("com.apple.TextEdit", ActionFamily::Launch));
    }

    fn test_win() -> WindowId {
        let a = AppId::new(1, 1, "com.test".into(), 0);
        WindowId::new(a, 1, 1, 0)
    }

    #[tokio::test]
    async fn lease_serializes_and_reuses_backend() {
        let f = Arc::new(MockDesktopFactory::default());
        let s = DesktopSession::new(DesktopConfig::default(), f.clone());
        let mut joins = Vec::new();
        for _ in 0..6 {
            let s = s.clone();
            joins.push(tokio::spawn(async move {
                let l = s.lease().await.unwrap();
                l.backend().snapshot(&test_win(), None).await.unwrap();
            }));
        }
        for j in joins {
            j.await.unwrap();
        }
        assert_eq!(f.made.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn tripped_actor_lease_is_dead_until_reset() {
        let s = DesktopSession::new(DesktopConfig::default(), mock_factory());
        s.lease().await.unwrap();
        for _ in 0..3 {
            s.actor_request_replacement().await;
        }
        assert!(matches!(s.lease().await, Err(DesktopError::Dead(_))));
        s.actor_reset();
        assert!(s.lease().await.is_ok());
    }

    #[test]
    fn resolve_window_rejects_empty_token() {
        let s = DesktopSession::new(DesktopConfig::default(), mock_factory());
        let app = AppId::new(1, 1, "com.test".into(), 0);
        assert!(s.resolve_window(app.clone(), "").is_err());
        assert!(s.resolve_window(app, "w1").is_ok());
    }
}
