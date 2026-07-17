//! Host-managed team board. `.zode` stays sandbox-read-only for tools — all
//! IO happens here in the host: a stable `board.lock` file (fs4 exclusive)
//! serializes writers, and `board.json` is replaced atomically (tmp→rename;
//! we never lock the data file itself — its inode changes on every write).
//! Section updates are CAS'd on a revision counter so two writers based on a
//! stale snapshot cannot silently overwrite each other.

use std::io::Write;
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::claims::ClaimEntry;
use super::TeamError;

pub const MAX_SECTION_BYTES: usize = 16 * 1024;
pub const MAX_NOTE_BYTES: usize = 4 * 1024;
pub const MAX_BOARD_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub revision: u64,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub sections: IndexMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub claims: Vec<ClaimEntry>,
}

/// CAS conflict: carries the CURRENT snapshot so the caller can rebase.
/// The snapshot is boxed to keep the enum (and the `Result` returning it)
/// small.
#[derive(Debug)]
pub enum BoardConflict {
    Stale(Box<BoardSnapshot>),
    Team(TeamError),
}

/// Refuse a symlink at the `.zode` or `.zode/team` components — the two
/// directories the host owns. System-level symlinks higher up the path (e.g.
/// macOS `/var` → `/private/var`) are legitimate and not our concern; a
/// redirected `.zode/team`, however, would let a tool escape the read-only
/// `.zode` sandbox carveout.
pub(crate) fn ensure_no_symlink(team_dir: &Path) -> Result<(), TeamError> {
    let candidates = [Some(team_dir), team_dir.parent()];
    for cand in candidates.into_iter().flatten() {
        if let Ok(m) = std::fs::symlink_metadata(cand) {
            if m.file_type().is_symlink() {
                return Err(TeamError::Io(format!(
                    "refusing symlinked team path component: {}",
                    cand.display()
                )));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Board {
    team_dir: PathBuf,
}

impl Board {
    pub fn new(team_dir: PathBuf) -> Self {
        Self { team_dir }
    }

    fn data_path(&self) -> PathBuf {
        self.team_dir.join("board.json")
    }
    fn lock_path(&self) -> PathBuf {
        self.team_dir.join("board.lock")
    }

    /// Take the writer lock. Creates the team dir lazily (mutating paths
    /// only — `read` never calls this).
    fn lock(&self) -> Result<std::fs::File, TeamError> {
        ensure_no_symlink(&self.team_dir)?;
        std::fs::create_dir_all(&self.team_dir).map_err(|e| TeamError::Io(e.to_string()))?;
        let f = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.lock_path())
            .map_err(|e| TeamError::Io(e.to_string()))?;
        FileExt::lock_exclusive(&f).map_err(|e| TeamError::Io(e.to_string()))?;
        Ok(f)
    }

    fn load_unlocked(&self) -> Result<BoardSnapshot, TeamError> {
        match std::fs::read_to_string(self.data_path()) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| TeamError::Io(format!("board.json: {e}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BoardSnapshot::default()),
            Err(e) => Err(TeamError::Io(e.to_string())),
        }
    }

    fn store_unlocked(&self, snap: &BoardSnapshot) -> Result<(), TeamError> {
        let json = serde_json::to_string_pretty(snap).map_err(|e| TeamError::Io(e.to_string()))?;
        if json.len() > MAX_BOARD_BYTES {
            return Err(TeamError::Io(format!(
                "board exceeds {MAX_BOARD_BYTES} bytes"
            )));
        }
        let tmp = self.team_dir.join("board.json.tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| TeamError::Io(e.to_string()))?;
            f.write_all(json.as_bytes())
                .map_err(|e| TeamError::Io(e.to_string()))?;
            f.flush().map_err(|e| TeamError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, self.data_path()).map_err(|e| TeamError::Io(e.to_string()))
    }

    /// Read-only snapshot. No team yet → empty snapshot at revision 0.
    /// Never creates directories or files.
    pub fn read(&self) -> Result<BoardSnapshot, TeamError> {
        ensure_no_symlink(&self.team_dir)?;
        self.load_unlocked()
    }

    /// CAS section write: `expected_revision` must match the stored one, or
    /// the CURRENT snapshot comes back for a rebase.
    pub fn update_section(
        &self,
        section: &str,
        content: &str,
        expected_revision: u64,
    ) -> Result<BoardSnapshot, BoardConflict> {
        if content.len() > MAX_SECTION_BYTES {
            return Err(BoardConflict::Team(TeamError::Io(format!(
                "section exceeds {MAX_SECTION_BYTES} bytes"
            ))));
        }
        let _lock = self.lock().map_err(BoardConflict::Team)?;
        let mut snap = self.load_unlocked().map_err(BoardConflict::Team)?;
        if snap.revision != expected_revision {
            return Err(BoardConflict::Stale(Box::new(snap)));
        }
        snap.sections
            .insert(section.to_string(), content.to_string());
        snap.revision += 1;
        self.store_unlocked(&snap).map_err(BoardConflict::Team)?;
        Ok(snap)
    }

    /// Append-only note (no CAS — appends commute).
    pub fn append_note(&self, note: &str) -> Result<(), TeamError> {
        if note.len() > MAX_NOTE_BYTES {
            return Err(TeamError::Io(format!(
                "note exceeds {MAX_NOTE_BYTES} bytes"
            )));
        }
        let _lock = self.lock()?;
        let mut snap = self.load_unlocked()?;
        snap.notes.push(note.to_string());
        snap.revision += 1;
        self.store_unlocked(&snap)
    }

    /// Mutate claims under the writer lock (used by the claims module; keeps
    /// claims in the same atomic snapshot as the rest of the board).
    pub(crate) fn with_claims<R>(
        &self,
        f: impl FnOnce(&mut Vec<ClaimEntry>) -> Result<R, TeamError>,
    ) -> Result<R, TeamError> {
        let _lock = self.lock()?;
        let mut snap = self.load_unlocked()?;
        let out = f(&mut snap.claims)?;
        snap.revision += 1;
        self.store_unlocked(&snap)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cas_rejects_stale_revision_and_returns_latest() {
        let dir = tempfile::tempdir().unwrap();
        let board = Board::new(dir.path().join(".zode/team"));
        let snap = board.read().unwrap();
        assert_eq!(snap.revision, 0);
        board.update_section("plan", "v1", 0).unwrap();
        let err = board
            .update_section("plan", "v2-from-stale", 0)
            .unwrap_err();
        match err {
            BoardConflict::Stale(latest) => {
                assert_eq!(latest.revision, 1);
                assert_eq!(latest.sections["plan"], "v1");
            }
            BoardConflict::Team(e) => panic!("expected stale, got {e}"),
        }
    }

    #[test]
    fn size_limits_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let board = Board::new(dir.path().join(".zode/team"));
        assert!(board
            .update_section("s", &"x".repeat(MAX_SECTION_BYTES + 1), 0)
            .is_err());
        assert!(board.append_note(&"x".repeat(MAX_NOTE_BYTES + 1)).is_err());
    }

    #[test]
    fn read_without_a_team_does_no_io() {
        let dir = tempfile::tempdir().unwrap();
        let board = Board::new(dir.path().join(".zode/team"));
        assert_eq!(board.read().unwrap(), BoardSnapshot::default());
        assert!(!dir.path().join(".zode").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_team_dir_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere");
        std::fs::create_dir(&real).unwrap();
        let zode = dir.path().join(".zode");
        std::fs::create_dir(&zode).unwrap();
        std::os::unix::fs::symlink(&real, zode.join("team")).unwrap();
        let board = Board::new(zode.join("team"));
        assert!(board.read().is_err());
    }
}
