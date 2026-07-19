//! DesktopSession: process-wide target registry, per-target leases, and the
//! permission ladder. Reads are NOT un-gated ReadOnly — the ladder is
//! subsystem-consent → app allowlist → per-family scoped grants (spec §权限).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::config::DesktopConfig;

use super::actor::ActorHandle;
use super::backend::{AppId, DesktopBackend, DesktopBackendFactory, DesktopError, WindowId};

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
        self.grants
            .lock()
            .unwrap()
            .insert((exe.to_string(), family));
    }
}

#[derive(Debug)]
pub struct DesktopSession {
    cfg: DesktopConfig,
    actor: Arc<ActorHandle>,
    scopes: PermissionScopes,
    slot: Arc<tokio::sync::Mutex<()>>,
    perm_flags: StdMutex<Vec<(String, Arc<AtomicBool>)>>,
    /// Optional CDP attachment (an Electron/Chromium instance attached over its
    /// debug port). When present, `desktop_eval` and CDP-routed calls use it.
    cdp: tokio::sync::Mutex<Option<Arc<super::cdp::CdpBackend>>>,
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

/// The backend chosen for a given target app: the CDP attachment (for apps
/// addressed by a `cdp#…` identity) or the platform backend (AX/UIA/AT-SPI2,
/// held behind the global input lease). Unifies dispatch for the tools.
pub enum ResolvedBackend {
    /// Platform backend held behind the serializing input lease.
    Platform(TargetLease),
    /// CDP-attached Electron/Chromium instance (no input lease needed).
    Cdp(Arc<super::cdp::CdpBackend>),
}

impl ResolvedBackend {
    pub fn backend(&self) -> Arc<dyn DesktopBackend> {
        match self {
            ResolvedBackend::Platform(l) => l.backend(),
            ResolvedBackend::Cdp(c) => c.clone(),
        }
    }
}

/// An executable identity refers to the CDP attachment when it starts `cdp`.
pub fn is_cdp_identity(exe: &str) -> bool {
    exe.starts_with("cdp")
}

impl DesktopSession {
    pub fn new(cfg: DesktopConfig, factory: Arc<dyn DesktopBackendFactory>) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            actor: ActorHandle::new(factory, DEFAULT_MAX_REPLACEMENTS),
            scopes: PermissionScopes::default(),
            slot: Arc::new(tokio::sync::Mutex::new(())),
            perm_flags: StdMutex::new(Vec::new()),
            cdp: tokio::sync::Mutex::new(None),
        })
    }

    /// Attach a CDP backend to a running Electron/Chromium debug port (loopback
    /// only). Replaces any existing attachment. Enables `desktop_eval`.
    pub async fn attach_cdp(&self, port: u16) -> Result<(), DesktopError> {
        let backend = super::cdp::CdpBackend::attach(port).await?;
        *self.cdp.lock().await = Some(backend);
        Ok(())
    }

    /// The current CDP attachment, if any.
    pub async fn cdp(&self) -> Option<Arc<super::cdp::CdpBackend>> {
        self.cdp.lock().await.clone()
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
        if self.cfg.esc_cancel() {
            crate::desktop::esc_watch::arm();
        }
        let guard = self.slot.clone().lock_owned().await;
        let backend = self.actor.backend().await?;
        Ok(TargetLease {
            backend,
            _guard: guard,
        })
    }

    /// Choose the backend for a target app: the CDP attachment when the app is
    /// addressed by a `cdp#…` identity and one is attached, else the platform
    /// backend (behind the input lease). This is how `desktop_read`/`desktop_act`
    /// route Electron apps to CDP and native apps to AX/UIA/AT-SPI2.
    pub async fn resolve_backend(
        self: &Arc<Self>,
        exe: &str,
    ) -> Result<ResolvedBackend, DesktopError> {
        if self.cfg.esc_cancel() {
            crate::desktop::esc_watch::arm();
        }
        if is_cdp_identity(exe) {
            if let Some(cdp) = self.cdp().await {
                return Ok(ResolvedBackend::Cdp(cdp));
            }
            return Err(DesktopError::NotFound(
                "no CDP attachment; run `/desktop attach <port>` first".into(),
            ));
        }
        Ok(ResolvedBackend::Platform(self.lease().await?))
    }

    /// Build a generation-bound AppId for an executable identity. M1: pid and
    /// start-time are placeholders filled by the platform backend later; the
    /// actor generation is stamped so ids fail after an actor replacement.
    pub fn resolve_app(&self, exe: &str) -> AppId {
        AppId::new(0, 0, exe.to_string(), self.actor.generation())
    }

    /// Resolve a model-supplied window token into a generation-bound WindowId.
    /// Backends mint tokens from their window index ("0", "1", …) and
    /// dereference the key as that index, so the numeric part of the token
    /// must round-trip into the key unchanged. The id is stamped with the
    /// current actor generation so it fails as StaleRef after an actor
    /// replacement. Empty/non-numeric tokens are rejected (never a silent
    /// default).
    pub fn resolve_window(&self, app: AppId, token: &str) -> Result<WindowId, DesktopError> {
        let digits: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
        let key = digits
            .parse::<u64>()
            .map_err(|_| DesktopError::NotFound(format!("invalid window token {token:?}")))?;
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
            format!(
                "always_allow={}",
                if grants.is_empty() {
                    "none".into()
                } else {
                    grants.join(",")
                }
            ),
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

    #[test]
    fn resolve_window_key_round_trips_token() {
        let s = DesktopSession::new(DesktopConfig::default(), mock_factory());
        let app = AppId::new(1, 1, "com.test".into(), 0);
        // Platform backends dereference the key as their window index, so the
        // same token must yield the same key no matter how many resolutions
        // happened before it.
        for _ in 0..3 {
            let w = s.resolve_window(app.clone(), "0").unwrap();
            assert_eq!(w.actor_local_key(), 0);
        }
        let w = s.resolve_window(app.clone(), "2").unwrap();
        assert_eq!(w.actor_local_key(), 2);
        // Mock-style alphanumeric tokens keep their numeric part.
        let w = s.resolve_window(app, "w1").unwrap();
        assert_eq!(w.actor_local_key(), 1);
    }
}
