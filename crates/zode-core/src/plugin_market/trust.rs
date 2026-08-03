//! Trust-review data layer (M2 groundwork): `~/.zode/plugins/trust.json`
//! records, per plugin, the ref + a content hash for every capability that
//! executes something (hook scripts, MCP launch commands/URLs). A capability
//! whose ref or content hash no longer matches the granted record needs
//! re-review before it can run — the review UI itself (a later wave) reads
//! `TrustStatus::NeedsReview`/`Drifted` items and shows their `content` in
//! full (never summarized, per the design doc's "show the exact text").
//!
//! Skills carry no entry here at all: `Capability::is_reviewable` excludes
//! them, so they're always usable with no gate.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{Capability, PluginManifest};
use super::PluginMarketError;

pub const TRUST_FILE_NAME: &str = "trust.json";

/// One reviewable capability's live, human-facing content — the exact text
/// a review UI must render in full (the MCP command line, or the hook
/// script's source), plus its stable key for `grant_items`/lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustItem {
    pub key: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStatus {
    /// Every reviewable capability matches a previously granted record at
    /// the manifest's current ref.
    Trusted,
    /// No trust record exists yet for one or more capabilities (fresh
    /// install, or a capability newly added since the last grant).
    NeedsReview(Vec<TrustItem>),
    /// A trust record exists but no longer matches — the ref changed, or a
    /// capability's content changed since it was granted.
    Drifted(Vec<TrustItem>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginTrust {
    #[serde(rename = "ref")]
    reference: String,
    /// trust_key -> content hash, granted at `reference`.
    capabilities: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustFile {
    #[serde(default)]
    plugins: HashMap<String, PluginTrust>,
}

/// Loaded `trust.json`, held in memory; call [`TrustStore::save`] to persist
/// changes made via `grant_all`/`grant_items`/`remove_plugin`.
#[derive(Debug)]
pub struct TrustStore {
    path: PathBuf,
    file: TrustFile,
}

impl TrustStore {
    /// Load `<config_dir>/plugins/trust.json`, or start empty if absent.
    pub fn load(config_dir: &Path) -> Result<Self, PluginMarketError> {
        let path = config_dir.join("plugins").join(TRUST_FILE_NAME);
        let file = match std::fs::read_to_string(&path) {
            Ok(s) => {
                serde_json::from_str(&s).map_err(|e| PluginMarketError::Manifest(e.to_string()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => TrustFile::default(),
            Err(e) => return Err(PluginMarketError::Io(e)),
        };
        Ok(Self { path, file })
    }

    pub fn save(&self) -> Result<(), PluginMarketError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.file)
            .map_err(|e| PluginMarketError::Manifest(e.to_string()))?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    /// Current trust status for `plugin_id`'s reviewable capabilities,
    /// re-reading live content from `plugin_dir` (not any cached snapshot)
    /// so a capability edited in place after install/grant is caught.
    pub fn status(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> TrustStatus {
        let record = self.file.plugins.get(plugin_id);
        let mut needs_review = Vec::new();
        let mut drifted = Vec::new();
        for cap in &manifest.capabilities {
            if !cap.is_reviewable() {
                continue;
            }
            let content = reviewable_content(cap, plugin_dir);
            let hash = content_hash(&content);
            let key = cap.trust_key();
            match record {
                None => needs_review.push(TrustItem { key, content }),
                Some(r) if r.reference != manifest.reference => {
                    drifted.push(TrustItem { key, content })
                }
                Some(r) => match r.capabilities.get(&key) {
                    None => needs_review.push(TrustItem { key, content }),
                    Some(h) if *h == hash => {}
                    Some(_) => drifted.push(TrustItem { key, content }),
                },
            }
        }
        if !drifted.is_empty() {
            TrustStatus::Drifted(drifted)
        } else if !needs_review.is_empty() {
            TrustStatus::NeedsReview(needs_review)
        } else {
            TrustStatus::Trusted
        }
    }

    /// Grant trust to every currently reviewable capability ("trust all",
    /// mirroring Codex's `trustAll`). Overwrites any prior record for this
    /// plugin — including one that had drifted — with the live content.
    pub fn grant_all(&mut self, plugin_id: &str, manifest: &PluginManifest, plugin_dir: &Path) {
        let keys: Vec<String> = manifest
            .capabilities
            .iter()
            .filter(|c| c.is_reviewable())
            .map(Capability::trust_key)
            .collect();
        self.grant_items(plugin_id, &keys, manifest, plugin_dir);
    }

    /// Grant trust to just the named capabilities — the per-item "trust"
    /// half of the design doc's "逐项'信任'或'全部信任'".
    pub fn grant_items(
        &mut self,
        plugin_id: &str,
        keys: &[String],
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) {
        let entry = self.file.plugins.entry(plugin_id.to_string()).or_default();
        entry.reference = manifest.reference.clone();
        for cap in &manifest.capabilities {
            if !cap.is_reviewable() {
                continue;
            }
            let key = cap.trust_key();
            if !keys.contains(&key) {
                continue;
            }
            let content = reviewable_content(cap, plugin_dir);
            entry.capabilities.insert(key, content_hash(&content));
        }
    }

    /// Drop all trust records for a plugin (uninstall).
    pub fn remove_plugin(&mut self, plugin_id: &str) {
        self.file.plugins.remove(plugin_id);
    }
}

/// The exact text a review UI shows for one reviewable capability: the MCP
/// launch command line (or remote URL), or the hook script's full source.
/// Unreadable content (e.g. a missing script) hashes as empty — which can
/// never match a prior grant, so it safely falls to NeedsReview/Drifted
/// rather than silently trusting something that can't be inspected.
fn reviewable_content(cap: &Capability, plugin_dir: &Path) -> String {
    match cap {
        Capability::Skill { .. } => String::new(),
        Capability::Mcp { command, args, .. } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            }
        }
        Capability::Hook { script, .. } => {
            std::fs::read_to_string(plugin_dir.join(script)).unwrap_or_default()
        }
    }
}

/// A drift-detection checksum, NOT a cryptographic integrity guarantee: zode
/// has no hashing crate dependency to add for this, and `trust.json` only
/// needs to notice "this changed since it was granted", not resist a
/// deliberate second-preimage attack. `DefaultHasher` (SipHash-1-3, fixed
/// keys) is deterministic across runs and processes, which is required for
/// the hash to be meaningfully persisted and compared later.
fn content_hash(content: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with(reference: &str, caps: Vec<Capability>) -> PluginManifest {
        PluginManifest {
            repo: "acme/tools".into(),
            reference: reference.into(),
            installed_at: 0,
            commit: None,
            updated_at: None,
            capabilities: caps,
        }
    }

    fn empty_store(dir: &Path) -> TrustStore {
        TrustStore {
            path: dir.join(TRUST_FILE_NAME),
            file: TrustFile::default(),
        }
    }

    #[test]
    fn fresh_install_needs_review() {
        let dir = tempfile::tempdir().unwrap();
        let store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "npx".into(),
                args: vec![],
            }],
        );
        match store.status("acme__tools", &manifest, dir.path()) {
            TrustStatus::NeedsReview(items) => assert_eq!(items.len(), 1),
            other => panic!("expected NeedsReview, got {other:?}"),
        }
    }

    #[test]
    fn grant_then_status_is_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "npx".into(),
                args: vec![],
            }],
        );
        store.grant_all("acme__tools", &manifest, dir.path());
        assert_eq!(
            store.status("acme__tools", &manifest, dir.path()),
            TrustStatus::Trusted
        );
    }

    #[test]
    fn ref_change_drifts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = empty_store(dir.path());
        let caps = vec![Capability::Mcp {
            name: "srv".into(),
            command: "npx".into(),
            args: vec![],
        }];
        let old = manifest_with("main", caps.clone());
        store.grant_all("acme__tools", &old, dir.path());
        let new = manifest_with("v2", caps);
        match store.status("acme__tools", &new, dir.path()) {
            TrustStatus::Drifted(items) => assert_eq!(items.len(), 1),
            other => panic!("expected Drifted, got {other:?}"),
        }
    }

    #[test]
    fn content_change_drifts() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = empty_store(dir.path());
        let old = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "npx".into(),
                args: vec![],
            }],
        );
        store.grant_all("acme__tools", &old, dir.path());
        let new = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "curl".into(),
                args: vec!["evil.sh".into()],
            }],
        );
        match store.status("acme__tools", &new, dir.path()) {
            TrustStatus::Drifted(items) => assert_eq!(items[0].content, "curl evil.sh"),
            other => panic!("expected Drifted, got {other:?}"),
        }
    }

    #[test]
    fn hook_script_content_hashed_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("check.sh"), "echo one").unwrap();
        let mut store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![Capability::Hook {
                event: "before_tool_use".into(),
                tool: None,
                script: "check.sh".into(),
            }],
        );
        store.grant_all("acme__tools", &manifest, dir.path());
        assert_eq!(
            store.status("acme__tools", &manifest, dir.path()),
            TrustStatus::Trusted
        );

        // Script edited in place post-grant -> drift, even with no re-scan.
        std::fs::write(dir.path().join("check.sh"), "echo two").unwrap();
        match store.status("acme__tools", &manifest, dir.path()) {
            TrustStatus::Drifted(_) => {}
            other => panic!("expected Drifted, got {other:?}"),
        }
    }

    #[test]
    fn skills_are_never_gated() {
        let dir = tempfile::tempdir().unwrap();
        let store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![Capability::Skill {
                name: "review".into(),
                path: "skills/review".into(),
            }],
        );
        assert_eq!(
            store.status("acme__tools", &manifest, dir.path()),
            TrustStatus::Trusted
        );
    }

    #[test]
    fn save_and_reload_round_trips() {
        let config_dir = tempfile::tempdir().unwrap();
        let plugin_dir = tempfile::tempdir().unwrap();
        let manifest = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "npx".into(),
                args: vec![],
            }],
        );
        let mut store = TrustStore::load(config_dir.path()).unwrap();
        store.grant_all("acme__tools", &manifest, plugin_dir.path());
        store.save().unwrap();

        let reloaded = TrustStore::load(config_dir.path()).unwrap();
        assert_eq!(
            reloaded.status("acme__tools", &manifest, plugin_dir.path()),
            TrustStatus::Trusted
        );
    }

    #[test]
    fn remove_plugin_clears_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![Capability::Mcp {
                name: "srv".into(),
                command: "npx".into(),
                args: vec![],
            }],
        );
        store.grant_all("acme__tools", &manifest, dir.path());
        store.remove_plugin("acme__tools");
        match store.status("acme__tools", &manifest, dir.path()) {
            TrustStatus::NeedsReview(_) => {}
            other => panic!("expected NeedsReview after removal, got {other:?}"),
        }
    }

    #[test]
    fn grant_items_only_trusts_the_named_capability() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = empty_store(dir.path());
        let manifest = manifest_with(
            "main",
            vec![
                Capability::Mcp {
                    name: "a".into(),
                    command: "npx".into(),
                    args: vec![],
                },
                Capability::Mcp {
                    name: "b".into(),
                    command: "uvx".into(),
                    args: vec![],
                },
            ],
        );
        store.grant_items("acme__tools", &["mcp:a".to_string()], &manifest, dir.path());
        match store.status("acme__tools", &manifest, dir.path()) {
            TrustStatus::NeedsReview(items) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].key, "mcp:b");
            }
            other => panic!("expected NeedsReview for the ungranted item, got {other:?}"),
        }
    }
}
