//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::io::Stdout;
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
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
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
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::{EngineTemplate, ZodeEngine};

use crate::event::{AppEvent, ReassembleEffect, ReassembleNotify, ReassembledEngine};
use crate::tab::SessionTab;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::{Autocomplete, DynCmd};
use crate::ui::chat::{ChatRenderMeta, ChatSelection, ChatSelectionPoint, ChatView, ImagePreview};
use crate::ui::dialog::agents_dialog::{AgentKind, AgentRow, AgentsAction, AgentsDialog};
use crate::ui::dialog::connect::{ConnectAction, ConnectDialog, ConnectField, ConnectStage};
use crate::ui::dialog::mcp_dialog::McpDialog;
use crate::ui::dialog::permission::PermissionDialog;
use crate::ui::dialog::plugin_picker::PluginPicker;
use crate::ui::dialog::question::QuestionDialog;
use crate::ui::dialog::session_picker::SessionPicker;
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

const PROMPT_HISTORY_FILE: &str = "prompt_history.json";
/// Cap on persisted prompt-history entries PER PROJECT. When exceeded, the
/// OLDEST are dropped first (FIFO) — see `record_prompt_history_entry`.
const PROMPT_HISTORY_LIMIT: usize = 100;

/// First turn of the autonomous goal loop, queued when a goal is set.
const GOAL_LOOP_START_PROMPT: &str =
    "Begin working toward the goal now. Take concrete steps. When it is fully \
     complete, call the goal_complete tool with a short summary.";
/// Continuation turn, queued after each successful loop turn that did not signal
/// completion.
const GOAL_LOOP_CONTINUE_PROMPT: &str =
    "Continue working toward the goal. Take the next concrete step now. When it \
     is fully complete, call the goal_complete tool with a short summary — do \
     not call it prematurely.";

fn prompt_history_path() -> Option<std::path::PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|dir| dir.join(PROMPT_HISTORY_FILE))
}

/// Stable key for a project's history bucket: the canonical cwd path (falling
/// back to the raw path string if canonicalization fails, e.g. the dir was
/// removed). The same cwd notion already keys `.zode/state.json`.
fn prompt_history_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn load_prompt_history(project_key: &str) -> Vec<String> {
    prompt_history_path()
        .as_deref()
        .map(|path| load_prompt_history_from_path(path, project_key))
        .unwrap_or_default()
}

fn save_prompt_history(project_key: &str, history: &[String]) {
    let Some(path) = prompt_history_path() else {
        return;
    };
    if let Err(e) = save_prompt_history_to_path(&path, project_key, history) {
        tracing::warn!(error = %e, path = %path.display(), "failed to save prompt history");
    }
}

/// Read the whole project-keyed history map from disk. A legacy flat-array
/// file (`["a","b"]`) is migrated under `project_key` so existing records are
/// never lost. Missing/corrupt files yield an empty map (logged), so a bad
/// file never wipes the user's history on the next save.
fn load_history_map(path: &Path, project_key: &str) -> BTreeMap<String, Vec<String>> {
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
                    map.insert(project_key.to_string(), entries);
                }
            }
        }
        _ => {
            tracing::warn!(path = %path.display(), "prompt history had unexpected JSON type");
        }
    }
    map
}

fn load_prompt_history_from_path(path: &Path, project_key: &str) -> Vec<String> {
    load_history_map(path, project_key)
        .remove(project_key)
        .unwrap_or_default()
}

fn save_prompt_history_to_path(
    path: &Path,
    project_key: &str,
    history: &[String],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Read-modify-write: preserve every OTHER project's bucket and migrate a
    // legacy flat-array file, then replace only this project's entries. This is
    // what keeps saving from one project from clearing another's records.
    let mut map = load_history_map(path, project_key);
    let entries = sanitize_prompt_history(history.to_vec());
    if entries.is_empty() {
        map.remove(project_key);
    } else {
        map.insert(project_key.to_string(), entries);
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

pub struct UiConfig {
    pub theme_id: Option<String>,
    pub yolo: bool,
    pub sandbox: bool,
    /// Named providers (config.providers keys) for the settings dialog.
    pub provider_names: Vec<String>,
    /// No provider credentials are configured yet — show a one-time setup hint
    /// in the transcript pointing the user at `/connect`.
    pub needs_setup: bool,
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
    theme_store: ThemeStore,
    theme: Theme,
    should_quit: bool,
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
    /// Whether any overlay (modal/panel/toast) was open on the previous frame.
    /// When one closes, the next frame forces a FULL terminal repaint: diff
    /// rendering never re-sends "unchanged" cells, so a terminal that dropped
    /// cells under the overlay (observed in Warp) would keep the gap forever.
    overlay_was_open: bool,
    /// One-shot full-repaint request (overlay close, Ctrl+L).
    force_redraw: bool,
    show_help: bool,
    toast: Option<Toast>,
    provider_names: Vec<String>,
    /// Chat display prefs (`/thinking`, `/tool-details`), persisted in config
    /// and applied to the active tab's chat each frame.
    show_thinking: bool,
    show_tool_details: bool,
    /// Submitted prompts, oldest first, for Up/Down recall in the input box.
    prompt_history: Vec<String>,
    /// Project bucket key (the session cwd) under which `prompt_history` is
    /// persisted in `~/.zode/prompt_history.json`.
    prompt_history_key: String,
    /// Cursor into `prompt_history` while browsing (None = editing live text).
    history_pos: Option<usize>,
    /// The in-progress text saved when history browsing began, restored when
    /// the user pages back down past the newest entry.
    history_draft: String,
    /// Index of the queued follow-up currently mirrored in the prompt editor.
    queued_edit_index: Option<usize>,
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
        tab0.titled = resumed_id.is_some();
        // A resumed session (--continue/--resume): replay its transcript into
        // the chat and restore its title (the engine already holds the store).
        if let Some(id) = &resumed_id {
            if let Ok(store) = tab0.engine.store.lock() {
                tab0.chat = rebuild_chat_from_store(&store);
                tab0.context_tokens = estimate_store_tokens(&store);
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

        // Seed input-line history with the conversation's prompts so Up/Down
        // recalls them immediately — even on a fresh/resumed session before
        // anything is typed. Persisted entries (this project's bucket in
        // prompt_history.json) come first; then this conversation's user
        // messages (deduped, in order).
        let prompt_history_key = prompt_history_key(template.cwd());
        let mut prompt_history = load_prompt_history(&prompt_history_key);
        for msg in tab0.chat.messages() {
            if msg.role == crate::ui::chat::Role::User {
                record_prompt_history_entry(&mut prompt_history, &msg.text);
            }
        }

        // Read display prefs before `template` is moved into the struct.
        let show_thinking = template.show_thinking();
        let show_tool_details = template.show_tool_details();
        // Mouse capture drives BOTH terminal setup and app-managed selection:
        // with capture off (`"mouseCapture": false`) the terminal owns
        // selection — ⌘C copies natively — and no mouse events reach the app.
        let mouse_capture = template.mouse_capture();
        // Apply the configured UI language so the chrome renders localized.
        if let Some(lang) = template.language() {
            zode_core::i18n::set_language_code(lang);
        }

        Self {
            tabs: vec![tab0],
            active: 0,
            next_tab_id: 1,
            // Capture the startup strict-read bit before `template` is moved.
            sandbox_restrict_reads: template
                .sandbox()
                .map(|c| c.restrict_reads())
                .unwrap_or(false),
            template,
            sidebar_visibility: SidebarVisibility::Auto,
            selection_mode: mouse_capture,
            active_selection: None,
            active_input_selection: None,
            input: InputBox::new(),
            pending_cursor_seq: None,
            status,
            theme_store,
            theme,
            should_quit: false,
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
            agents_dialog: None,
            workflows_dialog: None,
            mcp_dialog: None,
            session_picker: None,
            tasks_panel: None,
            bg_shells: Vec::new(),
            subagents_panel: None,
            subagents: Vec::new(),
            mcp_section_collapsed: false,
            files_section_collapsed: false,
            todo_section_collapsed: false,
            files_panel: None,
            sidebar_hits: crate::ui::tabs::SidebarHits::default(),
            sidebar_area: None,
            last_sidebar_poll: None,
            overlay_was_open: false,
            force_redraw: false,
            show_help: false,
            toast: None,
            provider_names: ui.provider_names,
            show_thinking,
            show_tool_details,
            prompt_history,
            prompt_history_key,
            history_pos: None,
            history_draft: String::new(),
            queued_edit_index: None,
        }
    }

    /// Record a submitted prompt for Up/Down recall (skips blanks and exact
    /// consecutive duplicates), and reset the browse cursor.
    fn record_prompt_history(&mut self, text: &str) {
        if record_prompt_history_entry(&mut self.prompt_history, text) {
            save_prompt_history(&self.prompt_history_key, &self.prompt_history);
        }
        self.history_pos = None;
        self.history_draft.clear();
    }

    /// Recall the previous (older) prompt into the input box. On first step it
    /// stashes the current draft so Down can restore it.
    fn history_prev(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        let next = match self.history_pos {
            None => {
                self.history_draft = self.input.text();
                self.prompt_history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.history_pos = Some(next);
        let entry = self.prompt_history[next].clone();
        self.input.set_text(&entry);
    }

    /// Step to a newer prompt; past the newest, restore the stashed draft.
    fn history_next(&mut self) {
        let Some(p) = self.history_pos else {
            return;
        };
        if p + 1 < self.prompt_history.len() {
            self.history_pos = Some(p + 1);
            let entry = self.prompt_history[p + 1].clone();
            self.input.set_text(&entry);
        } else {
            self.history_pos = None;
            let draft = std::mem::take(&mut self.history_draft);
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
        self.history_pos = None;
        self.history_draft.clear();
        self.active_input_selection = None;
    }

    fn save_queued_edit_text(&mut self, text: String) -> Option<usize> {
        let index = self.queued_edit_index?;
        self.queued_edit_index = None;
        let queue = &mut self.tabs[self.active].queued_input;
        if index >= queue.len() {
            return None;
        }
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
    fn close_active_tab(&mut self) {
        // Drop the tab's clipboard preview temp files before it goes away.
        let temps: Vec<std::path::PathBuf> = self.tabs[self.active]
            .pending_images
            .iter()
            .map(|i| i.path.clone())
            .collect();
        for path in &temps {
            cleanup_clipboard_temp(&mut self.clipboard_temps, path);
        }
        if self.tabs.len() == 1 {
            self.should_quit = true;
            return;
        }
        if let Some(abort) = self.tabs[self.active].turn_abort.take() {
            abort.abort_with_reason("tab closed");
        }
        self.tabs.remove(self.active);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.dismiss_input_popups();
        self.queued_edit_index = None;
    }

    /// Abort the active tab's in-flight turn, if any. Returns true when a turn
    /// was actually interrupted (false when the tab was already idle). Shared
    /// by Ctrl+C and Esc.
    fn interrupt_active_turn(&mut self) -> bool {
        let tab = &mut self.tabs[self.active];
        if let Some(abort) = tab.turn_abort.take() {
            abort.abort_with_reason("user interrupted");
            tab.active_turn_id = 0;
            // Esc / Ctrl+C also halts the goal auto-loop (and purges any queued
            // continuation) so it doesn't restart.
            stop_goal_loop(tab);
            tab.chat.end_turn();
            tab.chat.push_system(crate::tr("(interrupted)"));
            tab.mode = Mode::Ready;
            true
        } else {
            false
        }
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

    /// Build the permission dialog for `req`, first focusing the tab that
    /// requested it (its gate is labeled with the tab id) so the prompt shows
    /// over the right conversation and uses that tab's cwd. Only called when a
    /// request becomes the ACTIVE dialog — queued requests don't move focus.
    fn open_approval(&mut self, req: ApprovalRequest) -> PermissionDialog {
        if let Some(src) = req.source.as_deref().and_then(|s| s.parse::<usize>().ok()) {
            if let Some(pos) = self.tabs.iter().position(|t| t.id == src) {
                self.active = pos;
                // A request from another tab can steal focus mid-compose; drop
                // any popup whose candidates belong to the previous tab.
                self.dismiss_input_popups();
                self.queued_edit_index = None;
            }
        }
        let cwd = self.active_tab().engine.cwd.clone();
        PermissionDialog::new(req, cwd)
    }

    /// Respond to the active permission prompt, then surface the next queued
    /// request (if any), focusing its source tab/cwd.
    fn answer_permission(&mut self, approval: Approval) {
        // Capture the tool before answering (responding consumes the request).
        let tool = self.active_dialog.as_ref().map(|d| d.tool().to_string());
        let responded = self
            .active_dialog
            .as_mut()
            .map(|d| d.answer(approval))
            .unwrap_or(false);
        if responded {
            // Persist an "allow always" decision to the project state so the
            // tool is auto-allowed (no prompt) in future sessions here.
            if approval == Approval::AllowAlways {
                if let Some(tool) = tool {
                    self.persist_allow_always(&tool);
                }
            }
            let next = self.pending_requests.pop_front();
            self.active_dialog = next.map(|r| self.open_approval(r));
        }
    }

    /// Record a tool name into `<cwd>/.zode/state.json` `permissions.allow`
    /// (deduped) so an "allow always" choice survives restarts.
    fn persist_allow_always(&self, tool: &str) {
        let cwd = self.active_tab().engine.cwd.clone();
        let tool = tool.to_string();
        // Best-effort: a failed persist must not interrupt the turn.
        let _ = zode_core::config::ConfigManager::update_project_state(&cwd, |s| {
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

    /// Start rebuilding the active tab's engine from `template` off the UI loop
    /// (model/provider/goal/plugin/sandbox switches all land here), carrying the
    /// conversation store + cwd over so the context survives. The template and
    /// status are committed only when the background result arrives.
    fn start_reassemble_active(
        &mut self,
        template: EngineTemplate,
        effect: ReassembleEffect,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> bool {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't switch during a turn — Ctrl+C first",
            )));
            return false;
        }
        // Plan mode is per-tab: re-apply THIS tab's mode to whatever template a
        // caller passed (a model/provider/yolo swap must not drop or leak it).
        let template = template.with_plan_mode(self.active_tab().plan_mode);
        let (store, cwd, id, seq) = {
            let tab = self.active_tab();
            let store = match tab.engine.store.lock() {
                Ok(s) => s.clone(),
                Err(_) => return false,
            };
            (
                store,
                tab.engine.cwd.clone(),
                tab.id,
                tab.reassemble_seq + 1,
            )
        };
        {
            let tab = self.active_tab_mut();
            tab.reassemble_seq = seq;
            tab.reassemble_pending = true;
            tab.mode = Mode::Switching;
            tab.active_tool_names.clear();
        }

        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = match template.assemble_tab(Some(cwd), Some(id.to_string())).await {
                Ok(engine) => Ok(ReassembledEngine {
                    template,
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
        let Some(tab_idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        if !self.tabs[tab_idx].reassemble_pending || self.tabs[tab_idx].reassemble_seq != seq {
            return;
        }

        self.tabs[tab_idx].reassemble_pending = false;
        // A new/resumed tab assembles under an UNCHANGED template clone —
        // installing it back would clobber a template switch that happened
        // while the assembly ran.
        let tab_creation = matches!(
            effect,
            ReassembleEffect::NewTab | ReassembleEffect::ResumeTab
        );
        match result {
            Ok(done) => {
                let model = done.engine.model.clone();
                self.tabs[tab_idx].engine = Arc::new(done.engine);
                self.tabs[tab_idx].mode = Mode::Ready;
                if !tab_creation {
                    self.template = done.template;
                    self.status.model = model;
                }
                if self.active < self.tabs.len() && self.tabs[self.active].id == tab_id {
                    self.refresh_dynamic_commands();
                }
                self.apply_reassemble_effect(tab_idx, effect);
            }
            Err(e) if tab_creation => {
                // The placeholder tab never got a real engine — remove it
                // (keeping it would leave a tab aliasing another tab's engine
                // and store). If it was the last tab (the parent was closed
                // mid-assembly), quit like closing the last tab does.
                self.tabs.remove(tab_idx);
                if self.tabs.is_empty() {
                    self.should_quit = true;
                    return;
                }
                if self.active >= self.tabs.len() {
                    self.active = self.tabs.len() - 1;
                }
                self.toast = Some(Toast::error(e));
            }
            Err(e) => {
                if let ReassembleEffect::Plan { on } = effect {
                    self.tabs[tab_idx].plan_mode = !on;
                }
                self.tabs[tab_idx]
                    .chat
                    .push_system(&format!("{}: {e}", crate::tr("switch failed")));
                self.tabs[tab_idx].mode = Mode::Error;
                self.toast = Some(Toast::error(format!("{}: {e}", crate::tr("switch failed"))));
            }
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
            ReassembleEffect::Connect { provider_name } => {
                self.toast = Some(Toast::info(format!(
                    "{} -> {provider_name}",
                    crate::tr("provider")
                )));
                self.tabs[tab_idx]
                    .chat
                    .push_system(&format!("{} -> {provider_name}", crate::tr("provider")));
            }
            ReassembleEffect::Effort { notify } => {
                self.apply_reassemble_notify(tab_idx, notify);
            }
            ReassembleEffect::Goal { goal } => self.apply_goal_effect(tab_idx, goal),
            ReassembleEffect::Model { id } => self.apply_model_effect(tab_idx, &id),
            // Fresh tab: nothing to announce — the tab flipping from
            // Switching to Ready IS the signal.
            ReassembleEffect::NewTab => {}
            // Resumed tab: the engine arrived with the saved store attached —
            // replay it into the transcript and seed the context gauge.
            ReassembleEffect::ResumeTab => {
                let rebuilt = {
                    let tab = &self.tabs[tab_idx];
                    tab.engine.store.lock().ok().map(|store| {
                        (
                            rebuild_chat_from_store(&store),
                            estimate_store_tokens(&store),
                        )
                    })
                };
                if let Some((chat, tokens)) = rebuilt {
                    let tab = &mut self.tabs[tab_idx];
                    tab.chat = chat;
                    tab.context_tokens = tokens;
                }
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
            ReassembleEffect::Yolo { notify } => {
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
        let _ = zode_core::config::ConfigManager::update_project_state(&cwd, |s| {
            s.insert(
                "sandbox".into(),
                serde_json::json!({
                    "enabled": enabled,
                    "mode": mode,
                    "network": network,
                }),
            );
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
            self.apply_plugins(dialog.disabled_ids(), dialog.all_ids(), agent_tx)
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
                    "Create a reusable JS workflow for me using the `define_workflow` tool. \
                     Here is what it should accomplish:\n\n{brief}\n\nWrite the orchestration \
                     script with agent()/parallel()/pipeline() so zode can execute it \
                     deterministically with run_workflow, then call define_workflow."
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
        let metas: Vec<SessionMeta> = SessionIndex::load()
            .map(|i| i.newest_first().into_iter().cloned().collect())
            .unwrap_or_default();
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
            KeyCode::Esc => self.session_picker = None,
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
                let target = self.session_picker.as_ref().and_then(|p| p.selected());
                if let Some(meta) = target {
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
        tab.title = meta.title.clone();
        tab.titled = true;
        tab.reassemble_pending = true;
        tab.reassemble_seq = 1;
        tab.mode = Mode::Switching;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.dismiss_input_popups();

        let template = self.template.clone();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = async {
                let store = Session::load(&path)
                    .await
                    .map_err(|e| format!("{}: {e}", crate::tr("load failed")))?;
                let engine = template
                    .assemble_tab(cwd_override, Some(id.to_string()))
                    .await
                    .map_err(|e| format!("{}: {e}", crate::tr("assemble failed")))?;
                Ok(ReassembledEngine {
                    template,
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

    /// Refresh the active tab's sidebar section data on a slow cadence: the
    /// MCP connection snapshot (sync, cheap) and a spawned git working-tree
    /// poll (subprocess — never run on the UI loop, one in flight per tab).
    fn refresh_sidebar_sections(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        const INTERVAL: Duration = Duration::from_secs(2);
        if !should_show_sidebar(self.tabs.len(), self.sidebar_visibility) {
            return;
        }
        if self
            .last_sidebar_poll
            .is_some_and(|t| t.elapsed() < INTERVAL)
        {
            return;
        }
        self.last_sidebar_poll = Some(std::time::Instant::now());
        let tab = &mut self.tabs[self.active];
        tab.mcp_status = tab.engine.mcp_status();
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
            || self.agents_dialog.is_some()
            || self.workflows_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.session_picker.is_some()
            || self.tasks_panel.is_some()
            || self.subagents_panel.is_some()
            || self.files_panel.is_some()
            || self.show_help
            || self.toast.is_some()
    }

    /// Left-click on a collapsible sidebar section header toggles its fold;
    /// clicking the modified-files "…+k more" row opens the full-list overlay.
    fn try_sidebar_header_click(&mut self, mouse: &MouseEvent) -> bool {
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

        loop {
            // Full repaint when an overlay just closed (or Ctrl+L asked): see
            // `overlay_was_open` — repairs cells the terminal lost under it.
            let overlay_open = self.any_overlay_open();
            if self.force_redraw || (self.overlay_was_open && !overlay_open) {
                self.force_redraw = false;
                terminal.clear()?;
            }
            self.overlay_was_open = overlay_open;
            terminal.draw(|f| self.draw(f))?;
            if self.should_quit {
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

            tokio::select! {
                maybe_ev = term_events.next() => {
                    if let Some(Ok(ev)) = maybe_ev {
                        self.handle_term(ev, &agent_tx).await;
                        // Coalesce the rest of an input burst before redrawing.
                        // A trackpad/wheel momentum flick floods scroll events;
                        // handling them all here (then one draw at the top of
                        // the loop) stops over-scrolling at the top/bottom from
                        // backing up into a multi-second redraw storm.
                        for ev in drain_ready_events(&mut term_events, INPUT_COALESCE_CAP) {
                            if self.should_quit {
                                break;
                            }
                            self.handle_term(ev, &agent_tx).await;
                        }
                        // Switching to a tab that has queued input (and is now
                        // idle) flushes it here, not just on its own turn-done.
                        self.dispatch_queued_input(&agent_tx).await;
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_agent_event(app_ev);
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
                                self.handle_agent_event(ev);
                                drained += 1;
                            }
                            Err(_) => break,
                        }
                    }
                    // A turn may have just finished — if it left the context at
                    // the auto-compact threshold, compact before anything new is
                    // sent, then flush any queued input.
                    self.maybe_auto_compact(&agent_tx);
                    self.dispatch_queued_input(&agent_tx).await;
                }
                Some(req) = self.approval_rx.next() => {
                    // An approval is the highest-priority modal: dismiss any
                    // settings/help/picker/panel overlay so it can't hide the
                    // prompt that is now capturing input.
                    self.settings = None;
                    self.connect = None;
                    self.session_picker = None;
                    self.tasks_panel = None;
                    self.subagents_panel = None;
                    self.files_panel = None;
                    self.show_help = false;
                    // Only focus the source tab when this request becomes the
                    // active dialog; a queued request must not steal focus from
                    // the dialog currently shown.
                    if self.active_dialog.is_none() {
                        let d = self.open_approval(req);
                        self.active_dialog = Some(d);
                    } else {
                        self.pending_requests.push_back(req);
                    }
                }
                Some(req) = self.question_rx.next() => {
                    // A question is a modal like an approval: clear overlays so
                    // it can't be hidden, then show it (or queue if one's up).
                    self.settings = None;
                    self.connect = None;
                    self.session_picker = None;
                    self.tasks_panel = None;
                    self.subagents_panel = None;
                    self.files_panel = None;
                    self.show_help = false;
                    if self.active_question.is_none() {
                        self.open_question(req);
                    } else {
                        self.pending_questions.push_back(req);
                    }
                }
                _ = ticker.tick() => {
                    self.status.tick();
                    if let Some(t) = &mut self.toast {
                        if t.tick() {
                            self.toast = None;
                        }
                    }
                    // Keep the open tasks panel's shell list live.
                    if self.tasks_panel.is_some() {
                        self.refresh_bg_shells().await;
                    }
                    // Keep the active tab's sub-agent snapshot fresh every tick:
                    // it feeds BOTH the sidebar "subagents" section (always
                    // visible) and the overlay (when open).
                    self.refresh_subagents();
                    // Keep each tab's cached todo snapshot fresh so the sync
                    // sidebar render reads current state. Index-based to avoid
                    // holding a `&self.tabs` borrow across the await. Cheap:
                    // an RwLock read + small Vec clone per tab at ~10 fps.
                    for i in 0..self.tabs.len() {
                        let engine = self.tabs[i].engine.clone();
                        let snap = engine.todo_state.snapshot().await;
                        self.tabs[i].todos = snap;
                    }
                    // Throttled sidebar data poll: git working-tree stats
                    // (spawned off-loop) + MCP connection state, active tab.
                    self.refresh_sidebar_sections(&agent_tx);
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
        // Mirror the active tab's live mode/token counts into the status bar.
        {
            let tab = &self.tabs[self.active];
            self.status.mode = tab.mode;
            self.status.input_tokens = tab.input_tokens;
            self.status.output_tokens = tab.output_tokens;
            // Context-window occupancy: last prompt size vs the active model's window.
            self.status.context_tokens = tab.context_tokens;
            self.status.context_window = tab.engine.model_max_tokens;
            // Plan mode is per-tab, so the badge always reflects the active tab.
            self.status.plan_mode = tab.plan_mode;
            self.status.selection_mode = self.selection_mode;
        }
        // Active provider group (for the `model(provider)` label), from the live
        // template — current across startup, model switch, and connect.
        self.status.provider = self.template.active_provider_name().unwrap_or_default();
        // Keep the approval + sandbox badges in sync with the live template
        // (single source of truth across startup / toggles / tab switches).
        self.status.yolo = self.template.yolo();
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

        let areas = split_main(area, show_sidebar);
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
                    git_files: self.tabs[self.active].git_files.as_deref().unwrap_or(&[]),
                    files_collapsed: self.files_section_collapsed,
                    version: env!("CARGO_PKG_VERSION"),
                },
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
        if let (Some(strip), Some(dialog)) = (perm_inline, &self.active_dialog) {
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
        self.status.render(f, areas.status, &theme);
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
            let turns: Vec<String> = self
                .tabs
                .iter()
                .filter(|t| t.is_busy())
                .map(|t| format!("{}: running", t.title))
                .collect();
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
            if let Some(panel) = &mut self.subagents_panel {
                panel.render(f, area, &agents, now, &theme);
            }
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
        if self.show_help {
            crate::ui::help::render_help(f, area, &theme);
        }
        // Toast renders before the question modal so it can never cover it.
        if let Some(toast) = &self.toast {
            toast.render(f, area, &theme);
        }
        if let Some(q) = &self.active_question {
            q.render(f, area, &theme);
        }
        // The permission prompt normally renders INLINE above the input (see
        // `perm_inline` above) so it never blocks the view or the input box.
        // Only fall back to the centered popup when the terminal was too short
        // to dock it inline.
        if perm_inline.is_none() {
            if let Some(dialog) = &self.active_dialog {
                dialog.render(f, area, &theme);
            }
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
                self.handle_mouse(mouse);
                return;
            }
            CtEvent::Resize(_, _) => {
                // The layout reflows on resize, so a held selection's screen
                // mapping (anchored to the pre-resize frame) is now stale — drop
                // it rather than risk copying the wrong region on the next chord.
                self.active_selection = None;
                self.active_input_selection = None;
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
            // Paste uses the platform primary modifier (Cmd on macOS, Ctrl on
            // Windows/Linux), like the other app chords — not Ctrl-only.
            (KeyCode::Char('v'), m) if is_primary_mod(m) => {
                self.paste_from_clipboard();
                return;
            }
            // App chords use the platform primary modifier: Cmd (⌘) on macOS,
            // Ctrl elsewhere (see `is_primary_mod`).
            (KeyCode::Char('o'), m) if is_primary_mod(m) => {
                self.open_settings();
                return;
            }
            (KeyCode::Char('t'), m) if is_primary_mod(m) => {
                self.new_tab(agent_tx);
                return;
            }
            (KeyCode::Char('w'), m) if is_primary_mod(m) => {
                self.close_active_tab();
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
            // ⌘/Ctrl + 1..9 jump to a tab by position.
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
                if !text.trim().is_empty() {
                    // A follow-up typed while the tab is busy will be QUEUED by
                    // submit(); queued follow-ups are intentionally never
                    // recorded — neither persisted nor added to Up/Down recall.
                    if !self.active_tab().is_busy() {
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
            }
            (KeyCode::Down, m) if m.is_empty() && self.input.cursor_on_last_line() => {
                if !self.edit_next_queued_input() {
                    self.history_next();
                }
            }
            _ => {
                self.active_input_selection = None;
                self.input.input(key);
                // Editing the text exits history-browse mode.
                self.history_pos = None;
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

    fn handle_mouse(&mut self, mouse: MouseEvent) {
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

        if self.active_dialog.is_some()
            || self.active_question.is_some()
            || self.settings.is_some()
            || self.connect.is_some()
            || self.plugin_picker.is_some()
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

        // (Cmd/Ctrl)+left-click on an image chip opens it in the OS viewer.
        if self.try_view_image_chip_click(&mouse) {
            return;
        }

        // Left-click on a collapsible sidebar section header toggles its fold.
        if self.try_sidebar_header_click(&mouse) {
            return;
        }

        let Ok((width, height)) = crossterm::terminal::size() else {
            return;
        };
        let area = Rect::new(0, 0, width, height);
        let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
        let areas = split_main(area, show_sidebar);
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
                }
                true
            }
            _ => false,
        }
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
            let Ok((width, height)) = crossterm::terminal::size() else {
                return;
            };
            let area = Rect::new(0, 0, width, height);
            let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
            let chat_area = split_main(area, show_sidebar).chat;
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
        let providers = self.template.provider_names();
        if providers.is_empty() {
            self.active_tab_mut().chat.push_system(crate::tr(
                "no named providers configured — add one under `providers` in your config, \
                 then pick it here to handle image understanding",
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
        match zode_core::sandbox::resolve(cwd, true, mode, net, &[], false, false) {
            // Re-apply the startup strict-read bit (a fresh resolve, e.g. on
            // `/sandbox on` from off, would otherwise drop it).
            Ok(opt) => Ok(opt.map(|c| c.with_restrict_reads(self.sandbox_restrict_reads))),
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
                // the define_agent tool (requires orchestration, default on).
                self.agents_dialog = None;
                let prompt = format!(
                    "Create a new sub-agent for me using the `define_agent` tool. \
                     Here is what it should do:\n\n{brief}\n\nChoose a concise \
                     kebab-case name, a one-line description, and a clear system \
                     prompt, then call define_agent with them."
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

        let provider_name = action.name;
        // Carry the just-saved providers map onto the template so the status
        // bar's `model(provider)` label resolves the freshly connected group.
        let t = self
            .template
            .with_provider_config(provider)
            .with_providers_map(cfg.providers.clone());
        self.start_reassemble_active(t, ReassembleEffect::Connect { provider_name }, agent_tx);
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
                    self.apply_plugins(picker.disabled_ids(), picker.all_ids(), agent_tx)
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

    /// Persist the disabled-plugin set to the global config and reassemble the
    /// active tab so the change (tools dropped, MCP disconnected, skills hidden)
    /// applies to the running conversation.
    ///
    /// `disabled` is the off-set the picker showed; `owned` is every id it
    /// presented. Disabled ids in config but NOT in `owned` (e.g. a
    /// project-scoped MCP server or skill from a different workspace, or the
    /// not-yet-shown `lsp:*` rows) are preserved verbatim — replacing the whole
    /// list with just `disabled` would silently re-enable them.
    async fn apply_plugins(
        &mut self,
        disabled: Vec<String>,
        owned: Vec<String>,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(crate::tr(
                "can't change plugins during a turn — Ctrl+C first",
            )));
            return;
        }
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
        self.start_reassemble_active(
            t,
            ReassembleEffect::Notify(ReassembleNotify::Toast(
                crate::tr("plugins updated").to_string(),
            )),
            agent_tx,
        );
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
        let template = self.template.with_plan_mode(self.tabs[tab_idx].plan_mode);
        let hot_result = {
            let tab = &mut self.tabs[tab_idx];
            match Arc::get_mut(&mut tab.engine) {
                Some(engine) => template.hot_swap_model(engine, id.to_string()).map(Some),
                None => Ok(None),
            }
        };

        match hot_result {
            Ok(Some(template)) => {
                self.template = template;
                self.apply_model_effect(tab_idx, id);
            }
            Ok(None) => {
                let t = self.template.with_model(id.to_string());
                self.start_reassemble_active(
                    t,
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

        let template = self.template.with_plan_mode(self.tabs[tab_idx].plan_mode);
        let hot_template = {
            let tab = &mut self.tabs[tab_idx];
            Arc::get_mut(&mut tab.engine)
                .map(|engine| template.hot_swap_goal(engine, new_goal.clone()))
        };

        if let Some(template) = hot_template {
            self.template = template;
            self.apply_goal_effect(tab_idx, new_goal);
            return;
        }

        let t = self.template.with_goal(new_goal.clone());
        if !self.start_reassemble_active(
            t,
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
        if self.active_tab().is_busy() {
            return;
        }
        if self.queued_edit_index == Some(0) {
            return;
        }
        let next = self.active_tab_mut().queued_input.pop_front();
        if let Some(text) = next {
            if let Some(index) = self.queued_edit_index.as_mut() {
                *index = index.saturating_sub(1);
            }
            self.submit(&text, agent_tx).await;
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
        let abort = AbortController::new();
        let owned_slot = !self.active_tab().is_busy();
        if owned_slot {
            // Reuse the turn-busy machinery: spinner shows, prompts queue, and
            // Esc (interrupt_active_turn) aborts — the select below sees it
            // and the child dies with the dropped future (kill_on_drop).
            self.active_tab_mut().turn_abort = Some(abort.clone());
        }
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
                owned_slot,
            });
        });
    }

    async fn submit(&mut self, text: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let cwd = self.active_tab().engine.cwd.clone();

        // `!<cmd>` runs a shell command directly (no agent turn). The command +
        // its output show inline AND are buffered as context for the next prompt
        // so the agent knows what was run locally.
        if let Some(cmd) = text.trim().strip_prefix('!') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.spawn_local_shell(cmd, agent_tx);
            }
            return;
        }
        let parsed = match split_pasted_image_paths(&cwd, text) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.toast = Some(Toast::error(e.to_string()));
                return;
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
                        return;
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
            return;
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
                    return;
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
            if !submitted_text.trim().is_empty() {
                self.active_tab_mut()
                    .queued_input
                    .push_back(submitted_text.to_string());
                let n = self.active_tab().queued_input.len();
                self.toast = Some(Toast::info(
                    crate::tr("queued ({n}) — sends when the turn finishes (Esc to interrupt now)")
                        .replace("{n}", &n.to_string()),
                ));
            } else if pasted_count > 0 {
                self.toast = Some(Toast::info(format!(
                    "{} {pasted_count} {}",
                    crate::tr("attached"),
                    crate::tr("images")
                )));
            }
            return;
        }

        let has_images = !self.active_tab().pending_images.is_empty();
        let images_cfg = self.template.images().clone();
        let image_route = resolve_image_submit_route(
            has_images,
            images_cfg.effective_mode(),
            self.active_tab().engine.supports_images(),
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
                    return;
                }
                None
            }
            ImageSubmitRoute::VisionModel => {
                let Some(provider_name) = images_cfg.vision_provider.as_deref() else {
                    self.toast = Some(Toast::error(crate::tr(
                        "configure /vision provider <name> first",
                    )));
                    return;
                };
                let Some(template) = self.template.with_provider(provider_name) else {
                    self.toast = Some(Toast::error(
                        crate::tr("vision provider '{provider_name}' is not configured")
                            .replace("{provider_name}", provider_name),
                    ));
                    return;
                };
                Some((template, provider_name.to_string()))
            }
        };

        // Stamp the session title from the first user prompt of this tab.
        if !self.active_tab().titled {
            let title_source = if submitted_text.trim().is_empty() {
                self.active_tab()
                    .pending_images
                    .first()
                    .map(|image| image.display_name.as_str())
                    .unwrap_or("image")
                    .to_string()
            } else {
                submitted_text.clone()
            };
            self.active_tab_mut().stamp_title(&title_source);
        }

        // The pending images are about to be consumed; drop any chip selection.
        self.selected_image = None;
        let tab = &mut self.tabs[self.active];
        let images = std::mem::take(&mut tab.pending_images);
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

        tab.turn_seq += 1;
        let turn_id = tab.turn_seq;
        tab.active_turn_id = turn_id;
        let tab_id = tab.id;
        let abort = AbortController::new();
        tab.turn_abort = Some(abort.clone());

        let engine = tab.engine.clone();
        let images_for_vision = images.clone();
        let submitted_text_for_vision = submitted_text.clone();
        let vision_prompt = images_cfg.effective_prompt().to_string();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let stream_result = if let Some((vision_template, provider_name)) = vision_template {
                let vision_engine = match vision_template
                    .assemble_tab(Some(engine.cwd.clone()), Some(format!("{tab_id}:vision")))
                    .await
                {
                    Ok(e) if e.supports_images() => Arc::new(e),
                    Ok(_) => {
                        let _ = tx.send(AppEvent::TurnDone {
                            tab_id,
                            turn_id,
                            result: Err(crate::tr(
                                "vision provider '{provider_name}' does not declare image support",
                            )
                            .replace("{provider_name}", &provider_name)),
                        });
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::TurnDone {
                            tab_id,
                            turn_id,
                            result: Err(format!("{}: {e}", crate::tr("vision provider failed"))),
                        });
                        return;
                    }
                };
                match run_vision_description(
                    vision_engine,
                    vision_prompt,
                    submitted_text_for_vision.clone(),
                    images_for_vision,
                    abort.clone(),
                )
                .await
                {
                    Ok(description) => {
                        let prompt =
                            merge_prompt_with_vision(&submitted_text_for_vision, &description);
                        engine.turn(&prompt, abort).await
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::TurnDone {
                            tab_id,
                            turn_id,
                            result: Err(e),
                        });
                        return;
                    }
                }
            } else {
                engine.turn_blocks(content, abort).await
            };

            match stream_result {
                Ok(mut stream) => {
                    // Surface the post-compaction restoration note (if the
                    // engine injected one at the start of this turn) through
                    // the same channel as provider events, so the generic
                    // Notice renderer shows it in the transcript.
                    if let Some(note) = engine.take_restore_note() {
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
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(event) => {
                                // Feed the cost tracker (counts only Usage events).
                                engine.cost.observe(&event).await;
                                let cost_label = if matches!(
                                    event,
                                    Event::Usage { .. } | Event::ToolResult { .. }
                                ) {
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
                                    return;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::TurnDone {
                                    tab_id,
                                    turn_id,
                                    result: Err(e.to_string()),
                                });
                                return;
                            }
                        }
                    }
                    let _ = tx.send(AppEvent::TurnDone {
                        tab_id,
                        turn_id,
                        result: Ok(()),
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::TurnDone {
                        tab_id,
                        turn_id,
                        result: Err(e.to_string()),
                    });
                }
            }
        });
    }

    fn handle_agent_event(&mut self, ev: AppEvent) {
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
            result,
            auto,
        } = ev
        {
            let Some(tab) = self.tab_by_id(tab_id) else {
                return;
            };
            tab.turn_abort = None;
            tab.active_turn_id = 0;
            let ok = result.is_ok();
            match result {
                Ok(summary) => {
                    tab.auto_compact_failures = 0;
                    tab.chat.push_system(&summary);
                    tab.mode = Mode::Ready;
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
            // transcript survives a resume (mirrors the post-turn save).
            if ok {
                // Refresh the context gauge immediately from the shrunken store,
                // so the "% ctx" badge drops right after /compact instead of
                // lingering at the pre-compact count until the next Usage event.
                if let Ok(store) = tab.engine.store.lock() {
                    tab.context_tokens = estimate_store_tokens(&store);
                }
                let (session_id, engine, title) = (
                    tab.session_id.clone(),
                    tab.engine.clone(),
                    tab.title.clone(),
                );
                tokio::spawn(crate::tab::persist_session(session_id, engine, title));
            }
            return;
        }
        if let AppEvent::BgProgress { tab_id, line } = ev {
            let Some(tab) = self.tab_by_id(tab_id) else {
                return;
            };
            if tab.is_busy() {
                tab.chat.push_system(&line);
            }
            return;
        }
        if let AppEvent::BgDone { tab_id, result } = ev {
            let Some(tab) = self.tab_by_id(tab_id) else {
                return;
            };
            tab.turn_abort = None;
            tab.active_turn_id = 0;
            tab.active_tool_names.clear();
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
            owned_slot,
        } = ev
        {
            let Some(tab) = self.tab_by_id(tab_id) else {
                return;
            };
            // Release the busy slot only if this run took it — and never
            // clobber a LIVE agent turn's abort handle (possible if the user
            // Esc'd this command and started a turn before the kill completed;
            // `active_turn_id != 0` only while a turn is in flight).
            if owned_slot && tab.active_turn_id == 0 {
                tab.turn_abort = None;
            }
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
            | AppEvent::ReassembleDone { .. } => {
                unreachable!("handled above")
            }
        };
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        // Drop events from an aborted/superseded turn within that tab.
        if turn_id != tab.active_turn_id {
            return;
        }
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
                        let title = tool_call_title(name, input);
                        tab.active_tool_names.insert(id.clone(), title);
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                    Event::ToolResult { ref id, .. } => {
                        let known_tool = tab.active_tool_names.remove(id);
                        if let Some(line) = process_line_for_event(&event, known_tool.as_deref()) {
                            tab.chat.push_tool(&line);
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
                            tab.chat.push_tool(&line);
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
                tab.chat.end_turn();
                tab.turn_abort = None;
                tab.active_turn_id = 0;
                tab.active_tool_names.clear();
                let ok = result.is_ok();
                tab.mode = match result {
                    Ok(()) => Mode::Ready,
                    Err(e) => {
                        tab.chat
                            .push_system(&format!("{}: {e}", crate::tr("turn failed")));
                        Mode::Error
                    }
                };
                // Goal auto-loop: keep taking turns toward the goal until the
                // agent calls `goal_complete` — or the user interrupts / clears
                // the goal, or a turn fails. Only continues on a successful turn.
                if tab.goal_loop_active {
                    if !ok {
                        // A failed/interrupted turn halts the loop cleanly.
                        stop_goal_loop(tab);
                    } else if tab.engine.take_goal_completed() {
                        stop_goal_loop(tab);
                        tab.chat
                            .push_system(crate::tr("✓ goal complete — auto-loop stopped"));
                    } else {
                        // Count the turn that just ran, THEN honor the cap so
                        // `autoLoopMaxTurns = N` runs exactly N turns.
                        tab.goal_loop_iter = tab.goal_loop_iter.saturating_add(1);
                        if tab
                            .engine
                            .auto_loop_max_turns()
                            .is_some_and(|max| tab.goal_loop_iter >= max)
                        {
                            stop_goal_loop(tab);
                            tab.chat.push_system(crate::tr(
                                "goal-loop: reached autoLoopMaxTurns — paused (send a message to resume)",
                            ));
                        } else {
                            // Queue the next iteration; `dispatch_queued_input` (main
                            // loop, right after this drains) submits it once idle.
                            tab.queued_input
                                .push_back(GOAL_LOOP_CONTINUE_PROMPT.to_string());
                        }
                    }
                }
                // Persist the session off the event loop.
                let (session_id, engine, title) = (
                    tab.session_id.clone(),
                    tab.engine.clone(),
                    tab.title.clone(),
                );
                // Mine the just-completed turn for durable memories (no-op
                // unless autoExtract is on; runs detached, never blocks).
                engine.spawn_post_turn_extraction();
                tokio::spawn(crate::tab::persist_session(session_id, engine, title));
            }
            AppEvent::Toast { .. }
            | AppEvent::CompactDone { .. }
            | AppEvent::BgProgress { .. }
            | AppEvent::BgDone { .. }
            | AppEvent::GitStatDone { .. }
            | AppEvent::LocalShellDone { .. }
            | AppEvent::ConnectDialogReady { .. }
            | AppEvent::ReassembleDone { .. } => {
                unreachable!("handled above")
            }
        }
    }

    async fn handle_slash(
        &mut self,
        name: &str,
        args: &str,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match name {
            "exit" => self.should_quit = true,
            "help" => self.show_help = true,
            "clear" => {
                // Mutating the store mid-turn races the running QueryLoop.
                if self.active_tab().is_busy() {
                    self.toast = Some(Toast::info(crate::tr(
                        "can't clear during a turn — Ctrl+C first",
                    )));
                } else {
                    let tab = &mut self.tabs[self.active];
                    tab.chat = ChatView::new();
                    if let Ok(mut store) = tab.engine.store.lock() {
                        *store = agent::message::MessageStore::new();
                    }
                    // The context gauge reflects the (now empty) store again
                    // only at the next Usage event — reset it here so the
                    // auto-compact trigger can't fire on a stale 98%+ badge.
                    // A fresh conversation also re-arms the breaker.
                    tab.context_tokens = 0;
                    tab.auto_compact_failures = 0;
                }
            }
            "theme" => self.handle_theme(args),
            // Run undo/redo off the event loop (the history mutex + file
            // restore could block) and toast the result back as an event.
            "undo" => self.spawn_history_op(agent_tx, true),
            "redo" => self.spawn_history_op(agent_tx, false),
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
                let on = !self.template.yolo();
                let t = self.template.with_yolo(on);
                let msg = if on {
                    crate::tr("yolo: ON — tools auto-approve (deny rules still apply)")
                } else {
                    crate::tr("yolo: OFF — tools prompt for approval")
                };
                self.start_reassemble_active(
                    t,
                    ReassembleEffect::Yolo {
                        notify: ReassembleNotify::System(msg.to_string()),
                    },
                    agent_tx,
                );
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
                let path =
                    zode_core::export::resolve_export_path(&self.active_tab().engine.cwd, args);
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
                self.toast = Some(Toast::info(format!(
                    "/{other} {}",
                    crate::tr("lands in a later phase")
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
        let abort = AbortController::new();
        self.active_tab_mut().turn_abort = Some(abort.clone());
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
                        line.truncate(4000);
                        line.push('…');
                    }
                    line
                })
                .map_err(|e| format!("workflow '{name}': {e}"));
            let _ = tx.send(AppEvent::BgDone { tab_id, result });
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
        let abort = AbortController::new();
        self.active_tab_mut().turn_abort = Some(abort.clone());
        self.active_tab_mut()
            .chat
            .push_system(&format!("{} {tool}…", crate::tr("calling op")));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let _ = &abort; // keep the controller alive for the duration
            let result = match OpConnection::ensure(&cfg, consent.as_ref(), &tag).await {
                Ok(client) => match client.call(&tool, args).await {
                    Ok(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())),
                    Err(e) => Err(format!("/op {tool} failed: {e}")),
                },
                Err(e) => Err(format!("/op: {e}")),
            };
            let _ = tx.send(AppEvent::BgDone { tab_id, result });
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
        let abort = AbortController::new();
        self.active_tab_mut().turn_abort = Some(abort.clone());
        self.active_tab_mut()
            .chat
            .push_system(&format!("{}: {prompt}", crate::tr("generating design")));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = match OpConnection::ensure(&cfg, consent.as_ref(), &tag).await {
                Ok(client) => {
                    let g =
                        load_guidance(skills.as_ref(), &["frontend-design", "openpencil-design"]);
                    let gen = DirectLlmContentGenerator { provider, model };
                    // Stream each phase into the originating tab's transcript.
                    let ptx = tx.clone();
                    let progress = move |p| {
                        let _ = ptx.send(AppEvent::BgProgress {
                            tab_id,
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
            let _ = tx.send(AppEvent::BgDone { tab_id, result });
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
        let tab = &mut self.tabs[tab_idx];
        let tab_id = tab.id;
        let engine = tab.engine.clone();
        // Hand the engine the REAL occupancy (provider-reported badge value):
        // it picks the compaction direction from it, and a transcript that is
        // near/over the window must not be sent whole (the summarize request
        // itself would 400 with context-overflow, deadlocking compaction).
        let context_tokens = tab.context_tokens;
        let abort = AbortController::new();
        tab.turn_abort = Some(abort.clone());
        tab.mode = Mode::Compacting;
        tab.chat
            .push_system(crate::tr("compacting the conversation…"));
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let result = engine
                .compact_sized((context_tokens > 0).then_some(context_tokens), abort)
                .await
                .map(|o| {
                    format!(
                        "compacted {} message{} · ~{} → ~{} tokens",
                        o.replaced,
                        if o.replaced == 1 { "" } else { "s" },
                        o.pre_tokens,
                        o.post_tokens,
                    )
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::CompactDone {
                tab_id,
                result,
                auto,
            });
        });
    }

    /// Auto-compact any idle tab whose REAL context occupancy (the badge value,
    /// from the last Usage event) has reached [`AUTO_COMPACT_CONTEXT_PERCENT`].
    /// The runtime's own auto-compaction keys off a byte estimate that
    /// under-counts (especially CJK), so a long conversation can sail past the
    /// provider's input limit and get a hard 400 before the runtime ever trips.
    /// This guard uses the accurate post-turn count instead, and runs between
    /// turns (before any queued input is dispatched).
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
            if needs_auto_compact(tab.context_tokens, tab.engine.model_max_tokens) {
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
                    let providers = self.template.provider_names();
                    let msg = if providers.is_empty() {
                        crate::tr("no named providers configured").to_string()
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
                Some(("mcp", self.mcp_section_collapsed))
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

/// Context occupancy (real tokens / model window, as a percent) at which zode
/// auto-compacts the conversation. Kept just under 100 so compaction happens
/// before the prompt hits the provider's hard input limit.
const AUTO_COMPACT_CONTEXT_PERCENT: u64 = 98;

/// Consecutive auto-compaction failures per tab before the auto trigger stops
/// firing (manual `/compact` stays available; any success resets the count).
const AUTO_COMPACT_MAX_FAILURES: u32 = 3;

/// Whether a tab's real context occupancy has reached the auto-compact
/// threshold. Pure (no side effects) so the decision is unit-testable. A zero
/// window (unknown model size) never triggers.
fn needs_auto_compact(context_tokens: u32, window: u32) -> bool {
    window != 0 && (context_tokens as u64 * 100 / window as u64) >= AUTO_COMPACT_CONTEXT_PERCENT
}

/// Halt the goal auto-loop for a tab: clear the active flag, reset the turn
/// counter, and PURGE any goal-loop prompts still sitting in the input queue so
/// a stale continuation can't dispatch after the loop was stopped (by
/// `goal_complete`, the cap, a failed turn, an interrupt, or `/goal clear`).
/// User-typed follow-ups in the queue are preserved.
fn stop_goal_loop(tab: &mut SessionTab) {
    tab.goal_loop_active = false;
    tab.goal_loop_iter = 0;
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

/// The primary chord modifier for app shortcuts: Cmd (⌘ / SUPER) on macOS,
/// Ctrl elsewhere. On macOS Ctrl is also accepted as a fallback, since many
/// terminals don't deliver the Cmd modifier to TUI apps.
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

/// A short, human-readable preview of a tool's result payload for the chat —
/// stdout for shells, `content` for file reads, etc. Truncated so a chatty tool
/// can't flood the transcript. `None` when there's nothing worth showing beyond
/// the "done" status (e.g. an edit that only returns `{path, status}`).
fn tool_output_preview(output: &serde_json::Value) -> Option<String> {
    const MAX_LINES: usize = 12;
    const MAX_CHARS: usize = 1000;

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
    command.current_dir(cwd).kill_on_drop(true);

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
                            text_parts.push(text.as_str());
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
                    chat.push_user_with_images(&text_parts.join("\n"), images);
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
                            if summary.is_empty() {
                                chat.push_tool(&title);
                            } else {
                                chat.push_tool(&format!("{title} {summary}"));
                            }
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
        Some(c) => {
            let mode = match c.mode() {
                SandboxMode::ReadOnly => "read-only (no file writes — shell or tools)",
                SandboxMode::WorkspaceWrite => "workspace-write (writes confined to the workspace)",
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

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
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
            AppEvent::LocalShellDone { owned_slot, .. } => assert!(!owned_slot),
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
            AppEvent::Toast { .. } => "Toast",
            AppEvent::CompactDone { .. } => "CompactDone",
            AppEvent::BgProgress { .. } => "BgProgress",
            AppEvent::BgDone { .. } => "BgDone",
            AppEvent::GitStatDone { .. } => "GitStatDone",
            AppEvent::LocalShellDone { .. } => "LocalShellDone",
            AppEvent::ConnectDialogReady { .. } => "ConnectDialogReady",
            AppEvent::ReassembleDone { .. } => "ReassembleDone",
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
        // A live agent turn owns the busy slot (active_turn_id != 0) — a stale
        // shell completion must not release it.
        app.active_tab_mut().turn_abort = Some(AbortController::new());
        app.active_tab_mut().active_turn_id = 7;
        let tab_id = app.active_tab().id;
        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "echo x".into(),
            output: Some("x".into()),
            owned_slot: true,
        });
        assert!(
            app.active_tab().turn_abort.is_some(),
            "live turn kept its abort handle"
        );
        // The output itself still lands (it did run).
        assert_eq!(app.active_tab().pending_shell_context.len(), 1);
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
    async fn local_shell_interrupted_posts_nothing() {
        let (mut app, _tx) = make_test_app().await;
        let tab_id = app.active_tab().id;
        app.active_tab_mut().turn_abort = Some(AbortController::new());
        let before = app.active_tab().chat.messages().len();
        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "sleep 100".into(),
            output: None,
            owned_slot: true,
        });
        assert!(!app.active_tab().is_busy());
        assert_eq!(app.active_tab().chat.messages().len(), before);
        assert!(app.active_tab().pending_shell_context.is_empty());
    }

    async fn make_test_app() -> (TuiApp, mpsc::UnboundedSender<AppEvent>) {
        let (app, tx, _temp) = make_test_app_with_dir().await;
        // The tempdir guard drops here — fine for tests that never touch the
        // cwd again after assembly.
        (app, tx)
    }

    /// Like [`make_test_app`] but keeps the cwd tempdir alive — required by
    /// tests that run shell commands or assemble engines AFTER construction.
    async fn make_test_app_with_dir() -> (TuiApp, mpsc::UnboundedSender<AppEvent>, tempfile::TempDir)
    {
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
        let app = TuiApp::new(
            engine,
            template,
            UiConfig {
                theme_id: None,
                yolo: false,
                sandbox: false,
                provider_names: Vec::new(),
                needs_setup: false,
            },
            approval_rx,
            question_rx,
            op_question_queue,
            None,
        );
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        (app, agent_tx, temp)
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
        save_prompt_history_to_path(&path, "/proj/a", &history).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/a"),
            vec!["first prompt".to_string(), "second prompt".to_string()]
        );
    }

    #[tokio::test]
    async fn up_down_recalls_prompt_history_when_idle() {
        let (mut app, agent_tx) = make_test_app().await;
        app.prompt_history = vec!["first prompt".into(), "写个 /tmp/hello.txt".into()];
        app.history_pos = None;
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

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
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

        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
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
        assert!(needs_auto_compact(
            app.tabs[0].context_tokens,
            app.tabs[0].engine.model_max_tokens
        ));

        for _ in 0..AUTO_COMPACT_MAX_FAILURES {
            app.handle_agent_event(AppEvent::CompactDone {
                tab_id,
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
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            result: Ok("compacted".into()),
            auto: false,
        });
        assert_eq!(app.tabs[0].auto_compact_failures, 0);

        // Manual-failure counting is out of scope for the breaker: a manual
        // /compact failure must not advance it.
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            result: Err("boom".into()),
            auto: false,
        });
        assert_eq!(app.tabs[0].auto_compact_failures, 0);
    }

    #[tokio::test]
    async fn model_switch_hot_swaps_without_reassemble_pending() {
        let (mut app, agent_tx) = make_test_app().await;

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

    #[test]
    fn auto_compact_triggers_only_near_full_context() {
        // The threshold is a PERCENT of the model's own window, not an absolute
        // token count — so it scales with model_max_tokens.

        // 200K model → ~196K trigger.
        assert!(!needs_auto_compact(0, 200_000));
        assert!(!needs_auto_compact(180_000, 200_000)); // 90%
        assert!(!needs_auto_compact(195_999, 200_000)); // 97% (integer floor)
        assert!(needs_auto_compact(196_000, 200_000)); // 98%
        assert!(needs_auto_compact(200_000, 200_000)); // 100%

        // 1M model → ~980K trigger, NOT 196K.
        assert!(!needs_auto_compact(196_000, 1_000_000)); // ~20%, nowhere near
        assert!(!needs_auto_compact(900_000, 1_000_000)); // 90%
        assert!(needs_auto_compact(980_000, 1_000_000)); // 98%
        assert!(needs_auto_compact(1_000_000, 1_000_000)); // 100%

        // Unknown window (badge hidden) never triggers.
        assert!(!needs_auto_compact(196_000, 0));
    }

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
        // Long output is truncated with a marker.
        let many = (0..30)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let p = tool_output_preview(&json!({"stdout": many})).unwrap();
        assert!(p.contains("truncated"));
        assert!(p.lines().count() <= 13);
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
        let init = source
            .split("pub fn new(")
            .nth(1)
            .and_then(|tail| tail.split("history_pos: None").next())
            .expect("TuiApp::new initialization block should exist");
        assert!(init.contains("load_prompt_history(&prompt_history_key)"));
    }

    #[test]
    fn prompt_history_round_trips_per_project_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");

        save_prompt_history_to_path(&path, "/proj/a", &["a1".into(), "a2".into()]).unwrap();
        save_prompt_history_to_path(&path, "/proj/b", &["b1".into()]).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/a"),
            vec!["a1".to_string(), "a2".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/b"),
            vec!["b1".to_string()]
        );
        // An unknown project starts empty.
        assert!(load_prompt_history_from_path(&path, "/proj/c").is_empty());
    }

    #[test]
    fn saving_one_project_preserves_other_projects_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");

        save_prompt_history_to_path(&path, "/proj/a", &["a1".into()]).unwrap();
        save_prompt_history_to_path(&path, "/proj/b", &["b1".into()]).unwrap();
        // Overwrite project A — B must be untouched ("不要清空记录").
        save_prompt_history_to_path(&path, "/proj/a", &["a1".into(), "a2".into()]).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/a"),
            vec!["a1".to_string(), "a2".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/b"),
            vec!["b1".to_string()]
        );
    }

    #[test]
    fn legacy_flat_array_migrates_into_current_project_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");
        // Old format: a bare JSON array of prompts.
        fs::write(&path, r#"["old one","old two"]"#).unwrap();

        // Loading from project A pulls the legacy entries into A's bucket.
        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/a"),
            vec!["old one".to_string(), "old two".to_string()]
        );

        // Saving any project rewrites the file in the new map format while
        // keeping the migrated legacy entries under the project that loaded them.
        save_prompt_history_to_path(&path, "/proj/a", &["old one".into(), "old two".into()])
            .unwrap();
        save_prompt_history_to_path(&path, "/proj/b", &["b1".into()]).unwrap();
        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/a"),
            vec!["old one".to_string(), "old two".to_string()]
        );
        assert_eq!(
            load_prompt_history_from_path(&path, "/proj/b"),
            vec!["b1".to_string()]
        );
    }

    #[test]
    fn per_project_history_keeps_recent_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt_history.json");
        let entries: Vec<String> = (0..(PROMPT_HISTORY_LIMIT + 5))
            .map(|i| format!("prompt {i}"))
            .collect();

        save_prompt_history_to_path(&path, "/proj/a", &entries).unwrap();

        let loaded = load_prompt_history_from_path(&path, "/proj/a");
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
        app.handle_term(CtEvent::Resize(80, 24), &agent_tx).await;
        assert!(
            app.active_selection.is_none(),
            "a resize must drop the now-stale selection"
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
        app.prompt_history.clear();
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

        app.prompt_history.clear();
        app.active_tab_mut().titled = true;
        app.active_tab_mut().turn_seq = 1;
        app.active_tab_mut().active_turn_id = 1;
        app.active_tab_mut().turn_abort = Some(AbortController::new());

        app.input.set_text("queued follow-up");
        send_key(&mut app, &agent_tx, KeyCode::Enter, KeyModifiers::NONE).await;

        assert_eq!(app.active_tab().queued_input.len(), 1);
        // A queued follow-up is never recorded into recall/prompt history.
        assert!(!app.prompt_history.iter().any(|p| p == "queued follow-up"));
        assert!(!app
            .active_tab()
            .chat
            .messages()
            .iter()
            .any(|msg| msg.role == Role::User && msg.text == "queued follow-up"));
        assert_eq!(count_store_user_text(&app, "queued follow-up"), 0);
        assert_eq!(app.active_tab().active_turn_id, 1);

        app.active_tab_mut().turn_abort = None;
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
}
