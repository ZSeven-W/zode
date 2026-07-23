//! Routes files to language servers. Owns one [`LspClient`] per language,
//! spawned lazily on first use (so configuring rust-analyzer doesn't pay its
//! startup cost until an `lsp_*` tool actually touches a `.rs` file).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use agent::abort::AbortController;

use crate::config::{LspConfig, LspServerConfig};
use crate::lsp::client::LspClient;
use crate::lsp::install;
use crate::process_supervision::spawn_blocking_tracked;

#[derive(Debug)]
pub struct LspManager {
    root: PathBuf,
    /// Enabled language → server config.
    servers: HashMap<String, LspServerConfig>,
    /// File extension (lowercased, no dot) → language key.
    ext_index: HashMap<String, String>,
    /// Lazily-spawned clients, keyed by language.
    clients: Mutex<HashMap<String, Arc<LspClient>>>,
    /// Cached workspace language scan for [`Self::status`], refreshed on a
    /// TTL — the sidebar polls every couple of seconds and must not walk
    /// the tree each time.
    scan: std::sync::Mutex<Option<(Instant, HashSet<String>)>>,
}

impl LspManager {
    pub fn new(cfg: LspConfig, root: PathBuf) -> Self {
        let mut ext_index = HashMap::new();
        for (lang, sc) in &cfg.servers {
            for ext in &sc.extensions {
                let key = ext.trim_start_matches('.').to_ascii_lowercase();
                ext_index.insert(key, lang.clone());
            }
        }
        Self {
            root,
            servers: cfg.servers,
            ext_index,
            clients: Mutex::new(HashMap::new()),
            scan: std::sync::Mutex::new(None),
        }
    }

    /// The enabled language keys, sorted — what the system prompt advertises.
    pub fn langs(&self) -> Vec<String> {
        let mut langs: Vec<String> = self.servers.keys().cloned().collect();
        langs.sort();
        langs
    }

    /// Resolve a possibly-relative file argument against the workspace root.
    pub fn resolve(&self, file: &str) -> PathBuf {
        let p = Path::new(file);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn lang_for(&self, path: &Path) -> Option<String> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.ext_index.get(&ext).cloned()
    }

    /// `(language, client-running)` for the enabled servers the PROJECT
    /// actually uses — a language shows only when the workspace holds a
    /// matching file (bounded scan) or its client is already running.
    /// Sorted by language; feeds the sidebar's LSP section. Uses `try_lock`
    /// (the render-side caller must never block); during the brief
    /// spawn-time contention everything reads as not-running until the
    /// next poll.
    pub fn status(&self) -> Vec<(String, bool)> {
        let running: HashSet<String> = self
            .clients
            .try_lock()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default();
        let present = self.project_languages();
        let mut langs: Vec<(String, bool)> = self
            .servers
            .keys()
            .filter(|lang| present.contains(*lang) || running.contains(*lang))
            .map(|lang| (lang.clone(), running.contains(lang)))
            .collect();
        langs.sort();
        langs
    }

    /// Languages with at least one matching file in the workspace, cached
    /// for [`SCAN_TTL`] so the sidebar poll doesn't re-walk the tree; a
    /// file of a new language created mid-session shows up on the next
    /// rescan.
    fn project_languages(&self) -> HashSet<String> {
        let mut guard = self.scan.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, set)) = &*guard {
            if at.elapsed() < SCAN_TTL {
                return set.clone();
            }
        }
        let set = scan_project_languages(&self.root, &self.ext_index);
        *guard = Some((Instant::now(), set.clone()));
        set
    }

    /// Get (spawning if needed) the client that handles `path`.
    ///
    /// Callers that own a turn should use [`Self::client_for_with_abort`] so
    /// startup and initialization are tied to that turn. This compatibility
    /// entry point keeps the pre-watchdog API for non-turn callers.
    pub async fn client_for(&self, path: &Path) -> Result<Arc<LspClient>, String> {
        let abort = AbortController::new();
        self.client_for_with_abort(path, &abort).await
    }

    /// Abort-aware variant of [`Self::client_for`]. Errors when no
    /// configured+enabled server claims the file's extension, or the server
    /// fails to start.
    pub async fn client_for_with_abort(
        &self,
        path: &Path,
        abort: &AbortController,
    ) -> Result<Arc<LspClient>, String> {
        let lang = self.lang_for(path).ok_or_else(|| {
            format!(
                "no language server configured for {} (enable one via /plugin)",
                path.display()
            )
        })?;
        // Held across spawn so two concurrent calls don't start the server
        // twice; LSP calls aren't hot, so the brief serialization is fine.
        let mut clients = tokio::select! {
            biased;
            _ = abort.cancelled() => {
                return Err("language server startup cancelled".to_string());
            }
            clients = self.clients.lock() => clients,
        };
        if let Some(c) = clients.get(&lang) {
            return Ok(c.clone());
        }
        let cfg = self
            .servers
            .get(&lang)
            .ok_or_else(|| format!("server config for {lang} missing"))?
            .clone();
        // For a built-in language, resolve the runnable command — installing it
        // on demand into ~/.zode/lsp if needed (blocking, hence spawn_blocking).
        // User-defined languages AND user-overridden commands (a config entry
        // whose command differs from the built-in default) spawn exactly what
        // the config says — auto-provisioning must not trample an override.
        let resolved = match install::spec_for_lang(&lang) {
            Some(spec) if cfg.command == spec.command => {
                // Auto-provisioning mutates the user's toolchain/config area.
                // Latch retry safety before the installer can begin. The work
                // guard lives inside the blocking closure, so hard-cancelling
                // this tool cannot make the turn look quiescent while npm,
                // rustup, or go is still running.
                abort.mark_side_effect_risk();
                let path = spawn_blocking_tracked(&abort.activity(), move || install::ensure(spec))
                    .await
                    .map_err(|e| format!("install task failed: {e}"))??;
                LspServerConfig {
                    command: path.to_string_lossy().into_owned(),
                    ..cfg
                }
            }
            _ => cfg,
        };
        if abort.is_aborted() {
            return Err("language server startup cancelled".to_string());
        }
        // Starting any server is an external side effect, including custom
        // languages and command overrides that bypass auto-provisioning. Latch
        // before spawning so scheduler retries can never duplicate a server
        // whose startup outcome became uncertain.
        abort.mark_side_effect_risk();
        let client = Arc::new(
            LspClient::start_with_abort(lang.clone(), &resolved, self.root.clone(), abort).await?,
        );
        clients.insert(lang, client.clone());
        Ok(client)
    }
}

/// How long a workspace language scan stays fresh.
const SCAN_TTL: Duration = Duration::from_secs(60);

/// Walk depth and entry caps: keep the (UI-poll-triggered) scan bounded to
/// a few milliseconds even in a huge workspace.
const SCAN_MAX_DEPTH: usize = 5;
const SCAN_MAX_ENTRIES: usize = 4000;

/// Which languages (per `ext_index`) have at least one file under `root`.
/// Breadth-first, bounded by [`SCAN_MAX_DEPTH`] / [`SCAN_MAX_ENTRIES`];
/// skips hidden directories and dependency/build trees (their files belong
/// to dependencies, not the project). Early-exits once every language in
/// the index has been seen.
fn scan_project_languages(root: &Path, ext_index: &HashMap<String, String>) -> HashSet<String> {
    const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", "out", "vendor"];
    let all_langs: HashSet<&String> = ext_index.values().collect();
    let mut found: HashSet<String> = HashSet::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> =
        std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut seen_entries = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen_entries += 1;
            if seen_entries > SCAN_MAX_ENTRIES || found.len() == all_langs.len() {
                return found;
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if depth < SCAN_MAX_DEPTH
                    && !name.starts_with('.')
                    && !SKIP_DIRS.contains(&name.as_ref())
                {
                    queue.push_back((path, depth + 1));
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Some(lang) = ext_index.get(&ext.to_ascii_lowercase()) {
                    found.insert(lang.clone());
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LspConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "rust".to_string(),
            LspServerConfig {
                command: "rust-analyzer".into(),
                args: vec![],
                extensions: vec!["rs".into()],
            },
        );
        LspConfig { servers }
    }

    #[test]
    fn routes_extension_to_language() {
        let m = LspManager::new(cfg(), PathBuf::from("/proj"));
        assert_eq!(
            m.lang_for(Path::new("/proj/src/main.rs")).as_deref(),
            Some("rust")
        );
        assert_eq!(m.lang_for(Path::new("/proj/README.md")), None);
    }

    #[test]
    fn resolve_joins_relative_to_root() {
        let m = LspManager::new(cfg(), PathBuf::from("/proj"));
        assert_eq!(m.resolve("src/main.rs"), PathBuf::from("/proj/src/main.rs"));
        assert_eq!(m.resolve("/abs/x.rs"), PathBuf::from("/abs/x.rs"));
    }

    #[tokio::test]
    async fn custom_server_start_latches_side_effect_risk_before_spawn() {
        let mut servers = HashMap::new();
        servers.insert(
            "custom".to_string(),
            LspServerConfig {
                command: "__zode_missing_custom_lsp__".into(),
                args: vec![],
                extensions: vec!["custom".into()],
            },
        );
        let manager = LspManager::new(LspConfig { servers }, PathBuf::from("/proj"));
        let abort = AbortController::new();

        let error = manager
            .client_for_with_abort(Path::new("/proj/file.custom"), &abort)
            .await
            .expect_err("the deliberately missing custom server cannot start");

        assert!(error.contains("spawn"), "{error}");
        assert!(abort.activity().side_effect_risk());
    }

    fn two_lang_cfg() -> LspConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "rust".to_string(),
            LspServerConfig {
                command: "rust-analyzer".into(),
                args: vec![],
                extensions: vec!["rs".into()],
            },
        );
        servers.insert(
            "typescript".to_string(),
            LspServerConfig {
                command: "typescript-language-server".into(),
                args: vec!["--stdio".into()],
                extensions: vec!["ts".into()],
            },
        );
        LspConfig { servers }
    }

    #[test]
    fn status_lists_only_languages_the_project_uses() {
        // rust + typescript configured, but the workspace only holds a .rs
        // file — the sidebar must not list every offered language, only
        // what the project actually uses.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), b"fn main(){}").unwrap();
        let m = LspManager::new(two_lang_cfg(), dir.path().to_path_buf());
        assert_eq!(m.status(), vec![("rust".to_string(), false)]);
    }

    #[test]
    fn scan_skips_dependency_trees_and_hidden_dirs() {
        // A .ts file that exists ONLY under node_modules (or a hidden dir)
        // belongs to dependencies, not the project — it must not surface
        // typescript in the sidebar.
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("index.ts"), b"x").unwrap();
        let hidden = dir.path().join(".cache");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(hidden.join("gen.rs"), b"x").unwrap();
        let m = LspManager::new(two_lang_cfg(), dir.path().to_path_buf());
        assert_eq!(m.status(), Vec::<(String, bool)>::new());
    }
}
