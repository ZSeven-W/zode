//! Events that drive the TUI main loop. Terminal input comes straight from
//! crossterm's EventStream in the select!; the agent turn and the tick are
//! delivered as AppEvents over an mpsc channel.
//!
//! Each agent event carries the `tab_id` of the session tab that spawned the
//! turn (so the app routes it to the right tab) and the `turn_id` of the turn
//! that produced it (so the app can drop events from an aborted/superseded
//! turn — the agent `Event` itself has no turn identity).

use agent::stream::Event;
use zode_core::{EngineTemplate, ZodeEngine};

use crate::ui::dialog::connect::ConnectDialog;

#[derive(Debug, Clone)]
pub enum ReassembleNotify {
    None,
    Toast(String),
    System(String),
}

#[derive(Debug, Clone)]
pub enum ReassembleEffect {
    AgentReload {
        notify: ReassembleNotify,
        refresh_dialog: bool,
    },
    Connect {
        provider_name: String,
    },
    Effort {
        notify: ReassembleNotify,
    },
    Goal {
        goal: Option<String>,
    },
    Model {
        id: String,
    },
    /// A fresh tab (Ctrl+T) whose engine was assembled off-loop. The tab
    /// already exists as a busy placeholder; on failure it is removed.
    NewTab,
    /// A resumed session's engine (with its store attached) assembled
    /// off-loop. On success the transcript is rebuilt from the store; on
    /// failure the placeholder tab is removed.
    ResumeTab,
    Notify(ReassembleNotify),
    Orchestration {
        on: bool,
        notify: ReassembleNotify,
    },
    Plan {
        on: bool,
    },
    ReloadSkills,
    Sandbox,
    Yolo {
        notify: ReassembleNotify,
    },
}

pub struct ReassembledEngine {
    pub template: EngineTemplate,
    pub engine: ZodeEngine,
}

// A high-frequency, short-lived event enum: the `Agent` variant streams one per
// token. Boxing its payload to equalize variant size would add a heap allocation
// on the hottest path, so we accept the size difference here.
#[allow(clippy::large_enum_variant)]
pub enum AppEvent {
    /// One event from turn `turn_id` running in tab `tab_id`.
    Agent {
        tab_id: usize,
        turn_id: u64,
        cost_label: Option<String>,
        event: Event,
    },
    /// Turn `turn_id` in tab `tab_id` finished (Ok) or errored.
    TurnDone {
        tab_id: usize,
        turn_id: u64,
        result: Result<(), String>,
    },
    /// A transient toast, posted from off-loop work (e.g. /undo running on a
    /// spawned task) so the event loop is never blocked. Not tab-scoped.
    Toast { text: String, error: bool },
    /// A manual `/compact` run (spawned off-loop) finished. `result` is a
    /// ready-to-show summary line on success, or an error message. Routed to
    /// the originating tab so its busy state is cleared and the note lands in
    /// the right transcript.
    CompactDone {
        tab_id: usize,
        result: Result<String, String>,
        /// Whether the compaction was auto-triggered (context threshold)
        /// rather than a manual `/compact`. Auto failures feed the per-tab
        /// circuit breaker so a failing provider can't loop compactions.
        auto: bool,
    },
    /// A progress line from an off-loop background op (e.g. a direct `/op`
    /// tool/MCP/design call). Pushed into the originating tab's transcript so a
    /// long task shows live status instead of freezing the UI. Tab-scoped, not
    /// turn-scoped; dropped if the tab is no longer busy (the user interrupted).
    BgProgress { tab_id: usize, line: String },
    /// An off-loop background op finished. `result` is a ready-to-show line on
    /// success or an error message. Clears the tab's busy state and posts the
    /// result. Mirrors `CompactDone` but for the generic direct-invocation path.
    BgDone {
        tab_id: usize,
        result: Result<String, String>,
    },
    /// A throttled background git working-tree poll (for the sidebar
    /// "modified files" section) finished. `files` is `None` when the tab's
    /// cwd is not inside a git work tree.
    GitStatDone {
        tab_id: usize,
        files: Option<Vec<zode_core::GitFileStat>>,
    },
    /// A `!<cmd>` shell escape (run off-loop so the UI never freezes on a slow
    /// command) finished. `output` is the captured stdout+stderr; `None` means
    /// the user interrupted it (Esc killed the child — nothing to show, the
    /// interrupt handler already posted "(interrupted)"). `owned_slot` is
    /// whether this run took the tab's turn-busy slot (it started while the
    /// tab was idle) — only then may completion release the slot.
    LocalShellDone {
        tab_id: usize,
        cmd: String,
        output: Option<String>,
        owned_slot: bool,
    },
    /// The `/connect` dialog, built off-loop (the catalog + config reads are
    /// small local files, but any sync disk I/O in the event loop can
    /// stutter). Opens on arrival unless a modal took the screen meanwhile.
    ConnectDialogReady { dialog: Box<ConnectDialog> },
    /// A model/provider/config change finished rebuilding the tab's engine
    /// off-loop. `seq` drops stale completions if a tab is closed/reused or a
    /// later rebuild supersedes it.
    ReassembleDone {
        tab_id: usize,
        seq: u64,
        effect: ReassembleEffect,
        result: Result<ReassembledEngine, String>,
    },
}
