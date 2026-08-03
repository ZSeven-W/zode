//! Per-site authorization for the gated browser tools.
//!
//! "Allow always" on `browser_act` / `browser_tabs` / `browser_eval` used
//! to be one process-wide boolean per tool: approving a click on a local
//! dev server also pre-approved every future click on every other site.
//! This module adds the narrower grant — "always, but only on this
//! origin" — plus its on-disk store, so the choice survives restarts.
//!
//! Origins are scheme + host + port of the page the action runs against
//! (see [`Origin`]). Matching is exact: subdomains are separate origins,
//! `http` and `https` are separate origins, and a non-default port is
//! part of the identity.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "browser-site-auth.json";
const CURRENT_VERSION: u32 = 1;
/// Cap on remembered-but-unclaimed scope choices; a UI that announces a
/// scope and then never answers the prompt must not leak entries.
const MAX_PENDING_CHOICES: usize = 64;

/// A web origin: scheme + host + port, normalized. Only `http`/`https`
/// URLs have one — `about:`, `data:`, `file:` and friends are opaque and
/// deliberately un-grantable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(String);

impl Origin {
    /// Extracts the origin of `url`, or `None` when the URL has no
    /// grantable origin (opaque scheme, missing host, unparseable).
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.trim().to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return None;
        }
        // Authority ends at the first path/query/fragment delimiter.
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            // Drop any userinfo — credentials are not part of the origin.
            .rsplit('@')
            .next()
            .unwrap_or_default();
        if authority.is_empty() {
            return None;
        }
        let (host, port) = split_host_port(authority)?;
        if host.is_empty() {
            return None;
        }
        let host = host.to_ascii_lowercase();
        let default_port = if scheme == "https" { 443 } else { 80 };
        Some(Origin(match port {
            Some(p) if p != default_port => format!("{scheme}://{host}:{p}"),
            _ => format!("{scheme}://{host}"),
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Splits an authority into host and optional port, keeping IPv6
/// brackets intact. Returns `None` when the port is present but not a
/// number (a malformed URL we refuse to key grants on).
fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    if let Some(end) = authority.rfind(']') {
        // IPv6 literal: [::1] or [::1]:8080
        let host = &authority[..=end];
        return match authority[end + 1..].strip_prefix(':') {
            Some(port) => Some((host, Some(port.parse().ok()?))),
            None if authority.len() == end + 1 => Some((host, None)),
            None => None,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host, Some(port.parse().ok()?))),
        None => Some((authority, None)),
    }
}

/// Scope the user picked when answering "allow always".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlwaysScope {
    /// Allow this tool everywhere, for the rest of the session. The
    /// default, and what a UI that knows nothing about scopes implies by
    /// answering plain "always".
    #[default]
    Global,
    /// Allow this tool only on the origin the prompt named.
    Site,
}

/// What the gate should do for one call, before any prompt happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// A stored grant covers this tool on this origin.
    AllowSite,
    /// The tool's session-wide "always" flag is set.
    AllowGlobal,
    /// Ask the user.
    Prompt,
}

impl GateDecision {
    /// Precedence: a per-origin grant is consulted first, then the
    /// session-wide flag, then we prompt. A site grant is checked before
    /// the global flag so the reason reported for an auto-allow is the
    /// narrowest one that applies.
    pub fn resolve(global_always: bool, origin_granted: bool) -> Self {
        if origin_granted {
            GateDecision::AllowSite
        } else if global_always {
            GateDecision::AllowGlobal
        } else {
            GateDecision::Prompt
        }
    }

    pub fn is_prompt(self) -> bool {
        matches!(self, GateDecision::Prompt)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct SiteAuthState {
    #[serde(default)]
    version: u32,
    /// Tool name -> origins granted "always" for that tool. Keyed per
    /// tool so `browser_eval` grants never widen `browser_act`.
    #[serde(default)]
    grants: BTreeMap<String, BTreeSet<String>>,
}

/// Persistent per-origin grants plus the pending-scope side channel used
/// to receive a site-scoped "always" answer from a UI (see
/// [`SiteAuthStore::choose_scope`]).
#[derive(Debug)]
pub struct SiteAuthStore {
    /// `None` for an in-memory store (tests, or a machine with no usable
    /// config dir): grants still work, they just don't survive restart.
    path: Option<PathBuf>,
    state: Mutex<SiteAuthState>,
    pending: Mutex<HashMap<u64, AlwaysScope>>,
    next_id: AtomicU64,
}

impl SiteAuthStore {
    /// Loads the store from the zode config dir, falling back to an
    /// in-memory store if the directory cannot be resolved.
    pub fn load_default() -> Arc<Self> {
        match crate::config::ConfigManager::config_dir() {
            Ok(dir) => Self::at_path(dir.join(STORE_FILE)),
            Err(e) => {
                tracing::debug!("browser site-auth: no config dir ({e}); grants are in-memory");
                Self::in_memory()
            }
        }
    }

    /// Loads (or starts) a store backed by `path`.
    pub fn at_path(path: PathBuf) -> Arc<Self> {
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| match serde_json::from_str::<SiteAuthState>(&raw) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("browser site-auth: ignoring unreadable {path:?}: {e}");
                    None
                }
            })
            .unwrap_or_default();
        Arc::new(Self {
            path: Some(path),
            state: Mutex::new(state),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    /// Store that never touches the disk.
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self {
            path: None,
            state: Mutex::new(SiteAuthState::default()),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn is_granted(&self, tool: &str, origin: &Origin) -> bool {
        self.state
            .lock()
            .unwrap()
            .grants
            .get(tool)
            .is_some_and(|set| set.contains(origin.as_str()))
    }

    /// Records "always allow `tool` on `origin`" and persists it.
    pub fn grant(&self, tool: &str, origin: &Origin) {
        {
            let mut state = self.state.lock().unwrap();
            state.version = CURRENT_VERSION;
            state
                .grants
                .entry(tool.to_string())
                .or_default()
                .insert(origin.as_str().to_string());
        }
        self.persist();
    }

    /// Drops one grant. Returns whether anything was removed.
    pub fn revoke(&self, tool: &str, origin: &Origin) -> bool {
        let removed = {
            let mut state = self.state.lock().unwrap();
            let removed = state
                .grants
                .get_mut(tool)
                .is_some_and(|set| set.remove(origin.as_str()));
            state.grants.retain(|_, set| !set.is_empty());
            removed
        };
        if removed {
            self.persist();
        }
        removed
    }

    /// Drops every grant for every tool.
    pub fn clear(&self) {
        self.state.lock().unwrap().grants.clear();
        self.persist();
    }

    /// Granted origins for one tool, sorted.
    pub fn granted_origins(&self, tool: &str) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .grants
            .get(tool)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Every grant, as `(tool, origins)` pairs — for a permissions panel.
    pub fn all_grants(&self) -> Vec<(String, Vec<String>)> {
        self.state
            .lock()
            .unwrap()
            .grants
            .iter()
            .map(|(tool, set)| (tool.clone(), set.iter().cloned().collect()))
            .collect()
    }

    /// Id for one approval prompt; echoed to the UI as `_scope_request`.
    pub fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// UI-facing: declare which scope an upcoming "allow always" answer
    /// means. Call this *before* responding to the approval request whose
    /// `_scope_request` id is `request_id`. A UI that never calls this —
    /// i.e. today's TUI dialog — leaves the answer meaning `Global`,
    /// exactly the pre-existing behavior.
    pub fn choose_scope(&self, request_id: u64, scope: AlwaysScope) {
        let mut pending = self.pending.lock().unwrap();
        if pending.len() >= MAX_PENDING_CHOICES {
            // Cheap bound: abandoned prompts (turn aborted before the
            // answer) would otherwise accumulate forever.
            let stale: Vec<u64> = pending
                .keys()
                .copied()
                .filter(|id| *id < request_id.saturating_sub(MAX_PENDING_CHOICES as u64))
                .collect();
            for id in stale {
                pending.remove(&id);
            }
        }
        pending.insert(request_id, scope);
    }

    /// Consumes the scope announced for `request_id`, defaulting to
    /// [`AlwaysScope::Global`].
    pub fn take_scope(&self, request_id: u64) -> AlwaysScope {
        self.pending
            .lock()
            .unwrap()
            .remove(&request_id)
            .unwrap_or_default()
    }

    fn persist(&self) {
        let Some(path) = &self.path else { return };
        let snapshot = {
            let mut state = self.state.lock().unwrap();
            state.version = CURRENT_VERSION;
            state.clone()
        };
        if let Err(e) = write_private_json(path, &snapshot) {
            tracing::warn!("browser site-auth: could not save {path:?}: {e}");
        }
    }
}

/// Writes `state` as owner-only JSON (0600), creating the parent dir —
/// same handling as the bridge token file.
fn write_private_json(path: &std::path::Path, state: &SiteAuthState) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    serde_json::to_writer_pretty(&mut file, state)?;
    file.write_all(b"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(url: &str) -> Origin {
        Origin::parse(url).expect("origin")
    }

    #[test]
    fn origin_normalizes_scheme_host_and_default_ports() {
        assert_eq!(
            origin("https://Example.TEST/a/b?c#d").as_str(),
            "https://example.test"
        );
        assert_eq!(
            origin("https://example.test:443/").as_str(),
            "https://example.test"
        );
        assert_eq!(
            origin("http://example.test:80").as_str(),
            "http://example.test"
        );
        assert_eq!(
            origin("http://user:pw@example.test/x").as_str(),
            "http://example.test"
        );
        assert_eq!(origin("http://[::1]:8080/x").as_str(), "http://[::1]:8080");
        assert_eq!(origin("http://[::1]/x").as_str(), "http://[::1]");
    }

    #[test]
    fn origins_distinguish_scheme_port_and_subdomain() {
        assert_ne!(origin("http://x.test"), origin("https://x.test"));
        assert_ne!(origin("https://x.test"), origin("https://x.test:8443"));
        assert_ne!(origin("https://a.x.test"), origin("https://x.test"));
        assert_ne!(origin("https://x.test"), origin("https://y.test"));
        assert_eq!(origin("https://x.test/one"), origin("https://x.test/two"));
    }

    #[test]
    fn opaque_and_malformed_urls_have_no_origin() {
        for url in [
            "about:blank",
            "data:text/html,<h1>hi</h1>",
            "file:///tmp/x.html",
            "chrome://settings",
            "https://",
            "not a url",
            "https://x.test:notaport/",
        ] {
            assert!(Origin::parse(url).is_none(), "{url} should have no origin");
        }
    }

    #[test]
    fn grants_are_per_tool_and_per_origin() {
        let store = SiteAuthStore::in_memory();
        let a = origin("https://a.test");
        let b = origin("https://b.test");
        store.grant("browser_act", &a);

        assert!(store.is_granted("browser_act", &a));
        assert!(!store.is_granted("browser_act", &b), "other origin");
        assert!(
            !store.is_granted("browser_eval", &a),
            "eval stays independently gated"
        );
        assert!(!store.is_granted("browser_act", &origin("https://sub.a.test")));
        assert!(!store.is_granted("browser_act", &origin("http://a.test")));
    }

    #[test]
    fn revoke_and_clear_drop_grants() {
        let store = SiteAuthStore::in_memory();
        let a = origin("https://a.test");
        store.grant("browser_act", &a);
        assert!(store.revoke("browser_act", &a));
        assert!(!store.revoke("browser_act", &a), "second revoke is a no-op");
        assert!(store.all_grants().is_empty());

        store.grant("browser_act", &a);
        store.grant("browser_eval", &a);
        assert_eq!(store.all_grants().len(), 2);
        store.clear();
        assert!(store.all_grants().is_empty());
    }

    #[test]
    fn scope_choice_defaults_to_global_and_is_single_use() {
        let store = SiteAuthStore::in_memory();
        let id = store.next_request_id();
        assert_eq!(store.take_scope(id), AlwaysScope::Global);

        let id = store.next_request_id();
        store.choose_scope(id, AlwaysScope::Site);
        assert_eq!(store.take_scope(id), AlwaysScope::Site);
        assert_eq!(
            store.take_scope(id),
            AlwaysScope::Global,
            "consumed, so the next answer is unscoped again"
        );
    }

    #[test]
    fn request_ids_are_unique() {
        let store = SiteAuthStore::in_memory();
        let a = store.next_request_id();
        let b = store.next_request_id();
        assert_ne!(a, b);
    }

    #[test]
    fn decision_precedence_site_then_global_then_prompt() {
        assert_eq!(GateDecision::resolve(false, true), GateDecision::AllowSite);
        assert_eq!(GateDecision::resolve(true, true), GateDecision::AllowSite);
        assert_eq!(
            GateDecision::resolve(true, false),
            GateDecision::AllowGlobal
        );
        assert_eq!(GateDecision::resolve(false, false), GateDecision::Prompt);
        assert!(GateDecision::resolve(false, false).is_prompt());
    }

    #[test]
    fn persistence_round_trips_and_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser-site-auth.json");
        let store = SiteAuthStore::at_path(path.clone());
        store.grant("browser_act", &origin("https://keep.test:8443/page"));
        store.grant("browser_eval", &origin("https://keep.test"));

        let reloaded = SiteAuthStore::at_path(path.clone());
        assert!(reloaded.is_granted("browser_act", &origin("https://keep.test:8443")));
        assert!(reloaded.is_granted("browser_eval", &origin("https://keep.test")));
        assert!(!reloaded.is_granted("browser_act", &origin("https://keep.test")));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn unreadable_store_starts_empty_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser-site-auth.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let store = SiteAuthStore::at_path(path);
        assert!(store.all_grants().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn default_store_lives_in_the_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let store = SiteAuthStore::load_default();
        store.grant("browser_act", &origin("https://cfg.test"));
        assert!(dir.path().join("browser-site-auth.json").is_file());

        let reloaded = SiteAuthStore::load_default();
        assert!(reloaded.is_granted("browser_act", &origin("https://cfg.test")));
        std::env::remove_var("ZODE_CONFIG_DIR");
    }
}
