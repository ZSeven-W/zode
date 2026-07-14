//! Events that drive the TUI main loop. Terminal input comes straight from
//! crossterm's EventStream in the select!; the agent turn and the tick are
//! delivered as AppEvents over an mpsc channel.
//!
//! Each agent event carries the `tab_id` of the session tab that spawned the
//! turn (so the app routes it to the right tab) and the `turn_id` of the turn
//! that produced it (so the app can drop events from an aborted/superseded
//! turn — the agent `Event` itself has no turn identity).

use agent::stream::Event;
use serde_json::Value;
use zode_core::session_meta::SessionIndex;
use zode_core::{EngineTemplate, ToolAccessMode, ZodeEngine};

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
    /// Same lifecycle as `NewTab`, but created by one extension connection.
    /// Completion sends that connection a fresh authoritative snapshot.
    ExtensionNewTab {
        connection_id: u64,
        failure_code: Option<&'static str>,
    },
    /// Same lifecycle as `ResumeTab`, without changing terminal focus.
    ExtensionResumeTab {
        connection_id: u64,
        failure_code: Option<&'static str>,
    },
    /// A model/access change for any open extension task. The target may be a
    /// background tab, so completion carries every task-local value needed to
    /// install the result without consulting (or mutating) terminal focus.
    ExtensionReconfigure {
        connection_id: u64,
        failure_code: Option<&'static str>,
        model: String,
        access: ToolAccessMode,
    },
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
        access: ToolAccessMode,
        notify: ReassembleNotify,
    },
}

pub struct ReassembledEngine {
    pub template: EngineTemplate,
    pub engine: ZodeEngine,
}

/// Parsed extension request carried across the asynchronous index preflight.
/// Keeping this typed prevents a background completion from re-parsing or
/// accidentally applying a different method on the main loop.
#[derive(Debug, Clone)]
pub enum ExtensionTaskRequest {
    SnapshotRead {
        task_id: Option<String>,
    },
    Create,
    Select {
        task_id: String,
    },
    ModelSet {
        task_id: String,
        model: String,
    },
    PermissionSet {
        task_id: String,
        mode: ToolAccessMode,
    },
    TurnStart {
        task_id: String,
        input: String,
        attachment_ids: Vec<String>,
    },
    AttachmentBegin {
        task_id: String,
        name: String,
        media_type: String,
        size: usize,
    },
    AttachmentChunk {
        upload_id: String,
        sequence: u64,
        data: Vec<u8>,
    },
    AttachmentFinish {
        upload_id: String,
    },
    AttachmentCancel {
        upload_id: String,
    },
    TurnInterrupt {
        task_id: String,
        turn_id: u64,
    },
    ApprovalRespond {
        task_id: String,
        turn_id: u64,
        approval_id: String,
        decision: ExtensionApprovalDecision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionApprovalDecision {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone)]
pub struct ExtensionTaskFailure {
    pub code: String,
    pub message: String,
}

/// Why an index worker was started. Request work remains correlated by both
/// connection and request id; completion work emits an authoritative snapshot
/// followed by its optional failure event.
#[derive(Debug, Clone)]
pub enum ExtensionIndexPurpose {
    Request {
        request_id: String,
        request: ExtensionTaskRequest,
    },
    Completion {
        failure: Option<(String, String)>,
    },
}

#[derive(Debug, Clone)]
pub enum ExtensionSnapshotPurpose {
    Response {
        request_id: String,
        /// Only a successful snapshot/read handshake may claim orphaned live
        /// turn routes after its response has entered the outbound FIFO.
        rebind_orphan_routes: bool,
    },
    Completion {
        failure: Option<(String, String)>,
    },
}

/// Results produced outside the TUI loop. The main loop only validates the
/// connection/request context, applies short in-memory state changes, and
/// sends already-built protocol frames.
#[derive(Debug)]
pub enum ExtensionTaskEvent {
    IndexReady {
        connection_id: u64,
        purpose: ExtensionIndexPurpose,
        result: Result<SessionIndex, ExtensionTaskFailure>,
    },
    SnapshotReady {
        connection_id: u64,
        purpose: ExtensionSnapshotPurpose,
        result: Result<Value, ExtensionTaskFailure>,
    },
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
        /// Exact non-agent busy-slot generation that produced this event.
        op_id: u64,
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
    BgProgress {
        tab_id: usize,
        op_id: u64,
        line: String,
    },
    /// An off-loop background op finished. `result` is a ready-to-show line on
    /// success or an error message. Clears the tab's busy state and posts the
    /// result. Mirrors `CompactDone` but for the generic direct-invocation path.
    BgDone {
        tab_id: usize,
        op_id: u64,
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
    /// interrupt handler already posted "(interrupted)"). `op_id` is `Some`
    /// only when this run took the tab's turn-busy slot; concurrent shells use
    /// `None` and therefore never release another operation's slot.
    LocalShellDone {
        tab_id: usize,
        cmd: String,
        output: Option<String>,
        op_id: Option<u64>,
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
    /// Index/history work for the Chrome side-panel task protocol completed.
    /// Routed separately from turn events because applying it may start the
    /// next short background stage.
    ExtensionTask(ExtensionTaskEvent),
}
