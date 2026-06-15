//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

use std::collections::VecDeque;
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
    Event as CtEvent, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures::StreamExt;
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

use crate::event::AppEvent;
use crate::tab::SessionTab;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::{Autocomplete, DynCmd};
use crate::ui::chat::{ChatRenderMeta, ChatSelection, ChatSelectionPoint, ChatView, ImagePreview};
use crate::ui::dialog::agents_dialog::{AgentKind, AgentRow, AgentsAction, AgentsDialog};
use crate::ui::dialog::connect::{ConnectAction, ConnectDialog, ConnectStage};
use crate::ui::dialog::mcp_dialog::McpDialog;
use crate::ui::dialog::permission::PermissionDialog;
use crate::ui::dialog::plugin_picker::PluginPicker;
use crate::ui::dialog::question::QuestionDialog;
use crate::ui::dialog::session_picker::SessionPicker;
use crate::ui::dialog::settings::{SettingsAction, SettingsDialog, SettingsLevel};
use crate::ui::dialog::tasks_panel::TasksPanel;
use crate::ui::dialog::workflows_dialog::{WorkflowRow, WorkflowsAction, WorkflowsDialog};
use crate::ui::input::InputBox;
use crate::ui::layout::{render_header, split_main, HeaderInfo};
use crate::ui::status::{Mode, StatusBar};
use crate::ui::tabs::{render_sidebar, SidebarInfo};
use crate::ui::toast::Toast;

const PROMPT_HISTORY_FILE: &str = "prompt_history.json";
const PROMPT_HISTORY_LIMIT: usize = 200;

fn prompt_history_path() -> Option<std::path::PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|dir| dir.join(PROMPT_HISTORY_FILE))
}

fn load_prompt_history() -> Vec<String> {
    prompt_history_path()
        .as_deref()
        .map(load_prompt_history_from_path)
        .unwrap_or_default()
}

fn save_prompt_history(history: &[String]) {
    let Some(path) = prompt_history_path() else {
        return;
    };
    if let Err(e) = save_prompt_history_to_path(&path, history) {
        tracing::warn!(error = %e, path = %path.display(), "failed to save prompt history");
    }
}

fn load_prompt_history_from_path(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<String>>(&text) {
        Ok(entries) => sanitize_prompt_history(entries),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to parse prompt history");
            Vec::new()
        }
    }
}

fn save_prompt_history_to_path(path: &Path, history: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let history = sanitize_prompt_history(history.to_vec());
    let json = serde_json::to_string_pretty(&history).map_err(|e| e.to_string())?;
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
    if text.is_empty() || history.last().map(String::as_str) == Some(text) {
        return false;
    }
    history.push(text.to_string());
    if history.len() > PROMPT_HISTORY_LIMIT {
        let excess = history.len() - PROMPT_HISTORY_LIMIT;
        history.drain(0..excess);
    }
    true
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
    /// User visibility preference for the right session sidebar.
    sidebar_visibility: SidebarVisibility,
    /// App-managed text selection. When enabled, zode captures mouse drag
    /// events so selecting can auto-scroll and copy to the system clipboard.
    selection_mode: bool,
    active_selection: Option<ChatSelection>,
    input: InputBox,
    pending_cursor_seq: Option<FragmentedCursorSeqState>,
    status: StatusBar,
    theme_store: ThemeStore,
    theme: Theme,
    should_quit: bool,
    /// Approval requests from gated tools (one dialog shown at a time).
    approval_rx: ApprovalReceiver,
    active_dialog: Option<PermissionDialog>,
    pending_requests: VecDeque<ApprovalRequest>,
    /// AskUserQuestion channel + its modal (parallel to the approval path).
    question_rx: QuestionReceiver,
    active_question: Option<QuestionDialog>,
    pending_questions: VecDeque<QuestionRequest>,
    autocomplete: Autocomplete,
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
    show_help: bool,
    toast: Option<Toast>,
    provider_names: Vec<String>,
    /// Chat display prefs (`/thinking`, `/tool-details`), persisted in config
    /// and applied to the active tab's chat each frame.
    show_thinking: bool,
    show_tool_details: bool,
    /// Submitted prompts, oldest first, for Up/Down recall in the input box.
    prompt_history: Vec<String>,
    /// Cursor into `prompt_history` while browsing (None = editing live text).
    history_pos: Option<usize>,
    /// The in-progress text saved when history browsing began, restored when
    /// the user pages back down past the newest entry.
    history_draft: String,
}

impl TuiApp {
    pub fn new(
        engine: ZodeEngine,
        template: EngineTemplate,
        ui: UiConfig,
        approval_rx: ApprovalReceiver,
        question_rx: QuestionReceiver,
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
            }
            if let Some(meta) = SessionIndex::load()
                .ok()
                .and_then(|i| i.find_prefix(id).cloned())
            {
                tab0.title = meta.title;
            }
        }

        // Read display prefs before `template` is moved into the struct.
        let show_thinking = template.show_thinking();
        let show_tool_details = template.show_tool_details();
        // Apply the configured UI language so the chrome renders localized.
        if let Some(lang) = template.language() {
            zode_core::i18n::set_language_code(lang);
        }

        Self {
            tabs: vec![tab0],
            active: 0,
            next_tab_id: 1,
            template,
            sidebar_visibility: SidebarVisibility::Auto,
            selection_mode: true,
            active_selection: None,
            input: InputBox::new(),
            pending_cursor_seq: None,
            status,
            theme_store,
            theme,
            should_quit: false,
            approval_rx,
            active_dialog: None,
            pending_requests: VecDeque::new(),
            question_rx,
            active_question: None,
            pending_questions: VecDeque::new(),
            autocomplete: Autocomplete::new(),
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
            show_help: false,
            toast: None,
            provider_names: ui.provider_names,
            show_thinking,
            show_tool_details,
            prompt_history: load_prompt_history(),
            history_pos: None,
            history_draft: String::new(),
        }
    }

    /// Record a submitted prompt for Up/Down recall (skips blanks and exact
    /// consecutive duplicates), and reset the browse cursor.
    fn record_prompt_history(&mut self, text: &str) {
        if record_prompt_history_entry(&mut self.prompt_history, text) {
            save_prompt_history(&self.prompt_history);
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

    fn active_tab(&self) -> &SessionTab {
        &self.tabs[self.active]
    }

    fn active_tab_mut(&mut self) -> &mut SessionTab {
        &mut self.tabs[self.active]
    }

    /// Open a fresh tab (Ctrl+T) with its own engine; focus it.
    async fn new_tab(&mut self) {
        let id = self.next_tab_id;
        match self.template.assemble_tab(None, Some(id.to_string())).await {
            Ok(engine) => {
                self.next_tab_id += 1;
                let session_id = Uuid::new_v4().simple().to_string();
                self.tabs
                    .push(SessionTab::new(id, Arc::new(engine), session_id));
                self.active = self.tabs.len() - 1;
                self.autocomplete.dismiss();
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!("new tab failed: {e}")));
            }
        }
    }

    /// Close the active tab (Ctrl+W). Aborts its in-flight turn first; closing
    /// the last tab quits.
    fn close_active_tab(&mut self) {
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
        self.autocomplete.dismiss();
    }

    /// Abort the active tab's in-flight turn, if any. Returns true when a turn
    /// was actually interrupted (false when the tab was already idle). Shared
    /// by Ctrl+C and Esc.
    fn interrupt_active_turn(&mut self) -> bool {
        let tab = &mut self.tabs[self.active];
        if let Some(abort) = tab.turn_abort.take() {
            abort.abort_with_reason("user interrupted");
            tab.active_turn_id = 0;
            tab.chat.end_turn();
            tab.chat.push_system("(interrupted)");
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
            self.autocomplete.dismiss();
        }
    }

    /// Cycle to the next tab (Ctrl+Tab), wrapping around.
    fn cycle_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
            self.autocomplete.dismiss();
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
            }
        }
        let cwd = self.active_tab().engine.cwd.clone();
        PermissionDialog::new(req, cwd)
    }

    /// Respond to the active permission prompt, then surface the next queued
    /// request (if any), focusing its source tab/cwd.
    fn answer_permission(&mut self, approval: Approval) {
        let responded = self
            .active_dialog
            .as_mut()
            .map(|d| d.answer(approval))
            .unwrap_or(false);
        if responded {
            let next = self.pending_requests.pop_front();
            self.active_dialog = next.map(|r| self.open_approval(r));
        }
    }

    /// Show a question modal, focusing the tab that asked (its `source` id) —
    /// but not while a permission dialog is up (which captures input on top and
    /// is about a different tab), so we don't disorient by switching away.
    fn open_question(&mut self, req: QuestionRequest) {
        if self.active_dialog.is_none() {
            if let Some(src) = req.source.as_deref().and_then(|s| s.parse::<usize>().ok()) {
                if let Some(pos) = self.tabs.iter().position(|t| t.id == src) {
                    self.active = pos;
                }
            }
        }
        self.active_question = Some(QuestionDialog::new(req));
    }

    /// Rebuild the active tab's engine from `template` (a model / provider /
    /// yolo hot-switch), carrying the conversation store + cwd over so the
    /// context survives. On failure the old engine stays in place. Refused
    /// mid-turn: the in-flight turn writes to the OLD engine's store, which the
    /// new engine wouldn't see — switch after the turn (or Ctrl+C).
    /// Returns true if the swap succeeded (callers commit template/status
    /// changes only then, so a refused/failed switch leaves no half-state).
    async fn reassemble_active(&mut self, template: EngineTemplate) -> bool {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info("can't switch during a turn — Ctrl+C first"));
            return false;
        }
        // Plan mode is per-tab: re-apply THIS tab's mode to whatever template a
        // caller passed (a model/provider/yolo swap must not drop or leak it).
        let template = template.with_plan_mode(self.active_tab().plan_mode);
        let (store, cwd, id) = {
            let tab = self.active_tab();
            let store = match tab.engine.store.lock() {
                Ok(s) => s.clone(),
                Err(_) => return false,
            };
            (store, tab.engine.cwd.clone(), tab.id)
        };
        match template.assemble_tab(Some(cwd), Some(id.to_string())).await {
            Ok(engine) => {
                let engine = engine.with_store(store);
                let model = engine.model.clone();
                self.active_tab_mut().engine = Arc::new(engine);
                self.status.model = model;
                self.refresh_dynamic_commands();
                true
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!("switch failed: {e}")));
                false
            }
        }
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
    async fn close_mcp_dialog(&mut self) {
        let Some(dialog) = self.mcp_dialog.take() else {
            return;
        };
        if dialog.is_dirty() {
            self.apply_plugins(dialog.disabled_ids(), dialog.all_ids())
                .await;
        }
    }

    async fn handle_mcp_dialog_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.close_mcp_dialog().await,
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
                        let state = if on { "enabled" } else { "disabled" };
                        self.toast = Some(Toast::info(format!("{name} {state} (esc to apply)")));
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
                let cwd = self.active_tab().engine.cwd.clone();
                let def = zode_core::workflows::load_workflow_defs(&cwd)
                    .into_iter()
                    .find(|w| w.name == name);
                match def {
                    Some(def) if !def.steps.is_empty() => {
                        // Deterministic execution: zode runs each step as its
                        // OWN sequential turn (queued so step N+1 only fires when
                        // step N's turn finishes — the model can't skip, reorder,
                        // or merge steps). Each step is directed at its sub-agent.
                        let n = def.steps.len();
                        for (i, s) in def.steps.iter().enumerate() {
                            let turn = format!(
                                "[workflow \"{name}\" — step {}/{n}] Use the `{}` sub-agent (via \
                                 the Task tool) for exactly this step, then report its result:\n\n{}",
                                i + 1,
                                s.agent_type,
                                s.prompt
                            );
                            self.active_tab_mut().queued_input.push_back(turn);
                        }
                        self.toast = Some(Toast::info(format!(
                            "running workflow '{name}' ({n} steps)"
                        )));
                        // Kick off step 1; the rest auto-chain as each turn ends.
                        self.dispatch_queued_input(agent_tx).await;
                    }
                    _ => self.toast = Some(Toast::error(format!("workflow '{name}' has no steps"))),
                }
            }
            Some(WorkflowsAction::AiCreate { brief }) => {
                self.workflows_dialog = None;
                let prompt = format!(
                    "Create a reusable workflow for me using the `define_workflow` tool. \
                     Here is what it should accomplish:\n\n{brief}\n\nBreak it into ordered \
                     steps, each with a fitting sub-agent type and a precise instruction, so \
                     the workflow runs the same way every time, then call define_workflow."
                );
                self.submit(&prompt, agent_tx).await;
            }
            Some(WorkflowsAction::Delete { name }) => {
                match zode_core::workflows::delete_workflow_def(&name) {
                    Ok(true) => {
                        let _ = self.reassemble_active(self.template.clone()).await;
                        self.toast = Some(Toast::info(format!("workflow deleted: {name}")));
                        self.workflows_dialog = Some(WorkflowsDialog::new(self.workflow_rows()));
                    }
                    Ok(false) => self.toast = Some(Toast::info(format!("{name} not found"))),
                    Err(e) => self.toast = Some(Toast::error(format!("delete failed: {e}"))),
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
            self.toast = Some(Toast::info("no saved sessions yet"));
            return;
        }
        self.session_picker = Some(SessionPicker::new(metas));
    }

    async fn handle_picker_key(&mut self, code: KeyCode) {
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
                    self.resume_session(meta).await;
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

    /// Resume a saved session in a new tab, replaying its history into the
    /// chat. If the session is already open, just focus that tab.
    async fn resume_session(&mut self, meta: SessionMeta) {
        if let Some(pos) = self.tabs.iter().position(|t| t.session_id == meta.id) {
            self.active = pos;
            self.autocomplete.dismiss();
            return;
        }
        let path = match SessionIndex::session_path(&meta.id) {
            Ok(p) => p,
            Err(_) => {
                self.toast = Some(Toast::error("bad session path"));
                return;
            }
        };
        let store = match Session::load(&path).await {
            Ok(s) => s,
            Err(e) => {
                self.toast = Some(Toast::error(format!("load failed: {e}")));
                return;
            }
        };
        let chat = rebuild_chat_from_store(&store);
        // Resume in the session's original directory when it still exists, so
        // tools operate in the right repo (not the launch cwd).
        let cwd_override = if std::path::Path::new(&meta.cwd).is_dir() {
            Some(std::path::PathBuf::from(&meta.cwd))
        } else {
            None
        };
        let id = self.next_tab_id;
        let engine = match self
            .template
            .assemble_tab(cwd_override, Some(id.to_string()))
            .await
        {
            Ok(e) => e.with_store(store),
            Err(e) => {
                self.toast = Some(Toast::error(format!("assemble failed: {e}")));
                return;
            }
        };
        self.next_tab_id += 1;
        let mut tab = SessionTab::new(id, Arc::new(engine), meta.id.clone());
        tab.title = meta.title.clone();
        tab.titled = true;
        tab.chat = chat;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.autocomplete.dismiss();
    }

    /// Delete a saved session's transcript file and index entry. Open tabs are
    /// untouched (they re-create the file on the next save). The index write
    /// goes through the shared lock so it can't race a concurrent save.
    async fn delete_session(&mut self, id: &str) {
        if let Ok(path) = SessionIndex::session_path(id) {
            let _ = std::fs::remove_file(path);
        }
        crate::tab::index_remove(id).await;
        self.toast = Some(Toast::info("session deleted"));
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
                            self.toast = Some(Toast::info(format!("killed {}", shell.shell_id)))
                        }
                        Err(e) => self.toast = Some(Toast::error(e.to_string())),
                    }
                    self.refresh_bg_shells().await;
                }
            }
            _ => {}
        }
    }

    pub async fn run(mut self) -> std::io::Result<()> {
        // Seed the autocomplete with the initial tab's agents/skills/MCP tools.
        self.refresh_dynamic_commands();
        let mut terminal = setup_terminal()?;
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
            terminal.draw(|f| self.draw(f))?;
            if self.should_quit {
                break;
            }

            tokio::select! {
                maybe_ev = term_events.next() => {
                    if let Some(Ok(ev)) = maybe_ev {
                        self.handle_term(ev, &agent_tx).await;
                        // Switching to a tab that has queued input (and is now
                        // idle) flushes it here, not just on its own turn-done.
                        self.dispatch_queued_input(&agent_tx).await;
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_agent_event(app_ev);
                    // A turn may have just finished — flush any queued input.
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
            // Plan mode is per-tab, so the badge always reflects the active tab.
            self.status.plan_mode = tab.plan_mode;
            self.status.selection_mode = self.selection_mode;
        }
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
            let mode = mode_label(self.status.mode);
            render_sidebar(
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
                },
                &theme,
            );
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
            render_pending_image_chips(
                f,
                chips_area,
                &self.tabs[self.active].pending_images,
                &theme,
            );
            input_area.y = input_area.y.saturating_add(1);
            input_area.height = input_area.height.saturating_sub(1);
        }
        let input_text = self.input.text();
        let completion_placeholder = self
            .completion_hint
            .as_ref()
            .and_then(|hint| (input_text == hint.prefix).then_some(hint.placeholder.as_str()));
        self.input.render(
            f,
            input_area,
            &theme,
            self.status.mode,
            completion_placeholder,
        );
        self.status.render(f, areas.status, &theme);
        // Autocomplete popup floats above the input row.
        self.autocomplete.render(f, input_area, &theme);
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
            self.handle_settings_key(key.code).await;
            return;
        }

        // 2a. Connect dialog captures provider search and API key entry.
        if self.connect.is_some() {
            self.handle_connect_key(key.code).await;
            return;
        }

        // 2a2. Plugin picker captures toggle + filter input.
        if self.plugin_picker.is_some() {
            self.handle_plugin_key(key.code).await;
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
            self.handle_mcp_dialog_key(key.code).await;
            return;
        }

        // 2b. Session picker captures input (typing filters the list).
        if self.session_picker.is_some() {
            self.handle_picker_key(key.code).await;
            return;
        }

        // 2c. Tasks panel captures input.
        if self.tasks_panel.is_some() {
            self.handle_tasks_panel_key(key.code).await;
            return;
        }

        // 3. Help overlay: Esc / F1 / q closes it.
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q')) {
                self.show_help = false;
            }
            return;
        }

        // 4. Global chords.
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                // Clear a prompt draft first; with an empty prompt, interrupt a
                // running turn or quit when idle.
                if !self.input.is_empty() {
                    self.input.take();
                    self.completion_hint = None;
                    self.autocomplete.dismiss();
                    self.history_pos = None;
                    self.history_draft.clear();
                    return;
                }
                if !self.interrupt_active_turn() {
                    self.should_quit = true;
                }
                return;
            }
            // Esc interrupts a running turn. An open autocomplete popup gets
            // Esc first (to dismiss) — that's handled later, so only steal Esc
            // here when the popup is closed and a turn is actually in flight.
            (KeyCode::Esc, _)
                if self.tabs[self.active].is_busy() && !self.autocomplete.is_active() =>
            {
                self.interrupt_active_turn();
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
                return;
            }
            (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                match zode_core::clipboard::read_from_clipboard() {
                    Ok(text) => self.handle_paste(&text),
                    Err(e) => self.toast = Some(Toast::error(format!("paste failed: {e}"))),
                }
                return;
            }
            // App chords use the platform primary modifier: Cmd (⌘) on macOS,
            // Ctrl elsewhere (see `is_primary_mod`).
            (KeyCode::Char('o'), m) if is_primary_mod(m) => {
                self.open_settings();
                return;
            }
            (KeyCode::Char('t'), m) if is_primary_mod(m) => {
                self.new_tab().await;
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
                    self.open_connect_dialog();
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
                self.completion_hint = None;
                self.autocomplete.dismiss();
                if !text.trim().is_empty() {
                    self.record_prompt_history(&text);
                    self.submit(&text, agent_tx).await;
                }
            }
            (KeyCode::Enter, _) => self.input.insert_newline(),
            (KeyCode::Up, m) if m.is_empty() && self.input.cursor_on_first_line() => {
                self.history_prev();
            }
            (KeyCode::Down, m) if m.is_empty() && self.input.cursor_on_last_line() => {
                self.history_next();
            }
            _ => {
                self.input.input(key);
                // Editing the text exits history-browse mode.
                self.history_pos = None;
            }
        }
        // 7. Refresh the autocomplete popup from the new input text.
        self.autocomplete.update(&self.input.text());
    }

    fn handle_paste(&mut self, text: &str) {
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
            || self.show_help
        {
            return;
        }

        let cwd = self.active_tab().engine.cwd.clone();
        match split_pasted_image_paths(&cwd, text) {
            Ok(parsed) => {
                let image_count = parsed.images.len();
                if image_count > 0 {
                    self.active_tab_mut().pending_images.extend(parsed.images);
                    self.toast = Some(Toast::info(format!(
                        "attached {image_count} image{}",
                        if image_count == 1 { "" } else { "s" }
                    )));
                }
                if !parsed.remaining_text.is_empty() {
                    self.input.insert_str(&parsed.remaining_text);
                }
                self.autocomplete.update(&self.input.text());
            }
            Err(e) => {
                self.toast = Some(Toast::error(e.to_string()));
            }
        }
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

        if self.active_dialog.is_some()
            || self.active_question.is_some()
            || self.settings.is_some()
            || self.connect.is_some()
            || self.plugin_picker.is_some()
            || self.agents_dialog.is_some()
            || self.workflows_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.tasks_panel.is_some()
            || self.show_help
        {
            return;
        }

        let Ok((width, height)) = crossterm::terminal::size() else {
            return;
        };
        let area = Rect::new(0, 0, width, height);
        let show_sidebar = should_show_sidebar(self.tabs.len(), self.sidebar_visibility);
        let areas = split_main(area, show_sidebar);

        if self.selection_mode && self.handle_selection_mouse(mouse, areas.chat) {
            return;
        }

        match chat_scroll_from_mouse(mouse.kind, mouse.column, mouse.row, areas.chat) {
            Some(ChatMouseScroll::Up(n)) => self.tabs[self.active].chat.scroll_up(n),
            Some(ChatMouseScroll::Down(n)) => self.tabs[self.active].chat.scroll_down(n),
            None => {}
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
                self.copy_chat_selection(selection, chat_area);
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
            return;
        }
        match zode_core::clipboard::copy_to_clipboard(&text) {
            Ok(_) => self.toast = Some(Toast::info("copied selection to clipboard")),
            Err(e) => self.toast = Some(Toast::error(format!("copy failed: {e}"))),
        }
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

    fn open_connect_dialog(&mut self) {
        self.connect = Some(ConnectDialog::new());
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
                        let _ = self.reassemble_active(self.template.clone()).await;
                        self.toast = Some(Toast::info(format!("agent created: {name}")));
                    }
                    Err(e) => self.toast = Some(Toast::error(format!("create failed: {e}"))),
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
                        let _ = self.reassemble_active(self.template.clone()).await;
                        self.toast = Some(Toast::info(format!("agent deleted: {name}")));
                        // Rebuild the dialog's rows to drop the deleted entry.
                        self.agents_dialog = Some(AgentsDialog::new(self.agent_rows()));
                    }
                    Ok(false) => {
                        self.toast =
                            Some(Toast::info(format!("{name} is built-in (not deletable)")))
                    }
                    Err(e) => self.toast = Some(Toast::error(format!("delete failed: {e}"))),
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

    async fn handle_settings_key(&mut self, code: KeyCode) {
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
            self.apply_settings(action).await;
        }
    }

    async fn handle_connect_key(&mut self, code: KeyCode) {
        let action = {
            let Some(dialog) = &mut self.connect else {
                return;
            };
            match code {
                KeyCode::Esc => {
                    self.connect = None;
                    None
                }
                KeyCode::Up if dialog.stage() == ConnectStage::Provider => {
                    dialog.prev();
                    None
                }
                KeyCode::Down if dialog.stage() == ConnectStage::Provider => {
                    dialog.next();
                    None
                }
                KeyCode::Backspace if dialog.stage() == ConnectStage::Provider => {
                    dialog.pop_filter_char();
                    None
                }
                KeyCode::Backspace if dialog.stage() == ConnectStage::ApiKey => {
                    dialog.pop_api_key_char();
                    None
                }
                KeyCode::Enter => dialog.confirm(),
                KeyCode::Char(c) if dialog.stage() == ConnectStage::Provider => {
                    dialog.push_filter_char(c);
                    None
                }
                KeyCode::Char(c) if dialog.stage() == ConnectStage::ApiKey => {
                    dialog.push_api_key_char(c);
                    None
                }
                _ => None,
            }
        };

        if let Some(action) = action {
            self.connect = None;
            self.apply_connect(action).await;
        }
    }

    async fn apply_settings(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::SetTheme(id) => {
                self.theme = self.theme_store.resolve(Some(&id));
                if let Ok(mut cfg) = ConfigManager::load_global() {
                    cfg.theme = Some(id.clone());
                    let _ = ConfigManager::save_global(&cfg);
                }
                self.toast = Some(Toast::info(format!("theme → {id}")));
            }
            SettingsAction::SetModel(id) => self.apply_model(&id).await,
            SettingsAction::SetProvider(name) => {
                // Real hot switch: reassemble the active tab from the named
                // provider, carrying the conversation over. Commit only on
                // success (else the template/status would drift from reality).
                match self.template.with_provider(&name) {
                    Some(t) => {
                        if self.reassemble_active(t.clone()).await {
                            self.template = t;
                            self.toast = Some(Toast::info(format!("provider → {name}")));
                        }
                    }
                    None => {
                        self.toast = Some(Toast::error(format!("no provider '{name}' in config")));
                    }
                }
            }
            SettingsAction::SetMode(m) => {
                // Map the approval mode to yolo: "dontAsk" auto-approves.
                let yolo = m == "dontAsk";
                let t = self.template.with_yolo(yolo);
                if self.reassemble_active(t.clone()).await {
                    self.template = t;
                    self.status.yolo = yolo;
                    self.toast = Some(Toast::info(format!("mode → {m}")));
                }
            }
            SettingsAction::SetEffort(level) => {
                let t = self.template.with_effort(Some(level.clone()));
                if self.reassemble_active(t.clone()).await {
                    self.template = t;
                    self.toast = Some(Toast::info(format!("effort → {level}")));
                }
            }
            SettingsAction::SetSidebar(choice) => {
                self.sidebar_visibility = match choice.as_str() {
                    "visible" => SidebarVisibility::Visible,
                    "hidden" => SidebarVisibility::Hidden,
                    _ => SidebarVisibility::Auto,
                };
                self.toast = Some(Toast::info(format!("sidebar → {choice}")));
            }
            SettingsAction::SetThinking(choice) => {
                self.show_thinking = choice == "on";
                self.persist_show_thinking(self.show_thinking);
                self.toast = Some(Toast::info(format!(
                    "thinking output {}",
                    on_off(self.show_thinking)
                )));
            }
            SettingsAction::SetToolDetails(choice) => {
                self.show_tool_details = choice == "on";
                self.persist_show_tool_details(self.show_tool_details);
                self.toast = Some(Toast::info(format!(
                    "tool details {}",
                    on_off(self.show_tool_details)
                )));
            }
            SettingsAction::SetOrchestration(choice) => {
                let on = choice == "on";
                let t = self.template.with_autonomous_orchestration(on);
                if self.reassemble_active(t.clone()).await {
                    self.template = t;
                    if let Ok(mut cfg) = ConfigManager::load_global() {
                        cfg.autonomous_orchestration = Some(on);
                        let _ = ConfigManager::save_global(&cfg);
                    }
                    self.toast = Some(Toast::info(format!(
                        "autonomous orchestration {}",
                        on_off(on)
                    )));
                }
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
                    self.toast = Some(Toast::info(format!("language → {name}")));
                }
            }
        }
    }

    async fn apply_connect(&mut self, action: ConnectAction) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(
                "can't switch provider during a turn - Ctrl+C first",
            ));
            return;
        }

        let mut cfg = match ConfigManager::load_global() {
            Ok(cfg) => cfg,
            Err(e) => {
                self.toast = Some(Toast::error(format!("load config failed: {e}")));
                return;
            }
        };
        cfg.provider = action.provider.clone();
        if let Err(e) = ConfigManager::save_global(&cfg) {
            self.toast = Some(Toast::error(format!("save config failed: {e}")));
            return;
        }

        let provider_name = action.name;
        let t = self.template.with_provider_config(action.provider);
        if self.reassemble_active(t.clone()).await {
            self.template = t;
            self.toast = Some(Toast::info(format!("provider -> {provider_name}")));
            self.active_tab_mut()
                .chat
                .push_system(&format!("provider -> {provider_name}"));
        }
    }

    /// Drive the plugin picker. Space/Enter flips the selected plugin in place;
    /// Esc closes and, if anything changed, persists the new disabled set and
    /// reassembles the active tab once so it takes effect live.
    async fn handle_plugin_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                let Some(picker) = self.plugin_picker.take() else {
                    return;
                };
                if picker.is_dirty() {
                    self.apply_plugins(picker.disabled_ids(), picker.all_ids())
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
                    let state = if on { "on" } else { "off" };
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
    async fn apply_plugins(&mut self, disabled: Vec<String>, owned: Vec<String>) {
        if self.active_tab().is_busy() {
            self.toast = Some(Toast::info(
                "can't change plugins during a turn — Ctrl+C first",
            ));
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
                    self.toast = Some(Toast::error(format!("save config failed: {e}")));
                    return;
                }
                next
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!("load config failed: {e}")));
                return;
            }
        };
        let t = self.template.with_plugins_disabled(merged);
        if self.reassemble_active(t.clone()).await {
            self.template = t;
            self.toast = Some(Toast::info("plugins updated"));
        }
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

    async fn apply_model(&mut self, id: &str) {
        let t = self.template.with_model(id.to_string());
        if self.reassemble_active(t.clone()).await {
            self.template = t;
            self.active_tab_mut()
                .chat
                .push_system(&format!("model → {id}"));
        }
    }

    /// When the active tab goes idle, send the next queued message (one per
    /// turn, FIFO). Called after each agent event, so it fires as soon as a
    /// turn's `TurnDone` clears the busy flag.
    async fn dispatch_queued_input(&mut self, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if self.active_tab().is_busy() {
            return;
        }
        let next = self.active_tab_mut().queued_input.pop_front();
        if let Some(text) = next {
            self.submit(&text, agent_tx).await;
        }
    }

    async fn submit(&mut self, text: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let cwd = self.active_tab().engine.cwd.clone();
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
        // when this tab goes idle — see `dispatch_queued_input`.
        if self.active_tab().is_busy() {
            if !submitted_text.trim().is_empty() {
                self.active_tab_mut()
                    .queued_input
                    .push_back(submitted_text.to_string());
                let n = self.active_tab().queued_input.len();
                self.toast = Some(Toast::info(format!(
                    "queued ({n}) — sends when the turn finishes (Esc to interrupt now)"
                )));
            } else if pasted_count > 0 {
                self.toast = Some(Toast::info(format!(
                    "attached {pasted_count} image{}",
                    if pasted_count == 1 { "" } else { "s" }
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
        let vision_engine = match image_route {
            ImageSubmitRoute::Direct => None,
            ImageSubmitRoute::Unsupported => {
                if has_images {
                    self.toast = Some(Toast::error(
                        "current provider does not declare image support; set supportsImages=true or configure /vision provider <name>",
                    ));
                    return;
                }
                None
            }
            ImageSubmitRoute::VisionModel => {
                let Some(provider_name) = images_cfg.vision_provider.as_deref() else {
                    self.toast = Some(Toast::error("configure /vision provider <name> first"));
                    return;
                };
                let Some(template) = self.template.with_provider(provider_name) else {
                    self.toast = Some(Toast::error(format!(
                        "vision provider '{provider_name}' is not configured"
                    )));
                    return;
                };
                match template
                    .assemble_tab(
                        Some(self.active_tab().engine.cwd.clone()),
                        Some(format!("{}:vision", self.active_tab().id)),
                    )
                    .await
                {
                    Ok(engine) if engine.supports_images() => Some(Arc::new(engine)),
                    Ok(_) => {
                        self.toast = Some(Toast::error(format!(
                            "vision provider '{provider_name}' does not declare image support"
                        )));
                        return;
                    }
                    Err(e) => {
                        self.toast = Some(Toast::error(format!("vision provider failed: {e}")));
                        return;
                    }
                }
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
            self.active_tab_mut().stamp_title(&title_source).await;
        }

        let tab = &mut self.tabs[self.active];
        let images = std::mem::take(&mut tab.pending_images);
        let previews = image_previews(&images);
        let content = user_content_blocks(&submitted_text, &images);
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
            let stream_result = if let Some(vision_engine) = vision_engine {
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
        // Toasts (from off-loop work) carry no tab/turn id.
        if let AppEvent::Toast { text, error } = ev {
            self.toast = Some(if error {
                Toast::error(text)
            } else {
                Toast::info(text)
            });
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
            AppEvent::Toast { .. } => unreachable!("handled above"),
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
                        ..
                    } => {
                        tab.input_tokens = tab.input_tokens.saturating_add(input_tokens);
                        tab.output_tokens = tab.output_tokens.saturating_add(output_tokens);
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                    Event::Notice { .. } | Event::Result { .. } | Event::Unknown => {
                        if let Some(line) = process_line_for_event(&event, None) {
                            tab.chat.push_tool(&line);
                        }
                    }
                    Event::Error { code, message } => {
                        tab.chat.push_system(&format!("error [{code}]: {message}"));
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
                tab.mode = match result {
                    Ok(()) => Mode::Ready,
                    Err(e) => {
                        tab.chat.push_system(&format!("turn failed: {e}"));
                        Mode::Error
                    }
                };
                // Persist the session off the event loop.
                let (session_id, engine, title) = (
                    tab.session_id.clone(),
                    tab.engine.clone(),
                    tab.title.clone(),
                );
                tokio::spawn(crate::tab::persist_session(session_id, engine, title));
            }
            AppEvent::Toast { .. } => unreachable!("handled above"),
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
                    self.toast = Some(Toast::info("can't clear during a turn — Ctrl+C first"));
                } else {
                    let tab = &mut self.tabs[self.active];
                    tab.chat = ChatView::new();
                    if let Ok(mut store) = tab.engine.store.lock() {
                        *store = agent::message::MessageStore::new();
                    }
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
            "sessions" | "resume" => self.open_session_picker(),
            "tab" => self.handle_tab_command(args),
            "connect" => self.open_connect_dialog(),
            "plugin" => self.open_plugin_picker(),
            "vision" => self.handle_vision(args),
            "selection" => self.handle_selection_command(args),
            "sidebar" => {
                if args.trim().is_empty() {
                    self.open_sidebar_picker();
                } else {
                    self.handle_sidebar_command(args);
                }
            }
            "tasks" => self.open_tasks_panel().await,
            "config" => {
                let msg = format!(
                    "model={} cwd={}",
                    self.active_tab().engine.model,
                    self.active_tab().engine.cwd.display()
                );
                self.active_tab_mut().chat.push_system(&msg);
            }
            "compact" => {
                self.active_tab_mut()
                    .chat
                    .push_system("(auto-compaction is enabled; manual /compact lands later)");
            }
            "model" => {
                if args.is_empty() {
                    self.open_model_picker();
                } else {
                    self.apply_model(args).await;
                }
            }
            "yolo" => {
                let on = !self.template.yolo();
                let t = self.template.with_yolo(on);
                if self.reassemble_active(t.clone()).await {
                    self.template = t;
                    self.status.yolo = on;
                    self.active_tab_mut().chat.push_system(if on {
                        "yolo: ON — tools auto-approve (deny rules still apply)"
                    } else {
                        "yolo: OFF — tools prompt for approval"
                    });
                }
            }
            "plan" => {
                // Per-tab: flip THIS tab's flag, then reassemble (which re-applies
                // it). The status badge syncs from the active tab on render.
                let on = !self.active_tab().plan_mode;
                self.active_tab_mut().plan_mode = on;
                if self.reassemble_active(self.template.clone()).await {
                    self.active_tab_mut().chat.push_system(if on {
                        "plan mode: ON — read-only tools only; research and present a plan, then /plan to execute"
                    } else {
                        "plan mode: OFF — full tools restored"
                    });
                } else {
                    // Reassembly refused (busy) — revert the flag.
                    self.active_tab_mut().plan_mode = !on;
                }
            }
            "mcp" => self.open_mcp_dialog(),
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
                    self.active_tab_mut().chat.push_system("(no skills loaded)");
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
                        Some(g) => format!("current goal: {g}\n(clear with /goal clear)"),
                        None => "no goal set — use /goal <text> to set one".to_string(),
                    };
                    self.active_tab_mut().chat.push_system(&msg);
                } else {
                    // "clear"/"none" wipes the goal; anything else sets it.
                    let new_goal = (!trimmed.eq_ignore_ascii_case("clear")
                        && !trimmed.eq_ignore_ascii_case("none"))
                    .then(|| trimmed.to_string());
                    let t = self.template.with_goal(new_goal.clone());
                    if self.reassemble_active(t.clone()).await {
                        self.template = t;
                        let msg = match &new_goal {
                            Some(g) => format!("goal set: {g}"),
                            None => "goal cleared".to_string(),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
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
                    self.toast = Some(Toast::info("usage: /effort low|medium|high"));
                } else {
                    let new_effort =
                        matches!(level.as_str(), "low" | "medium" | "high").then(|| level.clone());
                    let t = self.template.with_effort(new_effort.clone());
                    if self.reassemble_active(t.clone()).await {
                        self.template = t;
                        let msg = match &new_effort {
                            Some(e) => format!("effort set: {e}"),
                            None => "effort reset to medium (default)".to_string(),
                        };
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                }
            }
            "copy" => match self.active_tab().engine.last_assistant_text() {
                Some(text) => match zode_core::clipboard::copy_to_clipboard(&text) {
                    Ok(_) => self.toast = Some(Toast::info("copied last response to clipboard")),
                    Err(e) => self.toast = Some(Toast::error(format!("copy failed: {e}"))),
                },
                None => self.toast = Some(Toast::info("nothing to copy yet")),
            },
            "export" => {
                let path =
                    zode_core::export::resolve_export_path(&self.active_tab().engine.cwd, args);
                let md = self.active_tab().engine.export_markdown();
                match std::fs::write(&path, md) {
                    Ok(()) => {
                        let msg = format!("exported conversation to {}", path.display());
                        self.active_tab_mut().chat.push_system(&msg);
                    }
                    Err(e) => self.toast = Some(Toast::error(format!("export failed: {e}"))),
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
                        .push_system("(no hooks configured)");
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
                        if self.reassemble_active(t.clone()).await {
                            self.template = t;
                            self.active_tab_mut().chat.push_system(
                                "reloaded — tools, MCP, skills, and LSP re-discovered",
                            );
                        }
                    }
                    Err(e) => self.toast = Some(Toast::error(format!("reload failed: {e}"))),
                }
            }
            "reload-skills" => {
                if self.reassemble_active(self.template.clone()).await {
                    let n = self.active_tab().engine.skills.list().len();
                    let msg = format!("reloaded skills ({n} loaded)");
                    self.active_tab_mut().chat.push_system(&msg);
                }
            }
            "language" => self.open_language_picker(),
            "orchestration" => {
                let on = !self.template.autonomous_orchestration();
                let t = self.template.with_autonomous_orchestration(on);
                if self.reassemble_active(t.clone()).await {
                    self.template = t;
                    if let Ok(mut cfg) = ConfigManager::load_global() {
                        cfg.autonomous_orchestration = Some(on);
                        let _ = ConfigManager::save_global(&cfg);
                    }
                    self.active_tab_mut().chat.push_system(if on {
                        "autonomous orchestration: ON — the agent may decompose tasks, spawn sub-agents, and define new ones"
                    } else {
                        "autonomous orchestration: OFF"
                    });
                }
            }
            "thinking" => {
                self.show_thinking = !self.show_thinking;
                self.persist_show_thinking(self.show_thinking);
                self.toast = Some(Toast::info(format!(
                    "thinking output {}",
                    on_off(self.show_thinking)
                )));
            }
            "tool-details" => {
                self.show_tool_details = !self.show_tool_details;
                self.persist_show_tool_details(self.show_tool_details);
                self.toast = Some(Toast::info(format!(
                    "tool details {}",
                    on_off(self.show_tool_details)
                )));
            }
            other => {
                self.toast = Some(Toast::info(format!("/{other} lands in a later phase")));
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
                .push_system(&format!("theme → {args}"));
        } else {
            self.active_tab_mut()
                .chat
                .push_system(&format!("unknown theme: {args}"));
        }
    }

    fn handle_vision(&mut self, args: &str) {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            let msg = vision_summary(
                self.template.images(),
                self.active_tab().engine.supports_images(),
            );
            self.active_tab_mut().chat.push_system(&msg);
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
                    format!("vision mode -> {}", image_mode_label(mode))
                }
                None => {
                    self.toast = Some(Toast::info("usage: /vision mode auto|direct|vision-model"));
                    return;
                }
            },
            "provider" => {
                if value.is_empty() {
                    let providers = self.template.provider_names();
                    let msg = if providers.is_empty() {
                        "no named providers configured".to_string()
                    } else {
                        format!("vision providers: {}", providers.join(", "))
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
                    self.toast = Some(Toast::error(format!("no provider '{value}' in config")));
                    return;
                }
                images.vision_provider = Some(value.to_string());
                images.mode = Some(ImageMode::VisionModel);
                format!("vision provider -> {value}")
            }
            "prompt" => {
                if value.is_empty() {
                    self.toast = Some(Toast::info("usage: /vision prompt <text>"));
                    return;
                }
                images.vision_prompt = Some(value.to_string());
                "vision prompt updated".to_string()
            }
            "clear" | "reset" => {
                images = ImagesConfig::default();
                "vision config reset".to_string()
            }
            _ => {
                self.toast = Some(Toast::info("usage: /vision [mode|provider|prompt|reset]"));
                return;
            }
        };

        match ConfigManager::load_global() {
            Ok(mut cfg) => {
                cfg.images = images.clone();
                if let Err(e) = ConfigManager::save_global(&cfg) {
                    self.toast = Some(Toast::error(format!("save config failed: {e}")));
                    return;
                }
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!("load config failed: {e}")));
                return;
            }
        }

        self.template = self.template.with_images_config(images);
        self.active_tab_mut().chat.push_system(&message);
    }

    fn handle_tab_command(&mut self, args: &str) {
        match resolve_tab_target(args, self.active, self.tabs.len()) {
            Ok(idx) => self.switch_to(idx),
            Err(msg) => self.active_tab_mut().chat.push_system(&msg),
        }
    }

    fn handle_sidebar_command(&mut self, args: &str) {
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
                    .push_system(&format!("sidebar -> {state}"));
            }
            Err(msg) => self.active_tab_mut().chat.push_system(&msg),
        }
    }

    fn handle_selection_command(&mut self, args: &str) {
        match resolve_selection_mode(args, self.selection_mode) {
            Ok(enabled) => self.set_selection_mode(enabled),
            Err(msg) => self.active_tab_mut().chat.push_system(&msg),
        }
    }

    fn set_selection_mode(&mut self, enabled: bool) {
        if self.selection_mode == enabled {
            self.toast = Some(Toast::info(format!(
                "selection mode {}",
                on_off(self.selection_mode)
            )));
            return;
        }
        self.selection_mode = enabled;
        self.active_selection = None;
        let mut stdout = std::io::stdout();
        let result = if enabled {
            stdout.execute(EnableMouseCapture)
        } else {
            stdout.execute(DisableMouseCapture)
        };
        match result {
            Ok(_) => {
                self.toast = Some(Toast::info(format!("selection mode {}", on_off(enabled))));
            }
            Err(e) => {
                self.selection_mode = !enabled;
                self.toast = Some(Toast::error(format!("selection mode failed: {e}")));
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FragmentedCursorSeqState {
    AfterEsc,
    AfterEscO,
    AfterEscBracket,
    MaybeBareO { count: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FragmentedCursorAction {
    None,
    ReplayBareO(usize),
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

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Ready => "ready",
        Mode::Thinking => "thinking",
        Mode::Streaming => "streaming",
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
    if !modifiers.is_empty() {
        if let Some(FragmentedCursorSeqState::MaybeBareO { count }) = *state {
            *state = None;
            return FragmentedCursorAction::ReplayBareO(count);
        }
        *state = None;
        return FragmentedCursorAction::None;
    }

    match *state {
        Some(FragmentedCursorSeqState::AfterEsc) => match code {
            KeyCode::Char('O') => {
                *state = Some(FragmentedCursorSeqState::AfterEscO);
                FragmentedCursorAction::Consumed
            }
            KeyCode::Char('[') => {
                *state = Some(FragmentedCursorSeqState::AfterEscBracket);
                FragmentedCursorAction::Consumed
            }
            _ => {
                *state = None;
                FragmentedCursorAction::None
            }
        },
        Some(FragmentedCursorSeqState::MaybeBareO { count }) => {
            *state = None;
            match code {
                KeyCode::Up => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Down => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Char('A') => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Char('B') => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Char('C') | KeyCode::Char('D') => FragmentedCursorAction::Consumed,
                KeyCode::Char('O') if input_is_empty => {
                    *state = Some(FragmentedCursorSeqState::MaybeBareO {
                        count: count.saturating_add(1),
                    });
                    FragmentedCursorAction::Consumed
                }
                _ => FragmentedCursorAction::ReplayBareO(count),
            }
        }
        Some(FragmentedCursorSeqState::AfterEscO)
        | Some(FragmentedCursorSeqState::AfterEscBracket) => {
            *state = None;
            match code {
                KeyCode::Char('A') => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Up(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Char('B') => {
                    FragmentedCursorAction::Scroll(ChatMouseScroll::Down(CHAT_WHEEL_SCROLL_LINES))
                }
                KeyCode::Char('C') | KeyCode::Char('D') => FragmentedCursorAction::Consumed,
                _ => FragmentedCursorAction::None,
            }
        }
        None => match code {
            KeyCode::Esc => {
                *state = Some(FragmentedCursorSeqState::AfterEsc);
                FragmentedCursorAction::Consumed
            }
            KeyCode::Char('O') if input_is_empty => {
                *state = Some(FragmentedCursorSeqState::MaybeBareO { count: 1 });
                FragmentedCursorAction::Consumed
            }
            _ => FragmentedCursorAction::None,
        },
    }
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
        Event::ToolResult { ok, .. } => {
            let status = if *ok { "done" } else { "failed" };
            Some(format!("{} {status}", tool_result_title(known_tool)))
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
        _ => Err("usage: /sidebar [on|off|toggle|auto]".to_string()),
    }
}

fn resolve_selection_mode(args: &str, current: bool) -> Result<bool, String> {
    match args.trim().to_ascii_lowercase().as_str() {
        "" | "toggle" => Ok(!current),
        "on" | "show" | "open" | "enable" | "enabled" => Ok(true),
        "off" | "hide" | "close" | "disable" | "disabled" => Ok(false),
        _ => Err("usage: /selection [on|off|toggle]".to_string()),
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

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn render_pending_image_chips(
    f: &mut ratatui::Frame,
    area: Rect,
    images: &[ImageAttachment],
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut spans = vec![Span::styled(
        "▣ ",
        Style::default()
            .bg(theme.bg_input)
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    for (idx, image) in images.iter().take(4).enumerate() {
        if idx > 0 {
            spans.push(Span::styled(
                "  ",
                Style::default().bg(theme.bg_input).fg(theme.fg_subtle),
            ));
        }
        spans.push(Span::styled(
            image.display_name.clone(),
            Style::default()
                .bg(theme.bg_input)
                .fg(theme.fg_white)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {}", image.media_type),
            Style::default().bg(theme.bg_input).fg(theme.fg_subtle),
        ));
    }
    if images.len() > 4 {
        spans.push(Span::styled(
            format!("  +{}", images.len() - 4),
            Style::default()
                .bg(theme.bg_input)
                .fg(theme.accent_secondary),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg_input)),
        area,
    );
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
        .turn_blocks(blocks, abort)
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

fn setup_terminal() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    // Undo raw mode if any subsequent step fails, so we never leave the
    // terminal in a broken state on a setup error.
    if let Err(e) = stdout.execute(EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    if let Err(e) = stdout.execute(EnableMouseCapture) {
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        return Err(e);
    }
    if let Err(e) = stdout.execute(EnableBracketedPaste) {
        let _ = stdout.execute(DisableMouseCapture);
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
        return Err(e);
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

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    disable_raw_mode()?;
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
        let _ = std::io::stdout().execute(DisableBracketedPaste);
        let _ = std::io::stdout().execute(DisableMouseCapture);
        let _ = std::io::stdout().execute(LeaveAlternateScreen);
        original(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::config::{ProviderConfig, ProviderKind, ZodeConfig};

    async fn make_test_app() -> (TuiApp, mpsc::UnboundedSender<AppEvent>) {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().to_path_buf();
        let cfg = ZodeConfig {
            provider: ProviderConfig {
                r#type: Some(ProviderKind::Ollama),
                base_url: Some("http://localhost:11434".to_string()),
                model: Some("test-model".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let (approval_queue, approval_rx) = zode_core::approval::approval_queue();
        let (question_queue, question_rx) = zode_core::question::question_queue();
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
            },
            approval_rx,
            question_rx,
            None,
        );
        let (agent_tx, _agent_rx) = mpsc::unbounded_channel::<AppEvent>();
        (app, agent_tx)
    }

    #[test]
    fn sidebar_is_hidden_until_multiple_tabs_exist() {
        assert!(!should_show_sidebar(0, SidebarVisibility::Auto));
        assert!(!should_show_sidebar(1, SidebarVisibility::Auto));
        assert!(should_show_sidebar(2, SidebarVisibility::Auto));
        assert!(should_show_sidebar(1, SidebarVisibility::Visible));
        assert!(!should_show_sidebar(2, SidebarVisibility::Hidden));
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
            Err("usage: /sidebar [on|off|toggle|auto]".to_string())
        );
    }

    #[test]
    fn selection_command_resolves_mode_targets() {
        assert_eq!(resolve_selection_mode("", false), Ok(true));
        assert_eq!(resolve_selection_mode("toggle", true), Ok(false));
        assert_eq!(resolve_selection_mode("on", false), Ok(true));
        assert_eq!(resolve_selection_mode("enable", false), Ok(true));
        assert_eq!(resolve_selection_mode("off", true), Ok(false));
        assert_eq!(resolve_selection_mode("disable", true), Ok(false));
        assert_eq!(
            resolve_selection_mode("wat", false),
            Err("usage: /selection [on|off|toggle]".to_string())
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
        save_prompt_history_to_path(&path, &history).unwrap();

        assert_eq!(
            load_prompt_history_from_path(&path),
            vec!["first prompt".to_string(), "second prompt".to_string()]
        );
    }

    #[test]
    fn tui_initialization_loads_local_prompt_history() {
        let source = include_str!("app.rs");
        let init = source
            .split("pub fn new(")
            .nth(1)
            .and_then(|tail| tail.split("history_pos: None").next())
            .expect("TuiApp::new initialization block should exist");
        assert!(init.contains("prompt_history: load_prompt_history()"));
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

        app.handle_term(
            CtEvent::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )),
            &agent_tx,
        )
        .await;

        assert_eq!(app.input.text(), "");
        assert!(!app.should_quit);

        app.handle_term(
            CtEvent::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )),
            &agent_tx,
        )
        .await;

        assert!(app.should_quit);
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

    #[test]
    fn setup_uses_mouse_capture_without_scroll_key_emulation() {
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
        assert!(setup.contains(concat!("Enable", "Mouse", "Capture")));
        assert!(source.contains(concat!("Disable", "Mouse", "Capture")));
    }

    #[test]
    fn app_managed_selection_defaults_on_with_mouse_capture() {
        let source = include_str!("app.rs");
        let init = source
            .split("pub fn new(")
            .nth(1)
            .and_then(|tail| tail.split("input: InputBox::new()").next())
            .expect("TuiApp::new initialization block should exist");
        assert!(init.contains("selection_mode: true"));
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
                    ContentBlock::Text {
                        text: "I wrote hello.rs.".into(),
                    },
                    ContentBlock::Thinking {
                        thinking: "The user asked for a file.".into(),
                        signature: None,
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
