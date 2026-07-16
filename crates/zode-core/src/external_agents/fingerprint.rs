//! Trust-grant fingerprints. An approval binds to WHAT was approved — the
//! canonical executable (by content hash), its argv template, cwd, env
//! surface and effective sandbox. Any drift invalidates the grant. Grants
//! are two-phase (spec v2.3 §3.3): approval creates a `Pending` fingerprint
//! (hash known, version not — hashing reads the file, probing executes it);
//! the first spawn re-hashes, runs `--version`, and promotes to `Granted`.
//! Grants live in CarryState only — never persisted to disk.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use super::profiles::ExternalAgentDef;

#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    pub canonical_path: PathBuf,
    pub content_hash: String,
    /// `None` = pending (approved, `--version` not yet run).
    pub version_output: Option<String>,
    pub argv_template: Vec<String>,
    pub cwd: PathBuf,
    pub env_names: Vec<String>,
    pub effective_sandbox: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantCheck {
    /// Fully granted and fingerprint matches (ignoring version_output, which
    /// only the store knows).
    Granted,
    /// Approved but the first spawn (version probe) hasn't happened yet.
    Pending,
    /// No grant, or the fingerprint drifted — re-approval required.
    Missing,
}

/// Streaming sha256 of a file, hex-encoded. Pure read — never executes.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Fingerprint computed BEFORE approval (shown in the trust view): hash is a
/// pure read; `version_output` stays `None` until post-approval probing.
pub fn preapproval_fingerprint(def: &ExternalAgentDef, cwd: &Path) -> std::io::Result<Fingerprint> {
    let mut env_names = def.auth_env.clone();
    env_names.extend(def.env_allow.iter().cloned());
    Ok(Fingerprint {
        canonical_path: def.command.clone(),
        content_hash: hash_file(&def.command)?,
        version_output: None,
        argv_template: def.args.clone(),
        cwd: cwd.to_path_buf(),
        env_names,
        effective_sandbox: def.capability.effective_sandbox.to_string(),
    })
}

/// Session-scoped grant registry, keyed by profile name. Carried across
/// engine rebuilds via CarryState; intentionally not serializable.
#[derive(Debug, Default)]
pub struct GrantStore {
    grants: Mutex<HashMap<String, Fingerprint>>,
}

impl GrantStore {
    /// Compare `fp` (a fresh pre-approval fingerprint, version=None) against
    /// the stored grant for its profile.
    pub fn check(&self, name: &str, fp: &Fingerprint) -> GrantCheck {
        let grants = self.grants.lock().expect("grant lock");
        let Some(stored) = grants.get(name) else {
            return GrantCheck::Missing;
        };
        let matches = stored.canonical_path == fp.canonical_path
            && stored.content_hash == fp.content_hash
            && stored.argv_template == fp.argv_template
            && stored.cwd == fp.cwd
            && stored.env_names == fp.env_names
            && stored.effective_sandbox == fp.effective_sandbox;
        if !matches {
            return GrantCheck::Missing;
        }
        if stored.version_output.is_some() {
            GrantCheck::Granted
        } else {
            GrantCheck::Pending
        }
    }

    /// Record a freshly approved (pending) fingerprint.
    pub fn store_pending(&self, name: &str, fp: Fingerprint) {
        self.grants
            .lock()
            .expect("grant lock")
            .insert(name.to_string(), fp);
    }

    /// Fill in the version output after the post-approval probe succeeded.
    pub fn promote(&self, name: &str, version_output: String) {
        if let Some(fp) = self.grants.lock().expect("grant lock").get_mut(name) {
            fp.version_output = Some(version_output);
        }
    }

    /// Drop a grant (hash drift, version failure, user revocation).
    pub fn revoke(&self, name: &str) {
        self.grants.lock().expect("grant lock").remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::super::capability::{
        EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport,
    };
    use super::*;

    fn test_def(name: &str, bin: &Path) -> ExternalAgentDef {
        ExternalAgentDef {
            name: name.to_string(),
            command: bin.to_path_buf(),
            args: vec!["run".to_string()],
            capability: ProfileCapability {
                prompt_transport: PromptTransport::Stdin,
                output_protocol: OutputProtocol::Text,
                resume_flag: None,
                effective_sandbox: EffectiveSandbox::Unknown,
                version_requirement: None,
                session_id_source: None,
            },
            auth_env: vec![],
            env_allow: vec![],
            trusted: false,
        }
    }

    #[test]
    fn hash_changes_invalidate_grant() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-cli");
        std::fs::write(&bin, b"#!/bin/sh\necho hi\n").unwrap();
        let def = test_def("fake", &bin);
        let fp = preapproval_fingerprint(&def, dir.path()).unwrap();
        let store = GrantStore::default();
        assert_eq!(store.check("fake", &fp), GrantCheck::Missing);
        store.store_pending("fake", fp.clone());
        assert_eq!(store.check("fake", &fp), GrantCheck::Pending);
        store.promote("fake", "1.2.3".into());
        assert_eq!(store.check("fake", &fp), GrantCheck::Granted);
        // binary replaced -> hash mismatch -> Missing (must re-approve)
        std::fs::write(&bin, b"#!/bin/sh\necho evil\n").unwrap();
        let fp2 = preapproval_fingerprint(&def, dir.path()).unwrap();
        assert_eq!(store.check("fake", &fp2), GrantCheck::Missing);
    }

    #[test]
    fn revoke_clears_grant() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-cli");
        std::fs::write(&bin, b"x").unwrap();
        let def = test_def("fake", &bin);
        let fp = preapproval_fingerprint(&def, dir.path()).unwrap();
        let store = GrantStore::default();
        store.store_pending("fake", fp.clone());
        store.revoke("fake");
        assert_eq!(store.check("fake", &fp), GrantCheck::Missing);
    }
}
