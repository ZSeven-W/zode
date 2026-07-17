//! Agent team collaboration (spec v2.3 §4-§6, Phase B).
//!
//! A `TeamManager` is created per tab and carried across engine rebuilds via
//! `CarryState`. Teammates are persistent sessions — `Internal` (an in-process
//! QueryLoop over a shared `MessageStore`) or `External` (a resumable agent
//! CLI). Collaboration flows through the leader (the root model): a host-side
//! board with claim leases, and `@ask` relay parsed from teammate replies.
//! All state under `<cwd>/.zode/team/` is host-managed — the workspace
//! sandbox keeps `.zode` read-only for tools, so teammates use dedicated
//! identity-bound team tools instead of raw file access.

pub mod ask;
pub mod board;
pub mod claims;
pub mod external;
pub mod internal;
pub mod lease;
pub mod manager;
pub mod persist;
pub mod playbook;
pub mod preamble;
pub mod tools;

pub use manager::{HireRequest, SendOutcome, TeamDeps};

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use board::{Board, BoardConflict, BoardSnapshot};
pub use claims::ClaimEntry;

/// Team-layer errors, surfaced to the model as tool errors.
#[derive(Debug, Clone, PartialEq)]
pub enum TeamError {
    UnknownTeammate(String),
    Busy {
        desc: String,
    },
    NameInvalid(String),
    NameTaken(String),
    Occupied {
        pid: Option<u32>,
    },
    Io(String),
    /// team.json failed HMAC verification and was quarantined; the roster is
    /// NOT loaded — the user must re-hire explicitly.
    Quarantined,
}

impl std::fmt::Display for TeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeamError::UnknownTeammate(n) => write!(f, "unknown teammate '{n}'"),
            TeamError::Busy { desc } => write!(f, "teammate is busy: {desc}"),
            TeamError::NameInvalid(n) => write!(
                f,
                "invalid teammate name '{n}' (want [a-z0-9-], 1-32 chars)"
            ),
            TeamError::NameTaken(n) => write!(f, "teammate name '{n}' already taken"),
            TeamError::Occupied { pid } => write!(
                f,
                "team is held by another zode process{}",
                pid.map(|p| format!(" (pid {p})")).unwrap_or_default()
            ),
            TeamError::Io(e) => write!(f, "team io error: {e}"),
            TeamError::Quarantined => write!(
                f,
                "team.json failed integrity verification and was quarantined; re-hire explicitly"
            ),
        }
    }
}

impl std::error::Error for TeamError {}

/// Teammate execution status. `generation` guards against a late completion
/// callback clobbering a newer execution's state.
#[derive(Debug, Clone, PartialEq)]
pub enum TeammateStatus {
    Idle,
    Busy { desc: String, generation: u64 },
}

/// Backends hold `Arc` so a send can clone the handle out from under the
/// roster lock and drive it WITHOUT holding the lock across `.await` (which
/// would both block the whole team and risk a use-after-realloc if the Vec
/// grew). Internal sessions drive through `&self`; external ones need `&mut`
/// for session-id capture, so they sit behind an async mutex.
#[derive(Debug, Clone)]
pub enum TeammateBackend {
    Internal(Arc<internal::InternalSession>),
    External(Arc<tokio::sync::Mutex<external::ExternalSession>>),
}

/// Persistence-facing metadata mirrored on the roster so `persist` (sync)
/// never needs to lock the external session's async mutex. `session_id` is
/// updated after each external send.
#[derive(Debug, Clone)]
pub enum TeammateMeta {
    Internal {
        model: String,
        session_path: PathBuf,
    },
    External {
        profile: String,
        session_id: Option<String>,
    },
}

#[derive(Debug)]
pub struct Teammate {
    pub name: String,
    pub role: String,
    pub backend: TeammateBackend,
    pub status: TeammateStatus,
    /// Monotonic execution counter; bumped at every send.
    pub generation: u64,
    pub usage_in: u64,
    pub usage_out: u64,
    /// Display label, e.g. `codex(external)` / `reviewer(internal, haiku)`.
    pub model_label: String,
    /// Optional caller-supplied narrowing of the teammate's tool set
    /// (internal teammates only; may only subtract from the role default).
    pub allowed_tools: Option<Vec<String>>,
    pub meta: TeammateMeta,
}

/// Read-only roster row for the `/team` panel and `team_list`.
#[derive(Debug, Clone, PartialEq)]
pub struct TeammateSnapshot {
    pub name: String,
    pub role: String,
    pub model_label: String,
    pub status_line: String,
    pub usage_in: u64,
    pub usage_out: u64,
}

pub(crate) struct TeamState {
    pub(crate) teammates: Vec<Teammate>,
    pub(crate) lease: Option<lease::TeamLease>,
    pub(crate) shutdown: bool,
    /// Whether a persisted roster has been loaded this session (once).
    pub(crate) recovered: bool,
}

/// Per-tab team manager. Creation does NO IO (lazy: the lease and directory
/// scaffold happen on the first mutating team operation); read-only paths
/// (`roster`, board read without a team) never scaffold anything.
pub struct TeamManager {
    pub(crate) cwd: PathBuf,
    pub(crate) state: Mutex<TeamState>,
}

impl std::fmt::Debug for TeamManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamManager")
            .field("cwd", &self.cwd)
            .finish()
    }
}

impl TeamManager {
    pub fn new(cwd: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            cwd,
            state: Mutex::new(TeamState {
                teammates: Vec::new(),
                lease: None,
                shutdown: false,
                recovered: false,
            }),
        })
    }

    /// `<cwd>/.zode/team` — the host-managed team directory.
    pub fn team_dir(&self) -> PathBuf {
        self.cwd.join(".zode/team")
    }

    /// Teammate names: `[a-z0-9-]{1,32}` (compared case-insensitively at
    /// registration; uppercase input is rejected outright to keep names
    /// canonical in transcripts and @ask lines).
    pub fn validate_name(name: &str) -> Result<(), TeamError> {
        let ok = !name.is_empty()
            && name.len() <= 32
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok {
            Ok(())
        } else {
            Err(TeamError::NameInvalid(name.to_string()))
        }
    }

    /// Human-readable roster report for `/team` / `/team status`.
    pub fn status_report(&self) -> String {
        let roster = self.roster();
        if roster.is_empty() {
            return "No teammates. Use team_hire (via a task) to build a team.".to_string();
        }
        let mut out = format!("Team ({} teammate(s)):\n", roster.len());
        for t in &roster {
            out.push_str(&format!(
                "  {} [{}] — {} · ↑{} ↓{}\n",
                t.name, t.model_label, t.status_line, t.usage_in, t.usage_out
            ));
        }
        out.trim_end().to_string()
    }

    /// Board summary for `/team board`. Read-only; no scaffolding.
    pub fn board_report(&self) -> String {
        match board::Board::new(self.team_dir()).read() {
            Ok(snap) => {
                let mut out = format!("Board (rev {}):\n", snap.revision);
                if !snap.goal.is_empty() {
                    out.push_str(&format!("  goal: {}\n", snap.goal));
                }
                for (k, v) in &snap.sections {
                    out.push_str(&format!("  [{k}] {}\n", v.lines().next().unwrap_or("")));
                }
                for n in &snap.notes {
                    out.push_str(&format!("  note: {n}\n"));
                }
                for c in &snap.claims {
                    out.push_str(&format!("  claim: {} → {}\n", c.holder, c.path));
                }
                out.trim_end().to_string()
            }
            Err(e) => format!("board unavailable: {e}"),
        }
    }

    /// Read-only roster snapshot. Never performs IO.
    pub fn roster(&self) -> Vec<TeammateSnapshot> {
        let state = self.state.lock().expect("team state lock");
        state
            .teammates
            .iter()
            .map(|t| TeammateSnapshot {
                name: t.name.clone(),
                role: t.role.clone(),
                model_label: t.model_label.clone(),
                status_line: match &t.status {
                    TeammateStatus::Idle => "idle".to_string(),
                    TeammateStatus::Busy { desc, .. } => format!("busy: {desc}"),
                },
                usage_in: t.usage_in,
                usage_out: t.usage_out,
            })
            .collect()
    }

    /// Abort in-flight work, flush state, release the lease. Idempotent;
    /// explicit because Drop cannot await. Owned by tab close / app shutdown
    /// — an old engine dropped during a hot rebuild must NOT call this on a
    /// manager the new engine carried over.
    pub async fn shutdown(&self) {
        let mut state = self.state.lock().expect("team state lock");
        if state.shutdown {
            return;
        }
        state.shutdown = true;
        for t in &mut state.teammates {
            t.status = TeammateStatus::Idle;
        }
        // Dropping the lease releases the OS lock (the sole authority).
        state.lease = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teammate_name_rules() {
        assert!(TeamManager::validate_name("codex-impl").is_ok());
        assert!(
            TeamManager::validate_name("Codex").is_err(),
            "uppercase rejected"
        );
        assert!(TeamManager::validate_name("").is_err());
        assert!(TeamManager::validate_name(&"x".repeat(33)).is_err());
        assert!(TeamManager::validate_name("a b").is_err());
    }

    #[tokio::test]
    async fn shutdown_is_idempotent_and_new_does_no_io() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = TeamManager::new(dir.path().to_path_buf());
        assert!(!dir.path().join(".zode").exists(), "lazy: no dirs created");
        let _ = mgr.roster();
        assert!(
            !dir.path().join(".zode").exists(),
            "roster must not scaffold"
        );
        mgr.shutdown().await;
        mgr.shutdown().await; // must not panic
    }
}
