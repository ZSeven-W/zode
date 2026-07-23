//! Claim leases — the host-side answer to "parallel agents in one working
//! tree". A claim reserves a path (file or subtree) for one holder with a
//! TTL; conflicting claims are rejected atomically (all-or-nothing). Honest
//! boundary: a lease prevents DOUBLE-CLAIMING, not out-of-bounds writes —
//! violations are detected after the fact by diffing `changed_files` against
//! the holder's claims.
//!
//! Holder identity is injected by the host (roster name), never read from
//! model-supplied input — `ToolUseContext` has no teammate identity, so a
//! shared tool instance could not tell callers apart.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::board::Board;
use super::TeamError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimEntry {
    pub holder: String,
    /// Canonicalized, cwd-relative path (lexically normalized).
    pub path: String,
    pub expires_at_ms: u64,
}

#[derive(Debug)]
pub struct ClaimConflict {
    pub conflicts: Vec<ClaimEntry>,
}

/// The canonical paths covered by one claim request and the subset inserted
/// by that request. Callers may renew every requested path, but must release
/// only `acquired` so a pre-existing claim from the same holder survives.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaimLease {
    pub requested: Vec<String>,
    pub acquired: Vec<String>,
}

/// Canonicalize a claim path: canonicalize the NEAREST EXISTING ancestor,
/// then lexically normalize the remainder (so brand-new files are claimable).
/// The result must stay inside the canonical cwd (cwd confinement).
pub fn canonicalize_claim(cwd: &Path, raw: &str) -> Result<PathBuf, TeamError> {
    let cwd = cwd
        .canonicalize()
        .map_err(|e| TeamError::Io(format!("cwd: {e}")))?;
    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    // Split into existing prefix + non-existing tail.
    let mut prefix = joined.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !prefix.exists() {
        match (prefix.file_name(), prefix.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                prefix = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut resolved = prefix
        .canonicalize()
        .map_err(|e| TeamError::Io(format!("{}: {e}", prefix.display())))?;
    for name in tail.iter().rev() {
        // Lexical normalization of the never-created remainder.
        match name.to_str() {
            Some(".") => {}
            Some("..") => {
                return Err(TeamError::Io(format!("claim path escapes via '..': {raw}")));
            }
            _ => resolved.push(name),
        }
    }
    let rel = resolved
        .strip_prefix(&cwd)
        .map_err(|_| TeamError::Io(format!("claim path outside the workspace: {raw}")))?;
    if rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(TeamError::Io(format!("claim path escapes: {raw}")));
    }
    Ok(rel.to_path_buf())
}

/// Subtree-aware overlap: equal paths conflict, and a directory claim
/// conflicts with anything beneath it — in both directions.
fn overlaps(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long.strip_prefix(short)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Atomically claim `paths` for `holder`. Expired claims are swept first;
/// ANY conflict rejects the whole batch and reports the conflicting entries.
pub fn claim(
    board: &Board,
    holder: &str,
    paths: &[String],
    cwd: &Path,
    ttl: Duration,
    now_ms: u64,
) -> Result<ClaimLease, ClaimConflict> {
    let mut canonical = Vec::with_capacity(paths.len());
    for raw in paths {
        match canonicalize_claim(cwd, raw) {
            Ok(p) => canonical.push(p.to_string_lossy().to_string()),
            Err(e) => {
                return Err(ClaimConflict {
                    conflicts: vec![ClaimEntry {
                        holder: format!("<invalid: {e}>"),
                        path: raw.clone(),
                        expires_at_ms: 0,
                    }],
                });
            }
        }
    }
    let requested = canonical.clone();
    let expires_at_ms = now_ms.saturating_add(ttl.as_millis() as u64);
    let holder = holder.to_string();
    board
        .with_claims(move |claims| {
            claims.retain(|c| c.expires_at_ms > now_ms);
            let conflicts: Vec<ClaimEntry> = claims
                .iter()
                .filter(|c| c.holder != holder && canonical.iter().any(|p| overlaps(&c.path, p)))
                .cloned()
                .collect();
            if !conflicts.is_empty() {
                return Err(TeamError::Io(
                    serde_json::to_string(&conflicts).unwrap_or_default(),
                ));
            }
            let mut acquired = Vec::new();
            for p in canonical {
                if let Some(existing) = claims
                    .iter_mut()
                    .find(|c| c.holder == holder && c.path == p)
                {
                    existing.expires_at_ms = existing.expires_at_ms.max(expires_at_ms);
                } else {
                    claims.push(ClaimEntry {
                        holder: holder.clone(),
                        path: p.clone(),
                        expires_at_ms,
                    });
                    acquired.push(p);
                }
            }
            Ok(acquired)
        })
        .map(|acquired| ClaimLease {
            requested,
            acquired,
        })
        .map_err(|e| match e {
            TeamError::Io(json) => ClaimConflict {
                conflicts: serde_json::from_str(&json).unwrap_or_default(),
            },
            other => ClaimConflict {
                conflicts: vec![ClaimEntry {
                    holder: format!("<error: {other}>"),
                    path: String::new(),
                    expires_at_ms: 0,
                }],
            },
        })
}

/// Release `holder`'s claims (`None` = all of them).
pub fn release(board: &Board, holder: &str, paths: Option<&[String]>) {
    let holder = holder.to_string();
    let paths: Option<Vec<String>> = paths.map(|p| p.to_vec());
    let _ = board.with_claims(move |claims| {
        claims.retain(|c| {
            c.holder != holder
                || paths
                    .as_ref()
                    .is_some_and(|ps| !ps.iter().any(|p| p == &c.path))
        });
        Ok(())
    });
}

/// Extend the TTL of the selected `holder` claims (backend-neutral renewal —
/// TeamManager drives this while a send is in flight).
pub fn renew(board: &Board, holder: &str, paths: &[String], ttl: Duration, now_ms: u64) {
    let holder = holder.to_string();
    let paths = paths.to_vec();
    let expires_at_ms = now_ms.saturating_add(ttl.as_millis() as u64);
    let _ = board.with_claims(move |claims| {
        for c in claims
            .iter_mut()
            .filter(|c| c.holder == holder && paths.iter().any(|path| path == &c.path))
        {
            c.expires_at_ms = c.expires_at_ms.max(expires_at_ms);
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(600);

    fn setup() -> (tempfile::TempDir, PathBuf, Board) {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(cwd.join("src/api")).unwrap();
        let board = Board::new(cwd.join(".zode/team"));
        (dir, cwd, board)
    }

    #[test]
    fn subtree_overlap_conflicts_both_directions() {
        let (_d, cwd, board) = setup();
        claim(&board, "alice", &["src".into()], &cwd, TTL, 1_000).unwrap();
        let e = claim(
            &board,
            "bob",
            &["src/api/handler.rs".into()],
            &cwd,
            TTL,
            1_000,
        )
        .unwrap_err();
        assert_eq!(e.conflicts[0].holder, "alice");
        release(&board, "alice", None);
        claim(
            &board,
            "bob",
            &["src/api/handler.rs".into()],
            &cwd,
            TTL,
            1_000,
        )
        .unwrap();
        assert!(claim(&board, "alice", &["src".into()], &cwd, TTL, 1_000).is_err());
    }

    #[test]
    fn expired_claims_are_swept_and_new_files_claimable() {
        let (_d, cwd, board) = setup();
        claim(
            &board,
            "alice",
            &["src/new-file.rs".into()],
            &cwd,
            TTL,
            1_000,
        )
        .unwrap();
        let later = 1_000 + TTL.as_millis() as u64 + 1;
        claim(&board, "bob", &["src/new-file.rs".into()], &cwd, TTL, later).unwrap();
    }

    #[test]
    fn claims_are_confined_to_cwd() {
        let (_d, cwd, _b) = setup();
        assert!(canonicalize_claim(&cwd, "../outside.rs").is_err());
        assert!(canonicalize_claim(&cwd, "/etc/passwd").is_err());
        assert!(
            canonicalize_claim(&cwd, "src/api/new.rs").is_ok(),
            "new files ok"
        );
    }

    #[test]
    fn claim_returns_exact_canonical_paths_written_to_board() {
        let (_d, cwd, board) = setup();
        let absolute = cwd.join("src/api").to_string_lossy().to_string();
        let claimed = claim(
            &board,
            "alice",
            &["./src".into(), absolute],
            &cwd,
            TTL,
            1_000,
        )
        .unwrap();

        assert_eq!(claimed.requested, vec!["src", "src/api"]);
        assert_eq!(claimed.acquired, vec!["src", "src/api"]);
        let stored: Vec<String> = board
            .read()
            .unwrap()
            .claims
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(stored, claimed.acquired);

        let repeated = claim(&board, "alice", &["./src".into()], &cwd, TTL, 2_000).unwrap();
        assert_eq!(repeated.requested, vec!["src"]);
        assert!(repeated.acquired.is_empty());
        assert_eq!(board.read().unwrap().claims.len(), 2);
    }

    #[test]
    fn renew_extends_and_batch_is_atomic() {
        let (_d, cwd, board) = setup();
        claim(&board, "alice", &["src/api".into()], &cwd, TTL, 1_000).unwrap();
        // bob's batch has one clean and one conflicting path → whole batch fails
        let e = claim(
            &board,
            "bob",
            &["docs.md".into(), "src/api/x.rs".into()],
            &cwd,
            TTL,
            1_000,
        )
        .unwrap_err();
        assert!(!e.conflicts.is_empty());
        // the clean path must NOT have been claimed
        claim(&board, "carol", &["docs.md".into()], &cwd, TTL, 1_000).unwrap();
        renew(
            &board,
            "alice",
            &["src/api".into()],
            Duration::from_secs(1200),
            1_000,
        );
        let snap = board.read().unwrap();
        let alice = snap.claims.iter().find(|c| c.holder == "alice").unwrap();
        assert!(alice.expires_at_ms >= 1_000 + 1_200_000);
    }
}
