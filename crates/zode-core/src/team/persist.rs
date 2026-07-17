//! Team state persistence with tamper detection. `team.json` lives inside the
//! project and is hand-editable, so it carries an HMAC-SHA256 over the roster
//! payload (key in `~/.zode/team-state.key`, 0600, generated on first use).
//! A MAC mismatch does NOT auto-recover: the file is quarantined and the user
//! must re-hire. Honest boundary (spec §4.2): the key is readable by the same
//! OS user the external CLIs run as, so this guards accidental corruption and
//! key-less tampering, not a malicious same-user teammate.

use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::TeamError;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedTeammate {
    pub name: String,
    pub role: String,
    pub kind: String, // "internal" | "external"
    pub model_label: String,
    /// External only: resume session id, bound to the fields below.
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub fingerprint_hash: Option<String>,
    /// Internal only: session history file.
    #[serde(default)]
    pub session_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamStateFile {
    pub schema: u32,
    pub roster: Vec<PersistedTeammate>,
    pub board_revision: u64,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    mac: String,
    payload: TeamStateFile,
}

fn key_path() -> Result<PathBuf, TeamError> {
    let dir =
        crate::config::ConfigManager::config_dir().map_err(|e| TeamError::Io(e.to_string()))?;
    Ok(dir.join("team-state.key"))
}

/// Load or generate the per-user MAC key.
pub fn load_or_create_key() -> Result<Vec<u8>, TeamError> {
    let path = key_path()?;
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() >= 32 {
            return Ok(bytes);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TeamError::Io(e.to_string()))?;
    }
    // Derive a key from process + time entropy (no rng dep). This is a
    // best-effort integrity key, not a cryptographic secret (see module doc).
    let seed = format!(
        "{}-{}-team",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let mut mac = HmacSha256::new_from_slice(b"zode-team-state-key-derivation").expect("hmac key");
    mac.update(seed.as_bytes());
    let key = mac.finalize().into_bytes().to_vec();
    // Create exclusively so concurrent creators don't overwrite each other's
    // key (which would fail MAC verification of already-saved state). If
    // someone else won the race, adopt their key.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(&key)
                .map_err(|e| TeamError::Io(e.to_string()))?;
            Ok(key)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another process/thread created it first — use theirs.
            std::fs::read(&path).map_err(|e| TeamError::Io(e.to_string()))
        }
        Err(e) => Err(TeamError::Io(e.to_string())),
    }
}

fn compute_mac(key: &[u8], payload: &TeamStateFile) -> Result<String, TeamError> {
    let json = serde_json::to_vec(payload).map_err(|e| TeamError::Io(e.to_string()))?;
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| TeamError::Io(e.to_string()))?;
    mac.update(&json);
    Ok(format!("{:x}", mac.finalize().into_bytes()))
}

pub fn save(state: &TeamStateFile, team_dir: &Path, key: &[u8]) -> Result<(), TeamError> {
    super::board::ensure_no_symlink(team_dir)?;
    std::fs::create_dir_all(team_dir).map_err(|e| TeamError::Io(e.to_string()))?;
    let env = Envelope {
        mac: compute_mac(key, state)?,
        payload: state.clone(),
    };
    let json = serde_json::to_string_pretty(&env).map_err(|e| TeamError::Io(e.to_string()))?;
    let path = team_dir.join("team.json");
    let tmp = team_dir.join("team.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| TeamError::Io(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| TeamError::Io(e.to_string()))?;
    Ok(())
}

/// Load and verify. `Ok(None)` = no team file. MAC mismatch → the file is
/// renamed to `team.json.quarantined` and `Quarantined` is returned.
pub fn load(team_dir: &Path, key: &[u8]) -> Result<Option<TeamStateFile>, TeamError> {
    let path = team_dir.join("team.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(TeamError::Io(e.to_string())),
    };
    let env: Envelope =
        serde_json::from_str(&text).map_err(|e| TeamError::Io(format!("team.json: {e}")))?;
    let expected = compute_mac(key, &env.payload)?;
    if !constant_time_eq(expected.as_bytes(), env.mac.as_bytes()) {
        let _ = std::fs::rename(&path, team_dir.join("team.json.quarantined"));
        return Err(TeamError::Quarantined);
    }
    Ok(Some(env.payload))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TeamStateFile {
        TeamStateFile {
            schema: 1,
            roster: vec![PersistedTeammate {
                name: "codex-impl".into(),
                role: "builder".into(),
                kind: "external".into(),
                model_label: "codex(external)".into(),
                session_id: Some("sess-A".into()),
                profile: Some("codex".into()),
                fingerprint_hash: Some("abc".into()),
                session_path: None,
            }],
            board_revision: 3,
        }
    }

    #[test]
    fn round_trip_ok() {
        let dir = tempfile::tempdir().unwrap();
        let key = b"test-key-0123456789012345678901x";
        save(&sample(), dir.path(), key).unwrap();
        let loaded = load(dir.path(), key).unwrap().unwrap();
        assert_eq!(loaded, sample());
    }

    #[test]
    fn tampered_team_json_is_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        let key = b"test-key-0123456789012345678901x";
        save(&sample(), dir.path(), key).unwrap();
        let p = dir.path().join("team.json");
        let txt = std::fs::read_to_string(&p)
            .unwrap()
            .replace("sess-A", "sess-EVIL");
        std::fs::write(&p, txt).unwrap();
        assert_eq!(load(dir.path(), key), Err(TeamError::Quarantined));
        assert!(dir.path().join("team.json.quarantined").exists());
        assert!(!p.exists());
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path(), b"k").unwrap(), None);
    }
}
