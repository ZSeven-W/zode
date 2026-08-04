//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Stdout;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::message::{ContentBlock, Message, MessageStore};
use agent::session::Session;
use agent::stream::Event;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyModifiers, KeyboardEnhancementFlags,
    MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures::{FutureExt, StreamExt};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use tokio::sync::mpsc;
use uuid::Uuid;
use zode_core::approval::{Approval, ApprovalReceiver, ApprovalRequest};
use zode_core::bg_shells::BgShell;
use zode_core::commands::parse_slash;
use zode_core::config::{ConfigManager, ImageMode, ImagesConfig};
use zode_core::images::{split_pasted_image_paths, ImageAttachment};
use zode_core::question::{QuestionReceiver, QuestionRequest};
use zode_core::run_event::{RunEvent, RunEventContext, RunStatus, TurnOutcome, TurnRecorder};
use zode_core::scheduler::{DueKind, Scheduler};
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::sessions::SessionStore;
use zode_core::{EngineTemplate, ZodeEngine};

use crate::event::{AppEvent, ReassembleEffect, ReassembleNotify, ReassembledEngine};
use crate::tab::SessionTab;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::{Autocomplete, DynCmd};
use crate::ui::chat::{ChatRenderMeta, ChatSelection, ChatSelectionPoint, ChatView, ImagePreview};
use crate::ui::dialog::agents_dialog::{AgentKind, AgentRow, AgentsAction, AgentsDialog};
use crate::ui::dialog::browser_panel::{BrowserPanel, BrowserPanelAction, BrowserPanelStatus};
use crate::ui::dialog::connect::{ConnectAction, ConnectDialog, ConnectField, ConnectStage};
use crate::ui::dialog::mcp_dialog::McpDialog;
use crate::ui::dialog::permission::{PermissionDialog, PermissionRequestIdentity};
use crate::ui::dialog::plugin_picker::PluginPicker;
use crate::ui::dialog::question::QuestionDialog;
use crate::ui::dialog::session_picker::{DeletePress, SessionPicker};
use crate::ui::dialog::settings::{SettingsAction, SettingsDialog, SettingsLevel};
use crate::ui::dialog::tasks_panel::TasksPanel;
use crate::ui::dialog::workflows_dialog::{WorkflowRow, WorkflowsAction, WorkflowsDialog};
use crate::ui::input::{InputBox, InputSelection};
use crate::ui::layout::{render_header, split_main, HeaderInfo};
use crate::ui::mention::{
    at_mention_query, collect_cwd_files, MentionItem, MentionKind, MentionPicker,
};
use crate::ui::status::{Mode, StatusBar};
use crate::ui::tabs::{render_sidebar, SidebarInfo};
use crate::ui::toast::Toast;

mod extension_attachments;
mod extension_tasks;
mod watchdog;

use watchdog::{
    BackgroundWatchdog, Failure as WatchdogFailure, FailureCause, Recovery, WatchdogAction,
};

type SharedTurnRecorder = Arc<std::sync::Mutex<TurnRecorder>>;

const PROMPT_HISTORY_FILE: &str = "prompt_history.json";
/// Cap on persisted prompt-history entries PER SESSION. When exceeded, the
/// OLDEST are dropped first (FIFO) — see `record_prompt_history_entry`.
const PROMPT_HISTORY_LIMIT: usize = 100;

/// First turn of the autonomous goal loop, queued when a goal is set.
const GOAL_LOOP_START_PROMPT: &str =
    "Begin working toward the goal now. Take concrete steps. When it is fully \
     complete, call the GoalComplete tool with a short summary.";
/// Continuation turn, queued after each successful loop turn that did not signal
/// completion.
const GOAL_LOOP_CONTINUE_PROMPT: &str =
    "Continue working toward the goal. Take the next concrete step now. When it \
     is fully complete, call the GoalComplete tool with a short summary — do \
     not call it prematurely.";

/// Default cap on autonomous goal-loop turns when `autoLoopMaxTurns` is unset.
/// Without a default an unbounded loop can burn tokens indefinitely; the user
/// can raise it via config or just resume by sending a message.
const GOAL_LOOP_DEFAULT_MAX_TURNS: u32 = 25;

/// Consecutive goal-loop turns with NO tool use before the loop stops for
/// lack of progress: a model that keeps replying "I'll continue" without
/// doing any work (no tool calls, no diff) is spinning, not progressing.
const GOAL_LOOP_NO_PROGRESS_LIMIT: u32 = 3;

/// A Tokio abort is only a request: nested provider/tool workers may still be
/// running after their owner future returns. Watched turns get this secondary
/// deadline before the tab and schedule are quarantined. The lease remains
/// held after the deadline until every tracked worker actually drops.
const HARD_STOP_QUIESCE_TIMEOUT: Duration = Duration::from_secs(5);
const SCHEDULE_ROSTER_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SCHEDULE_FINALIZER_RETRY_INTERVAL: Duration = Duration::from_secs(1);

fn prompt_history_path() -> Option<std::path::PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|dir| dir.join(PROMPT_HISTORY_FILE))
}

fn load_prompt_history(history_key: &str) -> Vec<String> {
    prompt_history_path()
        .as_deref()
        .map(|path| load_prompt_history_from_path(path, history_key))
        .unwrap_or_default()
}

fn save_prompt_history(history_key: &str, history: &[String]) {
    let Some(path) = prompt_history_path() else {
        return;
    };
    if let Err(e) = save_prompt_history_to_path(&path, history_key, history) {
        tracing::warn!(error = %e, path = %path.display(), "failed to save prompt history");
    }
}

/// Read the whole session-keyed history map from disk. A legacy flat-array
/// file (`["a","b"]`) is migrated under `history_key` so existing records are
/// never lost. Missing/corrupt files yield an empty map (logged), so a bad
/// file never wipes the user's history on the next save.
fn load_history_map(path: &Path, history_key: &str) -> BTreeMap<String, Vec<String>> {
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let value = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to parse prompt history");
            return BTreeMap::new();
        }
    };
    let mut map = BTreeMap::new();
    match value {
        // New format: { "<project-cwd>": ["entry", ...], ... }.
        v @ serde_json::Value::Object(_) => {
            match serde_json::from_value::<BTreeMap<String, Vec<String>>>(v) {
                Ok(parsed) => {
                    for (key, entries) in parsed {
                        map.insert(key, sanitize_prompt_history(entries));
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "prompt history map had unexpected shape");
                }
            }
        }
        // Legacy format: a flat array → migrate into the current project.
        v @ serde_json::Value::Array(_) => {
            if let Ok(entries) = serde_json::from_value::<Vec<String>>(v) {
                let entries = sanitize_prompt_history(entries);
                if !entries.is_empty() {
                    map.insert(history_key.to_string(), entries);
                }
            }
        }
        _ => {
            tracing::warn!(path = %path.display(), "prompt history had unexpected JSON type");
        }
    }
    map
}

fn load_prompt_history_from_path(path: &Path, history_key: &str) -> Vec<String> {
    load_history_map(path, history_key)
        .remove(history_key)
        .unwrap_or_default()
}

fn save_prompt_history_to_path(
    path: &Path,
    history_key: &str,
    history: &[String],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Read-modify-write: preserve every OTHER bucket and migrate a legacy
    // flat-array file. This session's own bucket is merged ADDITIVELY —
    // buckets are project-scoped, so two live sessions in one workspace
    // write the same key; replacing wholesale would drop whatever the other
    // session recorded since this one was seeded.
    let mut map = load_history_map(path, history_key);
    let mut entries = map.remove(history_key).unwrap_or_default();
    for entry in sanitize_prompt_history(history.to_vec()) {
        if !entries.contains(&entry) {
            record_prompt_history_entry(&mut entries, &entry);
        }
    }
    if entries.is_empty() {
        map.remove(history_key);
    } else {
        map.insert(history_key.to_string(), entries);
    }
    let json = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn sanitize_prompt_history(entries: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for entry in entries {
        record_prompt_history_entry(&mut out, &entry);
    }
    out
}

fn record_prompt_history_entry(history: &mut Vec<String>, text: &str) -> bool {
    let text = text.trim();
    // Skip blanks, consecutive dups, and a bare single-line slash command
    // (e.g. `/sandbox`, `/model x`) — those are UI actions, not prompts worth
    // recalling. A multi-line message that happens to start with `/` is kept.
    if text.is_empty()
        || (text.starts_with('/') && !text.contains('\n'))
        || history.last().map(String::as_str) == Some(text)
    {
        return false;
    }
    history.push(text.to_string());
    if history.len() > PROMPT_HISTORY_LIMIT {
        let excess = history.len() - PROMPT_HISTORY_LIMIT;
        history.drain(0..excess);
    }
    true
}

fn seed_prompt_history_for_tab(tab: &mut SessionTab) {
    // PROJECT-scoped (see `SessionTab::new`): every session in the same
    // workspace shares one Up/Down recall bucket, like Claude Code.
    tab.prompt_history_key = format!("project:{}", tab.engine.cwd.display());
    let mut history = load_prompt_history(&tab.prompt_history_key);
    for msg in tab.chat.messages() {
        if msg.role == crate::ui::chat::Role::User {
            record_prompt_history_entry(&mut history, &msg.text);
        }
    }
    tab.prompt_history = history;
    tab.history_pos = None;
    tab.history_draft.clear();
}

/// Render one design-pipeline phase as a transcript progress line. Kept in the
/// TUI (not zode-core) so wording/formatting is a presentation concern.
fn design_progress_line(p: &zode_core::openpencil::design::DesignProgress) -> String {
    use zode_core::openpencil::design::DesignProgress as P;
    match p {
        P::Planning => "planning layout...".to_string(),
        P::Planned { sections } => format!("planned {sections} sections"),
        P::SkeletonReady { sections } => format!("skeleton ready ({sections} sections)"),
        P::Section { index, total } => format!("section {index}/{total}: generating..."),
        P::SectionDone { index, total } => format!("section {index}/{total}: done"),
        P::SectionFailed {
            index,
            total,
            error,
        } => format!("section {index}/{total}: failed - {error}"),
        P::Refining => "refining...".to_string(),
    }
}

/// A `/browser` op run off the event loop by `spawn_browser_op`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserOp {
    Launch,
    Close,
    /// `None` saves under the config dir's `screenshots/` with a timestamped
    /// name (mirrors `BrowserRead`'s own screenshot-save path in zode-core).
    Screenshot {
        path: Option<String>,
    },
}

/// Run one `BrowserOp` against the shared session. Free function (not a
/// method) so it can be moved into `tokio::spawn` without borrowing `App`.
async fn run_browser_op(
    session: Arc<zode_core::browser::BrowserSession>,
    op: BrowserOp,
) -> Result<String, String> {
    match op {
        BrowserOp::Launch => session
            .lease()
            .await
            .map(|_lease| "browser launched".to_string())
            .map_err(|e| e.to_string()),
        BrowserOp::Close => {
            session.close().await;
            Ok("browser closed".to_string())
        }
        BrowserOp::Screenshot { path } => {
            let lease = session.lease().await.map_err(|e| e.to_string())?;
            let shot = lease
                .backend()
                .screenshot()
                .await
                .map_err(|e| e.to_string())?;
            drop(lease); // release the browser before disk I/O
            let dest = match path {
                Some(p) => std::path::PathBuf::from(p),
                None => {
                    let dir = ConfigManager::config_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from(".zode"))
                        .join("screenshots");
                    let name = format!(
                        "shot-{}.jpg",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                    );
                    dir.join(name)
                }
            };
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("screenshot dir: {e}"))?;
            }
            std::fs::write(&dest, &shot.bytes).map_err(|e| format!("save screenshot: {e}"))?;
            Ok(format!("screenshot saved: {}", dest.display()))
        }
    }
}

#[derive(Debug, Clone)]
struct CompletionHint {
    prefix: String,
    placeholder: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageSubmitRoute {
    Direct,
    VisionModel,
    Unsupported,
}

struct PreparedTabInterrupt {
    tab_id: usize,
    turn_id: Option<u64>,
    abort: AbortController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiApprovalCleanupTarget {
    Source { tab_id: usize },
    Turn { tab_id: usize, turn_id: u64 },
    LocalOperation { tab_id: usize, op_id: u64 },
}

impl TuiApprovalCleanupTarget {
    fn matches(self, source: Option<&str>, turn_id: Option<u64>, local_op_id: Option<u64>) -> bool {
        let Some(source_tab_id) = source.and_then(|source| source.parse::<usize>().ok()) else {
            return false;
        };
        match self {
            Self::Source { tab_id } => source_tab_id == tab_id,
            Self::Turn {
                tab_id,
                turn_id: id,
            } => source_tab_id == tab_id && turn_id == Some(id) && local_op_id.is_none(),
            Self::LocalOperation { tab_id, op_id } => {
                source_tab_id == tab_id && turn_id.is_none() && local_op_id == Some(op_id)
            }
        }
    }

    fn matches_request(self, request: &ApprovalRequest) -> bool {
        self.matches(
            request.source.as_deref(),
            request.turn_id,
            request.local_op_id,
        )
    }

    fn matches_identity(self, identity: &PermissionRequestIdentity) -> bool {
        self.matches(
            identity.source.as_deref(),
            identity.turn_id,
            identity.local_op_id,
        )
    }
}

pub struct UiConfig {
    pub theme_id: Option<String>,
    pub yolo: bool,
    /// Tool access actually used to assemble the initial engine. It can differ
    /// from the clean global `yolo` default during resume (including a failed
    /// transcript load), so the initial tab must record it explicitly.
    pub initial_access: zode_core::ToolAccessMode,
    pub sandbox: bool,
    /// Named providers (config.providers keys) for the settings dialog.
    pub provider_names: Vec<String>,
    /// No provider credentials are configured yet — show a one-time setup hint
    /// in the transcript pointing the user at `/connect`.
    pub needs_setup: bool,
    /// Set by the background self-updater when a new build was swapped in
    /// (value = the release tag). The TUI polls it on its tick and shows a
    /// one-time "restart to apply" notice. `None` → no updater wired (tests,
    /// or update disabled).
    pub update_applied: Option<std::sync::Arc<std::sync::OnceLock<String>>>,
}

/// Identifies which scheduler job queued a prompt, for turn-outcome
/// attribution: `App::sched_pending` maps each queued prompt occurrence to its
/// job so a turn-start call site (`submit()` for the active tab,
/// `dispatch_scheduler_queued()` for the tick-driven drain) can stamp
/// `SessionTab::active_sched_job`, and `TurnDone` consumes it to update
/// `App::sched_fail_streak` (the 3-strikes circuit breaker). `Loop` ids are
/// process-local `u32`s from `Scheduler::add_loop`; `Schedule` ids are the
/// persisted 12-hex-char ids from `schedules.json` (legacy ids remain valid).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SchedJobRef {
    Loop(u32),
    Schedule(String),
}

/// Key into [`TuiApp::sched_pending`]: `(SessionTab::id, exact prompt text)`.
///
/// The tab id is part of the key because the prompt alone is not unique — two
/// tabs can run identically-worded jobs, and a *stale* entry (one whose queued
/// prompt was purged, or whose turn bailed before starting) must not be able to
/// capture an unrelated user-typed message that happens to read the same on
/// another tab. The value is an occurrence-ordered queue: distinct jobs on the
/// same tab may intentionally have identical prompt text, and each queued copy
/// must retain its own attribution.
type SchedPendingKey = (usize, String);
type SchedPendingJobs = VecDeque<SchedJobRef>;

#[derive(Debug, Clone)]
enum ForcedTurnStop {
    Watchdog(WatchdogFailure),
    Manual {
        job: SchedJobRef,
        failure: Option<WatchdogFailure>,
    },
    Canonical {
        job: SchedJobRef,
        result: Result<(), String>,
    },
}

#[derive(Debug, Clone)]
struct QuarantinedRecovery {
    job: SchedJobRef,
    failures: u32,
    last_failure_ms: u64,
}

struct PendingForcedTurnStop {
    outcome: ForcedTurnStop,
    attempt_lease: Option<zode_core::scheduler::ScheduleAttemptLease>,
    activity: Option<agent::abort::TurnActivity>,
    quarantine: Option<QuarantinedRecovery>,
    source_terminal_seen: bool,
    /// The turn's shared recorder and engine, captured so a forced stop can
    /// journal the terminal record and close the checkpoint even if the owning
    /// tab is removed before `TurnTaskStopped` arrives (Ctrl+W on a non-last
    /// tab mid-turn). `None` on the Canonical path, where the source worker
    /// completes the recorder itself.
    recorder: Option<SharedTurnRecorder>,
    engine: Option<Arc<ZodeEngine>>,
}

#[derive(Clone)]
struct ScheduledTurnPersistence {
    session_id: String,
    title: String,
    persisted: Arc<std::sync::atomic::AtomicUsize>,
}

struct PendingScheduleLease {
    lease: zode_core::scheduler::ScheduleAttemptLease,
    origin: PendingScheduleOrigin,
    queued_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingScheduleOrigin {
    /// A canonical schedule slot claimed by `try_claim_watchdog_fire`.
    Fire,
    /// A persisted retry token claimed into the local queue.
    Retry(u64),
}

#[derive(Debug, Clone)]
enum ScheduleTerminalMutation {
    /// Preserve every watchdog field and clear only the active attempt token.
    ClearOnly,
    /// Commit the complete terminal recovery state owned by this attempt.
    WatchdogState {
        failures: u32,
        last_failure_ms: Option<u64>,
        retry_at_ms: Option<u64>,
        enabled: Option<bool>,
    },
}

struct PendingScheduleFinalizer {
    lease: zode_core::scheduler::ScheduleAttemptLease,
    mutation: ScheduleTerminalMutation,
    retry_at: std::time::Instant,
}

#[cfg(test)]
struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

#[cfg(test)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(self.key, previous),
            None => std::env::remove_var(self.key),
        }
    }
}

#[cfg(test)]
struct TestConfigIsolation {
    // Field order matters: restore the process environment and remove the
    // temporary config directory before releasing the process-wide lock.
    _env: EnvVarGuard,
    _config: tempfile::TempDir,
    _env_lock: tokio::sync::MutexGuard<'static, ()>,
}

pub struct TuiApp {
    /// One independent conversation per tab; `active` indexes the focused one.
    tabs: Vec<SessionTab>,
    active: usize,
    /// Monotonic tab-id source (never reused, so stale events from a closed
    /// tab can't land on a freshly-opened tab that took its Vec slot).
    next_tab_id: usize,
    /// Assembly context for spinning up a fresh engine on Ctrl+T / resume.
    template: EngineTemplate,
    /// Stable task-channel owner from the initially assembled engine. Engine
    /// rebuilds may replace `template.browser`; extension replies must stay on
    /// the socket that delivered the request.
    extension_browser: Arc<zode_core::browser::BrowserSession>,
    /// Single consumer for the authenticated extension task channel. `None`
    /// after closure; the select branch then parks instead of busy-looping.
    extension_task_rx: Option<zode_core::browser::bridge::TaskReceiver>,
    extension_tasks: extension_tasks::ExtensionTaskState,
    extension_attachments: extension_attachments::AttachmentRegistry,
    /// Startup strict-read preference, remembered so a `/sandbox off` → `on`
    /// toggle re-applies it (mode/network toggles carry it via with_mode/network,
    /// but re-enabling from off rebuilds a fresh config that would otherwise drop
    /// it).
    sandbox_restrict_reads: bool,
    /// User visibility preference for the right session sidebar.
    sidebar_visibility: SidebarVisibility,
    /// App-managed text selection. When enabled, zode captures mouse drag
    /// events so selecting can auto-scroll and copy to the system clipboard.
    selection_mode: bool,
    active_selection: Option<ChatSelection>,
    active_input_selection: Option<InputSelection>,
    input: InputBox,
    pending_cursor_seq: Option<FragmentedCursorSeqState>,
    status: StatusBar,
    ui_extensions: zode_core::ui_extensions::UiExtensionHost,
    ui_data_revision: u64,
    status_rows: u16,
    /// Cached `agent type → declared model` from the on-disk agent definitions,
    /// for the HUD's `[model]` label. Reloading walks every agent-def directory,
    /// so it is refreshed on a slow TTL and only while sub-agent rows show.
    agent_models: HashMap<String, String>,
    agent_models_at: Option<std::time::Instant>,
    theme_store: ThemeStore,
    theme: Theme,
    should_quit: bool,
    /// One-shot transition that rejects UI/extension request sources before
    /// the scheduler finalizer drain begins.
    shutdown_cleanup_started: bool,
    /// True after the first idle Esc on a non-empty draft: a second Esc then
    /// clears it. Any other key disarms it (so a stray Esc never wipes a draft).
    esc_clear_armed: bool,
    /// Index of the pending image chip currently selected for delete/view (↑ to
    /// select). `None` = no chip selected. Always points into the ACTIVE tab's
    /// `pending_images`; cleared on submit / tab switch / when it empties.
    selected_image: Option<usize>,
    /// Click hitboxes for the rendered image chips: `(col_start, col_end, index)`
    /// in absolute terminal columns, all on row `image_chip_row`. Rebuilt each
    /// frame so a (Cmd/Ctrl)+left-click can open the chip under the cursor.
    image_chip_hits: Vec<(u16, u16, usize)>,
    image_chip_row: u16,
    /// Clipboard preview temp files THIS process created (for the chip "view").
    /// Only paths in this set are ever deleted, so a real user-supplied image
    /// (even one that happens to live in the temp dir) is never removed.
    clipboard_temps: HashSet<std::path::PathBuf>,
    /// Approval requests from gated tools (one dialog shown at a time).
    approval_rx: ApprovalReceiver,
    active_dialog: Option<PermissionDialog>,
    pending_requests: VecDeque<ApprovalRequest>,
    /// AskUserQuestion channel + its modal (parallel to the approval path).
    question_rx: QuestionReceiver,
    /// Question-queue sender clone: lets `/op` raise consent prompts (for
    /// install/launch) through the same modal the agent's questions use.
    question_queue: zode_core::question::QuestionQueue,
    active_question: Option<QuestionDialog>,
    pending_questions: VecDeque<QuestionRequest>,
    autocomplete: Autocomplete,
    /// `@`-mention picker (cwd file / skill / MCP server). Built once when `@`
    /// first appears as the trailing token; re-filtered in place on keystrokes.
    active_mention: Option<MentionPicker>,
    completion_hint: Option<CompletionHint>,
    settings: Option<SettingsDialog>,
    connect: Option<ConnectDialog>,
    plugin_picker: Option<PluginPicker>,
    /// `/browser` status panel (bare `/browser`, no subcommand).
    browser_panel: Option<BrowserPanel>,
    /// `/team` status panel (bare `/team`, no subcommand).
    team_panel: Option<crate::ui::dialog::team_panel::TeamPanel>,
    agents_dialog: Option<AgentsDialog>,
    workflows_dialog: Option<WorkflowsDialog>,
    mcp_dialog: Option<McpDialog>,
    session_picker: Option<SessionPicker>,
    tasks_panel: Option<TasksPanel>,
    /// Snapshot of the active tab's background shells, refreshed while the
    /// tasks panel is open (the tracker's `list()` is async; the render path
    /// is not).
    bg_shells: Vec<BgShell>,
    subagents_panel: Option<crate::ui::dialog::subagents::SubAgentsPanel>,
    /// Cached snapshot of the active tab's sub-agent registry. Refreshed while
    /// the sub-agents panel is open; `snapshot()` is sync so no await needed.
    subagents: Vec<zode_core::SubAgent>,
    /// Fold state of the collapsible sidebar sections (session-scoped;
    /// toggled by a header click or `/sidebar mcp|files|todo`).
    mcp_section_collapsed: bool,
    lsp_section_collapsed: bool,
    files_section_collapsed: bool,
    todo_section_collapsed: bool,
    /// Full modified-files overlay, opened by clicking the sidebar section's
    /// "…+k more" row.
    files_panel: Option<crate::ui::dialog::files_panel::FilesPanel>,
    /// Header-row hitboxes of the collapsible sidebar sections, rebuilt each
    /// frame so a left-click can toggle the section under the cursor.
    sidebar_hits: crate::ui::tabs::SidebarHits,
    /// The sidebar's rendered area (None while hidden), for click hit-testing.
    sidebar_area: Option<Rect>,
    /// When the last sidebar data poll (git stat + MCP state) started.
    last_sidebar_poll: Option<std::time::Instant>,
    /// When the last status-HUD data poll (background shells) started.
    last_hud_poll: Option<std::time::Instant>,
    /// Whether any overlay (modal/panel/toast) was open on the previous frame.
    /// When one closes, the next frame forces a FULL terminal repaint: diff
    /// rendering never re-sends "unchanged" cells, so a terminal that dropped
    /// cells under the overlay (observed in Warp) would keep the gap forever.
    overlay_was_open: bool,
    /// One-shot full-repaint request (overlay close, Ctrl+L).
    force_redraw: bool,
    /// Set by an idle tick (nothing animatable changed) to skip the next
    /// redraw — keeps a fully idle app from rebuilding the widget tree at the
    /// tick rate. Consumed (reset) at the top of the loop.
    skip_next_draw: bool,
    show_help: bool,
    toast: Option<Toast>,
    provider_names: Vec<String>,
    /// See [`UiConfig::update_applied`]; polled on the tick.
    update_applied: Option<std::sync::Arc<std::sync::OnceLock<String>>>,
    /// The "self-updated — restart to apply" notice fires exactly once.
    update_notice_shown: bool,
    /// The area of the most recently painted frame. Mouse hit-testing and
    /// selection copy resolve against this — the layout the user actually
    /// saw — falling back to a live terminal query only before the first
    /// draw. `Rect::default()` until then.
    last_frame_area: Rect,
    /// Chat display prefs (`/thinking`, `/tool-details`), persisted in config
    /// and applied to the active tab's chat each frame.
    show_thinking: bool,
    show_tool_details: bool,
    /// Index of the queued follow-up currently mirrored in the prompt editor.
    queued_edit_index: Option<usize>,
    /// `/loop` (in-memory) + `/schedule` (persisted) job registry, polled once
    /// per tick (`poll_scheduler`). Pure/I-O-free itself; `schedules.json`
    /// load/save happens around it here and in the slash-command handlers.
    scheduler: Scheduler,
    /// Throttle authoritative roster reconciliation across concurrently
    /// running zode processes.
    last_schedule_roster_refresh: std::time::Instant,
    /// Queued-but-not-yet-submitted scheduler prompts, keyed by
    /// `(owning tab id, exact prompt text)` — see [`SchedPendingKey`]. Each
    /// value is FIFO because multiple distinct jobs may queue the same text on
    /// one tab. `submit()` pops only the oldest occurrence for the text it's
    /// about to run so `SessionTab::active_sched_job` can attribute the turn
    /// back to its job.
    /// Entries are purged when their job goes away (`/loop stop`,
    /// `/schedule rm|disable`) or their tab closes, so a stale entry can never
    /// consume a later user-typed message.
    sched_pending: HashMap<SchedPendingKey, SchedPendingJobs>,
    /// Monotonic enqueue time per unique scheduler job. It bounds the
    /// claim-to-start phase even when an interactive turn or queued user input
    /// keeps the owning tab busy indefinitely.
    sched_queued_at: HashMap<SchedJobRef, std::time::Instant>,
    /// Cross-process leases for persisted schedule occurrences that have been
    /// claimed but are still waiting in a tab queue. The lease moves into the
    /// tab at turn start. If exact-token cleanup fails, it remains here as a
    /// fail-closed fence even after the queue occurrence is removed.
    pending_schedule_leases: HashMap<String, PendingScheduleLease>,
    /// Terminal schedule mutations whose first durable write failed. The OS
    /// lease remains held and the scheduler id remains blocked until a retry
    /// commits, or a stale CAS is durably disabled for manual review.
    pending_schedule_finalizers: HashMap<(String, u64), PendingScheduleFinalizer>,
    /// Consecutive turn failures per scheduler job. Reset to zero (removed) on
    /// a success; at 3 the job is stopped/disabled — a persistently broken
    /// prompt must not retry forever.
    sched_fail_streak: HashMap<SchedJobRef, u32>,
    /// Liveness and bounded recovery for unattended scheduler-owned turns.
    /// Ordinary interactive turns are deliberately never registered here.
    watchdog: BackgroundWatchdog,
    /// Hard-abort requests awaiting proof that the Tokio owner actually
    /// stopped. The tab remains draining and its schedule lease remains held
    /// until `TurnTaskStopped` arrives, preventing old/new drivers from sharing
    /// one message store.
    forced_turn_stops: HashMap<(usize, u64), PendingForcedTurnStop>,
    #[cfg(test)]
    _test_config_isolation: Option<TestConfigIsolation>,
}

struct AgentTurnStreamContext {
    engine: Arc<ZodeEngine>,
    recorder: Option<SharedTurnRecorder>,
    abort: Option<AbortController>,
    tab_id: usize,
    turn_id: u64,
    tx: mpsc::UnboundedSender<AppEvent>,
    watchdog_pulse: Option<watchdog::WatchdogPulse>,
    scheduled_persistence: Option<ScheduledTurnPersistence>,
}

async fn forward_agent_turn_stream(
    context: AgentTurnStreamContext,
    stream_result: Result<Box<dyn agent::stream::EventStream>, String>,
) {
    let AgentTurnStreamContext {
        engine,
        recorder,
        abort,
        tab_id,
        turn_id,
        tx,
        watchdog_pulse,
        scheduled_persistence,
    } = context;

    let turn_ran = stream_result.is_ok();
    let scheduler_owned = scheduled_persistence.is_some();
    let mut stop_reason: Option<String> = None;
    let mut result = match stream_result {
        Ok(mut stream) => {
            if let Some(note) = engine.take_restore_note() {
                if let Some(recorder) = &recorder {
                    if let Ok(mut recorder) = recorder.lock() {
                        recorder.record(RunEvent::Notice {
                            code: "zode.compact.restore".into(),
                            message: note.clone(),
                        });
                    }
                }
                let _ = tx.send(AppEvent::Agent {
                    tab_id,
                    turn_id,
                    cost_label: None,
                    event: Event::Notice {
                        code: "zode.compact.restore".into(),
                        message: note,
                    },
                });
            }
            let mut result = Ok(());
            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => {
                        if let Some(pulse) = &watchdog_pulse {
                            pulse.activity(std::time::Instant::now());
                        }
                        engine.cost.observe(&event).await;
                        if let Some(recorder) = &recorder {
                            if let Ok(mut recorder) = recorder.lock() {
                                recorder.record_agent(&event);
                            }
                        }
                        if let Event::Result { data } = &event {
                            stop_reason = data.stop_reason.clone();
                        }
                        let cost_label =
                            if matches!(event, Event::Usage { .. } | Event::ToolResult { .. }) {
                                Some(engine.cost.sidebar_label().await)
                            } else {
                                None
                            };
                        if tx
                            .send(AppEvent::Agent {
                                tab_id,
                                turn_id,
                                cost_label,
                                event,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        if let Some(pulse) = &watchdog_pulse {
                            pulse.activity(std::time::Instant::now());
                        }
                        if let Some(recorder) = &recorder {
                            if let Ok(mut recorder) = recorder.lock() {
                                recorder.record(RunEvent::Error {
                                    code: "stream_error".into(),
                                    message: error.to_string(),
                                });
                            }
                        }
                        result = Err(error.to_string());
                        break;
                    }
                }
            }
            result
        }
        Err(error) => Err(error),
    };
    if let Some(pulse) = &watchdog_pulse {
        pulse.activity(std::time::Instant::now());
    }
    let mut terminal_quarantined = false;
    if scheduler_owned {
        if let Some(activity) = abort.as_ref().map(AbortController::activity) {
            if tokio::time::timeout(HARD_STOP_QUIESCE_TIMEOUT, activity.wait_for_quiescence())
                .await
                .is_err()
            {
                terminal_quarantined = true;
                result = Err(
                    "watchdog quarantine: tracked workers exceeded the quiescence deadline"
                        .to_string(),
                );
                let _ = tx.send(AppEvent::TurnTaskQuarantined {
                    tab_id,
                    turn_id,
                    result: Some(result.clone()),
                });
                // Do not journal, publish a terminal, or release the schedule
                // while a tracked worker can still mutate the shared store.
                // The UI disables the job and holds its attempt lease after
                // the quarantine event; this waiter is intentionally
                // unbounded so only real quiescence can lift that isolation.
                activity.wait_for_quiescence().await;
                if let Some(pulse) = &watchdog_pulse {
                    pulse.activity(std::time::Instant::now());
                }
            }
        }
    }
    if let Some(persistence) = scheduled_persistence {
        if !crate::tab::persist_session(
            persistence.session_id,
            engine.clone(),
            persistence.title,
            persistence.persisted,
            false,
        )
        .await
        {
            result = Err("durable session save failed".to_string());
            if !terminal_quarantined {
                terminal_quarantined = true;
                let _ = tx.send(AppEvent::TurnTaskQuarantined {
                    tab_id,
                    turn_id,
                    result: Some(result.clone()),
                });
            }
        }
        if let Some(pulse) = &watchdog_pulse {
            pulse.activity(std::time::Instant::now());
        }
    }
    if let Some(recorder) = &recorder {
        let Ok(mut recorder) = recorder.lock() else {
            if let Some(pulse) = &watchdog_pulse {
                pulse.terminal();
            }
            let sent = if terminal_quarantined {
                tx.send(AppEvent::TurnTaskStopped { tab_id, turn_id })
            } else {
                tx.send(AppEvent::TurnDone {
                    tab_id,
                    turn_id,
                    result,
                })
            };
            let _ = sent;
            return;
        };
        // A user cancel journals Interrupted (like `zode -p`), not a generic
        // failure. stop_reason stays the model's short token — never the raw
        // error text, which telemetry.rs exports verbatim.
        let watchdog_timeout = watchdog_pulse
            .as_ref()
            .and_then(watchdog::WatchdogPulse::timeout_kind);
        let interrupted =
            watchdog_timeout.is_none() && abort.as_ref().is_some_and(|abort| abort.is_aborted());
        let failed = result.is_err();
        if let Some(kind) = watchdog_timeout {
            recorder.record(RunEvent::Notice {
                code: "watchdog.timeout".into(),
                message: format!("{} timeout", kind.label()),
            });
        }
        let outcome = TurnOutcome {
            status: if terminal_quarantined || watchdog_timeout.is_some() {
                RunStatus::Failed
            } else {
                RunStatus::derive(interrupted, failed, stop_reason.as_deref())
            },
            stop_reason: terminal_quarantined
                .then(|| "watchdog_quarantined".to_string())
                .or_else(|| {
                    watchdog_timeout
                        .map(|kind| format!("watchdog_{}_timeout", kind.label().replace(' ', "_")))
                })
                .or(stop_reason),
            partial: terminal_quarantined || failed || watchdog_timeout.is_some(),
        };
        recorder.complete(Some(&engine.checkpoints), turn_ran, &outcome);
    }
    if let Some(pulse) = &watchdog_pulse {
        // Recorder finalization and the nested-worker gate are both complete.
        // Mark terminal before enqueueing so a concurrent UI tick cannot turn
        // the tiny send/mark window into a false abort-grace expiration.
        pulse.terminal();
    }
    let sent = if terminal_quarantined {
        tx.send(AppEvent::TurnTaskStopped { tab_id, turn_id })
    } else {
        tx.send(AppEvent::TurnDone {
            tab_id,
            turn_id,
            result,
        })
    };
    let _ = sent;
}

impl TuiApp {
    pub fn new(
        engine: ZodeEngine,
        template: EngineTemplate,
        ui: UiConfig,
        approval_rx: ApprovalReceiver,
        question_rx: QuestionReceiver,
        question_queue: zode_core::question::QuestionQueue,
        resumed_id: Option<String>,
    ) -> Self {
        if !cfg!(test) {
            if let Err(error) = zode_core::browser::bridge::native_host::install(&engine.cwd) {
                tracing::debug!(%error, "browser native host registration failed");
            }
        }
        let mut theme_store = ThemeStore::with_builtins();
        if let Ok(dir) = ConfigManager::config_dir() {
            theme_store.merge_user(crate::theme::loader::load_dir(&dir.join("themes")));
        }
        let theme = theme_store.resolve(ui.theme_id.as_deref());
        let mut status = StatusBar::new(engine.model.clone());
        status.yolo = ui.yolo;
        status.sandbox = ui.sandbox;

        // Tab 0 wraps the already-assembled engine. A resumed session keeps
        // its id (and is pre-titled); a fresh one gets a new id.
        let session_id = resumed_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let mut tab0 = SessionTab::new(0, Arc::new(engine), session_id);
        // The engine may have been assembled from a task-local resume template
        // even when loading its transcript later failed (`resumed_id=None`).
        // Record the actual gate used to assemble it; never infer access from
        // persistence outcome or the clean global defaults.
        tab0.extension_access = ui.initial_access;
        tab0.titled = resumed_id.is_some();
        let extension_browser = tab0.engine.browser.clone();
        let extension_task_rx = extension_browser.take_extension_task_receiver();
        start_browser_bridge_listener(extension_browser.clone());
        // A resumed session (--continue/--resume): replay its transcript into
        // the chat and restore its title (the engine already holds the store).
        if let Some(id) = &resumed_id {
            if let Ok(store) = tab0.engine.store.lock() {
                tab0.chat = rebuild_chat_from_store(&store);
                tab0.context_tokens = estimate_store_tokens(&store);
                // Seed the append watermark to the loaded length (see the
                // ResumeTab reassemble effect for the rationale).
                tab0.persisted_msgs
                    .store(store.len(), std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(meta) = SessionIndex::load()
                .ok()
                .and_then(|i| i.find_prefix(id).cloned())
            {
                tab0.title = meta.title;
            }
        }

        // First-run / unconfigured: the user reached the UI but no provider key
        // is set yet. Point them at `/connect` (and the config file) right in
        // the transcript instead of letting the first message fail silently.
        if ui.needs_setup {
            let path = ConfigManager::config_dir()
                .map(|d| d.join("config.json").display().to_string())
                .unwrap_or_else(|_| "~/.zode/config.json".to_string());
            tab0.chat.push_system(
                &crate::tr(
                    "Welcome to zode. No provider is configured yet — run /connect to set one up, \
                     or add your provider's apiKey to {path}. (Messages won't send until a provider \
                     with an API key is configured.)",
                )
                .replace("{path}", &path),
            );
        }

        // Seed input-line history from this session's own bucket plus the
        // conversation's user prompts, so Up/Down never crosses sessions.
        seed_prompt_history_for_tab(&mut tab0);

        // Read display prefs before `template` is moved into the struct.
        let show_thinking = template.show_thinking();
        let show_tool_details = template.show_tool_details();
        // Mouse capture drives BOTH terminal setup and app-managed selection:
        // with capture off (`"mouseCapture": false`) the terminal owns
        // selection — ⌘C copies natively — and no mouse events reach the app.
        let mouse_capture = template.mouse_capture();
        let mut scheduler = Scheduler::default();
        scheduler.set_schedules(zode_core::scheduler::load_schedules());
        let watchdog_config = template.background_watchdog().clone();
        let mut watchdog = BackgroundWatchdog::new(watchdog_config.clone());
        let restore_instant = std::time::Instant::now();
        let restore_epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let schedules_at_startup = scheduler.schedules().to_vec();
        for schedule in &schedules_at_startup {
            if let Some(active_token_ms) = schedule.watchdog_active_since_ms {
                match zode_core::scheduler::recover_orphaned_watchdog_attempt(
                    &schedule.id,
                    active_token_ms,
                ) {
                    Ok(zode_core::scheduler::OrphanAttemptRecovery::Live) => {
                        tab0.chat.push_system(&format!(
                            "watchdog: schedule {} is currently owned by another live zode process",
                            schedule.id
                        ));
                    }
                    Ok(zode_core::scheduler::OrphanAttemptRecovery::Stale) => {
                        scheduler.set_schedules(zode_core::scheduler::load_schedules());
                    }
                    Ok(zode_core::scheduler::OrphanAttemptRecovery::Recovered(roster)) => {
                        scheduler.set_schedules(roster);
                        tab0.chat.push_system(&format!(
                            "watchdog: schedule {} was active when its owner exited; disabled for manual review",
                            schedule.id
                        ));
                    }
                    Err(error) => {
                        tracing::warn!(%error, schedule_id = %schedule.id, "failed to inspect active watchdog lease");
                    }
                }
                continue;
            }
            if !schedule.enabled {
                continue;
            }
            if !watchdog_config.enabled()
                && (schedule.watchdog_failures > 0 || schedule.watchdog_retry_at_ms.is_some())
            {
                match zode_core::scheduler::persist_idle_watchdog_state_if_matches(
                    schedule, 0, None, None, None,
                ) {
                    Ok(true) => {
                        scheduler.clear_watchdog_failures(&schedule.id);
                    }
                    Ok(false) => {
                        scheduler.set_schedules(zode_core::scheduler::load_schedules());
                    }
                    Err(error) => {
                        tracing::warn!(%error, schedule_id = %schedule.id, "failed to clear disabled watchdog state");
                    }
                }
                continue;
            }
            if schedule.watchdog_failures == 0 {
                continue;
            }
            if schedule.watchdog_failures > watchdog_config.max_retries() {
                match zode_core::scheduler::persist_idle_watchdog_state_if_matches(
                    schedule,
                    schedule.watchdog_failures,
                    schedule.watchdog_last_failure_ms,
                    None,
                    Some(false),
                ) {
                    Ok(true) => {
                        scheduler.disable_schedule(&schedule.id);
                    }
                    Ok(false) => {
                        scheduler.set_schedules(zode_core::scheduler::load_schedules());
                    }
                    Err(error) => {
                        tracing::warn!(%error, schedule_id = %schedule.id, "failed to persist exhausted watchdog state");
                    }
                }
                continue;
            }
            if let Some(retry_at_ms) = schedule.watchdog_retry_at_ms {
                let delay =
                    std::time::Duration::from_millis(retry_at_ms.saturating_sub(restore_epoch_ms));
                let Some(due_at) = restore_instant.checked_add(delay) else {
                    tracing::warn!(schedule_id = %schedule.id, "watchdog retry deadline is out of range");
                    continue;
                };
                watchdog.restore_retry(
                    SchedJobRef::Schedule(schedule.id.clone()),
                    tab0.id,
                    schedule.prompt.clone(),
                    schedule.watchdog_failures,
                    due_at,
                    retry_at_ms,
                );
            }
        }
        // Apply the configured UI language so the chrome renders localized.
        if let Some(lang) = template.language() {
            zode_core::i18n::set_language_code(lang);
        }

        Self {
            tabs: vec![tab0],
            active: 0,
            next_tab_id: 1,
            // Capture the startup strict-read bit before `template` is moved.
            // Falls back to the config section when the sandbox starts
            // disabled (e.g. --no-sandbox), so a later `/sandbox on` still
            // honors a configured `restrictReads`.
            sandbox_restrict_reads: template
                .sandbox()
                .map(|c| c.restrict_reads())
                .unwrap_or(template.sandbox_settings().restrict_reads.unwrap_or(false)),
            template,
            extension_browser,
            extension_task_rx,
            extension_tasks: extension_tasks::ExtensionTaskState::default(),
            extension_attachments: extension_attachments::AttachmentRegistry::new(),
            sidebar_visibility: SidebarVisibility::Auto,
            selection_mode: mouse_capture,
            active_selection: None,
            active_input_selection: None,
            input: InputBox::new(),
            pending_cursor_seq: None,
            status,
            ui_extensions: if cfg!(test) {
                zode_core::ui_extensions::UiExtensionHost::default()
            } else {
                zode_core::ui_extensions::UiExtensionHost::load()
            },
            ui_data_revision: 0,
            status_rows: 1,
            agent_models: HashMap::new(),
            agent_models_at: None,
            theme_store,
            theme,
            should_quit: false,
            shutdown_cleanup_started: false,
            esc_clear_armed: false,
            selected_image: None,
            image_chip_hits: Vec::new(),
            image_chip_row: 0,
            clipboard_temps: HashSet::new(),
            approval_rx,
            active_dialog: None,
            pending_requests: VecDeque::new(),
            question_rx,
            question_queue,
            active_question: None,
            pending_questions: VecDeque::new(),
            autocomplete: Autocomplete::new(),
            active_mention: None,
            completion_hint: None,
            settings: None,
            connect: None,
            plugin_picker: None,
            browser_panel: None,
            team_panel: None,
            agents_dialog: None,
            workflows_dialog: None,
            mcp_dialog: None,
            session_picker: None,
            tasks_panel: None,
            bg_shells: Vec::new(),
            subagents_panel: None,
            subagents: Vec::new(),
            mcp_section_collapsed: false,
            lsp_section_collapsed: false,
            files_section_collapsed: false,
            todo_section_collapsed: false,
            files_panel: None,
            sidebar_hits: crate::ui::tabs::SidebarHits::default(),
            sidebar_area: None,
            last_sidebar_poll: None,
            last_hud_poll: None,
            overlay_was_open: false,
            force_redraw: false,
            skip_next_draw: false,
            show_help: false,
            toast: None,
            provider_names: ui.provider_names,
            update_applied: ui.update_applied,
            update_notice_shown: false,
            last_frame_area: Rect::default(),
            show_thinking,
            show_tool_details,
            queued_edit_index: None,
            scheduler,
            last_schedule_roster_refresh: std::time::Instant::now(),
            sched_pending: HashMap::new(),
            sched_queued_at: HashMap::new(),
            pending_schedule_leases: HashMap::new(),
            pending_schedule_finalizers: HashMap::new(),
            sched_fail_streak: HashMap::new(),
            watchdog,
            forced_turn_stops: HashMap::new(),
            #[cfg(test)]
            _test_config_isolation: None,
        }
    }

    /// Apply a learned lite verdict to the ACTIVE tab once it is idle: the
    /// loop-guard evidence arrived mid-turn (when reassembly is impossible),
    /// so the profile switch happens here. Attempted once per tab — when
    /// explicit `profile: "standard"` config keeps the assembly standard,
    /// this must not spin.
    fn maybe_apply_learned_profile(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let tab = self.active_tab();
        let model = tab.engine.model.clone();
        if tab.is_busy()
            || tab.lite_reassemble_attempted
            || tab.engine.lite_profile
            || !self.template.would_be_lite(&model)
        {
            return;
        }
        self.active_tab_mut().lite_reassemble_attempted = true;
        self.start_reassemble_active(
            self.template.clone(),
            ReassembleEffect::Notify(ReassembleNotify::System(
                crate::tr("lite accommodations applied for this model").to_string(),
            )),
            agent_tx,
        );
    }

    /// Show the one-time "self-updated — restart to apply" notice once the
    /// background updater reports a swapped-in build. Runs on the tick (both
    /// the TUI loop and the extension daemon poll it; only the TUI shows it).
    fn maybe_notice_self_update(&mut self) {
        if self.update_notice_shown {
            return;
        }
        let Some(tag) = self
            .update_applied
            .as_ref()
            .and_then(|cell| cell.get())
            .cloned()
        else {
            return;
        };
        self.update_notice_shown = true;
        let msg = crate::tr("self-updated to {tag} — restart to apply").replace("{tag}", &tag);
        self.active_tab_mut().chat.push_system(&msg);
        self.toast = Some(Toast::info(msg));
    }

    /// Record a submitted prompt for Up/Down recall (skips blanks and exact
    /// consecutive duplicates), and reset the browse cursor.
    fn record_prompt_history(&mut self, text: &str) {
        let tab = &mut self.tabs[self.active];
        if record_prompt_history_entry(&mut tab.prompt_history, text) {
            save_prompt_history(&tab.prompt_history_key, &tab.prompt_history);
        }
        tab.history_pos = None;
        tab.history_draft.clear();
    }

    /// Recall the previous (older) prompt into the input box. On first step it
    /// stashes the current draft so Down can restore it.
    fn history_prev(&mut self) {
        let tab = &mut self.tabs[self.active];
        if tab.prompt_history.is_empty() {
            return;
        }
        let next = match tab.history_pos {
            None => {
                tab.history_draft = self.input.text();
                tab.prompt_history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        tab.history_pos = Some(next);
        let entry = tab.prompt_history[next].clone();
        self.input.set_text(&entry);
    }

    /// Step to a newer prompt; past the newest, restore the stashed draft.
    fn history_next(&mut self) {
        let tab = &mut self.tabs[self.active];
        let Some(p) = tab.history_pos else {
            return;
        };
        if p + 1 < tab.prompt_history.len() {
            tab.history_pos = Some(p + 1);
            let entry = tab.prompt_history[p + 1].clone();
            self.input.set_text(&entry);
        } else {
            tab.history_pos = None;
            let draft = std::mem::take(&mut tab.history_draft);
            self.input.set_text(&draft);
        }
    }

    /// Close the transient popups that float over the input row: the slash
    /// autocomplete and the `@`-mention picker. Call on every active-tab change
    /// — the `@`-mention candidates are built from the active tab's engine
    /// (files/skills/MCP), so a picker left open across a switch would otherwise
    /// insert references from the previous session.
    fn dismiss_input_popups(&mut self) {
        self.autocomplete.dismiss();
        self.active_mention = None;
    }

    fn reset_input_browse_state(&mut self) {
        self.completion_hint = None;
        self.dismiss_input_popups();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.history_pos = None;
            tab.history_draft.clear();
        }
        self.active_input_selection = None;
    }

    fn sched_job_is_pending(&self, job: &SchedJobRef) -> bool {
        self.sched_pending
            .values()
            .any(|jobs| jobs.iter().any(|candidate| candidate == job))
    }

    fn reload_schedule_roster(&mut self) -> bool {
        match zode_core::scheduler::try_load_schedules() {
            Ok(schedules) => {
                self.scheduler.set_schedules(schedules);
                true
            }
            Err(error) => {
                tracing::warn!(%error, "failed to refresh persisted schedule roster");
                false
            }
        }
    }

    fn sched_job_has_pending_lease(&self, job: &SchedJobRef) -> bool {
        match job {
            SchedJobRef::Loop(_) => false,
            SchedJobRef::Schedule(id) => {
                self.pending_schedule_leases.contains_key(id)
                    || self
                        .pending_schedule_finalizers
                        .keys()
                        .any(|(pending_id, _)| pending_id == id)
            }
        }
    }

    /// Linearize a queued->running transition against remote disable/remove.
    /// Read failures leave the prompt and lease queued; an authoritative
    /// disabled, missing, or token-mismatched row cancels only this occurrence.
    fn queued_schedule_claim_is_runnable(
        &mut self,
        job: &SchedJobRef,
    ) -> Result<bool, zode_core::CoreError> {
        let SchedJobRef::Schedule(id) = job else {
            return Ok(true);
        };
        let Some(pending) = self.pending_schedule_leases.get(id) else {
            return Ok(true);
        };
        let active_token_ms = pending.lease.active_token_ms();
        let roster = zode_core::scheduler::try_load_schedules()?;
        let runnable = roster.iter().any(|schedule| {
            schedule.id == *id
                && schedule.enabled
                && schedule.watchdog_active_since_ms == Some(active_token_ms)
        });
        self.scheduler.set_schedules(roster);
        Ok(runnable)
    }

    fn push_sched_pending(&mut self, key: SchedPendingKey, job: SchedJobRef) {
        self.sched_queued_at
            .entry(job.clone())
            .or_insert_with(std::time::Instant::now);
        self.sched_pending.entry(key).or_default().push_back(job);
    }

    fn pop_sched_pending(&mut self, key: &SchedPendingKey) -> Option<SchedJobRef> {
        let (job, now_empty) = {
            let jobs = self.sched_pending.get_mut(key)?;
            let job = jobs.pop_front();
            (job, jobs.is_empty())
        };
        if now_empty {
            self.sched_pending.remove(key);
        }
        if let Some(job) = job.as_ref() {
            if !self.sched_job_is_pending(job) {
                self.sched_queued_at.remove(job);
            }
        }
        job
    }

    fn pop_sched_pending_if_front(
        &mut self,
        key: &SchedPendingKey,
        expected: &SchedJobRef,
    ) -> Option<SchedJobRef> {
        let matches = self.sched_pending.get(key).and_then(|jobs| jobs.front()) == Some(expected);
        matches.then(|| self.pop_sched_pending(key)).flatten()
    }

    /// Remove one exact scheduler-owned queue occurrence without releasing its
    /// persisted lease. Queue-timeout recovery needs to move that lease
    /// directly into terminal persistence, not briefly expose an idle token.
    fn take_sched_pending_occurrence(&mut self, expected: &SchedJobRef) -> Option<(usize, String)> {
        let (key, occurrence) = self.sched_pending.iter().find_map(|(key, jobs)| {
            jobs.iter()
                .position(|job| job == expected)
                .map(|occurrence| (key.clone(), occurrence))
        })?;
        let (tab_id, prompt) = key.clone();

        let active_tab_id = self.tabs.get(self.active).map(|tab| tab.id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            // Resolve the absolute queue index of the `occurrence`-th entry
            // whose text matches this prompt, so we can both remove it and keep
            // any open queued-edit cursor aligned with the shift.
            let mut seen = 0usize;
            let mut removed_index = None;
            for (index, queued) in tab.queued_input.iter().enumerate() {
                if queued == &prompt {
                    if seen == occurrence {
                        removed_index = Some(index);
                        break;
                    }
                    seen = seen.saturating_add(1);
                }
            }
            if let Some(index) = removed_index {
                tab.queued_input.remove(index);
                // The edit cursor only tracks the active tab's queue. If this
                // expiry removed the entry being edited, cancel the edit; if it
                // removed one before it, shift the cursor down so a save can't
                // overwrite the wrong message.
                if active_tab_id == Some(tab_id) {
                    match self.queued_edit_index {
                        Some(edit) if edit == index => self.queued_edit_index = None,
                        Some(edit) if edit > index => self.queued_edit_index = Some(edit - 1),
                        _ => {}
                    }
                }
            }
        }
        let now_empty = if let Some(jobs) = self.sched_pending.get_mut(&key) {
            jobs.remove(occurrence);
            jobs.is_empty()
        } else {
            false
        };
        if now_empty {
            self.sched_pending.remove(&key);
        }
        self.sched_queued_at.remove(expected);
        Some((tab_id, prompt))
    }

    /// Release a queued schedule's exact persisted active token before
    /// dropping its OS lease. An I/O failure retains the lease in memory, so
    /// no other process can mistake the still-active token for abandoned work.
    fn release_pending_schedule_lease(&mut self, job: &SchedJobRef) -> bool {
        let SchedJobRef::Schedule(id) = job else {
            return true;
        };
        let Some(pending) = self.pending_schedule_leases.remove(id) else {
            return true;
        };
        self.finalize_schedule_attempt(pending.lease, ScheduleTerminalMutation::ClearOnly)
    }

    /// Cancel an unstarted occurrence without treating it as a successful
    /// retry. The combined retry claim already cleared its retry token, while
    /// exact lease release clears only active ownership, so consecutive
    /// failure history remains untouched.
    fn cancel_pending_sched_job(&mut self, job: &SchedJobRef) {
        self.release_pending_schedule_lease(job);
        self.watchdog.cancel_job(job);
    }

    fn cancel_persisted_retry_if_present(&mut self, job: &SchedJobRef) {
        let SchedJobRef::Schedule(id) = job else {
            return;
        };
        let roster = match zode_core::scheduler::try_load_schedules() {
            Ok(roster) => roster,
            Err(error) => {
                tracing::warn!(%error, schedule_id = %id, "failed to inspect persisted retry during cancellation");
                return;
            }
        };
        let retry_at_ms = roster
            .iter()
            .find(|schedule| &schedule.id == id)
            .and_then(|schedule| schedule.watchdog_retry_at_ms);
        let Some(retry_at_ms) = retry_at_ms else {
            self.scheduler.set_schedules(roster);
            return;
        };
        if let Err(error) = zode_core::scheduler::clear_watchdog_retry_if(id, retry_at_ms) {
            tracing::warn!(%error, schedule_id = %id, "failed to cancel persisted watchdog retry");
        }
        self.reload_schedule_roster();
    }

    fn cancel_sched_pending_if_front(
        &mut self,
        key: &SchedPendingKey,
        expected: &SchedJobRef,
    ) -> Option<SchedJobRef> {
        let job = self.pop_sched_pending_if_front(key, expected)?;
        self.cancel_pending_sched_job(&job);
        Some(job)
    }

    /// Graceful application exit may discard queued, never-started work. Clear
    /// those tokens; active turns deliberately keep theirs so an interrupted
    /// process is recovered fail-closed on the next startup.
    fn release_all_pending_schedule_leases(&mut self) {
        let ids: Vec<String> = self.pending_schedule_leases.keys().cloned().collect();
        for id in ids {
            let Some(pending) = self.pending_schedule_leases.remove(&id) else {
                continue;
            };
            let result = match pending.origin {
                PendingScheduleOrigin::Retry(retry_token_ms) => {
                    zode_core::scheduler::restore_claimed_watchdog_retry_for_shutdown(
                        &id,
                        pending.lease.active_token_ms(),
                        retry_token_ms,
                    )
                }
                PendingScheduleOrigin::Fire => {
                    zode_core::scheduler::restore_claimed_watchdog_fire_for_shutdown(&pending.lease)
                }
            };
            match result {
                Ok(true) => drop(pending),
                Ok(false) => {
                    match zode_core::scheduler::quarantine_claimed_watchdog_queue_restore_conflict(
                        &id,
                        pending.lease.active_token_ms(),
                    ) {
                        Ok(conflict) => {
                            tracing::error!(schedule_id = %id, ?conflict, "queued schedule restore conflicted; quarantined for manual review");
                            drop(pending);
                        }
                        Err(error) => {
                            tracing::error!(%error, schedule_id = %id, "failed to quarantine queued restore conflict; retaining lease");
                            self.pending_schedule_leases.insert(id, pending);
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(%error, schedule_id = %id, "failed to release queued schedule during shutdown; retaining lease");
                    self.pending_schedule_leases.insert(id, pending);
                }
            }
        }
        self.reload_schedule_roster();
    }

    /// Convert every scheduler-owned turn into an owned shutdown finalizer.
    /// The UI keeps polling agent events until those finalizers have proved
    /// worker quiescence and completed durable persistence; only then may the
    /// app drop the tabs and their attempt leases.
    fn begin_scheduler_shutdown(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let targets: Vec<(usize, usize, u64, SchedJobRef, bool)> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_idx, tab)| {
                let job = tab.active_sched_job.clone()?;
                if tab.active_turn_id > 0 {
                    Some((tab_idx, tab.id, tab.active_turn_id, job, true))
                } else {
                    tab.draining_turn_id
                        .map(|turn_id| (tab_idx, tab.id, turn_id, job, false))
                }
            })
            .collect();

        for (tab_idx, tab_id, turn_id, job, active) in targets {
            if self.forced_turn_stops.contains_key(&(tab_id, turn_id)) {
                continue;
            }
            if active {
                let Some(interrupt) = self.prepare_tab_interrupt(tab_idx, Some(turn_id)) else {
                    continue;
                };
                self.watchdog
                    .cancel_turn(tab_id, turn_id, std::time::Instant::now());
                self.resolve_extension_approvals_before_tui_interrupt(tab_id, turn_id);
                self.mark_extension_turn_interrupt_requested(tab_id, turn_id, None);
                interrupt.abort.abort_with_reason("application exiting");
            }
            self.begin_forced_turn_stop(
                tab_id,
                turn_id,
                ForcedTurnStop::Manual { job, failure: None },
                agent_tx,
            );
        }
    }

    fn scheduler_shutdown_pending(&self) -> bool {
        !self.pending_schedule_leases.is_empty()
            || !self.pending_schedule_finalizers.is_empty()
            || !self.forced_turn_stops.is_empty()
            || self.tabs.iter().any(|tab| {
                tab.active_sched_job.is_some()
                    && (tab.active_turn_id > 0 || tab.draining_turn_id.is_some())
            })
    }

    fn release_unsupervised_terminal_lease(
        &mut self,
        lease: Option<zode_core::scheduler::ScheduleAttemptLease>,
    ) {
        let Some(lease) = lease else {
            return;
        };
        self.finalize_schedule_attempt(lease, ScheduleTerminalMutation::ClearOnly);
    }

    /// Commit a terminal mutation while retaining the live OS lease across
    /// transient store failures. A stale CAS is not treated as success: the
    /// authoritative row is disabled under the roster lock before the lease is
    /// released, preventing an unexpectedly superseded attempt from replaying.
    fn try_apply_schedule_finalizer(
        &mut self,
        mut pending: PendingScheduleFinalizer,
    ) -> Option<PendingScheduleFinalizer> {
        let id = pending.lease.schedule_id().to_string();
        let active_token_ms = pending.lease.active_token_ms();
        let result = match &pending.mutation {
            ScheduleTerminalMutation::ClearOnly => {
                zode_core::scheduler::clear_watchdog_attempt_if(&id, active_token_ms)
            }
            ScheduleTerminalMutation::WatchdogState {
                failures,
                last_failure_ms,
                retry_at_ms,
                enabled,
            } => zode_core::scheduler::persist_watchdog_state_for_attempt(
                &id,
                active_token_ms,
                *failures,
                *last_failure_ms,
                *retry_at_ms,
                None,
                *enabled,
            ),
        };

        match result {
            Ok(true) => {
                drop(pending);
                self.reload_schedule_roster();
                None
            }
            Ok(false) => match zode_core::scheduler::quarantine_watchdog_terminal_conflict(
                &id,
                active_token_ms,
            ) {
                Ok(zode_core::scheduler::WatchdogTerminalConflict::StillOwned) => {
                    pending.retry_at = std::time::Instant::now()
                        .checked_add(SCHEDULE_FINALIZER_RETRY_INTERVAL)
                        .unwrap_or_else(std::time::Instant::now);
                    Some(pending)
                }
                Ok(zode_core::scheduler::WatchdogTerminalConflict::Missing) => {
                    tracing::warn!(schedule_id = %id, active_token_ms, "schedule disappeared before terminal persistence");
                    drop(pending);
                    self.reload_schedule_roster();
                    None
                }
                Ok(zode_core::scheduler::WatchdogTerminalConflict::SafeToRelease {
                    persisted_active_token_ms,
                    newly_disabled,
                }) => {
                    tracing::error!(
                        schedule_id = %id,
                        active_token_ms,
                        ?persisted_active_token_ms,
                        newly_disabled,
                        "schedule terminal CAS conflicted; authoritative row is disabled for manual review"
                    );
                    drop(pending);
                    self.reload_schedule_roster();
                    None
                }
                Err(error) => {
                    tracing::error!(%error, schedule_id = %id, "failed to quarantine stale schedule terminal; retaining lease");
                    pending.retry_at = std::time::Instant::now()
                        .checked_add(SCHEDULE_FINALIZER_RETRY_INTERVAL)
                        .unwrap_or_else(std::time::Instant::now);
                    Some(pending)
                }
            },
            Err(error) => {
                tracing::error!(%error, schedule_id = %id, "failed to persist schedule terminal; retaining lease for retry");
                pending.retry_at = std::time::Instant::now()
                    .checked_add(SCHEDULE_FINALIZER_RETRY_INTERVAL)
                    .unwrap_or_else(std::time::Instant::now);
                Some(pending)
            }
        }
    }

    fn finalize_schedule_attempt(
        &mut self,
        lease: zode_core::scheduler::ScheduleAttemptLease,
        mutation: ScheduleTerminalMutation,
    ) -> bool {
        let key = (lease.schedule_id().to_string(), lease.active_token_ms());
        let pending = PendingScheduleFinalizer {
            lease,
            mutation,
            retry_at: std::time::Instant::now(),
        };
        if let Some(pending) = self.try_apply_schedule_finalizer(pending) {
            let replaced = self.pending_schedule_finalizers.insert(key, pending);
            debug_assert!(
                replaced.is_none(),
                "attempt token uniquely owns its finalizer"
            );
            false
        } else {
            true
        }
    }

    fn retry_pending_schedule_finalizers(&mut self, now: std::time::Instant) {
        let due: Vec<(String, u64)> = self
            .pending_schedule_finalizers
            .iter()
            .filter(|(_, pending)| pending.retry_at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            let Some(pending) = self.pending_schedule_finalizers.remove(&key) else {
                continue;
            };
            if let Some(pending) = self.try_apply_schedule_finalizer(pending) {
                self.pending_schedule_finalizers.insert(key, pending);
            }
        }
    }

    /// A failed claim can mean a live owner, a just-completed winner, or a
    /// crashed owner whose persisted token is now orphaned. Reconcile the
    /// authoritative roster and disable only a provably orphaned attempt.
    fn reconcile_failed_schedule_claim(&mut self, id: &str, tab_idx: usize) {
        let roster = match zode_core::scheduler::try_load_schedules() {
            Ok(roster) => roster,
            Err(error) => {
                tracing::warn!(%error, schedule_id = %id, "failed to read schedule owner state");
                return;
            }
        };
        let active_token = roster
            .iter()
            .find(|schedule| schedule.id == id)
            .and_then(|schedule| schedule.watchdog_active_since_ms);
        self.scheduler.set_schedules(roster);
        let Some(active_token) = active_token else {
            return;
        };
        match zode_core::scheduler::recover_orphaned_watchdog_attempt(id, active_token) {
            Ok(zode_core::scheduler::OrphanAttemptRecovery::Recovered(roster)) => {
                self.scheduler.set_schedules(roster);
                self.watchdog
                    .cancel_job(&SchedJobRef::Schedule(id.to_string()));
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    tab.chat.push_system(&format!(
                        "watchdog: schedule {id} lost its owner; disabled for manual review"
                    ));
                }
            }
            Ok(zode_core::scheduler::OrphanAttemptRecovery::Stale) => {
                self.reload_schedule_roster();
            }
            Ok(zode_core::scheduler::OrphanAttemptRecovery::Live) => {}
            Err(error) => {
                tracing::warn!(%error, schedule_id = %id, "failed to reconcile schedule attempt owner")
            }
        }
    }

    /// Detach the scheduler attribution, if any, for one concrete queued
    /// message before the user edits or deletes it. Equal-text occurrences are
    /// paired FIFO with the per-key attribution queue, so removing occurrence
    /// N cannot accidentally cancel a different same-text job.
    fn detach_sched_occurrence_at(
        &mut self,
        tab_idx: usize,
        queue_index: usize,
    ) -> Option<SchedJobRef> {
        let tab = self.tabs.get(tab_idx)?;
        let prompt = tab.queued_input.get(queue_index)?.clone();
        let occurrence = tab
            .queued_input
            .iter()
            .take(queue_index)
            .filter(|candidate| *candidate == &prompt)
            .count();
        let key = (tab.id, prompt);
        let (job, now_empty) = {
            let jobs = self.sched_pending.get_mut(&key)?;
            if occurrence >= jobs.len() {
                return None;
            }
            let job = jobs.remove(occurrence);
            (job, jobs.is_empty())
        };
        if now_empty {
            self.sched_pending.remove(&key);
        }
        if let Some(job) = job.as_ref() {
            if !self.sched_job_is_pending(job) {
                self.sched_queued_at.remove(job);
            }
        }
        job
    }

    fn save_queued_edit_text(&mut self, text: String) -> Option<usize> {
        let index = self.queued_edit_index?;
        self.queued_edit_index = None;
        if index >= self.tabs[self.active].queued_input.len() {
            return None;
        }
        // Taking control of a scheduler-injected occurrence turns it into an
        // ordinary queued user message. Detach before mutating the text so the
        // exact occurrence (not merely every equal string) is cancelled.
        if let Some(job) = self.detach_sched_occurrence_at(self.active, index) {
            self.cancel_pending_sched_job(&job);
        }
        if !text.trim().is_empty() {
            let edited_key = (self.tabs[self.active].id, text.clone());
            // The edited occurrence is user-owned. If its new text collides
            // with other scheduler occurrences, their text-only queue entries
            // can no longer be distinguished safely by position. Drop those
            // attributions too (leave the messages queued as ordinary user
            // input) rather than let the edited message capture a job.
            if let Some(jobs) = self.sched_pending.remove(&edited_key) {
                for job in jobs {
                    self.sched_queued_at.remove(&job);
                    self.cancel_pending_sched_job(&job);
                }
            }
        }
        let queue = &mut self.tabs[self.active].queued_input;
        if text.trim().is_empty() {
            queue.remove(index);
            Some(index.min(queue.len()))
        } else {
            queue[index] = text;
            self.queued_edit_index = Some(index);
            Some(index)
        }
    }

    fn save_current_queued_edit(&mut self) -> Option<usize> {
        let text = self.input.text();
        self.save_queued_edit_text(text)
    }

    fn select_queued_edit(&mut self, index: usize) -> bool {
        let Some(text) = self.active_tab().queued_input.get(index).cloned() else {
            self.queued_edit_index = None;
            return false;
        };
        self.queued_edit_index = Some(index);
        self.input.set_text(&text);
        self.reset_input_browse_state();
        true
    }

    fn edit_previous_queued_input(&mut self) -> bool {
        if !self.active_tab().is_busy() {
            return false;
        }
        let current = if self.queued_edit_index.is_some() {
            self.save_current_queued_edit()
                .unwrap_or_else(|| self.active_tab().queued_input.len())
        } else {
            self.active_tab().queued_input.len()
        };
        let len = self.active_tab().queued_input.len();
        if len == 0 {
            return false;
        }
        let target = current.saturating_sub(1).min(len - 1);
        self.select_queued_edit(target)
    }

    fn edit_next_queued_input(&mut self) -> bool {
        let Some(_) = self.queued_edit_index else {
            return false;
        };
        let current = self.save_current_queued_edit().unwrap_or(0);
        let len = self.active_tab().queued_input.len();
        if len == 0 || current + 1 >= len {
            self.queued_edit_index = None;
            self.input.take();
            self.reset_input_browse_state();
            return true;
        }
        self.select_queued_edit(current + 1)
    }

    fn finish_queued_edit(&mut self, text: String) -> bool {
        if self.queued_edit_index.is_none() {
            return false;
        }
        let removed = text.trim().is_empty();
        self.save_queued_edit_text(text);
        self.queued_edit_index = None;
        self.reset_input_browse_state();
        self.toast = Some(Toast::info(if removed {
            crate::tr("queued message removed")
        } else {
            crate::tr("queued message updated")
        }));
        true
    }

    fn active_tab(&self) -> &SessionTab {
        &self.tabs[self.active]
    }

    fn active_tab_mut(&mut self) -> &mut SessionTab {
        &mut self.tabs[self.active]
    }

    /// Open a fresh tab (Ctrl+T) and focus it immediately; its engine is
    /// assembled OFF the event loop (skills scan, MCP connect, LSP discovery
    /// can take seconds — run inline they froze every tab). Until the
    /// `ReassembleDone` lands, the tab shows as Switching and borrows the
    /// current tab's engine Arc as a placeholder — it is busy the whole time,
    /// so nothing can run against the borrowed engine.
    fn new_tab(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let session_id = Uuid::new_v4().simple().to_string();
        let placeholder = self.active_tab().engine.clone();
        let mut tab = SessionTab::new(id, placeholder, session_id);
        tab.extension_access = self.template.tool_access();
        tab.reassemble_pending = true;
        tab.reassemble_seq = 1;
        tab.mode = Mode::Switching;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.dismiss_input_popups();
        self.queued_edit_index = None;

        let template = self.template.clone();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = match template.assemble_tab(None, Some(id.to_string())).await {
                Ok(engine) => Ok(ReassembledEngine { template, engine }),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id: id,
                seq: 1,
                effect: ReassembleEffect::NewTab,
                result,
            });
        });
    }

    /// Close the active tab (Ctrl+W). Aborts its in-flight turn first; closing
    /// the last tab quits.
    #[cfg(test)]
    fn close_active_tab(&mut self) {
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel();
        self.close_active_tab_with_events(&agent_tx);
    }

    fn close_active_tab_with_events(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        // Drop the tab's clipboard preview temp files before it goes away.
        let temps: Vec<std::path::PathBuf> = self.tabs[self.active]
            .pending_images
            .iter()
            .map(|i| i.path.clone())
            .collect();
        for path in &temps {
            cleanup_clipboard_temp(&mut self.clipboard_temps, path);
        }
        let closing_tab_id = self.tabs[self.active].id;
        let closing_task_id = self.tabs[self.active].session_id.clone();
        self.extension_attachments.remove_task(&closing_task_id);
        self.template
            .remove_approval_source(&closing_tab_id.to_string());
        self.deny_tui_approvals(TuiApprovalCleanupTarget::Source {
            tab_id: closing_tab_id,
        });
        self.clear_extension_turn_state_for_closed_tab(closing_tab_id);
        // A `/loop` owned by this tab can never run again: `poll_scheduler`
        // finds no owning tab and drops the prompt — but `due()` has already
        // incremented `runs`, so a `--max N` loop would silently burn all N
        // executions and linger in `/loop list` forever. Retire them with the
        // tab, and forget their failure streaks.
        let orphaned = self.scheduler.stop_loops_for_owner(closing_tab_id as u64);
        if !orphaned.is_empty() {
            self.purge_sched_jobs(|job| match job {
                SchedJobRef::Loop(id) => orphaned.contains(id),
                SchedJobRef::Schedule(_) => false,
            });
        }
        let active_job = self.tabs[self.active].active_sched_job.clone();
        let mut active_turn_id = (self.tabs[self.active].active_turn_id > 0)
            .then_some(self.tabs[self.active].active_turn_id)
            .or(self.tabs[self.active].draining_turn_id);
        if self.tabs[self.active].active_turn_id > 0 {
            if let Some(interrupt) = self.prepare_tab_interrupt(self.active, active_turn_id) {
                active_turn_id = interrupt.turn_id;
                if let Some(turn_id) = interrupt.turn_id {
                    self.watchdog
                        .cancel_turn(interrupt.tab_id, turn_id, std::time::Instant::now());
                }
                interrupt.abort.abort_with_reason("tab closed");
            }
        } else if active_turn_id.is_none() {
            // Local operations do not own scheduler state, but still receive
            // their cooperative cancellation before the tab disappears.
            if let Some(abort) = self.tabs[self.active].turn_abort.take() {
                abort.abort_with_reason("tab closed");
            }
        }
        if let (Some(turn_id), Some(job)) = (active_turn_id, active_job.clone()) {
            self.begin_forced_turn_stop(
                closing_tab_id,
                turn_id,
                ForcedTurnStop::Manual { job, failure: None },
                agent_tx,
            );
        }
        // A non-final tab close is an explicit cancellation. Closing the last
        // tab is application shutdown instead: keep queued schedule leases so
        // the two-phase exit can restore their exact fire/retry tokens rather
        // than silently consuming work that never started.
        if self.tabs.len() == 1 {
            let queued_jobs: Vec<SchedJobRef> = self
                .sched_pending
                .iter()
                .filter(|((tab_id, _), _)| *tab_id == closing_tab_id)
                .flat_map(|(_, jobs)| jobs.iter().cloned())
                .collect();
            self.sched_pending
                .retain(|(tab_id, _), _| *tab_id != closing_tab_id);
            for job in queued_jobs {
                self.sched_queued_at.remove(&job);
            }
        } else {
            self.purge_sched_pending_for_tab(closing_tab_id);
        }
        let cancelled_watchdog_jobs = self.watchdog.cancel_tab(closing_tab_id);
        for job in &cancelled_watchdog_jobs {
            if active_job.as_ref() != Some(job) {
                self.cancel_persisted_retry_if_present(job);
            }
        }
        // A scheduler turn's task was already taken by `begin_forced_turn_stop`
        // above; anything left here is an interactive turn or local op. It
        // received a cooperative abort, so let it drain briefly — for an
        // interactive turn that lets its `TurnRecorder` (which lives inside the
        // task, alongside the engine) journal the terminal record and close the
        // checkpoint instead of leaving a dangling turn. Hard-abort only if the
        // task ignores cancellation within the grace window.
        if let Some(mut task) = self.tabs[self.active].turn_task.take() {
            tokio::spawn(async move {
                if tokio::time::timeout(HARD_STOP_QUIESCE_TIMEOUT, &mut task)
                    .await
                    .is_err()
                {
                    task.abort();
                    let _ = task.await;
                }
            });
        }
        if self.tabs.len() == 1 {
            self.should_quit = true;
            return;
        }
        self.tabs.remove(self.active);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.dismiss_input_popups();
        self.queued_edit_index = None;
        // The sidebar row→tab mapping just changed; drop the hitboxes until
        // the next draw rebuilds them, so a buffered click processed before
        // the redraw can't close or switch to the wrong tab via stale rows.
        // Covers every close trigger (Ctrl+W, the row × click, commands).
        self.sidebar_hits = crate::ui::tabs::SidebarHits::default();
    }

    /// Abort the active tab's in-flight turn, if any. Returns true when a turn
    /// was actually interrupted (false when the tab was already idle). Shared
    /// by Ctrl+C and Esc.
    fn interrupt_active_turn(&mut self) -> bool {
        let Some(interrupt) = self.prepare_tab_interrupt(self.active, None) else {
            return false;
        };
        if let Some(turn_id) = interrupt.turn_id {
            self.watchdog
                .cancel_turn(interrupt.tab_id, turn_id, std::time::Instant::now());
            self.resolve_extension_approvals_before_tui_interrupt(interrupt.tab_id, turn_id);
            self.mark_extension_turn_interrupt_requested(interrupt.tab_id, turn_id, None);
        }
        interrupt.abort.abort_with_reason("user interrupted");
        true
    }

    /// Desktop-Esc stops EVERYTHING: interrupt every tab with a running turn.
    /// Returns how many turns were interrupted.
    fn interrupt_all_running_turns(&mut self) -> usize {
        let mut n = 0;
        for idx in 0..self.tabs.len() {
            let Some(interrupt) = self.prepare_tab_interrupt(idx, None) else {
                continue;
            };
            if let Some(turn_id) = interrupt.turn_id {
                self.watchdog
                    .cancel_turn(interrupt.tab_id, turn_id, std::time::Instant::now());
                self.resolve_extension_approvals_before_tui_interrupt(interrupt.tab_id, turn_id);
                self.mark_extension_turn_interrupt_requested(interrupt.tab_id, turn_id, None);
            }
            interrupt.abort.abort_with_reason("user interrupted");
            n += 1;
        }
        n
    }

    fn prepare_tab_interrupt(
        &mut self,
        tab_idx: usize,
        expected_turn_id: Option<u64>,
    ) -> Option<PreparedTabInterrupt> {
        self.prepare_tab_interrupt_labeled(tab_idx, expected_turn_id, true)
    }

    /// `prepare_tab_interrupt` with control over the transcript notice. A
    /// watchdog timeout passes `push_interrupted = false` and prints its own
    /// "watchdog: … timeout" line instead, so a supervised failure is never
    /// mislabeled in the transcript as a user interruption.
    fn prepare_tab_interrupt_labeled(
        &mut self,
        tab_idx: usize,
        expected_turn_id: Option<u64>,
        push_interrupted: bool,
    ) -> Option<PreparedTabInterrupt> {
        let (prepared, local_op_id) = {
            let tab = self.tabs.get_mut(tab_idx)?;
            let active_turn_id = (tab.active_turn_id > 0).then_some(tab.active_turn_id);
            if expected_turn_id.is_some() && expected_turn_id != active_turn_id {
                return None;
            }
            let abort = tab.turn_abort.take()?;
            let local_op_id = if active_turn_id.is_none() {
                // The interrupted handle belonged to a local operation.
                // Invalidate its generation immediately; its eventual
                // completion/progress is stale even if another operation
                // starts before the event arrives.
                if std::mem::take(&mut tab.local_op_is_auto_compact) {
                    // Interrupting an AUTO compaction must open the breaker:
                    // the occupancy is still over threshold, so the trigger
                    // would otherwise restart compaction on the very next
                    // event and the tab would look stuck on "compacting"
                    // forever. Manual /compact stays available; a later
                    // successful compaction re-arms the trigger.
                    tab.auto_compact_failures = AUTO_COMPACT_MAX_FAILURES;
                    tab.chat.push_system(crate::tr(
                        "auto-compact interrupted — paused for this session; run /compact to compact manually",
                    ));
                }
                tab.active_local_op_id.take()
            } else {
                None
            };
            tab.active_turn_id = 0;
            // Only real agent turns emit TurnDone and therefore get a draining
            // latch. Local shell / compact / background abort users keep None.
            tab.draining_turn_id = active_turn_id;
            tab.active_tool_names.clear();
            tab.active_tool_api_names.clear();
            tab.active_tool_started.clear();
            stop_goal_loop(tab);
            tab.chat.end_turn();
            if push_interrupted {
                tab.chat.push_system(crate::tr("(interrupted)"));
            }
            tab.mode = Mode::Ready;
            (
                PreparedTabInterrupt {
                    tab_id: tab.id,
                    turn_id: active_turn_id,
                    abort,
                },
                local_op_id,
            )
        };
        if let Some(local_op_id) = local_op_id {
            self.template
                .clear_approval_local_operation_if(&prepared.tab_id.to_string(), local_op_id);
            self.deny_tui_approvals(TuiApprovalCleanupTarget::LocalOperation {
                tab_id: prepared.tab_id,
                op_id: local_op_id,
            });
        }
        if let Some(turn_id) = prepared.turn_id {
            self.template
                .clear_approval_turn_if(&prepared.tab_id.to_string(), turn_id);
            self.deny_tui_approvals(TuiApprovalCleanupTarget::Turn {
                tab_id: prepared.tab_id,
                turn_id,
            });
        }
        Some(prepared)
    }

    /// Focus the tab at position `idx` (Ctrl+digit), if it exists.
    fn switch_to(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active = idx;
            self.dismiss_input_popups();
            self.queued_edit_index = None;
            // The chip selection indexes the previous tab's images.
            self.selected_image = None;
        }
    }

    /// Cycle to the next tab (Ctrl+Tab), wrapping around.
    fn cycle_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
            self.dismiss_input_popups();
            self.queued_edit_index = None;
        }
    }

    /// The tab whose id matches, if still open (events from closed tabs drop).
    fn tab_by_id(&mut self, id: usize) -> Option<&mut SessionTab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Reserve one tab's shared abort/busy slot for a non-agent operation.
    /// Every producer uses the checked monotonic id returned here; completion
    /// and progress events are accepted only while this exact id still owns
    /// the slot.
    fn begin_local_operation(&mut self, tab_idx: usize) -> Option<(u64, AbortController)> {
        let tab = self.tabs.get_mut(tab_idx)?;
        if tab.is_busy() {
            return None;
        }
        let Some(op_id) = tab.local_op_seq.checked_add(1) else {
            self.toast = Some(Toast::error(crate::tr("local operation id exhausted")));
            return None;
        };
        let abort = AbortController::new();
        tab.local_op_seq = op_id;
        tab.active_local_op_id = Some(op_id);
        tab.turn_abort = Some(abort.clone());
        let tab_id = tab.id;
        self.template
            .bind_approval_local_operation(&tab_id.to_string(), op_id);
        Some((op_id, abort))
    }

    /// Route one immutable core approval request exactly once. The request's
    /// stamped `(source, typed owner)` is authoritative; current source
    /// bindings are never consulted here, so an old N request cannot attach to
    /// N+1 or cross from the turn domain into a same-numbered local operation.
    fn route_approval_request(&mut self, request: ApprovalRequest) {
        if self.should_quit {
            let _ = request.respond(Approval::Deny);
            return;
        }
        let Some(source) = request.source.as_deref() else {
            let _ = request.respond(Approval::Deny);
            return;
        };
        let Ok(tab_id) = source.parse::<usize>() else {
            let _ = request.respond(Approval::Deny);
            return;
        };
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            let _ = request.respond(Approval::Deny);
            return;
        };

        match (request.turn_id, request.local_op_id) {
            (Some(turn_id), None) => {
                if tab.active_turn_id != turn_id
                    || tab.turn_abort.is_none()
                    || tab.draining_turn_id.is_some()
                    || tab.reassemble_pending
                {
                    let _ = request.respond(Approval::Deny);
                    return;
                }
                let task_id = tab.session_id.clone();
                match self.classify_extension_approval_route(tab_id, turn_id, &task_id) {
                    extension_tasks::ExtensionApprovalRoute::Tui => {
                        self.enqueue_tui_approval(request);
                    }
                    extension_tasks::ExtensionApprovalRoute::Extension { connection_id } => {
                        self.enqueue_extension_approval(
                            request,
                            tab_id,
                            turn_id,
                            task_id,
                            connection_id,
                        );
                    }
                    extension_tasks::ExtensionApprovalRoute::Deny => {
                        let _ = request.respond(Approval::Deny);
                    }
                }
            }
            (None, Some(local_op_id))
                if tab.active_turn_id == 0
                    && tab.active_local_op_id == Some(local_op_id)
                    && tab.turn_abort.is_some()
                    && tab.draining_turn_id.is_none()
                    && !tab.reassemble_pending =>
            {
                // Extension routes own agent turns only. A local operation's
                // exact approval always remains in the terminal UI.
                self.enqueue_tui_approval(request);
            }
            // Missing or ambiguous typed ownership, a stale local operation,
            // or an otherwise non-live tab fails closed.
            _ => {
                let _ = request.respond(Approval::Deny);
            }
        }
    }

    fn enqueue_tui_approval(&mut self, request: ApprovalRequest) {
        // An approval is the highest-priority modal: dismiss overlays that
        // could hide the prompt now capturing input.
        self.settings = None;
        self.connect = None;
        self.session_picker = None;
        self.tasks_panel = None;
        self.subagents_panel = None;
        self.team_panel = None;
        self.files_panel = None;
        self.browser_panel = None;
        self.show_help = false;
        if self.active_dialog.is_none() {
            self.active_dialog = Some(self.open_approval(request));
        } else {
            self.pending_requests.push_back(request);
        }
    }

    /// Deny every terminal-UI approval matching one immutable typed owner.
    /// This is deliberately independent from busy-slot acceptance: a stale
    /// terminal still owns cleanup of its old cards, but cannot release a
    /// newer turn/local-operation slot or its core approval binding.
    fn deny_tui_approvals(&mut self, target: TuiApprovalCleanupTarget) {
        let active_matches = self
            .active_dialog
            .as_ref()
            .and_then(PermissionDialog::identity)
            .is_some_and(|identity| target.matches_identity(&identity));
        if active_matches {
            if let Some(request) = self
                .active_dialog
                .as_mut()
                .and_then(PermissionDialog::take_request)
            {
                let _ = request.respond(Approval::Deny);
            }
            self.active_dialog = None;
        }

        let mut kept = VecDeque::with_capacity(self.pending_requests.len());
        while let Some(request) = self.pending_requests.pop_front() {
            if target.matches_request(&request) {
                let _ = request.respond(Approval::Deny);
            } else {
                kept.push_back(request);
            }
        }
        self.pending_requests = kept;
        self.promote_next_tui_approval();
    }

    fn promote_next_tui_approval(&mut self) {
        while self.active_dialog.is_none() {
            let Some(request) = self.pending_requests.pop_front() else {
                break;
            };
            // Revalidate at promotion time: its immutable owner may have gone
            // stale while another dialog was ahead of it.
            self.route_approval_request(request);
        }
    }

    /// Build the permission dialog for `req`, first focusing the tab that
    /// requested it (its gate is labeled with the tab id) so the prompt shows
    /// over the right conversation and uses that tab's cwd. Only called when a
    /// request becomes the ACTIVE dialog — queued requests don't move focus.
    fn open_approval(&mut self, req: ApprovalRequest) -> PermissionDialog {
        // Show the modal WITHOUT switching the active tab: the dialog renders
        // on top and captures input regardless of which tab is focused, so a
        // background tab's approval no longer teleports the user (and wipes
        // their compose state) away from the tab they're typing in. Only when
        // the request is FOR the active tab do we clear its input popups (the
        // dialog now owns input). The dialog's cwd comes from the source tab
        // so the prompt shows the right directory even without focusing it.
        let src_tab = req
            .source
            .as_deref()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|src| self.tabs.iter().find(|t| t.id == src));
        let cwd = src_tab
            .map(|t| t.engine.cwd.clone())
            .unwrap_or_else(|| self.active_tab().engine.cwd.clone());
        let is_active_tab = src_tab.map(|t| t.id) == Some(self.active_tab().id);
        if is_active_tab {
            self.dismiss_input_popups();
            self.queued_edit_index = None;
        }
        PermissionDialog::new(req, cwd)
    }

    /// Respond to the active permission prompt, then surface the next queued
    /// request (if any), focusing its source tab/cwd.
    fn answer_permission(&mut self, approval: Approval) {
        let Some(mut dialog) = self.active_dialog.take() else {
            return;
        };
        let identity = dialog.identity();
        let exact_live = identity
            .as_ref()
            .is_some_and(|identity| self.tui_approval_identity_is_live(identity));
        let cwd = dialog.cwd().to_path_buf();
        if let Some(request) = dialog.take_request() {
            let tool = request.tool.clone();
            // Scope gate: an external-agent trust grant (CarryFingerprintGrant
            // etc.) is session-only — persisting it as a project tool allow
            // would silently un-gate every future Task call.
            let persistable = request.scope.persist_allow_always();
            let actual = if exact_live { approval } else { Approval::Deny };
            let respond_ok = request.respond(actual).is_ok();
            if exact_live && respond_ok && approval == Approval::AllowAlways && persistable {
                self.persist_allow_always_at(&cwd, &tool);
            }
        }
        // `had_request` dismisses the card even if its receiver disappeared;
        // an empty dialog must never remain above the input.
        self.promote_next_tui_approval();
    }

    fn tui_approval_identity_is_live(&self, identity: &PermissionRequestIdentity) -> bool {
        let Some(tab_id) = identity
            .source
            .as_deref()
            .and_then(|source| source.parse::<usize>().ok())
        else {
            return false;
        };
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return false;
        };
        if tab.turn_abort.is_none() || tab.draining_turn_id.is_some() || tab.reassemble_pending {
            return false;
        }
        match (identity.turn_id, identity.local_op_id) {
            (Some(turn_id), None) if tab.active_turn_id == turn_id => {
                !self.extension_turn_has_route(tab_id, turn_id)
            }
            (None, Some(local_op_id)) => {
                tab.active_turn_id == 0 && tab.active_local_op_id == Some(local_op_id)
            }
            _ => false,
        }
    }

    /// Record a tool name into `<cwd>/.zode/state.json` `permissions.allow`
    /// (deduped) so an "allow always" choice survives restarts.
    fn persist_allow_always_at(&self, cwd: &std::path::Path, tool: &str) {
        let tool = tool.to_string();
        // Best-effort: a failed persist must not interrupt the turn.
        let _ = zode_core::config::ConfigManager::update_project_state(cwd, |s| {
            let perms = s
                .entry("permissions")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(perms) = perms.as_object_mut() {
                let allow = perms
                    .entry("allow")
                    .or_insert_with(|| serde_json::json!([]));
                if let Some(arr) = allow.as_array_mut() {
                    let val = serde_json::Value::String(tool);
                    if !arr.contains(&val) {
                        arr.push(val);
                    }
                }
            }
        });
    }

    /// Show a question modal, focusing the tab that asked (its `source` id) —
    /// but not while a permission dialog is up (which captures input on top and
    /// is about a different tab), so we don't disorient by switching away.
    fn open_question(&mut self, req: QuestionRequest) {
        if self.active_dialog.is_none() {
            if let Some(src) = req.source.as_deref().and_then(|s| s.parse::<usize>().ok()) {
                if let Some(pos) = self.tabs.iter().position(|t| t.id == src) {
                    self.active = pos;
                    // Focus moved tabs; drop a popup tied to the previous tab.
                    self.dismiss_input_popups();
                    self.queued_edit_index = None;
                }
            }
        }
        self.active_question = Some(QuestionDialog::new(req));
    }

    fn route_question_request(&mut self, req: QuestionRequest) {
        if self.should_quit {
            let _ = req.respond(None);
            return;
        }
        // A question is a modal like an approval: clear overlays so it cannot
        // be hidden, then show it or queue it behind the active question.
        self.settings = None;
        self.connect = None;
        self.session_picker = None;
        self.tasks_panel = None;
        self.subagents_panel = None;
        self.files_panel = None;
        self.team_panel = None;
        self.browser_panel = None;
        self.show_help = false;
        if self.active_question.is_none() {
            self.open_question(req);
        } else {
            self.pending_questions.push_back(req);
        }
    }

    fn reject_pending_ui_requests_for_shutdown(&mut self) {
        if let Some(mut dialog) = self.active_dialog.take() {
            if let Some(request) = dialog.take_request() {
                let _ = request.respond(Approval::Deny);
            }
        }
        while let Some(request) = self.pending_requests.pop_front() {
            let _ = request.respond(Approval::Deny);
        }
        if let Some(mut question) = self.active_question.take() {
            question.dismiss();
        }
        while let Some(request) = self.pending_questions.pop_front() {
            let _ = request.respond(None);
        }
    }

    /// Start rebuilding the active tab's engine from `template` off the UI loop
    /// (model/provider/goal/plugin/sandbox switches all land here), carrying the
    /// conversation store + cwd over so the context survives. The template and
    /// status are committed only when the background result arrives.
    fn start_reassemble_active(
        &mut self,
        global_candidate: EngineTemplate,
        effect: ReassembleEffect,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't switch during a turn — Ctrl+C first",
            )));
            return false;
        }
        // Keep the candidate stored in `self.template` clean. The engine is
        // assembled from an effective task-local overlay; only the clean
        // candidate is committed when the background result succeeds.
        let candidate_changes_model = global_candidate.model() != self.template.model();
        let candidate_changes_access =
            global_candidate.tool_access() != self.template.tool_access();
        let (store, carry, cwd, id, seq, active_model, active_access, plan_mode) = {
            let tab = self.active_tab();
            // The Arc, not a clone: deep-copying a large transcript inline
            // stalled the event loop on every /yolo //sandbox /model switch.
            // The tab is marked reassemble_pending before the spawn below, so
            // nothing mutates the store while the task snapshots it.
            let store = tab.engine.store.clone();
            (
                store,
                // Carry the long-lived session state (cost, undo history, bg
                // shells, todos, sub-agents, compaction latches, file cache)
                // so a hot-swap doesn't reset it or orphan running shells.
                tab.engine.carry_state(),
                tab.engine.cwd.clone(),
                tab.id,
                tab.reassemble_seq + 1,
                tab.engine.model.clone(),
                tab.extension_access,
                tab.plan_mode,
            )
        };
        let target_model = match &effect {
            ReassembleEffect::Model { id } => id.clone(),
            _ if candidate_changes_model => global_candidate
                .model()
                .map(str::to_string)
                .unwrap_or(active_model),
            _ => active_model,
        };
        let target_access = match &effect {
            ReassembleEffect::Yolo { access, .. } => *access,
            _ if candidate_changes_access => global_candidate.tool_access(),
            _ => active_access,
        };
        let engine_template = global_candidate
            .with_model(target_model)
            .with_tool_access(target_access)
            .with_plan_mode(plan_mode);
        {
            let tab = self.active_tab_mut();
            tab.reassemble_seq = seq;
            tab.reassemble_pending = true;
            tab.mode = Mode::Switching;
            tab.active_tool_names.clear();
            tab.active_tool_api_names.clear();
            tab.active_tool_started.clear();
        }

        let tx = agent_tx.clone();
        tokio::spawn(async move {
            // Deep-copy the transcript HERE, off the event loop (the tab is
            // reassemble_pending, so nothing mutates it meanwhile).
            let store = match store.lock() {
                Ok(s) => s.clone(),
                Err(_) => {
                    let _ = tx.send(AppEvent::ReassembleDone {
                        tab_id: id,
                        seq,
                        effect,
                        result: Err("store lock poisoned".to_string()),
                    });
                    return;
                }
            };
            // A sandbox switch must prove the sandbox actually enforces
            // before it is applied (fail-closed). Verified HERE, off the
            // event loop — the inline `verify().await` used to freeze the
            // UI for the sandbox-exec round trip.
            if matches!(effect, ReassembleEffect::Sandbox) {
                if let Some(sb) = engine_template.sandbox().cloned() {
                    if let Err(e) = sb.verify().await {
                        let _ = tx.send(AppEvent::ReassembleDone {
                            tab_id: id,
                            seq,
                            effect,
                            result: Err(format!("sandbox: {e}")),
                        });
                        return;
                    }
                }
            }
            let result = match engine_template
                .assemble_tab_with_carry(Some(cwd), Some(id.to_string()), carry)
                .await
            {
                Ok(engine) => Ok(ReassembledEngine {
                    template: global_candidate,
                    engine: engine.with_store(store),
                }),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id: id,
                seq,
                effect,
                result,
            });
        });
        true
    }

    fn handle_reassemble_done(
        &mut self,
        tab_id: usize,
        seq: u64,
        effect: ReassembleEffect,
        result: Result<ReassembledEngine, String>,
    ) {
        let extension_context = match &effect {
            ReassembleEffect::ExtensionNewTab {
                connection_id,
                failure_code,
            }
            | ReassembleEffect::ExtensionResumeTab {
                connection_id,
                failure_code,
            }
            | ReassembleEffect::ExtensionReconfigure {
                connection_id,
                failure_code,
                ..
            } => Some((*connection_id, *failure_code)),
            _ => None,
        };
        let extension_failure = extension_context.and_then(|(_, failure_code)| {
            result.as_ref().err().map(|message| {
                (
                    failure_code.unwrap_or("engine_assemble_failed"),
                    message.clone(),
                )
            })
        });
        let extension_connection = extension_context.map(|(connection_id, _)| connection_id);
        let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            if extension_connection.is_some() {
                let pending_task_ids: HashSet<String> = self
                    .tabs
                    .iter()
                    .filter(|tab| tab.reassemble_pending)
                    .map(|tab| tab.session_id.clone())
                    .collect();
                let removed_task_ids = self.extension_tasks.retain_pending_tasks(&pending_task_ids);
                let fallback = self.tabs.get(self.active).map(|tab| tab.session_id.clone());
                for removed_task_id in removed_task_ids {
                    self.extension_tasks
                        .replace_task_selection(&removed_task_id, fallback.as_deref());
                }
                self.extension_tasks.queue_completion(
                    extension_failure
                        .as_ref()
                        .map(|(code, message)| (*code, message.as_str())),
                );
            }
            return;
        };
        if !self.tabs[tab_idx].reassemble_pending || self.tabs[tab_idx].reassemble_seq != seq {
            return;
        }
        let extension_task_id = extension_connection
            .is_some()
            .then(|| self.tabs[tab_idx].session_id.clone());
        let focused_tab_id = self.tabs.get(self.active).map(|tab| tab.id);
        self.tabs[tab_idx].reassemble_pending = false;
        // A new/resumed tab assembles under an UNCHANGED template clone —
        // installing it back would clobber a template switch that happened
        // while the assembly ran.
        let tab_creation = matches!(
            &effect,
            ReassembleEffect::NewTab
                | ReassembleEffect::ResumeTab
                | ReassembleEffect::ExtensionNewTab { .. }
                | ReassembleEffect::ExtensionResumeTab { .. }
        );
        let extension_reconfigure =
            matches!(&effect, ReassembleEffect::ExtensionReconfigure { .. });
        match result {
            Ok(done) => {
                let model = done.engine.model.clone();
                self.tabs[tab_idx].engine = Arc::new(done.engine);
                self.tabs[tab_idx].mode = Mode::Ready;
                if !tab_creation && !extension_reconfigure {
                    self.template = done.template;
                    self.status.model = model;
                    // The `/browser` panel's default-enabled flag lives on the
                    // shared session; sync it only now that the new tool set is
                    // real (a failed rebuild must not flip the visible state).
                    self.template.sync_browser_session_enabled();
                    // Re-read managed UI plugin renderers so plugin
                    // install/enable/disable done since the last load (e.g.
                    // via the `zode plugin` CLI) takes effect — including
                    // stopping a removed plugin's background HTTP data tasks.
                    // Cheap no-op when nothing changed.
                    self.ui_extensions.reload();
                }
                if self.active < self.tabs.len() && self.tabs[self.active].id == tab_id {
                    self.refresh_dynamic_commands();
                }
                self.apply_reassemble_effect(tab_idx, effect);
            }
            Err(e) if tab_creation => {
                if self.tabs.len() == 1 {
                    // The parent may have been closed while this placeholder
                    // assembled. Keep the sole remaining tab renderable for
                    // the final draw/exit hint path, but exit immediately: the
                    // failed placeholder still owns a clone of the parent's
                    // engine/store and must never become interactive.
                    self.tabs[tab_idx].mode = Mode::Error;
                    self.active = 0;
                    self.should_quit = true;
                    self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("switch failed"))));
                } else {
                    // With another real tab available, discard the failed
                    // placeholder instead of retaining its cloned engine/store.
                    self.tabs.remove(tab_idx);
                    self.active = focused_tab_id
                        .and_then(|focused_id| {
                            self.tabs.iter().position(|tab| tab.id == focused_id)
                        })
                        .unwrap_or_else(|| tab_idx.min(self.tabs.len() - 1));
                    self.toast = Some(Toast::error(e));
                }
            }
            Err(e) if extension_reconfigure => {
                // The old engine and task-local fields were never replaced;
                // return the task to its prior ready state and surface the
                // failure only through the extension completion channel.
                self.tabs[tab_idx].mode = Mode::Ready;
                self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("switch failed"))));
            }
            Err(e) => {
                if let ReassembleEffect::Plan { on } = &effect {
                    self.tabs[tab_idx].plan_mode = !*on;
                }
                self.tabs[tab_idx]
                    .chat
                    .push_system(&format!("{}: {e}", crate::tr("switch failed")));
                self.tabs[tab_idx].mode = Mode::Error;
                self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("switch failed"))));
            }
        }
        if extension_connection.is_some() {
            if let Some(task_id) = extension_task_id.as_deref() {
                self.extension_tasks.finish_background_task(task_id);
                if !self.tabs.iter().any(|tab| tab.session_id == task_id) {
                    let fallback = self.tabs.get(self.active).map(|tab| tab.session_id.clone());
                    self.extension_tasks
                        .replace_task_selection(task_id, fallback.as_deref());
                }
            }
            self.extension_tasks.queue_completion(
                extension_failure
                    .as_ref()
                    .map(|(code, message)| (*code, message.as_str())),
            );
        }
    }

    fn apply_reassemble_effect(&mut self, tab_idx: usize, effect: ReassembleEffect) {
        match effect {
            ReassembleEffect::AgentReload {
                notify,
                refresh_dialog,
            } => {
                self.apply_reassemble_notify(tab_idx, notify);
                if refresh_dialog {
                    self.agents_dialog = Some(AgentsDialog::new(self.agent_rows()));
                }
            }
            ReassembleEffect::Connect {
                provider_name,
                model,
            } => {
                // Provider and model are labeled separately: the dialog's
                // display name may BE a model id, and flashing that under a
                // "provider" label reads as the wrong state.
                let message = match &model {
                    Some(model) => format!(
                        "{} -> {provider_name} · {} -> {model}",
                        crate::tr("provider"),
                        crate::tr("model")
                    ),
                    None => format!("{} -> {provider_name}", crate::tr("provider")),
                };
                self.toast = Some(Toast::info(message.clone()));
                self.tabs[tab_idx].chat.push_system(&message);
            }
            ReassembleEffect::Effort { notify } => {
                self.apply_reassemble_notify(tab_idx, notify);
            }
            ReassembleEffect::Goal { goal } => self.apply_goal_effect(tab_idx, goal),
            ReassembleEffect::Model { id } => self.apply_model_effect(tab_idx, &id),
            // Fresh tab: nothing to announce — the tab flipping from
            // Switching to Ready IS the signal.
            ReassembleEffect::NewTab | ReassembleEffect::ExtensionNewTab { .. } => {}
            // Resumed tab: the engine arrived with the saved store attached —
            // replay it into the transcript and seed the context gauge.
            ReassembleEffect::ResumeTab | ReassembleEffect::ExtensionResumeTab { .. } => {
                let rebuilt = {
                    let tab = &self.tabs[tab_idx];
                    tab.engine.store.lock().ok().map(|store| {
                        (
                            rebuild_chat_from_store(&store),
                            estimate_store_tokens(&store),
                            store.len(),
                        )
                    })
                };
                if let Some((chat, tokens, len)) = rebuilt {
                    let tab = &mut self.tabs[tab_idx];
                    tab.chat = chat;
                    tab.context_tokens = tokens;
                    seed_prompt_history_for_tab(tab);
                    // Seed the append watermark to the loaded length so the
                    // first post-resume save appends onto the existing file
                    // instead of mismatching and rewriting it.
                    tab.persisted_msgs
                        .store(len, std::sync::atomic::Ordering::Relaxed);
                }
            }
            ReassembleEffect::ExtensionReconfigure { access, .. } => {
                self.tabs[tab_idx].extension_access = access;
            }

            ReassembleEffect::Notify(notify) => self.apply_reassemble_notify(tab_idx, notify),
            ReassembleEffect::Orchestration { on, notify } => {
                if let Ok(mut cfg) = ConfigManager::load_global() {
                    cfg.autonomous_orchestration = Some(on);
                    let _ = ConfigManager::save_global(&cfg);
                }
                self.apply_reassemble_notify(tab_idx, notify);
            }
            ReassembleEffect::Plan { on } => {
                self.tabs[tab_idx].chat.push_system(if on {
                    crate::tr("plan mode: ON — read-only tools only; research and present a plan, then /plan to execute")
                } else {
                    crate::tr("plan mode: OFF — full tools restored")
                });
            }
            ReassembleEffect::ReloadSkills => {
                let n = self.tabs[tab_idx].engine.skills.list().len();
                let msg = format!(
                    "{} ({n} {})",
                    crate::tr("reloaded skills"),
                    crate::tr("loaded")
                );
                self.tabs[tab_idx].chat.push_system(&msg);
            }
            ReassembleEffect::Sandbox => self.apply_sandbox_reassemble_effect(tab_idx),
            ReassembleEffect::Yolo { access, notify } => {
                self.tabs[tab_idx].extension_access = access;
                // Persist the choice GLOBALLY (`~/.zode/config.json`) so every
                // workspace's next launch starts with it. Applied only here —
                // after the reassemble actually succeeded. Also drop any older
                // per-project state entry: project layers override global at
                // load, so a stale value would shadow this new choice.
                let on = matches!(access, zode_core::ToolAccessMode::Auto);
                self.persist_global_toggle(|cfg| cfg.yolo = Some(on));
                let cwd = self.tabs[tab_idx].engine.cwd.clone();
                let _ = zode_core::config::ConfigManager::update_project_state(&cwd, |s| {
                    s.remove("yolo");
                });
                self.apply_reassemble_notify(tab_idx, notify);
            }
        }
    }

    fn apply_goal_effect(&mut self, tab_idx: usize, goal: Option<String>) {
        match goal {
            Some(g) => {
                self.tabs[tab_idx]
                    .chat
                    .push_system(&format!("{}: {g}", crate::tr("goal set")));
                self.tabs[tab_idx].engine.reset_goal_completed();
                let tab = &mut self.tabs[tab_idx];
                tab.goal_loop_active = true;
                tab.goal_loop_iter = 0;
                tab.goal_no_progress_streak = 0;
                tab.goal_text = Some(g);
                tab.goal_started_at = Some(std::time::Instant::now());
                tab.queued_input
                    .push_back(GOAL_LOOP_START_PROMPT.to_string());
                self.toast = Some(Toast::info(crate::tr("goal-loop: started")));
            }
            None => {
                let tab = &mut self.tabs[tab_idx];
                stop_goal_loop(tab);
                tab.chat.push_system(crate::tr("goal cleared"));
            }
        }
    }

    fn apply_model_effect(&mut self, tab_idx: usize, id: &str) {
        self.persist_active_model_choice(id);
        self.status.model = self.tabs[tab_idx].engine.model.clone();
        self.tabs[tab_idx]
            .chat
            .push_system(&format!("{} → {id}", crate::tr("model")));
    }

    fn toggle_yolo(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        // Toggle the active task's effective access, not the clean global
        // default: resumed/extension tasks can intentionally diverge from that
        // default and must switch on the first use.
        let on = !matches!(
            self.active_tab().extension_access,
            zode_core::ToolAccessMode::Auto
        );
        let template = self.template.with_yolo(on);
        let access = if on {
            zode_core::ToolAccessMode::Auto
        } else {
            zode_core::ToolAccessMode::Prompt
        };
        let message = if on {
            crate::tr("yolo: ON — tools auto-approve (deny rules still apply)")
        } else {
            crate::tr("yolo: OFF — tools prompt for approval")
        };
        self.start_reassemble_active(
            template,
            ReassembleEffect::Yolo {
                access,
                notify: ReassembleNotify::System(message.to_string()),
            },
            agent_tx,
        );
    }

    fn persist_active_model_choice(&mut self, id: &str) {
        #[cfg(test)]
        {
            let _ = id;
        }

        #[cfg(not(test))]
        {
            if let Ok(mut cfg) = ConfigManager::load_global() {
                cfg.set_active_model(id);
                if let Err(e) = ConfigManager::save_global(&cfg) {
                    self.toast = Some(Toast::error(format!(
                        "{}: {e}",
                        crate::tr("save config failed")
                    )));
                }
            }
        }
    }

    fn apply_reassemble_notify(&mut self, tab_idx: usize, notify: ReassembleNotify) {
        match notify {
            ReassembleNotify::None => {}
            ReassembleNotify::Toast(text) => self.toast = Some(Toast::info(text)),
            ReassembleNotify::System(text) => self.tabs[tab_idx].chat.push_system(&text),
        }
    }

    /// Best-effort global-config update for runtime toggles (`/yolo`,
    /// `/sandbox`): load → mutate → atomic save, so the choice applies to
    /// EVERY workspace's next launch. A failure surfaces as a toast but never
    /// breaks the toggle itself (this run's in-memory state already switched).
    fn persist_global_toggle(&mut self, f: impl FnOnce(&mut zode_core::config::ZodeConfig)) {
        let result = ConfigManager::load_global().and_then(|mut cfg| {
            f(&mut cfg);
            ConfigManager::save_global(&cfg)
        });
        if let Err(e) = result {
            self.toast = Some(Toast::error(format!(
                "{}: {e}",
                crate::tr("save config failed")
            )));
        }
    }

    fn apply_sandbox_reassemble_effect(&mut self, tab_idx: usize) {
        use zode_core::sandbox::SandboxMode;

        let new_sandbox = self.template.sandbox().cloned();
        self.status.sandbox = new_sandbox.is_some();
        let cwd = self.tabs[tab_idx].engine.cwd.clone();
        let mode = new_sandbox.as_ref().map(|c| match c.mode() {
            SandboxMode::ReadOnly => "read-only",
            SandboxMode::WorkspaceWrite => "workspace-write",
        });
        let network = new_sandbox.as_ref().map(|c| c.allow_network());
        let enabled = new_sandbox.is_some();
        // Persist GLOBALLY so the toggle applies to every workspace's next
        // launch. Disabling only records `enabled=false` — mode/network keep
        // their previous values so re-enabling restores the old shape. Drop
        // any older per-project state entry (project layers override global
        // at load, so a stale value would shadow this new choice).
        self.persist_global_toggle(|cfg| {
            cfg.sandbox.enabled = Some(enabled);
            if let Some(mode) = mode {
                cfg.sandbox.mode = Some(mode.to_string());
            }
            if let Some(network) = network {
                cfg.sandbox.network = Some(network);
            }
        });
        let _ = zode_core::config::ConfigManager::update_project_state(&cwd, |s| {
            s.remove("sandbox");
        });
        let line = sandbox_status_line(new_sandbox.as_ref());
        self.tabs[tab_idx].chat.push_system(&line);
    }

    /// Rebuild the autocomplete's dynamic command set from the active engine:
    /// user+built-in sub-agents, skills, and MCP tools. Call after assembly and
    /// each reassemble (these change with provider/plugin switches).
    fn refresh_dynamic_commands(&mut self) {
        let mut cmds: Vec<DynCmd> = Vec::new();
        {
            let eng = &self.active_tab().engine;
            for (name, desc) in &eng.agent_types {
                cmds.push(DynCmd {
                    name: name.clone(),
                    kind: "agent",
                    description: desc.clone(),
                });
            }
            for s in eng.skills.list() {
                cmds.push(DynCmd {
                    name: s.name.clone(),
                    kind: "skill",
                    description: s.description.clone(),
                });
            }
            if let Some(lc) = &eng.mcp {
                for server in lc.registry.snapshot() {
                    for tool in server.state.tool_names() {
                        cmds.push(DynCmd {
                            name: tool.clone(),
                            kind: "MCP",
                            description: format!("{} tool", server.name),
                        });
                    }
                }
            }
            for c in &eng.user_commands {
                cmds.push(DynCmd {
                    name: c.name.clone(),
                    kind: "command",
                    description: c.description.clone(),
                });
            }
        }
        // Builtins always win a name clash (`expand_dynamic_command` returns
        // None for any registry name), so a dynamic entry shadowing a builtin
        // would render as a duplicate popup row that does nothing when
        // chosen. Drop them here instead of offering dead rows.
        let registry = zode_core::commands::CommandRegistry::with_builtins();
        cmds.retain(|c| registry.get(&c.name).is_none());
        self.autocomplete.set_dynamic(cmds);
    }

    /// If `/name` is a dynamic command (agent / skill / MCP tool, not a
    /// built-in), expand it to a templated turn that directs the agent to use
    /// it. Returns None for built-ins (handled by `handle_slash`) and unknowns.
    fn expand_dynamic_command(&self, name: &str, args: &str) -> Option<String> {
        if zode_core::commands::CommandRegistry::with_builtins()
            .get(name)
            .is_some()
        {
            return None;
        }
        let eng = &self.active_tab().engine;
        if eng.agent_types.iter().any(|(n, _)| n == name) {
            return Some(format!(
                "Use the `{name}` sub-agent (via the Task tool) for the following task:\n\n{args}"
            ));
        }
        if eng.skills.list().iter().any(|s| s.name == name) {
            return Some(format!(
                "Use the `{name}` skill for the following:\n\n{args}"
            ));
        }
        if let Some(lc) = &eng.mcp {
            let is_tool = lc
                .registry
                .snapshot()
                .iter()
                .any(|s| s.state.tool_names().iter().any(|t| t == name));
            if is_tool {
                return Some(format!(
                    "Use the MCP tool `{name}` for the following:\n\n{args}"
                ));
            }
        }
        // User/plugin command: submit its prompt body (with any args appended).
        if let Some(cmd) = eng.user_commands.iter().find(|c| c.name == name) {
            return Some(if args.trim().is_empty() {
                cmd.body.clone()
            } else {
                format!("{}\n\n{args}", cmd.body)
            });
        }
        None
    }

    fn open_workflows_dialog(&mut self) {
        self.workflows_dialog = Some(WorkflowsDialog::new(self.workflow_rows()));
    }

    fn open_mcp_dialog(&mut self) {
        let plugins = self.active_tab().engine.plugin_list();
        self.mcp_dialog = Some(McpDialog::new(plugins));
    }

    /// Apply staged MCP enable/disable on close (reuses the plugin apply path,
    /// scoped to MCP ids so other plugins' disabled state is preserved).
    async fn close_mcp_dialog(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let Some(dialog) = self.mcp_dialog.take() else {
            return;
        };
        if dialog.is_dirty() {
            // MCP-scoped dialog: it never shows packages, so nothing to flip.
            self.apply_plugins(
                dialog.disabled_ids(),
                dialog.all_ids(),
                Vec::new(),
                agent_tx,
            )
            .await;
        }
    }

    async fn handle_mcp_dialog_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match code {
            KeyCode::Esc => self.close_mcp_dialog(agent_tx).await,
            KeyCode::Up => {
                if let Some(d) = &mut self.mcp_dialog {
                    d.prev();
                }
            }
            KeyCode::Down => {
                if let Some(d) = &mut self.mcp_dialog {
                    d.next();
                }
            }
            KeyCode::Char(' ') => {
                if let Some(d) = &mut self.mcp_dialog {
                    if let Some((name, on)) = d.toggle_selected() {
                        let state = if on {
                            crate::tr("enabled")
                        } else {
                            crate::tr("disabled")
                        };
                        self.toast = Some(Toast::info(format!(
                            "{name} {state} ({})",
                            crate::tr("esc to apply")
                        )));
                    }
                }
            }
            _ => {}
        }
    }

    fn workflow_rows(&self) -> Vec<WorkflowRow> {
        let cwd = self.active_tab().engine.cwd.clone();
        zode_core::workflows::load_workflow_defs(&cwd)
            .into_iter()
            .map(|w| WorkflowRow {
                name: w.name,
                description: w.description,
            })
            .collect()
    }

    async fn handle_workflows_dialog_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let Some(dialog) = &mut self.workflows_dialog else {
            return;
        };
        let action: Option<WorkflowsAction> = if dialog.is_input_mode() {
            match code {
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => dialog.submit(),
                KeyCode::Char(c) => {
                    dialog.form_push(c);
                    None
                }
                KeyCode::Backspace => {
                    dialog.form_backspace();
                    None
                }
                KeyCode::Enter => dialog.submit(),
                KeyCode::Esc => dialog.on_esc(),
                _ => None,
            }
        } else {
            match code {
                KeyCode::Up => {
                    dialog.prev();
                    None
                }
                KeyCode::Down => {
                    dialog.next();
                    None
                }
                KeyCode::Enter => dialog.on_enter(),
                KeyCode::Char('d') => dialog.on_delete(),
                KeyCode::Esc => dialog.on_esc(),
                _ => None,
            }
        };
        match action {
            Some(WorkflowsAction::Close) => self.workflows_dialog = None,
            Some(WorkflowsAction::Run { name }) => {
                self.workflows_dialog = None;
                self.spawn_workflow_run(name, agent_tx);
            }
            Some(WorkflowsAction::AiCreate { brief }) => {
                self.workflows_dialog = None;
                let prompt = format!(
                    "Create a reusable JS workflow for me using the `DefineWorkflow` tool. \
                     Here is what it should accomplish:\n\n{brief}\n\nWrite the orchestration \
                     script with agent()/parallel()/pipeline() so zode can execute it \
                     deterministically with RunWorkflow, then call DefineWorkflow."
                );
                self.submit(&prompt, agent_tx).await;
            }
            Some(WorkflowsAction::Delete { name }) => {
                match zode_core::workflows::delete_workflow_def(&name) {
                    Ok(true) => {
                        self.start_reassemble_active(
                            self.template.clone(),
                            ReassembleEffect::Notify(ReassembleNotify::Toast(format!(
                                "{}: {name}",
                                crate::tr("workflow deleted")
                            ))),
                            agent_tx,
                        );
                        self.workflows_dialog = Some(WorkflowsDialog::new(self.workflow_rows()));
                    }
                    Ok(false) => {
                        self.toast = Some(Toast::info(format!("{name} {}", crate::tr("not found"))))
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("delete failed"))))
                    }
                }
            }
            None => {}
        }
    }

    /// Open the session picker (/sessions, /resume) from the saved index.
    fn open_session_picker(&mut self) {
        let metas: Vec<SessionMeta> = match SessionIndex::load() {
            Ok(index) => index.newest_first().into_iter().cloned().collect(),
            Err(error) => {
                self.toast = Some(Toast::error(format!(
                    "{}: {error}",
                    crate::tr("load failed")
                )));
                return;
            }
        };
        if metas.is_empty() {
            self.toast = Some(Toast::info(crate::tr("no saved sessions yet")));
            return;
        }
        self.session_picker = Some(SessionPicker::new(metas));
    }

    async fn handle_picker_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match code {
            KeyCode::Esc => {
                // First Esc only disarms a pending delete; the picker closes
                // on the next one.
                let armed = self
                    .session_picker
                    .as_mut()
                    .is_some_and(|p| p.cancel_pending_delete());
                if !armed {
                    self.session_picker = None;
                }
            }
            KeyCode::Up => {
                if let Some(p) = &mut self.session_picker {
                    p.prev();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.session_picker {
                    p.next();
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = &mut self.session_picker {
                    p.pop_filter_char();
                }
            }
            KeyCode::Delete => {
                // Two-press confirmation: the first Delete arms (the picker
                // shows the prompt + red highlight), the second one deletes.
                let confirmed = match self.session_picker.as_mut().and_then(|p| p.press_delete()) {
                    Some(DeletePress::Confirmed(meta)) => Some(meta),
                    Some(DeletePress::Armed) | None => None,
                };
                if let Some(meta) = confirmed {
                    self.delete_session(&meta.id).await;
                    if let Some(p) = &mut self.session_picker {
                        p.remove(&meta.id);
                    }
                }
            }
            KeyCode::Enter => {
                let target = self.session_picker.as_ref().and_then(|p| p.selected());
                self.session_picker = None;
                if let Some(meta) = target {
                    self.resume_session(meta, agent_tx);
                }
            }
            KeyCode::Char(c) => {
                if let Some(p) = &mut self.session_picker {
                    p.push_filter_char(c);
                }
            }
            _ => {}
        }
    }

    /// Resume a saved session in a new tab. If the session is already open,
    /// just focus that tab. The transcript load + engine assembly run OFF the
    /// event loop (a large transcript + MCP connect can take seconds); until
    /// the `ReassembleDone` lands the tab is a busy Switching placeholder, and
    /// the `ResumeTab` effect rebuilds the chat from the loaded store.
    fn resume_session(&mut self, meta: SessionMeta, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if let Some(pos) = self.tabs.iter().position(|t| t.session_id == meta.id) {
            self.active = pos;
            self.dismiss_input_popups();
            return;
        }
        let path = match SessionIndex::session_path(&meta.id) {
            Ok(p) => p,
            Err(_) => {
                self.toast = Some(Toast::error(crate::tr("bad session path")));
                return;
            }
        };
        // Resume in the session's original directory when it still exists, so
        // tools operate in the right repo (not the launch cwd).
        let cwd_override = if std::path::Path::new(&meta.cwd).is_dir() {
            Some(std::path::PathBuf::from(&meta.cwd))
        } else {
            None
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let placeholder = self.active_tab().engine.clone();
        let mut tab = SessionTab::new(id, placeholder, meta.id.clone());
        tab.extension_access = zode_core::ToolAccessMode::Prompt;
        tab.title = meta.title.clone();
        tab.titled = true;
        tab.reassemble_pending = true;
        tab.reassemble_seq = 1;
        tab.mode = Mode::Switching;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.dismiss_input_popups();

        let clean_template = self.template.clone();
        let engine_template = clean_template
            .with_model(meta.model.clone())
            .with_tool_access(zode_core::ToolAccessMode::Prompt)
            .with_plan_mode(false);
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let store = Session::load(&path)
                    .await
                    .map_err(|e| format!("{}: {e}", crate::tr("load failed")))?;
                let engine = engine_template
                    .assemble_tab(cwd_override, Some(id.to_string()))
                    .await
                    .map_err(|e| format!("{}: {e}", crate::tr("assemble failed")))?;
                Ok(ReassembledEngine {
                    template: clean_template,
                    engine: engine.with_store(store),
                })
            }
            .await;
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id: id,
                seq: 1,
                effect: ReassembleEffect::ResumeTab,
                result,
            });
        });
    }

    /// Delete a saved session's transcript file and index entry. Open tabs are
    /// untouched (they re-create the file on the next save). The index write
    /// goes through the shared lock so it can't race a concurrent save.
    async fn delete_session(&mut self, id: &str) {
        if let Ok(path) = SessionIndex::session_path(id) {
            let _ = std::fs::remove_file(path);
        }
        crate::tab::index_remove(id).await;
        self.toast = Some(Toast::info(crate::tr("session deleted")));
    }

    /// Open the background tasks panel (Ctrl+B / /tasks).
    async fn open_tasks_panel(&mut self) {
        self.refresh_bg_shells().await;
        self.tasks_panel = Some(TasksPanel::new());
    }

    /// Refresh the cached shell snapshot from the active tab's tracker.
    async fn refresh_bg_shells(&mut self) {
        self.bg_shells = self.active_tab().engine.bg_shells_meta.list().await;
    }

    async fn handle_tasks_panel_key(&mut self, code: KeyCode) {
        let len = self.bg_shells.len();
        match code {
            KeyCode::Esc => self.tasks_panel = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.tasks_panel {
                    p.prev(len);
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.tasks_panel {
                    p.next(len);
                }
            }
            KeyCode::Char('k') => {
                let idx = self.tasks_panel.as_ref().map(|p| p.selected()).unwrap_or(0);
                if let Some(shell) = self.bg_shells.get(idx).cloned() {
                    let engine = self.active_tab().engine.clone();
                    match engine.kill_shell(&shell.shell_id).await {
                        Ok(()) => {
                            self.toast = Some(Toast::info(format!(
                                "{} {}",
                                crate::tr("killed"),
                                shell.shell_id
                            )))
                        }
                        Err(e) => self.toast = Some(Toast::error(e.to_string())),
                    }
                    self.refresh_bg_shells().await;
                }
            }
            _ => {}
        }
    }

    fn refresh_subagents(&mut self) {
        self.subagents = self.active_tab().engine.subagents.snapshot();
        // Newest-first so new sub-agents appear at the top of the list.
        self.subagents.reverse();
    }

    /// Whether any cached sub-agent still qualifies for a status-HUD row —
    /// running, or finished inside the recency window.
    fn has_hud_subagent_rows(&self) -> bool {
        let now = now_secs();
        self.subagents
            .iter()
            .any(|agent| crate::ui::hud::is_hud_visible(agent, now))
    }

    /// Reload the agent-definition model map on a slow TTL, and only while a
    /// sub-agent could use it — the load reads every agent-definition file.
    fn refresh_agent_models(&mut self) {
        const TTL: Duration = Duration::from_secs(30);
        if self.subagents.is_empty() || self.agent_models_at.is_some_and(|t| t.elapsed() < TTL) {
            return;
        }
        self.agent_models_at = Some(std::time::Instant::now());
        self.agent_models = zode_core::agents::load_agent_defs(&self.active_tab().engine.cwd)
            .into_iter()
            .filter_map(|def| def.model.map(|model| (def.name, model)))
            .collect();
    }

    /// The status HUD's sub-agent rows for this frame, with the count that did
    /// not fit. `None` when the HUD is suppressed (terminal too short).
    fn hud_subagent_rows(
        &self,
        terminal_height: u16,
    ) -> Option<(Vec<crate::ui::hud::SubAgentRow>, usize)> {
        if terminal_height < crate::ui::hud::MIN_HEIGHT_FOR_HUD {
            return None;
        }
        let tab = &self.tabs[self.active];
        Some(crate::ui::hud::subagent_rows(
            &self.subagents,
            now_secs(),
            &self.agent_models,
            &tab.engine.model,
        ))
    }

    /// Refresh the active tab's sidebar section data on a slow cadence: the
    /// MCP connection snapshot (sync, cheap) and a spawned git working-tree
    /// poll (subprocess — never run on the UI loop, one in flight per tab).
    fn refresh_sidebar_sections(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        const INTERVAL: Duration = Duration::from_secs(2);
        if self
            .last_sidebar_poll
            .is_some_and(|t| t.elapsed() < INTERVAL)
        {
            return;
        }
        self.last_sidebar_poll = Some(std::time::Instant::now());
        let tab = &mut self.tabs[self.active];
        // MCP/LSP snapshots and the instruction-file count are in-memory or
        // existence-check cheap, and the status HUD shows them with the sidebar
        // closed — so they are polled regardless of sidebar visibility.
        tab.mcp_status = tab.engine.mcp_status();
        tab.lsp_status = tab.engine.lsp_status();
        tab.instruction_files = zode_core::instructions::instruction_paths(&tab.engine.cwd).len();
        // The git working-tree poll spawns a subprocess — only pay for it when
        // the sidebar that renders it is actually on screen.
        if !should_show_sidebar(self.tabs.len(), self.sidebar_visibility) {
            return;
        }
        let tab = &mut self.tabs[self.active];
        if tab.git_poll_inflight {
            return;
        }
        tab.git_poll_inflight = true;
        let (tab_id, cwd) = (tab.id, tab.engine.cwd.clone());
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let files = zode_core::git_stat::git_modified_files(&cwd).await;
            let _ = tx.send(AppEvent::GitStatDone { tab_id, files });
        });
    }

    /// Whether anything is drawn on top of the base layout this frame —
    /// modals, panels, pickers, help, toast. Drives the close-repaint above.
    fn any_overlay_open(&self) -> bool {
        self.active_dialog.is_some()
            || self.active_question.is_some()
            || self.settings.is_some()
            || self.connect.is_some()
            || self.plugin_picker.is_some()
            || self.browser_panel.is_some()
            || self.team_panel.is_some()
            || self.agents_dialog.is_some()
            || self.workflows_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.session_picker.is_some()
            || self.tasks_panel.is_some()
            || self.subagents_panel.is_some()
            || self.files_panel.is_some()
            || self.show_help
        // NB: toasts are deliberately NOT overlays here. They're small, they
        // expire on their own, and the ratatui diff restores the cells under
        // them — counting them would full-clear the terminal on every expiry
        // (a visible flash a few seconds after every copy/notification).
    }

    /// Left-click on a collapsible sidebar section header toggles its fold;
    /// clicking the modified-files "…+k more" row opens the full-list overlay.
    fn try_sidebar_header_click(
        &mut self,
        mouse: &MouseEvent,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return false;
        }
        let Some(area) = self.sidebar_area else {
            return false;
        };
        if mouse.column < area.x || mouse.column >= area.x + area.width {
            return false;
        }
        if Some(mouse.row) == self.sidebar_hits.mcp_header_row {
            self.mcp_section_collapsed = !self.mcp_section_collapsed;
            return true;
        }
        if Some(mouse.row) == self.sidebar_hits.lsp_header_row {
            self.lsp_section_collapsed = !self.lsp_section_collapsed;
            return true;
        }
        if Some(mouse.row) == self.sidebar_hits.files_header_row {
            self.files_section_collapsed = !self.files_section_collapsed;
            return true;
        }
        if Some(mouse.row) == self.sidebar_hits.files_more_row {
            self.files_panel = Some(crate::ui::dialog::files_panel::FilesPanel::new());
            return true;
        }
        if Some(mouse.row) == self.sidebar_hits.todo_header_row {
            self.todo_section_collapsed = !self.todo_section_collapsed;
            return true;
        }
        // A click on a row's `×` closes that tab (same as Ctrl+W on it);
        // anywhere else on the row focuses it (same as the keyboard switch).
        if let Some(i) = self.sidebar_hits.tab_close_at(mouse.row, mouse.column) {
            if i < self.tabs.len() {
                self.active = i;
                // close_active_tab also invalidates sidebar_hits so a second
                // buffered click can't act on the stale row→tab mapping.
                self.close_active_tab_with_events(agent_tx);
            }
            return true;
        }
        if let Some(i) = self.sidebar_hits.tab_index_at(mouse.row) {
            if i < self.tabs.len() && i != self.active {
                self.active = i;
                self.dismiss_input_popups();
            }
            return true;
        }
        false
    }

    /// The active tab's cached git-stat list length (0 while unknown).
    fn active_git_file_count(&self) -> usize {
        self.active_tab()
            .git_files
            .as_ref()
            .map(|f| f.len())
            .unwrap_or(0)
    }

    fn handle_files_panel_key(&mut self, code: KeyCode) {
        let total = self.active_git_file_count();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.files_panel = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.files_panel {
                    p.scroll_up(1);
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.files_panel {
                    p.scroll_down(1, total);
                }
            }
            KeyCode::PageUp => {
                if let Some(p) = &mut self.files_panel {
                    p.scroll_up(10);
                }
            }
            KeyCode::PageDown => {
                if let Some(p) = &mut self.files_panel {
                    p.scroll_down(10, total);
                }
            }
            _ => {}
        }
    }

    fn open_subagents_panel(&mut self) {
        self.refresh_subagents();
        self.subagents_panel = Some(crate::ui::dialog::subagents::SubAgentsPanel::new());
    }

    fn handle_subagents_panel_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.subagents_panel = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.subagents_panel {
                    p.select_prev(&self.subagents);
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.subagents_panel {
                    p.select_next(&self.subagents);
                }
            }
            KeyCode::PageUp => {
                if let Some(p) = &mut self.subagents_panel {
                    p.scroll_up();
                }
            }
            KeyCode::PageDown => {
                if let Some(p) = &mut self.subagents_panel {
                    p.scroll_down();
                }
            }
            _ => {}
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        // Seed the autocomplete with the initial tab's agents/skills/MCP tools.
        self.refresh_dynamic_commands();
        // selection_mode == effective mouseCapture (set once in `new`).
        let mut terminal = setup_terminal(self.selection_mode)?;
        let result = self.event_loop(&mut terminal).await;
        restore_terminal(&mut terminal)?;
        self.print_resume_hint();
        result
    }

    /// Run only the extension task/agent event pump, without taking over a
    /// terminal. Chrome starts this mode through Native Messaging when the
    /// side panel is opened while no regular zode process is available.
    pub async fn run_extension_daemon(
        mut self,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> std::io::Result<()> {
        self.refresh_dynamic_commands();
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                inbound = extension_tasks::recv_extension_task(&mut self.extension_task_rx) => {
                    if let Some(inbound) = inbound {
                        self.dispatch_extension_inbound(inbound, &agent_tx);
                    } else {
                        self.extension_task_rx = None;
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_runtime_event(app_ev, &agent_tx);
                    let mut drained = 0;
                    while drained < AGENT_COALESCE_CAP {
                        match agent_rx.try_recv() {
                            Ok(event) => {
                                self.handle_runtime_event(event, &agent_tx);
                                drained += 1;
                            }
                            Err(_) => break,
                        }
                    }
                    self.dispatch_extension_completions(&agent_tx);
                    self.maybe_auto_compact(&agent_tx);
                    self.maybe_apply_learned_profile(&agent_tx);
                    self.dispatch_queued_input(&agent_tx).await;
                }
                Some(request) = self.approval_rx.next() => {
                    self.route_approval_request(request);
                }
                Some(request) = self.question_rx.next() => {
                    // The task side panel does not expose the TUI's question
                    // picker yet. Dismiss instead of leaving the tool hung in
                    // a daemon with no terminal to answer it.
                    let _ = request.respond(None);
                }
                _ = ticker.tick() => {
                    self.cleanup_extension_attachments_at(std::time::Instant::now());
                    self.maybe_notice_self_update();
                }
                _ = &mut shutdown => break,
            }
        }
        Ok(())
    }

    pub async fn ensure_extension_bridge_listening(
        &self,
    ) -> Result<u16, zode_core::browser::BrowserError> {
        self.extension_browser.ensure_bridge_listening().await
    }

    /// On exit, print how to continue this session (like opencode/codex). Only
    /// for sessions that were actually used (a real title or some history).
    fn print_resume_hint(&self) {
        let tab = self.active_tab();
        let used = tab.titled || !tab.chat.messages().is_empty();
        if tab.session_id.is_empty() || !used {
            return;
        }
        let title = if tab.title.is_empty() {
            "untitled"
        } else {
            tab.title.as_str()
        };
        println!();
        println!("  {}   {title}", crate::tr("Session"));
        println!(
            "  {}  zode --resume {}",
            crate::tr("Continue"),
            tab.session_id
        );
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> std::io::Result<()> {
        let mut term_events = EventStream::new();
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        // Skip missed ticks instead of bursting to catch up: after a long
        // synchronous handler the ticker would otherwise fire a storm of
        // back-to-back ticks (each a redraw).
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Global-Esc fire channel from the desktop automation event tap; the
        // TUI is its single owner (None if some other loop already claimed it).
        let mut esc_fire = zode_core::desktop::esc_watch::take_receiver();

        // Set once the user asks to force-quit during the graceful drain (a
        // second Ctrl+C). Leases stay retained, so recovery still handles them
        // — this only abandons the wait, it never releases running work.
        let mut force_quit = false;

        loop {
            // Full repaint when an overlay just closed (or Ctrl+L asked): see
            // `overlay_was_open` — repairs cells the terminal lost under it.
            let overlay_open = self.any_overlay_open();
            let full_clear =
                std::mem::take(&mut self.force_redraw) || (self.overlay_was_open && !overlay_open);
            self.overlay_was_open = overlay_open;
            // An idle tick (nothing animatable changed) skips the redraw — a
            // fully idle app shouldn't rebuild the widget tree 10×/second.
            // A pending full clear always draws: clearing without repainting
            // would leave the screen blank until the next event.
            if std::mem::take(&mut self.skip_next_draw) && !full_clear {
                // still fell through the tick handler's state refresh below
            } else {
                // Bracket erase + repaint in a synchronized update (CSI ?2026)
                // so the terminal presents them as one atomic frame — without
                // it, the blank screen between `clear()` and the repaint shows
                // as a full-screen flash, and a scroll's whole-transcript diff
                // can tear mid-write. Unsupporting terminals ignore the marks.
                let mut out = std::io::stdout();
                let _ = out.execute(BeginSynchronizedUpdate);
                let drawn = (|| {
                    if full_clear {
                        terminal.clear()?;
                    }
                    terminal.draw(|f| self.draw(f)).map(|_| ())
                })();
                let _ = out.execute(EndSynchronizedUpdate);
                drawn?;
            }
            if self.should_quit {
                // Queued schedule occurrences never started, so a graceful
                // exit can safely release their exact active tokens. Running
                // scheduler turns enter an owned finalizer and keep the event
                // loop alive until worker quiescence and durable persistence
                // complete; crash recovery is reserved for a real process
                // loss, not a normal Ctrl-D, `/exit`, or last-tab close.
                if !self.shutdown_cleanup_started {
                    self.shutdown_cleanup_started = true;
                    self.reject_pending_ui_requests_for_shutdown();
                    self.begin_extension_shutdown();
                }
                self.release_all_pending_schedule_leases();
                self.begin_scheduler_shutdown(&agent_tx);
                self.retry_pending_schedule_finalizers(std::time::Instant::now());
                if self.scheduler_shutdown_pending() && !force_quit {
                    // Keep the loop alive until worker quiescence + durable
                    // persistence, but DON'T freeze the screen: surface a
                    // draining notice (once) and keep redrawing so the user
                    // isn't staring at a dead UI, and can press Ctrl+C to
                    // force-quit. Leases are retained either way.
                    if self.toast.is_none() {
                        self.toast = Some(Toast::info(
                            crate::tr("finishing background work… (Ctrl+C to force quit)")
                                .to_string(),
                        ));
                    }
                } else {
                    // Sweep any clipboard preview temp files still held by any tab.
                    let temps: Vec<std::path::PathBuf> = self
                        .tabs
                        .iter()
                        .flat_map(|t| t.pending_images.iter().map(|i| i.path.clone()))
                        .collect();
                    for path in &temps {
                        cleanup_clipboard_temp(&mut self.clipboard_temps, path);
                    }
                    break;
                }
            }

            // Keep Tokio's default fair selection. Extension worker results
            // validate the bridge's current active connection synchronously,
            // so correctness no longer depends on consuming Disconnected first.
            tokio::select! {
                inbound = extension_tasks::recv_extension_task(&mut self.extension_task_rx) => {
                    if let Some(inbound) = inbound {
                        self.dispatch_extension_inbound(inbound, &agent_tx);
                    } else {
                        // A closed receiver is immediately ready forever. Drop
                        // it so subsequent iterations park on `pending()`.
                        self.extension_task_rx = None;
                    }
                }
                Some(()) = recv_desktop_esc(&mut esc_fire) => {
                    // Global Esc during desktop automation stops the whole run,
                    // not just the current desktop action (same path as TUI Esc).
                    let stopped = self.interrupt_all_running_turns();
                    zode_core::desktop::esc_watch::disarm();
                    zode_core::desktop::overlay::hide_global();
                    if stopped > 0 {
                        self.toast = Some(Toast::info(
                            crate::tr("desktop automation stopped (Esc)").to_string(),
                        ));
                    }
                }
                maybe_ev = term_events.next() => {
                    if let Some(Ok(ev)) = maybe_ev {
                        if self.should_quit {
                            // The app is draining. Ignore all input except a
                            // force-quit (Ctrl+C), which abandons the wait on
                            // the next loop pass without releasing retained
                            // leases.
                            if is_force_quit_event(&ev) {
                                force_quit = true;
                            }
                            continue;
                        }
                        let mut burst = vec![ev];
                        // Coalesce the rest of an input burst before redrawing.
                        // A trackpad/wheel momentum flick floods scroll events;
                        // handling them all here (then one draw at the top of
                        // the loop) stops over-scrolling at the top/bottom from
                        // backing up into a multi-second redraw storm.
                        let mut ready = drain_ready_events(&mut term_events, INPUT_COALESCE_CAP);
                        let first_chunk_full = ready.len() == INPUT_COALESCE_CAP;
                        burst.append(&mut ready);
                        if cfg!(target_os = "windows") {
                            extend_windows_text_burst(
                                &mut term_events,
                                &mut burst,
                                first_chunk_full,
                            );
                        }
                        let clipboard = if cfg!(windows) && windows_burst_needs_clipboard(&burst) {
                            zode_core::clipboard::read_from_clipboard_with_timeout(
                                WINDOWS_CLIPBOARD_READ_TIMEOUT,
                            )
                            .await
                            .ok()
                        } else {
                            None
                        };
                        self.handle_term_burst(
                            burst,
                            &agent_tx,
                            cfg!(windows),
                            clipboard.as_deref(),
                        )
                        .await;
                        // Switching to a tab that has queued input (and is now
                        // idle) flushes it here, not just on its own turn-done.
                        if !self.should_quit {
                            self.dispatch_queued_input(&agent_tx).await;
                        }
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_runtime_event(app_ev, &agent_tx);
                    // Coalesce a burst of streaming events (text deltas, tool
                    // updates) into ONE redraw. Providers stream tokens in
                    // bursts; handling each in its own loop pass forces a full
                    // transcript re-render per token, which stutters on long
                    // conversations. Drain everything already queued (capped),
                    // then fall through to the single draw at the loop top.
                    let mut drained = 0;
                    while drained < AGENT_COALESCE_CAP {
                        match agent_rx.try_recv() {
                            Ok(ev) => {
                                self.handle_runtime_event(ev, &agent_tx);
                                drained += 1;
                            }
                            Err(_) => break,
                        }
                    }
                    if !self.should_quit {
                        self.dispatch_extension_completions(&agent_tx);
                        // A turn may have just finished — if it left the context at
                        // the auto-compact threshold, compact before anything new is
                        // sent, then flush any queued input.
                        self.maybe_auto_compact(&agent_tx);
                        self.maybe_apply_learned_profile(&agent_tx);
                        self.dispatch_queued_input(&agent_tx).await;
                        // A turn going idle here may have been on a background
                        // tab — its own queued scheduler prompt (if any) needs
                        // the same drain `dispatch_queued_input` gives the
                        // active tab.
                        self.dispatch_scheduler_queued(&agent_tx).await;
                    }
                }
                Some(req) = self.approval_rx.next() => {
                    self.route_approval_request(req);
                }
                Some(req) = self.question_rx.next() => {
                    self.route_question_request(req);
                }
                _ = ticker.tick() => {
                    self.status.tick();
                    self.maybe_notice_self_update();
                    let ui_data_revision = self.ui_extensions.data_revision();
                    let ui_data_changed = ui_data_revision != self.ui_data_revision;
                    self.ui_data_revision = ui_data_revision;
                    self.retry_pending_schedule_finalizers(std::time::Instant::now());
                    if !self.should_quit {
                        self.poll_queued_watchdog();
                        self.poll_watchdog(&agent_tx);
                        self.dispatch_watchdog_retries();
                        self.poll_scheduler();
                        self.dispatch_scheduler_queued(&agent_tx).await;
                    }
                    self.cleanup_extension_attachments_at(std::time::Instant::now());
                    let had_toast = self.toast.is_some();
                    if let Some(t) = &mut self.toast {
                        if t.tick() {
                            self.toast = None;
                        }
                    }
                    // Todo/sub-agent state only changes while a turn runs (or
                    // its panel is open) — skip the per-tab lock+clone traffic
                    // when fully idle rather than paying it 10×/second.
                    let any_busy = self.tabs.iter().any(|t| t.is_busy());
                    // Background shells feed the status HUD's mode row, so the
                    // snapshot is refreshed on a slow cadence even with the
                    // tasks panel closed.
                    const HUD_POLL: Duration = Duration::from_secs(2);
                    let hud_due = self.last_hud_poll.is_none_or(|t| t.elapsed() >= HUD_POLL);
                    if hud_due {
                        self.last_hud_poll = Some(std::time::Instant::now());
                    }
                    if self.tasks_panel.is_some() || hud_due {
                        self.refresh_bg_shells().await;
                    }
                    // Sub-agent HUD rows outlive the turn that spawned them
                    // (finished agents linger briefly), so keep polling while
                    // any cached row is still displayable.
                    if any_busy
                        || self.subagents_panel.is_some()
                        || self.has_hud_subagent_rows()
                    {
                        self.refresh_subagents();
                    }
                    self.refresh_agent_models();
                    if any_busy {
                        for i in 0..self.tabs.len() {
                            let engine = self.tabs[i].engine.clone();
                            let snap = engine.todo_state.snapshot().await;
                            self.tabs[i].todos = snap;
                        }
                    }
                    // Throttled sidebar data poll: git working-tree stats
                    // (spawned off-loop) + MCP connection state, active tab.
                    self.refresh_sidebar_sections(&agent_tx);
                    // Skip the redraw for a fully-idle tick — nothing that
                    // animates (spinner, toast countdown) is active, so the
                    // frame would be identical. Any real event path leaves
                    // `skip_next_draw` false and redraws normally.
                    let animating =
                        any_busy || self.toast.is_some() || had_toast || ui_data_changed;
                    self.skip_next_draw = !animating && !overlay_open;
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, f: &mut ratatui::Frame) {
        // Clone the theme so the &mut self field borrows below (autocomplete /
        // settings hold ListState) don't conflict with an immutable theme
        // borrow. Cheap relative to a frame at 10fps.
        let theme = self.theme.clone();
        let area = f.area();
        // Remember the painted frame: mouse hit-testing must resolve against
        // what the user actually saw, and headless/test environments have no
        // tty for a live `terminal::size()` query.
        self.last_frame_area = area;
        // Mirror the active tab's live mode/token counts into the status bar.
        {
            let tab = &self.tabs[self.active];
            self.status.mode = tab.mode;
            self.status.input_tokens = tab.input_tokens;
            self.status.output_tokens = tab.output_tokens;
            // Context-window occupancy: last prompt size vs the active model's window.
            self.status.context_tokens = tab.context_tokens;
            self.status.context_window = tab.engine.model_max_tokens;
            self.status.model = tab.engine.model.clone();
            // Plan mode is per-tab, so the badge always reflects the active tab.
            self.status.plan_mode = tab.plan_mode;
            self.status.yolo = matches!(tab.extension_access, zode_core::ToolAccessMode::Auto);
            self.status.selection_mode = self.selection_mode;
        }
        // Active provider group (for the `model(provider)` label), from the live
        // template — current across startup, model switch, and connect.
        self.status.provider = self
            .template
            .with_model(self.tabs[self.active].engine.model.clone())
            .active_provider_name()
            .unwrap_or_default();
        // Sandbox remains global; approval mode is task-local and was synced
        // from the active tab above.
        let sandbox = self.template.sandbox();
        self.status.sandbox = sandbox.is_some();
        self.status.sandbox_read_only = sandbox
            .map(|c| c.mode() == zode_core::sandbox::SandboxMode::ReadOnly)
            .unwrap_or(false);
        self.status.sandbox_network = sandbox.map(|c| c.allow_network()).unwrap_or(false);
        let active_title = self.tabs[self.active].title.clone();
        let active_model = self.tabs[self.active].engine.model.clone();
        let active_cwd = self.tabs[self.active].engine.cwd.clone();
        let active_busy = self.tabs[self.active].is_busy();
        let active_cost = self.tabs[self.active].cost_label.clone();
        let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
        // UI plugin extensions: the render context below is allocation-heavy
        // (tool lists, git files, service maps), so skip all of it when no
        // renderer is installed — the overwhelmingly common case.
        let (status_extensions, sidebar_extensions) = if self.ui_extensions.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let active_session_id = self.tabs[self.active].session_id.clone();
            let mut available_tools = self.tabs[self.active]
                .engine
                .tools
                .names()
                .map(str::to_string)
                .collect::<Vec<_>>();
            available_tools.sort();
            let mut active_tools = self.tabs[self.active]
                .active_tool_api_names
                .values()
                .cloned()
                .collect::<Vec<_>>();
            active_tools.sort();
            active_tools.dedup();
            let recent_tools = self.tabs[self.active]
                .recent_tools
                .iter()
                .map(|activity| {
                    serde_json::json!({
                        "name": activity.name,
                        "status": activity.status,
                        "durationMs": activity.duration_ms
                    })
                })
                .collect::<Vec<_>>();
            let workspace_files = self.tabs[self.active]
                .git_files
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .take(50)
                .map(|file| {
                    serde_json::json!({
                        "path": file.path,
                        "added": file.added,
                        "removed": file.removed
                    })
                })
                .collect::<Vec<_>>();
            let todo_statuses = self.tabs[self.active]
                .todos
                .iter()
                .map(|todo| format!("{:?}", todo.status).to_ascii_lowercase())
                .collect::<Vec<_>>();
            let mcp_services = self.tabs[self.active]
                .mcp_status
                .iter()
                .map(|(name, connected)| serde_json::json!({"name": name, "connected": connected}))
                .collect::<Vec<_>>();
            let lsp_services = self.tabs[self.active]
            .lsp_status
            .iter()
            .map(
                |(language, running)| serde_json::json!({"language": language, "running": running}),
            )
            .collect::<Vec<_>>();
            let subagent_states = self
                .subagents
                .iter()
                .map(|agent| {
                    serde_json::json!({
                        "type": agent.agent_type,
                        "status": format!("{:?}", agent.status).to_ascii_lowercase()
                    })
                })
                .collect::<Vec<_>>();

            let context_percent = (self.status.context_window > 0).then(|| {
                (self.status.context_tokens as u64 * 100 / self.status.context_window as u64)
                    .min(100)
            });
            let ui_context = serde_json::json!({
                "apiVersion": 1,
                "terminal": {
                    "width": area.width,
                    "height": area.height
                },
                "session": {
                    "id": active_session_id,
                    "title": &active_title,
                    "cwd": active_cwd.display().to_string(),
                    "busy": active_busy
                },
                "model": {
                    "id": &active_model,
                    "provider": &self.status.provider
                },
                "status": {
                    "mode": format!("{:?}", self.status.mode).to_ascii_lowercase(),
                    "planMode": self.status.plan_mode,
                    "selectionMode": self.status.selection_mode,
                    "yolo": self.status.yolo,
                    "sandbox": {
                        "enabled": self.status.sandbox,
                        "readOnly": self.status.sandbox_read_only,
                        "network": self.status.sandbox_network
                    }
                },
                "tokens": {
                    "input": self.status.input_tokens,
                    "output": self.status.output_tokens
                },
                "context": {
                    "used": self.status.context_tokens,
                    "window": self.status.context_window,
                    "usedPercent": context_percent
                },
                "app": {
                    "version": env!("CARGO_PKG_VERSION"),
                    "effort": self.template.effort().unwrap_or("medium")
                },
                "tabs": {
                    "active": self.active + 1,
                    "count": self.tabs.len()
                },
                "workspace": {
                    "modifiedFiles": workspace_files
                },
                "tools": {
                    "available": available_tools,
                    "active": active_tools,
                    "recent": recent_tools
                },
                "tasks": {
                    "todoStatuses": todo_statuses,
                    "subagents": subagent_states,
                    "goal": {
                        "active": self.tabs[self.active].goal_loop_active,
                        "turn": self.tabs[self.active].goal_loop_iter
                    }
                },
                "services": {
                    "mcp": mcp_services,
                    "lsp": lsp_services
                }
            });
            let status = self.ui_extensions.status_line(ui_context.clone()).to_vec();
            let sidebar = if show_sidebar {
                self.ui_extensions.sidebar(ui_context).to_vec()
            } else {
                Vec::new()
            };
            (status, sidebar)
        };
        // Adaptive status HUD: extra rows stacked above the main status line.
        // `hud_subagent_rows` returns None when the terminal is too short.
        let hud_subagents = self.hud_subagent_rows(area.height);
        let hud_input = hud_subagents
            .as_ref()
            .map(|(rows, overflow)| crate::ui::hud::HudInput {
                tally: self.tabs[self.active].hud_tally(),
                subagents: rows,
                subagent_overflow: *overflow,
                mode: self.tabs[self.active].extension_access,
                infra: crate::ui::hud::InfraCounts {
                    shells: self.bg_shells.iter().filter(|s| !s.killed).count(),
                    mcps: self.tabs[self.active]
                        .mcp_status
                        .iter()
                        .filter(|(_, connected)| *connected)
                        .count(),
                    instruction_files: self.tabs[self.active].instruction_files,
                },
            });
        // Render the rows to owned lines right away so the HUD's borrow of the
        // active tab ends before the chat view takes it mutably below.
        let (hud_rows, hud_lines) = match &hud_input {
            Some(input) => (
                crate::ui::hud::row_count(input, area.height),
                crate::ui::hud::hud_lines(input, &theme),
            ),
            None => (0, Vec::new()),
        };
        let plugin_rows = u16::from(!status_extensions.is_empty());
        self.status_rows = hud_rows.saturating_add(1).saturating_add(plugin_rows);
        let areas = split_main(area, show_sidebar, self.status_rows);
        if let Some(header) = areas.header {
            render_header(
                f,
                header,
                &theme,
                HeaderInfo {
                    theme_name: &theme.name,
                    model: &active_model,
                    cwd: &active_cwd,
                    tab_title: &active_title,
                    busy: active_busy,
                    effort: self.template.effort().unwrap_or("medium"),
                },
            );
        }
        if let Some(tab_area) = areas.tabs {
            let mode = crate::tr(mode_label(self.status.mode));
            let hits = render_sidebar(
                f,
                tab_area,
                &self.tabs,
                self.active,
                SidebarInfo {
                    session_title: &active_title,
                    theme_name: &theme.name,
                    model: &active_model,
                    cwd: &active_cwd,
                    mode,
                    input_tokens: self.status.input_tokens,
                    output_tokens: self.status.output_tokens,
                    cost_label: &active_cost,
                    yolo: self.status.yolo,
                    sandbox: self.status.sandbox,
                    todos: &self.tabs[self.active].todos,
                    busy: active_busy,
                    todos_collapsed: self.todo_section_collapsed,
                    subagents: &self.subagents,
                    goal: self.tabs[self.active].goal_text.as_deref(),
                    goal_elapsed: self.tabs[self.active]
                        .goal_started_at
                        .map(|t| format_elapsed(t.elapsed())),
                    mcp_servers: &self.tabs[self.active].mcp_status,
                    mcp_collapsed: self.mcp_section_collapsed,
                    lsp_servers: &self.tabs[self.active].lsp_status,
                    lsp_collapsed: self.lsp_section_collapsed,
                    git_files: self.tabs[self.active].git_files.as_deref().unwrap_or(&[]),
                    files_collapsed: self.files_section_collapsed,
                    version: env!("CARGO_PKG_VERSION"),
                },
                &sidebar_extensions,
                &theme,
            );
            self.sidebar_hits = hits;
            self.sidebar_area = Some(tab_area);
        } else {
            self.sidebar_hits = crate::ui::tabs::SidebarHits::default();
            self.sidebar_area = None;
        }

        let chat_meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: &active_model,
            cwd: &active_cwd,
        };
        // A pending permission prompt docks INLINE, between the conversation
        // and the input — carve its rows off the bottom of the chat area so it
        // never covers the conversation (Claude-Code-style). On a terminal too
        // short to dock it, `perm_inline` stays None and we fall back to a
        // centered popup below.
        let mut chat_area = areas.chat;
        let mut perm_inline: Option<Rect> = None;
        if let Some(dialog) = &self.active_dialog {
            let want = dialog.desired_height(chat_area.width, &theme);
            // Keep at least 3 rows of conversation visible above the card.
            if chat_area.height > want + 3 {
                let strip = Rect::new(
                    chat_area.x,
                    chat_area.y + chat_area.height - want,
                    chat_area.width,
                    want,
                );
                chat_area.height -= want;
                perm_inline = Some(strip);
            }
        }
        let (show_thinking, show_tool_details) = (self.show_thinking, self.show_tool_details);
        let selection = self.active_selection;
        let active_chat = &mut self.tabs[self.active].chat;
        active_chat.set_display_prefs(show_thinking, show_tool_details);
        active_chat.render_with_selection(f, chat_area, &theme, chat_meta, selection);
        if let (Some(strip), Some(dialog)) = (perm_inline, self.active_dialog.as_mut()) {
            dialog.render_inline(f, strip, &theme);
        }
        let mut input_area: Rect = areas.composer;
        if !self.tabs[self.active].pending_images.is_empty() && input_area.height > 2 {
            let chips_area = Rect::new(input_area.x, input_area.y, input_area.width, 1);
            let hits = render_pending_image_chips(
                f,
                chips_area,
                &self.tabs[self.active].pending_images,
                self.selected_image,
                &theme,
            );
            // Remember where each chip sits so a (Cmd/Ctrl)+click can open it.
            self.image_chip_row = chips_area.y;
            self.image_chip_hits = hits;
            input_area.y = input_area.y.saturating_add(1);
            input_area.height = input_area.height.saturating_sub(1);
        } else {
            self.image_chip_hits.clear();
        }
        let input_text = self.input.text();
        let completion_placeholder = self
            .completion_hint
            .as_ref()
            .and_then(|hint| (input_text == hint.prefix).then_some(hint.placeholder.as_str()));
        self.input.render_with_selection(
            f,
            input_area,
            &theme,
            self.status.mode,
            completion_placeholder,
            self.active_input_selection,
        );
        // The HUD occupies the top of the status region; the classic status
        // line (plus any plugin row) keeps the bottom.
        let mut bar_area = areas.status;
        let hud_h = hud_rows.min(bar_area.height.saturating_sub(1));
        if hud_h > 0 {
            let hud_area = Rect::new(bar_area.x, bar_area.y, bar_area.width, hud_h);
            crate::ui::hud::render_lines(f, hud_area, &theme, hud_lines);
            bar_area.y = bar_area.y.saturating_add(hud_h);
            bar_area.height = bar_area.height.saturating_sub(hud_h);
        }
        self.status.render(f, bar_area, &theme, &status_extensions);
        // Autocomplete popup floats above the input row.
        self.autocomplete.render(f, input_area, &theme);
        // @-mention popup occupies the same band; the two are mutually exclusive
        // (autocomplete only activates on a leading `/`).
        if let Some(mention) = &mut self.active_mention {
            mention.render(f, input_area, &theme);
        }
        // Overlays, lowest first. The permission dialog renders LAST (above
        // settings/help) because it captures input with the highest
        // precedence — it must never be hidden behind another overlay.
        if let Some(settings) = &mut self.settings {
            settings.render(f, area, &theme);
        }
        if let Some(connect) = &self.connect {
            connect.render(f, area, &theme);
        }
        if let Some(picker) = &self.plugin_picker {
            picker.render(f, area, &theme);
        }
        if let Some(dialog) = &self.agents_dialog {
            dialog.render(f, area, &theme);
        }
        if let Some(dialog) = &self.workflows_dialog {
            dialog.render(f, area, &theme);
        }
        if let Some(dialog) = &self.mcp_dialog {
            dialog.render(f, area, &theme);
        }
        if let Some(picker) = &mut self.session_picker {
            picker.render(f, area, &theme);
        }
        if self.tasks_panel.is_some() {
            let mut turns: Vec<String> = self
                .tabs
                .iter()
                .filter(|t| t.is_busy())
                .map(|t| format!("{}: running", t.title))
                .collect();
            turns.extend(self.watchdog_status_lines(std::time::Instant::now()));
            let now = now_secs();
            let shells = std::mem::take(&mut self.bg_shells);
            if let Some(panel) = &mut self.tasks_panel {
                panel.render(f, area, &shells, &turns, now, &theme);
            }
            self.bg_shells = shells;
        }
        // Sub-agents overlay. Use std::mem::take to move the cached Vec out of
        // self so panel (&mut self.subagents_panel) and the data aren't both
        // borrowed from self at the same time.
        if self.subagents_panel.is_some() {
            let now = now_secs();
            let agents = std::mem::take(&mut self.subagents);
            let session_model = self.tabs[self.active].engine.model.clone();
            let defs = std::mem::take(&mut self.agent_models);
            if let Some(panel) = &mut self.subagents_panel {
                let models = crate::ui::dialog::subagents::ModelLabels {
                    defs: &defs,
                    session: &session_model,
                };
                panel.render(f, area, &agents, now, models, &theme);
            }
            self.agent_models = defs;
            self.subagents = agents;
        }
        // Full modified-files overlay. Same take/restore dance so the panel
        // (&mut) and the active tab's cached list aren't both borrowed.
        if self.files_panel.is_some() {
            let files = std::mem::take(&mut self.tabs[self.active].git_files);
            if let Some(panel) = &mut self.files_panel {
                panel.render(f, area, files.as_deref().unwrap_or(&[]), &theme);
            }
            self.tabs[self.active].git_files = files;
        }
        if let Some(panel) = &mut self.browser_panel {
            panel.render(f, area, &theme);
        }
        if let Some(panel) = &mut self.team_panel {
            panel.render(f, area, &theme);
        }
        if self.show_help {
            crate::ui::help::render_help(f, area, &theme);
        }
        // Toast renders before the question modal so it can never cover it.
        if let Some(toast) = &self.toast {
            toast.render(f, area, &theme);
        }
        if let Some(q) = &mut self.active_question {
            q.render(f, area, &theme);
        }
        // The permission prompt normally renders INLINE above the input (see
        // `perm_inline` above) so it never blocks the view or the input box.
        // Only fall back to the centered popup when the terminal was too short
        // to dock it inline.
        if perm_inline.is_none() {
            if let Some(dialog) = &mut self.active_dialog {
                dialog.render(f, area, &theme);
            }
        }
    }

    async fn handle_term_burst(
        &mut self,
        events: Vec<CtEvent>,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
        windows_console: bool,
        clipboard_text: Option<&str>,
    ) {
        let mut segments = if windows_console {
            windows_paste_segments(&events, clipboard_text)
                .into_iter()
                .peekable()
        } else {
            Vec::new().into_iter().peekable()
        };
        let mut paste_end = 0;

        for (index, event) in events.into_iter().enumerate() {
            if self.should_quit {
                break;
            }

            if segments
                .peek()
                .is_some_and(|segment| segment.events.start == index)
            {
                let segment = segments.next().expect("peeked paste segment");
                paste_end = segment.events.end;
                self.handle_paste(&segment.text);
            }

            if index < paste_end {
                if matches!(
                    event,
                    CtEvent::Key(KeyEvent {
                        kind: crossterm::event::KeyEventKind::Release,
                        ..
                    })
                ) {
                    self.handle_term(event, agent_tx).await;
                }
                continue;
            }
            self.handle_term(event, agent_tx).await;
        }
    }

    async fn handle_term(&mut self, ev: CtEvent, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let key = match ev {
            CtEvent::Key(key) => key,
            CtEvent::Paste(text) => {
                self.handle_paste(&text);
                return;
            }
            CtEvent::Mouse(mouse) => {
                self.handle_mouse(mouse, agent_tx);
                return;
            }
            CtEvent::Resize(_, _) => {
                // The layout reflows on resize, so a held selection's screen
                // mapping (anchored to the pre-resize frame) is now stale — drop
                // it rather than risk copying the wrong region on the next chord.
                self.active_selection = None;
                self.active_input_selection = None;
                // Resizes can leave terminal-owned cells from the old layout
                // behind (most visibly in the right sidebar). Ask the event
                // loop to clear once before the next draw.
                self.force_redraw = true;
                return;
            }
            _ => return,
        };
        // Ignore key-release events (crossterm reports them on some terminals).
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }

        // 1. Permission prompt — NON-BLOCKING (modeled on Claude Code). The
        // prompt docks inline above the input and does NOT capture the
        // keyboard: the user can keep typing to queue a follow-up while a tool
        // waits for approval ("插队"). Only while the input is EMPTY do the
        // numbered options (1/2/3) and Esc answer it — so prose that starts
        // with a letter is never swallowed. Anything else falls through to the
        // normal input handling below (typing, Enter→queue, …).
        if self.active_dialog.is_some() && self.input.text().is_empty() {
            // Arrow keys move the highlight, Enter confirms it; 1/2/3 still pick
            // directly and Esc denies. (Only while the input is empty, so typing
            // a draft is never captured — the prompt stays non-blocking.)
            match key.code {
                KeyCode::Up | KeyCode::Left => {
                    if let Some(d) = &mut self.active_dialog {
                        d.select_prev();
                    }
                    return;
                }
                KeyCode::Down | KeyCode::Right => {
                    if let Some(d) = &mut self.active_dialog {
                        d.select_next();
                    }
                    return;
                }
                KeyCode::Enter => {
                    if let Some(approval) =
                        self.active_dialog.as_ref().map(|d| d.selected_approval())
                    {
                        self.answer_permission(approval);
                    }
                    return;
                }
                _ => {}
            }
            let decision = match key.code {
                KeyCode::Char(c) => crate::ui::dialog::permission::approval_for_key(c),
                KeyCode::Esc => Some(Approval::Deny),
                _ => None,
            };
            if let Some(approval) = decision {
                self.answer_permission(approval);
                return;
            }
        }

        // 1b. Question modal captures input until answered/dismissed.
        if let Some(q) = &mut self.active_question {
            if q.on_key(key.code) {
                self.active_question = None;
                if let Some(next) = self.pending_questions.pop_front() {
                    self.open_question(next);
                }
            }
            return;
        }

        // 2. Settings dialog captures input.
        if self.settings.is_some() {
            self.handle_settings_key(key.code, agent_tx).await;
            return;
        }

        // 2a. Connect dialog captures provider search and API key entry.
        if self.connect.is_some() {
            self.handle_connect_key(key.code, agent_tx).await;
            return;
        }

        // 2a2. Plugin picker captures toggle + filter input.
        if self.plugin_picker.is_some() {
            self.handle_plugin_key(key.code, agent_tx).await;
            return;
        }

        // 2a3. Agents manager captures list nav + create-form input.
        if self.agents_dialog.is_some() {
            self.handle_agents_dialog_key(key.code, key.modifiers, agent_tx)
                .await;
            return;
        }

        // 2a4. Workflows manager captures list nav + create-form input.
        if self.workflows_dialog.is_some() {
            self.handle_workflows_dialog_key(key.code, key.modifiers, agent_tx)
                .await;
            return;
        }

        // 2a5. MCP manager captures nav + space-toggle.
        if self.mcp_dialog.is_some() {
            self.handle_mcp_dialog_key(key.code, agent_tx).await;
            return;
        }

        // 2b. Session picker captures input (typing filters the list).
        if self.session_picker.is_some() {
            self.handle_picker_key(key.code, agent_tx).await;
            return;
        }

        // 2c. Tasks panel captures input.
        if self.tasks_panel.is_some() {
            self.handle_tasks_panel_key(key.code).await;
            return;
        }

        // 2d. Sub-agents panel captures input (sync handler, no .await needed).
        if self.subagents_panel.is_some() {
            self.handle_subagents_panel_key(key.code);
            return;
        }

        // 2e. Modified-files overlay captures input.
        if self.files_panel.is_some() {
            self.handle_files_panel_key(key.code);
            return;
        }

        // 2f. Browser panel captures nav + Enter (may await a plugin toggle).
        if self.browser_panel.is_some() {
            self.handle_browser_panel_key(key.code, agent_tx).await;
            return;
        }

        // 2f'. Team panel: read-only; scroll + Esc to close.
        if self.team_panel.is_some() {
            match key.code {
                KeyCode::Esc => self.team_panel = None,
                KeyCode::Up => {
                    if let Some(p) = &mut self.team_panel {
                        p.scroll_up();
                    }
                }
                KeyCode::Down => {
                    if let Some(p) = &mut self.team_panel {
                        p.scroll_down();
                    }
                }
                _ => {}
            }
            return;
        }

        // 3. Help overlay: Esc / F1 / q closes it.
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q')) {
                self.show_help = false;
            }
            return;
        }

        // Any key other than Esc disarms the two-Esc "clear draft" gesture, so
        // a single stray Esc never wipes a draft on its own.
        if key.code != KeyCode::Esc {
            self.esc_clear_armed = false;
        }

        // 3b. Pending-image chips: ↑ selects, ←/→/↑/↓ move, Backspace/Delete
        // removes, Enter (or Cmd/Ctrl+Enter) views, Esc/typing exit selection.
        if self.handle_image_chip_key(key) {
            return;
        }

        // 4. Global chords.
        match (key.code, key.modifiers) {
            // An EXPLICIT copy of the active selection on the platform copy chord
            // — Ctrl+C (or Cmd+C where the terminal delivers it). Selecting also
            // auto-copies on release (copy-on-select), so this chord is a
            // secondary path. Guarded by an active (non-empty) selection, so a
            // bare Ctrl+C with nothing selected still clears the draft /
            // interrupts / quits below.
            (KeyCode::Char('c'), m) if is_primary_mod(m) && self.has_active_selection() => {
                self.copy_active_selection();
                return;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                // Clear a prompt draft first; with an empty prompt, interrupt a
                // running turn or quit when idle.
                if !self.input.is_empty() {
                    self.input.take();
                    self.queued_edit_index = None;
                    self.reset_input_browse_state();
                    return;
                }
                if !self.interrupt_active_turn() {
                    self.should_quit = true;
                }
                return;
            }
            // Esc interrupts a running turn. An open autocomplete or @-mention
            // popup gets Esc first (to dismiss) — that's handled later, so only
            // steal Esc here when no popup is open and a turn is in flight.
            (KeyCode::Esc, _)
                if self.tabs[self.active].is_busy()
                    && !self.autocomplete.is_active()
                    && !self.mention_active() =>
            {
                self.interrupt_active_turn();
                return;
            }
            // Idle with a non-empty draft (and no popup to dismiss): two Escs
            // clear it. The first arms + hints; the second wipes the draft.
            (KeyCode::Esc, _)
                if !self.tabs[self.active].is_busy()
                    && !self.autocomplete.is_active()
                    && !self.mention_active()
                    && !self.input.is_empty() =>
            {
                if self.esc_clear_armed {
                    self.input.take();
                    self.queued_edit_index = None;
                    self.reset_input_browse_state();
                    self.esc_clear_armed = false;
                } else {
                    self.esc_clear_armed = true;
                    self.toast = Some(Toast::info(crate::tr("press Esc again to clear the input")));
                }
                return;
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return;
            }
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                // Ctrl+L REDRAWS the conversation from the persisted store
                // rather than wiping it to empty: it clears transient render
                // state and RECOVERS a view that has gone blank, without losing
                // any messages (the store is the source of truth). Use `/clear`
                // to actually discard the conversation.
                let tab = &mut self.tabs[self.active];
                let rebuilt = tab
                    .engine
                    .store
                    .lock()
                    .ok()
                    .map(|store| rebuild_chat_from_store(&store));
                if let Some(chat) = rebuilt {
                    tab.chat = chat;
                }
                // Also force a FULL terminal repaint: cells ratatui considers
                // unchanged (e.g. the sidebar rail) are never re-sent, so a
                // terminal that lost them (Warp glitches) shows gaps forever.
                self.force_redraw = true;
                return;
            }
            // Paste uses the documented Control chord; macOS Command/SUPER is
            // accepted by `is_primary_mod` as a compatibility alias.
            (KeyCode::Char('v'), m) if is_primary_mod(m) => {
                self.paste_from_clipboard();
                return;
            }
            // App chords are documented as Ctrl; `is_primary_mod` also accepts
            // macOS Command/SUPER where terminals deliver it.
            (KeyCode::Char('o'), m) if is_primary_mod(m) => {
                self.open_settings();
                return;
            }
            (KeyCode::Char('t'), m) if is_primary_mod(m) => {
                self.new_tab(agent_tx);
                return;
            }
            (KeyCode::Char('w'), m) if is_primary_mod(m) => {
                self.close_active_tab_with_events(agent_tx);
                return;
            }
            (KeyCode::Char('b'), m) if is_primary_mod(m) => {
                self.open_tasks_panel().await;
                return;
            }
            (KeyCode::Char('g'), m) if is_primary_mod(m) => {
                self.handle_sidebar_command("toggle");
                return;
            }
            // Fold toggle for tool/thinking blocks: expand all when any is
            // folded, fold all otherwise (a click on a block header toggles
            // just that block).
            (KeyCode::Char('e'), m) if is_primary_mod(m) => {
                self.tabs[self.active].chat.toggle_all_collapsed();
                return;
            }
            // Terminals report Shift+Tab either as BackTab (with or without
            // the Shift modifier) or as Tab+Shift. Both toggle the active
            // task between prompt-for-approval (ask) and auto-approve (yolo).
            (KeyCode::BackTab, KeyModifiers::NONE)
            | (KeyCode::BackTab, KeyModifiers::SHIFT)
            | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.toggle_yolo(agent_tx);
                return;
            }
            // Ctrl+1..9 jumps to a tab by position; macOS Command/SUPER is an
            // accepted alias where terminals deliver it.
            (KeyCode::Char(c), m) if is_primary_mod(m) && c.is_ascii_digit() && c != '0' => {
                let n = (c as u8 - b'1') as usize;
                self.switch_to(n);
                return;
            }
            (KeyCode::Tab, m) if is_primary_mod(m) => {
                self.cycle_tab();
                return;
            }
            (KeyCode::F(1), _) => {
                self.show_help = true;
                return;
            }
            (KeyCode::F(2), _) => {
                self.open_subagents_panel();
                return;
            }
            (KeyCode::PageUp, _) => {
                self.tabs[self.active].chat.scroll_up(5);
                return;
            }
            (KeyCode::PageDown, _) => {
                self.tabs[self.active].chat.scroll_down(5);
                return;
            }
            // End jumps to the latest output ("render to the bottom"); Home to
            // the start of the conversation.
            (KeyCode::End, _) => {
                self.tabs[self.active].chat.scroll_to_bottom();
                return;
            }
            (KeyCode::Home, _) => {
                self.tabs[self.active].chat.scroll_to_top();
                return;
            }
            _ => {}
        }

        if let Some(scroll) =
            chat_scroll_from_alt_scroll_key(key.code, key.modifiers, self.input.text().is_empty())
        {
            match scroll {
                ChatMouseScroll::Up(n) => self.tabs[self.active].chat.scroll_up(n),
                ChatMouseScroll::Down(n) => self.tabs[self.active].chat.scroll_down(n),
            }
            return;
        }

        // 4b. @-mention picker (cwd file / skill / MCP server). Intercepts nav
        // keys while a non-empty picker is open; selecting inserts the bare
        // reference behind a leading `@`.
        if self.mention_active() {
            match key.code {
                KeyCode::Up => {
                    if let Some(p) = &mut self.active_mention {
                        p.prev();
                    }
                    return;
                }
                KeyCode::Down => {
                    if let Some(p) = &mut self.active_mention {
                        p.next();
                    }
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    self.apply_mention();
                    return;
                }
                KeyCode::Esc => {
                    self.active_mention = None;
                    return;
                }
                _ => {}
            }
        }

        // 5a. /op subcommand hint popup (active after "/op " prefix is typed).
        if self.autocomplete.is_op_sub_active() {
            match key.code {
                KeyCode::Up => {
                    self.autocomplete.op_sub_prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.op_sub_next();
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(insert) = self.autocomplete.op_sub_confirm() {
                        self.input.take();
                        self.input.insert_str(&insert);
                        self.completion_hint = None;
                    }
                    self.autocomplete.dismiss();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                _ => {}
            }
        }

        // 5a2. /browser subcommand hint popup (active after "/browser " prefix).
        if self.autocomplete.is_browser_sub_active() {
            match key.code {
                KeyCode::Up => {
                    self.autocomplete.browser_sub_prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.browser_sub_next();
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(insert) = self.autocomplete.browser_sub_confirm() {
                        self.input.take();
                        self.input.insert_str(&insert);
                        self.completion_hint = None;
                    }
                    self.autocomplete.dismiss();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                _ => {}
            }
        }

        // 5a3. /loop subcommand hint popup (active after "/loop " prefix is typed).
        if self.autocomplete.is_loop_sub_active() {
            match key.code {
                KeyCode::Up => {
                    self.autocomplete.loop_sub_prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.loop_sub_next();
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(insert) = self.autocomplete.loop_sub_confirm() {
                        self.input.take();
                        self.input.insert_str(&insert);
                        self.completion_hint = None;
                    }
                    self.autocomplete.dismiss();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                _ => {}
            }
        }

        // 5a4. /schedule subcommand hint popup (active after "/schedule " prefix).
        if self.autocomplete.is_schedule_sub_active() {
            match key.code {
                KeyCode::Up => {
                    self.autocomplete.schedule_sub_prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.schedule_sub_next();
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(insert) = self.autocomplete.schedule_sub_confirm() {
                        self.input.take();
                        self.input.insert_str(&insert);
                        self.completion_hint = None;
                    }
                    self.autocomplete.dismiss();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                _ => {}
            }
        }

        // 5. Autocomplete interception (when the popup is open).
        if self.autocomplete.is_active() {
            match key.code {
                KeyCode::Up => {
                    self.autocomplete.prev();
                    return;
                }
                KeyCode::Down => {
                    self.autocomplete.next();
                    return;
                }
                KeyCode::Enter if self.autocomplete.selected_name() == Some("theme") => {
                    self.input.take();
                    self.completion_hint = None;
                    self.autocomplete.dismiss();
                    self.open_theme_picker();
                    return;
                }
                KeyCode::Enter if self.autocomplete.selected_name() == Some("model") => {
                    self.input.take();
                    self.completion_hint = None;
                    self.autocomplete.dismiss();
                    self.open_model_picker();
                    return;
                }
                KeyCode::Enter if self.autocomplete.selected_name() == Some("connect") => {
                    self.input.take();
                    self.completion_hint = None;
                    self.autocomplete.dismiss();
                    self.open_connect_dialog(agent_tx);
                    return;
                }
                KeyCode::Enter if self.autocomplete.selected_name() == Some("plugin") => {
                    self.input.take();
                    self.completion_hint = None;
                    self.autocomplete.dismiss();
                    self.open_plugin_picker();
                    return;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    self.apply_completion();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete.dismiss();
                    return;
                }
                _ => {}
            }
        }

        match fragmented_cursor_sequence_action(
            &mut self.pending_cursor_seq,
            key.code,
            key.modifiers,
            self.input.text().is_empty(),
        ) {
            FragmentedCursorAction::None => {}
            FragmentedCursorAction::ReplayBareO(count) => self.input.insert_str(&"O".repeat(count)),
            FragmentedCursorAction::ReplaySgr(text) => self.input.insert_str(&text),
            FragmentedCursorAction::Consumed => return,
            FragmentedCursorAction::Scroll(scroll) => {
                if self.input.text().is_empty() {
                    match scroll {
                        ChatMouseScroll::Up(n) => self.tabs[self.active].chat.scroll_up(n),
                        ChatMouseScroll::Down(n) => self.tabs[self.active].chat.scroll_down(n),
                    }
                }
                return;
            }
        }

        // 6. Enter submits; Shift/Alt+Enter newline; Up/Down recall submitted
        //    prompts (shell-style) when the cursor is at the input's edge; else
        //    feed the input box. (Autocomplete already claimed Up/Down above
        //    when its popup is open, so history only triggers otherwise.)
        match (key.code, key.modifiers) {
            (KeyCode::Enter, m)
                if !m.contains(KeyModifiers::SHIFT) && !m.contains(KeyModifiers::ALT) =>
            {
                let text = self.input.take();
                self.reset_input_browse_state();
                if self.finish_queued_edit(text.clone()) {
                    return;
                }
                // An empty composer still submits when image chips are
                // attached — an image-only turn (submit() titles it from the
                // image name). Without this, Enter over attached chips did
                // nothing and the chips looked stuck above the input box.
                let has_pending_images = !self.active_tab().pending_images.is_empty();
                if !text.trim().is_empty() || has_pending_images {
                    // A follow-up typed while the tab is busy will be QUEUED by
                    // submit(); queued follow-ups are intentionally never
                    // recorded — neither persisted nor added to Up/Down recall.
                    if !text.trim().is_empty() && !self.active_tab().is_busy() {
                        self.record_prompt_history(&text);
                    }
                    self.submit(&text, agent_tx).await;
                }
            }
            (KeyCode::Enter, _) => self.input.insert_newline(),
            (KeyCode::Up, m) if m.is_empty() && self.input.cursor_on_first_line() => {
                if !self.edit_previous_queued_input() {
                    self.history_prev();
                }
                // Swapping the whole composer text invalidates arbitrary cells;
                // during a streaming turn the terminal's incremental diff has
                // been observed leaving stale glyphs from the previous (CJK-
                // wide) entry behind. A full clear+repaint per browse keypress
                // is cheap and runs inside the synchronized update (no flash).
                self.force_redraw = true;
            }
            (KeyCode::Down, m) if m.is_empty() && self.input.cursor_on_last_line() => {
                if !self.edit_next_queued_input() {
                    self.history_next();
                }
                self.force_redraw = true;
            }
            _ => {
                self.active_input_selection = None;
                self.input.input(key);
                // Editing the text exits history-browse mode.
                self.tabs[self.active].history_pos = None;
                // A file dragged into the terminal arrives as typed keystrokes
                // (not a bracketed paste), so handle_paste never sees it. Once a
                // complete, existing image path lands in the input, lift it into
                // an image chip — same display as a pasted/clipboard image.
                self.absorb_image_paths_from_input();
            }
        }
        // 7. Refresh the autocomplete + @-mention popups from the new input.
        self.autocomplete.update(&self.input.text());
        self.refresh_mention();
    }

    /// If the current input text contains a complete, existing image path (e.g.
    /// a file dragged into the terminal), move it into a pending image chip and
    /// strip it from the text. Cheap-guards on an image extension so ordinary
    /// typing doesn't hit the filesystem; silently leaves the text unchanged if
    /// nothing resolves (a half-typed path is not an error here).
    fn absorb_image_paths_from_input(&mut self) {
        let text = self.input.text();
        let lower = text.to_ascii_lowercase();
        let has_image_ext = [".png", ".jpg", ".jpeg", ".gif", ".webp"]
            .iter()
            .any(|ext| lower.contains(ext));
        if !has_image_ext {
            return;
        }
        let cwd = self.active_tab().engine.cwd.clone();
        if let Ok(parsed) = split_pasted_image_paths(&cwd, &text) {
            if !parsed.images.is_empty() {
                let n = parsed.images.len();
                self.active_tab_mut().pending_images.extend(parsed.images);
                self.input.set_text(&parsed.remaining_text);
                self.toast = Some(Toast::info(format!(
                    "{} {n} {}",
                    crate::tr("attached"),
                    crate::tr("images")
                )));
            }
        }
    }

    /// Handle keys for the pending-image chips. Returns `true` if the key was
    /// consumed. ↑ enters/moves selection (only when the input is empty, so it
    /// doesn't fight history/cursor); ←/→/↑/↓ move; Backspace/Delete removes the
    /// selected image; Enter (or the platform primary modifier + Enter) views
    /// it; Esc or any other key exits selection.
    fn handle_image_chip_key(&mut self, key: KeyEvent) -> bool {
        let len = self.active_tab().pending_images.len();
        if len == 0 {
            self.selected_image = None;
            return false;
        }
        // Only the first MAX_VISIBLE_CHIPS chips are rendered (+N for the rest),
        // so selection is capped to what's actually shown/highlighted.
        let visible = len.min(MAX_VISIBLE_CHIPS);
        // Keep a stale index in range.
        if let Some(i) = self.selected_image {
            if i >= visible {
                self.selected_image = Some(visible - 1);
            }
        }
        let selected = self.selected_image;
        match key.code {
            // Enter selection from the empty input; once selecting, move toward
            // earlier chips.
            KeyCode::Up if key.modifiers.is_empty() && self.input.is_empty() => {
                self.selected_image = Some(match selected {
                    None => visible - 1,
                    Some(i) => i.saturating_sub(1),
                });
                true
            }
            KeyCode::Left if selected.is_some() => {
                self.selected_image = Some(selected.unwrap().saturating_sub(1));
                true
            }
            KeyCode::Right | KeyCode::Down if selected.is_some() => {
                let i = selected.unwrap();
                // Past the last visible chip → leave selection.
                self.selected_image = if i + 1 < visible { Some(i + 1) } else { None };
                true
            }
            KeyCode::Backspace | KeyCode::Delete if selected.is_some() => {
                let i = selected.unwrap();
                let removed = self.active_tab_mut().pending_images.remove(i);
                cleanup_clipboard_temp(&mut self.clipboard_temps, &removed.path);
                let remaining = self.active_tab().pending_images.len();
                self.selected_image = (remaining > 0).then(|| i.min(remaining - 1));
                self.toast = Some(Toast::info(crate::tr("removed attached image")));
                true
            }
            KeyCode::Enter if selected.is_some() => {
                self.view_selected_image();
                true
            }
            KeyCode::Esc if selected.is_some() => {
                self.selected_image = None;
                true
            }
            // Any other key leaves selection and is handled normally.
            _ => {
                if selected.is_some()
                    && !matches!(key.code, KeyCode::Up | KeyCode::Left | KeyCode::Right)
                {
                    self.selected_image = None;
                }
                false
            }
        }
    }

    /// Open the selected pending image in the OS image viewer (`open` on macOS,
    /// `xdg-open` on Linux, `start` on Windows). Clipboard images are backed by
    /// a temp file at attach time, so every chip has a path to open.
    fn view_selected_image(&mut self) {
        let Some(i) = self.selected_image else { return };
        let path = match self.active_tab().pending_images.get(i) {
            Some(img) if !img.path.as_os_str().is_empty() => img.path.clone(),
            _ => {
                self.toast = Some(Toast::error(crate::tr("no file to view for this image")));
                return;
            }
        };
        match open_in_os_viewer(&path) {
            Ok(()) => self.toast = Some(Toast::info(crate::tr("opening image…"))),
            Err(e) => self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("view failed")))),
        }
    }

    /// Ctrl+V: prefer an IMAGE on the clipboard (a screenshot or copied image),
    /// then fall back to text. Terminals only deliver pastes as text and never
    /// hand image data to a TUI, so we query the OS clipboard directly.
    fn paste_from_clipboard(&mut self) {
        // A text field is focused (connect form / filter) → paste text directly,
        // never an image.
        if self.connect.is_some() {
            self.paste_clipboard_text();
            return;
        }
        match zode_core::clipboard::read_image_from_clipboard() {
            Ok(Some(bytes)) => self.attach_clipboard_image(bytes),
            // No image (or the image read failed) → treat it as a text paste.
            Ok(None) | Err(_) => self.paste_clipboard_text(),
        }
    }

    fn paste_clipboard_text(&mut self) {
        match zode_core::clipboard::read_from_clipboard() {
            Ok(text) => self.handle_paste(&text),
            Err(e) => {
                self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("paste failed"))))
            }
        }
    }

    /// Attach raw image bytes from the clipboard as a pending image (same queue
    /// as a pasted image path), so the next prompt sends it.
    fn attach_clipboard_image(&mut self, bytes: Vec<u8>) {
        match zode_core::images::image_attachment_from_bytes(&bytes, "clipboard image") {
            Ok(mut image) => {
                // Back the clipboard image with a temp file so it can be VIEWED
                // (Enter on the chip opens this path). The content_block (base64)
                // is still what gets sent to the model.
                if let Some(path) = write_clipboard_temp_image(&bytes, &image.media_type) {
                    self.clipboard_temps.insert(path.clone());
                    image.path = path;
                }
                self.active_tab_mut().pending_images.push(image);
                self.toast = Some(Toast::info(crate::tr("attached image from clipboard")));
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!(
                    "{}: {e}",
                    crate::tr("paste image failed")
                )))
            }
        }
    }

    fn handle_paste(&mut self, text: &str) {
        // Normalize line endings FIRST: bracketed paste delivers newlines as
        // `\r` in several terminals (iTerm2 among them), and Windows-origin
        // clipboard text carries `\r\n` — the textarea splits lines on `\n`
        // only, so stray CRs ended up embedded in one long line and wrecked
        // wrapping/width ("paste scrambles formatting").
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = normalized.as_str();
        // The connect dialog accepts pasted text into its focused field (API key,
        // base URL, …) or, in the provider stage, its search filter.
        if let Some(dialog) = &mut self.connect {
            dialog.paste(text);
            return;
        }
        // NOTE: `active_dialog` (the permission prompt) is intentionally NOT in
        // this block-list. The permission prompt is non-blocking — the user can
        // type/queue a follow-up while a tool waits for approval — so paste must
        // reach the input box too. The remaining entries are truly modal.
        if self.active_question.is_some()
            || self.settings.is_some()
            || self.connect.is_some()
            || self.plugin_picker.is_some()
            || self.browser_panel.is_some()
            || self.team_panel.is_some()
            || self.agents_dialog.is_some()
            || self.workflows_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.session_picker.is_some()
            || self.tasks_panel.is_some()
            || self.subagents_panel.is_some()
            || self.files_panel.is_some()
            || self.show_help
        {
            return;
        }

        // An empty paste usually means the terminal's own ⌘V (Edit▸Paste) fired
        // on an IMAGE clipboard — it had no text to send. Probe for an image so
        // ⌘V attaches it even in terminals that intercept ⌘V (Terminal.app,
        // iTerm2) instead of forwarding the key event to the app.
        if text.trim().is_empty() {
            if let Ok(Some(bytes)) = zode_core::clipboard::read_image_from_clipboard() {
                self.attach_clipboard_image(bytes);
            }
            return;
        }

        let cwd = self.active_tab().engine.cwd.clone();
        match split_pasted_image_paths(&cwd, text) {
            Ok(parsed) => {
                let image_count = parsed.images.len();
                if image_count > 0 {
                    self.active_tab_mut().pending_images.extend(parsed.images);
                    self.toast = Some(Toast::info(format!(
                        "{} {image_count} {}",
                        crate::tr("attached"),
                        crate::tr("images")
                    )));
                }
                if !parsed.remaining_text.is_empty() {
                    self.input.insert_str(&parsed.remaining_text);
                }
                self.autocomplete.update(&self.input.text());
                self.refresh_mention();
            }
            Err(e) => {
                self.toast = Some(Toast::error(e.to_string()));
            }
        }
    }

    /// (Cmd/Ctrl)+left-click on a pending-image chip → select + open it in the
    /// OS viewer. Returns true if the click hit a chip. Note: terminals only
    /// report Shift/Alt/Ctrl modifiers on mouse events (the mouse protocol
    /// can't carry ⌘), so on macOS this is effectively Ctrl-click.
    fn try_view_image_chip_click(&mut self, mouse: &MouseEvent) -> bool {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            || !is_primary_mod(mouse.modifiers)
            || mouse.row != self.image_chip_row
        {
            return false;
        }
        let hit = self
            .image_chip_hits
            .iter()
            .find(|(start, end, _)| mouse.column >= *start && mouse.column < *end)
            .map(|(_, _, idx)| *idx);
        if let Some(idx) = hit {
            self.selected_image = Some(idx);
            self.view_selected_image();
            return true;
        }
        false
    }

    /// The frame rect mouse coordinates should be resolved against: the last
    /// painted frame, or (before any draw) a live terminal query. `None` when
    /// neither is available — the event is then ignored, since there is no
    /// layout it could meaningfully hit.
    fn hit_test_area(&self) -> Option<Rect> {
        if self.last_frame_area.width > 0 && self.last_frame_area.height > 0 {
            return Some(self.last_frame_area);
        }
        crossterm::terminal::size()
            .ok()
            .map(|(width, height)| Rect::new(0, 0, width, height))
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if let Some(picker) = &mut self.session_picker {
            match session_picker_scroll_from_mouse(mouse.kind) {
                Some(SessionPickerMouseScroll::Up(n)) => picker.scroll_up(n),
                Some(SessionPickerMouseScroll::Down(n)) => picker.scroll_down(n),
                None => {}
            }
            return;
        }

        // Wheel-scroll the provider list (only the list stage has rows).
        if let Some(dialog) = &mut self.connect {
            if dialog.stage() == ConnectStage::Provider {
                match mouse.kind {
                    MouseEventKind::ScrollDown => dialog.next(),
                    MouseEventKind::ScrollUp => dialog.prev(),
                    _ => {}
                }
            }
            return;
        }

        // Wheel-scroll the modified-files overlay.
        if self.files_panel.is_some() {
            let total = self.active_git_file_count();
            if let Some(panel) = &mut self.files_panel {
                match mouse.kind {
                    MouseEventKind::ScrollDown => panel.scroll_down(3, total),
                    MouseEventKind::ScrollUp => panel.scroll_up(3),
                    _ => {}
                }
            }
            return;
        }

        // Left clicks drive the question modal (options/chips/submit) and the
        // permission prompt (answer chips) — mirroring their key paths.
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some(q) = &mut self.active_question {
                if q.on_mouse(mouse.column, mouse.row) {
                    self.active_question = None;
                    if let Some(next) = self.pending_questions.pop_front() {
                        self.open_question(next);
                    }
                }
                return;
            }
            if let Some(d) = &self.active_dialog {
                if let Some(approval) = d.approval_at(mouse.column, mouse.row) {
                    self.answer_permission(approval);
                    return;
                }
                // A click elsewhere falls through — the prompt is non-blocking.
            }
        }

        // NB: an open permission prompt (`active_dialog`) deliberately does NOT
        // gate the mouse — it's non-blocking like its keyboard handling, so
        // scroll and clicks outside its chips keep working on the chat below.
        if self.active_question.is_some()
            || self.settings.is_some()
            || self.connect.is_some()
            || self.plugin_picker.is_some()
            || self.browser_panel.is_some()
            || self.team_panel.is_some()
            || self.agents_dialog.is_some()
            || self.workflows_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.tasks_panel.is_some()
            || self.subagents_panel.is_some()
            || self.files_panel.is_some()
            || self.show_help
        {
            return;
        }

        // The floating "jump to bottom" pill overlays the transcript, so it
        // takes a left click before any row underneath it. It hit-tests against
        // the rect it was painted into, so it works regardless of selection
        // mode and without recomputing the chat area.
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.tabs[self.active]
                .chat
                .jump_pill_hit(mouse.column, mouse.row)
        {
            self.tabs[self.active].chat.scroll_to_bottom();
            self.active_selection = None;
            return;
        }

        // (Cmd/Ctrl)+left-click on an image chip opens it in the OS viewer.
        if self.try_view_image_chip_click(&mouse) {
            return;
        }

        // Left-click on a collapsible sidebar section header toggles its fold.
        if self.try_sidebar_header_click(&mouse, agent_tx) {
            return;
        }

        let Some(area) = self.hit_test_area() else {
            return;
        };
        let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
        let areas = split_main(area, show_sidebar, self.status_rows);
        let input_area = self.input_area_for_composer(areas.composer);

        if self.selection_mode {
            if self.handle_input_selection_mouse(mouse, input_area) {
                return;
            }
            if self.handle_selection_mouse(mouse, areas.chat) {
                return;
            }
        }

        match chat_scroll_from_mouse(mouse.kind, mouse.column, mouse.row, areas.chat) {
            Some(ChatMouseScroll::Up(n)) => self.tabs[self.active].chat.scroll_up(n),
            Some(ChatMouseScroll::Down(n)) => self.tabs[self.active].chat.scroll_down(n),
            None => {}
        }
    }

    fn input_area_for_composer(&self, mut input_area: Rect) -> Rect {
        if !self.tabs[self.active].pending_images.is_empty() && input_area.height > 2 {
            input_area.y = input_area.y.saturating_add(1);
            input_area.height = input_area.height.saturating_sub(1);
        }
        input_area
    }

    fn handle_input_selection_mouse(&mut self, mouse: MouseEvent, input_area: Rect) -> bool {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.active_input_selection = None;
                let Some(point) =
                    self.input
                        .selection_point_at(input_area, mouse.column, mouse.row)
                else {
                    return false;
                };
                self.active_selection = None;
                self.active_input_selection = Some(InputSelection::new(point, point));
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(selection) = self.active_input_selection else {
                    return false;
                };
                if let Some(point) =
                    self.input
                        .selection_point_at(input_area, mouse.column, mouse.row)
                {
                    self.active_input_selection =
                        Some(InputSelection::new(selection.anchor, point));
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(selection) = self.active_input_selection else {
                    return false;
                };
                let selection = self
                    .input
                    .selection_point_at(input_area, mouse.column, mouse.row)
                    .map(|point| InputSelection::new(selection.anchor, point))
                    .unwrap_or(selection);
                self.active_input_selection = Some(selection);
                // Copy-on-select: finishing a drag puts the selection on the
                // system clipboard (pbcopy) + terminal clipboard (OSC 52) so
                // Cmd+V pastes it — Cmd+C never reaches a TUI on macOS. Same as
                // the transcript selection below.
                if selection.anchor != selection.focus {
                    self.copy_input_selection(selection);
                }
                true
            }
            _ => false,
        }
    }

    fn handle_selection_mouse(&mut self, mouse: MouseEvent, chat_area: Rect) -> bool {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !rect_contains(chat_area, mouse.column, mouse.row) {
                    self.active_selection = None;
                    return false;
                }
                if let Some(point) = self.chat_selection_point(chat_area, mouse.column, mouse.row) {
                    self.active_selection = Some(ChatSelection::new(point, point));
                    return true;
                }
                false
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(selection) = self.active_selection else {
                    return false;
                };
                if let Some(scroll) =
                    selection_scroll_from_drag(mouse.kind, mouse.column, mouse.row, chat_area)
                {
                    match scroll {
                        ChatMouseScroll::Up(n) => self.tabs[self.active].chat.scroll_up(n),
                        ChatMouseScroll::Down(n) => self.tabs[self.active].chat.scroll_down(n),
                    }
                }
                if let Some(point) = self.chat_selection_point(chat_area, mouse.column, mouse.row) {
                    self.active_selection = Some(ChatSelection::new(selection.anchor, point));
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(selection) = self.active_selection else {
                    return false;
                };
                let selection = self
                    .chat_selection_point(chat_area, mouse.column, mouse.row)
                    .map(|point| ChatSelection::new(selection.anchor, point))
                    .unwrap_or(selection);
                self.active_selection = Some(selection);
                // Copy-on-select (opencode's default): finishing a drag puts the
                // selection on the system clipboard (pbcopy) + terminal clipboard
                // (OSC 52), so Cmd+V pastes it without a copy key — Cmd+C is eaten
                // by the terminal on macOS. The Ctrl/Cmd+C chord also copies.
                if selection.anchor != selection.focus {
                    self.copy_chat_selection(selection, chat_area);
                } else if rect_contains(chat_area, mouse.column, mouse.row) {
                    // A plain click (no drag): toggle the fold of the tool /
                    // thinking block whose header row was hit, if any.
                    self.try_chat_collapse_click(chat_area, mouse.row);
                }
                true
            }
            _ => false,
        }
    }

    /// A plain chat-area click: toggle the collapsed/expanded state of the
    /// tool or thinking block whose header row sits at `row`.
    fn try_chat_collapse_click(&mut self, chat_area: Rect, row: u16) -> bool {
        let theme = self.theme.clone();
        let active_model = self.tabs[self.active].engine.model.clone();
        let active_cwd = self.tabs[self.active].engine.cwd.clone();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: &active_model,
            cwd: &active_cwd,
        };
        self.tabs[self.active]
            .chat
            .toggle_collapse_at(&theme, meta, chat_area, row)
    }

    fn chat_selection_point(
        &mut self,
        chat_area: Rect,
        column: u16,
        row: u16,
    ) -> Option<ChatSelectionPoint> {
        let theme = self.theme.clone();
        let active_model = self.tabs[self.active].engine.model.clone();
        let active_cwd = self.tabs[self.active].engine.cwd.clone();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: &active_model,
            cwd: &active_cwd,
        };
        self.tabs[self.active]
            .chat
            .selection_point_at(&theme, meta, chat_area, column, row)
    }

    /// Whether a non-empty chat OR input selection is currently held. Used to
    /// decide whether the copy chord copies (selection present) or falls through
    /// to the interrupt/quit handling (nothing selected).
    fn has_active_selection(&self) -> bool {
        self.active_selection.is_some_and(|s| s.anchor != s.focus)
            || self
                .active_input_selection
                .is_some_and(|s| s.anchor != s.focus)
    }

    /// Copy whatever is selected — the chat transcript selection if one is held,
    /// otherwise the input-box selection. Bound to the platform copy chord.
    ///
    /// Clears the selection afterward so (a) the highlight doesn't linger and
    /// (b) a follow-up Ctrl+C falls through to interrupt/quit — important because
    /// on macOS terminals Cmd+C isn't delivered, so Ctrl+C is the copy chord AND
    /// the interrupt key; copying must not permanently block the kill path.
    fn copy_active_selection(&mut self) {
        if let Some(selection) = self.active_selection.filter(|s| s.anchor != s.focus) {
            // Recompute the chat area exactly like `handle_mouse` does, so
            // `selected_text` resolves against the painted width.
            let Some(area) = self.hit_test_area() else {
                return;
            };
            let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
            let chat_area = split_main(area, show_sidebar, self.status_rows).chat;
            self.copy_chat_selection(selection, chat_area);
        } else if let Some(selection) = self.active_input_selection {
            self.copy_input_selection(selection);
        }
        self.active_selection = None;
        self.active_input_selection = None;
    }

    fn copy_chat_selection(&mut self, selection: ChatSelection, chat_area: Rect) {
        let theme = self.theme.clone();
        let active_model = self.tabs[self.active].engine.model.clone();
        let active_cwd = self.tabs[self.active].engine.cwd.clone();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: &active_model,
            cwd: &active_cwd,
        };
        let text = self.tabs[self.active]
            .chat
            .selected_text(selection, &theme, meta, chat_area);
        if text.trim().is_empty() {
            // A real drag (anchor ≠ focus) that landed only on blank/prefix area
            // copies nothing — say so instead of silently doing nothing, which
            // reads as "copy didn't work".
            if selection.anchor != selection.focus {
                self.toast = Some(Toast::info(crate::tr("nothing to copy in selection")));
            }
            return;
        }
        // Copy via OSC 52 (terminal clipboard — works in Warp, iTerm2, kitty,
        // Ghostty and over SSH/tmux, where the in-app ⌘C never arrives) AND
        // pbcopy (local fallback + large payloads OSC 52 may cap). Surface a
        // failing system-clipboard write instead of a "copied" toast that lies.
        write_osc52_clipboard(&text);
        self.toast = Some(match zode_core::clipboard::copy_to_clipboard(&text) {
            Ok(_) => Toast::info(crate::tr("copied selection to clipboard")),
            Err(e) => Toast::error(format!("{}: {e}", crate::tr("copy failed"))),
        });
    }

    fn copy_input_selection(&mut self, selection: InputSelection) {
        let text = self.input.selected_text(selection);
        if text.trim().is_empty() {
            return;
        }
        write_osc52_clipboard(&text);
        self.toast = Some(match zode_core::clipboard::copy_to_clipboard(&text) {
            Ok(_) => Toast::info(crate::tr("copied input selection to clipboard")),
            Err(e) => Toast::error(format!("{}: {e}", crate::tr("copy failed"))),
        });
    }

    fn open_settings(&mut self) {
        let theme_ids = self.theme_ids();
        self.settings = Some(SettingsDialog::new(theme_ids, self.provider_names.clone()));
    }

    fn open_theme_picker(&mut self) {
        self.settings = Some(SettingsDialog::theme_picker(self.theme_ids()));
    }

    fn open_model_picker(&mut self) {
        self.settings = Some(SettingsDialog::model_picker(self.model_ids()));
    }

    fn open_effort_picker(&mut self) {
        self.settings = Some(SettingsDialog::effort_picker());
    }

    fn open_sidebar_picker(&mut self) {
        self.settings = Some(SettingsDialog::sidebar_picker());
    }

    fn open_language_picker(&mut self) {
        self.settings = Some(SettingsDialog::language_picker());
    }

    fn open_sandbox_picker(&mut self) {
        self.settings = Some(SettingsDialog::sandbox_picker());
    }

    /// Open the image-understanding provider picker (`/vision`). With no named
    /// providers configured there's nothing to pick, so fall back to a hint.
    fn open_vision_picker(&mut self) {
        let providers = self.template.vision_provider_names();
        if providers.is_empty() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "no image-capable providers configured — add a vision model under `providers` \
                 or set supportsImages=true, then pick it here",
            ));
            return;
        }
        self.settings = Some(SettingsDialog::vision_provider_picker(providers));
    }

    /// Resolve a sandbox config for a `/sandbox` toggle, FAIL-CLOSED: on a
    /// backend error, report it in the transcript and return `Err(())` so the
    /// caller aborts the toggle instead of silently enabling no isolation.
    fn resolve_sandbox_or_report(
        &mut self,
        cwd: &std::path::Path,
        mode: zode_core::sandbox::SandboxMode,
        net: bool,
    ) -> Result<Option<zode_core::sandbox::SandboxConfig>, ()> {
        // Rebuild from the persisted config section so a runtime toggle keeps
        // the configured writableRoots / excludeSlashTmp / excludeTmpdirEnvVar
        // (previously it rebuilt from bare defaults, silently re-widening /tmp
        // for a user who had excluded it).
        match zode_core::sandbox::resolve_with_settings(
            cwd,
            self.template.sandbox_settings(),
            mode,
            net,
        ) {
            // Strict-read: config OR the remembered startup bit (a CLI flag can
            // enable it beyond config; a fresh resolve, e.g. on `/sandbox on`
            // from off, would otherwise drop it).
            Ok(c) => {
                let restrict = c.restrict_reads() || self.sandbox_restrict_reads;
                Ok(Some(c.with_restrict_reads(restrict)))
            }
            Err(e) => {
                self.active_tab_mut()
                    .chat
                    .push_system(&format!("{}: {e}", crate::tr("sandbox")));
                Err(())
            }
        }
    }

    /// Apply a `/sandbox` action (also used by the sandbox picker): toggle the
    /// sandbox on/off, switch mode, or toggle network, then rebuild the active
    /// tab's engine and report the new state.
    async fn apply_sandbox_action(
        &mut self,
        action: &str,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        use zode_core::sandbox::{SandboxConfig, SandboxMode};
        let cwd = self.active_tab().engine.cwd.clone();
        let current = self.template.sandbox().cloned();
        let arg = action
            .trim()
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let target: Option<Option<SandboxConfig>> = match arg.as_str() {
            "off" | "disable" => Some(None),
            "on" | "enable" => {
                let mode = current.as_ref().map(|c| c.mode()).unwrap_or_default();
                let net = current.as_ref().map(|c| c.allow_network()).unwrap_or(false);
                // Codex defaults: /tmp + $TMPDIR writable (re-enabling from off).
                match self.resolve_sandbox_or_report(&cwd, mode, net) {
                    Ok(opt) => Some(opt),
                    Err(()) => return,
                }
            }
            "read-only" | "readonly" | "ro" => match current.clone() {
                Some(c) => Some(Some(c.with_mode(SandboxMode::ReadOnly))),
                None => match self.resolve_sandbox_or_report(&cwd, SandboxMode::ReadOnly, false) {
                    Ok(opt) => Some(opt),
                    Err(()) => return,
                },
            },
            "workspace-write" | "write" | "ww" => match current.clone() {
                Some(c) => Some(Some(c.with_mode(SandboxMode::WorkspaceWrite))),
                None => {
                    match self.resolve_sandbox_or_report(&cwd, SandboxMode::WorkspaceWrite, false) {
                        Ok(opt) => Some(opt),
                        Err(()) => return,
                    }
                }
            },
            "network on" | "net on" | "network" => match current.clone() {
                Some(c) => Some(Some(c.with_network(true))),
                None => match self.resolve_sandbox_or_report(&cwd, SandboxMode::default(), true) {
                    Ok(opt) => Some(opt),
                    Err(()) => return,
                },
            },
            "network off" | "net off" => Some(current.clone().map(|c| c.with_network(false))),
            _ => {
                let line = sandbox_status_line(current.as_ref());
                self.active_tab_mut().chat.push_system(&line);
                return;
            }
        };
        if let Some(new_sandbox) = target {
            // The sandbox must prove it actually enforces before the toggle
            // applies (some hosts have a backend that runs but does not
            // confine). FAIL-CLOSED, but verified inside the reassemble task
            // (see `start_reassemble_active`) — awaiting the sandbox-exec
            // round trip here froze the event loop.
            let t = self.template.with_sandbox(new_sandbox);
            if !self.start_reassemble_active(t, ReassembleEffect::Sandbox, agent_tx) {
                self.active_tab_mut().chat.push_system(&format!(
                    "{}: {}",
                    crate::tr("sandbox"),
                    crate::tr("unavailable on this host (need sandbox-exec / bwrap)")
                ));
            }
        }
    }

    fn open_connect_dialog(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        // Build the dialog OFF the event loop: the catalog + config reads are
        // small local files (never the network on this path), but any sync
        // disk I/O in the loop can stutter. The dialog arrives as an event a
        // beat later; the same blocking thread then refreshes the disk cache
        // best-effort (sync I/O + a 5-second HTTP timeout — it must not run
        // on an async worker thread) so the next open is current.
        let tx = agent_tx.clone();
        tokio::task::spawn_blocking(move || {
            let cat = zode_core::Catalog::load_blocking();
            // The user's configured providers form the "Configured" section
            // (listed first); load them best-effort from the global config.
            let configured = ConfigManager::load_global()
                .map(|c| c.providers)
                .unwrap_or_default();
            let dialog = ConnectDialog::with_catalog_and_providers(&cat, &configured);
            let _ = tx.send(AppEvent::ConnectDialogReady {
                dialog: Box::new(dialog),
            });
            let _ = zode_core::Catalog::refresh_blocking();
        });
    }

    /// Open the `/plugin` picker over the active tab's discovered plugins
    /// (tool groups, MCP servers with live state, skills, LSP servers).
    fn open_plugin_picker(&mut self) {
        let plugins = self.active_tab().engine.plugin_list();
        self.plugin_picker = Some(PluginPicker::new(plugins));
    }

    /// Snapshot the active tab's browser session/plugin state into the shape
    /// the `/browser` panel renders. Cheap: `target()`, `is_enabled`, and bridge
    /// connection state are plain reads, no I/O.
    fn browser_panel_status(&self) -> BrowserPanelStatus {
        let engine = &self.active_tab().engine;
        BrowserPanelStatus {
            // Reflect BOTH switches: `browser.enabled`/`--no-browser` (which
            // skips tool registration entirely, engine.rs `cfg.browser.enabled()`)
            // and the `tools:browser` plugin-group toggle. Either one being off
            // means zero browser tools are actually live.
            group_enabled: engine.browser.enabled() && engine.plugins.is_enabled("tools:browser"),
            default_enabled: engine.browser.enabled(),
            target: match engine.browser.target() {
                zode_core::browser::BrowserTarget::Managed => "managed".into(),
                zode_core::browser::BrowserTarget::Bridge => "bridge".into(),
            },
            paired: engine.browser.bridge_connected(),
            // There is no cheap sync "is the managed browser up" read yet.
            running: false,
        }
    }

    /// Open the bare `/browser` status panel.
    fn open_team_panel(&mut self, team: &std::sync::Arc<zode_core::team::TeamManager>) {
        use crate::ui::dialog::team_panel::{TeamPanel, TeamPanelRow};
        let rows: Vec<TeamPanelRow> = team
            .roster()
            .into_iter()
            .map(|t| TeamPanelRow {
                name: t.name,
                model_label: t.model_label,
                status_line: t.status_line,
                usage_in: t.usage_in,
                usage_out: t.usage_out,
            })
            .collect();
        let board_summary: Vec<String> = team
            .board_report()
            .lines()
            .skip(1) // drop the "Board (rev N):" header — the panel adds its own
            .map(|s| s.trim_start().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.team_panel = Some(TeamPanel::new(rows, board_summary));
    }

    fn open_browser_panel(&mut self) {
        self.browser_panel = Some(BrowserPanel::new(self.browser_panel_status()));
    }

    async fn start_browser_pairing(&mut self, _agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let session = self.active_tab().engine.browser.clone();
        match session.start_pairing().await {
            Ok(handle) => {
                let url = browser_extension_pairing_url(handle.port, &handle.code);
                let open_note =
                    browser_extension_open_note(&url, session.open_extension_url(&url).await);
                self.active_tab_mut().chat.push_system(&format!(
                    "Pairing code: {} (valid 2 min). WS port {}. {open_note}",
                    handle.code, handle.port
                ));
            }
            Err(e) => self.active_tab_mut().chat.push_system(&e.to_string()),
        }
        if self.browser_panel.is_some() {
            let status = self.browser_panel_status();
            if let Some(panel) = &mut self.browser_panel {
                panel.set_status(status);
            }
        }
    }

    fn open_agents_dialog(&mut self) {
        self.agents_dialog = Some(AgentsDialog::new(self.agent_rows()));
    }

    /// Build the agent list for the dialog: user-defined agents (deletable)
    /// first, then the built-ins. User defs are read fresh from disk.
    fn agent_rows(&self) -> Vec<AgentRow> {
        let cwd = self.active_tab().engine.cwd.clone();
        let user_defs = zode_core::agents::load_agent_defs(&cwd);
        let user_names: std::collections::HashSet<String> =
            user_defs.iter().map(|d| d.name.clone()).collect();
        let mut rows: Vec<AgentRow> = user_defs
            .into_iter()
            .map(|d| AgentRow {
                name: d.name,
                description: d.description,
                kind: AgentKind::User,
            })
            .collect();
        for (n, desc) in &self.active_tab().engine.agent_types {
            if !user_names.contains(n) {
                rows.push(AgentRow {
                    name: n.clone(),
                    description: desc.clone(),
                    kind: AgentKind::BuiltIn,
                });
            }
        }
        rows
    }

    async fn handle_agents_dialog_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let Some(dialog) = &mut self.agents_dialog else {
            return;
        };
        let action: Option<AgentsAction> = if dialog.is_input_mode() {
            match code {
                KeyCode::Tab => {
                    dialog.form_next_field();
                    None
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => dialog.submit(),
                KeyCode::Char(c) => {
                    dialog.form_push(c);
                    None
                }
                KeyCode::Backspace => {
                    dialog.form_backspace();
                    None
                }
                KeyCode::Enter => dialog.form_enter(),
                KeyCode::Esc => dialog.on_esc(),
                _ => None,
            }
        } else {
            match code {
                KeyCode::Up => {
                    dialog.prev();
                    None
                }
                KeyCode::Down => {
                    dialog.next();
                    None
                }
                KeyCode::Enter => {
                    dialog.on_enter();
                    None
                }
                KeyCode::Char('d') => dialog.on_delete(),
                KeyCode::Esc => dialog.on_esc(),
                _ => None,
            }
        };
        match action {
            Some(AgentsAction::Close) => self.agents_dialog = None,
            Some(AgentsAction::Create {
                name,
                description,
                system,
            }) => {
                match zode_core::agents::write_agent_def(&name, &description, &system) {
                    Ok(_) => {
                        self.agents_dialog = None;
                        // Reload so the new agent is spawnable + in autocomplete.
                        self.start_reassemble_active(
                            self.template.clone(),
                            ReassembleEffect::AgentReload {
                                notify: ReassembleNotify::Toast(format!(
                                    "{}: {name}",
                                    crate::tr("agent created")
                                )),
                                refresh_dialog: false,
                            },
                            agent_tx,
                        );
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("create failed"))))
                    }
                }
            }
            Some(AgentsAction::AiCreate { brief }) => {
                // Close the dialog and ask the main agent to build the agent via
                // the DefineAgent tool (requires orchestration, default on).
                self.agents_dialog = None;
                let prompt = format!(
                    "Create a new sub-agent for me using the `DefineAgent` tool. \
                     Here is what it should do:\n\n{brief}\n\nChoose a concise \
                     kebab-case name, a one-line description, and a clear system \
                     prompt, then call DefineAgent with them."
                );
                self.submit(&prompt, agent_tx).await;
            }
            Some(AgentsAction::Delete { name }) => {
                match zode_core::agents::delete_agent_def(&name) {
                    Ok(true) => {
                        self.start_reassemble_active(
                            self.template.clone(),
                            ReassembleEffect::AgentReload {
                                notify: ReassembleNotify::Toast(format!(
                                    "{}: {name}",
                                    crate::tr("agent deleted")
                                )),
                                refresh_dialog: true,
                            },
                            agent_tx,
                        );
                    }
                    Ok(false) => {
                        self.toast = Some(Toast::info(format!(
                            "{name} {}",
                            crate::tr("is built-in (not deletable)")
                        )))
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("delete failed"))))
                    }
                }
            }
            None => {}
        }
    }

    fn theme_ids(&self) -> Vec<String> {
        self.theme_store
            .list()
            .iter()
            .map(|t| t.id.clone())
            .collect()
    }

    fn model_ids(&self) -> Vec<String> {
        self.template.model_ids()
    }

    async fn handle_settings_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        // Extract a confirmed action (if any), then drop the dialog borrow
        // before the async apply.
        let action = {
            let Some(d) = &mut self.settings else {
                return;
            };
            match code {
                KeyCode::Up => {
                    d.prev();
                    None
                }
                KeyCode::Down => {
                    d.next();
                    None
                }
                KeyCode::Esc => {
                    if d.is_root_level() {
                        self.settings = None;
                    } else {
                        d.back();
                    }
                    None
                }
                KeyCode::Enter => {
                    if d.level() == SettingsLevel::Top {
                        d.enter();
                        None
                    } else {
                        d.confirm()
                    }
                }
                _ => None,
            }
        };
        if let Some(action) = action {
            self.settings = None;
            self.apply_settings(action, agent_tx).await;
        }
    }

    async fn handle_connect_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let action = {
            let Some(dialog) = &mut self.connect else {
                return;
            };
            match (code, dialog.stage()) {
                (KeyCode::Esc, _) => {
                    self.connect = None;
                    None
                }
                // Provider stage: Up/Down scroll the list, chars filter.
                (KeyCode::Up, ConnectStage::Provider) => {
                    dialog.prev();
                    None
                }
                (KeyCode::Down, ConnectStage::Provider) => {
                    dialog.next();
                    None
                }
                (KeyCode::Home, ConnectStage::Provider) => {
                    dialog.first();
                    None
                }
                (KeyCode::End, ConnectStage::Provider) => {
                    dialog.last();
                    None
                }
                (KeyCode::PageUp, ConnectStage::Provider) => {
                    dialog.page_up();
                    None
                }
                (KeyCode::PageDown, ConnectStage::Provider) => {
                    dialog.page_down();
                    None
                }
                (KeyCode::Backspace, ConnectStage::Provider) => {
                    dialog.pop_filter_char();
                    None
                }
                (KeyCode::Char(c), ConnectStage::Provider) => {
                    dialog.push_filter_char(c);
                    None
                }
                // Form stage: field navigation, type cycling, text editing.
                (KeyCode::Up, ConnectStage::ApiKey) => {
                    dialog.focus_prev();
                    None
                }
                (KeyCode::Down | KeyCode::Tab, ConnectStage::ApiKey) => {
                    dialog.focus_next();
                    None
                }
                (KeyCode::Left, ConnectStage::ApiKey)
                    if dialog.focused_field() == ConnectField::Type =>
                {
                    dialog.cycle_type(false);
                    None
                }
                (KeyCode::Right, ConnectStage::ApiKey)
                    if dialog.focused_field() == ConnectField::Type =>
                {
                    dialog.cycle_type(true);
                    None
                }
                (KeyCode::Left, ConnectStage::ApiKey)
                    if dialog.focused_field() == ConnectField::Model =>
                {
                    dialog.cycle_model(false);
                    None
                }
                (KeyCode::Right, ConnectStage::ApiKey)
                    if dialog.focused_field() == ConnectField::Model =>
                {
                    dialog.cycle_model(true);
                    None
                }
                (KeyCode::Backspace, ConnectStage::ApiKey) => {
                    dialog.backspace();
                    None
                }
                (KeyCode::Char(c), ConnectStage::ApiKey) => {
                    dialog.input_char(c);
                    None
                }
                (KeyCode::Enter, _) => dialog.confirm(),
                _ => None,
            }
        };

        if let Some(action) = action {
            self.connect = None;
            self.apply_connect(action, agent_tx).await;
        }
    }

    async fn apply_settings(
        &mut self,
        action: SettingsAction,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match action {
            SettingsAction::SetTheme(id) => {
                self.theme = self.theme_store.resolve(Some(&id));
                if let Ok(mut cfg) = ConfigManager::load_global() {
                    cfg.theme = Some(id.clone());
                    let _ = ConfigManager::save_global(&cfg);
                }
                self.toast = Some(Toast::info(format!("{} → {id}", crate::tr("theme"))));
            }
            SettingsAction::SetModel(id) => self.apply_model(&id, agent_tx),
            SettingsAction::SetProvider(name) => {
                // Real hot switch: reassemble the active tab from the named
                // provider, carrying the conversation over. Commit only on
                // success (else the template/status would drift from reality).
                match self.template.with_provider(&name) {
                    Some(t) => {
                        self.start_reassemble_active(
                            t,
                            ReassembleEffect::Notify(ReassembleNotify::Toast(format!(
                                "{} → {name}",
                                crate::tr("provider")
                            ))),
                            agent_tx,
                        );
                    }
                    None => {
                        self.toast = Some(Toast::error(
                            crate::tr("no provider '{name}' in config").replace("{name}", &name),
                        ));
                    }
                }
            }
            SettingsAction::SetMode(m) => {
                // Map the approval mode to yolo: "dontAsk" auto-approves.
                let yolo = m == "dontAsk";
                let t = self.template.with_yolo(yolo);
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Yolo {
                        access: if yolo {
                            zode_core::ToolAccessMode::Auto
                        } else {
                            zode_core::ToolAccessMode::Prompt
                        },
                        notify: ReassembleNotify::Toast(format!("{} → {m}", crate::tr("mode"))),
                    },
                    agent_tx,
                );
            }
            SettingsAction::SetEffort(level) => {
                let t = self.template.with_effort(Some(level.clone()));
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Effort {
                        notify: ReassembleNotify::Toast(format!(
                            "{} → {level}",
                            crate::tr("effort")
                        )),
                    },
                    agent_tx,
                );
            }
            SettingsAction::SetSidebar(choice) => {
                self.sidebar_visibility = match choice.as_str() {
                    "visible" => SidebarVisibility::Visible,
                    "hidden" => SidebarVisibility::Hidden,
                    _ => SidebarVisibility::Auto,
                };
                self.toast = Some(Toast::info(format!("{} → {choice}", crate::tr("sidebar"))));
            }
            SettingsAction::SetThinking(choice) => {
                self.show_thinking = choice == "on";
                self.persist_show_thinking(self.show_thinking);
                self.toast = Some(Toast::info(format!(
                    "{} {}",
                    crate::tr("thinking output"),
                    on_off(self.show_thinking)
                )));
            }
            SettingsAction::SetToolDetails(choice) => {
                self.show_tool_details = choice == "on";
                self.persist_show_tool_details(self.show_tool_details);
                self.toast = Some(Toast::info(format!(
                    "{} {}",
                    crate::tr("tool details"),
                    on_off(self.show_tool_details)
                )));
            }
            SettingsAction::SetOrchestration(choice) => {
                let on = choice == "on";
                let t = self.template.with_autonomous_orchestration(on);
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Orchestration {
                        on,
                        notify: ReassembleNotify::Toast(format!(
                            "{} {}",
                            crate::tr("autonomous orchestration"),
                            on_off(on)
                        )),
                    },
                    agent_tx,
                );
            }
            SettingsAction::SetLanguage(code) => {
                if zode_core::i18n::set_language_code(&code) {
                    if let Ok(mut cfg) = ConfigManager::load_global() {
                        cfg.language = Some(code.clone());
                        let _ = ConfigManager::save_global(&cfg);
                    }
                    let name = zode_core::i18n::Lang::from_code(&code)
                        .map(|l| l.native_name())
                        .unwrap_or(code.as_str());
                    self.toast = Some(Toast::info(format!("{} → {name}", crate::tr("language"))));
                }
            }
            SettingsAction::SetSandbox(action) => {
                self.apply_sandbox_action(&action, agent_tx).await;
            }
            SettingsAction::SetVisionProvider(provider) => {
                self.apply_vision_provider(&provider);
            }
            SettingsAction::SetCurrency(code) => {
                // Switch the display currency in place (no engine rebuild),
                // refresh the shown cost, and persist for future sessions.
                let applied = self.active_tab().engine.cost.set_currency(&code);
                let label = self.active_tab().engine.cost.sidebar_label().await;
                self.active_tab_mut().cost_label = label;
                if let Ok(mut cfg) = ConfigManager::load_global() {
                    cfg.currency = Some(applied.to_string());
                    let _ = ConfigManager::save_global(&cfg);
                }
                self.toast = Some(Toast::info(format!(
                    "{} → {applied}",
                    crate::tr("currency")
                )));
            }
        }
    }

    async fn apply_connect(
        &mut self,
        action: ConnectAction,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't switch provider during a turn - Ctrl+C first",
            )));
            return;
        }

        let mut cfg = match ConfigManager::load_global() {
            Ok(cfg) => cfg,
            Err(e) => {
                self.toast = Some(Toast::error(format!(
                    "{}: {e}",
                    crate::tr("load config failed")
                )));
                return;
            }
        };
        // Group the connected model under its provider in the `providers` map
        // (shared credentials, one entry per provider) and set it active.
        let provider = action.provider.clone();
        let provider_key = action.provider_key.clone();
        cfg.connect_provider(
            &action.provider_key,
            provider.clone(),
            action.model_override,
        );
        if let Err(e) = ConfigManager::save_global(&cfg) {
            self.toast = Some(Toast::error(format!(
                "{}: {e}",
                crate::tr("save config failed")
            )));
            return;
        }

        // Flash the group key the rest of the UI calls this provider (status
        // bar, /vision) — action.name may be a model id when one was typed —
        // and the pinned model separately.
        let provider_name = action.provider_key.clone();
        let model = action.provider.model.clone();
        // Carry the just-saved providers map onto the template so the status
        // bar's `model(provider)` label resolves the freshly connected group.
        let t = self
            .template
            .with_provider_config_for_key(provider, provider_key)
            .with_providers_map(cfg.providers.clone());
        self.start_reassemble_active(
            t,
            ReassembleEffect::Connect {
                provider_name,
                model,
            },
            agent_tx,
        );
    }

    /// Drive the plugin picker. Space/Enter flips the selected plugin in place;
    /// Esc closes and, if anything changed, persists the new disabled set and
    /// reassembles the active tab once so it takes effect live.
    async fn handle_plugin_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match code {
            KeyCode::Esc => {
                let Some(picker) = self.plugin_picker.take() else {
                    return;
                };
                if picker.is_dirty() {
                    self.apply_plugins(
                        picker.disabled_ids(),
                        picker.all_ids(),
                        picker.package_changes(),
                        agent_tx,
                    )
                    .await;
                }
            }
            KeyCode::Up => {
                if let Some(p) = &mut self.plugin_picker {
                    p.prev();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.plugin_picker {
                    p.next();
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some((name, on)) = self
                    .plugin_picker
                    .as_mut()
                    .and_then(PluginPicker::toggle_selected)
                {
                    let state = if on {
                        crate::tr("on")
                    } else {
                        crate::tr("off")
                    };
                    self.toast = Some(Toast::info(format!("{name}: {state}")));
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = &mut self.plugin_picker {
                    p.pop_filter_char();
                }
            }
            KeyCode::Char(c) => {
                if let Some(p) = &mut self.plugin_picker {
                    p.push_filter_char(c);
                }
            }
            _ => {}
        }
    }

    /// Drive the `/browser` panel. Up/Down move the selection; Enter applies
    /// the selected row's action (may reassemble the active tab, for
    /// `ToggleDefault`); Esc closes it.
    async fn handle_browser_panel_key(
        &mut self,
        code: KeyCode,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match code {
            KeyCode::Esc => self.browser_panel = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.browser_panel {
                    p.prev();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.browser_panel {
                    p.next();
                }
            }
            KeyCode::Enter => {
                let Some(action) = self.browser_panel.as_ref().and_then(BrowserPanel::confirm)
                else {
                    return;
                };
                self.apply_browser_panel_action(action, agent_tx).await;
            }
            _ => {}
        }
    }

    /// Apply one `/browser` panel action, then refresh the panel's status
    /// header so the change is visible without closing it.
    async fn apply_browser_panel_action(
        &mut self,
        action: BrowserPanelAction,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let mut status = self.browser_panel_status();
        match action {
            BrowserPanelAction::SelectTarget => {
                let engine = self.active_tab().engine.clone();
                let next = match engine.browser.target() {
                    zode_core::browser::BrowserTarget::Managed => {
                        zode_core::browser::BrowserTarget::Bridge
                    }
                    zode_core::browser::BrowserTarget::Bridge => {
                        zode_core::browser::BrowserTarget::Managed
                    }
                };
                let want_bridge = matches!(next, zode_core::browser::BrowserTarget::Bridge);
                let target_name = if want_bridge { "bridge" } else { "managed" };
                let persisted = ConfigManager::persist_browser_default_target(target_name);
                match persisted.and_then(|()| {
                    engine
                        .browser
                        .set_target(next)
                        .map_err(|error| zode_core::CoreError::Other(error.to_string()))
                }) {
                    // set_target mutates the shared session synchronously (no
                    // reassembly involved), so a fresh snapshot already
                    // reflects it.
                    Ok(()) => {
                        if want_bridge {
                            ensure_browser_bridge_and_maybe_reconnect(engine.browser.clone()).await;
                        }
                        status = self.browser_panel_status();
                    }
                    Err(e) => self
                        .active_tab_mut()
                        .chat
                        .push_system(&format!("{}: {e}", crate::tr("save config failed"))),
                }
            }
            BrowserPanelAction::ManagePermissions => {
                let flags = self.active_tab().engine.browser.perm_flags();
                for (_, flag) in &flags {
                    flag.store(false, std::sync::atomic::Ordering::SeqCst);
                }
                let msg = if flags.is_empty() {
                    "no always-allow grants yet".to_string()
                } else {
                    let names: Vec<&str> = flags.iter().map(|(n, _)| n.as_str()).collect();
                    format!("reset always-allow for: {}", names.join(", "))
                };
                self.active_tab_mut().chat.push_system(&msg);
            }
            BrowserPanelAction::Reconnect => {
                self.start_browser_pairing(agent_tx).await;
                status = self.browser_panel_status();
            }
            BrowserPanelAction::ToggleDefault => {
                if self.active_tab().is_busy() {
                    self.toast = Some(Toast::info(crate::tr(
                        "can't switch during a turn — Ctrl+C first",
                    )));
                    return;
                }
                let enabled = !status.default_enabled;
                if let Err(e) = ConfigManager::persist_browser_enabled(enabled) {
                    self.toast = Some(Toast::error(format!(
                        "{}: {e}",
                        crate::tr("save config failed")
                    )));
                    return;
                }
                let t = self.template.with_browser_enabled(enabled);
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Notify(ReassembleNotify::Toast(
                        crate::tr("browser config updated").to_string(),
                    )),
                    agent_tx,
                );
                // Reassembly lands asynchronously; reflect the saved intent
                // immediately while retaining any independent plugin disable.
                status.default_enabled = enabled;
                status.group_enabled =
                    enabled && self.active_tab().engine.plugins.is_enabled("tools:browser");
            }
        }
        if let Some(p) = &mut self.browser_panel {
            p.set_status(status);
        }
    }

    /// Persist the disabled-plugin set to the global config and reassemble the
    /// active tab so the change (tools dropped, MCP disconnected, skills hidden)
    /// applies to the running conversation.
    ///
    /// `disabled` is the off-set the picker showed; `owned` is every id it
    /// presented. Disabled ids in config but NOT in `owned` (e.g. a
    /// project-scoped MCP server or skill from a different workspace, or the
    /// not-yet-shown `lsp:*` rows) are preserved verbatim — replacing the whole
    /// list with just `disabled` would silently re-enable them.
    ///
    /// `packages` are installed plugin packages the user flipped. Those live in
    /// the install registry (enabling moves the package directory back under
    /// `plugins/`), never in `plugins.disabled`.
    async fn apply_plugins(
        &mut self,
        disabled: Vec<String>,
        owned: Vec<String>,
        packages: Vec<(String, bool)>,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't change plugins during a turn — Ctrl+C first",
            )));
            return;
        }
        // A package that failed to flip (its directory is gone, or the
        // destination already exists) must not abort the rest: whatever DID
        // change still has to reach the running tab. Report it after the
        // reassemble instead of the success toast.
        let package_error = self.apply_plugin_packages(packages);
        let owned: std::collections::HashSet<String> = owned.into_iter().collect();
        let merged = match ConfigManager::load_global() {
            Ok(mut cfg) => {
                let mut next: Vec<String> = cfg
                    .plugins
                    .disabled
                    .iter()
                    .filter(|id| !owned.contains(id.as_str()))
                    .cloned()
                    .collect();
                next.extend(disabled);
                next.sort();
                next.dedup();
                cfg.plugins.disabled = next.clone();
                if let Err(e) = ConfigManager::save_global(&cfg) {
                    self.toast = Some(Toast::error(format!(
                        "{}: {e}",
                        crate::tr("save config failed")
                    )));
                    return;
                }
                next
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!(
                    "{}: {e}",
                    crate::tr("load config failed")
                )));
                return;
            }
        };
        let t = self.template.with_plugins_disabled(merged);
        let notice = package_error.unwrap_or_else(|| crate::tr("plugins updated").to_string());
        self.start_reassemble_active(
            t,
            ReassembleEffect::Notify(ReassembleNotify::Toast(notice)),
            agent_tx,
        );
    }

    /// Flip installed plugin packages in the install registry. Returns the
    /// first failure's message; the remaining packages are still attempted, so
    /// one bad row can't strand the others.
    fn apply_plugin_packages(&self, packages: Vec<(String, bool)>) -> Option<String> {
        if packages.is_empty() {
            return None;
        }
        let manager = match zode_core::plugin_package::PluginPackageManager::open_default() {
            Ok(manager) => manager,
            Err(e) => return Some(format!("{}: {e}", crate::tr("plugin update failed"))),
        };
        let mut error = None;
        for (name, enabled) in packages {
            if let Err(e) = manager.set_enabled(&name, enabled) {
                error.get_or_insert(format!(
                    "{}: {name}: {e}",
                    crate::tr("plugin update failed")
                ));
            }
        }
        error
    }

    fn apply_completion(&mut self) {
        self.completion_hint = None;
        if let Some(completion) = self.autocomplete.confirm() {
            self.input.take();
            self.input.insert_str(&completion.insert);
            self.completion_hint = completion.placeholder.map(|placeholder| CompletionHint {
                prefix: completion.insert,
                placeholder: placeholder.to_string(),
            });
        }
        self.autocomplete.dismiss();
    }

    /// Whether a non-empty `@`-mention picker is open (so it should intercept
    /// navigation keys). An open-but-empty picker (query matches nothing) does
    /// not capture keys — Enter still submits the turn.
    fn mention_active(&self) -> bool {
        self.active_mention.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Re-sync the `@`-mention picker against the current input. Builds the
    /// candidate set the first time `@` appears as the trailing token (one cwd
    /// walk per mention session), then only re-filters as the query changes.
    fn refresh_mention(&mut self) {
        let text = self.input.text();
        match at_mention_query(&text) {
            Some(query) => {
                if let Some(picker) = &mut self.active_mention {
                    picker.filter(query);
                } else {
                    let items = self.build_mention_items();
                    self.active_mention = Some(MentionPicker::new(items, query));
                }
            }
            None => self.active_mention = None,
        }
    }

    /// Gather `@`-mention candidates from the active tab: skills and MCP servers
    /// first (few, kept visible at the top of the empty-query list), then cwd
    /// files (found by typing).
    fn build_mention_items(&self) -> Vec<MentionItem> {
        let eng = &self.active_tab().engine;
        let mut items = Vec::new();
        for s in eng.skills.list() {
            items.push(MentionItem {
                insert: s.name.clone(),
                detail: s
                    .description
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(60)
                    .collect(),
                kind: MentionKind::Skill,
            });
        }
        if let Some(lc) = &eng.mcp {
            for server in lc.registry.snapshot() {
                items.push(MentionItem {
                    insert: server.name.clone(),
                    detail: String::new(),
                    kind: MentionKind::Mcp,
                });
            }
        }
        let cwd = eng.cwd.clone();
        for rel in collect_cwd_files(&cwd, 1000) {
            items.push(MentionItem {
                insert: rel,
                detail: String::new(),
                kind: MentionKind::File,
            });
        }
        items
    }

    /// Replace the trailing `@query` token with the selected reference (a bare
    /// path for files; a name for skills/MCP), keeping the leading `@` so it
    /// reads as a mention, then append a space and close the picker.
    fn apply_mention(&mut self) {
        let Some(picker) = self.active_mention.take() else {
            return;
        };
        let Some(insert) = picker.selected_insert().map(str::to_string) else {
            return;
        };
        let text = self.input.text();
        let new_text = match at_mention_query(&text) {
            // `@query` is the trailing token; `query.len() + 1` covers the `@`.
            Some(query) => {
                let prefix = &text[..text.len().saturating_sub(query.len() + 1)];
                format!("{prefix}@{insert} ")
            }
            None => format!("{text}@{insert} "),
        };
        self.input.take();
        self.input.insert_str(&new_text);
        self.completion_hint = None;
    }

    fn apply_model(&mut self, id: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't switch during a turn — Ctrl+C first",
            )));
            return;
        }

        let tab_idx = self.active;
        let global_candidate = self.template.with_model(id.to_string());
        let engine_template = global_candidate
            .with_tool_access(self.tabs[tab_idx].extension_access)
            .with_plan_mode(self.tabs[tab_idx].plan_mode);
        let hot_result = {
            let tab = &mut self.tabs[tab_idx];
            match Arc::get_mut(&mut tab.engine) {
                Some(engine) => engine_template
                    .hot_swap_model(engine, id.to_string())
                    .map(Some),
                None => Ok(None),
            }
        };

        match hot_result {
            Ok(Some(_effective_template)) => {
                self.template = global_candidate;
                self.apply_model_effect(tab_idx, id);
            }
            Ok(None) => {
                self.start_reassemble_active(
                    global_candidate,
                    ReassembleEffect::Model { id: id.to_string() },
                    agent_tx,
                );
            }
            Err(e) => {
                self.tabs[tab_idx]
                    .chat
                    .push_system(&format!("{}: {e}", crate::tr("switch failed")));
                self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("switch failed"))));
            }
        }
    }

    fn apply_goal(&mut self, new_goal: Option<String>, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let tab_idx = self.active;
        if new_goal.is_none() {
            let was_looping = self.tabs[tab_idx].goal_loop_active;
            stop_goal_loop(&mut self.tabs[tab_idx]);
            if was_looping {
                self.interrupt_active_turn();
            }
        }

        if self.active_tab().is_busy() {
            if new_goal.is_none() {
                self.active_tab_mut().chat.push_system(crate::tr(
                    "can't clear the goal text during a turn — run /goal clear again when idle",
                ));
            } else {
                self.toast = Some(Toast::info(crate::tr(
                    "can't set goal during a turn — Ctrl+C first",
                )));
            }
            return;
        }

        let global_candidate = self.template.with_goal(new_goal.clone());
        let engine_template = global_candidate
            .with_model(self.tabs[tab_idx].engine.model.clone())
            .with_tool_access(self.tabs[tab_idx].extension_access)
            .with_plan_mode(self.tabs[tab_idx].plan_mode);
        let hot_swapped = {
            let tab = &mut self.tabs[tab_idx];
            Arc::get_mut(&mut tab.engine)
                .map(|engine| engine_template.hot_swap_goal(engine, new_goal.clone()))
        };

        if hot_swapped.is_some() {
            self.template = global_candidate;
            self.apply_goal_effect(tab_idx, new_goal);
            return;
        }

        if !self.start_reassemble_active(
            global_candidate,
            ReassembleEffect::Goal {
                goal: new_goal.clone(),
            },
            agent_tx,
        ) && new_goal.is_none()
        {
            self.active_tab_mut().chat.push_system(crate::tr(
                "can't clear the goal text during a turn — run /goal clear again when idle",
            ));
        }
    }

    /// When the active tab goes idle, send the next queued message (one per
    /// turn, FIFO). Called after each agent event, so it fires as soon as a
    /// turn's `TurnDone` clears the busy flag.
    async fn dispatch_queued_input(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.should_quit || self.active_tab().is_busy() {
            return;
        }
        if self.queued_edit_index == Some(0) {
            return;
        }
        let tab_id = self.active_tab().id;
        let pending = self.active_tab().queued_input.front().and_then(|prompt| {
            let key = (tab_id, prompt.clone());
            self.sched_pending
                .get(&key)
                .and_then(|jobs| jobs.front())
                .cloned()
                .map(|job| (key, job))
        });
        if let Some((key, job)) = pending.as_ref() {
            match self.queued_schedule_claim_is_runnable(job) {
                Ok(true) => {}
                Ok(false) => {
                    self.active_tab_mut().queued_input.pop_front();
                    self.cancel_sched_pending_if_front(key, job);
                    self.active_tab_mut().chat.push_system(
                        "scheduler: queued occurrence was disabled or lost ownership before start",
                    );
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to validate queued schedule; keeping it queued");
                    return;
                }
            }
        }
        let next = self.active_tab_mut().queued_input.pop_front();
        if let Some(text) = next {
            let started = if let Some((_, job)) = pending.as_ref() {
                self.submit_scheduler_occurrence(&text, job, agent_tx).await
            } else {
                self.submit(&text, agent_tx).await
            };
            let deferred = !started
                && pending.as_ref().is_some_and(|(key, job)| {
                    self.sched_pending.get(key).and_then(|jobs| jobs.front()) == Some(job)
                });
            if deferred {
                self.active_tab_mut().queued_input.push_front(text);
            } else {
                if let Some(index) = self.queued_edit_index.as_mut() {
                    *index = index.saturating_sub(1);
                }
                if !started {
                    if let Some((key, job)) = pending {
                        self.cancel_sched_pending_if_front(&key, &job);
                    }
                }
            }
        }
    }

    /// Run a `!<cmd>` shell escape (no agent turn) OFF the event loop: echo the
    /// command immediately, spawn the process, and post the output back as a
    /// `LocalShellDone` event — run inline it froze the whole TUI for up to the
    /// 20s timeout. On an idle tab it takes the turn-busy slot, so a follow-up
    /// prompt queues behind it (the output is prepended as context) and Esc
    /// kills the child. On a busy tab (agent turn / op call in flight) it runs
    /// concurrently without touching the slot — same immediacy as the old
    /// inline version, minus the freeze.
    fn spawn_local_shell(&mut self, cmd: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let cwd = self.active_tab().engine.cwd.clone();
        let tab_id = self.active_tab().id;
        self.active_tab_mut().chat.push_system(&format!("$ {cmd}"));
        let owned_slot = !self.active_tab().is_busy();
        let (op_id, abort) = if owned_slot {
            // Reuse the turn-busy machinery: spinner shows, prompts queue, and
            // Esc (interrupt_active_turn) aborts — the select below sees it
            // and the child dies with the dropped future (kill_on_drop).
            let Some((op_id, abort)) = self.begin_local_operation(self.active) else {
                return;
            };
            (Some(op_id), abort)
        } else {
            // Concurrent shells never own or release the tab's shared slot.
            (None, AbortController::new())
        };
        let cmd = cmd.to_string();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let output = tokio::select! {
                out = run_shell_capture(&cmd, &cwd) => Some(out),
                _ = abort.cancelled() => None,
            };
            let _ = tx.send(AppEvent::LocalShellDone {
                tab_id,
                cmd,
                output,
                op_id,
            });
        });
    }

    /// Reconcile persisted schedules written by other zode processes without
    /// turning a transient read error into an empty authoritative roster.
    /// This also imports retry tokens and recovers active tokens whose OS
    /// owner disappeared after this process started.
    fn refresh_persisted_schedule_roster(&mut self, now: std::time::Instant) {
        if now.saturating_duration_since(self.last_schedule_roster_refresh)
            < SCHEDULE_ROSTER_REFRESH_INTERVAL
        {
            return;
        }
        self.last_schedule_roster_refresh = now;
        let mut roster = match zode_core::scheduler::try_load_schedules() {
            Ok(roster) => roster,
            Err(error) => {
                tracing::warn!(%error, "failed to reconcile persisted schedules");
                return;
            }
        };

        let mut locally_owned = self
            .pending_schedule_leases
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        locally_owned.extend(self.tabs.iter().filter_map(|tab| {
            tab.watchdog_attempt_lease
                .as_ref()
                .map(|lease| lease.schedule_id().to_string())
        }));
        locally_owned.extend(self.forced_turn_stops.values().filter_map(|pending| {
            pending
                .attempt_lease
                .as_ref()
                .map(|lease| lease.schedule_id().to_string())
        }));
        locally_owned.extend(
            self.pending_schedule_finalizers
                .keys()
                .map(|(id, _)| id.clone()),
        );

        let orphan_candidates: Vec<(String, u64)> = roster
            .iter()
            .filter_map(|schedule| {
                let token = schedule.watchdog_active_since_ms?;
                (!locally_owned.contains(&schedule.id)).then(|| (schedule.id.clone(), token))
            })
            .collect();
        for (id, token) in orphan_candidates {
            match zode_core::scheduler::recover_orphaned_watchdog_attempt(&id, token) {
                Ok(zode_core::scheduler::OrphanAttemptRecovery::Recovered(updated)) => {
                    roster = updated;
                    self.watchdog.cancel_job(&SchedJobRef::Schedule(id.clone()));
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.chat.push_system(&format!(
                            "watchdog: schedule {id} lost its owner; disabled for manual review"
                        ));
                    }
                }
                Ok(zode_core::scheduler::OrphanAttemptRecovery::Live)
                | Ok(zode_core::scheduler::OrphanAttemptRecovery::Stale) => {}
                Err(error) => {
                    tracing::warn!(%error, schedule_id = %id, "failed to inspect runtime schedule owner");
                }
            }
        }

        self.scheduler.set_schedules(roster.clone());
        let runnable_ids: HashSet<String> = roster
            .iter()
            .filter(|schedule| schedule.enabled)
            .map(|schedule| schedule.id.clone())
            .collect();
        let stale_queued_ids: HashSet<String> = self
            .sched_pending
            .values()
            .flat_map(|jobs| jobs.iter())
            .filter_map(|job| match job {
                SchedJobRef::Schedule(id) if !runnable_ids.contains(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        for id in stale_queued_ids {
            let job = SchedJobRef::Schedule(id);
            self.purge_sched_jobs(|candidate| candidate == &job);
        }
        for id in self.watchdog.occupied_schedule_ids() {
            if !runnable_ids.contains(&id) {
                let job = SchedJobRef::Schedule(id);
                self.watchdog
                    .cancel_recoveries_matching(|candidate| candidate == &job);
            }
        }

        let now_epoch_ms = current_epoch_ms();
        let retry_tab_id = self.tabs[self.active].id;
        for schedule in roster {
            let Some(retry_token_ms) = schedule.watchdog_retry_at_ms else {
                continue;
            };
            let job = SchedJobRef::Schedule(schedule.id.clone());
            if !schedule.enabled
                || schedule.watchdog_active_since_ms.is_some()
                || schedule.watchdog_failures == 0
                || self.watchdog.job_is_occupied(&job)
                || self.sched_job_is_pending(&job)
                || self.sched_job_has_pending_lease(&job)
            {
                continue;
            }
            let delay = Duration::from_millis(retry_token_ms.saturating_sub(now_epoch_ms));
            let Some(due_at) = now.checked_add(delay) else {
                tracing::warn!(schedule_id = %schedule.id, "watchdog retry deadline is out of range");
                continue;
            };
            self.watchdog.restore_retry(
                job,
                retry_tab_id,
                schedule.prompt,
                schedule.watchdog_failures,
                due_at,
                retry_token_ms,
            );
        }
    }

    /// Once-per-tick scheduler poll: ask `Scheduler::due` what's ready to fire
    /// and queue each due prompt onto its owning tab, same injection path as
    /// a user typing while busy (`SessionTab::queued_input`). Draining that
    /// queue back into a turn is NOT this function's job: `dispatch_queued_input`
    /// covers the active tab whenever the user or an agent event wakes the loop,
    /// and `dispatch_scheduler_queued` covers EVERY tab (active included) from
    /// the tick, so a due prompt runs unattended.
    fn poll_scheduler(&mut self) {
        let now = std::time::Instant::now();
        self.refresh_persisted_schedule_roster(now);
        let mut blocked_loops = self.watchdog.occupied_loop_ids();
        blocked_loops.extend(
            self.sched_pending
                .values()
                .flat_map(|jobs| jobs.iter())
                .filter_map(|job| match job {
                    SchedJobRef::Loop(id) => Some(*id),
                    SchedJobRef::Schedule(_) => None,
                }),
        );
        blocked_loops.extend(
            self.tabs
                .iter()
                .filter_map(|tab| tab.active_sched_job.as_ref())
                .filter_map(|job| match job {
                    SchedJobRef::Loop(id) => Some(*id),
                    SchedJobRef::Schedule(_) => None,
                }),
        );
        let mut blocked_schedules = self.watchdog.occupied_schedule_ids();
        blocked_schedules.extend(
            self.scheduler
                .schedules()
                .iter()
                .filter(|schedule| {
                    schedule.watchdog_active_since_ms.is_some()
                        || schedule.watchdog_retry_at_ms.is_some()
                })
                .map(|schedule| schedule.id.clone()),
        );
        blocked_schedules.extend(self.pending_schedule_leases.keys().cloned());
        blocked_schedules.extend(
            self.pending_schedule_finalizers
                .keys()
                .map(|(id, _)| id.clone()),
        );
        blocked_schedules.extend(
            self.sched_pending
                .values()
                .flat_map(|jobs| jobs.iter())
                .filter_map(|job| match job {
                    SchedJobRef::Loop(_) => None,
                    SchedJobRef::Schedule(id) => Some(id.clone()),
                }),
        );
        blocked_schedules.extend(self.tabs.iter().filter_map(|tab| {
            match tab.active_sched_job.as_ref() {
                Some(SchedJobRef::Schedule(id)) => Some(id.clone()),
                _ => None,
            }
        }));
        let wall_now = chrono::Local::now();
        let due = self.scheduler.due_candidates_with_blocked_jobs(
            now,
            wall_now.naive_local(),
            wall_now.timestamp_millis().max(0) as u64,
            &blocked_loops,
            &blocked_schedules,
        );
        for job in due {
            let job_ref = match &job.kind {
                DueKind::Loop { id, .. } => SchedJobRef::Loop(*id),
                DueKind::Schedule { id, .. } => SchedJobRef::Schedule(id.clone()),
            };
            // Anti-pileup is job-identity based, not prompt-text based. Two
            // distinct jobs may intentionally use the same prompt, while one
            // logical job must never own two queued/running occurrences.
            if self.watchdog.job_is_occupied(&job_ref)
                || self.sched_job_is_pending(&job_ref)
                || self.sched_job_has_pending_lease(&job_ref)
                || self
                    .tabs
                    .iter()
                    .any(|tab| tab.active_sched_job.as_ref() == Some(&job_ref))
            {
                continue;
            }
            let tab_idx = match &job.kind {
                DueKind::Loop { owner, .. } => self.tabs.iter().position(|t| t.id as u64 == *owner),
                DueKind::Schedule { .. } => Some(self.active),
            };
            let Some(tab_idx) = tab_idx else {
                continue; // owning tab was closed
            };
            let key = (self.tabs[tab_idx].id, job.prompt.clone());
            let attributed = self.sched_pending.get(&key).map_or(0, VecDeque::len);
            let queued_matches = self.tabs[tab_idx]
                .queued_input
                .iter()
                .filter(|candidate| *candidate == &job.prompt)
                .count();
            // Do not put scheduler attribution behind an equal-text user
            // message: without metadata inside `queued_input`, that earlier
            // occurrence must remain unambiguously user-owned. Equal counts
            // mean every existing match belongs to the FIFO attribution list,
            // so another distinct scheduler job can safely append its copy.
            if queued_matches != attributed {
                continue;
            }

            // Claim the exact fire slot and its attempt lease in one store
            // transaction before enqueueing. The returned OS lock remains in
            // `pending_schedule_leases`, fencing later cadences even while a
            // busy tab delays this turn.
            let claimed_lease = if let DueKind::Schedule {
                id, fire_epoch_ms, ..
            } = &job.kind
            {
                let lease = match zode_core::scheduler::try_claim_watchdog_fire(id, *fire_epoch_ms)
                {
                    Ok(Some(lease)) => lease,
                    Ok(None) => {
                        self.reconcile_failed_schedule_claim(id, tab_idx);
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(%error, schedule_id = %id, "failed to claim schedule fire");
                        continue;
                    }
                };
                let active_token_ms = lease.active_token_ms();
                self.reload_schedule_roster();
                self.scheduler.mark_watchdog_active(id, active_token_ms);
                Some((id.clone(), lease))
            } else {
                None
            };
            self.push_sched_pending(key, job_ref);
            self.tabs[tab_idx].queued_input.push_back(job.prompt);
            if let Some((id, lease)) = claimed_lease {
                let replaced = self.pending_schedule_leases.insert(
                    id,
                    PendingScheduleLease {
                        lease,
                        origin: PendingScheduleOrigin::Fire,
                        queued_at: now,
                    },
                );
                debug_assert!(
                    replaced.is_none(),
                    "schedule identity guard prevents replacement"
                );
            }
        }
    }

    /// Advance unattended-turn liveness, request cooperative aborts, and
    /// hard-stop a tab when an aborted provider task never drains. Its slot
    /// and lease remain fenced until tracked nested workers also quiesce.
    fn poll_queued_watchdog(&mut self) {
        if !self.watchdog.enabled() {
            return;
        }
        let now = std::time::Instant::now();
        let timeout = self.watchdog.queue_start_timeout();
        let overdue: Vec<SchedJobRef> = self
            .sched_queued_at
            .iter()
            .filter_map(|(job, queued_at)| {
                let queued_at = match job {
                    SchedJobRef::Schedule(id) => self
                        .pending_schedule_leases
                        .get(id)
                        .map(|pending| pending.queued_at)
                        .unwrap_or(*queued_at),
                    SchedJobRef::Loop(_) => *queued_at,
                };
                (now.saturating_duration_since(queued_at) >= timeout).then(|| job.clone())
            })
            .collect();

        for job in overdue {
            let Some((tab_id, prompt)) = self.take_sched_pending_occurrence(&job) else {
                self.sched_queued_at.remove(&job);
                continue;
            };
            let attempt_lease = match &job {
                SchedJobRef::Schedule(id) => {
                    let Some(pending) = self.pending_schedule_leases.remove(id) else {
                        tracing::error!(schedule_id = %id, "queued watchdog timeout had no attempt lease");
                        self.watchdog.cancel_job(&job);
                        continue;
                    };
                    if let Some(schedule) = self
                        .scheduler
                        .schedules()
                        .iter()
                        .find(|schedule| &schedule.id == id)
                    {
                        self.watchdog
                            .seed_failures(&job, schedule.watchdog_failures);
                    }
                    Some(pending.lease)
                }
                SchedJobRef::Loop(_) => None,
            };
            let failure = self
                .watchdog
                .fail_queued(job, tab_id, prompt, std::time::Instant::now());
            self.apply_watchdog_failure(failure, attempt_lease);
        }
    }

    fn watchdog_status_lines(&self, now: std::time::Instant) -> Vec<String> {
        let mut lines = self.watchdog.status_lines(now);
        let timeout = self.watchdog.queue_start_timeout();
        let mut queued: Vec<String> = self
            .sched_queued_at
            .iter()
            .map(|(job, queued_at)| {
                let age = now.saturating_duration_since(*queued_at);
                format!(
                    "{} · queued {}s · start timeout in {}s",
                    watchdog::job_label(job),
                    age.as_secs(),
                    timeout.saturating_sub(age).as_secs(),
                )
            })
            .collect();
        queued.extend(self.pending_schedule_finalizers.values().map(|pending| {
            format!(
                "schedule {} · terminal persistence fenced; retry in {}s",
                pending.lease.schedule_id(),
                pending.retry_at.saturating_duration_since(now).as_secs(),
            )
        }));
        queued.sort();
        lines.extend(queued);
        lines
    }

    fn poll_watchdog(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let now = std::time::Instant::now();
        for action in self.watchdog.poll(now) {
            match action {
                WatchdogAction::Abort {
                    tab_id,
                    turn_id,
                    kind,
                } => {
                    let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                        self.watchdog.forget_turn(tab_id, turn_id);
                        continue;
                    };
                    let Some(interrupt) =
                        self.prepare_tab_interrupt_labeled(tab_idx, Some(turn_id), false)
                    else {
                        // The terminal won the race with this tick or the tab
                        // moved to another generation; never abort that newer
                        // owner and never recover the stale record.
                        self.watchdog.forget_turn(tab_id, turn_id);
                        continue;
                    };
                    self.resolve_extension_approvals_before_tui_interrupt(tab_id, turn_id);
                    if let Some(tab) = self.tabs.get_mut(tab_idx) {
                        tab.chat.push_system(&format!(
                            "watchdog: {} timeout — cancelling turn {turn_id}",
                            kind.label()
                        ));
                    }
                    interrupt
                        .abort
                        .abort_with_reason(format!("watchdog {} timeout", kind.label()));
                }
                WatchdogAction::HardStop {
                    tab_id,
                    turn_id,
                    failure,
                } => {
                    self.begin_forced_turn_stop(
                        tab_id,
                        turn_id,
                        ForcedTurnStop::Watchdog(failure),
                        agent_tx,
                    );
                }
                WatchdogAction::ForceCancel {
                    tab_id,
                    turn_id,
                    job,
                    failure,
                } => {
                    self.begin_forced_turn_stop(
                        tab_id,
                        turn_id,
                        ForcedTurnStop::Manual { job, failure },
                        agent_tx,
                    );
                }
            }
        }
    }

    fn begin_forced_turn_stop(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        outcome: ForcedTurnStop,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            if let ForcedTurnStop::Watchdog(failure) = outcome {
                self.apply_watchdog_failure(failure, None);
            }
            return;
        };
        if tab.draining_turn_id != Some(turn_id) {
            return;
        }
        if self.forced_turn_stops.contains_key(&(tab_id, turn_id)) {
            return;
        }
        let task = tab.turn_task.take();
        let activity = tab.watchdog_activity.take();
        let attempt_lease = tab.watchdog_attempt_lease.take();
        // Capture the recorder + engine so `finish_forced_turn_stop` can
        // journal the terminal record and close the checkpoint even after the
        // tab is removed. Taking the recorder here also prevents the tab-side
        // completion path from firing a duplicate.
        let recorder = tab.watchdog_recorder.take();
        let scheduled_persistence =
            tab.active_sched_job
                .as_ref()
                .map(|_| ScheduledTurnPersistence {
                    session_id: tab.session_id.clone(),
                    title: tab.title.clone(),
                    persisted: tab.persisted_msgs.clone(),
                });
        let persistence_engine = tab.engine.clone();
        self.forced_turn_stops.insert(
            (tab_id, turn_id),
            PendingForcedTurnStop {
                outcome,
                attempt_lease,
                activity: activity.clone(),
                quarantine: None,
                source_terminal_seen: false,
                recorder,
                engine: Some(persistence_engine.clone()),
            },
        );
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + HARD_STOP_QUIESCE_TIMEOUT;
            let mut quarantined = false;
            let mut task = task;
            if let Some(task) = task.as_mut() {
                task.abort();
            }
            let task_stopped = match task.as_mut() {
                Some(task) => tokio::time::timeout_at(deadline, task).await.is_ok(),
                None => true,
            };
            let workers_stopped = if task_stopped {
                match activity.as_ref() {
                    Some(activity) => {
                        tokio::time::timeout_at(deadline, activity.wait_for_quiescence())
                            .await
                            .is_ok()
                    }
                    None => true,
                }
            } else {
                false
            };
            if !task_stopped || !workers_stopped {
                quarantined = true;
                let _ = tx.send(AppEvent::TurnTaskQuarantined {
                    tab_id,
                    turn_id,
                    result: None,
                });
                if !task_stopped {
                    if let Some(task) = task {
                        let _ = task.await;
                    }
                }
                if !workers_stopped {
                    if let Some(activity) = activity {
                        activity.wait_for_quiescence().await;
                    }
                }
            }
            if let Some(persistence) = scheduled_persistence {
                let mut save = Box::pin(crate::tab::persist_session(
                    persistence.session_id,
                    persistence_engine,
                    persistence.title,
                    persistence.persisted,
                    false,
                ));
                let persisted =
                    match tokio::time::timeout(HARD_STOP_QUIESCE_TIMEOUT, &mut save).await {
                        Ok(persisted) => persisted,
                        Err(_) => {
                            if !quarantined {
                                quarantined = true;
                                let _ = tx.send(AppEvent::TurnTaskQuarantined {
                                    tab_id,
                                    turn_id,
                                    result: None,
                                });
                            }
                            save.await
                        }
                    };
                if !persisted && !quarantined {
                    let _ = tx.send(AppEvent::TurnTaskQuarantined {
                        tab_id,
                        turn_id,
                        result: None,
                    });
                }
            }
            let _ = tx.send(AppEvent::TurnTaskStopped { tab_id, turn_id });
        });
    }

    fn quarantine_turn_task(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        canonical_result: Option<Result<(), String>>,
    ) {
        let key = (tab_id, turn_id);
        if !self.forced_turn_stops.contains_key(&key) {
            let Some(mut result) = canonical_result else {
                return;
            };
            if result.is_ok() {
                result = Err(
                    "watchdog quarantine: tracked workers exceeded the quiescence deadline"
                        .to_string(),
                );
            }
            let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                return;
            };
            if tab.active_turn_id != turn_id && tab.draining_turn_id != Some(turn_id) {
                return;
            }
            let Some(job) = tab.active_sched_job.clone() else {
                return;
            };
            // A quarantine is a draining fence even if the source raced away
            // with its abort handle before this event reached the UI.
            tab.active_turn_id = 0;
            tab.draining_turn_id = Some(turn_id);
            self.forced_turn_stops.insert(
                key,
                PendingForcedTurnStop {
                    outcome: ForcedTurnStop::Canonical { job, result },
                    attempt_lease: tab.watchdog_attempt_lease.take(),
                    activity: tab.watchdog_activity.take(),
                    quarantine: None,
                    source_terminal_seen: false,
                    // Canonical: the source worker owns recorder completion.
                    recorder: None,
                    engine: None,
                },
            );
        }

        let Some(pending) = self.forced_turn_stops.get(&key) else {
            return;
        };
        if pending.quarantine.is_some() {
            return;
        }
        let outcome = pending.outcome.clone();
        let active_token_ms = pending
            .attempt_lease
            .as_ref()
            .map(zode_core::scheduler::ScheduleAttemptLease::active_token_ms);
        let job = match &outcome {
            ForcedTurnStop::Watchdog(failure) => failure.job.clone(),
            ForcedTurnStop::Manual { job, .. } | ForcedTurnStop::Canonical { job, .. } => {
                job.clone()
            }
        };
        let current_failures = match &job {
            SchedJobRef::Schedule(id) => self
                .scheduler
                .schedules()
                .iter()
                .find(|schedule| &schedule.id == id)
                .map(|schedule| schedule.watchdog_failures)
                .unwrap_or(0),
            SchedJobRef::Loop(_) => 0,
        };
        let failures = match &outcome {
            ForcedTurnStop::Watchdog(failure) => match failure.recovery {
                Recovery::RetryScheduled { attempt, .. } => attempt,
                Recovery::Exhausted { failures } | Recovery::ManualReview { failures } => failures,
                Recovery::Cancelled => current_failures.saturating_add(1),
            },
            ForcedTurnStop::Manual { .. } | ForcedTurnStop::Canonical { .. } => {
                current_failures.saturating_add(1)
            }
        };
        let last_failure_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let recovery = QuarantinedRecovery {
            job: job.clone(),
            failures,
            last_failure_ms,
        };

        self.watchdog.cancel_job(&job);
        match &job {
            SchedJobRef::Loop(id) => {
                self.scheduler.stop_loop(Some(*id));
            }
            SchedJobRef::Schedule(id) => {
                let active_token_ms = active_token_ms.or_else(|| {
                    self.scheduler
                        .schedules()
                        .iter()
                        .find(|schedule| &schedule.id == id)
                        .and_then(|schedule| schedule.watchdog_active_since_ms)
                });
                if let Some(active_token_ms) = active_token_ms {
                    if let Err(error) = zode_core::scheduler::persist_watchdog_state_for_attempt(
                        id,
                        active_token_ms,
                        failures,
                        Some(last_failure_ms),
                        None,
                        Some(active_token_ms),
                        Some(false),
                    ) {
                        tracing::error!(%error, schedule_id = %id, "failed to persist watchdog quarantine");
                    }
                } else {
                    tracing::error!(schedule_id = %id, "refusing to quarantine a schedule without its active token");
                }
                self.reload_schedule_roster();
            }
        }
        self.purge_sched_jobs(|candidate| candidate == &job);
        if let Some(pending) = self.forced_turn_stops.get_mut(&key) {
            pending.quarantine = Some(recovery);
        }
        let message = format!(
            "watchdog: {} did not quiesce after hard stop; its tab/store is quarantined and the job is disabled until every worker exits",
            watchdog::job_label(&job)
        );
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.mode = Mode::Error;
            tab.chat.push_system(&message);
        } else {
            tracing::error!(job = %watchdog::job_label(&job), "{message}");
        }
    }

    fn manual_review_failure(&self, tab_id: usize, job: SchedJobRef) -> WatchdogFailure {
        let failures = match &job {
            SchedJobRef::Schedule(id) => self
                .scheduler
                .schedules()
                .iter()
                .find(|schedule| &schedule.id == id)
                .map(|schedule| schedule.watchdog_failures.saturating_add(1))
                .unwrap_or(1),
            SchedJobRef::Loop(_) => 1,
        };
        WatchdogFailure {
            job,
            tab_id,
            cause: FailureCause::ManualCancellationUnknown,
            recovery: Recovery::ManualReview { failures },
        }
    }

    fn finish_forced_turn_stop(&mut self, tab_id: usize, turn_id: u64) {
        let Some(PendingForcedTurnStop {
            mut outcome,
            attempt_lease,
            activity,
            quarantine,
            source_terminal_seen,
            recorder,
            engine,
        }) = self.forced_turn_stops.remove(&(tab_id, turn_id))
        else {
            return;
        };
        let unsafe_manual_job = match &outcome {
            ForcedTurnStop::Manual { job, failure }
                if failure.is_none()
                    && activity.as_ref().is_some_and(|activity| {
                        activity.side_effect_risk() || activity.unresolved_external_work()
                    }) =>
            {
                Some(job.clone())
            }
            _ => None,
        };
        if let Some(job) = unsafe_manual_job {
            let generated = self.manual_review_failure(tab_id, job);
            if let ForcedTurnStop::Manual { failure, .. } = &mut outcome {
                *failure = Some(generated);
            }
        }
        let result: Result<(), String> = if quarantine.is_some() {
            Err("watchdog quarantine: tracked workers exceeded the quiescence deadline".into())
        } else {
            match &outcome {
                ForcedTurnStop::Watchdog(_) => {
                    Err("watchdog hard stop completed after cancellation grace".into())
                }
                ForcedTurnStop::Manual { .. } => {
                    Err("user interruption hard stop completed".into())
                }
                ForcedTurnStop::Canonical { result, .. } => result.clone(),
            }
        };
        let terminal = AppEvent::TurnDone {
            tab_id,
            turn_id,
            result: result.clone(),
        };
        self.forward_extension_turn_event(tab_id, turn_id, &terminal);

        // Journal the terminal record and close the checkpoint from the
        // captured recorder + engine. This runs regardless of whether the tab
        // still exists, so Ctrl+W on a non-last tab mid-turn no longer leaves a
        // dangling journal turn and an unclosed checkpoint. Canonical quarantine
        // completes in the source worker instead (recorder captured as None).
        if let Some(recorder) = recorder {
            if !source_terminal_seen && !matches!(&outcome, ForcedTurnStop::Canonical { .. }) {
                if let Ok(mut recorder) = recorder.lock() {
                    let (code, message, status, stop_reason) = if quarantine.is_some() {
                        (
                            "watchdog.quarantine",
                            "hard-stopped worker exceeded the quiescence deadline",
                            RunStatus::Failed,
                            "watchdog_quarantined",
                        )
                    } else {
                        match &outcome {
                            ForcedTurnStop::Watchdog(_) => (
                                "watchdog.hard_stop",
                                "cancellation grace expired; worker was hard-stopped",
                                RunStatus::Failed,
                                "watchdog_abort_grace_expired",
                            ),
                            ForcedTurnStop::Manual { .. } => (
                                "watchdog.force_cancel",
                                "manual cancellation grace expired; worker was hard-stopped",
                                RunStatus::Interrupted,
                                "user_interrupted",
                            ),
                            ForcedTurnStop::Canonical { .. } => unreachable!(),
                        }
                    };
                    recorder.record(RunEvent::Notice {
                        code: code.into(),
                        message: message.into(),
                    });
                    recorder.complete(
                        engine.as_ref().map(|engine| &engine.checkpoints),
                        true,
                        &TurnOutcome {
                            status,
                            stop_reason: Some(stop_reason.into()),
                            partial: true,
                        },
                    );
                }
            }
        }

        let mut scheduled_job = None;
        let mut schedule_attempt_lease = attempt_lease;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            if tab.draining_turn_id == Some(turn_id) || tab.active_turn_id == turn_id {
                tab.draining_turn_id = None;
                tab.active_turn_id = 0;
                tab.turn_abort = None;
                tab.turn_task = None;
                tab.watchdog_activity = None;
                if schedule_attempt_lease.is_none() {
                    schedule_attempt_lease = tab.watchdog_attempt_lease.take();
                }
                scheduled_job = tab.active_sched_job.take();
                tab.turn_started_at = None;
                tab.turn_tool_count = 0;
                tab.settle_turn_tools();
                tab.active_tool_names.clear();
                tab.active_tool_api_names.clear();
                tab.active_tool_started.clear();
                tab.chat.end_turn();
                tab.mode = if quarantine.is_some()
                    || result.is_err()
                    || matches!(&outcome, ForcedTurnStop::Watchdog(_))
                {
                    Mode::Error
                } else {
                    Mode::Ready
                };
                tab.store_dirty = false;
            }
        }
        if let Some(recovery) = quarantine {
            if let SchedJobRef::Schedule(id) = &recovery.job {
                if let Some(lease) = schedule_attempt_lease.take() {
                    self.finalize_schedule_attempt(
                        lease,
                        ScheduleTerminalMutation::WatchdogState {
                            failures: recovery.failures,
                            last_failure_ms: Some(recovery.last_failure_ms),
                            retry_at_ms: None,
                            enabled: Some(false),
                        },
                    );
                } else {
                    tracing::error!(schedule_id = %id, "refusing to clear quarantine without its attempt lease");
                }
            }
        } else {
            match outcome {
                ForcedTurnStop::Watchdog(failure) => {
                    self.apply_watchdog_failure(failure, schedule_attempt_lease.take())
                }
                ForcedTurnStop::Manual { job, failure } => {
                    if let Some(failure) = failure {
                        self.apply_watchdog_failure(failure, schedule_attempt_lease.take());
                    } else {
                        // A side-effect-free force cancel keeps its watchdog
                        // run context until the worker join completes so a
                        // raced canonical terminal can still consume it.
                        self.watchdog.cancel_job(&job);
                        self.clear_schedule_watchdog_success(
                            scheduled_job.as_ref().unwrap_or(&job),
                            schedule_attempt_lease.take(),
                        );
                    }
                }
                ForcedTurnStop::Canonical { job, result } => {
                    if self.watchdog.enabled() {
                        if let Some(failure) = self.watchdog.finish(
                            tab_id,
                            turn_id,
                            &result,
                            std::time::Instant::now(),
                        ) {
                            self.apply_watchdog_failure(failure, schedule_attempt_lease.take());
                        } else {
                            self.clear_schedule_watchdog_success(
                                &job,
                                schedule_attempt_lease.take(),
                            );
                        }
                    } else {
                        self.release_unsupervised_terminal_lease(schedule_attempt_lease.take());
                    }
                }
            }
        }
        drop(schedule_attempt_lease);
    }

    /// Move retries whose backoff elapsed onto the original tab's scheduler
    /// queue. A busy tab or user-owned queued input delays dispatch without
    /// consuming the retry.
    fn dispatch_watchdog_retries(&mut self) {
        let now = std::time::Instant::now();
        for retry in self.watchdog.due_retries(now) {
            let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == retry.tab_id) else {
                self.watchdog.cancel_job(&retry.job);
                continue;
            };
            let tab = &self.tabs[tab_idx];
            if tab.is_busy()
                || !tab.queued_input.is_empty()
                || self.sched_job_is_pending(&retry.job)
                || self.sched_job_has_pending_lease(&retry.job)
            {
                continue;
            }

            if let SchedJobRef::Schedule(id) = &retry.job {
                let Some(retry_token_ms) = self.watchdog.retry_token(&retry.job) else {
                    tracing::error!(schedule_id = %id, "persisted watchdog retry has no claim token");
                    self.watchdog.cancel_job(&retry.job);
                    continue;
                };
                let lease = match zode_core::scheduler::try_claim_watchdog_retry(id, retry_token_ms)
                {
                    Ok(Some(lease)) => lease,
                    Ok(None) => {
                        let roster = match zode_core::scheduler::try_load_schedules() {
                            Ok(roster) => roster,
                            Err(error) => {
                                tracing::warn!(%error, schedule_id = %id, "failed to reconcile watchdog retry; keeping it pending");
                                continue;
                            }
                        };
                        let retry_still_pending = roster.iter().any(|schedule| {
                            schedule.id == *id
                                && schedule.enabled
                                && schedule.watchdog_active_since_ms.is_none()
                                && schedule.watchdog_retry_at_ms == Some(retry_token_ms)
                        });
                        self.scheduler.set_schedules(roster);
                        if retry_still_pending {
                            continue;
                        }
                        self.reconcile_failed_schedule_claim(id, tab_idx);
                        self.watchdog.cancel_job(&retry.job);
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(%error, schedule_id = %id, "failed to claim watchdog retry; keeping it pending");
                        continue;
                    }
                };
                let active_token_ms = lease.active_token_ms();
                self.reload_schedule_roster();
                self.scheduler.mark_watchdog_active(id, active_token_ms);
                let replaced = self.pending_schedule_leases.insert(
                    id.clone(),
                    PendingScheduleLease {
                        lease,
                        origin: PendingScheduleOrigin::Retry(retry_token_ms),
                        queued_at: now,
                    },
                );
                debug_assert!(
                    replaced.is_none(),
                    "retry identity guard prevents replacement"
                );
            }
            self.push_sched_pending((retry.tab_id, retry.prompt.clone()), retry.job.clone());
            self.tabs[tab_idx]
                .queued_input
                .push_back(retry.prompt.clone());
            self.tabs[tab_idx].chat.push_system(&format!(
                "watchdog: starting retry {} for {}",
                retry.attempt,
                watchdog::job_label(&retry.job)
            ));
        }
    }

    fn apply_watchdog_failure(
        &mut self,
        failure: WatchdogFailure,
        mut attempt_lease: Option<zode_core::scheduler::ScheduleAttemptLease>,
    ) {
        let job = failure.job.clone();
        let cause = failure.cause.label();
        let persisted_failures = match &failure.recovery {
            Recovery::RetryScheduled { attempt, .. } => Some(*attempt),
            Recovery::Exhausted { failures } | Recovery::ManualReview { failures } => {
                Some(*failures)
            }
            Recovery::Cancelled => None,
        };
        let persist_retry = matches!(&failure.recovery, Recovery::RetryScheduled { .. });
        let manual_review = matches!(&failure.recovery, Recovery::ManualReview { .. });
        let cancelled = matches!(&failure.recovery, Recovery::Cancelled);
        let stopped = matches!(
            &failure.recovery,
            Recovery::Exhausted { .. } | Recovery::ManualReview { .. }
        );
        let mut persisted_schedule_state = None;
        if let (SchedJobRef::Schedule(id), Some(failures)) = (&job, persisted_failures) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0);
            let retry_at_ms = match &failure.recovery {
                Recovery::RetryScheduled { delay, .. } => {
                    Some(now_ms.saturating_add(delay.as_millis() as u64))
                }
                Recovery::Exhausted { .. }
                | Recovery::ManualReview { .. }
                | Recovery::Cancelled => None,
            };
            self.scheduler
                .record_watchdog_failure(id, failures, now_ms, retry_at_ms);
            if let Some(retry_at_ms) = retry_at_ms {
                self.watchdog.set_retry_token(&job, retry_at_ms);
            }
            persisted_schedule_state = Some((id.clone(), failures, now_ms, retry_at_ms));
        }
        let message = match failure.recovery {
            Recovery::RetryScheduled { attempt, delay } => format!(
                "watchdog: {cause}; retry {attempt} scheduled in {}s",
                delay.as_secs()
            ),
            Recovery::Cancelled => {
                format!("watchdog: {cause}; recovery cancelled because the job was stopped")
            }
            Recovery::Exhausted { failures } | Recovery::ManualReview { failures } => {
                match &job {
                    SchedJobRef::Loop(id) => {
                        self.scheduler.stop_loop(Some(*id));
                    }
                    SchedJobRef::Schedule(id) => {
                        self.scheduler.disable_schedule(id);
                    }
                }
                self.purge_sched_jobs(|candidate| candidate == &job);
                self.watchdog.cancel_job(&job);
                if manual_review {
                    format!(
                        "watchdog: {cause}; possible side effects or unknown execution state — {} stopped after {failures} failure(s); inspect before re-enabling",
                        watchdog::job_label(&job)
                    )
                } else {
                    format!(
                        "watchdog: {cause}; exhausted after {failures} failures — {} stopped",
                        watchdog::job_label(&job)
                    )
                }
            }
        };
        if let Some((id, failures, last_failure_ms, retry_at_ms)) = persisted_schedule_state {
            if let Some(lease) = attempt_lease.take() {
                self.finalize_schedule_attempt(
                    lease,
                    ScheduleTerminalMutation::WatchdogState {
                        failures,
                        last_failure_ms: Some(last_failure_ms),
                        retry_at_ms,
                        enabled: stopped.then_some(false),
                    },
                );
            } else {
                tracing::error!(schedule_id = %id, "refusing to persist a schedule terminal without its active token");
            }
        } else if persist_retry {
            tracing::debug!(job = %watchdog::job_label(&job), "loop watchdog retry is process-local");
        } else if cancelled {
            if let SchedJobRef::Schedule(id) = &job {
                // Removal/disable suppresses recovery, but the terminal still
                // owns the active-attempt token. Leaving it behind would make
                // a disabled schedule impossible to re-enable in this process.
                if let Some(lease) = attempt_lease.take() {
                    self.finalize_schedule_attempt(lease, ScheduleTerminalMutation::ClearOnly);
                } else {
                    tracing::error!(schedule_id = %id, "refusing to clear cancelled schedule without its attempt lease");
                }
            }
        }
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == failure.tab_id) {
            tab.chat.push_system(&message);
        } else {
            tracing::warn!(job = %watchdog::job_label(&job), "{message}");
        }
    }

    fn clear_schedule_watchdog_success(
        &mut self,
        job: &SchedJobRef,
        attempt_lease: Option<zode_core::scheduler::ScheduleAttemptLease>,
    ) {
        let SchedJobRef::Schedule(id) = job else {
            return;
        };
        self.scheduler.clear_watchdog_failures(id);
        if let Some(lease) = attempt_lease {
            self.finalize_schedule_attempt(
                lease,
                ScheduleTerminalMutation::WatchdogState {
                    failures: 0,
                    last_failure_ms: None,
                    retry_at_ms: None,
                    enabled: None,
                },
            );
        } else {
            tracing::error!(schedule_id = %id, "refusing to persist schedule success without its attempt lease");
        }
    }

    /// Submit input and report whether it actually started a new agent turn.
    /// Scheduler queue drains use the result to discard attribution for a
    /// prompt they already popped when validation or command handling bails.
    async fn submit(&mut self, text: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) -> bool {
        self.submit_with_scheduler_origin(text, None, agent_tx)
            .await
    }

    async fn submit_scheduler_occurrence(
        &mut self,
        text: &str,
        expected_job: &SchedJobRef,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        self.submit_with_scheduler_origin(text, Some(expected_job), agent_tx)
            .await
    }

    async fn submit_with_scheduler_origin(
        &mut self,
        text: &str,
        expected_job: Option<&SchedJobRef>,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        if self.should_quit {
            return false;
        }
        if let Some(expected_job) = expected_job {
            if self.active_tab().is_busy() {
                return false;
            }
            // Scheduler prompts are stored program text, not paste events.
            // Treat an existing image path literally and keep the user's
            // compose-box attachments/shell context isolated from this turn.
            return self
                .start_turn_on_tab(self.active, text, text, Some(expected_job), agent_tx)
                .await;
        }
        let cwd = self.active_tab().engine.cwd.clone();

        // `!<cmd>` runs a shell command directly (no agent turn). The command +
        // its output show inline AND are buffered as context for the next prompt
        // so the agent knows what was run locally.
        if let Some(cmd) = text.trim().strip_prefix('!') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.spawn_local_shell(cmd, agent_tx);
            }
            return false;
        }
        let parsed = match split_pasted_image_paths(&cwd, text) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.toast = Some(Toast::error(e.to_string()));
                return false;
            }
        };
        let mut submitted_text = parsed.remaining_text;
        let pasted_images = parsed.images;

        if pasted_images.is_empty() {
            let expanded = match parse_slash(&submitted_text) {
                Some((name, args)) => match self.expand_dynamic_command(name, args) {
                    Some(e) => Some(e),
                    None => {
                        self.handle_slash(name, args, agent_tx).await;
                        return false;
                    }
                },
                None => None,
            };
            if let Some(e) = expanded {
                submitted_text = e; // dynamic command → run as a templated turn
            }
        }

        if submitted_text.trim().is_empty()
            && pasted_images.is_empty()
            && self.active_tab().pending_images.is_empty()
        {
            return false;
        }

        let pasted_count = pasted_images.len();
        if pasted_count > 0 {
            self.active_tab_mut().pending_images.extend(pasted_images);
        }

        if pasted_count > 0 && submitted_text.trim().is_empty() {
            submitted_text.clear();
        }

        let expanded = match parse_slash(&submitted_text) {
            Some((name, args)) => match self.expand_dynamic_command(name, args) {
                Some(e) => Some(e),
                None => {
                    self.handle_slash(name, args, agent_tx).await;
                    return false;
                }
            },
            None => None,
        };
        if let Some(e) = expanded {
            submitted_text = e; // dynamic command → run as a templated turn
        }
        // One turn per tab (a second QueryLoop would mutate the same store
        // concurrently). Instead of rejecting, QUEUE the message and send it
        // Prepend any buffered `!cmd` shell output so this turn's prompt shows
        // the agent what was run locally (travels with a queued message too).
        if !self.active_tab().pending_shell_context.is_empty() {
            let ctx = self
                .active_tab_mut()
                .pending_shell_context
                .drain(..)
                .collect::<Vec<_>>()
                .join("\n\n");
            submitted_text = if submitted_text.trim().is_empty() {
                ctx
            } else {
                format!("{ctx}\n\n{submitted_text}")
            };
        }
        // when this tab goes idle — see `dispatch_queued_input`.
        if self.active_tab().is_busy() {
            // Mid-turn steering: a LIVE agent turn (has an abort handle) can
            // absorb the message NOW — inject it into the running loop so the
            // model sees it on its next round-trip, Claude-Code style, instead
            // of waiting for the whole turn to finish. Text-only and only for a
            // live turn (not a reassemble/draining tab); images/empties fall
            // through to the queue. A LOCAL op (compaction, `!cmd` shell) also
            // holds `turn_abort` so Esc can cancel it, but it runs no
            // QueryLoop — a message steered then would sit in the steer buffer
            // unread until some LATER turn starts, instead of dispatching from
            // the queue the moment the op finishes.
            let live_turn = self.active_tab().turn_abort.is_some()
                && self.active_tab().active_local_op_id.is_none();
            if live_turn
                && !submitted_text.trim().is_empty()
                && self.active_tab().pending_images.is_empty()
            {
                // The model sees the wrapped form (an interjection outranks
                // the current plan — weak models otherwise note it and keep
                // going); the chat bubble below shows only the user's text.
                let blocks = vec![agent::message::ContentBlock::Text {
                    text: steer_payload(&submitted_text),
                }];
                if self.active_tab().engine.steer(blocks) {
                    self.active_tab_mut().chat.push_user(&submitted_text);
                    self.toast = Some(Toast::info(crate::tr(
                        "steered into the running turn — the agent will see it next step",
                    )));
                    return false;
                }
            }
            // Queue even an image-only submission (empty text, pending image
            // chips): the dispatched entry re-enters `submit()` on the idle
            // tab and `start_turn_on_tab` consumes the pending images there —
            // the same turn an idle Enter would have started. Without this,
            // Enter-with-images while busy only toasted "attached" with
            // nothing scheduled to send them, stranding the chips above the
            // composer until the user typed another message. Skip stacking a
            // second empty entry — one drains all pending images already.
            let image_only = submitted_text.trim().is_empty();
            let already_queued = image_only
                && self
                    .active_tab()
                    .queued_input
                    .iter()
                    .any(|queued| queued.trim().is_empty());
            if !already_queued {
                self.active_tab_mut()
                    .queued_input
                    .push_back(submitted_text.to_string());
            }
            let n = self.active_tab().queued_input.len();
            self.toast = Some(Toast::info(
                crate::tr("queued ({n}) — sends when the turn finishes (Esc to interrupt now)")
                    .replace("{n}", &n.to_string()),
            ));
            return false;
        }

        // Direct submissions carry no scheduler provenance. Even identical
        // text cannot consume a queued occurrence; only the two queue drains
        // call `submit_scheduler_occurrence` with its exact expected job.
        self.start_turn_on_tab(self.active, &submitted_text, text, None, agent_tx)
            .await
    }

    /// Bind `text` to `tab_idx` and spawn its turn: route/validate images,
    /// stamp the session title, push the user message, arm per-turn state
    /// (mode, tool counters, turn id), and kick off the engine's streaming
    /// task. This is the turn-spawning tail of `submit()` — the part that
    /// commits to running a turn once slash/queue/shell-context/mid-turn
    /// concerns are already resolved — factored out so a background
    /// scheduler firing (`dispatch_scheduler_queued`) can start a turn
    /// on an IDLE tab other than the active one. `submit()` calls this with
    /// `self.active`, so the active-tab path is unchanged: same field
    /// writes, same ordering, same spawned events.
    ///
    /// `expected_sched_job` is explicit queue-drain provenance. It must match
    /// the oldest occurrence under `sched_key`; text equality alone is never
    /// ownership. The match is removed and `active_sched_job` stamped only
    /// after the last image-route/turn-limit preflight, so a bail preserves the
    /// exact queued occurrence for its claim-to-start watchdog.
    async fn start_turn_on_tab(
        &mut self,
        tab_idx: usize,
        text: &str,
        sched_key: &str,
        expected_sched_job: Option<&SchedJobRef>,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        if self.should_quit {
            return false;
        }
        let submitted_text = text.to_string();
        let sched_key = (self.tabs[tab_idx].id, sched_key.to_string());
        let pending_job = self
            .sched_pending
            .get(&sched_key)
            .and_then(|jobs| jobs.front())
            .filter(|job| expected_sched_job == Some(*job))
            .cloned();
        if expected_sched_job.is_some() && pending_job.is_none() {
            return false;
        }
        let scheduler_owned = pending_job.is_some();
        let has_images = !scheduler_owned && !self.tabs[tab_idx].pending_images.is_empty();
        let images_cfg = self.template.images().clone();
        let image_route = resolve_image_submit_route(
            has_images,
            images_cfg.effective_mode(),
            self.tabs[tab_idx].engine.supports_images(),
            images_cfg.vision_provider.is_some(),
        );
        // Only ROUTE the image submission here; the vision engine itself is
        // assembled inside the spawned turn task below (skills scan + MCP
        // connect can take seconds — run inline it froze the whole TUI), so an
        // assembly failure surfaces as a turn error, under the turn's spinner.
        let vision_template = match image_route {
            ImageSubmitRoute::Direct => None,
            ImageSubmitRoute::Unsupported => {
                if has_images {
                    self.toast = Some(Toast::error(crate::tr(
                        "current provider does not declare image support; set supportsImages=true or configure /vision provider <name>",
                    )));
                    return false;
                }
                None
            }
            ImageSubmitRoute::VisionModel => {
                let Some(provider_name) = images_cfg.vision_provider.as_deref() else {
                    self.toast = Some(Toast::error(crate::tr(
                        "configure /vision provider <name> first",
                    )));
                    return false;
                };
                let Some(template) = self.template.with_vision_provider(provider_name) else {
                    self.toast = Some(Toast::error(
                        crate::tr("vision provider '{provider_name}' is not configured")
                            .replace("{provider_name}", provider_name),
                    ));
                    return false;
                };
                Some((template, provider_name.to_string()))
            }
        };

        let Some(turn_id) = self.tabs[tab_idx].turn_seq.checked_add(1) else {
            self.toast = Some(Toast::error(crate::tr(
                "turn limit reached — start a new task",
            )));
            return false;
        };

        // Past every early-return point above: this turn WILL start, so it's
        // now safe to consume the pending scheduler entry (if any) and stamp
        // attribution. Doing this any earlier would let a bailed call above
        // consume `sched_key` without ever starting a turn, leaving a stuck
        // `active_sched_job` that misattributes the tab's NEXT unrelated turn
        // to this job (since no turn starts here, `TurnDone` never clears it).
        let mut schedule_attempt_lease = None;
        if let Some(job @ SchedJobRef::Schedule(id)) = pending_job.as_ref() {
            let lease = match self.pending_schedule_leases.remove(id) {
                Some(pending) => Ok(Some(pending.lease)),
                None => {
                    // Compatibility fallback for tests/legacy injection paths.
                    // Production fire and retry dispatch claim before queueing.
                    if let Some(retry_token_ms) = self.watchdog.retry_token(job) {
                        zode_core::scheduler::try_claim_watchdog_retry(id, retry_token_ms)
                    } else {
                        zode_core::scheduler::try_begin_watchdog_attempt(id)
                    }
                }
            };
            let lease = match lease {
                Ok(Some(lease)) => lease,
                Ok(None) => {
                    if let Some(removed) = self.pop_sched_pending(&sched_key) {
                        self.cancel_pending_sched_job(&removed);
                    }
                    self.reconcile_failed_schedule_claim(id, tab_idx);
                    self.tabs[tab_idx].chat.push_system(&format!(
                        "watchdog: attempt for schedule {id} is owned by another zode process"
                    ));
                    return false;
                }
                Err(error) => {
                    tracing::warn!(%error, schedule_id = %id, "failed to claim queued schedule attempt");
                    if let Some(removed) = self.pop_sched_pending(&sched_key) {
                        self.cancel_pending_sched_job(&removed);
                    }
                    return false;
                }
            };
            let active_token_ms = lease.active_token_ms();
            self.reload_schedule_roster();
            self.scheduler.mark_watchdog_active(id, active_token_ms);
            schedule_attempt_lease = Some(lease);
        }
        self.tabs[tab_idx].active_sched_job = if scheduler_owned {
            self.pop_sched_pending(&sched_key)
        } else {
            None
        };
        self.tabs[tab_idx].watchdog_attempt_lease = schedule_attempt_lease;

        // Stamp the session title from the first user prompt of this tab.
        if !self.tabs[tab_idx].titled {
            let title_source = if submitted_text.trim().is_empty() {
                self.tabs[tab_idx]
                    .pending_images
                    .first()
                    .map(|image| image.display_name.as_str())
                    .unwrap_or("image")
                    .to_string()
            } else {
                submitted_text.clone()
            };
            self.tabs[tab_idx].stamp_title(&title_source);
        }

        // The pending images are about to be consumed; drop any chip
        // selection — only meaningful for the active tab's own compose box
        // (a background scheduler firing never touches it).
        if tab_idx == self.active && !scheduler_owned {
            self.selected_image = None;
        }
        let tab = &mut self.tabs[tab_idx];
        // Prefix-cache shape check: a changed system prompt / tool set means
        // this turn re-writes the provider's prompt cache. Name the cause
        // once (reassembles — /model, /yolo, /sandbox, /goal — are the usual
        // ones) so the cost blip in the usage row is explained.
        let shape = tab.engine.prefix_shape();
        if let Some(prev) = tab.last_prefix_shape {
            if prev != shape {
                let what = match (prev.0 != shape.0, prev.1 != shape.1) {
                    (true, true) => "system prompt + tool set",
                    (true, false) => "system prompt",
                    _ => "tool set",
                };
                tab.chat.push_system(
                    &crate::tr(
                        "cache: {what} changed since the last turn — the prompt-cache prefix re-writes once",
                    )
                    .replace("{what}", what),
                );
            }
        }
        tab.last_prefix_shape = Some(shape);
        let images = if scheduler_owned {
            Vec::new()
        } else {
            std::mem::take(&mut tab.pending_images)
        };
        let previews = image_previews(&images);
        let content = user_content_blocks(&submitted_text, &images);
        // The image bytes are now in `content` (base64); the clipboard preview
        // temp files are no longer needed — clean them up.
        for image in &images {
            cleanup_clipboard_temp(&mut self.clipboard_temps, &image.path);
        }
        tab.chat.push_user_with_images(&submitted_text, previews);
        // No begin_assistant(): push_delta lazily opens an assistant segment,
        // so text after a tool card starts a fresh segment.
        tab.mode = Mode::Thinking;
        tab.active_tool_names.clear();
        tab.active_tool_api_names.clear();
        tab.active_tool_started.clear();
        // Fresh turn: reset the per-turn tool-use flag (goal no-progress).
        tab.turn_used_tools = false;
        // Fresh turn: arm the completion-footer clock and tool counter.
        tab.turn_started_at = Some(std::time::Instant::now());
        tab.turn_tool_count = 0;
        tab.turn_tools.clear();
        tab.watchdog_recorder = None;
        tab.watchdog_activity = None;

        tab.turn_seq = turn_id;
        tab.active_turn_id = turn_id;
        let tab_id = tab.id;
        let abort = AbortController::new();
        tab.turn_abort = Some(abort.clone());
        let turn_activity = abort.activity();
        // A clone for the recorder so it can distinguish a user cancel from a
        // failure (the turn's own `abort` is moved into `engine.turn`).
        let abort_for_recorder = abort.clone();

        let engine = tab.engine.clone();
        let session_id = tab.session_id.clone();
        let watchdog_job = tab.active_sched_job.clone();
        let scheduler_owned = watchdog_job.is_some();
        let scheduled_persistence = watchdog_job.as_ref().map(|_| ScheduledTurnPersistence {
            session_id: session_id.clone(),
            title: tab.title.clone(),
            persisted: tab.persisted_msgs.clone(),
        });
        let watchdog_pulse = watchdog_job.and_then(|job| {
            // Persisted schedules carry their failure count across restarts;
            // seed it before registering this new attempt.
            if let SchedJobRef::Schedule(id) = &job {
                if let Some(schedule) = self.scheduler.schedules().iter().find(|s| &s.id == id) {
                    self.watchdog
                        .seed_failures(&job, schedule.watchdog_failures);
                }
            }
            self.watchdog.start(
                job,
                tab_id,
                turn_id,
                submitted_text.clone(),
                std::time::Instant::now(),
                turn_activity.clone(),
            )
        });
        if scheduler_owned {
            tab.watchdog_activity = Some(turn_activity);
        }
        let mut recorder = TurnRecorder::new(
            SessionStore::open_default().ok(),
            RunEventContext::new(
                session_id.clone(),
                Some(format!("tui-{turn_id}")),
                Some(Uuid::new_v4().simple().to_string()),
            ),
        );
        recorder.start();
        {
            let count = engine.store.lock().map(|store| store.len()).unwrap_or(0);
            if let Err(error) =
                recorder.begin_checkpoint(&engine.checkpoints, engine.cwd.clone(), count)
            {
                tracing::warn!("checkpoint start failed: {error}");
            }
        }
        let recorder = Arc::new(std::sync::Mutex::new(recorder));
        if scheduler_owned {
            tab.watchdog_recorder = Some(recorder.clone());
        }
        let images_for_vision = images.clone();
        let submitted_text_for_vision = submitted_text.clone();
        let vision_prompt = images_cfg.effective_prompt().to_string();
        let tx = agent_tx.clone();
        // Bind only after this turn is canonical in the tab, but before the
        // provider task can enqueue a tool approval request.
        self.template
            .bind_approval_turn(&tab_id.to_string(), turn_id);
        let turn_task = tokio::spawn(async move {
            let stream_result: Result<Box<dyn agent::stream::EventStream>, String> = async {
                if let Some((vision_template, provider_name)) = vision_template {
                    let assembled = vision_template
                        .assemble_tab(Some(engine.cwd.clone()), Some(format!("{tab_id}:vision")))
                        .await
                        .map_err(|error| {
                            format!("{}: {error}", crate::tr("vision provider failed"))
                        })?;
                    if !assembled.supports_images() {
                        return Err(crate::tr(
                            "vision provider '{provider_name}' does not declare image support; set supportsImages=true on it (or its models entry) in config.json",
                        )
                        .replace("{provider_name}", &provider_name));
                    }
                    let description = run_vision_description(
                        Arc::new(assembled),
                        vision_prompt,
                        submitted_text_for_vision.clone(),
                        images_for_vision,
                        abort.clone(),
                    )
                    .await?;
                    let prompt = merge_prompt_with_vision(&submitted_text_for_vision, &description);
                    engine
                        .turn(&prompt, abort)
                        .await
                        .map_err(|error| error.to_string())
                } else {
                    engine
                        .turn_blocks(content, abort)
                        .await
                        .map_err(|error| error.to_string())
                }
            }
            .await;
            forward_agent_turn_stream(
                AgentTurnStreamContext {
                    engine,
                    recorder: Some(recorder),
                    abort: Some(abort_for_recorder),
                    tab_id,
                    turn_id,
                    tx,
                    watchdog_pulse,
                    scheduled_persistence,
                },
                stream_result,
            )
            .await;
        });
        self.tabs[tab_idx].turn_task = Some(turn_task);
        true
    }

    /// Tick-driven drain for prompts `poll_scheduler` injected, on EVERY idle
    /// tab — the active one included.
    ///
    /// The active tab's other drain, `dispatch_queued_input`, only runs from
    /// the terminal-input and agent-event arms of the event loop. With a single
    /// tab and the user away from the keyboard neither arm ever fires, so a due
    /// `/loop` prompt used to sit queued until the next keypress while
    /// `poll_scheduler`'s anti-pileup check swallowed every later fire and
    /// `Scheduler::due` kept incrementing `runs` — a `--max N` loop could burn
    /// through all N runs having executed zero turns. Draining from the tick is
    /// what makes an unattended loop actually unattended.
    ///
    /// Only prompts the scheduler itself injected are drained (`sched_pending`
    /// still owns the front entry's key). A user-typed queued message is never
    /// auto-sent on a schedule — on a background tab it keeps waiting for the
    /// user to switch back, and on the active tab it keeps waiting for the
    /// normal drain, so the queued-edit UX is untouched.
    async fn dispatch_scheduler_queued(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.should_quit {
            return;
        }
        let mut idx = 0;
        while idx < self.tabs.len() {
            if self.tabs[idx].is_busy() {
                idx += 1;
                continue;
            }
            // Same guard `dispatch_queued_input` honors: the front entry is
            // currently mirrored in the prompt editor for the user to edit, so
            // sending it out from under them would be a regression. (Only the
            // active tab can have a queued edit open.)
            if idx == self.active && self.queued_edit_index == Some(0) {
                idx += 1;
                continue;
            }
            let tab_id = self.tabs[idx].id;
            let pending = self.tabs[idx].queued_input.front().and_then(|front| {
                let key = (tab_id, front.clone());
                self.sched_pending
                    .get(&key)
                    .and_then(|jobs| jobs.front())
                    .cloned()
                    .map(|job| (key, job))
            });
            let Some((pending_key, pending_job)) = pending else {
                idx += 1;
                continue;
            };
            match self.queued_schedule_claim_is_runnable(&pending_job) {
                Ok(true) => {}
                Ok(false) => {
                    self.tabs[idx].queued_input.pop_front();
                    self.cancel_sched_pending_if_front(&pending_key, &pending_job);
                    self.tabs[idx].chat.push_system(
                        "scheduler: queued occurrence was disabled or lost ownership before start",
                    );
                    idx += 1;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to validate queued schedule; keeping it queued");
                    idx += 1;
                    continue;
                }
            }
            let Some(prompt) = self.tabs[idx].queued_input.pop_front() else {
                idx += 1;
                continue;
            };
            let started = if idx == self.active {
                // Route the active tab through `submit()` exactly as
                // `dispatch_queued_input` would, so a tick-driven drain and a
                // keypress-driven drain are indistinguishable (shell-context
                // prepend, pending images, queued-edit index bookkeeping).
                self.submit_scheduler_occurrence(&prompt, &pending_job, agent_tx)
                    .await
            } else {
                // Key and text are the same string here — unlike `submit()`,
                // this prompt never gains a shell-context prefix. The actual
                // `sched_pending` removal + `active_sched_job` stamp happens
                // inside `start_turn_on_tab`, after its last early-return point.
                self.start_turn_on_tab(idx, &prompt, &prompt, Some(&pending_job), agent_tx)
                    .await
            };
            let deferred = !started
                && self
                    .sched_pending
                    .get(&pending_key)
                    .and_then(|jobs| jobs.front())
                    == Some(&pending_job);
            if deferred {
                // Preflight did not consume attribution or ownership. Put the
                // concrete occurrence back until its queue watchdog starts the
                // turn or converts the wait into bounded retry.
                self.tabs[idx].queued_input.push_front(prompt);
            } else if !started {
                self.cancel_sched_pending_if_front(&pending_key, &pending_job);
            }
            if idx == self.active && !deferred {
                if let Some(index) = self.queued_edit_index.as_mut() {
                    *index = index.saturating_sub(1);
                }
            }
            idx += 1;
        }
    }

    /// Drop every pending scheduler attribution entry whose job matches
    /// `is_gone`, and remove the prompts those entries had already queued.
    ///
    /// Called when a job stops existing (`/loop stop`, `/schedule rm`,
    /// `/schedule disable`). Without this, a stopped loop would still run one
    /// more time from its already-queued prompt, and its orphaned
    /// `sched_pending` entry would linger forever — ready to capture a later,
    /// unrelated user message with the same text and (on a background tab) even
    /// auto-run it as scheduler-owned.
    fn purge_sched_jobs(&mut self, is_gone: impl Fn(&SchedJobRef) -> bool) {
        let mut doomed_jobs: HashSet<SchedJobRef> = self
            .sched_pending
            .values()
            .flat_map(|jobs| jobs.iter())
            .filter(|job| is_gone(job))
            .cloned()
            .collect();
        doomed_jobs.extend(self.pending_schedule_leases.keys().filter_map(|id| {
            let job = SchedJobRef::Schedule(id.clone());
            is_gone(&job).then_some(job)
        }));
        let doomed: Vec<(SchedPendingKey, Vec<bool>)> = self
            .sched_pending
            .iter()
            .filter_map(|(key, jobs)| {
                let removals: Vec<bool> = jobs.iter().map(&is_gone).collect();
                removals
                    .iter()
                    .any(|remove| *remove)
                    .then(|| (key.clone(), removals))
            })
            .collect();

        for ((tab_id, prompt), removals) in doomed {
            // The first N equal-text queue entries correspond FIFO to the N
            // jobs captured in `removals`. Remove only the matching job's
            // concrete occurrences; equal-text occurrences owned by another
            // job (or later user input) survive.
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                let mut occurrence = 0;
                tab.queued_input.retain(|queued| {
                    if queued != &prompt {
                        return true;
                    }
                    let remove = removals.get(occurrence).copied().unwrap_or(false);
                    occurrence += 1;
                    !remove
                });
            }

            let key = (tab_id, prompt);
            let now_empty = if let Some(jobs) = self.sched_pending.get_mut(&key) {
                jobs.retain(|job| !is_gone(job));
                jobs.is_empty()
            } else {
                false
            };
            if now_empty {
                self.sched_pending.remove(&key);
            }
        }
        for job in &doomed_jobs {
            self.sched_queued_at.remove(job);
            self.release_pending_schedule_lease(job);
        }
        self.sched_fail_streak.retain(|job, _| !is_gone(job));
        self.watchdog.cancel_recoveries_matching(is_gone);
    }

    /// Drop pending scheduler entries belonging to a tab that is going away.
    /// The tab's `queued_input` disappears with it, so only the map needs
    /// clearing — but leaving entries behind would let a same-text prompt on a
    /// future tab that reuses the id be misattributed.
    fn purge_sched_pending_for_tab(&mut self, tab_id: usize) -> HashSet<SchedJobRef> {
        let jobs: HashSet<SchedJobRef> = self
            .sched_pending
            .iter()
            .filter(|((id, _), _)| *id == tab_id)
            .flat_map(|(_, jobs)| jobs.iter().cloned())
            .collect();
        self.sched_pending.retain(|(id, _), _| *id != tab_id);
        for job in &jobs {
            self.sched_queued_at.remove(job);
            self.release_pending_schedule_lease(job);
        }
        jobs
    }

    fn handle_runtime_event(&mut self, ev: AppEvent, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        match ev {
            AppEvent::ExtensionTask(event) => {
                self.handle_extension_task_event(event, agent_tx);
            }
            other => self.handle_agent_event(other),
        }
    }

    fn handle_agent_event(&mut self, ev: AppEvent) {
        if let AppEvent::TurnTaskStopped { tab_id, turn_id } = &ev {
            self.finish_forced_turn_stop(*tab_id, *turn_id);
            return;
        }
        if let AppEvent::TurnTaskQuarantined {
            tab_id,
            turn_id,
            result,
        } = &ev
        {
            self.quarantine_turn_task(*tab_id, *turn_id, result.clone());
            return;
        }
        if let AppEvent::ReassembleDone {
            tab_id,
            seq,
            effect,
            result,
        } = ev
        {
            self.handle_reassemble_done(tab_id, seq, effect, result);
            return;
        }
        // A background git-stat poll finished: cache it for the sidebar.
        if let AppEvent::GitStatDone { tab_id, files } = ev {
            if let Some(tab) = self.tab_by_id(tab_id) {
                tab.git_poll_inflight = false;
                tab.git_files = files;
            }
            return;
        }
        // Toasts (from off-loop work) carry no tab/turn id.
        if let AppEvent::Toast { text, error } = ev {
            self.toast = Some(if error {
                Toast::error(text)
            } else {
                Toast::info(text)
            });
            return;
        }
        // A manual /compact finished: clear the tab's busy state and post the
        // outcome. Routed by tab id only (it isn't a turn).
        if let AppEvent::CompactDone {
            tab_id,
            op_id,
            result,
            auto,
        } = ev
        {
            self.template
                .clear_approval_local_operation_if(&tab_id.to_string(), op_id);
            self.deny_tui_approvals(TuiApprovalCleanupTarget::LocalOperation { tab_id, op_id });
            let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return;
            };
            if self.tabs[tab_idx].active_local_op_id != Some(op_id) {
                return;
            }
            let tab = &mut self.tabs[tab_idx];
            tab.active_local_op_id = None;
            tab.turn_abort = None;
            tab.local_op_is_auto_compact = false;
            let ok = result.is_ok();
            if ok {
                // Refresh the context gauge immediately from the rewritten
                // store, so the "% ctx" badge drops right after /compact
                // instead of lingering at the pre-compact count until the
                // next Usage event — and so the progress check below reads
                // the same number the auto trigger will.
                if let Ok(store) = tab.engine.store.lock() {
                    tab.context_tokens = estimate_store_tokens(&store);
                }
            }
            match result {
                Ok(summary) => {
                    tab.chat.push_system(&summary);
                    tab.mode = Mode::Ready;
                    // A compaction that leaves the gauge over the trigger
                    // threshold made no effective progress — without this,
                    // Ok reset the breaker and maybe_auto_compact re-fired
                    // on the next event-loop pass, looping useless LLM
                    // calls forever with the context pinned at ~100%.
                    let stuck = auto && tab.engine.needs_pre_turn_compact(tab.context_tokens);
                    if stuck {
                        tab.auto_compact_failures = tab.auto_compact_failures.saturating_add(1);
                        if tab.auto_compact_failures == AUTO_COMPACT_MAX_FAILURES {
                            tab.chat.push_system(crate::tr(
                                "auto-compact paused: compaction is no longer shrinking the context — run /compact to retry manually, or /clear to start fresh",
                            ));
                        }
                    } else {
                        tab.auto_compact_failures = 0;
                    }
                }
                Err(e) => {
                    tab.chat
                        .push_system(&format!("{}: {e}", crate::tr("compact failed")));
                    // Trip the auto-compact breaker on repeated auto failures;
                    // tell the user ONCE what stopped and what to do instead.
                    if auto {
                        tab.auto_compact_failures = tab.auto_compact_failures.saturating_add(1);
                        if tab.auto_compact_failures == AUTO_COMPACT_MAX_FAILURES {
                            tab.chat.push_system(crate::tr(
                                "auto-compact paused after repeated failures — run /compact to retry manually, or /clear to start fresh",
                            ));
                        }
                    }
                    tab.mode = Mode::Error;
                }
            }
            // Compaction rewrote the message store; persist it so the compacted
            // transcript survives a resume (mirrors the post-turn save). The
            // PREFIX changed (tombstones + spliced summary), so this must be a
            // full rewrite, not an append.
            if ok {
                tab.store_dirty = false; // full rewrite makes the file current
                let (session_id, engine, title, persisted) = (
                    tab.session_id.clone(),
                    tab.engine.clone(),
                    tab.title.clone(),
                    tab.persisted_msgs.clone(),
                );
                tokio::spawn(crate::tab::persist_session(
                    session_id, engine, title, persisted, false,
                ));
            }
            return;
        }
        if let AppEvent::BgProgress {
            tab_id,
            op_id,
            line,
        } = ev
        {
            let Some(tab) = self.tab_by_id(tab_id) else {
                return;
            };
            if tab.active_local_op_id == Some(op_id) {
                tab.chat.push_system(&line);
            }
            return;
        }
        if let AppEvent::BgDone {
            tab_id,
            op_id,
            result,
        } = ev
        {
            self.template
                .clear_approval_local_operation_if(&tab_id.to_string(), op_id);
            self.deny_tui_approvals(TuiApprovalCleanupTarget::LocalOperation { tab_id, op_id });
            let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return;
            };
            if self.tabs[tab_idx].active_local_op_id != Some(op_id) {
                return;
            }
            let tab = &mut self.tabs[tab_idx];
            tab.active_local_op_id = None;
            tab.turn_abort = None;
            tab.active_tool_names.clear();
            tab.active_tool_api_names.clear();
            tab.active_tool_started.clear();
            match result {
                Ok(line) => {
                    tab.chat.push_system(&line);
                    tab.mode = Mode::Ready;
                }
                Err(e) => {
                    tab.chat.push_system(&e);
                    tab.mode = Mode::Error;
                }
            }
            return;
        }
        if let AppEvent::ConnectDialogReady { dialog } = ev {
            // An approval/question modal that arrived meanwhile owns the
            // screen — drop the dialog rather than covering the prompt (the
            // user can re-run /connect).
            if self.active_dialog.is_none() && self.active_question.is_none() {
                self.connect = Some(*dialog);
            }
            return;
        }
        if let AppEvent::LocalShellDone {
            tab_id,
            cmd,
            output,
            op_id,
        } = ev
        {
            if let Some(op_id) = op_id {
                self.template
                    .clear_approval_local_operation_if(&tab_id.to_string(), op_id);
                self.deny_tui_approvals(TuiApprovalCleanupTarget::LocalOperation { tab_id, op_id });
            }
            let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return;
            };
            // An owned shell must still be the exact slot owner. Stale and
            // duplicate completions are dropped as a whole, including output.
            // Concurrent shells carry no id and never mutate slot ownership.
            if let Some(op_id) = op_id {
                if self.tabs[tab_idx].active_local_op_id != Some(op_id) {
                    return;
                }
                let tab = &mut self.tabs[tab_idx];
                tab.active_local_op_id = None;
                tab.turn_abort = None;
            }
            let tab = &mut self.tabs[tab_idx];
            // `None` = interrupted: the child was killed and the interrupt
            // handler already posted "(interrupted)" — nothing more to show,
            // and a partial run must not become agent context.
            let Some(output) = output else {
                return;
            };
            let shown = output.trim_end();
            if shown.is_empty() {
                tab.chat.push_tool("(no output)");
            } else {
                for line in shown.lines() {
                    tab.chat.push_tool(line);
                }
            }
            tab.pending_shell_context
                .push(format_shell_context(&cmd, &output));
            return;
        }
        if let AppEvent::TurnDone {
            tab_id, turn_id, ..
        } = &ev
        {
            // Modal cleanup belongs to the immutable old generation even when
            // this terminal is stale for the tab's current busy slot.
            self.template
                .clear_approval_turn_if(&tab_id.to_string(), *turn_id);
            self.deny_tui_approvals(TuiApprovalCleanupTarget::Turn {
                tab_id: *tab_id,
                turn_id: *turn_id,
            });
            if let Some(pending) = self.forced_turn_stops.get_mut(&(*tab_id, *turn_id)) {
                // The hard-stop waiter owns terminal delivery and persistence.
                // A worker can race its abort and queue TurnDone just before
                // exiting; do not release the store before JoinHandle confirms
                // that nested drivers and tools have actually been dropped.
                pending.source_terminal_seen = true;
                if let AppEvent::TurnDone { result, .. } = &ev {
                    let canonical_job = match &pending.outcome {
                        ForcedTurnStop::Manual { job, failure } => {
                            failure.is_none().then(|| job.clone())
                        }
                        ForcedTurnStop::Canonical { job, .. } => Some(job.clone()),
                        ForcedTurnStop::Watchdog(_) => None,
                    };
                    if let Some(job) = canonical_job {
                        pending.outcome = ForcedTurnStop::Canonical {
                            job,
                            result: result.clone(),
                        };
                    }
                }
                return;
            }
        }
        // Route to the originating tab; drop events from a closed tab.
        let (tab_id, turn_id) = match &ev {
            AppEvent::Agent {
                tab_id, turn_id, ..
            }
            | AppEvent::TurnDone {
                tab_id, turn_id, ..
            } => (*tab_id, *turn_id),
            AppEvent::Toast { .. }
            | AppEvent::CompactDone { .. }
            | AppEvent::BgProgress { .. }
            | AppEvent::BgDone { .. }
            | AppEvent::GitStatDone { .. }
            | AppEvent::LocalShellDone { .. }
            | AppEvent::ConnectDialogReady { .. }
            | AppEvent::ReassembleDone { .. }
            | AppEvent::TurnTaskStopped { .. }
            | AppEvent::TurnTaskQuarantined { .. }
            | AppEvent::ExtensionTask(_) => {
                unreachable!("handled above")
            }
        };
        let Some(tab_idx) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        // Capture this before forwarding TurnDone: the forwarder consumes the
        // immutable route. Tests inject a TempDir-backed repository here, so
        // extension turns must not fall through to the user's global store.
        let extension_sessions = self
            .extension_tasks
            .session_repository_for_turn(tab_id, turn_id);
        // Drop events from an aborted/superseded turn within that tab.
        if turn_id != self.tabs[tab_idx].active_turn_id {
            if let AppEvent::TurnDone { ref result, .. } = ev {
                if self.tabs[tab_idx].draining_turn_id == Some(turn_id) {
                    let watchdog_failure =
                        self.watchdog
                            .finish(tab_id, turn_id, result, std::time::Instant::now());
                    // The exact interrupted turn's terminal is canonical for
                    // the draining lifecycle even though active_turn_id was
                    // cleared at abort time. It owns the one interrupted wire
                    // terminal and the final persistence, but deliberately
                    // skips post-turn memory extraction.
                    self.forward_extension_turn_event(tab_id, turn_id, &ev);
                    let tab = &mut self.tabs[tab_idx];
                    tab.draining_turn_id = None;
                    tab.turn_task = None;
                    tab.watchdog_recorder = None;
                    tab.watchdog_activity = None;
                    let mut schedule_attempt_lease = tab.watchdog_attempt_lease.take();
                    let scheduled_job = tab.active_sched_job.take();
                    tab.turn_started_at = None;
                    tab.turn_tool_count = 0;
                    tab.settle_turn_tools();
                    tab.active_tool_names.clear();
                    tab.active_tool_api_names.clear();
                    tab.active_tool_started.clear();
                    let allow_append = !tab.store_dirty;
                    tab.store_dirty = false;
                    let (session_id, engine, title, persisted) = (
                        tab.session_id.clone(),
                        tab.engine.clone(),
                        tab.title.clone(),
                        tab.persisted_msgs.clone(),
                    );
                    if scheduled_job.is_none() {
                        if let Some(sessions) = extension_sessions {
                            tokio::spawn(async move {
                                sessions
                                    .persist(session_id, engine, title, persisted, allow_append)
                                    .await;
                            });
                        } else {
                            tokio::spawn(crate::tab::persist_session(
                                session_id,
                                engine,
                                title,
                                persisted,
                                allow_append,
                            ));
                        }
                    }
                    if self.watchdog.enabled() {
                        if let Some(failure) = watchdog_failure {
                            self.apply_watchdog_failure(failure, schedule_attempt_lease.take());
                        } else if let Some(job) = scheduled_job.as_ref() {
                            self.clear_schedule_watchdog_success(
                                job,
                                schedule_attempt_lease.take(),
                            );
                        }
                    } else {
                        self.release_unsupervised_terminal_lease(schedule_attempt_lease.take());
                    }
                } else {
                    // Remember every real provider terminal for idempotent old
                    // interrupt requests, but never fan stale output to UI.
                    self.record_stale_extension_turn_done(tab_id, turn_id, result);
                }
            }
            return;
        }
        // Only canonical, still-live turn events reach the extension fanout.
        // This choke point shares the same stale/closed-tab fence as the TUI.
        self.forward_extension_turn_event(tab_id, turn_id, &ev);
        let tab = &mut self.tabs[tab_idx];
        match ev {
            AppEvent::Agent {
                event, cost_label, ..
            } => {
                if let Some(label) = cost_label {
                    tab.cost_label = label;
                }
                match event {
                    Event::TextDelta { delta } => {
                        tab.mode = Mode::Streaming;
                        tab.chat.push_delta(&delta);
                    }
                    Event::Thinking { delta } => {
                        tab.mode = Mode::Thinking;
                        tab.chat.push_thinking_delta(&delta);
                    }
                    Event::ToolUse {
                        ref id,
                        ref name,
                        ref input,
                    } => {
                        // Mark that this turn did real work (drives goal-loop
                        // no-progress detection).
                        tab.turn_used_tools = true;
                        tab.turn_tool_count = tab.turn_tool_count.saturating_add(1);
                        // The HUD tally counts a call the moment it starts, so
                        // an in-flight call is already visible (running glyph).
                        tab.turn_tools.record_start(name);
                        let title = tool_call_title(name, input);
                        tab.active_tool_names.insert(id.clone(), title);
                        tab.active_tool_api_names.insert(id.clone(), name.clone());
                        tab.active_tool_started
                            .insert(id.clone(), std::time::Instant::now());
                        tab.recent_tools.push_back(crate::tab::ToolActivity {
                            id: id.clone(),
                            name: name.clone(),
                            status: "running",
                            duration_ms: None,
                        });
                        while tab.recent_tools.len() > 20 {
                            tab.recent_tools.pop_front();
                        }
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool_call(&line, name);
                        }
                    }
                    Event::ToolResult { ref id, .. } => {
                        let known_tool = tab.active_tool_names.remove(id);
                        let api_name = tab.active_tool_api_names.remove(id);
                        let started = tab.active_tool_started.remove(id);
                        if let Some(name) = api_name {
                            tab.turn_tools.record_result(
                                &name,
                                matches!(&event, Event::ToolResult { ok: true, .. }),
                            );
                        }
                        if let Some(activity) = tab
                            .recent_tools
                            .iter_mut()
                            .rev()
                            .find(|item| item.id == *id)
                        {
                            activity.status =
                                if matches!(&event, Event::ToolResult { ok: true, .. }) {
                                    "succeeded"
                                } else {
                                    "failed"
                                };
                            activity.duration_ms = started.map(|instant| {
                                u64::try_from(instant.elapsed().as_millis()).unwrap_or(u64::MAX)
                            });
                        }
                        let cwd = tab.engine.cwd.clone();
                        if let Some(line) = process_line_for_event_with_cwd(
                            &event,
                            known_tool.as_deref(),
                            Some(cwd.as_path()),
                        ) {
                            let line = match started {
                                Some(t) => format!(
                                    "{line} · {}",
                                    zode_core::duration_fmt::format_duration_ms(
                                        u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
                                    )
                                ),
                                None => line,
                            };
                            tab.chat.push_tool_result(&line);
                        }
                    }
                    Event::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read,
                        cache_create,
                    } => {
                        tab.input_tokens = tab.input_tokens.saturating_add(input_tokens);
                        tab.output_tokens = tab.output_tokens.saturating_add(output_tokens);
                        // Current context occupancy = the FULL prompt size, not
                        // just the uncached input — with prompt caching the new
                        // input is tiny (cache hit), so the cached + cache-creation
                        // tokens are what actually fill the window. Overwrite (not
                        // accumulate); it drops after compaction.
                        let prompt = input_tokens
                            .saturating_add(cache_read)
                            .saturating_add(cache_create);
                        if prompt > 0 {
                            tab.context_tokens = prompt;
                        }
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_usage(&line);
                        }
                    }
                    // API-retry notices are the whole point of showing retries —
                    // surface them as SYSTEM lines so `/tool-details off` can't
                    // hide them (tool/process rows are hideable).
                    Event::Notice { ref code, .. } if code == "api_retry" => {
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_system(&line);
                        }
                    }
                    // The runtime's own (mid-turn) compaction just rewrote the
                    // store — refresh the "% ctx" badge from it right away,
                    // like the CompactDone handler does for TUI-driven
                    // compaction. Otherwise the badge lingers at the
                    // pre-compact count until the next Usage event (which
                    // never arrives if the turn errors first).
                    Event::Notice { ref code, .. }
                        if code == "agent.compact.ok" || code == "agent.compact.micro" =>
                    {
                        if let Ok(store) = tab.engine.store.lock() {
                            tab.context_tokens = estimate_store_tokens(&store);
                        }
                        // A mid-turn compaction rewrote the store's prefix —
                        // the next save can't be a pure append.
                        tab.store_dirty = true;
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                    // A loop-guard nudge is runtime evidence of a weak model:
                    // record the verdict (auto-lite, zero config) once. The
                    // reassembly that applies it runs when the tab next goes
                    // idle (`maybe_apply_learned_profile`).
                    Event::Notice { ref code, .. } if code == "agent.loop.repeat" => {
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_system(&line);
                        }
                        if !tab.weak_signal_noted {
                            tab.weak_signal_noted = true;
                            zode_core::config::learn_model_lite(&tab.engine.model);
                            if !tab.engine.lite_profile {
                                tab.chat.push_system(&crate::tr(
                                    "weak-model behavior detected — lite accommodations will be enabled for this model (remembered for future sessions; set profile: \"standard\" in config to opt out)",
                                ));
                            }
                        }
                    }
                    Event::Notice { .. } | Event::Result { .. } | Event::Unknown => {
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                    Event::Error { code, message } => {
                        tab.chat
                            .push_system(&format!("{} [{code}]: {message}", crate::tr("error")));
                        tab.mode = Mode::Error;
                    }
                    _ => {
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                }
            }
            AppEvent::TurnDone { result, .. } => {
                // Any desktop automation this turn is over: stop arming Esc and
                // hide the ghost cursor (both no-ops if never used this turn).
                zode_core::desktop::esc_watch::disarm();
                zode_core::desktop::overlay::hide_global();
                tab.chat.end_turn();
                tab.turn_abort = None;
                tab.turn_task = None;
                tab.watchdog_recorder = None;
                tab.watchdog_activity = None;
                let mut schedule_attempt_lease = tab.watchdog_attempt_lease.take();
                tab.active_turn_id = 0;
                tab.active_tool_names.clear();
                tab.active_tool_api_names.clear();
                tab.active_tool_started.clear();
                let turn_elapsed = tab.turn_started_at.take().map(|t| t.elapsed());
                let tool_count = std::mem::take(&mut tab.turn_tool_count);
                tab.settle_turn_tools();
                let ok = result.is_ok();
                let scheduled_job = tab.active_sched_job.take();
                let watchdog_failure = if self.watchdog.enabled() && scheduled_job.is_some() {
                    self.watchdog
                        .finish(tab_id, turn_id, &result, std::time::Instant::now())
                } else {
                    None
                };
                let watchdog_success =
                    (self.watchdog.enabled() && ok && watchdog_failure.is_none())
                        .then(|| scheduled_job.clone())
                        .flatten();
                tab.mode = match &result {
                    Ok(()) => {
                        if let Some(elapsed) = turn_elapsed {
                            let line = crate::tr("✓ done · {duration} · {n} tools")
                                .replace(
                                    "{duration}",
                                    &zode_core::duration_fmt::format_duration_ms(
                                        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                                    ),
                                )
                                .replace("{n}", &tool_count.to_string());
                            tab.chat.push_system(&line);
                        }
                        Mode::Ready
                    }
                    Err(e) => {
                        let mut line = format!("{}: {e}", crate::tr("turn failed"));
                        if let Some(elapsed) = turn_elapsed {
                            line.push_str(&format!(
                                " · {}",
                                zode_core::duration_fmt::format_duration_ms(
                                    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                                )
                            ));
                        }
                        tab.chat.push_system(&line);
                        // A tool-loop abort is definitive weak-model evidence
                        // — record the learned verdict (auto-lite, no config;
                        // see the agent.loop.repeat notice arm for the nudge
                        // tier of the same signal).
                        if e.contains("tool-call loop detected") && !tab.weak_signal_noted {
                            tab.weak_signal_noted = true;
                            zode_core::config::learn_model_lite(&tab.engine.model);
                            if !tab.engine.lite_profile {
                                tab.chat.push_system(&crate::tr(
                                    "weak-model behavior detected — lite accommodations will be enabled for this model (remembered for future sessions; set profile: \"standard\" in config to opt out)",
                                ));
                            }
                        }
                        Mode::Error
                    }
                };
                // Scheduler circuit breaker: a turn attributed to a /loop or
                // /schedule job (via `active_sched_job`, stamped at submit time)
                // updates that job's consecutive-failure streak. 3 in a row stops
                // the loop / disables the schedule — a persistently broken
                // prompt must not retry forever unattended.
                if !self.watchdog.enabled() {
                    if let Some(job_ref) = scheduled_job.as_ref() {
                        if ok {
                            self.sched_fail_streak.remove(job_ref);
                        } else {
                            let streak = self.sched_fail_streak.entry(job_ref.clone()).or_insert(0);
                            *streak += 1;
                            if *streak >= 3 {
                                self.sched_fail_streak.remove(job_ref);
                                match job_ref {
                                    SchedJobRef::Loop(id) => {
                                        self.scheduler.stop_loop(Some(*id));
                                    }
                                    SchedJobRef::Schedule(id) => {
                                        match zode_core::scheduler::disable_schedule_atomic(id) {
                                            Ok(update) => {
                                                self.scheduler.set_schedules(update.schedules)
                                            }
                                            Err(error) => tracing::warn!(
                                                %error,
                                                schedule_id = %id,
                                                "failed to persist scheduler circuit breaker"
                                            ),
                                        }
                                    }
                                }
                                tab.chat.push_system(crate::tr(
                                    "scheduler: stopped after 3 consecutive failures",
                                ));
                            }
                        }
                    }
                }
                // Goal auto-loop: keep taking turns toward the goal until the
                // agent calls `GoalComplete` — or the user interrupts / clears
                // the goal, or a turn fails. Only continues on a successful turn.
                if tab.goal_loop_active {
                    // No-progress tracking: a turn with no tool call did no
                    // real work. Update the streak before the decisions below.
                    if tab.turn_used_tools {
                        tab.goal_no_progress_streak = 0;
                    } else {
                        tab.goal_no_progress_streak = tab.goal_no_progress_streak.saturating_add(1);
                    }
                    // Effective cap: the user's `autoLoopMaxTurns`, or a sane
                    // built-in default so an unset config can't loop forever.
                    let max_turns = tab
                        .engine
                        .auto_loop_max_turns()
                        .unwrap_or(GOAL_LOOP_DEFAULT_MAX_TURNS);
                    if !ok {
                        // A failed/interrupted turn halts the loop cleanly.
                        stop_goal_loop(tab);
                    } else if tab.engine.take_goal_completed() {
                        stop_goal_loop(tab);
                        tab.chat
                            .push_system(crate::tr("✓ goal complete — auto-loop stopped"));
                    } else if tab.goal_no_progress_streak >= GOAL_LOOP_NO_PROGRESS_LIMIT {
                        // The model keeps replying without doing work — stop
                        // rather than burn turns spinning in place.
                        stop_goal_loop(tab);
                        tab.chat.push_system(crate::tr(
                            "goal-loop: no progress (no tool use) for several turns — paused (send a message to resume)",
                        ));
                    } else {
                        // Count the turn that just ran, THEN honor the cap so
                        // the loop runs exactly `max_turns` turns.
                        tab.goal_loop_iter = tab.goal_loop_iter.saturating_add(1);
                        if tab.goal_loop_iter >= max_turns {
                            stop_goal_loop(tab);
                            tab.chat.push_system(crate::tr(
                                "goal-loop: reached the turn cap — paused (send a message to resume)",
                            ));
                        } else if !tab
                            .queued_input
                            .iter()
                            .any(|q| q == GOAL_LOOP_CONTINUE_PROMPT)
                        {
                            // Queue the next iteration ONCE — a user message
                            // injected mid-loop must not leave a duplicate
                            // continuation stacked in the queue.
                            tab.queued_input
                                .push_back(GOAL_LOOP_CONTINUE_PROMPT.to_string());
                        }
                    }
                }
                // Persist the session off the event loop. A normal turn only
                // APPENDS to the store, so save incrementally — unless a
                // mid-turn compaction rewrote the prefix (`store_dirty`), in
                // which case a full rewrite is required. The dirty flag is
                // cleared either way (the save makes the file current).
                let allow_append = !tab.store_dirty;
                tab.store_dirty = false;
                let (session_id, engine, title, persisted) = (
                    tab.session_id.clone(),
                    tab.engine.clone(),
                    tab.title.clone(),
                    tab.persisted_msgs.clone(),
                );
                if scheduled_job.is_none() {
                    // Interactive turns keep memory extraction detached.
                    // Scheduler turns deliberately skip it: starting a new
                    // external LLM/write worker after their source-side
                    // quiescence and durable save would outlive the attempt
                    // lease and overlap the next recurrence.
                    engine.spawn_post_turn_extraction();
                    if let Some(sessions) = extension_sessions {
                        tokio::spawn(async move {
                            sessions
                                .persist(session_id, engine, title, persisted, allow_append)
                                .await;
                        });
                    } else {
                        tokio::spawn(crate::tab::persist_session(
                            session_id,
                            engine,
                            title,
                            persisted,
                            allow_append,
                        ));
                    }
                }
                if let Some(job) = watchdog_success {
                    self.clear_schedule_watchdog_success(&job, schedule_attempt_lease.take());
                }
                if let Some(failure) = watchdog_failure {
                    self.apply_watchdog_failure(failure, schedule_attempt_lease.take());
                }
                if self.watchdog.enabled() {
                    drop(schedule_attempt_lease);
                } else {
                    self.release_unsupervised_terminal_lease(schedule_attempt_lease.take());
                }
            }
            AppEvent::Toast { .. }
            | AppEvent::CompactDone { .. }
            | AppEvent::BgProgress { .. }
            | AppEvent::BgDone { .. }
            | AppEvent::GitStatDone { .. }
            | AppEvent::LocalShellDone { .. }
            | AppEvent::ConnectDialogReady { .. }
            | AppEvent::ReassembleDone { .. }
            | AppEvent::TurnTaskStopped { .. }
            | AppEvent::TurnTaskQuarantined { .. }
            | AppEvent::ExtensionTask(_) => {
                unreachable!("handled above")
            }
        }
    }

    fn handle_external_agents_command(
        &mut self,
        args: &str,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let subcommand = args.trim().to_ascii_lowercase();
        let cwd = self.active_tab().engine.cwd.clone();

        match subcommand.as_str() {
            "" | "list" => {
                let detected = zode_core::external_agents::detect_installed_presets();
                let cfg = match ConfigManager::load(&cwd) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        self.toast = Some(Toast::error(format!(
                            "{}: {e}",
                            crate::tr("load config failed")
                        )));
                        return;
                    }
                };
                let mut lines = vec!["External agent CLIs (explicit registration):".to_string()];
                if detected.is_empty() {
                    lines.push("  (no supported external agent CLIs found on PATH)".to_string());
                } else {
                    for item in detected {
                        let status = match cfg.external_agents.agents.get(&item.name) {
                            Some(entry) if entry.enabled == Some(false) => "disabled",
                            Some(_) if cfg.external_agents.enabled() => "registered",
                            Some(_) => "registered, but globally disabled",
                            None => "available",
                        };
                        lines.push(format!(
                            "  [{status}] {:<14} {}",
                            item.name,
                            item.command.display()
                        ));
                    }
                    lines.push(
                        "Use /external-agents discover to register every available preset."
                            .to_string(),
                    );
                }
                self.active_tab_mut().chat.push_system(&lines.join("\n"));
            }
            "discover" | "register" => {
                // Persisting first and then failing to rebuild would be
                // surprising, so reject while the active engine is running.
                if self.active_tab().is_busy() {
                    self.toast = Some(Toast::info(
                        "can't register external agents during a turn — Ctrl+C first",
                    ));
                    return;
                }
                let report = match zode_core::external_agents::detect_and_register_global(&cwd) {
                    Ok(report) => report,
                    Err(e) => {
                        self.toast = Some(Toast::error(format!(
                            "{}: {e}",
                            crate::tr("save config failed")
                        )));
                        return;
                    }
                };
                let message = if report.detected.is_empty() {
                    "No supported external agent CLIs were found on PATH; config was unchanged."
                        .to_string()
                } else {
                    let mut parts = Vec::new();
                    if !report.added.is_empty() {
                        parts.push(format!("registered: {}", report.added.join(", ")));
                    }
                    if !report.already_registered.is_empty() {
                        parts.push(format!(
                            "already registered: {}",
                            report.already_registered.join(", ")
                        ));
                    }
                    if !report.effective_enabled {
                        parts.push(
                            "external agents remain disabled by the project config".to_string(),
                        );
                    }
                    format!("External agent discovery — {}", parts.join("; "))
                };

                // Rebuild even when every preset was already present: the
                // config may have been edited after this tab was assembled.
                if !report.detected.is_empty() {
                    match self.template.reload_external_agents_from_disk(&cwd) {
                        Ok(template) => {
                            self.start_reassemble_active(
                                template,
                                ReassembleEffect::Notify(ReassembleNotify::System(message)),
                                agent_tx,
                            );
                        }
                        Err(e) => {
                            self.toast = Some(Toast::error(format!(
                                "registered, but {}: {e}",
                                crate::tr("reload failed")
                            )));
                        }
                    }
                } else {
                    self.active_tab_mut().chat.push_system(&message);
                }
            }
            _ => self
                .active_tab_mut()
                .chat
                .push_system("usage: /external-agents [list|discover]"),
        }
    }

    async fn handle_slash(
        &mut self,
        name: &str,
        args: &str,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        // Builtins are matched case-insensitively (`/HELP` works — the
        // registry lookup that routed us here is already case-insensitive).
        // Dynamic commands (skills/agents/MCP) keep their exact case and
        // never reach this match.
        let lowered = name.to_ascii_lowercase();
        let name = lowered.as_str();
        match name {
            "exit" => self.should_quit = true,
            "help" => self.show_help = true,
            "clear" | "new" => {
                // Mutating the store mid-turn races the running QueryLoop.
                if self.active_tab().is_busy() {
                    self.toast = Some(Toast::info(crate::tr(
                        "can't clear during a turn — Ctrl+C first",
                    )));
                } else {
                    let tab = &mut self.tabs[self.active];
                    // Swap the old transcript out and free it OFF the event
                    // loop: dropping a huge ChatView + MessageStore (hundreds
                    // of thousands of small allocations) inline showed up as
                    // a visible UI stall on /clear.
                    let old_chat = std::mem::replace(&mut tab.chat, ChatView::new());
                    let old_store = tab
                        .engine
                        .store
                        .lock()
                        .map(|mut store| {
                            std::mem::replace(&mut *store, agent::message::MessageStore::new())
                        })
                        .ok();
                    tokio::task::spawn_blocking(move || drop((old_chat, old_store)));
                    // The context gauge reflects the (now empty) store again
                    // only at the next Usage event — reset it here so the
                    // auto-compact trigger can't fire on a stale 98%+ badge.
                    // A fresh conversation also re-arms the breaker.
                    tab.context_tokens = 0;
                    tab.auto_compact_failures = 0;
                    // The store was emptied (prefix discarded) — the next save
                    // must be a full rewrite, not an append onto the old file.
                    tab.store_dirty = true;
                    // The runtime's compaction latches (no-progress, failure
                    // breaker) describe the conversation just discarded —
                    // reset them or the QueryLoop's own auto-compaction
                    // stays disabled for the rest of the session.
                    if let Ok(mut s) = tab.engine.compact_state.lock() {
                        *s = agent::compact::AutoCompactState::default();
                    }
                }
            }
            "theme" => self.handle_theme(args),
            // Run undo/redo off the event loop (the history mutex + file
            // restore could block) and toast the result back as an event.
            // Mark the store dirty so the next save is a full rewrite — undo
            // touches persisted state and a pure append could miss it.
            "undo" => {
                self.tabs[self.active].store_dirty = true;
                self.spawn_history_op(agent_tx, true);
            }
            "redo" => {
                self.tabs[self.active].store_dirty = true;
                self.spawn_history_op(agent_tx, false);
            }
            "cost" => {
                let report = self.active_tab().engine.cost.report().await;
                self.active_tab_mut().chat.push_system(&report);
            }
            "currency" => {
                let code = args.trim();
                if code.is_empty() {
                    let cur = self.active_tab().engine.cost.currency_code();
                    let list = zode_core::currency::CURRENCIES
                        .iter()
                        .map(|c| c.code)
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.active_tab_mut().chat.push_system(&format!(
                        "{}: {cur}\n{}: {list}\n{}",
                        crate::tr("currency"),
                        crate::tr("available"),
                        crate::tr("use /currency <code>")
                    ));
                } else {
                    // Switch the display currency IN PLACE (no engine rebuild, so
                    // no reassembly freeze) and refresh the shown cost right away.
                    let applied = self.active_tab().engine.cost.set_currency(code);
                    let label = self.active_tab().engine.cost.sidebar_label().await;
                    let tab = self.active_tab_mut();
                    tab.cost_label = label;
                    tab.chat
                        .push_system(&format!("{}: {applied}", crate::tr("currency set")));
                }
            }
            "op" => {
                use zode_core::commands::op::{map_subcommand, OpCommand};
                use zode_core::openpencil::connection::connection_status;
                let cfg = self.active_tab().engine.openpencil.clone();
                match map_subcommand(args) {
                    Err(e) => self.active_tab_mut().chat.push_system(&format!("/op: {e}")),
                    // `status` is a quick local connection check — fine inline.
                    Ok(OpCommand::Status) => {
                        let s = connection_status(&cfg).await;
                        self.active_tab_mut().chat.push_system(&s);
                    }
                    // Tool/MCP and design calls run OFF the event loop (they may
                    // connect/install/launch and stream an LLM for many seconds),
                    // so the UI never freezes: they stream progress + a result
                    // back as events, show the busy spinner, and Esc cancels.
                    Ok(OpCommand::Call { tool, args }) => self.spawn_op_call(tool, args, agent_tx),
                    Ok(OpCommand::Generate { prompt }) => self.spawn_op_generate(prompt, agent_tx),
                }
            }
            "team" => {
                use zode_core::commands::team::{parse, TeamCommand};
                let team = self.active_tab().engine.team.clone();
                let Some(team) = team else {
                    self.active_tab_mut()
                        .chat
                        .push_system("/team: the team tool group is disabled");
                    return;
                };
                // Bare `/team` opens the status panel; subcommands are text.
                if args.trim().is_empty() {
                    self.open_team_panel(&team);
                    return;
                }
                let input = format!("/team {args}");
                match parse(input.trim_end()) {
                    None => self
                        .active_tab_mut()
                        .chat
                        .push_system("/team: usage: /team [status|board|dismiss <name>]"),
                    Some(cmd) => {
                        let line = match cmd {
                            TeamCommand::Status => team.status_report(),
                            TeamCommand::Board => team.board_report(),
                            TeamCommand::Dismiss(name) => match team.dismiss(&name).await {
                                Ok(()) => format!("dismissed teammate '{name}'"),
                                Err(e) => e.to_string(),
                            },
                        };
                        self.active_tab_mut().chat.push_system(&line);
                    }
                }
            }
            "browser" => {
                use zode_core::commands::browser::{map_subcommand, BrowserCommand};
                match map_subcommand(args) {
                    Err(e) => self
                        .active_tab_mut()
                        .chat
                        .push_system(&format!("/browser: {e}")),
                    Ok(BrowserCommand::Panel) => self.open_browser_panel(),
                    // Fast local read (try_lock inside) — fine inline, like `/op status`.
                    Ok(BrowserCommand::Status) => {
                        let session = self.active_tab().engine.browser.clone();
                        let line = session.status().await;
                        self.active_tab_mut().chat.push_system(&line);
                    }
                    Ok(BrowserCommand::Launch) => {
                        self.spawn_browser_op(BrowserOp::Launch, agent_tx)
                    }
                    Ok(BrowserCommand::Close) => self.spawn_browser_op(BrowserOp::Close, agent_tx),
                    Ok(BrowserCommand::Pair) => self.start_browser_pairing(agent_tx).await,
                    Ok(BrowserCommand::Target { target }) => {
                        let t = if target == "bridge" {
                            zode_core::browser::BrowserTarget::Bridge
                        } else {
                            zode_core::browser::BrowserTarget::Managed
                        };
                        let want_bridge = matches!(t, zode_core::browser::BrowserTarget::Bridge);
                        let session = self.active_tab().engine.browser.clone();
                        let mut applied = false;
                        let msg = match ConfigManager::persist_browser_default_target(&target) {
                            Ok(()) => match session.set_target(t) {
                                Ok(()) => {
                                    applied = true;
                                    format!("browser target: {target} (saved as default)")
                                }
                                Err(e) => e.to_string(),
                            },
                            Err(e) => format!("{}: {e}", crate::tr("save config failed")),
                        };
                        if want_bridge && applied {
                            ensure_browser_bridge_and_maybe_reconnect(session).await;
                        }
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Ok(BrowserCommand::Screenshot { path }) => {
                        self.spawn_browser_op(BrowserOp::Screenshot { path }, agent_tx)
                    }
                }
            }
            "loop" => {
                use zode_core::commands::loop_sched::{parse_loop, LoopCommand};
                let input = format!("/loop {args}");
                match parse_loop(input.trim_end()) {
                    Err(e) => self.active_tab_mut().chat.push_system(&e),
                    Ok(LoopCommand::Start {
                        interval,
                        prompt,
                        max_runs,
                    }) => {
                        let owner = self.active_tab().id as u64;
                        let id = self.scheduler.add_loop(
                            owner,
                            prompt,
                            interval,
                            max_runs,
                            std::time::Instant::now(),
                        );
                        let line = crate::tr("loop started: every {interval} (id {id})")
                            .replace(
                                "{interval}",
                                &zode_core::duration_fmt::format_duration_ms(
                                    interval.as_millis() as u64
                                ),
                            )
                            .replace("{id}", &id.to_string());
                        self.active_tab_mut().chat.push_system(&line);
                    }
                    Ok(LoopCommand::List) => {
                        let lines: Vec<String> = self
                            .scheduler
                            .loops()
                            .iter()
                            .map(|j| {
                                format!(
                                    "#{} every {} · runs {} · {}",
                                    j.id,
                                    zode_core::duration_fmt::format_duration_ms(
                                        j.interval.as_millis() as u64
                                    ),
                                    j.runs,
                                    j.prompt
                                )
                            })
                            .collect();
                        let text = if lines.is_empty() {
                            "(no loops)".to_string()
                        } else {
                            lines.join("\n")
                        };
                        self.active_tab_mut().chat.push_system(&text);
                    }
                    Ok(LoopCommand::Stop(id)) => {
                        self.scheduler.stop_loop(id);
                        // A stopped loop must not get one more run out of a
                        // prompt it already queued.
                        self.purge_sched_jobs(|job| match (job, id) {
                            (SchedJobRef::Loop(job_id), Some(stopped)) => *job_id == stopped,
                            (SchedJobRef::Loop(_), None) => true,
                            _ => false,
                        });
                        self.active_tab_mut()
                            .chat
                            .push_system(crate::tr("loop stopped"));
                    }
                }
            }
            "schedule" => {
                use zode_core::commands::loop_sched::{parse_schedule, ScheduleCommand};
                let input = format!("/schedule {args}");
                match parse_schedule(input.trim_end()) {
                    Err(e) => self.active_tab_mut().chat.push_system(&e),
                    Ok(ScheduleCommand::Add { spec, prompt }) => {
                        let spec_desc = describe_schedule_spec(&spec);
                        let id = gen_schedule_id(self.scheduler.schedules());
                        let job = zode_core::scheduler::ScheduleJob {
                            id: id.clone(),
                            spec,
                            prompt,
                            enabled: true,
                            last_fired_ms: None,
                            watchdog_failures: 0,
                            watchdog_last_failure_ms: None,
                            watchdog_retry_at_ms: None,
                            watchdog_active_since_ms: None,
                        };
                        let msg = match zode_core::scheduler::add_schedule_atomic(job) {
                            Ok(update) => {
                                let applied = update.applied;
                                self.scheduler.set_schedules(update.schedules);
                                if applied {
                                    crate::tr("schedule added: {spec} (id {id})")
                                        .replace("{spec}", &spec_desc)
                                        .replace("{id}", &id)
                                } else {
                                    format!("schedule id collision: {id}; try again")
                                }
                            }
                            Err(e) => format!("{}: {e}", crate::tr("save config failed")),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Ok(ScheduleCommand::List) => {
                        let lines: Vec<String> = self
                            .scheduler
                            .schedules()
                            .iter()
                            .map(|j| {
                                format!(
                                    "{} {} · {} · {}",
                                    j.id,
                                    if j.enabled { "enabled" } else { "disabled" },
                                    describe_schedule_spec(&j.spec),
                                    j.prompt
                                )
                            })
                            .collect();
                        let text = if lines.is_empty() {
                            "(no schedules)".to_string()
                        } else {
                            lines.join("\n")
                        };
                        self.active_tab_mut().chat.push_system(&text);
                    }
                    Ok(ScheduleCommand::Rm(id)) => {
                        // A queued occurrence is locally owned but has not
                        // started: cancel it and exact-clear its active token
                        // before asking the store to delete the now-idle row.
                        let removed_job = SchedJobRef::Schedule(id.clone());
                        if self.sched_job_is_pending(&removed_job)
                            || self.sched_job_has_pending_lease(&removed_job)
                        {
                            self.purge_sched_jobs(|job| job == &removed_job);
                        }
                        let msg = match zode_core::scheduler::remove_schedule_atomic(&id) {
                            Ok(update) => {
                                let applied = update.applied;
                                let active = update
                                    .schedules
                                    .iter()
                                    .find(|schedule| schedule.id == id)
                                    .is_some_and(|schedule| {
                                        schedule.watchdog_active_since_ms.is_some()
                                    });
                                self.scheduler.set_schedules(update.schedules);
                                if applied {
                                    self.purge_sched_jobs(|job| job == &removed_job);
                                    format!("removed {id}")
                                } else if active {
                                    format!(
                                        "schedule {id} still has an active attempt; disable it now and remove it after the attempt finishes"
                                    )
                                } else {
                                    format!("no schedule with id {id}")
                                }
                            }
                            Err(e) => format!("{}: {e}", crate::tr("save config failed")),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Ok(ScheduleCommand::Enable(id)) => {
                        let msg = match zode_core::scheduler::enable_schedule_atomic(&id) {
                            Ok(update) => {
                                let applied = update.applied;
                                let active = update
                                    .schedules
                                    .iter()
                                    .find(|schedule| schedule.id == id)
                                    .is_some_and(|schedule| {
                                        schedule.watchdog_active_since_ms.is_some()
                                    });
                                self.scheduler.set_schedules(update.schedules);
                                if applied {
                                    self.watchdog.cancel_job(&SchedJobRef::Schedule(id.clone()));
                                    format!("enabled {id}")
                                } else if active {
                                    format!(
                                        "schedule {id} still has an active attempt; wait for it to finish or restart to recover an orphan before enabling"
                                    )
                                } else {
                                    format!("no schedule with id {id}")
                                }
                            }
                            Err(e) => format!("{}: {e}", crate::tr("save config failed")),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Ok(ScheduleCommand::Disable(id)) => {
                        let msg = match zode_core::scheduler::disable_schedule_atomic(&id) {
                            Ok(update) => {
                                let applied = update.applied;
                                self.scheduler.set_schedules(update.schedules);
                                if applied {
                                    self.purge_sched_jobs(|job| {
                                        job == &SchedJobRef::Schedule(id.clone())
                                    });
                                    format!("disabled {id}")
                                } else {
                                    format!("no schedule with id {id}")
                                }
                            }
                            Err(e) => format!("{}: {e}", crate::tr("save config failed")),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                }
            }
            "desktop" => {
                use zode_core::commands::desktop::{map_subcommand, DesktopCommand};
                match map_subcommand(args) {
                    Err(e) => self
                        .active_tab_mut()
                        .chat
                        .push_system(&format!("/desktop: {e}")),
                    Ok(DesktopCommand::Status) => {
                        let session = self.active_tab().engine.desktop.clone();
                        let lines = session.status_lines().await.join("\n");
                        self.active_tab_mut().chat.push_system(&lines);
                    }
                    Ok(DesktopCommand::Attach { port }) => {
                        let session = self.active_tab().engine.desktop.clone();
                        let msg = match session.attach_cdp(port).await {
                            Ok(()) => format!(
                                "desktop: attached CDP on 127.0.0.1:{port} — DesktopEval enabled"
                            ),
                            Err(e) => format!("desktop attach failed: {e}"),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                }
            }
            "sessions" | "resume" => self.open_session_picker(),
            "tab" => self.handle_tab_command(args),
            "connect" => self.open_connect_dialog(agent_tx),
            "plugin" => self.open_plugin_picker(),
            "vision" => self.handle_vision(args),
            "sidebar" => {
                if args.trim().is_empty() {
                    self.open_sidebar_picker();
                } else {
                    self.handle_sidebar_command(args);
                }
            }
            "tasks" => self.open_tasks_panel().await,
            "watchdog" => {
                let args = args.trim();
                let message = if args.is_empty() || args == "status" {
                    self.watchdog_status_lines(std::time::Instant::now())
                        .join("\n")
                } else {
                    "usage: /watchdog [status]".to_string()
                };
                self.active_tab_mut().chat.push_system(&message);
            }
            "subagents" => self.open_subagents_panel(),
            "config" => {
                let msg = format!(
                    "model={} cwd={}",
                    self.active_tab().engine.model,
                    self.active_tab().engine.cwd.display()
                );
                self.active_tab_mut().chat.push_system(&msg);
            }
            "compact" => self.spawn_compact(agent_tx),
            "model" => {
                if args.is_empty() {
                    self.open_model_picker();
                } else {
                    self.apply_model(args, agent_tx);
                }
            }
            "yolo" => {
                self.toggle_yolo(agent_tx);
            }
            "sandbox" => {
                // No args → open the picker (the options are too many to type);
                // a direct arg (`/sandbox off`) still applies immediately.
                if args.trim().is_empty() {
                    self.open_sandbox_picker();
                } else {
                    self.apply_sandbox_action(args, agent_tx).await;
                }
            }
            "plan" => {
                // Per-tab: flip THIS tab's flag, then reassemble (which re-applies
                // it). The status badge syncs from the active tab on render.
                let on = !self.active_tab().plan_mode;
                self.active_tab_mut().plan_mode = on;
                if !self.start_reassemble_active(
                    self.template.clone(),
                    ReassembleEffect::Plan { on },
                    agent_tx,
                ) {
                    // Reassembly refused (busy) — revert the flag.
                    self.active_tab_mut().plan_mode = !on;
                }
            }
            "mcp" => self.open_mcp_dialog(),
            "memory" => {
                let cwd = self.active_tab().engine.cwd.clone();
                let msg = self
                    .active_tab()
                    .engine
                    .noema
                    .handle_command(args, Some(&cwd));
                self.active_tab_mut().chat.push_system(&msg);
            }
            "skills" => {
                let list: Vec<String> = self
                    .active_tab()
                    .engine
                    .skills
                    .list()
                    .iter()
                    .map(|s| format!("{} — {}", s.name, s.description))
                    .collect();
                if list.is_empty() {
                    self.active_tab_mut()
                        .chat
                        .push_system(crate::tr("(no skills loaded)"));
                } else {
                    for l in list {
                        self.active_tab_mut().chat.push_system(&l);
                    }
                }
            }
            "goal" => {
                let trimmed = args.trim();
                if trimmed.is_empty() {
                    let msg = match self.template.goal() {
                        Some(g) => format!(
                            "{}: {g}\n{}",
                            crate::tr("current goal"),
                            crate::tr("(clear with /goal clear)")
                        ),
                        None => crate::tr("no goal set — use /goal <text> to set one").to_string(),
                    };
                    self.active_tab_mut().chat.push_system(&msg);
                } else {
                    // "clear"/"none" wipes the goal; anything else sets it.
                    let new_goal = (!trimmed.eq_ignore_ascii_case("clear")
                        && !trimmed.eq_ignore_ascii_case("none"))
                    .then(|| trimmed.to_string());
                    self.apply_goal(new_goal, agent_tx);
                }
            }
            "effort" => {
                let level = args.trim().to_ascii_lowercase();
                if level.is_empty() {
                    // No arg → open the picker (low/medium/high).
                    self.open_effort_picker();
                } else if !matches!(
                    level.as_str(),
                    "low" | "medium" | "high" | "clear" | "reset"
                ) {
                    self.toast = Some(Toast::info(crate::tr("usage: /effort low|medium|high")));
                } else {
                    let new_effort =
                        matches!(level.as_str(), "low" | "medium" | "high").then(|| level.clone());
                    let t = self.template.with_effort(new_effort.clone());
                    let msg = match &new_effort {
                        Some(e) => format!("{}: {e}", crate::tr("effort set")),
                        None => crate::tr("effort reset to medium (default)").to_string(),
                    };
                    self.start_reassemble_active(
                        t,
                        ReassembleEffect::Effort {
                            notify: ReassembleNotify::System(msg),
                        },
                        agent_tx,
                    );
                }
            }
            "copy" => match self.active_tab().engine.last_assistant_text() {
                Some(text) => match zode_core::clipboard::copy_to_clipboard(&text) {
                    Ok(_) => {
                        self.toast =
                            Some(Toast::info(crate::tr("copied last response to clipboard")))
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("copy failed"))))
                    }
                },
                None => self.toast = Some(Toast::info(crate::tr("nothing to copy yet"))),
            },
            "export" => {
                let Some(path) =
                    zode_core::export::try_resolve_export_path(&self.active_tab().engine.cwd, args)
                else {
                    self.toast = Some(Toast::error(crate::tr(
                        "export path escapes the workspace — use an absolute path to export elsewhere",
                    )));
                    return;
                };
                let md = self.active_tab().engine.export_markdown();
                match std::fs::write(&path, md) {
                    Ok(()) => {
                        let msg = format!(
                            "{} {}",
                            crate::tr("exported conversation to"),
                            path.display()
                        );
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("export failed"))))
                    }
                }
            }
            "diff" => {
                let cwd = self.active_tab().engine.cwd.clone();
                let out = zode_core::diff::working_tree_diff(&cwd).await;
                self.active_tab_mut().chat.push_system(&out);
            }
            "agents" => self.open_agents_dialog(),
            "external-agents" => self.handle_external_agents_command(args, agent_tx),
            "workflows" => self.open_workflows_dialog(),
            "permissions" => {
                for line in self.template.permissions_summary() {
                    self.active_tab_mut().chat.push_system(&line);
                }
            }
            "hooks" => {
                let lines = self.template.hooks_summary();
                if lines.is_empty() {
                    self.active_tab_mut()
                        .chat
                        .push_system(crate::tr("(no hooks configured)"));
                } else {
                    for line in lines {
                        self.active_tab_mut().chat.push_system(&line);
                    }
                }
            }
            "reload-plugins" => {
                // Re-read plugins.disabled from disk (global ⊕ project) for the
                // active tab's cwd so out-of-band config edits take effect.
                let cwd = self.active_tab().engine.cwd.clone();
                match self.template.reload_plugins_from_disk(&cwd) {
                    Ok(t) => {
                        self.start_reassemble_active(
                            t,
                            ReassembleEffect::Notify(ReassembleNotify::System(
                                crate::tr("reloaded — tools, MCP, skills, and LSP re-discovered")
                                    .to_string(),
                            )),
                            agent_tx,
                        );
                    }
                    Err(e) => {
                        self.toast =
                            Some(Toast::error(format!("{}: {e}", crate::tr("reload failed"))))
                    }
                }
            }
            "reload-skills" => {
                self.start_reassemble_active(
                    self.template.clone(),
                    ReassembleEffect::ReloadSkills,
                    agent_tx,
                );
            }
            "language" => self.open_language_picker(),
            "orchestration" => {
                let on = !self.template.autonomous_orchestration();
                let t = self.template.with_autonomous_orchestration(on);
                let msg = if on {
                    crate::tr("autonomous orchestration: ON — the agent may decompose tasks, spawn sub-agents, and define new ones")
                } else {
                    crate::tr("autonomous orchestration: OFF")
                };
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Orchestration {
                        on,
                        notify: ReassembleNotify::System(msg.to_string()),
                    },
                    agent_tx,
                );
            }
            "thinking" => {
                self.show_thinking = !self.show_thinking;
                self.persist_show_thinking(self.show_thinking);
                self.toast = Some(Toast::info(format!(
                    "{} {}",
                    crate::tr("thinking output"),
                    on_off(self.show_thinking)
                )));
            }
            "tool-details" => {
                self.show_tool_details = !self.show_tool_details;
                self.persist_show_tool_details(self.show_tool_details);
                self.toast = Some(Toast::info(format!(
                    "{} {}",
                    crate::tr("tool details"),
                    on_off(self.show_tool_details)
                )));
            }
            other => {
                // Every registered builtin has an arm above, so this is a
                // typo/unknown — say so instead of implying a planned feature.
                self.toast = Some(Toast::info(format!(
                    "{}: /{other}  ({})",
                    crate::tr("unknown command"),
                    crate::tr("try /help")
                )));
            }
        }
    }

    fn persist_show_thinking(&self, value: bool) {
        if let Ok(mut cfg) = ConfigManager::load_global() {
            cfg.show_thinking = Some(value);
            let _ = ConfigManager::save_global(&cfg);
        }
    }

    fn persist_show_tool_details(&self, value: bool) {
        if let Ok(mut cfg) = ConfigManager::load_global() {
            cfg.show_tool_details = Some(value);
            let _ = ConfigManager::save_global(&cfg);
        }
    }

    /// Run a direct `/op <tool>` MCP call OFF the event loop. Even a "quick"
    /// call can block on connect/install/launch, which would freeze the UI (and
    /// could deadlock the consent prompt, which the event loop must pump). This
    /// keeps the UI live + cancelable and posts the result back as an event.
    /// Run a saved JS workflow off-loop (the `/workflows` dialog's Enter):
    /// `log()` lines stream in as BgProgress, the result lands via BgDone,
    /// and Esc aborts through the turn-busy slot like any other turn.
    fn spawn_workflow_run(&mut self, name: String, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.active_tab().is_busy() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "busy — finish or interrupt the current turn first",
            ));
            return;
        }
        let tab_id = self.active_tab().id;
        let engine = self.active_tab().engine.clone();
        let Some((op_id, abort)) = self.begin_local_operation(self.active) else {
            return;
        };
        self.active_tab_mut().mode = Mode::Thinking;
        self.active_tab_mut()
            .chat
            .push_system(&format!("{} '{name}'…", crate::tr("running workflow")));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let log_tx = tx.clone();
            let log: zode_core::workflows_js::LogSink = Arc::new(move |line| {
                let _ = log_tx.send(AppEvent::BgProgress {
                    tab_id,
                    op_id,
                    line: format!("  {line}"),
                });
            });
            let result = engine
                .run_workflow_named(&name, serde_json::Value::Null, log, abort)
                .await
                .map(|value| {
                    let pretty =
                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
                    let mut line = format!("workflow '{name}' → {pretty}");
                    if line.len() > 4000 {
                        truncate_at_char_boundary(&mut line, 4000);
                        line.push('…');
                    }
                    line
                })
                .map_err(|e| format!("workflow '{name}': {e}"));
            let _ = tx.send(AppEvent::BgDone {
                tab_id,
                op_id,
                result,
            });
        });
    }

    fn spawn_op_call(
        &mut self,
        tool: String,
        args: serde_json::Value,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        use zode_core::openpencil::connection::OpConnection;
        use zode_core::openpencil::tools::QueueConsent;
        use zode_core::openpencil::Consent;

        if self.active_tab().is_busy() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "busy — finish or interrupt the current turn first",
            ));
            return;
        }
        let tab_id = self.active_tab().id;
        let cfg = self.active_tab().engine.openpencil.clone();
        let consent: Arc<dyn Consent> = Arc::new(QueueConsent::new(
            self.question_queue.clone(),
            Some(tab_id.to_string()),
        ));
        let tag = cfg.release_tag().to_string();
        // Reuse the turn-busy machinery: spinner shows, Esc clears it.
        let Some((op_id, abort)) = self.begin_local_operation(self.active) else {
            return;
        };
        self.active_tab_mut()
            .chat
            .push_system(&format!("{} {tool}…", crate::tr("calling op")));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let _ = &abort; // keep the controller alive for the duration
            let result = match OpConnection::ensure(&cfg, consent.as_ref(), &tag, &abort).await {
                Ok(client) => match client.call(&tool, args).await {
                    Ok(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())),
                    Err(e) => Err(format!("/op {tool} failed: {e}")),
                },
                Err(e) => Err(format!("/op: {e}")),
            };
            let _ = tx.send(AppEvent::BgDone {
                tab_id,
                op_id,
                result,
            });
        });
    }

    /// Run a `/browser launch|close|screenshot` op OFF the event loop (mirrors
    /// `spawn_op_call`): each may block on a browser launch or a CDP round
    /// trip, so it streams progress via the turn-busy slot and posts its
    /// result back as `AppEvent::BgDone`.
    fn spawn_browser_op(&mut self, op: BrowserOp, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.active_tab().is_busy() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "busy — finish or interrupt the current turn first",
            ));
            return;
        }
        let tab_id = self.active_tab().id;
        let session = self.active_tab().engine.browser.clone();
        let Some((op_id, abort)) = self.begin_local_operation(self.active) else {
            return;
        };
        let label = match &op {
            BrowserOp::Launch => "launching browser",
            BrowserOp::Close => "closing browser",
            BrowserOp::Screenshot { .. } => "capturing screenshot",
        };
        self.active_tab_mut()
            .chat
            .push_system(&format!("{}…", crate::tr(label)));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let _ = &abort; // keep the controller alive for the duration
            let result = run_browser_op(session, op).await;
            let _ = tx.send(AppEvent::BgDone {
                tab_id,
                op_id,
                result,
            });
        });
    }

    /// Run the design-pipeline orchestrator (`/op generate`) OFF the event loop.
    /// The plan→skeleton→content→refine run streams an LLM for many seconds; run
    /// inline it froze the whole TUI. This mirrors `spawn_compact`: it takes the
    /// turn-busy slot (spinner + Esc-to-cancel — the stored abort clone shares
    /// the cancel token with the task), streams each phase into the transcript,
    /// and posts the final summary (including any per-section failures) back.
    fn spawn_op_generate(&mut self, prompt: String, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        use zode_core::openpencil::connection::OpConnection;
        use zode_core::openpencil::design::{
            load_guidance, DesignOrchestrator, DirectLlmContentGenerator,
        };
        use zode_core::openpencil::tools::QueueConsent;
        use zode_core::openpencil::Consent;

        if self.active_tab().is_busy() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "busy — finish or interrupt the current turn first",
            ));
            return;
        }
        let tab_id = self.active_tab().id;
        let cfg = self.active_tab().engine.openpencil.clone();
        let (provider, model, skills) = {
            let eng = &self.active_tab().engine;
            (eng.provider.clone(), eng.model.clone(), eng.skills.clone())
        };
        let consent: Arc<dyn Consent> = Arc::new(QueueConsent::new(
            self.question_queue.clone(),
            Some(tab_id.to_string()),
        ));
        let tag = cfg.release_tag().to_string();
        let Some((op_id, abort)) = self.begin_local_operation(self.active) else {
            return;
        };
        self.active_tab_mut()
            .chat
            .push_system(&format!("{}: {prompt}", crate::tr("generating design")));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = match OpConnection::ensure(&cfg, consent.as_ref(), &tag, &abort).await {
                Ok(client) => {
                    let g =
                        load_guidance(skills.as_ref(), &["frontend-design", "openpencil-design"]);
                    let gen = DirectLlmContentGenerator { provider, model };
                    // Stream each phase into the originating tab's transcript.
                    let ptx = tx.clone();
                    let progress = move |p| {
                        let _ = ptx.send(AppEvent::BgProgress {
                            tab_id,
                            op_id,
                            line: design_progress_line(&p),
                        });
                    };
                    match DesignOrchestrator
                        .run(&client, &gen, &g, &prompt, &abort, &progress)
                        .await
                    {
                        Ok(r) if r.failures.is_empty() => {
                            Ok(format!("✓ generated {} sections", r.section_ids.len()))
                        }
                        Ok(r) => Ok(format!(
                            "generated {} sections, {} failed:\n{}",
                            r.section_ids.len(),
                            r.failures.len(),
                            r.failures.join("\n"),
                        )),
                        Err(e) => Err(format!("/op generate failed: {e}")),
                    }
                }
                Err(e) => Err(format!("/op: {e}")),
            };
            let _ = tx.send(AppEvent::BgDone {
                tab_id,
                op_id,
                result,
            });
        });
    }

    fn spawn_history_op(&self, agent_tx: &mpsc::UnboundedSender<AppEvent>, undo: bool) {
        let engine = self.active_tab().engine.clone();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let (verb, r) = if undo {
                ("undid", engine.undo().await)
            } else {
                ("redid", engine.redo().await)
            };
            let ev = match r {
                Ok(p) => AppEvent::Toast {
                    text: format!("{verb} {}", p.display()),
                    error: false,
                },
                Err(e) => AppEvent::Toast {
                    text: e.to_string(),
                    error: true,
                },
            };
            let _ = tx.send(ev);
        });
    }

    /// Manually compact the active tab's conversation (`/compact`). Runs the
    /// summarization off-loop (it calls the provider) and reuses the turn-busy
    /// machinery so the UI shows progress and Esc can interrupt it. The result
    /// lands back as a `CompactDone` event.
    fn spawn_compact(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.active_tab().is_busy() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "busy — finish or interrupt the current turn before /compact",
            ));
            return;
        }
        let idx = self.active;
        self.start_compaction(idx, agent_tx, false);
    }

    /// Kick off compaction for a specific tab: reserve the turn-busy slot (so the
    /// spinner shows and Esc can interrupt), flip the status to `Compacting`, and
    /// run the summarization off-loop. The result lands as a `CompactDone` event.
    /// Shared by the manual `/compact` command (`auto: false`) and the
    /// auto-compact trigger (`auto: true`).
    /// Callers must ensure the tab is idle (`!is_busy()`).
    fn start_compaction(
        &mut self,
        tab_idx: usize,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
        auto: bool,
    ) {
        let Some((op_id, abort)) = self.begin_local_operation(tab_idx) else {
            return;
        };
        let tab = &mut self.tabs[tab_idx];
        let tab_id = tab.id;
        let engine = tab.engine.clone();
        // Hand the engine the REAL occupancy (provider-reported badge value):
        // it picks the compaction direction from it, and a transcript that is
        // near/over the window must not be sent whole (the summarize request
        // itself would 400 with context-overflow, deadlocking compaction).
        let context_tokens = tab.context_tokens;
        tab.mode = Mode::Compacting;
        tab.local_op_is_auto_compact = auto;
        tab.chat
            .push_system(crate::tr("compacting the conversation…"));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            // Hard deadline: the summarize call rides the provider's own
            // retry/backoff ladder, and a 5xx storm kept tabs pinned on
            // "compacting" for many minutes with no way to tell hung from
            // slow. A timeout aborts the attempt and lands as a normal
            // failure, so the breaker counts it and the tab recovers.
            let compact = engine.compact_sized(
                (context_tokens > 0).then_some(context_tokens),
                abort.clone(),
            );
            let result = match tokio::time::timeout(COMPACT_OP_TIMEOUT, compact).await {
                Ok(outcome) => outcome
                    .map(|o| {
                        format!(
                            "compacted {} message{} · ~{} → ~{} tokens",
                            o.replaced,
                            if o.replaced == 1 { "" } else { "s" },
                            o.pre_tokens,
                            o.post_tokens,
                        )
                    })
                    .map_err(|e| e.to_string()),
                Err(_) => {
                    abort.abort_with_reason("compaction timed out");
                    Err(format!(
                        "compaction timed out after {}s",
                        COMPACT_OP_TIMEOUT.as_secs()
                    ))
                }
            };
            let _ = tx.send(AppEvent::CompactDone {
                tab_id,
                op_id,
                result,
                auto,
            });
        });
    }

    /// Auto-compact any idle tab whose REAL context occupancy (the badge value,
    /// from the last Usage event) plus the configured completion budget has
    /// reached the engine's pre-turn threshold
    /// ([`zode_core::ZodeEngine::needs_pre_turn_compact`]). The runtime's own
    /// auto-compaction keys off a store estimate that can under-count, so a
    /// long conversation could otherwise start its next turn with so little
    /// headroom that the completion gets clamped to the floor and truncates
    /// mid tool call (or hard-400s on `prompt + max_tokens`). This guard uses
    /// the accurate post-turn count plus the output budget instead, and runs
    /// between turns (before any queued input is dispatched).
    fn maybe_auto_compact(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        for idx in 0..self.tabs.len() {
            let tab = &self.tabs[idx];
            if tab.is_busy() {
                continue;
            }
            // Circuit breaker: a CompactDone(Err) lands as an agent event, and
            // this trigger runs right after every event batch — without the
            // breaker a persistently failing compaction (e.g. provider 400s)
            // would loop start→fail→start forever, one LLM call per lap.
            if tab.auto_compact_failures >= AUTO_COMPACT_MAX_FAILURES {
                continue;
            }
            if tab.engine.needs_pre_turn_compact(tab.context_tokens) {
                self.start_compaction(idx, agent_tx, true);
            }
        }
    }

    fn handle_theme(&mut self, args: &str) {
        if args.is_empty() {
            self.open_theme_picker();
            return;
        }
        if self.theme_store.contains(args) {
            self.theme = self.theme_store.resolve(Some(args));
            if let Ok(mut cfg) = ConfigManager::load_global() {
                cfg.theme = Some(args.to_string());
                let _ = ConfigManager::save_global(&cfg);
            }
            self.active_tab_mut()
                .chat
                .push_system(&format!("{} → {args}", crate::tr("theme")));
        } else {
            self.active_tab_mut()
                .chat
                .push_system(&format!("{}: {args}", crate::tr("unknown theme")));
        }
    }

    fn handle_vision(&mut self, args: &str) {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            // Show the current config, then open the provider picker so the user
            // has an interactive place to configure image understanding.
            let msg = vision_summary(
                self.template.images(),
                self.active_tab().engine.supports_images(),
            );
            self.active_tab_mut().chat.push_system(&msg);
            self.open_vision_picker();
            return;
        }

        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default().trim();
        let mut images = self.template.images().clone();
        let message = match key {
            "mode" => match parse_image_mode(value) {
                Some(mode) => {
                    images.mode = Some(mode);
                    format!("{} -> {}", crate::tr("vision mode"), image_mode_label(mode))
                }
                None => {
                    self.toast = Some(Toast::info(crate::tr(
                        "usage: /vision mode auto|direct|vision-model",
                    )));
                    return;
                }
            },
            "provider" => {
                if value.is_empty() {
                    let providers = self.template.vision_provider_names();
                    let msg = if providers.is_empty() {
                        crate::tr("no image-capable providers configured").to_string()
                    } else {
                        format!(
                            "{}: {}",
                            crate::tr("vision providers"),
                            providers.join(", ")
                        )
                    };
                    self.active_tab_mut().chat.push_system(&msg);
                    return;
                }
                if !self
                    .template
                    .provider_names()
                    .iter()
                    .any(|name| name == value)
                {
                    self.toast = Some(Toast::error(
                        crate::tr("no provider '{name}' in config").replace("{name}", value),
                    ));
                    return;
                }
                if !self
                    .template
                    .vision_provider_names()
                    .iter()
                    .any(|name| name == value)
                {
                    self.toast = Some(Toast::error(
                        crate::tr(
                            "vision provider '{provider_name}' does not declare image support; set supportsImages=true on it (or its models entry) in config.json",
                        )
                        .replace("{provider_name}", value),
                    ));
                    return;
                }
                images.vision_provider = Some(value.to_string());
                images.mode = Some(ImageMode::VisionModel);
                format!("{} -> {value}", crate::tr("vision provider"))
            }
            "prompt" => {
                if value.is_empty() {
                    self.toast = Some(Toast::info(crate::tr("usage: /vision prompt <text>")));
                    return;
                }
                images.vision_prompt = Some(value.to_string());
                crate::tr("vision prompt updated").to_string()
            }
            "clear" | "reset" => {
                images = ImagesConfig::default();
                crate::tr("vision config reset").to_string()
            }
            _ => {
                self.toast = Some(Toast::info(crate::tr(
                    "usage: /vision [mode|provider|prompt|reset]",
                )));
                return;
            }
        };

        if !self.persist_images_config(images) {
            return;
        }
        self.active_tab_mut().chat.push_system(&message);
    }

    /// Save `images` to the global config and update the live template. Returns
    /// false (after toasting) on an IO error. Shared by `/vision` and the vision
    /// provider picker.
    fn persist_images_config(&mut self, images: ImagesConfig) -> bool {
        match ConfigManager::load_global() {
            Ok(mut cfg) => {
                cfg.images = images.clone();
                if let Err(e) = ConfigManager::save_global(&cfg) {
                    self.toast = Some(Toast::error(format!(
                        "{}: {e}",
                        crate::tr("save config failed")
                    )));
                    return false;
                }
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!(
                    "{}: {e}",
                    crate::tr("load config failed")
                )));
                return false;
            }
        }
        self.template = self.template.with_images_config(images);
        true
    }

    /// Set (or clear, on "off") the image-understanding provider — from the
    /// vision picker or the settings dialog. Mirrors `/vision provider <name>`.
    fn apply_vision_provider(&mut self, provider: &str) {
        let mut images = self.template.images().clone();
        let message = if provider == "off" || provider.is_empty() {
            images.vision_provider = None;
            images.mode = Some(ImageMode::Auto);
            crate::tr("vision model disabled (image mode → auto)").to_string()
        } else {
            if !self.template.provider_names().iter().any(|n| n == provider) {
                self.toast = Some(Toast::error(
                    crate::tr("no provider '{name}' in config").replace("{name}", provider),
                ));
                return;
            }
            if !self
                .template
                .vision_provider_names()
                .iter()
                .any(|name| name == provider)
            {
                self.toast = Some(Toast::error(
                    crate::tr("vision provider '{provider_name}' does not declare image support; set supportsImages=true on it (or its models entry) in config.json")
                        .replace("{provider_name}", provider),
                ));
                return;
            }
            images.vision_provider = Some(provider.to_string());
            images.mode = Some(ImageMode::VisionModel);
            format!("{} → {provider}", crate::tr("vision provider"))
        };
        if !self.persist_images_config(images) {
            return;
        }
        self.active_tab_mut().chat.push_system(&message);
    }

    fn handle_tab_command(&mut self, args: &str) {
        match resolve_tab_target(args, self.active, self.tabs.len()) {
            Ok(idx) => self.switch_to(idx),
            Err(msg) => self.active_tab_mut().chat.push_system(&msg),
        }
    }

    fn handle_sidebar_command(&mut self, args: &str) {
        // Section fold toggles (keyboard fallback for the header click).
        let folded = match args.trim().to_ascii_lowercase().as_str() {
            "mcp" => {
                self.mcp_section_collapsed = !self.mcp_section_collapsed;
                Some(("MCP", self.mcp_section_collapsed))
            }
            "lsp" => {
                self.lsp_section_collapsed = !self.lsp_section_collapsed;
                Some(("LSP", self.lsp_section_collapsed))
            }
            "files" => {
                self.files_section_collapsed = !self.files_section_collapsed;
                Some(("modified files", self.files_section_collapsed))
            }
            "todo" => {
                self.todo_section_collapsed = !self.todo_section_collapsed;
                Some(("Todo", self.todo_section_collapsed))
            }
            _ => None,
        };
        if let Some((section, collapsed)) = folded {
            let state = if collapsed { "folded" } else { "expanded" };
            self.active_tab_mut()
                .chat
                .push_system(&format!("{} -> {state}", crate::tr(section)));
            return;
        }
        match resolve_sidebar_visibility(args, self.sidebar_visibility, self.tabs.len()) {
            Ok(visibility) => {
                self.sidebar_visibility = visibility;
                let state = match visibility {
                    SidebarVisibility::Auto => "auto",
                    SidebarVisibility::Visible => "visible",
                    SidebarVisibility::Hidden => "hidden",
                };
                self.active_tab_mut()
                    .chat
                    .push_system(&format!("{} -> {state}", crate::tr("sidebar")));
            }
            Err(msg) => self.active_tab_mut().chat.push_system(&msg),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarVisibility {
    Auto,
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatMouseScroll {
    Up(u16),
    Down(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FragmentedCursorSeqState {
    AfterEsc,
    AfterEscO,
    AfterEscBracket,
    MaybeBareO {
        count: usize,
    },
    /// Mid SGR mouse report (`<Cb;Cx;Cy` so far) reached via a lost/fragmented
    /// `ESC[`. `buf` holds the bytes seen so they can be replayed verbatim if
    /// the run turns out not to be a real report.
    MaybeSgrMouse {
        buf: String,
    },
    /// A `[` seen right after a swallowed report — likely the next report's
    /// `ESC[` with the ESC lost. Held tentatively so it can be replayed.
    MaybeSgrBracket,
    /// Just swallowed a complete SGR mouse report; a following bare `[`/`<`
    /// continues a back-to-back momentum flood.
    AfterSgrMouse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FragmentedCursorAction {
    None,
    ReplayBareO(usize),
    /// Give back tentatively-buffered bytes (a `<…`/`[` run that wasn't a mouse
    /// report); the caller inserts them, then handles the current key.
    ReplaySgr(String),
    Consumed,
    Scroll(ChatMouseScroll),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPickerMouseScroll {
    Up(usize),
    Down(usize),
}

const CHAT_WHEEL_SCROLL_LINES: u16 = 1;
const SESSION_PICKER_MOUSE_SCROLL_ROWS: usize = 1;

/// Upper bound on how many already-buffered terminal events one loop iteration
/// drains before redrawing. A trackpad/mouse-wheel momentum flick can deliver
/// dozens to hundreds of scroll events at once; the cap keeps a sustained flood
/// from starving the agent/approval/question `select!` branches.
const INPUT_COALESCE_CAP: usize = 1024;
const WINDOWS_BRACKETED_PASTE_START: &str = "\u{1b}[200~";
const WINDOWS_BRACKETED_PASTE_END: &str = "\u{1b}[201~";
const WINDOWS_PASTE_EVENT_CAP: usize = 65_536;
const WINDOWS_PASTE_TEXT_BYTE_CAP: usize = WINDOWS_PASTE_EVENT_CAP * 4;
const WINDOWS_CLIPBOARD_READ_TIMEOUT: Duration = Duration::from_secs(1);

/// Max agent events (streaming text deltas, tool updates) drained per loop
/// iteration before redrawing. Each delta otherwise triggers a full-transcript
/// re-render at the top of the loop; coalescing a burst into one draw keeps
/// streaming smooth on long conversations. Capped so a sustained flood can't
/// starve the input/approval/tick branches.
const AGENT_COALESCE_CAP: usize = 1024;

/// Pull every terminal event that is *already buffered* — without awaiting —
/// up to `cap`. Returns the burst so the caller can handle it and redraw ONCE,
/// instead of once per event. Stops at the cap, at the first not-yet-ready
/// poll, or at end-of-stream / a read error (the next `select!` await picks
/// those back up). This is what stops over-scrolling at the top/bottom from
/// feeling frozen: the redraw storm collapses into one redraw per flick.
///
/// `now_or_never` polls with a noop waker, so a final not-ready probe leaves no
/// useful waker registered. That's safe here: the caller loops straight back to
/// `select!`, which re-polls this stream with the real task waker before it ever
/// parks, and the 100ms tick is a liveness backstop regardless.
/// Ctrl+C — the force-quit chord accepted while the app is draining shutdown.
fn is_force_quit_event(ev: &CtEvent) -> bool {
    matches!(
        ev,
        CtEvent::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers,
            ..
        }) if modifiers.contains(KeyModifiers::CONTROL)
    )
}

fn drain_ready_events<S>(stream: &mut S, cap: usize) -> Vec<CtEvent>
where
    S: futures::Stream<Item = std::io::Result<CtEvent>> + Unpin,
{
    let mut out = Vec::new();
    while out.len() < cap {
        match stream.next().now_or_never() {
            Some(Some(Ok(ev))) => out.push(ev),
            _ => break,
        }
    }
    out
}

fn extend_windows_text_burst<S>(
    stream: &mut S,
    burst: &mut Vec<CtEvent>,
    mut previous_chunk_full: bool,
) where
    S: futures::Stream<Item = std::io::Result<CtEvent>> + Unpin,
{
    while previous_chunk_full
        && burst.len() < WINDOWS_PASTE_EVENT_CAP
        && windows_burst_has_text_tail(burst)
    {
        let cap = INPUT_COALESCE_CAP.min(WINDOWS_PASTE_EVENT_CAP - burst.len());
        let mut more = drain_ready_events(stream, cap);
        previous_chunk_full = more.len() == cap;
        burst.append(&mut more);
    }
}

fn normalize_paste_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn windows_supported_key_char(key: &KeyEvent) -> Option<char> {
    let command_modifiers = KeyModifiers::CONTROL
        | KeyModifiers::ALT
        | KeyModifiers::SUPER
        | KeyModifiers::HYPER
        | KeyModifiers::META;
    if key.modifiers.intersects(command_modifiers) {
        return None;
    }
    match key.code {
        KeyCode::Char(character) => Some(character),
        KeyCode::Enter => Some('\n'),
        KeyCode::Tab => Some('\t'),
        KeyCode::Esc => Some('\u{1b}'),
        _ => None,
    }
}

fn windows_burst_has_text_tail(events: &[CtEvent]) -> bool {
    for event in events.iter().rev() {
        match event {
            CtEvent::Key(key) if key.kind == crossterm::event::KeyEventKind::Release => continue,
            CtEvent::Key(key) => return windows_supported_key_char(key).is_some(),
            _ => return false,
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsPasteSegment {
    events: Range<usize>,
    text: String,
}

#[derive(Debug)]
struct WindowsTextBoundary {
    byte: usize,
    event: usize,
    keys: usize,
}

#[derive(Debug)]
struct WindowsTextUnit {
    byte_start: usize,
    event_start: usize,
    key_count: usize,
}

#[derive(Debug)]
struct WindowsTextRunBuilder {
    text: String,
    units: Vec<WindowsTextUnit>,
    event_end: usize,
    previous_was_cr: bool,
}

impl WindowsTextRunBuilder {
    fn new() -> Self {
        Self {
            text: String::new(),
            units: Vec::new(),
            event_end: 0,
            previous_was_cr: false,
        }
    }

    fn push(&mut self, event: usize, character: char) {
        self.event_end = event + 1;
        let was_cr = character == '\r';
        if character == '\n' && self.previous_was_cr {
            if let Some(unit) = self.units.last_mut() {
                unit.key_count += 1;
            }
            self.previous_was_cr = false;
            return;
        }

        let character = if character == '\r' { '\n' } else { character };
        let byte_start = self.text.len();
        self.text.push(character);
        self.units.push(WindowsTextUnit {
            byte_start,
            event_start: event,
            key_count: 1,
        });
        self.previous_was_cr = was_cr;
    }

    fn include_duplicate_press(&mut self, event: usize) {
        self.event_end = event + 1;
        if let Some(unit) = self.units.last_mut() {
            unit.key_count += 1;
        }
    }

    fn finish(self) -> WindowsTextRun {
        let mut boundaries = Vec::with_capacity(self.units.len() + 1);
        let mut keys = 0;
        for unit in &self.units {
            boundaries.push(WindowsTextBoundary {
                byte: unit.byte_start,
                event: unit.event_start,
                keys,
            });
            keys += unit.key_count;
        }
        boundaries.push(WindowsTextBoundary {
            byte: self.text.len(),
            event: self.event_end,
            keys,
        });
        WindowsTextRun {
            text: self.text,
            boundaries,
        }
    }
}

#[derive(Debug)]
struct WindowsTextRun {
    text: String,
    boundaries: Vec<WindowsTextBoundary>,
}

impl WindowsTextRun {
    fn boundary(&self, byte: usize) -> Option<&WindowsTextBoundary> {
        self.boundaries
            .binary_search_by_key(&byte, |boundary| boundary.byte)
            .ok()
            .map(|index| &self.boundaries[index])
    }

    fn event_range(&self, bytes: &Range<usize>) -> Option<Range<usize>> {
        let start = self.boundary(bytes.start)?;
        let end = self.boundary(bytes.end)?;
        Some(start.event..end.event)
    }

    fn key_count(&self, bytes: &Range<usize>) -> Option<usize> {
        let start = self.boundary(bytes.start)?;
        let end = self.boundary(bytes.end)?;
        Some(end.keys.saturating_sub(start.keys))
    }
}

fn finish_windows_text_run(
    current: &mut Option<WindowsTextRunBuilder>,
    runs: &mut Vec<WindowsTextRun>,
) {
    if let Some(run) = current.take() {
        runs.push(run.finish());
    }
}

fn windows_duplicate_non_bmp_press(events: &[CtEvent], index: usize) -> Option<char> {
    // Crossterm's Windows surrogate decoder loses key-up kind information:
    // the UTF-16 down and up pairs both become identical Press events. Collapse
    // that adjacent non-BMP pair here while retaining both source event slots.
    let (CtEvent::Key(first), CtEvent::Key(second)) = (events.get(index)?, events.get(index + 1)?)
    else {
        return None;
    };
    if first.kind != crossterm::event::KeyEventKind::Press
        || second.kind != crossterm::event::KeyEventKind::Press
        || first.modifiers != second.modifiers
        || first.code != second.code
    {
        return None;
    }
    let character = windows_supported_key_char(first)?;
    (character.len_utf16() == 2).then_some(character)
}

fn windows_text_runs(events: &[CtEvent]) -> Vec<WindowsTextRun> {
    if events.len() > WINDOWS_PASTE_EVENT_CAP {
        return Vec::new();
    }

    let mut runs = Vec::new();
    let mut current: Option<WindowsTextRunBuilder> = None;
    let mut index = 0;
    while index < events.len() {
        if let Some(character) = windows_duplicate_non_bmp_press(events, index) {
            let run = current.get_or_insert_with(WindowsTextRunBuilder::new);
            run.push(index, character);
            run.include_duplicate_press(index + 1);
            index += 2;
            continue;
        }

        let event = &events[index];
        match event {
            CtEvent::Key(key) if key.kind == crossterm::event::KeyEventKind::Release => {
                if let Some(run) = &mut current {
                    run.event_end = index + 1;
                }
            }
            CtEvent::Key(key) => {
                if let Some(character) = windows_supported_key_char(key) {
                    current
                        .get_or_insert_with(WindowsTextRunBuilder::new)
                        .push(index, character);
                } else {
                    finish_windows_text_run(&mut current, &mut runs);
                }
            }
            _ => finish_windows_text_run(&mut current, &mut runs),
        }
        index += 1;
    }
    finish_windows_text_run(&mut current, &mut runs);
    runs
}

#[derive(Debug)]
struct WindowsBracketedFrame {
    source: Range<usize>,
    body: Range<usize>,
}

fn windows_bracketed_frames(text: &str) -> Vec<WindowsBracketedFrame> {
    let mut frames = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = text[cursor..].find(WINDOWS_BRACKETED_PASTE_START) {
        let start = cursor + relative_start;
        let body_start = start + WINDOWS_BRACKETED_PASTE_START.len();
        let Some(relative_end) = text[body_start..].find(WINDOWS_BRACKETED_PASTE_END) else {
            break;
        };
        let body_end = body_start + relative_end;
        let frame_end = body_end + WINDOWS_BRACKETED_PASTE_END.len();
        if text[body_start..body_end].contains(WINDOWS_BRACKETED_PASTE_START) {
            cursor = frame_end;
            continue;
        }
        frames.push(WindowsBracketedFrame {
            source: start..frame_end,
            body: body_start..body_end,
        });
        cursor = frame_end;
    }
    frames
}

fn windows_unoccupied_ranges(
    text_len: usize,
    frames: &[WindowsBracketedFrame],
) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(frames.len() + 1);
    let mut start = 0;
    for frame in frames {
        if start < frame.source.start {
            ranges.push(start..frame.source.start);
        }
        start = frame.source.end;
    }
    if start < text_len {
        ranges.push(start..text_len);
    }
    ranges
}

fn normalized_windows_clipboard(clipboard_text: Option<&str>) -> Option<String> {
    let clipboard_text = clipboard_text?;
    if clipboard_text.len() > WINDOWS_PASTE_TEXT_BYTE_CAP {
        return None;
    }
    let clipboard = normalize_paste_newlines(clipboard_text);
    (clipboard.contains('\n') && clipboard.chars().count() >= 2).then_some(clipboard)
}

fn append_windows_clipboard_segments(
    run: &WindowsTextRun,
    gap: Range<usize>,
    clipboard: &str,
    segments: &mut Vec<WindowsPasteSegment>,
) {
    let mut cursor = gap.start;
    while cursor <= gap.end && clipboard.len() <= gap.end - cursor {
        let Some(relative_start) = run.text[cursor..gap.end].find(clipboard) else {
            break;
        };
        let start = cursor + relative_start;
        let end = start + clipboard.len();
        let bytes = start..end;
        if run.key_count(&bytes).is_some_and(|count| count >= 2) {
            if let Some(events) = run.event_range(&bytes) {
                segments.push(WindowsPasteSegment {
                    events,
                    text: clipboard.to_string(),
                });
            }
        }
        cursor = end;
    }
}

fn windows_paste_segments(
    events: &[CtEvent],
    clipboard_text: Option<&str>,
) -> Vec<WindowsPasteSegment> {
    let clipboard = normalized_windows_clipboard(clipboard_text);
    let mut segments = Vec::new();
    for run in windows_text_runs(events) {
        let frames = windows_bracketed_frames(&run.text);
        for frame in &frames {
            if let Some(events) = run.event_range(&frame.source) {
                segments.push(WindowsPasteSegment {
                    events,
                    text: run.text[frame.body.clone()].to_string(),
                });
            }
        }
        if let Some(clipboard) = clipboard.as_deref() {
            for gap in windows_unoccupied_ranges(run.text.len(), &frames) {
                append_windows_clipboard_segments(&run, gap, clipboard, &mut segments);
            }
        }
    }
    segments.sort_by_key(|segment| segment.events.start);
    segments
}

fn windows_burst_needs_clipboard(events: &[CtEvent]) -> bool {
    for run in windows_text_runs(events) {
        let frames = windows_bracketed_frames(&run.text);
        for gap in windows_unoccupied_ranges(run.text.len(), &frames) {
            if run.text[gap.clone()].contains('\n')
                && run.key_count(&gap).is_some_and(|count| count >= 2)
            {
                return true;
            }
        }
    }
    false
}

/// Consecutive auto-compaction failures per tab before the auto trigger stops
/// firing (manual `/compact` stays available; any success resets the count).
const AUTO_COMPACT_MAX_FAILURES: u32 = 3;

/// Hard deadline for one compaction operation (the summarize call plus store
/// splice). Long enough for a slow model on a big transcript; short enough
/// that a provider outage can't pin a tab on "compacting" indefinitely.
const COMPACT_OP_TIMEOUT: Duration = Duration::from_secs(300);

/// Wrap a mid-turn interjection for the model: an explicit priority framing so
/// the running turn treats it as an override, not a side note — weak models
/// otherwise acknowledge a steered constraint ("only search in X") and keep
/// executing the old plan. The chat transcript shows only the raw user text.
fn steer_payload(text: &str) -> String {
    format!(
        "<system-reminder>The user interjected mid-task with the instruction below. It takes \
         precedence over your current plan and any earlier approach — comply with it \
         immediately, starting with your very next action.</system-reminder>\n{text}"
    )
}

/// Halt the goal auto-loop for a tab: clear the active flag, reset the turn
/// counter, and PURGE any goal-loop prompts still sitting in the input queue so
/// a stale continuation can't dispatch after the loop was stopped (by
/// `GoalComplete`, the cap, a failed turn, an interrupt, or `/goal clear`).
/// User-typed follow-ups in the queue are preserved.
/// Await the desktop-Esc fire channel; pends forever once it's unavailable
/// (a non-TUI owner took it, or a prior iteration dropped it) so the select
/// arm simply never fires instead of busy-looping on a closed channel.
async fn recv_desktop_esc(rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<()>>) -> Option<()> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

fn stop_goal_loop(tab: &mut SessionTab) {
    tab.goal_loop_active = false;
    tab.goal_loop_iter = 0;
    tab.goal_no_progress_streak = 0;
    tab.goal_text = None;
    tab.goal_started_at = None;
    tab.queued_input
        .retain(|s| s != GOAL_LOOP_CONTINUE_PROMPT && s != GOAL_LOOP_START_PROMPT);
}

/// A compact elapsed-time label for the sidebar goal section (e.g. `45s`,
/// `2m 05s`, `1h 03m`).
fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Ready => "ready",
        Mode::Thinking => "thinking",
        Mode::Streaming => "streaming",
        Mode::Compacting => "compacting",
        Mode::Switching => "switching",
        Mode::Error => "error",
    }
}

/// The primary chord modifier for app shortcuts is Ctrl. On macOS we also
/// accept Command/SUPER as a compatibility alias when the terminal delivers it.
fn is_primary_mod(m: KeyModifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.contains(KeyModifiers::SUPER) || m.contains(KeyModifiers::CONTROL)
    } else {
        m.contains(KeyModifiers::CONTROL)
    }
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

fn should_show_sidebar(tab_count: usize, visibility: SidebarVisibility) -> bool {
    match visibility {
        SidebarVisibility::Auto => tab_count > 1,
        SidebarVisibility::Visible => true,
        SidebarVisibility::Hidden => false,
    }
}

fn chat_scroll_from_mouse(
    kind: MouseEventKind,
    column: u16,
    row: u16,
    chat_area: Rect,
) -> Option<ChatMouseScroll> {
    if !rect_contains(chat_area, column, row) {
        return None;
    }

    match kind {
        MouseEventKind::ScrollUp => Some(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES)),
        MouseEventKind::ScrollDown => Some(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES)),
        _ => None,
    }
}

fn selection_scroll_from_drag(
    kind: MouseEventKind,
    column: u16,
    row: u16,
    chat_area: Rect,
) -> Option<ChatMouseScroll> {
    if !matches!(kind, MouseEventKind::Drag(MouseButton::Left)) || chat_area.height == 0 {
        return None;
    }
    let left = chat_area.x;
    let right = chat_area.x.saturating_add(chat_area.width);
    if column < left || column >= right {
        return None;
    }
    let top = chat_area.y;
    let bottom = chat_area
        .y
        .saturating_add(chat_area.height.saturating_sub(1));
    if row <= top {
        Some(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES))
    } else if row >= bottom {
        Some(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES))
    } else {
        None
    }
}

fn chat_scroll_from_alt_scroll_key(
    _code: KeyCode,
    _modifiers: KeyModifiers,
    _input_is_empty: bool,
) -> Option<ChatMouseScroll> {
    // Once crossterm has parsed an arrow key, a terminal-generated
    // alternate-scroll arrow is indistinguishable from the user pressing
    // Up/Down. Prefer prompt history; fragmented raw OA/OB sequences are
    // still handled by `fragmented_cursor_sequence_action`.
    None
}

fn fragmented_cursor_sequence_action(
    state: &mut Option<FragmentedCursorSeqState>,
    code: KeyCode,
    modifiers: KeyModifiers,
    input_is_empty: bool,
) -> FragmentedCursorAction {
    use FragmentedCursorAction as Action;
    use FragmentedCursorSeqState as St;
    let up = || Action::Scroll(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES));
    let down = || Action::Scroll(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES));

    // A modifier can't belong to a fragmented escape/mouse sequence (those
    // arrive as bare chars). Abort any pending run, replaying buffered bytes so
    // nothing the user typed is silently eaten.
    if !modifiers.is_empty() {
        return match state.take() {
            Some(St::MaybeBareO { count }) => Action::ReplayBareO(count),
            Some(St::MaybeSgrMouse { buf }) => Action::ReplaySgr(buf),
            Some(St::MaybeSgrBracket) => Action::ReplaySgr("[".to_string()),
            _ => Action::None,
        };
    }

    match state.take() {
        Some(St::AfterEsc) => match code {
            KeyCode::Char('O') => {
                *state = Some(St::AfterEscO);
                Action::Consumed
            }
            KeyCode::Char('[') => {
                *state = Some(St::AfterEscBracket);
                Action::Consumed
            }
            _ => Action::None,
        },
        Some(St::MaybeBareO { count }) => match code {
            KeyCode::Up | KeyCode::Char('A') => up(),
            KeyCode::Down | KeyCode::Char('B') => down(),
            KeyCode::Char('C') | KeyCode::Char('D') => Action::Consumed,
            KeyCode::Char('O') if input_is_empty => {
                *state = Some(St::MaybeBareO {
                    count: count.saturating_add(1),
                });
                Action::Consumed
            }
            _ => Action::ReplayBareO(count),
        },
        Some(St::AfterEscO) | Some(St::AfterEscBracket) => match code {
            KeyCode::Char('A') => up(),
            KeyCode::Char('B') => down(),
            KeyCode::Char('C') | KeyCode::Char('D') => Action::Consumed,
            // `ESC [ < … M/m` — a fragmented SGR mouse report. Start swallowing.
            KeyCode::Char('<') => {
                *state = Some(St::MaybeSgrMouse {
                    buf: String::from("<"),
                });
                Action::Consumed
            }
            _ => Action::None,
        },
        Some(St::MaybeSgrMouse { buf }) => sgr_mouse_step(state, buf, code),
        Some(St::AfterSgrMouse) => match code {
            // Back-to-back reports in a momentum flood: the next report's ESC
            // was lost, so a bare `[`/`<` begins the following sequence.
            KeyCode::Char('[') => {
                *state = Some(St::MaybeSgrBracket);
                Action::Consumed
            }
            KeyCode::Char('<') => {
                *state = Some(St::MaybeSgrMouse {
                    buf: String::from("<"),
                });
                Action::Consumed
            }
            KeyCode::Esc => {
                *state = Some(St::AfterEsc);
                Action::Consumed
            }
            KeyCode::Char('O') if input_is_empty => {
                *state = Some(St::MaybeBareO { count: 1 });
                Action::Consumed
            }
            _ => Action::None,
        },
        Some(St::MaybeSgrBracket) => match code {
            // Carry the held `[` so an invalid run replays `[<…` intact (real
            // text like `[<x` typed right after a scroll keeps its `[`).
            KeyCode::Char('<') => {
                *state = Some(St::MaybeSgrMouse {
                    buf: String::from("[<"),
                });
                Action::Consumed
            }
            // Fragmented arrow without ESC: `[` then A/B/C/D.
            KeyCode::Char('A') => up(),
            KeyCode::Char('B') => down(),
            KeyCode::Char('C') | KeyCode::Char('D') => Action::Consumed,
            // Not a sequence after all — give the `[` back, then let the caller
            // handle this key normally.
            _ => Action::ReplaySgr("[".to_string()),
        },
        None => match code {
            KeyCode::Esc => {
                *state = Some(St::AfterEsc);
                Action::Consumed
            }
            KeyCode::Char('O') if input_is_empty => {
                *state = Some(St::MaybeBareO { count: 1 });
                Action::Consumed
            }
            // Bare SGR mouse report whose `ESC[` was lost to fragmentation.
            KeyCode::Char('<') => {
                *state = Some(St::MaybeSgrMouse {
                    buf: String::from("<"),
                });
                Action::Consumed
            }
            _ => Action::None,
        },
    }
}

/// One step inside a fragmented SGR mouse report. `buf` holds the bytes seen so
/// far — an optional leading `[` (a swallowed report's lost `ESC[`) then `<`
/// then `Cb;Cx;Cy`. Digits and `;` accumulate; `M`/`m` completes ONLY if the
/// run is a well-formed report (`<` + exactly three non-empty numeric fields),
/// in which case it is dropped (it reaches the key stream only because crossterm
/// fragmented the real Mouse event, and letting the raw `<64;48;27M` bytes
/// through would type them into the input). Anything that breaks the shape —
/// including a premature `M`/`m` like the `<M` in `Vec<M>` — replays the
/// buffered bytes so real text is never eaten.
fn sgr_mouse_step(
    state: &mut Option<FragmentedCursorSeqState>,
    mut buf: String,
    code: KeyCode,
) -> FragmentedCursorAction {
    // Caps `<` + 3 fields; generous for huge terminals, bounded so a stray run
    // can't grow without end.
    const SGR_MOUSE_MAX_LEN: usize = 32;
    match code {
        KeyCode::Char(c) if c.is_ascii_digit() || c == ';' => {
            // Bail BEFORE pushing so the overflow char isn't both replayed and
            // re-handled by the caller (which would duplicate it).
            if buf.len() >= SGR_MOUSE_MAX_LEN {
                FragmentedCursorAction::ReplaySgr(buf)
            } else {
                buf.push(c);
                *state = Some(FragmentedCursorSeqState::MaybeSgrMouse { buf });
                FragmentedCursorAction::Consumed
            }
        }
        KeyCode::Char('M') | KeyCode::Char('m') if is_complete_sgr_mouse_report(&buf) => {
            *state = Some(FragmentedCursorSeqState::AfterSgrMouse);
            FragmentedCursorAction::Consumed
        }
        // Not a real report (premature/extra/missing field, or any other char):
        // give the buffered bytes back; the caller then handles `code` normally.
        _ => FragmentedCursorAction::ReplaySgr(buf),
    }
}

/// True iff `buf` is a complete SGR mouse report body: an optional leading `[`,
/// a `<`, then exactly three non-empty all-digit fields separated by `;`
/// (`Cb;Cx;Cy`). The terminating `M`/`m` is not part of `buf`.
fn is_complete_sgr_mouse_report(buf: &str) -> bool {
    let body = buf.strip_prefix('[').unwrap_or(buf);
    let Some(fields) = body.strip_prefix('<') else {
        return false;
    };
    let mut parts = fields.split(';');
    let valid = |p: Option<&str>| matches!(p, Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    valid(parts.next()) && valid(parts.next()) && valid(parts.next()) && parts.next().is_none()
}

fn session_picker_scroll_from_mouse(kind: MouseEventKind) -> Option<SessionPickerMouseScroll> {
    match kind {
        MouseEventKind::ScrollUp => Some(SessionPickerMouseScroll::Up(
            SESSION_PICKER_MOUSE_SCROLL_ROWS,
        )),
        MouseEventKind::ScrollDown => Some(SessionPickerMouseScroll::Down(
            SESSION_PICKER_MOUSE_SCROLL_ROWS,
        )),
        _ => None,
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

/// A tool's result payload rendered for the chat — stdout for shells,
/// `content` for file reads, etc. The FULL output is kept: tool rows are
/// collapsed to a `▸ … (+N)` header by default, and expanding one must show
/// everything, not a 12-line teaser. Only a distant safety net caps
/// pathological payloads (e.g. a runaway MCP tool). `None` when there's
/// nothing worth showing beyond the "done" status (e.g. an edit that only
/// returns `{path, status}`).
fn tool_output_preview(output: &serde_json::Value) -> Option<String> {
    const MAX_LINES: usize = 10_000;
    const MAX_CHARS: usize = 500_000;

    let raw = match output {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            let pick = |k: &str| map.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let mut t = pick("stdout")
                .or_else(|| pick("content"))
                .or_else(|| pick("text"))
                .or_else(|| pick("output"))
                .unwrap_or_default();
            if let Some(err) = pick("stderr").filter(|e| !e.trim().is_empty()) {
                if !t.trim().is_empty() {
                    t.push('\n');
                }
                t.push_str(&err);
            }
            t
        }
        _ => String::new(),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut lines: Vec<&str> = trimmed.lines().collect();
    let mut truncated = lines.len() > MAX_LINES;
    lines.truncate(MAX_LINES);
    let mut out = lines.join("\n");
    if out.chars().count() > MAX_CHARS {
        out = out.chars().take(MAX_CHARS).collect();
        truncated = true;
    }
    if truncated {
        out.push_str("\n… (truncated)");
    }
    Some(out)
}

fn process_line_for_event(event: &Event, known_tool: Option<&str>) -> Option<String> {
    process_line_for_event_with_cwd(event, known_tool, None)
}

fn process_line_for_event_with_cwd(
    event: &Event,
    known_tool: Option<&str>,
    cwd: Option<&Path>,
) -> Option<String> {
    match event {
        Event::TextDelta { .. } => None,
        Event::Thinking { delta } => (!delta.is_empty()).then(|| format!("Thinking: {delta}")),
        Event::ToolUse { name, input, .. } => {
            let title = tool_call_title(name, input);
            let summary = tool_input_summary(name, input);
            Some(if summary.is_empty() {
                title
            } else {
                format!("{title} {summary}")
            })
        }
        Event::ToolResult { ok, output, .. } => {
            let status = if *ok { "done" } else { "failed" };
            let mut line = format!("{} {status}", tool_result_title(known_tool));
            if let Some(summary) = file_mutation_result_location_summary(known_tool, output, cwd) {
                line.push(' ');
                line.push_str(&summary);
            }
            // Show the tool's actual output (stdout / file content / …), indented
            // under the status line and truncated. Hidden by `/tool-details off`
            // like the rest of the tool rows.
            if let Some(preview) = tool_output_preview(output) {
                for l in preview.lines() {
                    line.push_str("\n    ");
                    line.push_str(l);
                }
            }
            Some(line)
        }
        Event::Usage {
            input_tokens,
            output_tokens,
            cache_read,
            cache_create,
        } => {
            // Cache hit-rate over the TOTAL prompt. `input_tokens` is the
            // non-cached (full-rate) portion, `cache_read` the cached read,
            // `cache_create` written this turn — so total = the sum and the
            // hit rate is cache_read/total. (Both providers report input as
            // non-cached, so this is consistent.)
            let total = input_tokens
                .saturating_add(*cache_read)
                .saturating_add(*cache_create);
            let note = if (*cache_read > 0 || *cache_create > 0) && total > 0 {
                format!(
                    " · cache {}% ({cache_read})",
                    cache_read.saturating_mul(100) / total
                )
            } else {
                String::new()
            };
            Some(format!("Usage ↑{input_tokens} ↓{output_tokens}{note}"))
        }
        Event::Result { data } => {
            let stop = data.stop_reason.as_deref().unwrap_or("complete");
            if is_quiet_result_stop_reason(stop) {
                return None;
            }
            let model = data
                .model
                .as_deref()
                .map(|m| format!(" · {m}"))
                .unwrap_or_default();
            Some(format!("Result {stop}{model}"))
        }
        // Transient-API-error retries get their own clearer line.
        Event::Notice { code, message } if code == "api_retry" => Some(format!("⟳ {message}")),
        Event::Notice { code, message } => Some(format!("Notice {code}: {message}")),
        Event::Error { code, message } => Some(format!("Error {code}: {message}")),
        Event::Unknown => Some("Event unknown".to_string()),
        _ => Some("Event unknown".to_string()),
    }
}

fn is_quiet_result_stop_reason(stop: &str) -> bool {
    matches!(stop, "end_turn" | "stop" | "complete")
}

fn tool_call_title(name: &str, input: &serde_json::Value) -> String {
    if name == "Skill" {
        let skill = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return format!("Skill {skill}");
    }
    if let Some(rest) = name.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        if let (Some(server), Some(tool)) = (parts.next(), parts.next()) {
            return format!("MCP {server}.{tool}");
        }
    }
    format!("Tool {name}")
}

fn tool_result_title(known_tool: Option<&str>) -> String {
    let Some(name) = known_tool else {
        return "Tool result".to_string();
    };
    if name.starts_with("Tool ") || name.starts_with("Skill ") || name.starts_with("MCP ") {
        name.to_string()
    } else {
        format!("Tool {name}")
    }
}

fn file_mutation_result_location_summary(
    known_tool: Option<&str>,
    output: &serde_json::Value,
    cwd: Option<&Path>,
) -> Option<String> {
    let tool = known_tool
        .map(|name| name.strip_prefix("Tool ").unwrap_or(name))
        .unwrap_or("");
    if !matches!(
        tool,
        "FileWrite" | "FileEdit" | "Mkdir" | "Move" | "Remove" | "NotebookEdit"
    ) {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(obj) = output.as_object() {
        for key in ["path", "from", "to"] {
            if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                parts.push(format!("{key}={value}"));
            }
        }
    }
    if let Some(cwd) = cwd {
        parts.push(format!("cwd={}", cwd.display()));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn tool_input_summary(name: &str, input: &serde_json::Value) -> String {
    if name == "Skill" {
        return String::new();
    }
    let Some(obj) = input.as_object() else {
        return String::new();
    };

    for key in [
        "path",
        "file",
        "command",
        "query",
        "pattern",
        "url",
        "title",
        "agent_type",
    ] {
        if let Some(value) = obj.get(key).and_then(simple_value_summary) {
            return if key == "path" || key == "file" || key == "command" || key == "url" {
                value
            } else {
                format!("{key}={value}")
            };
        }
    }

    obj.iter()
        .filter(|(key, _)| !is_sensitive_key(key))
        .filter_map(|(key, value)| simple_value_summary(value).map(|v| format!("{key}={v}")))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn simple_value_summary(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(truncate_process_value(s)),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn truncate_process_value(value: &str) -> String {
    const MAX_CHARS: usize = 80;
    if value.chars().count() <= MAX_CHARS {
        return value.to_string();
    }
    format!("{}…", value.chars().take(MAX_CHARS).collect::<String>())
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
}

fn resolve_tab_target(args: &str, active: usize, len: usize) -> Result<usize, String> {
    if len == 0 {
        return Err("no tabs open".to_string());
    }
    let value = args.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("next") {
        return Ok((active + 1) % len);
    }
    if value.eq_ignore_ascii_case("prev") || value.eq_ignore_ascii_case("previous") {
        return Ok(active.checked_sub(1).unwrap_or(len - 1));
    }
    let n = value
        .parse::<usize>()
        .map_err(|_| "usage: /tab [n|next|prev]".to_string())?;
    if n == 0 || n > len {
        return Err(format!("tab {n} is out of range (1..{len})"));
    }
    Ok(n - 1)
}

fn resolve_sidebar_visibility(
    args: &str,
    current: SidebarVisibility,
    tab_count: usize,
) -> Result<SidebarVisibility, String> {
    match args.trim().to_ascii_lowercase().as_str() {
        "" | "toggle" => {
            if should_show_sidebar(tab_count, current) {
                Ok(SidebarVisibility::Hidden)
            } else {
                Ok(SidebarVisibility::Visible)
            }
        }
        "off" | "hide" | "close" => Ok(SidebarVisibility::Hidden),
        "on" | "show" | "open" => Ok(SidebarVisibility::Visible),
        "auto" | "default" => Ok(SidebarVisibility::Auto),
        _ => Err("usage: /sidebar [on|off|toggle|auto|mcp|files|todo]".to_string()),
    }
}

fn parse_image_mode(value: &str) -> Option<ImageMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(ImageMode::Auto),
        "direct" => Some(ImageMode::Direct),
        "vision-model" | "vision" | "model" => Some(ImageMode::VisionModel),
        _ => None,
    }
}

fn image_mode_label(mode: ImageMode) -> &'static str {
    match mode {
        ImageMode::Auto => "auto",
        ImageMode::Direct => "direct",
        ImageMode::VisionModel => "vision-model",
    }
}

fn vision_summary(images: &ImagesConfig, active_provider_supports_images: bool) -> String {
    format!(
        "vision mode: {}\nactive provider images: {}\nvision provider: {}\nprompt: {}",
        image_mode_label(images.effective_mode()),
        if active_provider_supports_images {
            "supported"
        } else {
            "not declared"
        },
        images.vision_provider.as_deref().unwrap_or("(not set)"),
        images.effective_prompt()
    )
}

/// Execute a `!<cmd>` shell escape in `cwd` and capture its combined output.
/// Runs on a spawned task, never on the event loop. `kill_on_drop` matters:
/// when the caller's `select!` abandons this future on Esc, the child dies
/// with it instead of lingering.
async fn run_shell_capture(cmd: &str, cwd: &std::path::Path) -> String {
    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    command
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env(
            "GIT_SSH_COMMAND",
            "ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new",
        )
        .env("GCM_INTERACTIVE", "never");
    detach_command_from_controlling_tty(&mut command);

    // Bound the worst case: a timeout caps a hung command and the output size
    // cap prevents `!yes`/`!find /` from growing chat + context until OOM.
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
    const MAX_OUTPUT: usize = 64 * 1024;
    match tokio::time::timeout(TIMEOUT, command.output()).await {
        Ok(Ok(o)) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                if !s.is_empty() && !s.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&err);
            }
            if s.len() > MAX_OUTPUT {
                let mut end = MAX_OUTPUT;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
                s.push_str("\n… (output truncated)");
            }
            // Surface a non-zero exit so the agent sees failures.
            if !matches!(o.status.code(), Some(0) | None) {
                if !s.is_empty() && !s.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&format!("[exit {}]", o.status.code().unwrap_or(-1)));
            }
            s
        }
        Ok(Err(e)) => format!("failed to run command: {e}"),
        Err(_) => format!("command timed out after {}s", TIMEOUT.as_secs()),
    }
}

#[cfg(unix)]
fn detach_command_from_controlling_tty(command: &mut tokio::process::Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(not(unix))]
fn detach_command_from_controlling_tty(_command: &mut tokio::process::Command) {}

/// Format a `!<cmd>` shell escape's command + output as a context note that's
/// prepended to the next prompt, so the agent sees what the user ran locally.
fn format_shell_context(cmd: &str, output: &str) -> String {
    let out = output.trim_end();
    if out.is_empty() {
        format!("I ran the shell command `{cmd}` locally (no output).")
    } else {
        format!("I ran the shell command `{cmd}` locally. Output:\n```\n{out}\n```")
    }
}

fn current_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Render a `ScheduleSpec` back into the syntax `/schedule add` accepts, for
/// echoing in the `schedule added: {spec} (id {id})` confirmation and `list`
/// rows.
fn describe_schedule_spec(spec: &zode_core::scheduler::ScheduleSpec) -> String {
    use zode_core::scheduler::ScheduleSpec;
    match spec {
        ScheduleSpec::Daily { hour, minute } => format!("{hour:02}:{minute:02}"),
        ScheduleSpec::Weekly {
            weekday,
            hour,
            minute,
        } => format!("{} {hour:02}:{minute:02}", weekday_code(*weekday)),
        ScheduleSpec::Interval { secs } => format!("every {}", interval_token(*secs)),
    }
}

/// Render an interval as the single compact `<N><unit>` token
/// `/schedule add every <token> <prompt>` and `parse_interval` accept — NOT
/// `format_duration_ms`'s human `"2h 00m"` form, which `parse_schedule_add`
/// would split on the first whitespace and treat `"00m"` as the start of the
/// prompt, corrupting the round trip. Picks the coarsest exact unit: whole
/// hours, else whole minutes, else seconds.
fn interval_token(secs: u64) -> String {
    if secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// Lowercase 3-letter weekday code, matching `parse_weekday` in
/// `zode_core::commands::loop_sched` (so `describe_schedule_spec`'s output
/// re-parses).
fn weekday_code(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "mon",
        chrono::Weekday::Tue => "tue",
        chrono::Weekday::Wed => "wed",
        chrono::Weekday::Thu => "thu",
        chrono::Weekday::Fri => "fri",
        chrono::Weekday::Sat => "sat",
        chrono::Weekday::Sun => "sun",
    }
}

/// Generate a compact but identity-safe id. Watchdog recovery and enable /
/// disable operations target ids, so collisions are not harmless: retry until
/// the 12-hex UUID prefix is unique in the current roster.
fn gen_schedule_id(existing: &[zode_core::scheduler::ScheduleJob]) -> String {
    loop {
        let full = Uuid::new_v4().simple().to_string();
        let id = full[..12].to_string();
        if existing.iter().all(|job| job.id != id) {
            return id;
        }
    }
}

fn resolve_image_submit_route(
    has_images: bool,
    mode: ImageMode,
    active_provider_supports_images: bool,
    vision_provider_configured: bool,
) -> ImageSubmitRoute {
    if !has_images {
        return ImageSubmitRoute::Direct;
    }
    match mode {
        ImageMode::Direct => {
            if active_provider_supports_images {
                ImageSubmitRoute::Direct
            } else {
                ImageSubmitRoute::Unsupported
            }
        }
        ImageMode::Auto => {
            if active_provider_supports_images {
                ImageSubmitRoute::Direct
            } else if vision_provider_configured {
                ImageSubmitRoute::VisionModel
            } else {
                ImageSubmitRoute::Unsupported
            }
        }
        ImageMode::VisionModel => {
            if vision_provider_configured {
                ImageSubmitRoute::VisionModel
            } else {
                ImageSubmitRoute::Unsupported
            }
        }
    }
}

/// Rebuild a ChatView from a resumed MessageStore so the conversation history
/// is visible after /resume. User messages that carry only tool results are
/// skipped (their tool card already shows under the assistant turn); System /
/// Progress / Tombstone messages are not chat content.
/// Estimate the token footprint of a restored conversation so a resumed session
/// shows a sensible context-usage % immediately (the exact value arrives with
/// the next `Usage` event). Sums the per-message estimate; the fixed system-
/// prompt/tools overhead isn't included, so it's a slight under-count.
fn estimate_store_tokens(store: &MessageStore) -> u32 {
    store
        .iter()
        .map(agent::compact::estimate_tokens)
        .fold(0u32, |acc, t| acc.saturating_add(t))
}

/// Strip a leading noema recall pack from a stored user message so a resumed
/// transcript shows only what the user actually typed.
///
/// `ZodeEngine::inject_noema_memory` prepends `MemoryPack::to_markdown()` —
/// `## Relevant Memories\n…\n## Subconscious Hints\n…` — plus a blank-line
/// separator to the turn's first text block, and that whole thing is persisted.
/// It's context for the model, not user input. Bullet lines never contain a
/// blank line (noema sanitizes inner newlines), so the first blank line after
/// the hints header is the separator before the user's text. Anything that
/// doesn't match the exact pack shape is returned unchanged — better to show a
/// stray header than to silently eat the user's own words.
fn strip_recalled_memory(text: &str) -> &str {
    const HEAD: &str = "## Relevant Memories\n";
    const HINTS: &str = "\n## Subconscious Hints\n";
    if !text.starts_with(HEAD) {
        return text;
    }
    let Some(hints_at) = text.find(HINTS) else {
        return text;
    };
    let after_hints = hints_at + HINTS.len();
    match text[after_hints..].find("\n\n") {
        Some(rel) => text[after_hints + rel..].trim_start_matches('\n'),
        None => text,
    }
}

/// Recognize only the server-generated attachment envelope and return a safe
/// display summary. The body remains persisted for the model/resume path, but
/// must never be copied into task snapshots or rendered as user-authored chat.
fn attached_file_summary(text: &str) -> Option<String> {
    let opening_end = text.find(">\n")?;
    let opening = &text[..=opening_end];
    if !opening.starts_with("<attached_file ") || !text.ends_with("</attached_file>") {
        return None;
    }
    let attribute = |name: &str| {
        let marker = format!("{name}=\"");
        let start = opening.find(&marker)? + marker.len();
        let end = opening[start..].find('"')? + start;
        Some(&opening[start..end])
    };
    let name = attribute("name")?;
    let media_type = attribute("media_type")?;
    let boundary = attribute("boundary")?;
    if !boundary.starts_with("ZODE-ATTACHMENT-zode_attachment_") {
        return None;
    }
    let begin = format!("\n--- BEGIN {boundary} ---\n");
    let end = format!("\n--- END {boundary} ---\n</attached_file>");
    if !text[opening_end + 1..].starts_with(&begin) || !text.ends_with(&end) {
        return None;
    }
    Some(format!("[Attached file: {name} ({media_type})]"))
}

/// Remove engine-injected context before rendering a stored user text block.
/// The browser side-panel hint is model-only, and noema recall must be stripped
/// before classifying attachment-only turns because it prefixes the envelope.
fn stored_user_text_for_display(text: &str) -> String {
    let text = strip_recalled_memory(text);
    if text.trim() == extension_tasks::SIDE_PANEL_BROWSER_CONTEXT.trim() {
        return String::new();
    }
    attached_file_summary(text).unwrap_or_else(|| text.to_string())
}

fn rebuild_chat_from_store(store: &MessageStore) -> ChatView {
    let mut chat = ChatView::new();
    for msg in store.iter() {
        match msg {
            Message::User { content, .. } => {
                let mut text_parts = Vec::new();
                let mut images = Vec::new();
                for (idx, block) in content.iter().enumerate() {
                    match block {
                        ContentBlock::Text { text } if !text.trim().is_empty() => {
                            let shown = stored_user_text_for_display(text);
                            if !shown.trim().is_empty() {
                                text_parts.push(shown);
                            }
                        }
                        ContentBlock::Image { source } => {
                            let media_type = match source {
                                agent::message::ImageSource::Base64 { media_type, .. } => {
                                    media_type.clone()
                                }
                                agent::message::ImageSource::Url { .. } => "image/url".into(),
                                agent::message::ImageSource::File { .. } => "image/file".into(),
                            };
                            images.push(ImagePreview {
                                display_name: format!("attached image {}", idx + 1),
                                media_type,
                                size_bytes: 0,
                            });
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() || !images.is_empty() {
                    // The noema recall pack is prepended to the stored user text
                    // for the model's benefit; it isn't something the user typed,
                    // so a resumed transcript must not render it.
                    let joined = text_parts.join("\n");
                    let shown = strip_recalled_memory(&joined);
                    if !shown.trim().is_empty() || !images.is_empty() {
                        chat.push_user_with_images(shown, images);
                    }
                }
            }
            Message::Assistant { content, .. } => {
                for block in content {
                    match block {
                        ContentBlock::Text { text } => chat.push_delta(text),
                        ContentBlock::Thinking { thinking, .. } => {
                            chat.push_thinking_delta(thinking);
                        }
                        ContentBlock::ToolUse { name, input, .. } => {
                            let title = tool_call_title(name, input);
                            let summary = tool_input_summary(name, input);
                            let line = if summary.is_empty() {
                                title
                            } else {
                                format!("{title} {summary}")
                            };
                            chat.push_tool_call(&line, name);
                        }
                        _ => {}
                    }
                }
                chat.end_turn();
            }
            _ => {}
        }
    }
    chat
}

/// One-line human summary of the sandbox state for `/sandbox`.
fn sandbox_status_line(sandbox: Option<&zode_core::sandbox::SandboxConfig>) -> String {
    use zode_core::sandbox::SandboxMode;
    match sandbox {
        None => "sandbox: OFF — shell commands AND file writes run unconfined".to_string(),
        Some(c) if c.is_windows_tier_two() => format!(
            "sandbox: ON — {}; {}; {}; {}",
            crate::tr("Windows Tier 2 sandbox"),
            crate::tr("best-effort write confinement"),
            crate::tr("network denied (AppContainer; loopback included)"),
            crate::tr(".git/.zode rename/delete through parent is not kernel-enforced")
        ),
        Some(c) if c.is_windows_tier_one() => format!(
            "sandbox: ON — {}; {}; {}; {}",
            crate::tr("Windows Tier 1 sandbox"),
            crate::tr("best-effort write confinement"),
            crate::tr("network unenforced"),
            crate::tr(".git/.zode rename/delete through parent is not kernel-enforced")
        ),
        Some(c) => {
            let mode = match c.mode() {
                SandboxMode::ReadOnly => "read-only (no file writes — shell or tools)".to_string(),
                SandboxMode::WorkspaceWrite => format!(
                    "workspace-write (writes confined to {})",
                    c.write_scope_summary()
                ),
            };
            let net = if c.allow_network() {
                "network allowed"
            } else {
                "network denied"
            };
            format!("sandbox: ON — {mode}; {net}  ·  toggle with /sandbox [off|read-only|workspace-write|network on|network off]")
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write clipboard image `bytes` to a uniquely-named temp file so the chip can
/// be opened in a viewer. Returns the path, or `None` on IO error (the image is
/// still usable — it just won't be openable).
/// Filename prefix for clipboard preview temp files created by THIS process.
fn clipboard_temp_prefix() -> String {
    format!("zode-clip-{}-", std::process::id())
}

fn write_clipboard_temp_image(bytes: &[u8], media_type: &str) -> Option<std::path::PathBuf> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let ext = match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "img",
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{}{nanos}-{n}.{ext}", clipboard_temp_prefix()));
    // create_new (O_EXCL) refuses to follow or clobber a symlink planted at the
    // path, closing the predictable-temp-path redirect; the bytes are written
    // only if WE created the file.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .ok()?;
    if f.write_all(bytes).is_err() {
        drop(f);
        let _ = std::fs::remove_file(&path); // don't leave an empty orphan
        return None;
    }
    Some(path)
}

/// Delete a clipboard preview temp file — but ONLY if it's one we created and
/// tracked (in `temps`). A real user-supplied image path is never in the set,
/// so it's never removed, even if it happens to live in the temp dir.
fn cleanup_clipboard_temp(temps: &mut HashSet<std::path::PathBuf>, path: &std::path::Path) {
    if temps.remove(path) {
        let _ = std::fs::remove_file(path);
    }
}

/// Open `path` in the OS default image viewer.
fn open_in_os_viewer(path: &std::path::Path) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn browser_extension_pairing_url(port: u16, code: &str) -> String {
    format!(
        "chrome-extension://{}/popup.html?port={port}&code={code}&connect=1",
        zode_core::browser::bridge::server::EXTENSION_ID
    )
}

fn browser_extension_connect_url(port: u16) -> String {
    format!(
        "chrome-extension://{}/popup.html?port={port}&connect=1",
        zode_core::browser::bridge::server::EXTENSION_ID
    )
}

fn browser_extension_open_note(
    url: &str,
    result: Result<(), zode_core::browser::BrowserError>,
) -> String {
    match result {
        Ok(()) => "Opened the zode extension page in Chrome.".into(),
        Err(error) => format!("Could not open Chrome automatically: {error}. Open manually: {url}"),
    }
}

fn start_browser_bridge_listener(session: Arc<zode_core::browser::BrowserSession>) {
    if cfg!(test) {
        return;
    }
    if !session.enabled() {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        if matches!(session.target(), zode_core::browser::BrowserTarget::Bridge) {
            ensure_browser_bridge_and_maybe_reconnect(session).await;
        } else if let Err(e) = session.ensure_bridge_listening().await {
            tracing::debug!(error = %e, "browser bridge listener start failed");
        }
    });
}

/// How long to wait for the extension to reconnect ON ITS OWN (saved port +
/// stored token) before nudging it by opening its page in Chrome. Opening the
/// page unconditionally made every zode launch pop a Chrome tab — the "why am
/// I pairing again every time" complaint — even though the extension
/// reconnects by itself within a few seconds in the common case.
const BRIDGE_RECONNECT_GRACE: Duration = Duration::from_secs(8);

async fn ensure_browser_bridge_and_maybe_reconnect(
    session: Arc<zode_core::browser::BrowserSession>,
) {
    match session.ensure_bridge_listening().await {
        Ok(port) if session.bridge_token_available() => {
            let deadline = std::time::Instant::now() + BRIDGE_RECONNECT_GRACE;
            while std::time::Instant::now() < deadline {
                if session.bridge_connected() {
                    return; // the extension found us — no Chrome tab needed
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if session.bridge_connected() {
                return;
            }
            let url = browser_extension_connect_url(port);
            if let Err(e) = session.open_extension_url(&url).await {
                tracing::debug!(error = %e, url = %url, "browser bridge reconnect page open failed");
            }
        }
        Ok(_) => {}
        Err(e) => tracing::debug!(error = %e, "browser bridge listener start failed"),
    }
}

/// How many image chips render before collapsing the rest into a `+N` marker.
/// Keyboard/mouse selection is capped to this so it never targets a hidden chip.
const MAX_VISIBLE_CHIPS: usize = 4;

/// Render the pending-image chips and return per-chip click hitboxes
/// `(col_start, col_end, index)` in absolute terminal columns, so the mouse
/// handler can open the chip under a (Cmd/Ctrl)+click.
fn render_pending_image_chips(
    f: &mut ratatui::Frame,
    area: Rect,
    images: &[ImageAttachment],
    selected: Option<usize>,
    theme: &Theme,
) -> Vec<(u16, u16, usize)> {
    use unicode_width::UnicodeWidthStr;
    let mut hits: Vec<(u16, u16, usize)> = Vec::new();
    if area.width == 0 || area.height == 0 {
        return hits;
    }
    const PREFIX: &str = "▣ ";
    const SEP: &str = "  ";
    let mut spans = vec![Span::styled(
        PREFIX,
        Style::default()
            .bg(theme.bg_input)
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    // Track the absolute column as spans are laid out, to record hitboxes.
    let mut col = area.x.saturating_add(UnicodeWidthStr::width(PREFIX) as u16);
    for (idx, image) in images.iter().take(MAX_VISIBLE_CHIPS).enumerate() {
        if idx > 0 {
            spans.push(Span::styled(
                SEP,
                Style::default().bg(theme.bg_input).fg(theme.fg_subtle),
            ));
            col = col.saturating_add(UnicodeWidthStr::width(SEP) as u16);
        }
        // The selected chip is reverse-highlighted (↑ to select; Backspace to
        // remove; Enter or Cmd/Ctrl+click to view).
        let is_selected = selected == Some(idx);
        let name_style = if is_selected {
            Style::default()
                .bg(theme.accent)
                .fg(theme.bg_input)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .bg(theme.bg_input)
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD)
        };
        let meta = format!(" {}", image.media_type);
        let start = col;
        col = col.saturating_add(UnicodeWidthStr::width(image.display_name.as_str()) as u16);
        col = col.saturating_add(UnicodeWidthStr::width(meta.as_str()) as u16);
        hits.push((start, col, idx));
        spans.push(Span::styled(image.display_name.clone(), name_style));
        spans.push(Span::styled(
            meta,
            Style::default().bg(theme.bg_input).fg(theme.fg_subtle),
        ));
    }
    if images.len() > MAX_VISIBLE_CHIPS {
        spans.push(Span::styled(
            format!("  +{}", images.len() - MAX_VISIBLE_CHIPS),
            Style::default()
                .bg(theme.bg_input)
                .fg(theme.accent_secondary),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg_input)),
        area,
    );
    hits
}

fn image_previews(images: &[ImageAttachment]) -> Vec<ImagePreview> {
    images.iter().map(image_preview).collect()
}

fn image_preview(image: &ImageAttachment) -> ImagePreview {
    ImagePreview {
        display_name: image.display_name.clone(),
        media_type: image.media_type.clone(),
        size_bytes: image.size_bytes,
    }
}

fn user_content_blocks(text: &str, images: &[ImageAttachment]) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if !text.trim().is_empty() {
        blocks.push(ContentBlock::Text {
            text: text.to_string(),
        });
    }
    blocks.extend(images.iter().map(|image| image.content_block.clone()));
    blocks
}

async fn run_vision_description(
    engine: Arc<ZodeEngine>,
    vision_prompt: String,
    user_text: String,
    images: Vec<ImageAttachment>,
    abort: AbortController,
) -> Result<String, String> {
    let mut blocks = Vec::new();
    let mut prompt = vision_prompt;
    if !user_text.trim().is_empty() {
        prompt.push_str("\n\nUser prompt:\n");
        prompt.push_str(user_text.trim());
    }
    prompt.push_str("\n\nReturn only the image description for the main coding model.");
    blocks.push(ContentBlock::Text { text: prompt });
    blocks.extend(images.iter().map(|image| image.content_block.clone()));

    let mut stream = engine
        .turn_blocks_raw(blocks, abort)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        match item.map_err(|e| e.to_string())? {
            Event::TextDelta { delta } => out.push_str(&delta),
            Event::Error { code, message } => {
                return Err(format!("vision model error [{code}]: {message}"));
            }
            _ => {}
        }
    }
    if out.trim().is_empty() {
        Err("vision model returned no image description".to_string())
    } else {
        Ok(out)
    }
}

fn merge_prompt_with_vision(user_text: &str, vision_description: &str) -> String {
    if user_text.trim().is_empty() {
        format!("Image context:\n{}", vision_description.trim())
    } else {
        format!(
            "{}\n\nImage context:\n{}",
            user_text.trim(),
            vision_description.trim()
        )
    }
}

/// Standard base64 encode (no padding omitted) — a tiny inline impl so OSC 52
/// needs no extra dependency.
fn base64_encode(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Copy `text` to the system clipboard via OSC 52 — an escape sequence the
/// TERMINAL turns into a clipboard write. This is how a full-screen TUI copies
/// when it holds mouse capture (so the terminal's own ⌘C/native selection is
/// unavailable): it works in Warp (which shows a one-time "allow clipboard"
/// prompt), iTerm2, kitty, Ghostty, AND over SSH / inside tmux — none of which
/// `pbcopy` covers. tmux/screen need the DCS passthrough wrapper.
fn write_osc52_clipboard(text: &str) {
    use std::io::Write;
    let seq = format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()));
    let payload = if std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() {
        format!("\x1bPtmux;\x1b{seq}\x1b\\")
    } else {
        seq
    };
    let mut out = std::io::stdout();
    let _ = out.write_all(payload.as_bytes());
    let _ = out.flush();
}

/// Whether we pushed the Kitty keyboard-enhancement flags, so restore/panic pop
/// exactly what we pushed (and only on terminals that accepted them).
static KITTY_KEYBOARD_PUSHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// `mouse_capture` (default on) wheel-scrolls the chat and enables in-app
/// drag selection; an alt-screen TUI that doesn't consume wheel events gets
/// its viewport sheared by the terminal's own scrolling (seen in Warp).
/// `"mouseCapture": false` leaves the mouse to the terminal instead: native
/// drag selection, copied by the terminal's own ⌘C — at the cost of the
/// wheel/in-app selection above.
fn setup_terminal(mouse_capture: bool) -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    // Undo raw mode if any subsequent step fails, so we never leave the
    // terminal in a broken state on a setup error.
    if let Err(e) = stdout.execute(EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    if mouse_capture {
        if let Err(e) = stdout.execute(EnableMouseCapture) {
            let _ = stdout.execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(e);
        }
    }
    if let Err(e) = stdout.execute(EnableBracketedPaste) {
        let _ = stdout.execute(DisableMouseCapture);
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        return Err(e);
    }
    // Kitty keyboard protocol (disambiguate): terminals that support it
    // deliver modified chords as CSI-u escape codes, so ⌘C reaches the app
    // where the emulator forwards it (kitty/Ghostty/WezTerm-family).
    // VERIFIED 2026-07 against Warp: it answers the support query, but its
    // own Copy keybinding swallows ⌘C BEFORE the protocol — under full
    // reporting flags only the lone Super press/release (57444) arrives,
    // never Super+C. In Warp copying is therefore served by copy-on-select
    // and the Ctrl+C-with-selection chord instead (a user can rebind Warp's
    // Copy shortcut to hand ⌘C through). Best-effort and gated on support,
    // so terminals without it (Terminal.app) are untouched.
    //
    // NOT crossterm's supports_keyboard_enhancement(): its poll retries
    // forever when the terminal answers neither the kitty query nor DA1
    // (sampled: an unbounded startup hang in kevent under non-answering
    // terminals/ptys). kitty_support_probe is the same query with a hard
    // deadline.
    if kitty_support_probe(Duration::from_millis(800))
        && stdout
            .execute(PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            ))
            .is_ok()
    {
        KITTY_KEYBOARD_PUSHED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(term) => {
            install_panic_hook();
            Ok(term)
        }
        Err(e) => {
            let _ = std::io::stdout().execute(DisableBracketedPaste);
            let _ = std::io::stdout().execute(DisableMouseCapture);
            let _ = std::io::stdout().execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();
            Err(e)
        }
    }
}

/// Bounded kitty-keyboard support probe (see `setup_terminal`): writes the
/// standard `CSI ? u` + DA1 query to /dev/tty and polls the reply against a
/// hard deadline. Runs pre-event-loop (raw mode on, no other tty reader);
/// bytes consumed here can only be keystrokes raced into the startup window.
#[cfg(unix)]
fn kitty_support_probe(timeout: Duration) -> bool {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    let Ok(mut tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        return false;
    };
    if tty
        .write_all(b"\x1b[?u\x1b[c")
        .and_then(|_| tty.flush())
        .is_err()
    {
        return false;
    }
    let deadline = std::time::Instant::now() + timeout;
    let fd = tty.as_raw_fd();
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 256];
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return false;
        }
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut pfd, 1, left.as_millis() as libc::c_int) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        if n == 0 {
            return false; // deadline
        }
        match tty.read(&mut tmp) {
            Ok(0) => return false,
            Ok(k) => buf.extend_from_slice(&tmp[..k]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
        // The kitty reply (`ESC [ ? … u`) confirms support; DA1 (`ESC [ ? … c`)
        // arriving without it means the query is unsupported.
        if csi_private_reply(&buf, b'u') {
            return true;
        }
        if csi_private_reply(&buf, b'c') {
            return false;
        }
    }
}

#[cfg(not(unix))]
fn kitty_support_probe(_timeout: Duration) -> bool {
    false
}

/// Whether `buf` contains a private-mode CSI reply: `ESC [ ? <params>
/// <terminator>` with only digit/`;` parameter bytes in between.
fn csi_private_reply(buf: &[u8], terminator: u8) -> bool {
    let mut i = 0;
    while i + 3 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b'[' && buf[i + 2] == b'?' {
            let mut j = i + 3;
            while j < buf.len() && (buf[j].is_ascii_digit() || buf[j] == b';') {
                j += 1;
            }
            if j < buf.len() && buf[j] == terminator {
                return true;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// Truncate to at most `max_bytes`, backing up to the previous char boundary —
/// a raw `String::truncate` panics when the cut lands inside a multi-byte
/// (e.g. CJK) character.
fn truncate_at_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    // If teardown happens between Begin/EndSynchronizedUpdate (draw error,
    // panic path), unlock the terminal or it keeps presenting the old frame.
    let _ = terminal.backend_mut().execute(EndSynchronizedUpdate);
    disable_raw_mode()?;
    if KITTY_KEYBOARD_PUSHED.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = terminal.backend_mut().execute(PopKeyboardEnhancementFlags);
    }
    terminal.backend_mut().execute(DisableBracketedPaste)?;
    terminal.backend_mut().execute(DisableMouseCapture)?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Restore the terminal on panic so a crash doesn't leave a garbled tty.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic mid-frame can leave a synchronized update open — unlock
        // first or the terminal keeps showing the frozen frame.
        let _ = std::io::stdout().execute(EndSynchronizedUpdate);
        let _ = disable_raw_mode();
        if KITTY_KEYBOARD_PUSHED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let _ = std::io::stdout().execute(PopKeyboardEnhancementFlags);
        }
        let _ = std::io::stdout().execute(DisableBracketedPaste);
        let _ = std::io::stdout().execute(DisableMouseCapture);
        let _ = std::io::stdout().execute(LeaveAlternateScreen);
        original(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chat::Role;
    use zode_core::config::{NoemaSettings, ProviderConfig, ProviderKind, ZodeConfig};

    #[test]
    fn sandbox_status_line_names_tmp_when_writable() {
        use zode_core::sandbox::{SandboxConfig, SandboxMode};
        let Ok(c) = SandboxConfig::new(
            std::path::Path::new("/x"),
            SandboxMode::WorkspaceWrite,
            false,
            &[],
        ) else {
            return; // unsupported OS
        };
        // /tmp is writable by default — the status line must say so, or a user
        // testing the sandbox with /tmp concludes it is broken.
        let line = sandbox_status_line(Some(&c));
        assert!(line.contains("/tmp"), "{line}");
        // Excluded via config → not advertised.
        let line = sandbox_status_line(Some(&c.with_temp_policy(true, true)));
        assert!(!line.contains("/tmp"), "{line}");
        assert!(sandbox_status_line(None).contains("OFF"));
    }

    #[test]
    fn shell_context_note_includes_command_and_output() {
        let note = format_shell_context("ls -la", "file_a\nfile_b\n");
        assert!(note.contains("`ls -la`"));
        assert!(note.contains("file_a"));
        assert!(note.contains("file_b"));
        // Empty output is noted explicitly (no dangling code fence).
        let empty = format_shell_context("true", "   \n");
        assert!(empty.contains("`true`"));
        assert!(empty.contains("no output"));
        assert!(!empty.contains("```"));
    }

    #[test]
    fn browser_extension_pairing_url_auto_connects() {
        let url = browser_extension_pairing_url(17657, "123456");

        assert!(url.starts_with(&format!(
            "chrome-extension://{}/popup.html?",
            zode_core::browser::bridge::server::EXTENSION_ID
        )));
        assert!(url.contains("port=17657"));
        assert!(url.contains("code=123456"));
        assert!(url.contains("connect=1"));
    }

    #[test]
    fn browser_extension_connect_url_uses_stored_token() {
        let url = browser_extension_connect_url(17657);

        assert_eq!(
            url,
            format!(
                "chrome-extension://{}/popup.html?port=17657&connect=1",
                zode_core::browser::bridge::server::EXTENSION_ID
            )
        );
    }

    #[test]
    fn browser_extension_open_note_reports_success() {
        assert_eq!(
            browser_extension_open_note("chrome-extension://zode/popup.html", Ok(())),
            "Opened the zode extension page in Chrome."
        );
    }

    #[test]
    fn browser_extension_open_note_keeps_locator_hint_and_manual_url() {
        let url = "chrome-extension://zode/popup.html?port=17657&code=123456&connect=1";
        let error = zode_core::browser::BrowserError::NotFound(
            "no Google Chrome found; tried [local-app, program-files, path-a/chrome.exe]; set browser.executable to the full Chrome executable path".into(),
        );
        let note = browser_extension_open_note(url, Err(error));
        for expected in [
            "Could not open Chrome automatically",
            "tried [local-app, program-files, path-a/chrome.exe]",
            "browser.executable",
            "Open manually",
            url,
        ] {
            assert!(note.contains(expected), "missing {expected}: {note}");
        }
    }

    #[test]
    fn workflow_result_cap_respects_char_boundaries() {
        // The 4000-byte cap on workflow results must never split a multi-byte
        // char — String::truncate panics mid-UTF-8 (CJK results would crash).
        let mut s = "汉".repeat(2000); // 3 bytes each → 6000 bytes
        truncate_at_char_boundary(&mut s, 4000);
        assert!(s.len() <= 4000);
        assert!(s.is_char_boundary(s.len()));
        assert!(s.chars().all(|c| c == '汉'));
        // No-op when already under the cap.
        let mut short = String::from("ok");
        truncate_at_char_boundary(&mut short, 4000);
        assert_eq!(short, "ok");
    }

    #[tokio::test]
    async fn clicking_a_sidebar_tab_row_switches_the_active_tab() {
        use ratatui::{backend::TestBackend, Terminal};
        let (mut app, agent_tx) = make_test_app().await;
        let engine = app.active_tab().engine.clone();
        app.tabs
            .push(crate::tab::SessionTab::new(2, engine, String::new()));
        app.active = 0;
        // A draw populates sidebar_area + sidebar_hits (like the real loop).
        let mut term = Terminal::new(TestBackend::new(120, 32)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let area = app.sidebar_area.expect("sidebar visible at 120 cols");
        let start = app
            .sidebar_hits
            .tabs_rows_start
            .expect("tab rows recorded during render");
        assert_eq!(app.sidebar_hits.tab_index_at(start + 1), Some(1));
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 2,
                row: start + 1,
                modifiers: KeyModifiers::NONE,
            },
            &agent_tx,
        );
        assert_eq!(app.active, 1, "click on the second tab row switches to it");
        // A click below the tab list does nothing.
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 2,
                row: start + app.sidebar_hits.tabs_shown,
                modifiers: KeyModifiers::NONE,
            },
            &agent_tx,
        );
        assert_eq!(app.active, 1);
    }

    #[tokio::test]
    async fn local_shell_runs_off_loop_and_posts_output() {
        let (mut app, _tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.submit("!echo off-loop-ok", &agent_tx).await;
        // submit returns immediately with the busy slot taken — the command is
        // running on a spawned task, not blocking the caller.
        assert!(
            app.active_tab().is_busy(),
            "shell escape holds the busy slot"
        );
        assert!(app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|m| m.text.contains("$ echo off-loop-ok")));
        let ev = tokio::time::timeout(Duration::from_secs(10), agent_rx.recv())
            .await
            .expect("shell result within timeout")
            .expect("channel open");
        assert!(matches!(ev, AppEvent::LocalShellDone { .. }));
        app.handle_agent_event(ev);
        assert!(!app.active_tab().is_busy(), "busy slot released on done");
        assert!(app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|m| m.text.contains("off-loop-ok") && !m.text.starts_with('$')));
        // The command + output became context for the next prompt.
        assert_eq!(app.active_tab().pending_shell_context.len(), 1);
        assert!(app.active_tab().pending_shell_context[0].contains("off-loop-ok"));
    }

    #[tokio::test]
    async fn local_shell_on_a_busy_tab_runs_without_taking_the_slot() {
        let (mut app, _tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let turn_abort = AbortController::new();
        app.active_tab_mut().turn_abort = Some(turn_abort);
        app.active_tab_mut().active_turn_id = 3;
        app.submit("!echo concurrent", &agent_tx).await;
        // Runs immediately (like the old inline version) but concurrently —
        // the live turn's busy slot is untouched, nothing was queued.
        assert!(app.active_tab().queued_input.is_empty());
        assert!(app.active_tab().turn_abort.is_some());
        let ev = tokio::time::timeout(Duration::from_secs(10), agent_rx.recv())
            .await
            .expect("shell result within timeout")
            .expect("channel open");
        match &ev {
            AppEvent::LocalShellDone { op_id, .. } => assert!(op_id.is_none()),
            other => panic!("unexpected event: {}", event_name(other)),
        }
        app.handle_agent_event(ev);
        // Completion must not release the agent turn's slot.
        assert!(app.active_tab().turn_abort.is_some());
        assert_eq!(app.active_tab().pending_shell_context.len(), 1);
    }

    fn event_name(ev: &AppEvent) -> &'static str {
        match ev {
            AppEvent::Agent { .. } => "Agent",
            AppEvent::TurnDone { .. } => "TurnDone",
            AppEvent::TurnTaskStopped { .. } => "TurnTaskStopped",
            AppEvent::TurnTaskQuarantined { .. } => "TurnTaskQuarantined",
            AppEvent::Toast { .. } => "Toast",
            AppEvent::CompactDone { .. } => "CompactDone",
            AppEvent::BgProgress { .. } => "BgProgress",
            AppEvent::BgDone { .. } => "BgDone",
            AppEvent::GitStatDone { .. } => "GitStatDone",
            AppEvent::LocalShellDone { .. } => "LocalShellDone",
            AppEvent::ConnectDialogReady { .. } => "ConnectDialogReady",
            AppEvent::ReassembleDone { .. } => "ReassembleDone",
            AppEvent::ExtensionTask(_) => "ExtensionTask",
        }
    }

    #[tokio::test]
    async fn connect_dialog_builds_off_loop_and_opens_on_arrival() {
        let (mut app, _tx) = make_test_app().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.open_connect_dialog(&agent_tx);
        assert!(
            app.connect.is_none(),
            "dialog opens via the event, not inline"
        );
        let ev = tokio::time::timeout(Duration::from_secs(10), agent_rx.recv())
            .await
            .expect("catalog load finishes")
            .expect("channel open");
        match &ev {
            AppEvent::ConnectDialogReady { .. } => {}
            other => panic!("unexpected event: {}", event_name(other)),
        }
        app.handle_agent_event(ev);
        assert!(app.connect.is_some());
    }

    #[tokio::test]
    async fn local_shell_done_never_clobbers_a_live_turn() {
        let (mut app, _tx) = make_test_app().await;
        // A live agent turn owns the busy slot — a stale owned-shell completion
        // must be dropped as a whole and cannot release it or append output.
        app.active_tab_mut().turn_abort = Some(AbortController::new());
        app.active_tab_mut().active_turn_id = 7;
        let tab_id = app.active_tab().id;
        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "echo x".into(),
            output: Some("x".into()),
            op_id: Some(1),
        });
        assert!(
            app.active_tab().turn_abort.is_some(),
            "live turn kept its abort handle"
        );
        assert!(app.active_tab().pending_shell_context.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_shell_runs_without_a_controlling_tty() {
        let dir = tempfile::tempdir().unwrap();
        let out = run_shell_capture(
            "if (: >/dev/tty) 2>/dev/null; then echo HAS_TTY; else echo NO_TTY; fi",
            dir.path(),
        )
        .await;
        assert_eq!(out.trim(), "NO_TTY");
    }

    #[tokio::test]
    async fn new_tab_opens_a_busy_placeholder_then_installs_the_engine() {
        let (mut app, _tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        // The tab exists and has focus immediately; it is busy (Switching)
        // until its own engine lands, so nothing can run on the borrowed one.
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 1);
        assert!(app.active_tab().is_busy());
        assert!(Arc::ptr_eq(&app.tabs[0].engine, &app.tabs[1].engine));
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        assert!(!app.active_tab().is_busy());
        assert!(
            !Arc::ptr_eq(&app.tabs[0].engine, &app.tabs[1].engine),
            "placeholder engine replaced by the tab's own"
        );
    }

    #[tokio::test]
    async fn failed_new_tab_assembly_removes_the_placeholder() {
        let (mut app, _tx) = make_test_app().await;
        let (agent_tx, mut _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        assert_eq!(app.tabs.len(), 2);
        let tab_id = app.active_tab().id;
        app.handle_reassemble_done(tab_id, 1, ReassembleEffect::NewTab, Err("boom".to_string()));
        assert_eq!(app.tabs.len(), 1, "placeholder removed on failure");
        assert_eq!(app.active, 0);
    }

    #[tokio::test]
    async fn tab_creation_result_does_not_install_its_template() {
        // A NewTab completion carries the template as it was when Ctrl+T was
        // pressed; installing it would revert any /model switch made while
        // the assembly ran. handle_reassemble_done must skip that for
        // tab-creation effects.
        let (mut app, _tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        let before = app.status.model.clone();
        app.status.model = "switched-mid-assembly".to_string();
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        assert_eq!(app.status.model, "switched-mid-assembly");
        let _ = before;
    }

    #[tokio::test]
    async fn regular_resume_uses_saved_model_and_prompt_without_changing_global_defaults() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let (mut app, _unused_tx, cwd) = make_test_app_with_dir_using_current_config().await;
        let session_id = "regular-resume-saved-model";
        let path = SessionIndex::session_path(session_id).unwrap();
        Session::save(&path, &agent::message::MessageStore::new())
            .await
            .unwrap();
        let meta = SessionMeta {
            id: session_id.into(),
            title: "Saved model session".into(),
            cwd: cwd.path().display().to_string(),
            model: "saved-model".into(),
            updated_at: 1,
        };
        let (tx, mut rx) = mpsc::unbounded_channel();

        app.resume_session(meta, &tx);
        let placeholder = app.tabs.last().unwrap();
        assert_eq!(
            placeholder.extension_access,
            zode_core::ToolAccessMode::Prompt
        );
        assert!(placeholder.reassemble_pending);

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("resume assembly finishes")
            .expect("event channel stays open");
        app.handle_agent_event(event);
        let resumed = app
            .tabs
            .iter()
            .find(|tab| tab.session_id == session_id)
            .unwrap();
        assert_eq!(resumed.engine.model, "saved-model");
        assert_eq!(resumed.extension_access, zode_core::ToolAccessMode::Prompt);
        assert_eq!(app.template.model(), Some("test-model"));
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt
        );
        assert!(!app.template.plan_mode());
    }

    #[tokio::test]
    async fn local_shell_interrupted_posts_nothing() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.active_tab().id;
        app.active_tab_mut().local_op_seq = 1;
        app.active_tab_mut().active_local_op_id = Some(1);
        app.active_tab_mut().turn_abort = Some(AbortController::new());
        assert!(app.interrupt_active_turn());
        let before = app.active_tab().chat.messages().len();
        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "sleep 100".into(),
            output: None,
            op_id: Some(1),
        });
        assert!(!app.active_tab().is_busy());
        assert_eq!(app.active_tab().chat.messages().len(), before);
        assert!(app.active_tab().pending_shell_context.is_empty());
    }

    #[tokio::test]
    async fn extension_daemon_exits_on_native_port_disconnect_without_a_terminal() {
        let (app, _tx) = make_test_app().await;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        shutdown_tx.send(()).unwrap();
        tokio::time::timeout(
            Duration::from_secs(1),
            app.run_extension_daemon(shutdown_rx),
        )
        .await
        .expect("daemon observes native disconnect")
        .expect("daemon exits cleanly");
    }

    async fn make_test_app() -> (TuiApp, mpsc::UnboundedSender<AppEvent>) {
        let (app, tx, _temp) = make_test_app_with_dir().await;
        // The tempdir guard drops here — fine for tests that never touch the
        // cwd again after assembly.
        (app, tx)
    }

    async fn make_test_app_using_current_config() -> (TuiApp, mpsc::UnboundedSender<AppEvent>) {
        let (app, tx, _temp) = make_test_app_with_dir_using_current_config().await;
        (app, tx)
    }

    fn arm_local_op_for_test(app: &mut TuiApp, tab_idx: usize) -> u64 {
        app.begin_local_operation(tab_idx)
            .expect("test tab accepts local operation")
            .0
    }

    /// Like [`make_test_app`] but keeps the cwd tempdir alive — required by
    /// tests that run shell commands or assemble engines AFTER construction.
    async fn make_test_app_with_dir() -> (TuiApp, mpsc::UnboundedSender<AppEvent>, tempfile::TempDir)
    {
        let config = tempfile::tempdir().unwrap();
        let env_lock: tokio::sync::MutexGuard<'static, ()> = crate::tab::TEST_ENV_LOCK.lock().await;
        let env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let (mut app, agent_tx, cwd) = make_test_app_with_dir_using_current_config().await;
        app._test_config_isolation = Some(TestConfigIsolation {
            _env: env,
            _config: config,
            _env_lock: env_lock,
        });
        (app, agent_tx, cwd)
    }

    /// Build against the caller's already-isolated `ZODE_CONFIG_DIR`.
    ///
    /// Persisted-scheduler and session tests hold `TEST_ENV_LOCK` for their
    /// whole transaction, so routing them through [`make_test_app_with_dir`]
    /// would deadlock. All other tests use that locked wrapper and therefore
    /// never load a developer's real schedules or another test's temporary
    /// roster.
    async fn make_test_app_with_dir_using_current_config(
    ) -> (TuiApp, mpsc::UnboundedSender<AppEvent>, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().to_path_buf();
        let cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Ollama),
                base_url: Some("http://localhost:11434".to_string()),
                model: Some("test-model".to_string()),
                ..Default::default()
            },
            noema: NoemaSettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let (approval_queue, approval_rx) = zode_core::approval::approval_queue();
        let (question_queue, question_rx) = zode_core::question::question_queue();
        let op_question_queue = question_queue.clone();
        let template = EngineTemplate::new(
            cfg,
            cwd.clone(),
            Some(approval_queue),
            false,
            None,
            "2026-06-15".to_string(),
        )
        .with_question_queue(Some(question_queue));
        let engine = template.assemble().await.unwrap();
        let initial_access = template.tool_access();
        let app = TuiApp::new(
            engine,
            template,
            UiConfig {
                theme_id: None,
                yolo: false,
                initial_access,
                sandbox: false,
                provider_names: Vec::new(),
                needs_setup: false,
                update_applied: None,
            },
            approval_rx,
            question_rx,
            op_question_queue,
            None,
        );
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        (app, agent_tx, temp)
    }

    #[tokio::test]
    async fn initial_tab_records_actual_engine_access_when_resume_load_failed() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let cwd = tempfile::tempdir().unwrap();
        let cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Ollama),
                base_url: Some("http://localhost:11434".into()),
                model: Some("saved-model".into()),
                ..Default::default()
            },
            noema: NoemaSettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let (approval_queue, approval_rx) = zode_core::approval::approval_queue();
        let (question_queue, question_rx) = zode_core::question::question_queue();
        let op_question_queue = question_queue.clone();
        let clean_template = EngineTemplate::new(
            cfg,
            cwd.path().to_path_buf(),
            Some(approval_queue),
            true,
            None,
            "2026-07-13".into(),
        )
        .with_question_queue(Some(question_queue));
        let resume_template = clean_template.with_tool_access(zode_core::ToolAccessMode::Prompt);
        let engine = resume_template.assemble().await.unwrap();

        let app = TuiApp::new(
            engine,
            clean_template,
            UiConfig {
                theme_id: None,
                yolo: true,
                initial_access: resume_template.tool_access(),
                sandbox: false,
                provider_names: Vec::new(),
                needs_setup: false,
                update_applied: None,
            },
            approval_rx,
            question_rx,
            op_question_queue,
            None, // transcript load failed, so attach_session returned no id
        );

        assert_eq!(
            app.tabs[0].extension_access,
            zode_core::ToolAccessMode::Prompt
        );
        assert_eq!(app.template.tool_access(), zode_core::ToolAccessMode::Auto);
    }

    async fn send_key(
        app: &mut TuiApp,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) {
        app.handle_term(
            CtEvent::Key(crossterm::event::KeyEvent::new(code, modifiers)),
            agent_tx,
        )
        .await;
    }

    fn windows_key_pair_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> [CtEvent; 2] {
        [
            CtEvent::Key(KeyEvent::new_with_kind(
                code,
                modifiers,
                crossterm::event::KeyEventKind::Press,
            )),
            CtEvent::Key(KeyEvent::new_with_kind(
                code,
                modifiers,
                crossterm::event::KeyEventKind::Release,
            )),
        ]
    }

    fn windows_key_pair(code: KeyCode) -> [CtEvent; 2] {
        windows_key_pair_with_modifiers(code, KeyModifiers::NONE)
    }

    fn windows_key_burst(text: &str) -> Vec<CtEvent> {
        text.chars()
            .flat_map(|character| {
                let code = match character {
                    '\n' | '\r' => KeyCode::Enter,
                    '\t' => KeyCode::Tab,
                    '\u{1b}' => KeyCode::Esc,
                    character => KeyCode::Char(character),
                };
                windows_key_pair(code)
            })
            .collect()
    }

    #[test]
    fn windows_bracketed_multiline_burst_decodes_without_clipboard() {
        let events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        assert_eq!(
            windows_paste_segments(&events, None),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "first\nsecond".to_string(),
            }]
        );
    }

    #[test]
    fn windows_bracketed_frame_leaves_data_after_end_replayable() {
        let frame =
            format!("{WINDOWS_BRACKETED_PASTE_START}first\nsecond{WINDOWS_BRACKETED_PASTE_END}");
        let mut events = windows_key_burst(&frame);
        let frame_end = events.len();
        events.extend(windows_key_burst(&format!(
            "junk{WINDOWS_BRACKETED_PASTE_END}"
        )));

        assert_eq!(
            windows_paste_segments(&events, None),
            vec![WindowsPasteSegment {
                events: 0..frame_end,
                text: "first\nsecond".to_string(),
            }]
        );
    }

    #[test]
    fn windows_bracketed_burst_rejects_nested_start_marker() {
        let raw = format!(
            "{WINDOWS_BRACKETED_PASTE_START}first\n{WINDOWS_BRACKETED_PASTE_START}second{WINDOWS_BRACKETED_PASTE_END}"
        );
        let events = windows_key_burst(&raw);

        assert!(windows_paste_segments(&events, None).is_empty());
    }

    #[test]
    fn windows_bracketed_burst_without_end_marker_is_not_complete() {
        let raw = format!("{WINDOWS_BRACKETED_PASTE_START}first\nsecond");
        let events = windows_key_burst(&raw);

        assert!(windows_paste_segments(&events, None).is_empty());
    }

    #[test]
    fn windows_malformed_bracket_start_needs_clipboard_match() {
        let raw = format!("{WINDOWS_BRACKETED_PASTE_START}first\nsecond");
        let events = windows_key_burst(&raw);

        assert!(windows_burst_needs_clipboard(&events));
        assert_eq!(
            windows_paste_segments(&events, Some(&raw)),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: raw,
            }]
        );
    }

    #[test]
    fn windows_unbracketed_multiline_burst_requires_clipboard_match() {
        let events = windows_key_burst("first\nsecond");
        assert_eq!(
            windows_paste_segments(&events, Some("first\r\nsecond")),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "first\nsecond".to_string(),
            }]
        );
        assert!(windows_paste_segments(&events, Some("different")).is_empty());
    }

    #[test]
    fn windows_standalone_enter_is_never_a_paste() {
        let events = windows_key_pair(KeyCode::Enter).to_vec();
        assert!(windows_paste_segments(&events, Some("\n")).is_empty());
    }

    #[test]
    fn windows_paste_before_navigation_does_not_swallow_the_navigation() {
        let mut events = windows_key_burst("first\nsecond");
        let paste_end = events.len();
        events.extend(windows_key_pair(KeyCode::Left));
        assert_eq!(
            windows_paste_segments(&events, Some("first\nsecond")),
            vec![WindowsPasteSegment {
                events: 0..paste_end,
                text: "first\nsecond".to_string(),
            }]
        );
    }

    #[test]
    fn windows_clipboard_lookup_is_only_needed_for_unbracketed_multiline_bursts() {
        let bracketed = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        let unbracketed = windows_key_burst("first\nsecond");
        let single_line = windows_key_burst("first second");

        assert!(!windows_burst_needs_clipboard(&bracketed));
        assert!(windows_burst_needs_clipboard(&unbracketed));
        assert!(!windows_burst_needs_clipboard(&single_line));
    }

    #[test]
    fn windows_mixed_burst_requests_clipboard_for_an_inner_multiline_run() {
        let mut mixed = windows_key_pair(KeyCode::Left).to_vec();
        mixed.extend(windows_key_burst("first\nsecond"));

        assert!(windows_burst_needs_clipboard(&mixed));

        let mut bracketed_only = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        bracketed_only.extend(windows_key_burst("x"));
        assert!(!windows_burst_needs_clipboard(&bracketed_only));
    }

    #[test]
    fn windows_paste_segments_map_unicode_text_to_exact_event_pairs() {
        let events = windows_key_burst("x🦀\ny");

        assert_eq!(
            windows_paste_segments(&events, Some("🦀\r\n")),
            vec![WindowsPasteSegment {
                events: 2..6,
                text: "🦀\n".to_string(),
            }]
        );
    }

    #[test]
    fn windows_duplicate_non_bmp_presses_form_one_text_unit() {
        let crab = KeyCode::Char('🦀');
        let mut events = vec![
            CtEvent::Key(KeyEvent::new_with_kind(
                crab,
                KeyModifiers::NONE,
                crossterm::event::KeyEventKind::Press,
            )),
            CtEvent::Key(KeyEvent::new_with_kind(
                crab,
                KeyModifiers::NONE,
                crossterm::event::KeyEventKind::Press,
            )),
        ];
        events.extend(windows_key_burst("\n"));

        let runs = windows_text_runs(&events);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "🦀\n");
        assert_eq!(runs[0].event_range(&(0..'🦀'.len_utf8())), Some(0..2));
        assert_eq!(
            windows_paste_segments(&events, Some("🦀\n")),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "🦀\n".to_string(),
            }]
        );
    }

    #[test]
    fn windows_four_non_bmp_presses_preserve_two_logical_characters() {
        let press = || {
            CtEvent::Key(KeyEvent::new_with_kind(
                KeyCode::Char('🦀'),
                KeyModifiers::NONE,
                crossterm::event::KeyEventKind::Press,
            ))
        };
        let mut events = vec![press(), press(), press(), press()];
        events.extend(windows_key_burst("\n"));

        let runs = windows_text_runs(&events);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "🦀🦀\n");
        assert_eq!(runs[0].event_range(&(0..'🦀'.len_utf8())), Some(0..2));
        assert_eq!(
            runs[0].event_range(&('🦀'.len_utf8()..('🦀'.len_utf8() * 2))),
            Some(2..4)
        );
        assert_eq!(
            windows_paste_segments(&events, Some("🦀🦀\n")),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "🦀🦀\n".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn windows_bracketed_paste_collapses_duplicate_non_bmp_presses() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_burst("\u{1b}[200~first ");
        events.push(CtEvent::Key(KeyEvent::new(
            KeyCode::Char('🦀'),
            KeyModifiers::NONE,
        )));
        events.push(CtEvent::Key(KeyEvent::new(
            KeyCode::Char('🦀'),
            KeyModifiers::NONE,
        )));
        events.extend(windows_key_burst("\nsecond\u{1b}[201~"));

        app.handle_term_burst(events, &agent_tx, true, None).await;

        assert_eq!(app.input.text(), "first 🦀\nsecond");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[test]
    fn windows_paste_segments_preserve_consecutive_enters() {
        let events = windows_key_burst("a\n\nb");

        assert_eq!(
            windows_paste_segments(&events, Some("a\n\nb")),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "a\n\nb".to_string(),
            }]
        );
    }

    #[test]
    fn windows_paste_segments_collapse_crlf_key_producers() {
        let mut events = windows_key_burst("a");
        events.extend(windows_key_pair(KeyCode::Char('\r')));
        events.extend(windows_key_pair(KeyCode::Enter));
        events.extend(windows_key_burst("b"));

        assert_eq!(
            windows_paste_segments(&events, Some("a\r\nb")),
            vec![WindowsPasteSegment {
                events: 0..events.len(),
                text: "a\nb".to_string(),
            }]
        );
    }

    #[test]
    fn windows_control_shortcut_is_a_boundary_before_a_text_run() {
        let mut events =
            windows_key_pair_with_modifiers(KeyCode::Char('v'), KeyModifiers::CONTROL).to_vec();
        let paste_start = events.len();
        events.extend(windows_key_burst("first\nsecond"));

        let runs = windows_text_runs(&events);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "first\nsecond");
        assert!(windows_paste_segments(&events, Some("vfirst\nsecond")).is_empty());
        assert_eq!(
            windows_paste_segments(&events, Some("first\nsecond")),
            vec![WindowsPasteSegment {
                events: paste_start..events.len(),
                text: "first\nsecond".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn windows_control_shortcut_replays_before_a_segmented_paste() {
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].chat.push_tool("Bash done\n    output");
        let mut events =
            windows_key_pair_with_modifiers(KeyCode::Char('e'), KeyModifiers::CONTROL).to_vec();
        events.extend(windows_key_burst("first\nsecond"));

        app.handle_term_burst(events, &agent_tx, true, Some("first\nsecond"))
            .await;

        assert!(!app.tabs[0].chat.messages()[0].collapsed);
        assert_eq!(app.input.text(), "first\nsecond");
        assert!(!app.active_tab().is_busy());
    }

    #[test]
    fn windows_paste_segments_leave_a_trailing_enter_replayable() {
        let mut events = windows_key_burst("first\nsecond");
        let paste_end = events.len();
        events.extend(windows_key_pair(KeyCode::Enter));

        assert_eq!(
            windows_paste_segments(&events, Some("first\r\nsecond")),
            vec![WindowsPasteSegment {
                events: 0..paste_end,
                text: "first\nsecond".to_string(),
            }]
        );
    }

    #[test]
    fn windows_paste_segments_find_repeated_unbracketed_occurrences() {
        let mut events = windows_key_burst("a\nb");
        let first_end = events.len();
        events.extend(windows_key_burst("x"));
        let second_start = events.len();
        events.extend(windows_key_burst("a\nb"));

        assert_eq!(
            windows_paste_segments(&events, Some("a\nb")),
            vec![
                WindowsPasteSegment {
                    events: 0..first_end,
                    text: "a\nb".to_string(),
                },
                WindowsPasteSegment {
                    events: second_start..events.len(),
                    text: "a\nb".to_string(),
                },
            ]
        );
    }

    #[test]
    fn windows_paste_segments_find_multiple_bracketed_frames_in_order() {
        let mut events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        let first_end = events.len();
        events.extend(windows_key_burst("x"));
        let second_start = events.len();
        events.extend(windows_key_burst("\u{1b}[200~third\nfourth\u{1b}[201~"));

        assert_eq!(
            windows_paste_segments(&events, None),
            vec![
                WindowsPasteSegment {
                    events: 0..first_end,
                    text: "first\nsecond".to_string(),
                },
                WindowsPasteSegment {
                    events: second_start..events.len(),
                    text: "third\nfourth".to_string(),
                },
            ]
        );
    }

    #[test]
    fn windows_paste_segments_reject_nested_frame_but_find_a_later_frame() {
        let nested = format!(
            "{WINDOWS_BRACKETED_PASTE_START}first{WINDOWS_BRACKETED_PASTE_START}second{WINDOWS_BRACKETED_PASTE_END}"
        );
        let mut events = windows_key_burst(&nested);
        let later_start = events.len();
        events.extend(windows_key_burst("\u{1b}[200~third\nfourth\u{1b}[201~"));

        assert_eq!(
            windows_paste_segments(&events, None),
            vec![WindowsPasteSegment {
                events: later_start..events.len(),
                text: "third\nfourth".to_string(),
            }]
        );
    }

    #[test]
    fn windows_oversized_clipboard_is_rejected() {
        let events = windows_key_burst("first\nsecond");
        let oversized_clipboard = "x".repeat(WINDOWS_PASTE_TEXT_BYTE_CAP + 1);

        assert!(windows_paste_segments(&events, Some(&oversized_clipboard)).is_empty());
    }

    #[test]
    fn windows_paste_text_byte_cap_is_four_bytes_per_event() {
        assert_eq!(WINDOWS_PASTE_TEXT_BYTE_CAP, WINDOWS_PASTE_EVENT_CAP * 4);
    }

    #[test]
    fn windows_automatic_clipboard_read_has_a_one_second_budget() {
        assert_eq!(
            WINDOWS_CLIPBOARD_READ_TIMEOUT,
            std::time::Duration::from_secs(1)
        );
    }

    #[tokio::test]
    async fn windows_multiline_key_burst_stays_in_composer_without_submitting() {
        let (mut app, agent_tx) = make_test_app().await;
        let events = windows_key_burst("first\nsecond");

        app.handle_term_burst(events, &agent_tx, true, Some("first\r\nsecond"))
            .await;

        assert_eq!(app.input.text(), "first\nsecond");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[tokio::test]
    async fn windows_bracketed_multiline_key_burst_stays_in_composer_without_submitting() {
        let (mut app, agent_tx) = make_test_app().await;
        let events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");

        app.handle_term_burst(events, &agent_tx, true, None).await;

        assert_eq!(app.input.text(), "first\nsecond");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[tokio::test]
    async fn windows_bracketed_paste_followed_by_enter_submits_the_full_paste() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        events.extend(windows_key_pair(KeyCode::Enter));

        app.handle_term_burst(events, &agent_tx, true, None).await;

        let user_messages: Vec<_> = app
            .active_tab()
            .chat
            .messages()
            .iter()
            .filter(|message| message.role == Role::User)
            .map(|message| message.text.as_str())
            .collect();
        assert_eq!(user_messages, vec!["first\nsecond"]);
        assert!(app.input.text().is_empty());
    }

    #[tokio::test]
    async fn windows_unbracketed_paste_followed_by_enter_submits_the_full_paste() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_burst("first\nsecond");
        events.extend(windows_key_pair(KeyCode::Enter));

        app.handle_term_burst(events, &agent_tx, true, Some("first\r\nsecond"))
            .await;

        let user_messages: Vec<_> = app
            .active_tab()
            .chat
            .messages()
            .iter()
            .filter(|message| message.role == Role::User)
            .map(|message| message.text.as_str())
            .collect();
        assert_eq!(user_messages, vec!["first\nsecond"]);
        assert!(app.input.text().is_empty());
    }

    #[tokio::test]
    async fn windows_leading_navigation_does_not_hide_an_unbracketed_paste() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_pair(KeyCode::Left).to_vec();
        events.extend(windows_key_burst("first\nsecond"));

        app.handle_term_burst(events, &agent_tx, true, Some("first\nsecond"))
            .await;

        assert_eq!(app.input.text(), "first\nsecond");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[tokio::test]
    async fn windows_two_bracketed_pastes_are_inserted_in_order() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        events.extend(windows_key_burst("\u{1b}[200~\nthird\nfourth\u{1b}[201~"));

        app.handle_term_burst(events, &agent_tx, true, None).await;

        assert_eq!(app.input.text(), "first\nsecond\nthird\nfourth");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[tokio::test]
    async fn windows_bracketed_paste_replays_a_trailing_character() {
        let (mut app, agent_tx) = make_test_app().await;
        let mut events = windows_key_burst("\u{1b}[200~first\nsecond\u{1b}[201~");
        events.extend(windows_key_burst("x"));

        app.handle_term_burst(events, &agent_tx, true, None).await;

        assert_eq!(app.input.text(), "first\nsecondx");
        assert!(app.active_tab().chat.messages().is_empty());
        assert!(!app.active_tab().is_busy());
    }

    #[tokio::test]
    async fn ctrl_e_toggles_tool_block_folds() {
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].chat.push_tool("Bash done\n    output");
        assert!(
            app.tabs[0].chat.messages()[0].collapsed,
            "folded by default"
        );

        send_key(
            &mut app,
            &agent_tx,
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
        )
        .await;
        assert!(
            !app.tabs[0].chat.messages()[0].collapsed,
            "Ctrl+E expands the folded blocks"
        );

        send_key(
            &mut app,
            &agent_tx,
            KeyCode::Char('e'),
            KeyModifiers::CONTROL,
        )
        .await;
        assert!(
            app.tabs[0].chat.messages()[0].collapsed,
            "Ctrl+E again folds them back"
        );
    }

    #[test]
    fn sidebar_is_hidden_until_multiple_tabs_exist() {
        assert!(!should_show_sidebar(0, SidebarVisibility::Auto));
        assert!(!should_show_sidebar(1, SidebarVisibility::Auto));
        assert!(should_show_sidebar(2, SidebarVisibility::Auto));
        assert!(should_show_sidebar(1, SidebarVisibility::Visible));
        assert!(!should_show_sidebar(2, SidebarVisibility::Hidden));
    }

    fn scroll_event(kind: MouseEventKind) -> std::io::Result<CtEvent> {
        Ok(CtEvent::Mouse(MouseEvent {
            kind,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }))
    }

    #[test]
    fn drain_ready_events_coalesces_a_buffered_burst() {
        // A trackpad/wheel momentum flick lands as many already-buffered scroll
        // events. They must all drain in one pass so the loop redraws ONCE per
        // batch instead of once per event (the over-scroll "freeze").
        let burst: Vec<_> = (0..50)
            .map(|_| scroll_event(MouseEventKind::ScrollDown))
            .collect();
        let mut stream = futures::stream::iter(burst);
        let drained = drain_ready_events(&mut stream, INPUT_COALESCE_CAP);
        assert_eq!(drained.len(), 50);
    }

    #[test]
    fn drain_ready_events_respects_the_cap() {
        // The cap bounds work per iteration so a sustained flood can't starve
        // the agent/approval/question select! branches.
        let burst: Vec<_> = (0..10)
            .map(|_| scroll_event(MouseEventKind::ScrollUp))
            .collect();
        let mut stream = futures::stream::iter(burst);
        let drained = drain_ready_events(&mut stream, 4);
        assert_eq!(drained.len(), 4);
    }

    #[test]
    fn windows_text_burst_drain_extends_past_normal_coalescing_cap() {
        let events = windows_key_burst(&"a".repeat(INPUT_COALESCE_CAP + 10));
        let mut stream = futures::stream::iter(events.into_iter().skip(1).map(Ok));
        let mut burst = vec![CtEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        ))];
        let mut first = drain_ready_events(&mut stream, INPUT_COALESCE_CAP);
        let first_chunk_full = first.len() == INPUT_COALESCE_CAP;
        burst.append(&mut first);

        extend_windows_text_burst(&mut stream, &mut burst, first_chunk_full);

        assert!(burst.len() > INPUT_COALESCE_CAP);
        assert!(burst.len() <= WINDOWS_PASTE_EVENT_CAP);
    }

    #[test]
    fn windows_text_tail_drain_extends_after_leading_navigation() {
        let mut events = windows_key_pair(KeyCode::Left).to_vec();
        events.extend(windows_key_burst(&"a".repeat(INPUT_COALESCE_CAP + 10)));
        let mut stream = futures::stream::iter(events.into_iter().skip(1).map(Ok));
        let mut burst = vec![CtEvent::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::NONE,
        ))];
        let mut first = drain_ready_events(&mut stream, INPUT_COALESCE_CAP);
        let first_chunk_full = first.len() == INPUT_COALESCE_CAP;
        burst.append(&mut first);

        extend_windows_text_burst(&mut stream, &mut burst, first_chunk_full);

        assert!(burst.len() > INPUT_COALESCE_CAP);
        assert!(burst.len() <= WINDOWS_PASTE_EVENT_CAP);
    }

    #[test]
    fn tab_command_resolves_numbers_and_cycle_targets() {
        assert_eq!(resolve_tab_target("2", 0, 3), Ok(1));
        assert_eq!(resolve_tab_target("next", 2, 3), Ok(0));
        assert_eq!(resolve_tab_target("", 0, 3), Ok(1));
        assert_eq!(resolve_tab_target("prev", 0, 3), Ok(2));
        assert_eq!(
            resolve_tab_target("9", 0, 3),
            Err("tab 9 is out of range (1..3)".to_string())
        );
        assert_eq!(
            resolve_tab_target("abc", 0, 3),
            Err("usage: /tab [n|next|prev]".to_string())
        );
    }

    #[test]
    fn csi_private_reply_detects_kitty_and_da1() {
        // Kitty reply followed by DA1 — the normal "supported" handshake.
        let both = b"\x1b[?1u\x1b[?62;22c";
        assert!(csi_private_reply(both, b'u'));
        assert!(csi_private_reply(both, b'c'));
        // DA1 alone — terminal answered but doesn't speak kitty.
        let da1 = b"\x1b[?62c";
        assert!(!csi_private_reply(da1, b'u'));
        assert!(csi_private_reply(da1, b'c'));
        // Partial / noise: never a match.
        assert!(!csi_private_reply(b"\x1b[?1", b'u'));
        assert!(!csi_private_reply(b"hello", b'u'));
        // A stray keystroke before the reply doesn't hide it.
        assert!(csi_private_reply(b"x\x1b[?0u", b'u'));
    }

    #[test]
    fn sidebar_command_resolves_visibility_targets() {
        assert_eq!(
            resolve_sidebar_visibility("", SidebarVisibility::Auto, 1),
            Ok(SidebarVisibility::Visible)
        );
        assert_eq!(
            resolve_sidebar_visibility("toggle", SidebarVisibility::Auto, 2),
            Ok(SidebarVisibility::Hidden)
        );
        assert_eq!(
            resolve_sidebar_visibility("toggle", SidebarVisibility::Hidden, 1),
            Ok(SidebarVisibility::Visible)
        );
        assert_eq!(
            resolve_sidebar_visibility("off", SidebarVisibility::Auto, 2),
            Ok(SidebarVisibility::Hidden)
        );
        assert_eq!(
            resolve_sidebar_visibility("hide", SidebarVisibility::Visible, 1),
            Ok(SidebarVisibility::Hidden)
        );
        assert_eq!(
            resolve_sidebar_visibility("close", SidebarVisibility::Visible, 1),
            Ok(SidebarVisibility::Hidden)
        );
        assert_eq!(
            resolve_sidebar_visibility("on", SidebarVisibility::Auto, 1),
            Ok(SidebarVisibility::Visible)
        );
        assert_eq!(
            resolve_sidebar_visibility("show", SidebarVisibility::Hidden, 1),
            Ok(SidebarVisibility::Visible)
        );
        assert_eq!(
            resolve_sidebar_visibility("open", SidebarVisibility::Hidden, 1),
            Ok(SidebarVisibility::Visible)
        );
        assert_eq!(
            resolve_sidebar_visibility("auto", SidebarVisibility::Hidden, 1),
            Ok(SidebarVisibility::Auto)
        );
        assert_eq!(
            resolve_sidebar_visibility("wat", SidebarVisibility::Auto, 1),
            Err("usage: /sidebar [on|off|toggle|auto|mcp|files|todo]".to_string())
        );
    }

    #[test]
    fn prompt_history_persists_and_loads_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROMPT_HISTORY_FILE);
        let mut history = Vec::new();

        assert!(record_prompt_history_entry(
            &mut history,
            "  first prompt  "
        ));
        assert!(record_prompt_history_entry(&mut history, "second prompt"));
        save_prompt_history_to_path(&path, "session:a", &history).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "session:a"),
            vec!["first prompt".to_string(), "second prompt".to_string()]
        );
    }

    #[tokio::test]
    async fn up_down_recalls_prompt_history_when_idle() {
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history = vec!["first prompt".into(), "写个 /tmp/hello.txt".into()];
        app.tabs[0].history_pos = None;
        app.input.take(); // empty input, idle, no queued messages
        assert!(!app.active_tab().is_busy());

        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(
            app.input.text(),
            "写个 /tmp/hello.txt",
            "Up → latest prompt"
        );

        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(
            app.input.text(),
            "first prompt",
            "Up again → earlier prompt"
        );

        send_key(&mut app, &agent_tx, KeyCode::Down, KeyModifiers::NONE).await;
        assert_eq!(
            app.input.text(),
            "写个 /tmp/hello.txt",
            "Down → newer prompt"
        );
    }

    #[tokio::test]
    async fn prompt_history_is_isolated_between_session_tabs() {
        let (mut app, _agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history.clear();
        app.tabs[0].history_pos = None;
        app.tabs[0].history_draft.clear();
        app.record_prompt_history("tab zero prompt");

        let tab1 = SessionTab::new(1, app.active_tab().engine.clone(), "session-one".into());
        app.tabs.push(tab1);
        app.active = 1;
        app.reset_input_browse_state();
        app.input.take();

        app.history_prev();
        assert_eq!(
            app.input.text(),
            "",
            "a new session tab must not recall another session's prompt"
        );

        app.record_prompt_history("tab one prompt");
        app.active = 0;
        app.reset_input_browse_state();
        app.input.take();

        app.history_prev();
        assert_eq!(
            app.input.text(),
            "tab zero prompt",
            "switching back must restore only that session's prompt history"
        );
    }

    #[tokio::test]
    async fn file_tool_result_line_shows_resolved_path_and_active_cwd() {
        let (mut app, _tx, dir) = make_test_app_with_dir().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        let resolved = dir.path().join("created.txt");

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolUse {
                id: "tool-1".into(),
                name: "FileWrite".into(),
                input: serde_json::json!({"path": "created.txt", "content": "x"}),
            },
        });
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolResult {
                id: "tool-1".into(),
                ok: true,
                output: serde_json::json!({
                    "path": resolved.display().to_string(),
                    "status": "ok",
                    "size_bytes": 1,
                }),
            },
        });

        let result_line = app.tabs[0]
            .chat
            .messages()
            .iter()
            .rev()
            .map(|m| m.text.as_str())
            .find(|text| text.contains("Tool FileWrite done"))
            .expect("tool result line");
        assert!(
            result_line.contains(&format!("path={}", resolved.display())),
            "{result_line}"
        );
        assert!(
            result_line.contains(&format!("cwd={}", dir.path().display())),
            "{result_line}"
        );
    }

    #[tokio::test]
    async fn tool_result_line_carries_duration_suffix() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        });
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolResult {
                id: "t1".into(),
                ok: true,
                output: serde_json::Value::Null,
            },
        });

        let last_tool_line = app.tabs[0]
            .chat
            .messages()
            .iter()
            .rev()
            .find(|m| m.role == Role::Tool)
            .map(|m| m.text.clone())
            .expect("a tool line was pushed");
        assert!(
            last_tool_line.contains(" · ") && last_tool_line.trim_end().ends_with('s'),
            "expected duration suffix, got: {last_tool_line}"
        );
        assert!(app.tabs[0].active_tool_api_names.is_empty());
        let activity = app.tabs[0].recent_tools.back().expect("tool activity");
        assert_eq!(activity.name, "Bash");
        assert_eq!(activity.status, "succeeded");
        assert!(activity.duration_ms.is_some());
    }

    #[tokio::test]
    async fn turn_done_pushes_duration_footer() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].turn_started_at = Some(std::time::Instant::now());
        app.tabs[0].turn_tool_count = 0;

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolUse {
                id: "t1".into(),
                name: "Read".into(),
                input: serde_json::json!({}),
            },
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 7,
            result: Ok(()),
        });

        let footer = app.tabs[0]
            .chat
            .messages()
            .iter()
            .rev()
            .find(|m| m.role == Role::System)
            .map(|m| m.text.clone())
            .expect("a system line was pushed");
        assert!(
            footer.starts_with('✓') && footer.contains("1 ") && footer.trim_end().ends_with('s'),
            "expected '✓ <dur> · 1 tools' style footer, got: {footer}"
        );
    }

    /// Drive one complete tool call through the event pipeline.
    fn run_tool(app: &mut TuiApp, tab_id: usize, turn_id: u64, id: &str, name: &str, ok: bool) {
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id,
            cost_label: None,
            event: Event::ToolUse {
                id: id.into(),
                name: name.into(),
                input: serde_json::json!({}),
            },
        });
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id,
            cost_label: None,
            event: Event::ToolResult {
                id: id.into(),
                ok,
                output: serde_json::Value::Null,
            },
        });
    }

    fn tally_cells(tally: &crate::ui::hud::ToolTally) -> Vec<String> {
        let (cells, overflow) = tally.top(crate::ui::hud::MAX_TALLY_CELLS);
        let mut out: Vec<String> = cells
            .into_iter()
            .map(crate::ui::hud::tally_cell_text)
            .collect();
        if overflow > 0 {
            out.push(format!("+{overflow} more"));
        }
        out
    }

    #[tokio::test]
    async fn hud_tally_aggregates_per_turn_tool_events() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;

        run_tool(&mut app, tab_id, 7, "b1", "Bash", true);
        run_tool(&mut app, tab_id, 7, "b2", "Bash", true);
        run_tool(&mut app, tab_id, 7, "e1", "Edit", false);
        run_tool(&mut app, tab_id, 7, "r1", "Read", true);
        run_tool(&mut app, tab_id, 7, "g1", "Grep", true);
        run_tool(&mut app, tab_id, 7, "w1", "WebFetch", true);
        // A call still in flight is counted immediately, under the running mark.
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::ToolUse {
                id: "b3".into(),
                name: "Bash".into(),
                input: serde_json::json!({}),
            },
        });

        assert_eq!(
            tally_cells(app.tabs[0].hud_tally()),
            [
                "◐ Bash ×3",
                "✗ Edit ×1",
                "✓ Read ×1",
                "✓ Grep ×1",
                "+1 more"
            ]
        );
    }

    #[tokio::test]
    async fn hud_tally_survives_turn_end_and_resets_on_the_next_turn() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].turn_started_at = Some(std::time::Instant::now());

        run_tool(&mut app, tab_id, 7, "b1", "Bash", true);
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 7,
            result: Ok(()),
        });
        // The live tally is cleared, but the completed turn's story stays on
        // the HUD while the tab is idle.
        assert!(app.tabs[0].turn_tools.is_empty());
        assert_eq!(tally_cells(app.tabs[0].hud_tally()), ["✓ Bash ×1"]);

        // A turn that used no tools must not blank the row a second time.
        app.tabs[0].active_turn_id = 8;
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 8,
            result: Ok(()),
        });
        assert_eq!(tally_cells(app.tabs[0].hud_tally()), ["✓ Bash ×1"]);

        // The next turn's first tool call takes the row over.
        app.tabs[0].active_turn_id = 9;
        app.tabs[0].turn_tools.clear();
        run_tool(&mut app, tab_id, 9, "r1", "Read", true);
        assert_eq!(tally_cells(app.tabs[0].hud_tally()), ["✓ Read ×1"]);
    }

    #[tokio::test]
    async fn hud_tally_settles_on_the_draining_turn_path() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        run_tool(&mut app, tab_id, 7, "b1", "Bash", false);

        // Interrupt: active_turn_id is cleared and the turn drains separately.
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].draining_turn_id = Some(7);
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 7,
            result: Ok(()),
        });

        assert!(app.tabs[0].turn_tools.is_empty());
        assert_eq!(tally_cells(app.tabs[0].hud_tally()), ["✗ Bash ×1"]);
    }

    #[tokio::test]
    async fn hud_subagent_rows_stay_fresh_while_a_finished_agent_lingers() {
        let (mut app, _tx) = make_test_app().await;
        assert!(
            !app.has_hud_subagent_rows(),
            "no sub-agents → nothing to poll for"
        );
        let now = now_secs();
        app.subagents = vec![zode_core::SubAgent {
            id: 1,
            agent_type: "general-purpose".into(),
            description: Some("scan".into()),
            depth: 0,
            status: zode_core::SubAgentStatus::Done,
            started_at: now.saturating_sub(30),
            finished_at: Some(now.saturating_sub(5)),
            input_tokens: 0,
            output_tokens: 0,
            transcript: Vec::new(),
            committed_input: 0,
            committed_output: 0,
            turn_input: 0,
            turn_output: 0,
        }];
        assert!(app.has_hud_subagent_rows(), "a recent finisher still shows");
        app.subagents[0].finished_at =
            Some(now.saturating_sub(crate::ui::hud::SUBAGENT_RECENT_SECS + 5));
        assert!(
            !app.has_hud_subagent_rows(),
            "an aged-out row stops polling"
        );
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// Rows of the painted frame, as plain strings.
    fn painted_rows(term: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn row_containing(rows: &[String], needle: &str) -> Option<u16> {
        rows.iter()
            .position(|row| row.contains(needle))
            .map(|y| y as u16)
    }

    #[tokio::test]
    async fn status_hud_stacks_above_the_status_line_and_yields_on_short_terminals() {
        use ratatui::{backend::TestBackend, Terminal};
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].extension_access = zode_core::ToolAccessMode::Auto;
        app.tabs[0].mcp_status = vec![("alpha".into(), true), ("beta".into(), false)];
        app.tabs[0].instruction_files = 2;
        run_tool(&mut app, tab_id, 7, "b1", "Bash", true);
        run_tool(&mut app, tab_id, 7, "e1", "Edit", false);

        let mut term = Terminal::new(TestBackend::new(120, 32)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&term);
        let tally = row_containing(&rows, "✓ Bash ×1").expect("tally row: {rows:?}");
        let mode = row_containing(&rows, "auto mode on").expect("mode row: {rows:?}");
        let bar = row_containing(&rows, "F1 help").expect("status line: {rows:?}");
        assert!(rows[tally as usize].contains("✗ Edit ×1"), "{rows:?}");
        // Exactly one MCP is connected, so the segment counts one, not two.
        assert!(rows[mode as usize].contains("1 MCP"), "{rows:?}");
        assert!(rows[mode as usize].contains("2 CLAUDE.md"), "{rows:?}");
        assert!(tally < mode && mode < bar, "HUD sits above the status line");
        assert_eq!(app.status_rows, 3);

        // Too short for the HUD: the classic single status row comes back.
        let mut short = Terminal::new(TestBackend::new(120, 16)).unwrap();
        short.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&short);
        assert_eq!(app.status_rows, 1);
        assert!(row_containing(&rows, "✓ Bash ×1").is_none(), "{rows:?}");
        assert!(row_containing(&rows, "auto mode on").is_none(), "{rows:?}");
        assert!(row_containing(&rows, "F1 help").is_some(), "{rows:?}");
    }

    #[tokio::test]
    async fn clicking_a_tool_activity_summary_expands_the_calls_behind_it() {
        use ratatui::{backend::TestBackend, Terminal};
        let (mut app, agent_tx) = make_test_app().await;
        // Three finished calls as the event stream records them (call row,
        // result row, usage row each) — a run long enough to fold behind a
        // single summary line (small runs render their rows directly).
        for cmd in ["cargo build", "cargo test", "cargo fmt"] {
            app.tabs[0]
                .chat
                .push_tool_call(&format!("Tool Bash {cmd}"), "Bash");
            app.tabs[0].chat.push_tool_result("Tool Bash ok");
            app.tabs[0].chat.push_usage("Usage ↑10 ↓5");
        }
        app.tabs[0].chat.end_turn();

        let mut term = Terminal::new(TestBackend::new(120, 32)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&term);
        assert!(
            row_containing(&rows, "cargo build").is_none(),
            "the call rows must be folded away by default: {rows:?}"
        );
        let summary = row_containing(&rows, "Ran 3 shell commands")
            .expect("a collapsed summary line should be painted");

        // A plain click (press + release, no drag) on that row opens the group.
        app.handle_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 6, summary),
            &agent_tx,
        );
        app.handle_mouse(
            mouse_event(MouseEventKind::Up(MouseButton::Left), 6, summary),
            &agent_tx,
        );

        term.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&term);
        assert!(
            row_containing(&rows, "cargo build").is_some(),
            "clicking the summary should reveal the calls: {rows:?}"
        );
        assert!(
            row_containing(&rows, "Usage").is_some(),
            "the usage row lives inside the expanded group: {rows:?}"
        );
    }

    #[tokio::test]
    async fn clicking_the_jump_pill_returns_to_the_tail() {
        use ratatui::{backend::TestBackend, Terminal};
        let (mut app, agent_tx) = make_test_app().await;
        for i in 0..120 {
            app.tabs[0].chat.push_user(&format!("question number {i}"));
        }
        app.tabs[0].chat.push_user("FINAL_TAIL_MARKER");

        let mut term = Terminal::new(TestBackend::new(120, 32)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        assert!(
            row_containing(&painted_rows(&term), "Jump to bottom").is_none(),
            "no pill while following the tail"
        );

        // Scroll well past two viewports: the pill floats over the bottom row.
        app.tabs[0].chat.scroll_up(200);
        term.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&term);
        let pill = row_containing(&rows, "Jump to bottom").expect("pill painted: {rows:?}");
        assert!(
            row_containing(&rows, "FINAL_TAIL_MARKER").is_none(),
            "scrolled away from the tail"
        );

        app.handle_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 60, pill),
            &agent_tx,
        );
        app.handle_mouse(
            mouse_event(MouseEventKind::Up(MouseButton::Left), 60, pill),
            &agent_tx,
        );

        term.draw(|f| app.draw(f)).unwrap();
        let rows = painted_rows(&term);
        assert!(
            row_containing(&rows, "FINAL_TAIL_MARKER").is_some(),
            "the pill click should jump back to the newest output: {rows:?}"
        );
        assert!(
            row_containing(&rows, "Jump to bottom").is_none(),
            "the pill retires once the tail is in view"
        );
    }

    #[tokio::test]
    async fn drag_select_copies_to_clipboard_on_release() {
        // Copy-on-select (opencode's default): finishing a drag over text puts
        // the selection on the clipboard immediately. Cmd+C can't reach a TUI on
        // macOS, so "select, then Cmd+V" is the copy path — the drag-release
        // must write the clipboard on its own.
        let (mut app, _agent_tx) = make_test_app().await;
        app.input.set_text("hello world");
        app.toast = None;

        // A single-row input box; its body starts at column 2 (input_body_area).
        let input_area = Rect::new(0, 0, 40, 1);

        // Press → drag → release selects "hello" (columns 2..7 → chars 0..5).
        app.handle_input_selection_mouse(
            mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 0),
            input_area,
        );
        app.handle_input_selection_mouse(
            mouse_event(MouseEventKind::Drag(MouseButton::Left), 7, 0),
            input_area,
        );
        app.handle_input_selection_mouse(
            mouse_event(MouseEventKind::Up(MouseButton::Left), 7, 0),
            input_area,
        );

        // The drag leaves a live (non-empty) selection...
        let sel = app
            .active_input_selection
            .expect("a drag should leave a selection");
        assert_ne!(sel.anchor, sel.focus, "selection is non-empty");
        // ...and copies it on release — the toast confirms the clipboard write.
        assert!(
            app.toast.is_some(),
            "finishing a drag should copy the selection (copy-on-select)"
        );
    }

    #[tokio::test]
    async fn compact_refreshes_the_context_gauge() {
        // Regression: after /compact shrinks the store, the "% ctx" badge must
        // drop right away. It reads tab.context_tokens live, so the CompactDone
        // handler has to recompute that field — otherwise it stays stuck at the
        // pre-compact value until the next turn's Usage event.
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;

        // Pretend the gauge holds a large pre-compact count while the (freshly
        // compacted) store is small.
        app.tabs[0].context_tokens = 50_000;
        let op_id = arm_local_op_for_test(&mut app, 0);

        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id,
            result: Ok("compacted the transcript".to_string()),
            auto: false,
        });

        let store_tokens = {
            let store = app.tabs[0].engine.store.lock().unwrap();
            estimate_store_tokens(&store)
        };
        assert_eq!(
            app.tabs[0].context_tokens, store_tokens,
            "ctx gauge must be recomputed from the store after compaction"
        );
        assert!(
            app.tabs[0].context_tokens < 50_000,
            "gauge should drop after compaction, not stay at the pre-compact value"
        );
    }

    #[tokio::test]
    async fn auto_compact_breaker_stops_the_retry_loop() {
        // Regression: with the context stuck over the threshold and the
        // provider failing every summarize call (e.g. 400 context-overflow),
        // CompactDone(Err) → maybe_auto_compact re-fired forever, one LLM
        // call per lap. After AUTO_COMPACT_MAX_FAILURES auto failures the
        // trigger must stop; a success re-arms it.
        let (mut app, agent_tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        // Pin the gauge over the auto threshold for a known window.
        app.tabs[0].context_tokens = u32::MAX;
        assert!(app.tabs[0]
            .engine
            .needs_pre_turn_compact(app.tabs[0].context_tokens));

        for _ in 0..AUTO_COMPACT_MAX_FAILURES {
            let op_id = arm_local_op_for_test(&mut app, 0);
            app.handle_agent_event(AppEvent::CompactDone {
                tab_id,
                op_id,
                result: Err("HTTP 400: input token limit".into()),
                auto: true,
            });
        }
        assert_eq!(app.tabs[0].auto_compact_failures, AUTO_COMPACT_MAX_FAILURES);

        // Breaker open: the trigger must NOT start another compaction.
        app.maybe_auto_compact(&agent_tx);
        assert!(
            !app.tabs[0].is_busy(),
            "auto-compact must stay off once the breaker is open"
        );
        assert!(!matches!(app.tabs[0].mode, Mode::Compacting));

        // A successful (manual) compaction re-arms the trigger.
        let op_id = arm_local_op_for_test(&mut app, 0);
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id,
            result: Ok("compacted".into()),
            auto: false,
        });
        assert_eq!(app.tabs[0].auto_compact_failures, 0);

        // Manual-failure counting is out of scope for the breaker: a manual
        // /compact failure must not advance it.
        let op_id = arm_local_op_for_test(&mut app, 0);
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id,
            result: Err("boom".into()),
            auto: false,
        });
        assert_eq!(app.tabs[0].auto_compact_failures, 0);
    }

    #[tokio::test]
    async fn auto_compact_success_without_progress_trips_the_breaker() {
        // Regression: near 100% occupancy an EarliestHalf compaction can
        // "succeed" while reclaiming almost nothing (the mass sits in the
        // preserved half). CompactDone(Ok) used to reset the breaker to 0,
        // so maybe_auto_compact re-fired on the very next event-loop pass —
        // an endless loop of useless LLM calls with the gauge pinned at
        // ~100%. A success that leaves the gauge over the threshold must
        // advance the breaker like a failure.
        let (mut app, agent_tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let window = app.tabs[0].engine.model_max_tokens;
        assert!(window > 0, "test engine needs a real context window");
        // A store whose own estimate stays at ~100% of the window: the
        // CompactDone handler recomputes the gauge from the store, so the
        // "compaction changed nothing" case is a store that stays huge.
        {
            let mut store = app.tabs[0].engine.store.lock().unwrap();
            store
                .push(agent::message::Message::User {
                    header: agent::message::Header::new(),
                    content: vec![agent::message::ContentBlock::Text {
                        text: "x".repeat(window as usize * 4),
                    }],
                })
                .unwrap();
        }

        for round in 1..=AUTO_COMPACT_MAX_FAILURES {
            let op_id = arm_local_op_for_test(&mut app, 0);
            app.handle_agent_event(AppEvent::CompactDone {
                tab_id,
                op_id,
                result: Ok("compacted 1 message · ~10 → ~10 tokens".into()),
                auto: true,
            });
            assert_eq!(
                app.tabs[0].auto_compact_failures, round,
                "a no-progress success must advance the breaker"
            );
        }
        assert!(app.tabs[0]
            .engine
            .needs_pre_turn_compact(app.tabs[0].context_tokens));

        // Breaker open: the trigger must NOT start another compaction.
        app.maybe_auto_compact(&agent_tx);
        assert!(
            !app.tabs[0].is_busy(),
            "no-progress breaker must stop the auto-compact loop"
        );

        // Genuine progress (gauge drops under the threshold) re-arms it.
        {
            let mut store = app.tabs[0].engine.store.lock().unwrap();
            *store = agent::message::MessageStore::new();
        }
        let op_id = arm_local_op_for_test(&mut app, 0);
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id,
            result: Ok("compacted".into()),
            auto: true,
        });
        assert_eq!(app.tabs[0].auto_compact_failures, 0);
    }

    #[tokio::test]
    async fn runtime_compact_notice_refreshes_the_context_gauge() {
        // The QueryLoop's own auto-compaction rewrites the store MID-TURN and
        // announces it with an agent.compact.ok / agent.compact.micro notice.
        // The "% ctx" badge must drop right then — not linger at ~100% until
        // the next Usage event (which never comes if the turn errors first).
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;

        for code in ["agent.compact.ok", "agent.compact.micro"] {
            app.tabs[0].context_tokens = 999_999;
            app.handle_agent_event(AppEvent::Agent {
                tab_id,
                turn_id: 7,
                cost_label: None,
                event: Event::Notice {
                    code: code.into(),
                    message: "auto-compact 100 → 10 tokens".into(),
                },
            });
            let store_tokens = {
                let store = app.tabs[0].engine.store.lock().unwrap();
                estimate_store_tokens(&store)
            };
            assert_eq!(
                app.tabs[0].context_tokens, store_tokens,
                "{code} must recompute the ctx gauge from the rewritten store"
            );
        }

        // Unrelated notices must NOT touch the gauge (Usage owns it).
        app.tabs[0].context_tokens = 999_999;
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: Event::Notice {
                code: "api_retry".into(),
                message: "retrying".into(),
            },
        });
        assert_eq!(app.tabs[0].context_tokens, 999_999);
    }

    #[tokio::test]
    async fn clear_resets_runtime_compact_latches() {
        // /clear starts a fresh conversation; runtime compaction latches
        // (no-progress, failure breaker) describe the discarded one and
        // would otherwise keep the QueryLoop's auto-compaction disabled
        // for the rest of the session.
        let (mut app, agent_tx) = make_test_app().await;
        {
            let mut s = app.tabs[0].engine.compact_state.lock().unwrap();
            s.record_no_progress();
            s.record_failure();
        }
        app.handle_slash("clear", "", &agent_tx).await;
        let s = app.tabs[0].engine.compact_state.lock().unwrap();
        assert!(!s.no_progress, "/clear must clear the no-progress latch");
        assert_eq!(s.consecutive_failures, 0, "/clear must re-arm the breaker");
    }

    #[tokio::test]
    async fn watchdog_slash_command_reports_status_and_usage() {
        let (mut app, agent_tx) = make_test_app().await;
        app.handle_slash("watchdog", "status", &agent_tx).await;
        let status = &app.active_tab().chat.messages().last().unwrap().text;
        assert!(status.contains("watchdog: enabled"));
        assert!(status.contains("inactivity"));

        app.handle_slash("watchdog", "unknown", &agent_tx).await;
        assert_eq!(
            app.active_tab().chat.messages().last().unwrap().text,
            "usage: /watchdog [status]"
        );
        assert_eq!(app.active_tab().active_turn_id, 0);
        assert!(app.active_tab().queued_input.is_empty());
    }

    #[tokio::test]
    async fn model_switch_hot_swaps_without_reassemble_pending() {
        let (mut app, agent_tx) = make_test_app().await;
        app.active_tab_mut().extension_access = zode_core::ToolAccessMode::ReadOnly;

        app.handle_slash("model", "other-model", &agent_tx).await;

        assert!(
            !app.active_tab().is_busy(),
            "model switch should not mark the tab busy"
        );
        assert!(!app.active_tab().reassemble_pending);
        assert_eq!(
            app.status.model, "other-model",
            "visible model should update immediately"
        );
        assert_eq!(app.active_tab().engine.model, "other-model");
        assert_eq!(
            app.active_tab().extension_access,
            zode_core::ToolAccessMode::ReadOnly,
            "ordinary /model preserves the active task's access"
        );
        assert_eq!(app.template.model(), Some("other-model"));
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt,
            "task-local access must not leak into the clean global default"
        );
        assert!(!app.template.plan_mode());
    }

    #[tokio::test]
    async fn sandbox_reload_and_plan_reassembly_preserve_task_local_overrides() {
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let effective = app
            .template
            .with_model("other-model".into())
            .with_tool_access(zode_core::ToolAccessMode::Auto)
            .with_plan_mode(true);
        effective
            .hot_swap_model(
                Arc::get_mut(&mut app.tabs[0].engine).expect("test owns the engine"),
                "other-model".into(),
            )
            .unwrap();
        app.tabs[0].extension_access = zode_core::ToolAccessMode::Auto;
        app.tabs[0].plan_mode = true;
        let carried_cost = app.tabs[0].engine.cost.clone();

        for effect in [
            ReassembleEffect::Sandbox,
            ReassembleEffect::ReloadSkills,
            ReassembleEffect::Plan { on: true },
        ] {
            let (tx, mut rx) = mpsc::unbounded_channel();
            assert!(app.start_reassemble_active(app.template.clone(), effect, &tx));
            let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
                .await
                .expect("reassembly finishes")
                .expect("event channel stays open");
            app.handle_agent_event(event);

            assert_eq!(app.tabs[0].engine.model, "other-model");
            assert_eq!(
                app.tabs[0].extension_access,
                zode_core::ToolAccessMode::Auto
            );
            assert!(app.tabs[0].plan_mode);
            assert!(Arc::ptr_eq(&app.tabs[0].engine.cost, &carried_cost));
            assert_eq!(app.template.model(), Some("test-model"));
            assert_eq!(
                app.template.tool_access(),
                zode_core::ToolAccessMode::Prompt
            );
            assert!(!app.template.plan_mode());
        }
    }

    #[tokio::test]
    async fn ordinary_yolo_toggles_active_when_global_and_task_access_diverge() {
        for (global, active, expected) in [
            (
                zode_core::ToolAccessMode::Auto,
                zode_core::ToolAccessMode::Prompt,
                zode_core::ToolAccessMode::Auto,
            ),
            (
                zode_core::ToolAccessMode::Prompt,
                zode_core::ToolAccessMode::Auto,
                zode_core::ToolAccessMode::Prompt,
            ),
        ] {
            let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
            app.template = app.template.with_tool_access(global);
            app.tabs[0].extension_access = active;
            let second_engine = app
                .template
                .assemble_tab(None, Some("2".into()))
                .await
                .unwrap();
            let mut second = SessionTab::new(2, Arc::new(second_engine), "second-task".into());
            second.extension_access = global;
            app.tabs.push(second);
            let (tx, mut rx) = mpsc::unbounded_channel();

            app.handle_slash("yolo", "", &tx).await;
            let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
                .await
                .expect("yolo reassembly finishes")
                .expect("event channel stays open");
            app.handle_agent_event(event);

            assert_eq!(app.tabs[0].extension_access, expected);
            assert_eq!(app.template.tool_access(), expected);
            assert_eq!(
                app.tabs[1].extension_access, global,
                "existing background tabs keep their local access"
            );
            assert!(!app.template.plan_mode());
        }
    }

    #[tokio::test]
    async fn shift_tab_toggles_yolo_and_ask_modes() {
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let (tx, mut rx) = mpsc::unbounded_channel();

        for (code, modifiers, expected) in [
            (
                KeyCode::BackTab,
                KeyModifiers::SHIFT,
                zode_core::ToolAccessMode::Auto,
            ),
            (
                KeyCode::Tab,
                KeyModifiers::SHIFT,
                zode_core::ToolAccessMode::Prompt,
            ),
        ] {
            send_key(&mut app, &tx, code, modifiers).await;
            let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
                .await
                .expect("Shift+Tab reassembly finishes")
                .expect("event channel stays open");
            app.handle_agent_event(event);

            assert_eq!(app.tabs[0].extension_access, expected);
            assert_eq!(app.template.tool_access(), expected);

            // The toggle persists GLOBALLY so the next launch in ANY
            // workspace starts with the same access mode; any per-project
            // state entry is removed so it can't shadow the new choice.
            let cfg = zode_core::config::ConfigManager::load_global().unwrap();
            assert_eq!(
                cfg.yolo,
                Some(expected == zode_core::ToolAccessMode::Auto),
                "yolo toggle must persist to the global config"
            );
            let state_path =
                zode_core::config::ConfigManager::project_state_path(&app.tabs[0].engine.cwd);
            let state_yolo = std::fs::read_to_string(&state_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("yolo").cloned());
            assert_eq!(state_yolo, None, "stale per-project yolo must be cleared");
        }
    }

    #[tokio::test]
    async fn sandbox_toggle_persists_globally() {
        // The sandbox reassemble effect must record the new state in the
        // GLOBAL config (so every workspace's next launch keeps it) and clear
        // any per-project state entry that would shadow it.
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let cwd = app.tabs[0].engine.cwd.clone();
        zode_core::config::ConfigManager::update_project_state(&cwd, |s| {
            s.insert("sandbox".into(), serde_json::json!({"enabled": true}));
        })
        .unwrap();

        app.template = app.template.with_sandbox(None); // "/sandbox off"
        app.apply_sandbox_reassemble_effect(0);

        let cfg = zode_core::config::ConfigManager::load_global().unwrap();
        assert_eq!(
            cfg.sandbox.enabled,
            Some(false),
            "sandbox off must persist to the global config"
        );
        let state_path = zode_core::config::ConfigManager::project_state_path(&cwd);
        let state_sandbox = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("sandbox").cloned());
        assert_eq!(
            state_sandbox, None,
            "stale per-project sandbox must be cleared"
        );
    }

    #[tokio::test]
    async fn goal_set_hot_swaps_prompt_and_starts_loop_immediately() {
        let (mut app, agent_tx) = make_test_app().await;

        app.handle_slash("goal", "ship the fix", &agent_tx).await;

        assert!(
            !app.active_tab().reassemble_pending,
            "setting a goal should not start engine reassembly"
        );
        assert!(
            app.active_tab().goal_loop_active,
            "goal loop should start immediately"
        );
        assert!(
            app.active_tab()
                .queued_input
                .iter()
                .any(|msg| msg == GOAL_LOOP_START_PROMPT),
            "first goal-loop prompt should be queued immediately"
        );
        assert!(
            app.active_tab()
                .engine
                .system
                .as_deref()
                .is_some_and(|system| system.contains("ship the fix")),
            "goal should be injected into the active system prompt immediately"
        );
    }

    #[tokio::test]
    async fn goal_clear_hot_swaps_prompt_without_reassemble_pending() {
        let (mut app, agent_tx) = make_test_app().await;

        app.handle_slash("goal", "ship the fix", &agent_tx).await;
        app.handle_slash("goal", "clear", &agent_tx).await;

        assert!(!app.active_tab().reassemble_pending);
        assert!(!app.active_tab().goal_loop_active);
        assert!(app.active_tab().queued_input.is_empty());
        assert!(
            app.active_tab()
                .engine
                .system
                .as_deref()
                .is_some_and(|system| !system.contains("ship the fix")),
            "cleared goal should be removed from the active system prompt"
        );
    }

    // The auto-compact threshold decision itself (occupancy + output budget
    // vs. the window) lives in zode-core: `pre_turn_compact_needed`, exposed
    // as `ZodeEngine::needs_pre_turn_compact`, with unit tests beside it.

    #[test]
    fn format_elapsed_is_compact() {
        use std::time::Duration;
        assert_eq!(format_elapsed(Duration::from_secs(5)), "5s");
        assert_eq!(format_elapsed(Duration::from_secs(59)), "59s");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1m 05s");
        assert_eq!(format_elapsed(Duration::from_secs(3725)), "1h 02m");
    }

    #[test]
    fn tool_output_preview_extracts_and_truncates() {
        use serde_json::json;
        // Bash stdout is shown; stderr is appended.
        assert_eq!(
            tool_output_preview(&json!({"stdout": "hello\nhello"})).as_deref(),
            Some("hello\nhello")
        );
        let p = tool_output_preview(&json!({"stdout": "out", "stderr": "err"})).unwrap();
        assert!(p.contains("out") && p.contains("err"));
        // File reads show `content`.
        assert_eq!(
            tool_output_preview(&json!({"content": "line"})).as_deref(),
            Some("line")
        );
        // Status-only payloads (an edit/write) have nothing to preview.
        assert!(tool_output_preview(&json!({"path": "/x", "status": "ok"})).is_none());
        assert!(tool_output_preview(&json!({"stdout": "   "})).is_none());
        // Full output is kept — expanding a tool block must show everything
        // (the collapsed header hides it by default anyway).
        let many = (0..30)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let p = tool_output_preview(&json!({"stdout": many})).unwrap();
        assert_eq!(p.lines().count(), 30);
        assert!(
            !p.contains("truncated"),
            "no truncation under the safety net"
        );
        // A distant safety net still guards pathological payloads.
        let huge = "x".repeat(600_000);
        let p = tool_output_preview(&json!({"stdout": huge})).unwrap();
        assert!(p.contains("truncated"));
        assert!(p.chars().count() <= 500_100);
    }

    #[tokio::test]
    async fn goal_loop_continues_on_success_and_stops_on_failure() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        // Arm the loop as `/goal <text>` does.
        app.tabs[0].goal_loop_active = true;
        app.tabs[0].goal_loop_iter = 0;

        // A successful turn with no completion signal → queue the next iteration.
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 0,
            result: Ok(()),
        });
        assert!(app.tabs[0].goal_loop_active, "loop stays active on success");
        assert_eq!(app.tabs[0].goal_loop_iter, 1);
        assert!(
            app.tabs[0]
                .queued_input
                .iter()
                .any(|s| s == GOAL_LOOP_CONTINUE_PROMPT),
            "a continuation turn is queued"
        );

        // A failed turn halts the loop (no runaway on errors).
        app.tabs[0].queued_input.clear();
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 0,
            result: Err("boom".to_string()),
        });
        assert!(
            !app.tabs[0].goal_loop_active,
            "a failed turn stops the loop"
        );
        assert!(
            app.tabs[0].queued_input.is_empty(),
            "no continuation queued after a failure"
        );
    }

    #[tokio::test]
    async fn goal_loop_stops_after_consecutive_no_progress_turns() {
        // A model that keeps "succeeding" without using any tool is spinning —
        // the loop must stop after GOAL_LOOP_NO_PROGRESS_LIMIT such turns.
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].goal_loop_active = true;

        for _ in 0..GOAL_LOOP_NO_PROGRESS_LIMIT {
            assert!(
                app.tabs[0].goal_loop_active,
                "still looping before the limit"
            );
            app.tabs[0].turn_used_tools = false; // no tool use this turn
            app.handle_agent_event(AppEvent::TurnDone {
                tab_id,
                turn_id: 0,
                result: Ok(()),
            });
        }
        assert!(
            !app.tabs[0].goal_loop_active,
            "no-progress streak stops the loop"
        );
    }

    #[tokio::test]
    async fn goal_loop_tool_use_resets_the_no_progress_streak() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].goal_loop_active = true;

        // Two no-progress turns, then a productive one resets the streak.
        for used in [false, false, true, false, false] {
            app.tabs[0].turn_used_tools = used;
            app.handle_agent_event(AppEvent::TurnDone {
                tab_id,
                turn_id: 0,
                result: Ok(()),
            });
        }
        // Streak never hit 3 in a row → loop still active.
        assert!(
            app.tabs[0].goal_loop_active,
            "a tool-using turn resets the no-progress streak"
        );
    }

    #[tokio::test]
    async fn goal_loop_honors_the_default_turn_cap() {
        // With no autoLoopMaxTurns configured, the built-in default caps the
        // loop so it can't run unbounded. Each turn uses a tool (no
        // no-progress stop) to isolate the cap.
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].goal_loop_active = true;
        for _ in 0..GOAL_LOOP_DEFAULT_MAX_TURNS + 2 {
            if !app.tabs[0].goal_loop_active {
                break;
            }
            app.tabs[0].turn_used_tools = true;
            app.handle_agent_event(AppEvent::TurnDone {
                tab_id,
                turn_id: 0,
                result: Ok(()),
            });
        }
        assert!(
            !app.tabs[0].goal_loop_active,
            "the default cap stops an otherwise-unbounded loop"
        );
        assert!(app.tabs[0].goal_loop_iter <= GOAL_LOOP_DEFAULT_MAX_TURNS);
    }

    #[tokio::test]
    async fn goal_loop_does_not_double_queue_continuations() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].goal_loop_active = true;
        // A user message already sits in the queue alongside a stale
        // continuation; a productive TurnDone must not stack a second one.
        app.tabs[0]
            .queued_input
            .push_back(GOAL_LOOP_CONTINUE_PROMPT.to_string());
        app.tabs[0].turn_used_tools = true;
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 0,
            result: Ok(()),
        });
        let continuations = app.tabs[0]
            .queued_input
            .iter()
            .filter(|s| *s == GOAL_LOOP_CONTINUE_PROMPT)
            .count();
        assert_eq!(continuations, 1, "continuation must not be double-queued");
    }

    #[tokio::test]
    async fn interrupt_keeps_tab_busy_until_the_aborted_turn_drains() {
        // A fast resubmit after Esc must not race the aborted task's teardown:
        // the tab stays busy (draining) until that task's terminal TurnDone.
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.tabs[0].active_turn_id = 42;
        app.tabs[0]
            .active_tool_names
            .insert("t1".into(), "Bash".into());

        assert!(app.interrupt_active_turn());
        assert!(
            app.tabs[0].is_busy(),
            "tab stays busy (draining) right after interrupt"
        );
        assert!(
            app.tabs[0].active_tool_names.is_empty(),
            "interrupt clears stale in-flight tool titles"
        );

        // The aborted turn's terminal TurnDone (its old turn_id) clears the
        // draining latch — the tab is idle again.
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 42,
            result: Err("aborted".to_string()),
        });
        assert!(
            !app.tabs[0].is_busy(),
            "draining clears when the aborted turn's TurnDone lands"
        );
    }

    #[tokio::test]
    async fn timed_out_draining_turn_done_finishes_watchdog_once() {
        let (mut app, _tx) = make_test_app().await;
        app.watchdog = BackgroundWatchdog::new(zode_core::config::BackgroundWatchdogConfig {
            enabled: Some(true),
            inactivity_timeout_secs: Some(5),
            max_runtime_secs: Some(30),
            abort_grace_secs: Some(2),
            max_retries: Some(2),
            initial_backoff_secs: Some(3),
            max_backoff_secs: Some(10),
        });
        let start = std::time::Instant::now();
        let tab_id = app.tabs[0].id;
        let job = SchedJobRef::Loop(77);
        app.watchdog.start(
            job.clone(),
            tab_id,
            42,
            "check".into(),
            start,
            AbortController::new().activity(),
        );
        assert!(matches!(
            app.watchdog
                .poll(start + std::time::Duration::from_secs(6))
                .as_slice(),
            [WatchdogAction::Abort { .. }]
        ));
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].draining_turn_id = Some(42);
        app.tabs[0].active_sched_job = Some(job);

        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 42,
            result: Err("aborted".into()),
        });
        assert_eq!(app.tabs[0].draining_turn_id, None);
        assert_eq!(
            app.watchdog
                .due_retries(start + std::time::Duration::from_secs(99))
                .len(),
            1
        );

        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 42,
            result: Err("aborted".into()),
        });
        assert_eq!(
            app.watchdog
                .due_retries(start + std::time::Duration::from_secs(99))
                .len(),
            1,
            "duplicate stale terminal cannot increment recovery twice"
        );
    }

    #[tokio::test]
    async fn raced_canonical_terminal_is_finished_with_its_real_outcome() {
        let (mut app, _tx) = make_test_app().await;
        let start = std::time::Instant::now();
        let tab_id = app.tabs[0].id;
        let job = SchedJobRef::Loop(900);
        app.watchdog.start(
            job.clone(),
            tab_id,
            42,
            "check".into(),
            start,
            AbortController::new().activity(),
        );
        app.watchdog.cancel_turn(tab_id, 42, start);
        let actions = app.watchdog.poll(start + Duration::from_secs(11));
        let (forced_job, forced_failure) = match actions.as_slice() {
            [WatchdogAction::ForceCancel { job, failure, .. }] => (job.clone(), failure.clone()),
            other => panic!("expected a real force-cancel action, got {other:?}"),
        };
        assert!(forced_failure.is_none());
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].draining_turn_id = Some(42);
        app.tabs[0].active_sched_job = Some(job.clone());
        app.forced_turn_stops.insert(
            (tab_id, 42),
            PendingForcedTurnStop {
                outcome: ForcedTurnStop::Manual {
                    job: forced_job,
                    failure: forced_failure,
                },
                attempt_lease: None,
                activity: None,
                quarantine: None,
                source_terminal_seen: false,
                recorder: None,
                engine: None,
            },
        );

        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 42,
            result: Err("provider failed before forced waiter completed".into()),
        });

        let pending = app.forced_turn_stops.get(&(tab_id, 42)).unwrap();
        assert!(pending.source_terminal_seen);
        assert!(matches!(
            &pending.outcome,
            ForcedTurnStop::Canonical { result: Err(_), .. }
        ));

        app.handle_agent_event(AppEvent::TurnTaskStopped {
            tab_id,
            turn_id: 42,
        });

        assert_eq!(app.tabs[0].mode, Mode::Error);
        assert_eq!(
            app.watchdog
                .due_retries(start + Duration::from_secs(99))
                .len(),
            1,
            "the canonical error enters recovery instead of being cleared as success"
        );
    }

    #[tokio::test]
    async fn canonical_turn_quarantine_keeps_the_tab_busy_until_workers_stop() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let loop_id = app.scheduler.add_loop(
            tab_id as u64,
            "check".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.tabs[0].active_turn_id = 42;
        app.tabs[0].active_sched_job = Some(SchedJobRef::Loop(loop_id));

        app.handle_agent_event(AppEvent::TurnTaskQuarantined {
            tab_id,
            turn_id: 42,
            result: Some(Ok(())),
        });

        assert!(app.forced_turn_stops.contains_key(&(tab_id, 42)));
        assert!(matches!(
            &app.forced_turn_stops[&(tab_id, 42)].outcome,
            ForcedTurnStop::Canonical { result: Err(_), .. }
        ));
        assert!(app.tabs[0].is_busy(), "quarantine keeps the slot fenced");
        assert!(
            app.scheduler.loops().iter().all(|job| job.id != loop_id),
            "the quarantined job is stopped before its lease can be released"
        );
        assert_eq!(app.tabs[0].mode, Mode::Error);

        app.handle_agent_event(AppEvent::TurnTaskStopped {
            tab_id,
            turn_id: 42,
        });

        assert!(!app.forced_turn_stops.contains_key(&(tab_id, 42)));
        assert!(!app.tabs[0].is_busy());
        assert_eq!(app.tabs[0].mode, Mode::Error);
    }

    #[tokio::test]
    async fn interrupting_non_agent_abort_handle_never_enters_draining_state() {
        let (mut app, _tx) = make_test_app().await;
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].local_op_seq = 9;
        app.tabs[0].active_local_op_id = Some(9);
        app.tabs[0].turn_abort = Some(AbortController::new());

        assert!(app.interrupt_active_turn());
        assert_eq!(app.tabs[0].draining_turn_id, None);
        assert_eq!(app.tabs[0].active_local_op_id, None);
        assert_eq!(app.tabs[0].local_op_seq, 9);
        assert!(!app.tabs[0].is_busy());
    }

    #[tokio::test]
    async fn local_operation_generation_never_wraps() {
        let (mut app, _tx) = make_test_app().await;
        app.tabs[0].local_op_seq = u64::MAX;

        assert!(app.begin_local_operation(0).is_none());
        assert_eq!(app.tabs[0].local_op_seq, u64::MAX);
        assert_eq!(app.tabs[0].active_local_op_id, None);
        assert!(app.tabs[0].turn_abort.is_none());
    }

    #[tokio::test]
    async fn tui_turn_generation_never_wraps_or_starts_turn_zero() {
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].titled = true;
        app.tabs[0].turn_seq = u64::MAX;
        let messages_before = app.tabs[0].chat.messages().len();

        app.submit("must not become turn zero", &agent_tx).await;

        assert_eq!(app.tabs[0].turn_seq, u64::MAX);
        assert_eq!(app.tabs[0].active_turn_id, 0);
        assert!(app.tabs[0].turn_abort.is_none());
        assert_eq!(app.tabs[0].chat.messages().len(), messages_before);
        assert!(
            app.toast.is_some(),
            "sequence exhaustion is visible to the user"
        );
    }

    #[tokio::test]
    async fn bailed_turn_start_never_stamps_or_consumes_sched_pending() {
        // Regression test: `SessionTab::active_sched_job` must only be
        // stamped — and the matching `App::sched_pending` entry only
        // consumed — once `start_turn_on_tab` is past its LAST early-return
        // point. Before the fix, the call sites stamped/removed BEFORE
        // calling `start_turn_on_tab`, so a bailed dispatch (turn_seq
        // overflow, unsupported image route, ...) consumed the
        // `sched_pending` entry and left a stuck `active_sched_job` on the
        // tab. Since no turn actually starts, `TurnDone` never clears it, so
        // the tab's NEXT unrelated turn would get misattributed to the
        // scheduler job — corrupting the 3-strikes circuit breaker.
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].titled = true;
        // Deterministic bail: `turn_seq` at `u64::MAX` makes `checked_add(1)`
        // fail, the last early-return point in `start_turn_on_tab`.
        app.tabs[0].turn_seq = u64::MAX;
        let job = SchedJobRef::Schedule("abcd".into());
        let key = (app.tabs[0].id, "check ci".to_string());
        app.push_sched_pending(key.clone(), job.clone());

        app.submit("check ci", &agent_tx).await;

        assert_eq!(
            app.tabs[0].turn_seq,
            u64::MAX,
            "the bail must not advance turn_seq — no turn started"
        );
        assert!(
            app.tabs[0].active_sched_job.is_none(),
            "a bailed dispatch must not stamp scheduler attribution"
        );
        assert_eq!(
            app.sched_pending.get(&key).and_then(|jobs| jobs.front()),
            Some(&job),
            "a bailed dispatch must not consume the pending scheduler entry"
        );
    }

    #[tokio::test]
    async fn direct_same_text_submit_never_claims_scheduler_occurrence() {
        let (mut app, agent_tx) = make_test_app().await;
        let job = SchedJobRef::Loop(701);
        let prompt = "check ci".to_string();
        let key = (app.tabs[0].id, prompt.clone());
        app.tabs[0].queued_input.push_back(prompt.clone());
        app.push_sched_pending(key.clone(), job.clone());

        assert!(app.submit(&prompt, &agent_tx).await);

        assert!(app.tabs[0].active_sched_job.is_none());
        assert_eq!(
            app.sched_pending.get(&key).and_then(|jobs| jobs.front()),
            Some(&job),
            "only an explicit queue-drain provenance may claim attribution"
        );
        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec![prompt]
        );
    }

    #[tokio::test]
    async fn tick_dispatch_preserves_a_popped_occurrence_when_turn_start_bails() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        app.tabs[0].turn_seq = u64::MAX;

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec!["check ci".to_string()]
        );
        assert!(app.tabs[0].active_sched_job.is_none());
        assert!(
            app.sched_job_is_pending(&SchedJobRef::Loop(id)),
            "a popped prompt that cannot start must restore its exact attribution"
        );

        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec!["check ci".to_string()],
            "the retained occurrence blocks duplicate fires until its queue deadline"
        );
    }

    #[tokio::test]
    async fn ordinary_queue_dispatch_preserves_a_bailed_scheduler_occurrence() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        app.tabs[0].turn_seq = u64::MAX;

        app.dispatch_queued_input(&agent_tx).await;

        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec!["check ci".to_string()],
            "a preflight failure must leave the exact scheduler occurrence queued"
        );
        assert!(app.sched_job_is_pending(&SchedJobRef::Loop(id)));
        assert!(app.tabs[0].active_sched_job.is_none());
    }

    #[tokio::test]
    async fn scheduler_image_path_preflight_does_not_accumulate_attachments() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("watch.png");
        std::fs::write(
            &path,
            [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0],
        )
        .unwrap();
        let prompt = path.display().to_string();
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            prompt.clone(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        app.tabs[0].turn_seq = u64::MAX;

        app.dispatch_scheduler_queued(&agent_tx).await;
        app.dispatch_scheduler_queued(&agent_tx).await;

        assert!(
            app.tabs[0].pending_images.is_empty(),
            "a scheduler path is literal text and cannot append an attachment per tick"
        );
        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec![prompt]
        );
        assert!(app.sched_job_is_pending(&SchedJobRef::Loop(id)));
    }

    #[tokio::test]
    async fn shutdown_fence_never_starts_a_queued_scheduler_occurrence() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        app.should_quit = true;

        app.dispatch_queued_input(&agent_tx).await;
        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(
            app.tabs[0].queued_input.iter().cloned().collect::<Vec<_>>(),
            vec!["check ci".to_string()]
        );
        assert_eq!(app.tabs[0].active_turn_id, 0);
        assert!(app.tabs[0].active_sched_job.is_none());
        assert!(app.sched_job_is_pending(&SchedJobRef::Loop(id)));
    }

    #[tokio::test]
    async fn stopping_the_loop_purges_queued_continuations_but_keeps_user_input() {
        // Regression (codex): a queued goal-loop continuation must not dispatch
        // after the loop stops — but a user's own queued follow-up survives.
        let (mut app, _tx) = make_test_app().await;
        app.tabs[0].goal_loop_active = true;
        app.tabs[0].goal_loop_iter = 3;
        app.tabs[0]
            .queued_input
            .push_back("user follow-up".to_string());
        app.tabs[0]
            .queued_input
            .push_back(GOAL_LOOP_CONTINUE_PROMPT.to_string());

        stop_goal_loop(&mut app.tabs[0]);

        assert!(!app.tabs[0].goal_loop_active);
        assert_eq!(app.tabs[0].goal_loop_iter, 0);
        let q: Vec<String> = app.tabs[0].queued_input.iter().cloned().collect();
        assert_eq!(
            q,
            vec!["user follow-up".to_string()],
            "continuation purged, user input kept"
        );
    }

    #[tokio::test]
    async fn two_escs_clear_a_non_empty_draft_when_idle() {
        let (mut app, agent_tx) = make_test_app().await;
        app.input
            .set_text("a long draft I don't want to lose by accident");
        assert!(!app.active_tab().is_busy());

        // First Esc only arms (draft preserved).
        send_key(&mut app, &agent_tx, KeyCode::Esc, KeyModifiers::NONE).await;
        assert!(app.esc_clear_armed, "first Esc arms");
        assert!(!app.input.is_empty(), "first Esc keeps the draft");

        // Second Esc clears it.
        send_key(&mut app, &agent_tx, KeyCode::Esc, KeyModifiers::NONE).await;
        assert!(app.input.is_empty(), "second Esc clears the draft");
        assert!(!app.esc_clear_armed, "clearing disarms");
    }

    #[tokio::test]
    async fn a_keystroke_between_escs_disarms_the_clear_gesture() {
        let (mut app, agent_tx) = make_test_app().await;
        app.input.set_text("draft");

        send_key(&mut app, &agent_tx, KeyCode::Esc, KeyModifiers::NONE).await;
        assert!(app.esc_clear_armed, "armed after first Esc");

        // Typing a character disarms; the next Esc must NOT wipe the draft.
        send_key(&mut app, &agent_tx, KeyCode::Char('x'), KeyModifiers::NONE).await;
        assert!(!app.esc_clear_armed, "a non-Esc key disarms");

        send_key(&mut app, &agent_tx, KeyCode::Esc, KeyModifiers::NONE).await;
        assert!(
            !app.input.is_empty(),
            "lone Esc after typing keeps the draft"
        );
    }

    #[tokio::test]
    async fn up_selects_image_chip_and_backspace_removes_it() {
        let (mut app, agent_tx) = make_test_app().await;
        app.input.take(); // empty input → ↑ drives chip selection, not history
        let png = [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        for _ in 0..2 {
            let img =
                zode_core::images::image_attachment_from_bytes(&png, "clipboard image").unwrap();
            app.active_tab_mut().pending_images.push(img);
        }
        assert_eq!(app.selected_image, None);

        // ↑ selects the last chip, ↑ again steps to the earlier one.
        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(app.selected_image, Some(1));
        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(app.selected_image, Some(0));

        // Backspace removes the selected image; selection clamps to what's left.
        send_key(&mut app, &agent_tx, KeyCode::Backspace, KeyModifiers::NONE).await;
        assert_eq!(app.active_tab().pending_images.len(), 1);
        assert_eq!(app.selected_image, Some(0));

        // Removing the last image clears the selection.
        send_key(&mut app, &agent_tx, KeyCode::Backspace, KeyModifiers::NONE).await;
        assert!(app.active_tab().pending_images.is_empty());
        assert_eq!(app.selected_image, None);
    }

    #[test]
    fn image_chip_hitboxes_cover_each_chip() {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};
        use unicode_width::UnicodeWidthStr;

        let png = [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        let images: Vec<_> = (0..2)
            .map(|_| {
                zode_core::images::image_attachment_from_bytes(&png, "clipboard image").unwrap()
            })
            .collect();
        let theme = ThemeStore::with_builtins().resolve(None);
        let mut term = Terminal::new(TestBackend::new(80, 3)).unwrap();
        let mut hits = Vec::new();
        term.draw(|f| {
            hits = render_pending_image_chips(f, Rect::new(0, 0, 80, 1), &images, None, &theme);
        })
        .unwrap();

        assert_eq!(hits.len(), 2, "one hitbox per shown chip");
        // First chip begins just after the "▣ " prefix (2 cols).
        assert_eq!(hits[0].0, 2);
        let chip_w = (UnicodeWidthStr::width("clipboard image")
            + UnicodeWidthStr::width(" image/png")) as u16;
        assert_eq!(
            hits[0].1 - hits[0].0,
            chip_w,
            "hitbox spans name + media type"
        );
        // The second chip starts after the first plus the 2-col separator.
        assert_eq!(hits[1].0, hits[0].1 + 2);
        assert_eq!(hits[1].2, 1, "carries the image index");
    }

    #[tokio::test]
    async fn paste_normalizes_cr_and_crlf_into_real_lines() {
        // Bracketed paste delivers newlines as `\r` in several terminals
        // (iTerm2), and Windows-origin text carries `\r\n`; the textarea
        // splits on `\n` only, so unnormalized CRs scrambled the composer.
        let (mut app, _tx) = make_test_app().await;
        app.input.take();
        app.handle_paste("fn main() {\r\n    body\r}\r\ntail");
        assert_eq!(app.input.text(), "fn main() {\n    body\n}\ntail");
        assert!(
            app.input.text().lines().nth(1) == Some("    body"),
            "indentation survives"
        );
    }

    #[tokio::test]
    async fn dragged_image_path_in_input_becomes_a_chip() {
        let (mut app, _tx) = make_test_app().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        std::fs::write(
            &path,
            [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0],
        )
        .unwrap();
        // A dragged path lands in the input as text; absorbing lifts it to a chip.
        app.input.set_text(&path.display().to_string());
        app.absorb_image_paths_from_input();
        assert_eq!(
            app.active_tab().pending_images.len(),
            1,
            "path lifted to a chip"
        );
        assert!(
            app.input.text().trim().is_empty(),
            "path stripped from input"
        );
    }

    #[tokio::test]
    async fn plain_text_mentioning_jpg_does_not_attach() {
        let (mut app, _tx) = make_test_app().await;
        app.input.set_text("see foo.jpg please");
        app.absorb_image_paths_from_input();
        assert!(
            app.active_tab().pending_images.is_empty(),
            "non-existent path ignored"
        );
        assert_eq!(
            app.input.text(),
            "see foo.jpg please",
            "text left untouched"
        );
    }

    #[tokio::test]
    async fn typing_exits_image_chip_selection() {
        let (mut app, agent_tx) = make_test_app().await;
        app.input.take();
        let png = [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        let img = zode_core::images::image_attachment_from_bytes(&png, "clipboard image").unwrap();
        app.active_tab_mut().pending_images.push(img);

        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(app.selected_image, Some(0));
        // A normal character exits selection AND types (image is kept).
        send_key(&mut app, &agent_tx, KeyCode::Char('h'), KeyModifiers::NONE).await;
        assert_eq!(app.selected_image, None);
        assert_eq!(app.input.text(), "h");
        assert_eq!(
            app.active_tab().pending_images.len(),
            1,
            "image not removed by typing"
        );
    }

    #[test]
    fn prompt_history_skips_bare_slash_commands() {
        let mut history = Vec::new();
        // Single-line slash commands are NOT recorded.
        assert!(!record_prompt_history_entry(&mut history, "/sandbox"));
        assert!(!record_prompt_history_entry(&mut history, "/model gpt"));
        assert!(!record_prompt_history_entry(&mut history, "  /help  "));
        assert!(
            history.is_empty(),
            "no slash commands recorded: {history:?}"
        );
        // Real prompts (incl. ones that merely contain a slash) ARE recorded.
        assert!(record_prompt_history_entry(
            &mut history,
            "写个 /tmp/hello.txt"
        ));
        assert!(record_prompt_history_entry(
            &mut history,
            "/note\nmulti-line body"
        ));
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn tui_initialization_loads_local_prompt_history() {
        let source = include_str!("app.rs");
        assert!(
            source.contains("seed_prompt_history_for_tab(&mut tab0)"),
            "TuiApp::new should seed prompt history from the active session"
        );
    }

    #[test]
    fn prompt_history_round_trips_per_session_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");

        save_prompt_history_to_path(&path, "session:a", &["a1".into(), "a2".into()]).unwrap();
        save_prompt_history_to_path(&path, "session:b", &["b1".into()]).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "session:a"),
            vec!["a1".to_string(), "a2".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "session:b"),
            vec!["b1".to_string()]
        );
        // An unknown session starts empty.
        assert!(load_prompt_history_from_path(&path, "session:c").is_empty());
    }

    #[test]
    fn saving_one_session_preserves_other_sessions_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");

        save_prompt_history_to_path(&path, "session:a", &["a1".into()]).unwrap();
        save_prompt_history_to_path(&path, "session:b", &["b1".into()]).unwrap();
        // Overwrite session A — B must be untouched ("不要清空记录").
        save_prompt_history_to_path(&path, "session:a", &["a1".into(), "a2".into()]).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "session:a"),
            vec!["a1".to_string(), "a2".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "session:b"),
            vec!["b1".to_string()]
        );
    }

    #[test]
    fn project_bucket_saves_merge_instead_of_replacing() {
        // Buckets are project-scoped, so two live sessions in one workspace
        // write the same key. A save from a session with a STALE in-memory
        // view must not drop what the other session recorded meanwhile.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");
        let key = "project:/w";

        save_prompt_history_to_path(&path, key, &["a".into()]).unwrap();
        // Session B (seeded with ["a"]) records "b".
        save_prompt_history_to_path(&path, key, &["a".into(), "b".into()]).unwrap();
        // Session A (still on ["a"]) records "c" — "b" must survive.
        save_prompt_history_to_path(&path, key, &["a".into(), "c".into()]).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, key),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[tokio::test]
    async fn prompt_history_key_is_project_scoped() {
        let (app, _tx) = make_test_app().await;
        assert!(
            app.tabs[0].prompt_history_key.starts_with("project:"),
            "recall must be shared across sessions in the same workspace, got {}",
            app.tabs[0].prompt_history_key
        );
    }

    #[test]
    fn legacy_flat_array_migrates_into_current_session_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");
        // Old format: a bare JSON array of prompts.
        fs::write(&path, r#"["old one","old two"]"#).unwrap();

        // Loading from session A pulls the legacy entries into A's bucket.
        assert_eq!(
            load_prompt_history_from_path(&path, "session:a"),
            vec!["old one".to_string(), "old two".to_string()]
        );

        // Saving any session rewrites the file in the new map format while
        // keeping the migrated legacy entries under the session that loaded them.
        save_prompt_history_to_path(&path, "session:a", &["old one".into(), "old two".into()])
            .unwrap();
        save_prompt_history_to_path(&path, "session:b", &["b1".into()]).unwrap();
        assert_eq!(
            load_prompt_history_from_path(&path, "session:a"),
            vec!["old one".to_string(), "old two".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "session:b"),
            vec!["b1".to_string()]
        );
    }

    #[test]
    fn per_session_history_keeps_recent_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");
        let entries: Vec<String> = (0..(PROMPT_HISTORY_LIMIT + 5))
            .map(|i| format!("prompt {i}"))
            .collect();

        save_prompt_history_to_path(&path, "session:a", &entries).unwrap();

        let loaded = load_prompt_history_from_path(&path, "session:a");
        assert_eq!(loaded.len(), PROMPT_HISTORY_LIMIT);
        assert_eq!(loaded.first().map(String::as_str), Some("prompt 5"));
    }

    #[test]
    fn prompt_history_skips_blanks_consecutive_duplicates_and_keeps_recent_limit() {
        let mut history = Vec::new();
        assert!(!record_prompt_history_entry(&mut history, "   "));
        assert!(record_prompt_history_entry(&mut history, "same"));
        assert!(!record_prompt_history_entry(&mut history, "same"));

        for i in 0..(PROMPT_HISTORY_LIMIT + 5) {
            assert!(record_prompt_history_entry(
                &mut history,
                &format!("prompt {i}")
            ));
        }

        assert_eq!(history.len(), PROMPT_HISTORY_LIMIT);
        assert_eq!(history.first().map(String::as_str), Some("prompt 5"));
        assert_eq!(
            history.last().map(String::as_str),
            Some(format!("prompt {}", PROMPT_HISTORY_LIMIT + 4).as_str())
        );
    }

    #[tokio::test]
    async fn ctrl_c_clears_prompt_text_before_quitting_when_idle() {
        let (mut app, agent_tx) = make_test_app().await;
        app.input.set_text("draft prompt");

        send_key(
            &mut app,
            &agent_tx,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )
        .await;

        assert_eq!(app.input.text(), "");
        assert!(!app.should_quit);

        send_key(
            &mut app,
            &agent_tx,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )
        .await;

        assert!(app.should_quit);
    }

    #[test]
    fn base64_encode_matches_rfc4648_vectors() {
        // The OSC 52 clipboard payload must be correct base64, so pin the
        // canonical vectors against the hand-rolled encoder.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[tokio::test]
    async fn resize_clears_active_selection() {
        let (mut app, agent_tx) = make_test_app().await;
        app.active_selection = Some(ChatSelection::new(
            ChatSelectionPoint { line: 0, column: 0 },
            ChatSelectionPoint { line: 2, column: 4 },
        ));
        app.active_input_selection = Some(InputSelection::new(
            crate::ui::input::InputSelectionPoint { row: 0, column: 0 },
            crate::ui::input::InputSelectionPoint { row: 0, column: 1 },
        ));
        app.handle_term(CtEvent::Resize(80, 24), &agent_tx).await;
        assert!(
            app.active_selection.is_none(),
            "a resize must drop the now-stale selection"
        );
        assert!(
            app.active_input_selection.is_none(),
            "a resize must drop the now-stale input selection"
        );
        assert!(
            app.force_redraw,
            "a resize must force a full repaint so stale sidebar cells cannot survive"
        );
    }

    #[tokio::test]
    async fn copy_chord_consumes_and_clears_without_interrupting() {
        let (mut app, agent_tx) = make_test_app().await;
        // A non-empty input selection over the (empty) input box: the copy chord
        // must be consumed (not fall through to the interrupt/quit arm) and must
        // clear the selection so a follow-up Ctrl+C can interrupt. Empty input →
        // no real clipboard write.
        app.active_input_selection = Some(InputSelection::new(
            crate::ui::input::InputSelectionPoint { row: 0, column: 0 },
            crate::ui::input::InputSelectionPoint { row: 0, column: 5 },
        ));
        send_key(
            &mut app,
            &agent_tx,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )
        .await;
        assert!(
            app.active_input_selection.is_none(),
            "copy must clear the selection"
        );
        assert!(!app.should_quit, "copy chord must not quit/interrupt");
    }

    #[tokio::test]
    async fn up_down_edit_queued_messages_while_turn_is_busy() {
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history.clear();
        app.active_tab_mut().turn_abort = Some(AbortController::new());
        app.active_tab_mut().queued_input.push_back("first".into());
        app.active_tab_mut().queued_input.push_back("second".into());

        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(app.input.text(), "second");

        app.input.set_text("second edited");
        send_key(&mut app, &agent_tx, KeyCode::Up, KeyModifiers::NONE).await;
        assert_eq!(app.input.text(), "first");
        assert_eq!(app.active_tab().queued_input[1], "second edited");

        app.input.set_text("first edited");
        send_key(&mut app, &agent_tx, KeyCode::Down, KeyModifiers::NONE).await;
        assert_eq!(app.input.text(), "second edited");
        assert_eq!(app.active_tab().queued_input[0], "first edited");

        app.input.set_text("second final");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;
        assert_eq!(app.input.text(), "");
        assert_eq!(
            app.active_tab()
                .queued_input
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["first edited".to_string(), "second final".to_string()]
        );
    }

    #[tokio::test]
    async fn queued_input_dispatches_as_new_user_turn_after_current_turn_finishes() {
        let (mut app, agent_tx) = make_test_app().await;
        let count_store_user_text = |app: &TuiApp, needle: &str| {
            let store = app.active_tab().engine.store.lock().unwrap();
            store
                .iter()
                .filter(|msg| {
                    matches!(msg, Message::User { content, .. } if content.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text == needle)
                    }))
                })
                .count()
        };

        app.tabs[0].prompt_history.clear();
        app.active_tab_mut().titled = true;
        app.active_tab_mut().turn_seq = 1;
        // Busy but NOT a live agent turn (a reassemble): no abort handle, so
        // the message QUEUES rather than steering into a running loop. (A live
        // turn steers — covered by `submit_during_a_live_turn_steers_it`.)
        app.active_tab_mut().reassemble_pending = true;

        app.input.set_text("queued follow-up");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;

        assert_eq!(app.active_tab().queued_input.len(), 1);
        // A queued follow-up is never recorded into recall/prompt history.
        assert!(!app.tabs[0]
            .prompt_history
            .iter()
            .any(|p| p == "queued follow-up"));
        assert!(!app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|msg| msg.role == Role::User && msg.text == "queued follow-up"));
        assert_eq!(count_store_user_text(&app, "queued follow-up"), 0);

        app.active_tab_mut().reassemble_pending = false;
        app.active_tab_mut().active_turn_id = 0;
        app.dispatch_queued_input(&agent_tx).await;

        assert!(app.active_tab().queued_input.is_empty());
        assert_eq!(app.active_tab().active_turn_id, 2);
        assert!(app.active_tab().is_busy());
        assert!(app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|msg| msg.role == Role::User && msg.text == "queued follow-up"));
        for _ in 0..20 {
            if count_store_user_text(&app, "queued follow-up") == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(count_store_user_text(&app, "queued follow-up"), 1);
    }

    #[tokio::test]
    async fn submit_during_a_live_turn_steers_it() {
        // Typing a follow-up while a LIVE agent turn runs injects it into the
        // running loop (mid-turn steering) instead of queuing it for a later
        // turn — the message is shown in chat and NOT left in the queue.
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history.clear();
        app.active_tab_mut().titled = true;
        app.active_tab_mut().turn_seq = 1;
        app.active_tab_mut().active_turn_id = 1;
        app.active_tab_mut().turn_abort = Some(AbortController::new());

        app.input.set_text("also handle the edge case");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;

        // Steered, not queued.
        assert!(
            app.active_tab().queued_input.is_empty(),
            "a live turn steers the message rather than queuing it"
        );
        // Shown in the transcript so the user sees what they injected.
        assert!(app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|m| m.role == Role::User && m.text == "also handle the edge case"));
        // The live turn keeps running (not superseded).
        assert_eq!(app.active_tab().active_turn_id, 1);
    }

    #[tokio::test]
    async fn image_only_interjection_queues_a_send_instead_of_stranding_chips() {
        // Regression: Enter while busy with image chips attached and no text
        // only toasted "attached N images" — nothing was queued, so when the
        // turn finished nothing sent the images and the chips sat above the
        // composer indefinitely. It must queue a (single) image-only entry;
        // dispatch re-enters submit() on the idle tab, whose turn-start path
        // consumes the pending images exactly like an idle image-only Enter.
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history.clear();
        app.active_tab_mut().titled = true;
        app.input.take();
        let png = [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        let img = zode_core::images::image_attachment_from_bytes(&png, "clipboard image").unwrap();
        app.active_tab_mut().pending_images.push(img);
        // Simulate a live turn (busy, no local op).
        app.active_tab_mut().turn_seq = 1;
        app.active_tab_mut().active_turn_id = 1;
        app.active_tab_mut().turn_abort = Some(AbortController::new());

        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;
        assert_eq!(
            app.active_tab().queued_input.len(),
            1,
            "image-only interjection must queue a send"
        );
        assert!(app.active_tab().queued_input[0].trim().is_empty());
        assert_eq!(
            app.active_tab().pending_images.len(),
            1,
            "chips stay visible while queued — they ride the dispatched turn"
        );

        // A second image-only Enter must not stack another empty entry (one
        // drains all pending images already).
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;
        assert_eq!(app.active_tab().queued_input.len(), 1);

        // A real message still queues behind it.
        app.input.set_text("and a follow-up");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;
        assert_eq!(app.active_tab().queued_input.len(), 2);
    }

    #[tokio::test]
    async fn submit_during_compaction_queues_instead_of_steering() {
        // Regression: a local op (compaction, `!cmd` shell) holds `turn_abort`
        // so Esc can cancel it, but runs no QueryLoop. Steering a message then
        // parks it in the steer buffer unread — the user saw their message in
        // chat, compaction finished, and nothing continued until they typed
        // again (the NEXT turn drained the buffer). It must queue instead, so
        // the post-op `dispatch_queued_input` pass sends it immediately.
        let (mut app, agent_tx) = make_test_app().await;
        app.tabs[0].prompt_history.clear();
        app.active_tab_mut().titled = true;
        let tab_id = app.tabs[0].id;
        let op_id = arm_local_op_for_test(&mut app, 0);
        assert!(app.active_tab().is_busy());

        app.input.set_text("follow-up during compaction");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;

        // Queued — NOT steered into a loop that isn't running, and not yet
        // echoed as a chat user bubble (the queue renders its own preview).
        assert_eq!(
            app.active_tab().queued_input.front().map(String::as_str),
            Some("follow-up during compaction")
        );
        assert!(!app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|m| m.role == Role::User && m.text == "follow-up during compaction"));

        // Compaction finishes → the very next queue drain starts the turn.
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id,
            result: Ok("compacted 2 messages · ~10 → ~5 tokens".into()),
            auto: false,
        });
        assert!(!app.active_tab().is_busy());
        app.dispatch_queued_input(&agent_tx).await;

        assert!(app.active_tab().queued_input.is_empty());
        assert!(app.active_tab().is_busy(), "queued follow-up starts a turn");
        assert!(app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|m| m.role == Role::User && m.text == "follow-up during compaction"));
    }

    #[tokio::test]
    async fn due_loop_job_queues_prompt_once() {
        let (mut app, _tx) = make_test_app().await;
        let owner = app.active_tab().id as u64;
        let now = std::time::Instant::now();
        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            now,
        );
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(
            app.active_tab().queued_input.back().map(String::as_str),
            Some("check ci")
        );
        // A second poll with the job due again but the prompt still queued must not stack.
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(
            app.active_tab()
                .queued_input
                .iter()
                .filter(|q| *q == "check ci")
                .count(),
            1,
            "no duplicate queued prompt"
        );
    }

    #[tokio::test]
    async fn queued_loop_prompt_does_not_consume_max_run_budget() {
        let (mut app, _tx) = make_test_app().await;
        let id = app.scheduler.add_loop(
            app.active_tab().id as u64,
            "check once per execution".into(),
            Duration::from_secs(60),
            Some(2),
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(app.scheduler.loops()[0].runs, 1);

        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(
            app.scheduler.loops()[0].runs,
            1,
            "queued work is not an execution and must not spend --max"
        );
        assert_eq!(app.active_tab().queued_input.len(), 1);
    }

    #[tokio::test]
    async fn loop_slash_command_starts_and_stops_jobs() {
        let (mut app, tx) = make_test_app().await;
        app.submit("/loop 5m check ci", &tx).await;
        assert_eq!(app.scheduler.loops().len(), 1);
        app.submit("/loop stop", &tx).await;
        assert!(app.scheduler.loops().is_empty());
    }

    #[tokio::test]
    async fn due_loop_job_on_background_tab_starts_its_own_turn() {
        // A `/loop` job owned by a tab that is NOT the active one used to
        // queue its prompt into that tab's `queued_input` and then sit
        // there forever: `dispatch_queued_input` only ever drains the
        // ACTIVE tab, so nothing popped it, `poll_scheduler`'s anti-pileup
        // dedup then swallowed every later fire (the prompt looked "still
        // queued"), and `runs`/`max_runs` kept advancing inside
        // `Scheduler::due()` with zero executions — loops only worked on
        // the focused tab. `dispatch_scheduler_queued` drains
        // scheduler-owned prompts off every OTHER idle tab too.
        // `make_test_app` (vs. `_with_dir`) drops its cwd tempdir immediately,
        // which is fine for tests that never assemble another engine — but
        // `new_tab` below needs the cwd to still exist on disk.
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        // `make_test_app_with_dir`'s sender has no live receiver; use our own
        // so the background tab's assembly-done event and turn-start events
        // are actually delivered.
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        assert_eq!(app.tabs.len(), 2);
        // `new_tab` focuses the new tab; put focus back on tab 0 so tab 1 is
        // the background tab under test.
        app.active = 0;
        assert!(!app.tabs[1].is_busy());
        let background_id = app.tabs[1].id as u64;

        let now = std::time::Instant::now();
        let id = app.scheduler.add_loop(
            background_id,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            now,
        );
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(
            app.tabs[1].queued_input.back().map(String::as_str),
            Some("check ci"),
            "poll_scheduler queues onto the OWNING tab, not the active one"
        );
        assert!(
            app.tabs[0].queued_input.is_empty(),
            "another tab's loop must not touch the active tab's queue"
        );

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert!(
            app.tabs[1].queued_input.is_empty(),
            "background tab's scheduler prompt was drained"
        );
        assert!(
            app.tabs[1].is_busy(),
            "background tab's turn actually started"
        );
        assert_eq!(app.tabs[1].active_turn_id, 1);
        // The active tab was never touched — no turn started there.
        assert!(!app.tabs[0].is_busy());
        assert_eq!(app.tabs[0].active_turn_id, 0);
    }

    #[tokio::test]
    async fn due_loop_job_on_active_tab_starts_its_turn_from_a_tick_alone() {
        // The unattended case, and the one the per-task reviews missed: ONE
        // tab, focused, user away from the keyboard. `dispatch_queued_input`
        // — the active tab's only drain before this fix — runs solely from the
        // terminal-input and agent-event arms of the event loop, neither of
        // which fires when nothing happens. So the due prompt sat queued until
        // the next keypress, while `poll_scheduler`'s anti-pileup check
        // swallowed every later fire and `Scheduler::due` kept incrementing
        // `runs` — `--max N` could be consumed with zero executions.
        //
        // This test drives exactly what the tick arm drives, and nothing else:
        // `poll_scheduler()` + `dispatch_scheduler_queued()`.
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        assert_eq!(app.tabs.len(), 1, "single-tab, active-tab scenario");
        let owner = app.tabs[0].id as u64;

        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));

        app.poll_scheduler();
        app.dispatch_scheduler_queued(&agent_tx).await;

        assert!(
            app.tabs[0].queued_input.is_empty(),
            "the tick drained the active tab's scheduler prompt"
        );
        assert!(
            app.tabs[0].is_busy(),
            "the active tab's turn actually started, with no terminal input \
             and no agent event"
        );
        assert_eq!(app.tabs[0].active_turn_id, 1);
        assert_eq!(
            app.tabs[0].active_sched_job,
            Some(SchedJobRef::Loop(id)),
            "the turn is attributed to the loop that queued it"
        );
    }

    #[tokio::test]
    async fn tick_drain_leaves_user_typed_queued_input_alone() {
        // The tick drain must be scheduler-only: a message the USER queued
        // while a turn was running keeps waiting for the normal path. Auto-
        // sending it on a timer would be a new, unasked-for behavior.
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.tabs[0]
            .queued_input
            .push_back("my own follow-up".to_string());

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(
            app.tabs[0].queued_input.front().map(String::as_str),
            Some("my own follow-up"),
            "user-typed queued input is never auto-drained on a schedule"
        );
        assert!(!app.tabs[0].is_busy(), "no turn was started");
    }

    #[tokio::test]
    async fn tick_drain_respects_an_open_queued_edit() {
        // `dispatch_queued_input` refuses to send the front entry while it is
        // mirrored in the prompt editor for the user to edit; the tick drain
        // honors the same guard, so the queued-edit UX is unchanged.
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let owner = app.tabs[0].id as u64;
        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));
        app.poll_scheduler();
        app.queued_edit_index = Some(0);

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(
            app.tabs[0].queued_input.front().map(String::as_str),
            Some("check ci"),
            "the entry being edited stays put"
        );
        assert!(!app.tabs[0].is_busy());
    }

    #[tokio::test]
    async fn stopping_a_loop_purges_its_already_queued_prompt() {
        // `/loop stop` used to leave an already-queued prompt in
        // `queued_input`, so a stopped loop still ran one more time — and its
        // orphaned `sched_pending` entry lived forever, ready to capture a
        // later user-typed message with the same text.
        let (mut app, agent_tx) = make_test_app().await;
        let owner = app.tabs[0].id as u64;
        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, std::time::Duration::from_secs(61));
        app.poll_scheduler();
        assert_eq!(app.tabs[0].queued_input.len(), 1);
        assert_eq!(app.sched_pending.len(), 1);

        app.submit("/loop stop", &agent_tx).await;

        assert!(app.scheduler.loops().is_empty());
        assert!(
            app.tabs[0].queued_input.is_empty(),
            "a stopped loop does not get one more run"
        );
        assert!(
            app.sched_pending.is_empty(),
            "no orphaned attribution entry survives the stop"
        );

        // And the freed text is now just ordinary user input again.
        app.dispatch_scheduler_queued(&agent_tx).await;
        assert!(!app.tabs[0].is_busy());
    }

    #[tokio::test]
    async fn removing_a_schedule_purges_its_already_queued_prompt() {
        let (mut app, agent_tx) = make_test_app().await;
        let job = SchedJobRef::Schedule("ab12".into());
        app.scheduler
            .set_schedules(vec![zode_core::scheduler::ScheduleJob {
                id: "ab12".into(),
                spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 60 },
                prompt: "sync upstream".into(),
                enabled: true,
                last_fired_ms: None,
                watchdog_failures: 0,
                watchdog_last_failure_ms: None,
                watchdog_retry_at_ms: None,
                watchdog_active_since_ms: None,
            }]);
        let key = (app.tabs[0].id, "sync upstream".to_string());
        app.push_sched_pending(key.clone(), job.clone());
        app.tabs[0]
            .queued_input
            .push_back("sync upstream".to_string());
        app.sched_fail_streak.insert(job.clone(), 2);

        app.submit("/schedule rm ab12", &agent_tx).await;

        assert!(
            app.tabs[0].queued_input.is_empty(),
            "a removed schedule does not get one more run"
        );
        assert!(app.sched_pending.is_empty(), "attribution entry purged");
        assert!(
            !app.sched_fail_streak.contains_key(&job),
            "the removed job's failure streak is forgotten too"
        );
    }

    #[tokio::test]
    async fn two_jobs_with_identical_prompt_text_are_attributed_independently() {
        // `sched_pending` used to be keyed on the prompt text alone, so two
        // jobs whose prompts read the same collided: the later insert won and
        // the 3-strikes breaker punished the wrong job. The tab id keeps
        // cross-tab occurrences independent too.
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        app.active = 0;
        assert_eq!(app.tabs.len(), 2);

        // Two loops on two tabs, word-for-word the same prompt.
        let a = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        let b = app.scheduler.add_loop(
            app.tabs[1].id as u64,
            "check ci".into(),
            std::time::Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(a, std::time::Duration::from_secs(61));
        app.scheduler
            .rewind_loop_for_test(b, std::time::Duration::from_secs(61));

        app.poll_scheduler();
        assert_eq!(
            app.sched_pending.len(),
            2,
            "identical prompt text does not collapse two jobs into one entry"
        );

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(app.tabs[0].active_sched_job, Some(SchedJobRef::Loop(a)));
        assert_eq!(app.tabs[1].active_sched_job, Some(SchedJobRef::Loop(b)));
        assert!(app.sched_pending.is_empty(), "both entries consumed");
    }

    #[tokio::test]
    async fn same_tab_same_text_jobs_queue_and_start_fifo() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let owner = app.tabs[0].id as u64;
        let a = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        let b = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(a, Duration::from_secs(61));
        app.scheduler
            .rewind_loop_for_test(b, Duration::from_secs(61));

        app.poll_scheduler();

        let key = (app.tabs[0].id, "check ci".to_string());
        assert_eq!(app.tabs[0].queued_input.len(), 2);
        assert_eq!(
            app.sched_pending.get(&key),
            Some(&VecDeque::from([
                SchedJobRef::Loop(a),
                SchedJobRef::Loop(b)
            ])),
            "one text key retains both occurrence identities"
        );

        app.dispatch_scheduler_queued(&agent_tx).await;

        assert_eq!(app.tabs[0].active_sched_job, Some(SchedJobRef::Loop(a)));
        assert_eq!(app.tabs[0].queued_input.len(), 1);
        assert_eq!(
            app.sched_pending.get(&key).and_then(|jobs| jobs.front()),
            Some(&SchedJobRef::Loop(b)),
            "starting one occurrence consumes only the FIFO head"
        );
    }

    #[tokio::test]
    async fn persisted_schedule_lease_is_held_from_queue_until_turn_start() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let id = "queuelease01";
        zode_core::scheduler::save_schedules(&[zode_core::scheduler::ScheduleJob {
            id: id.into(),
            spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 30 },
            prompt: "sync queue lease".into(),
            enabled: true,
            last_fired_ms: Some(now_ms - 31_000),
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
        }])
        .unwrap();
        let (mut app, _unused_tx, _cwd) = make_test_app_with_dir_using_current_config().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.tabs[0].reassemble_pending = true;

        app.poll_scheduler();

        let queued = zode_core::scheduler::load_schedules().pop().unwrap();
        let active_token = queued
            .watchdog_active_since_ms
            .expect("queue claim persists an active token");
        assert!(app.pending_schedule_leases.contains_key(id));
        assert_eq!(app.tabs[0].queued_input.len(), 1);
        assert!(zode_core::scheduler::try_claim_watchdog_fire(
            id,
            queued.last_fired_ms.unwrap() + 30_000
        )
        .unwrap()
        .is_none());

        app.tabs[0].reassemble_pending = false;
        app.dispatch_scheduler_queued(&agent_tx).await;

        assert!(!app.pending_schedule_leases.contains_key(id));
        assert_eq!(
            app.tabs[0]
                .watchdog_attempt_lease
                .as_ref()
                .map(zode_core::scheduler::ScheduleAttemptLease::active_token_ms),
            Some(active_token),
            "the exact queued lease moves into the running tab"
        );
        if let Some(task) = app.tabs[0].turn_task.take() {
            task.abort();
        }
    }

    #[tokio::test]
    async fn graceful_shutdown_restores_a_claimed_but_unstarted_fire() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let now_ms = current_epoch_ms();
        let anchor_ms = now_ms.saturating_sub(31_000);
        let id = "queueexit001";
        zode_core::scheduler::save_schedules(&[zode_core::scheduler::ScheduleJob {
            id: id.into(),
            spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 30 },
            prompt: "restore on exit".into(),
            enabled: true,
            last_fired_ms: Some(anchor_ms),
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
        }])
        .unwrap();
        let (mut app, _tx) = make_test_app_using_current_config().await;
        app.tabs[0].reassemble_pending = true;

        app.poll_scheduler();
        assert!(app.pending_schedule_leases.contains_key(id));
        assert_ne!(
            zode_core::scheduler::load_schedules()[0].last_fired_ms,
            Some(anchor_ms)
        );

        app.release_all_pending_schedule_leases();

        let restored = zode_core::scheduler::load_schedules().pop().unwrap();
        assert_eq!(restored.last_fired_ms, Some(anchor_ms));
        assert_eq!(restored.watchdog_active_since_ms, None);
        assert!(app.pending_schedule_leases.is_empty());
    }

    #[tokio::test]
    async fn scheduler_preflight_failure_keeps_the_owned_occurrence_queued() {
        let (mut app, _unused_tx) = make_test_app().await;
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "defer before start".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        app.tabs[0].turn_seq = u64::MAX;

        app.dispatch_scheduler_queued(&agent_tx).await;

        let job = SchedJobRef::Loop(id);
        assert_eq!(
            app.tabs[0].queued_input.front().map(String::as_str),
            Some("defer before start")
        );
        assert!(app.sched_job_is_pending(&job));
        assert!(app.sched_queued_at.contains_key(&job));
        assert!(!app.tabs[0].is_busy());
    }

    #[tokio::test]
    async fn queued_start_deadline_enters_bounded_watchdog_retry() {
        let (mut app, _tx) = make_test_app().await;
        let id = app.scheduler.add_loop(
            app.tabs[0].id as u64,
            "queue timeout".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        let job = SchedJobRef::Loop(id);
        let overdue = std::time::Instant::now()
            .checked_sub(app.watchdog.queue_start_timeout() + Duration::from_secs(1))
            .unwrap();
        app.sched_queued_at.insert(job.clone(), overdue);

        app.poll_queued_watchdog();

        assert!(app.tabs[0].queued_input.is_empty());
        assert!(!app.sched_job_is_pending(&job));
        assert!(app.watchdog.job_is_occupied(&job));
        assert!(!app
            .watchdog
            .due_retries(std::time::Instant::now() + Duration::from_secs(60))
            .is_empty());
    }

    #[tokio::test]
    async fn user_text_collision_does_not_consume_a_persisted_fire() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let anchor_ms = now_ms - 31_000;
        let id = "queuecollision01";
        zode_core::scheduler::save_schedules(&[zode_core::scheduler::ScheduleJob {
            id: id.into(),
            spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 30 },
            prompt: "same text".into(),
            enabled: true,
            last_fired_ms: Some(anchor_ms),
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
        }])
        .unwrap();
        let (mut app, _tx) = make_test_app_using_current_config().await;
        app.tabs[0].queued_input.push_back("same text".into());

        app.poll_scheduler();

        assert!(!app.pending_schedule_leases.contains_key(id));
        assert!(!app.sched_job_is_pending(&SchedJobRef::Schedule(id.into())));
        assert_eq!(app.scheduler.schedules()[0].last_fired_ms, Some(anchor_ms));
        assert_eq!(
            zode_core::scheduler::load_schedules()[0].last_fired_ms,
            Some(anchor_ms),
            "an unclaimed occurrence remains due"
        );

        app.tabs[0].queued_input.clear();
        app.poll_scheduler();

        assert!(app.pending_schedule_leases.contains_key(id));
        assert!(app.sched_job_is_pending(&SchedJobRef::Schedule(id.into())));
    }

    #[tokio::test]
    async fn editing_a_persisted_schedule_queue_releases_only_active_ownership() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let id = "queueedit001";
        zode_core::scheduler::save_schedules(&[zode_core::scheduler::ScheduleJob {
            id: id.into(),
            spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 30 },
            prompt: "sync editable lease".into(),
            enabled: true,
            last_fired_ms: Some(now_ms - 31_000),
            watchdog_failures: 2,
            watchdog_last_failure_ms: Some(now_ms - 5_000),
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
        }])
        .unwrap();
        let (mut app, _tx) = make_test_app_using_current_config().await;
        app.tabs[0].reassemble_pending = true;
        app.poll_scheduler();
        assert!(app.pending_schedule_leases.contains_key(id));

        app.queued_edit_index = Some(0);
        app.save_queued_edit_text("manual follow-up".into());

        let persisted = zode_core::scheduler::load_schedules().pop().unwrap();
        assert_eq!(persisted.watchdog_active_since_ms, None);
        assert_eq!(persisted.watchdog_failures, 2);
        assert_eq!(persisted.watchdog_last_failure_ms, Some(now_ms - 5_000));
        assert!(!app.pending_schedule_leases.contains_key(id));
        assert!(!app.sched_job_is_pending(&SchedJobRef::Schedule(id.into())));
        assert_eq!(
            app.tabs[0].queued_input.front().map(String::as_str),
            Some("manual follow-up")
        );
    }

    #[tokio::test]
    async fn persisted_retry_claims_its_lease_before_entering_the_queue() {
        let config = tempfile::tempdir().unwrap();
        let _env_lock = crate::tab::TEST_ENV_LOCK.lock().await;
        let _env = EnvVarGuard::set("ZODE_CONFIG_DIR", config.path());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let id = "queueretry01";
        let retry_at_ms = now_ms.saturating_sub(1);
        zode_core::scheduler::save_schedules(&[zode_core::scheduler::ScheduleJob {
            id: id.into(),
            spec: zode_core::scheduler::ScheduleSpec::Interval { secs: 300 },
            prompt: "retry with lease".into(),
            enabled: true,
            last_fired_ms: Some(now_ms),
            watchdog_failures: 1,
            watchdog_last_failure_ms: Some(now_ms - 5_000),
            watchdog_retry_at_ms: Some(retry_at_ms),
            watchdog_active_since_ms: None,
        }])
        .unwrap();
        let (mut app, _tx) = make_test_app_using_current_config().await;

        app.dispatch_watchdog_retries();

        let persisted = zode_core::scheduler::load_schedules().pop().unwrap();
        assert_eq!(persisted.watchdog_retry_at_ms, None);
        assert!(persisted.watchdog_active_since_ms.is_some());
        assert!(app.pending_schedule_leases.contains_key(id));
        assert!(app.sched_job_is_pending(&SchedJobRef::Schedule(id.into())));
        assert!(
            zode_core::scheduler::try_claim_watchdog_retry(id, retry_at_ms)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn purging_one_same_text_job_preserves_the_other_occurrence() {
        let (mut app, _unused_tx) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let key = (tab_id, "check ci".to_string());
        let a = SchedJobRef::Loop(10);
        let b = SchedJobRef::Loop(11);
        app.push_sched_pending(key.clone(), a.clone());
        app.push_sched_pending(key.clone(), b.clone());
        app.tabs[0].queued_input.push_back("check ci".into());
        app.tabs[0].queued_input.push_back("check ci".into());

        app.purge_sched_jobs(|job| job == &a);

        assert_eq!(
            app.tabs[0].queued_input,
            VecDeque::from(["check ci".to_string()]),
            "only the purged job's concrete prompt occurrence is removed"
        );
        assert_eq!(
            app.sched_pending.get(&key),
            Some(&VecDeque::from([b])),
            "the equal-text sibling keeps its attribution"
        );
    }

    #[tokio::test]
    async fn editing_a_queued_scheduler_prompt_detaches_and_unblocks_its_job() {
        let (mut app, _unused_tx) = make_test_app().await;
        let owner = app.tabs[0].id as u64;
        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert!(app.sched_job_is_pending(&SchedJobRef::Loop(id)));

        app.queued_edit_index = Some(0);
        app.save_queued_edit_text("manual follow-up".into());

        assert_eq!(
            app.tabs[0].queued_input.front().map(String::as_str),
            Some("manual follow-up")
        );
        assert!(
            !app.sched_job_is_pending(&SchedJobRef::Loop(id)),
            "edited text is ordinary user input, not scheduler-owned"
        );

        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert!(
            app.sched_job_is_pending(&SchedJobRef::Loop(id)),
            "detaching the edited occurrence releases identity-based pileup"
        );
    }

    #[tokio::test]
    async fn deleting_a_queued_scheduler_prompt_detaches_and_unblocks_its_job() {
        let (mut app, _unused_tx) = make_test_app().await;
        let owner = app.tabs[0].id as u64;
        let id = app.scheduler.add_loop(
            owner,
            "check ci".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();

        app.queued_edit_index = Some(0);
        app.save_queued_edit_text(String::new());

        assert!(app.tabs[0].queued_input.is_empty());
        assert!(!app.sched_job_is_pending(&SchedJobRef::Loop(id)));

        app.scheduler
            .rewind_loop_for_test(id, Duration::from_secs(61));
        app.poll_scheduler();
        assert!(
            app.sched_job_is_pending(&SchedJobRef::Loop(id)),
            "deleting the queued occurrence releases identity-based pileup"
        );
    }

    #[tokio::test]
    async fn closing_a_tab_purges_its_pending_scheduler_entries() {
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        assert_eq!(app.tabs.len(), 2);
        // `new_tab` focused tab 1; queue a scheduler prompt on it, then close
        // it without ever draining.
        let closing_id = app.tabs[app.active].id;
        app.push_sched_pending((closing_id, "check ci".to_string()), SchedJobRef::Loop(7));

        app.close_active_tab_with_events(&agent_tx);

        assert_eq!(app.tabs.len(), 1);
        assert!(
            app.sched_pending.is_empty(),
            "a closed tab's pending entries must not outlive it and capture \
             an identically-worded prompt later"
        );
    }

    /// A `/loop` whose owning tab is closed must be retired with the tab.
    /// Otherwise `due()` keeps returning it forever — `poll_scheduler` drops
    /// each fire for want of an owning tab, but `runs` has already been
    /// incremented, so a `--max N` loop burns its whole budget with zero
    /// executions while still showing up in `/loop list`.
    #[tokio::test]
    async fn closing_a_tab_stops_the_loops_it_owned() {
        let (mut app, _unused_tx, _dir) = make_test_app_with_dir().await;
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        app.new_tab(&agent_tx);
        let ev = tokio::time::timeout(Duration::from_secs(30), agent_rx.recv())
            .await
            .expect("assembly finishes")
            .expect("channel open");
        app.handle_agent_event(ev);
        assert_eq!(app.tabs.len(), 2);

        let closing_id = app.tabs[app.active].id;
        let surviving_id = app.tabs[1 - app.active].id;
        let doomed = app.scheduler.add_loop(
            closing_id as u64,
            "check ci".into(),
            Duration::from_secs(60),
            Some(5),
            std::time::Instant::now(),
        );
        let kept = app.scheduler.add_loop(
            surviving_id as u64,
            "watch deploys".into(),
            Duration::from_secs(60),
            None,
            std::time::Instant::now(),
        );
        app.sched_fail_streak.insert(SchedJobRef::Loop(doomed), 2);

        app.close_active_tab_with_events(&agent_tx);

        let ids: Vec<u32> = app.scheduler.loops().iter().map(|j| j.id).collect();
        assert_eq!(
            ids,
            vec![kept],
            "the closed tab's loop is retired; the other tab's is untouched"
        );
        assert!(
            !app.sched_fail_streak
                .contains_key(&SchedJobRef::Loop(doomed)),
            "the retired loop's failure streak is forgotten too"
        );
        // And it genuinely stops coming out of `due()` even once overdue.
        app.scheduler
            .rewind_loop_for_test(doomed, Duration::from_secs(120));
        let due = app.scheduler.due(
            std::time::Instant::now(),
            chrono::Local::now().naive_local(),
        );
        assert!(
            !due.iter()
                .any(|j| matches!(&j.kind, DueKind::Loop { id, .. } if *id == doomed)),
            "a retired loop never fires again"
        );
    }

    #[test]
    fn interval_token_renders_compact_round_trippable_forms() {
        assert_eq!(interval_token(7200), "2h");
        assert_eq!(interval_token(300), "5m");
        assert_eq!(interval_token(90), "90s");
    }

    #[test]
    fn describe_schedule_spec_interval_round_trips_through_schedule_add() {
        use zode_core::commands::loop_sched::{parse_schedule, ScheduleCommand};
        use zode_core::scheduler::ScheduleSpec;
        let spec = ScheduleSpec::Interval { secs: 7200 };
        let rendered = describe_schedule_spec(&spec);
        assert_eq!(rendered, "every 2h");
        let cmd = parse_schedule(&format!("/schedule add {rendered} check ci"))
            .expect("describe_schedule_spec's output must re-parse");
        assert_eq!(
            cmd,
            ScheduleCommand::Add {
                spec: ScheduleSpec::Interval { secs: 7200 },
                prompt: "check ci".to_string(),
            }
        );
    }

    #[test]
    fn dragging_selection_to_chat_edges_scrolls_one_line() {
        let chat = Rect::new(0, 1, 80, 20);

        assert_eq!(
            selection_scroll_from_drag(
                crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
                10,
                1,
                chat,
            ),
            Some(ChatMouseScroll::Up(1))
        );
        assert_eq!(
            selection_scroll_from_drag(
                crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
                10,
                20,
                chat,
            ),
            Some(ChatMouseScroll::Down(1))
        );
        assert_eq!(
            selection_scroll_from_drag(
                crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
                10,
                8,
                chat,
            ),
            None
        );
    }

    #[test]
    fn image_submit_route_prefers_direct_in_auto_when_supported() {
        assert_eq!(
            resolve_image_submit_route(true, ImageMode::Auto, true, true),
            ImageSubmitRoute::Direct
        );
    }

    #[test]
    fn image_submit_route_uses_vision_provider_when_auto_needs_fallback() {
        assert_eq!(
            resolve_image_submit_route(true, ImageMode::Auto, false, true),
            ImageSubmitRoute::VisionModel
        );
    }

    #[test]
    fn image_submit_route_blocks_direct_mode_without_image_support() {
        assert_eq!(
            resolve_image_submit_route(true, ImageMode::Direct, false, true),
            ImageSubmitRoute::Unsupported
        );
    }

    #[test]
    fn mouse_wheel_scrolls_only_the_chat_area() {
        let chat = Rect::new(0, 1, 80, 20);

        assert_eq!(
            chat_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollUp, 10, 5, chat),
            Some(ChatMouseScroll::Up(1))
        );
        assert_eq!(
            chat_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollDown, 10, 5, chat),
            Some(ChatMouseScroll::Down(1))
        );
        assert_eq!(
            chat_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollDown, 10, 25, chat),
            None
        );
        assert_eq!(
            chat_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollUp, 85, 5, chat),
            None
        );
    }

    #[test]
    fn mouse_wheel_scrolls_session_picker_one_row() {
        assert_eq!(
            session_picker_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollUp),
            Some(SessionPickerMouseScroll::Up(1))
        );
        assert_eq!(
            session_picker_scroll_from_mouse(crossterm::event::MouseEventKind::ScrollDown),
            Some(SessionPickerMouseScroll::Down(1))
        );
        assert_eq!(
            session_picker_scroll_from_mouse(crossterm::event::MouseEventKind::Moved),
            None
        );
    }

    #[test]
    fn parsed_arrow_keys_do_not_steal_prompt_history() {
        assert_eq!(
            chat_scroll_from_alt_scroll_key(KeyCode::Up, KeyModifiers::NONE, true),
            None
        );
        assert_eq!(
            chat_scroll_from_alt_scroll_key(KeyCode::Down, KeyModifiers::NONE, true),
            None
        );
        assert_eq!(
            chat_scroll_from_alt_scroll_key(KeyCode::Up, KeyModifiers::NONE, false),
            None
        );
        assert_eq!(
            chat_scroll_from_alt_scroll_key(KeyCode::Up, KeyModifiers::CONTROL, true),
            None
        );
    }

    #[test]
    fn fragmented_application_cursor_sequence_scrolls_instead_of_inserting_ob() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(&mut state, KeyCode::Esc, KeyModifiers::NONE, true),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(state, Some(FragmentedCursorSeqState::AfterEsc));
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(state, Some(FragmentedCursorSeqState::AfterEscO));
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('B'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Scroll(ChatMouseScroll::Down(1))
        );
        assert_eq!(state, None);
    }

    #[test]
    fn fragmented_bracket_cursor_sequence_scrolls_instead_of_inserting_text() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(&mut state, KeyCode::Esc, KeyModifiers::NONE, true),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('['),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('A'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Scroll(ChatMouseScroll::Up(1))
        );
        assert_eq!(state, None);
    }

    #[test]
    fn bare_application_cursor_sequence_scrolls_instead_of_inserting_oa() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            state,
            Some(FragmentedCursorSeqState::MaybeBareO { count: 1 })
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('A'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Scroll(ChatMouseScroll::Up(1))
        );
        assert_eq!(state, None);
    }

    #[test]
    fn repeated_bare_o_waits_for_scroll_final_instead_of_replaying_into_input() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('B'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Scroll(ChatMouseScroll::Down(1))
        );
        assert_eq!(state, None);
    }

    #[test]
    fn pending_bare_o_followed_by_parsed_arrow_scrolls_without_replay() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(&mut state, KeyCode::Down, KeyModifiers::NONE, true),
            FragmentedCursorAction::Scroll(ChatMouseScroll::Down(1))
        );
        assert_eq!(state, None);
    }

    #[test]
    fn fragmented_cursor_sequence_does_not_consume_plain_text() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                false,
            ),
            FragmentedCursorAction::None
        );
        assert_eq!(state, None);
    }

    #[test]
    fn bare_o_is_replayed_when_the_next_key_is_normal_text() {
        let mut state = None;

        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('O'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('p'),
                KeyModifiers::NONE,
                true,
            ),
            FragmentedCursorAction::ReplayBareO(1)
        );
        assert_eq!(state, None);
    }

    /// Feed each char of `s` through the reassembler and collect the actions.
    fn feed_seq(
        state: &mut Option<FragmentedCursorSeqState>,
        s: &str,
    ) -> Vec<FragmentedCursorAction> {
        s.chars()
            .map(|c| {
                fragmented_cursor_sequence_action(state, KeyCode::Char(c), KeyModifiers::NONE, true)
            })
            .collect()
    }

    #[test]
    fn fragmented_bare_sgr_mouse_report_is_swallowed_not_typed() {
        // A wheel report whose ESC[ was lost to fragmentation arrives as the
        // bare chars `<64;48;27M`. Every char must be consumed so none of it
        // leaks into the input box.
        let mut state = None;
        let actions = feed_seq(&mut state, "<64;48;27M");
        assert!(
            actions
                .iter()
                .all(|a| *a == FragmentedCursorAction::Consumed),
            "expected all consumed, got {actions:?}"
        );
        assert_eq!(state, Some(FragmentedCursorSeqState::AfterSgrMouse));
    }

    #[test]
    fn fragmented_esc_bracket_sgr_mouse_report_is_swallowed() {
        // The `ESC [ < ... M` shape (ESC and `[` arrive as their own keys).
        let mut state = None;
        assert_eq!(
            fragmented_cursor_sequence_action(&mut state, KeyCode::Esc, KeyModifiers::NONE, true),
            FragmentedCursorAction::Consumed
        );
        let actions = feed_seq(&mut state, "[<65;1;1M");
        assert!(
            actions
                .iter()
                .all(|a| *a == FragmentedCursorAction::Consumed),
            "expected all consumed, got {actions:?}"
        );
    }

    #[test]
    fn back_to_back_sgr_reports_swallow_the_stray_bracket() {
        // A momentum flood delivers `<65;105;38M[<64;48;27M...`; the `[` between
        // reports (next report's lost ESC) must also be swallowed, not typed.
        let mut state = None;
        let actions = feed_seq(&mut state, "<65;105;38M[<64;48;27M");
        assert!(
            actions
                .iter()
                .all(|a| *a == FragmentedCursorAction::Consumed),
            "expected all consumed, got {actions:?}"
        );
    }

    #[test]
    fn lone_less_than_then_text_is_replayed_into_input() {
        // A `<` that is NOT a mouse report (e.g. typing "x < y") must be given
        // back so real input is never eaten.
        let mut state = None;
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('<'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char(' '),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::ReplaySgr("<".to_string())
        );
        assert_eq!(state, None);
    }

    #[test]
    fn bracket_typed_in_idle_state_stays_plain_text() {
        // Outside a mouse-report context a `[` is ordinary input and must pass
        // straight through (no buffering lag for everyday typing).
        let mut state = None;
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('['),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::None
        );
        assert_eq!(state, None);
    }

    #[test]
    fn premature_terminator_replays_instead_of_eating_text() {
        // Typing `Vec<M>` must not vanish: `<M` is not a well-formed report.
        let mut state = None;
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('<'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('M'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::ReplaySgr("<".to_string())
        );
        assert_eq!(state, None);
    }

    #[test]
    fn incomplete_field_count_replays_buffer() {
        // `<1;2M` has only two fields — not a report; give the bytes back.
        let mut state = None;
        let consumed = feed_seq(&mut state, "<1;2");
        assert!(consumed
            .iter()
            .all(|a| *a == FragmentedCursorAction::Consumed));
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('M'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::ReplaySgr("<1;2".to_string())
        );
    }

    #[test]
    fn bracket_then_text_after_scroll_keeps_the_bracket() {
        // `[<x` typed right after a swallowed report must replay `[<`, not `<`.
        let mut state = Some(FragmentedCursorSeqState::AfterSgrMouse);
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('['),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('<'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::Consumed
        );
        assert_eq!(
            fragmented_cursor_sequence_action(
                &mut state,
                KeyCode::Char('x'),
                KeyModifiers::NONE,
                true
            ),
            FragmentedCursorAction::ReplaySgr("[<".to_string())
        );
    }

    #[test]
    fn overshooting_sgr_buffer_bails_bounded_without_duplicating_a_char() {
        // A pathological digit run must bail (never swallow) and the replayed
        // buffer must stay capped and exclude the char that triggered the bail.
        let mut state = None;
        let long: String = std::iter::once('<')
            .chain(std::iter::repeat_n('9', 40))
            .collect();
        let actions = feed_seq(&mut state, &long);
        let replay = actions
            .iter()
            .find_map(|a| match a {
                FragmentedCursorAction::ReplaySgr(s) => Some(s.clone()),
                _ => None,
            })
            .expect("the over-long run should ReplaySgr");
        assert_eq!(replay.len(), 32);
    }

    #[test]
    fn setup_gates_mouse_capture_without_scroll_key_emulation() {
        let source = include_str!("app.rs");
        let setup = source
            .split("fn setup_terminal")
            .nth(1)
            .and_then(|tail| tail.split("fn restore_terminal").next())
            .expect("setup_terminal source block should exist");
        let restore = source
            .split("fn restore_terminal")
            .nth(1)
            .and_then(|tail| tail.split("fn install_panic_hook").next())
            .expect("restore_terminal source block should exist");
        let alternate_scroll_mode = 1000 + 7;
        assert!(!setup.contains(&format!("?{alternate_scroll_mode}h")));
        assert!(!setup.contains(&format!("?{alternate_scroll_mode}l")));
        assert!(!setup.contains(concat!("Alternate", "Scroll")));
        assert!(!restore.contains(concat!("Alternate", "Scroll")));
        // Capture is opt-in per config (off by default on macOS so the
        // terminal keeps native selection and ⌘C copies it).
        assert!(setup.contains("(mouse_capture: bool)"));
        assert!(setup.contains(concat!("Enable", "Mouse", "Capture")));
        assert!(source.contains(concat!("Disable", "Mouse", "Capture")));
    }

    #[test]
    fn frame_paints_inside_synchronized_update() {
        let source = include_str!("app.rs");
        let event_loop = source
            .split("async fn event_loop")
            .nth(1)
            .and_then(|tail| tail.split("fn print_resume_hint").next().or(Some(tail)))
            .expect("event_loop source block should exist");
        // The full-repaint clear and the draw must be bracketed by a
        // synchronized update (CSI ?2026): the erased screen between
        // `terminal.clear()` and the repaint is otherwise visible as a
        // full-screen flash (e.g. every time a toast expired).
        assert!(event_loop.contains(concat!("Begin", "SynchronizedUpdate")));
        assert!(event_loop.contains(concat!("End", "SynchronizedUpdate")));
        let begin = event_loop
            .find(concat!("Begin", "SynchronizedUpdate"))
            .unwrap();
        let clear = event_loop
            .find("terminal.clear()")
            .expect("full-repaint clear should exist");
        let draw = event_loop
            .find("terminal.draw(")
            .expect("draw call should exist");
        let end = event_loop
            .find(concat!("End", "SynchronizedUpdate"))
            .unwrap();
        assert!(begin < clear && clear < draw && draw < end);
        // Defensive unlock on teardown: a crash between Begin/End must not
        // leave the terminal holding a frozen frame.
        let restore = source
            .split("fn restore_terminal")
            .nth(1)
            .and_then(|tail| tail.split("fn install_panic_hook").next())
            .expect("restore_terminal source block should exist");
        assert!(restore.contains(concat!("End", "SynchronizedUpdate")));
    }

    #[test]
    fn toast_expiry_does_not_trigger_full_clear() {
        let source = include_str!("app.rs");
        let overlay_fn = source
            .split("fn any_overlay_open")
            .nth(1)
            .and_then(|tail| tail.split("\n    }\n").next())
            .expect("any_overlay_open source block should exist");
        // Toasts are drawn from the ratatui buffer, so the normal diff
        // restores the cells under them when they expire. Counting them as
        // overlays made every toast expiry issue a terminal.clear() — a
        // visible full-screen flash a few seconds after every copy.
        assert!(!overlay_fn.contains("self.toast.is_some()"));
    }

    #[test]
    fn picker_delete_requires_confirmation_and_esc_cancels_first() {
        let source = include_str!("app.rs");
        let picker_fn = source
            .split("async fn handle_picker_key")
            .nth(1)
            .and_then(|tail| tail.split("\n    fn ").next())
            .expect("handle_picker_key source block should exist");
        // Delete must go through the picker's two-press confirmation, never
        // straight to delete_session on the first press.
        assert!(picker_fn.contains("press_delete"));
        // Esc cancels a pending delete before it closes the picker.
        assert!(picker_fn.contains("cancel_pending_delete"));
    }

    #[test]
    fn app_managed_selection_follows_mouse_capture() {
        let source = include_str!("app.rs");
        let init = source
            .split("pub fn new(")
            .nth(1)
            .and_then(|tail| tail.split("input: InputBox::new()").next())
            .expect("TuiApp::new initialization block should exist");
        // In-app selection only exists while we hold mouse capture; with
        // capture off the terminal's native selection (+ ⌘C) takes over.
        assert!(init.contains("selection_mode: mouse_capture"));
    }

    #[test]
    fn resumed_conversation_renders_visibly() {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};

        // Mirror a real resumed session: many turns, each with thinking + a
        // tool call + a long markdown answer (code fence, box-drawing, CJK) —
        // the shape that triggered "recovered but didn't render correctly".
        let mut store = MessageStore::new();
        // Long session: 80 turns, each a multi-paragraph markdown answer, so
        // the total wrapped-row count is in the thousands ("内容一长").
        let long_body: String = (0..25)
            .map(|n| format!("- 记忆条目 {n}：append-only 写入，定期压缩、生成 snapshot 快照\n"))
            .collect();
        for i in 0..80 {
            store
                .push(Message::User {
                    header: agent::message::Header::new(),
                    content: vec![ContentBlock::Text {
                        text: format!("问题 {i}：设计一下记忆系统"),
                    }],
                })
                .unwrap();
            store
                .push(Message::Assistant {
                    header: agent::message::Header::new(),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: format!("Turn {i}: the user wants a memory design; weigh options at length so the thinking block itself wraps over several rows of the terminal."),
                            signature: None,
                        },
                        ContentBlock::Text {
                            text: format!(
                                "### 方案 {i}\n\n```\n~/.zode/memory/\n├── global.jsonl\n└── projects/<hash>/\n```\n\n{long_body}\nTAILMARK{i}END 你倾向哪个？"
                            ),
                        },
                    ],
                })
                .unwrap();
        }

        let chat = rebuild_chat_from_store(&store);
        assert!(chat.messages().len() > 10, "rebuild produced messages");

        let theme = ThemeStore::with_builtins().resolve(None);
        let backend = TestBackend::new(120, 30);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "m",
            cwd: std::path::Path::new("/tmp/zode"),
        };
        let mut chat = chat;
        // Render twice — the first frame seeds last_render_total_rows, the
        // second exercises the growth-compensation path on a long history.
        term.draw(|f| chat.render(f, f.area(), &theme, meta))
            .unwrap();
        term.draw(|f| chat.render(f, f.area(), &theme, meta))
            .unwrap();
        let content: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        let non_space = content.chars().filter(|c| !c.is_whitespace()).count();
        assert!(non_space > 0, "resumed conversation rendered BLANK");
        assert!(
            content.contains("TAILMARK79END"),
            "tail of resumed conversation must be visible; got:\n{content}"
        );
    }

    #[test]
    fn strip_recalled_memory_removes_injected_pack() {
        // Reproduce exactly what noema's `MemoryPack::to_markdown()` +
        // `inject_noema_memory` persist: the pack, a blank-line separator, then
        // the user's own text. If either format drifts, this test breaks.
        let pack = "## Relevant Memories\n\
            - [user/preference][mem_1] 王小明 prefers dark themes\n\
            \n## Subconscious Hints\n\
            - cue: noema -> memory\n";
        let user = "帮我看下这个 bug";
        let stored = format!("{pack}\n\n{user}");
        assert_eq!(strip_recalled_memory(&stored), user);
    }

    #[test]
    fn strip_recalled_memory_handles_empty_sections() {
        // Pack with zero memories AND zero hints (headers only).
        let stored = "## Relevant Memories\n\n## Subconscious Hints\n\n\nhello";
        assert_eq!(strip_recalled_memory(stored), "hello");
    }

    #[test]
    fn strip_recalled_memory_leaves_ordinary_text_untouched() {
        assert_eq!(
            strip_recalled_memory("just a normal message"),
            "just a normal message"
        );
        // A message that merely mentions the header mid-body is not a pack.
        let msg = "see the ## Relevant Memories section below";
        assert_eq!(strip_recalled_memory(msg), msg);
        // Malformed (head but no hints header) → returned unchanged, never eats text.
        let malformed = "## Relevant Memories\n- orphan\n\nbody";
        assert_eq!(strip_recalled_memory(malformed), malformed);
    }

    #[test]
    fn stored_user_display_hides_only_the_injected_side_panel_context() {
        assert!(
            stored_user_text_for_display(extension_tasks::SIDE_PANEL_BROWSER_CONTEXT).is_empty()
        );
        let ordinary = "用户明确提到了 <browser_side_panel_context> 标签";
        assert_eq!(stored_user_text_for_display(ordinary), ordinary);
    }

    /// Diagnostic (not run in CI): load a REAL session file and render it the
    /// way resume does, dumping the buffer so we can see any garble/blank with
    /// the user's actual content. Run with:
    ///   ZODE_DIAG_SESSION=~/.zode/sessions/<id>.jsonl \
    ///     cargo test -p zode-tui diag_render_real_session -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn diag_render_real_session() {
        use crate::theme::ThemeStore;
        use ratatui::{backend::TestBackend, Terminal};

        let path = std::env::var("ZODE_DIAG_SESSION").expect("set ZODE_DIAG_SESSION");
        let path = shellexpand_tilde(&path);
        let store = agent::session::Session::load(&path)
            .await
            .expect("load session");
        eprintln!("loaded {} messages", store.iter().count());
        let mut chat = rebuild_chat_from_store(&store);
        eprintln!("rebuilt {} chat rows", chat.messages().len());
        for (i, m) in chat.messages().iter().enumerate() {
            let preview: String = m.text.chars().take(46).collect();
            eprintln!("MSG[{i:02}] {:?} | {}", m.role, preview.replace('\n', "⏎"));
        }

        let theme = ThemeStore::with_builtins().resolve(None);
        let (w, h) = (150u16, 40u16); // a realistic terminal
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        let meta = ChatRenderMeta {
            theme_name: &theme.name,
            model: "m",
            cwd: std::path::Path::new("/tmp/zode"),
        };
        term.draw(|f| chat.render(f, f.area(), &theme, meta))
            .unwrap();
        let buf = term.backend().buffer().clone();
        for y in 0..h {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            eprintln!("{y:02}|{}", row.trim_end());
        }
    }

    fn shellexpand_tilde(p: &str) -> std::path::PathBuf {
        if let Some(rest) = p.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return std::path::PathBuf::from(home).join(rest);
            }
        }
        std::path::PathBuf::from(p)
    }

    #[test]
    fn rebuild_chat_preserves_thinking_content() {
        let mut store = MessageStore::new();
        store
            .push(Message::Assistant {
                header: agent::message::Header::new(),
                content: vec![
                    // Models emit thinking BEFORE the answer; rebuild preserves
                    // the block order chronologically (not reordered).
                    ContentBlock::Thinking {
                        thinking: "The user asked for a file.".into(),
                        signature: None,
                    },
                    ContentBlock::Text {
                        text: "I wrote hello.rs.".into(),
                    },
                ],
            })
            .unwrap();

        let chat = rebuild_chat_from_store(&store);

        assert_eq!(chat.messages().len(), 2);
        assert_eq!(
            chat.messages()[0].text,
            "Thinking: The user asked for a file."
        );
        assert_eq!(chat.messages()[1].text, "I wrote hello.rs.");
    }

    #[test]
    fn process_line_formats_tool_use_sources() {
        let tool = Event::ToolUse {
            id: "t1".into(),
            name: "FileRead".into(),
            input: serde_json::json!({"path": "src/main.rs"}),
        };
        assert_eq!(
            process_line_for_event(&tool, None).as_deref(),
            Some("Tool FileRead src/main.rs")
        );

        let skill = Event::ToolUse {
            id: "t2".into(),
            name: "Skill".into(),
            input: serde_json::json!({"name": "code-review"}),
        };
        assert_eq!(
            process_line_for_event(&skill, None).as_deref(),
            Some("Skill code-review")
        );

        let mcp = Event::ToolUse {
            id: "t3".into(),
            name: "mcp__github__create_issue".into(),
            input: serde_json::json!({"title": "bug"}),
        };
        assert_eq!(
            process_line_for_event(&mcp, None).as_deref(),
            Some("MCP github.create_issue title=bug")
        );
    }

    #[test]
    fn process_line_formats_runtime_events() {
        let result = Event::ToolResult {
            id: "t1".into(),
            ok: true,
            output: serde_json::json!({"status": "ok"}),
        };
        assert_eq!(
            process_line_for_event(&result, Some("FileRead")).as_deref(),
            Some("Tool FileRead done")
        );

        let thinking = Event::Thinking {
            delta: "hidden reasoning".into(),
        };
        assert_eq!(
            process_line_for_event(&thinking, None).as_deref(),
            Some("Thinking: hidden reasoning")
        );

        let notice = Event::Notice {
            code: "retry".into(),
            message: "provider retry".into(),
        };
        assert_eq!(
            process_line_for_event(&notice, None).as_deref(),
            Some("Notice retry: provider retry")
        );

        let usage = Event::Usage {
            input_tokens: 10,
            output_tokens: 3,
            cache_read: 2,
            cache_create: 1,
        };
        assert_eq!(
            process_line_for_event(&usage, None).as_deref(),
            // total = input(10)+read(2)+create(1) = 13; hit = 2*100/13 = 15%.
            Some("Usage ↑10 ↓3 · cache 15% (2)")
        );

        let end_turn = Event::Result {
            data: agent::stream::ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("deepseek-v4-pro".into()),
                ..Default::default()
            },
        };
        assert_eq!(process_line_for_event(&end_turn, None), None);

        let tool_use_result = Event::Result {
            data: agent::stream::ResultData {
                stop_reason: Some("tool_use".into()),
                model: Some("deepseek-v4-pro".into()),
                ..Default::default()
            },
        };
        assert_eq!(
            process_line_for_event(&tool_use_result, None).as_deref(),
            Some("Result tool_use · deepseek-v4-pro")
        );
    }

    #[tokio::test]
    async fn simultaneously_ready_extension_agent_and_tick_sources_are_unbiased() {
        let source = include_str!("app.rs");
        let production = source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .expect("production source precedes tests");
        assert!(
            !production.contains(&["biased", ";"].concat()),
            "the production event loop must use Tokio's fair select"
        );
        let mut selected = [0usize; 3];
        for _ in 0..300 {
            tokio::select! {
                _ = std::future::ready(()) => selected[0] += 1,
                _ = std::future::ready(()) => selected[1] += 1,
                _ = std::future::ready(()) => selected[2] += 1,
            }
        }
        assert!(
            selected.into_iter().all(|count| count > 0),
            "all simultaneously-ready sources must be serviced: {selected:?}"
        );
    }
}
