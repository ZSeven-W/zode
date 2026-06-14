//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

use std::collections::VecDeque;
use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::message::{ContentBlock, Message, MessageStore};
use agent::session::Session;
use agent::stream::Event;
use crossterm::event::{Event as CtEvent, EventStream, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use tokio::sync::mpsc;
use uuid::Uuid;
use zode_core::approval::{ApprovalReceiver, ApprovalRequest};
use zode_core::bg_shells::BgShell;
use zode_core::commands::parse_slash;
use zode_core::config::ConfigManager;
use zode_core::question::{QuestionReceiver, QuestionRequest};
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::{EngineTemplate, ZodeEngine};

use crate::event::AppEvent;
use crate::tab::SessionTab;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::Autocomplete;
use crate::ui::chat::{ChatRenderMeta, ChatView};
use crate::ui::dialog::connect::{ConnectAction, ConnectDialog, ConnectStage};
use crate::ui::dialog::permission::PermissionDialog;
use crate::ui::dialog::plugin_picker::PluginPicker;
use crate::ui::dialog::question::QuestionDialog;
use crate::ui::dialog::session_picker::SessionPicker;
use crate::ui::dialog::settings::{SettingsAction, SettingsDialog, SettingsLevel};
use crate::ui::dialog::tasks_panel::TasksPanel;
use crate::ui::input::InputBox;
use crate::ui::layout::{render_header, split_main, HeaderInfo};
use crate::ui::status::{Mode, StatusBar};
use crate::ui::tabs::{render_sidebar, SidebarInfo};
use crate::ui::toast::Toast;

#[derive(Debug, Clone)]
struct CompletionHint {
    prefix: String,
    placeholder: String,
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
    input: InputBox,
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
    session_picker: Option<SessionPicker>,
    tasks_panel: Option<TasksPanel>,
    /// Snapshot of the active tab's background shells, refreshed while the
    /// tasks panel is open (the tracker's `list()` is async; the render path
    /// is not).
    bg_shells: Vec<BgShell>,
    show_help: bool,
    toast: Option<Toast>,
    provider_names: Vec<String>,
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

        Self {
            tabs: vec![tab0],
            active: 0,
            next_tab_id: 1,
            template,
            sidebar_visibility: SidebarVisibility::Auto,
            input: InputBox::new(),
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
            session_picker: None,
            tasks_panel: None,
            bg_shells: Vec::new(),
            show_help: false,
            toast: None,
            provider_names: ui.provider_names,
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
                true
            }
            Err(e) => {
                self.toast = Some(Toast::error(format!("switch failed: {e}")));
                false
            }
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
        let mut terminal = setup_terminal()?;
        let result = self.event_loop(&mut terminal).await;
        restore_terminal(&mut terminal)?;
        result
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
        self.tabs[self.active]
            .chat
            .render(f, areas.chat, &theme, chat_meta);
        let input_area: Rect = areas.composer;
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
        // Toast renders before the permission dialog so it can never cover an
        // active approval prompt; the dialog is the true top layer.
        if let Some(toast) = &self.toast {
            toast.render(f, area, &theme);
        }
        if let Some(q) = &self.active_question {
            q.render(f, area, &theme);
        }
        if let Some(dialog) = &self.active_dialog {
            dialog.render(f, area, &theme);
        }
    }

    async fn handle_term(&mut self, ev: CtEvent, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let CtEvent::Key(key) = ev else {
            return;
        };
        // Ignore key-release events (crossterm reports them on some terminals).
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }

        // 1. Permission dialog captures input until it's answered.
        if let Some(dialog) = &mut self.active_dialog {
            let answer = match key.code {
                KeyCode::Char(c) => c,
                KeyCode::Esc => 'n', // Esc denies
                _ => return,
            };
            if dialog.on_key(answer) {
                // Show the next queued request, focusing ITS source tab/cwd.
                let next = self.pending_requests.pop_front();
                self.active_dialog = next.map(|r| self.open_approval(r));
            }
            return;
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
                // Interrupt a running turn; quit when idle.
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
                self.tabs[self.active].chat = ChatView::new();
                return;
            }
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                self.open_settings();
                return;
            }
            (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
                self.new_tab().await;
                return;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                self.close_active_tab();
                return;
            }
            (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
                self.open_tasks_panel().await;
                return;
            }
            // Ctrl+1..9 jump to a tab by position.
            (KeyCode::Char(c), KeyModifiers::CONTROL) if c.is_ascii_digit() && c != '0' => {
                let n = (c as u8 - b'1') as usize;
                self.switch_to(n);
                return;
            }
            (KeyCode::Tab, KeyModifiers::CONTROL) => {
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
            _ => {}
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

        // 6. Enter submits; Shift/Alt+Enter newline; else feed the input box.
        match (key.code, key.modifiers) {
            (KeyCode::Enter, m)
                if !m.contains(KeyModifiers::SHIFT) && !m.contains(KeyModifiers::ALT) =>
            {
                let text = self.input.take();
                self.completion_hint = None;
                self.autocomplete.dismiss();
                if !text.trim().is_empty() {
                    self.submit(&text, agent_tx).await;
                }
            }
            (KeyCode::Enter, _) => self.input.insert_newline(),
            _ => self.input.input(key),
        }
        // 7. Refresh the autocomplete popup from the new input text.
        self.autocomplete.update(&self.input.text());
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

    fn open_connect_dialog(&mut self) {
        self.connect = Some(ConnectDialog::new());
    }

    /// Open the `/plugin` picker over the active tab's discovered plugins
    /// (tool groups, MCP servers with live state, skills, LSP servers).
    fn open_plugin_picker(&mut self) {
        let plugins = self.active_tab().engine.plugin_list();
        self.plugin_picker = Some(PluginPicker::new(plugins));
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
        if let Some((name, args)) = parse_slash(text) {
            self.handle_slash(name, args, agent_tx).await;
            return;
        }
        // One turn per tab (a second QueryLoop would mutate the same store
        // concurrently). Instead of rejecting, QUEUE the message and send it
        // when this tab goes idle — see `dispatch_queued_input`.
        if self.active_tab().is_busy() {
            self.active_tab_mut()
                .queued_input
                .push_back(text.to_string());
            let n = self.active_tab().queued_input.len();
            self.toast = Some(Toast::info(format!(
                "queued ({n}) — sends when the turn finishes (Esc to interrupt now)"
            )));
            return;
        }
        // Stamp the session title from the first user prompt of this tab.
        if !self.active_tab().titled {
            self.active_tab_mut().stamp_title(text).await;
        }

        let tab = &mut self.tabs[self.active];
        tab.chat.push_user(text);
        // No begin_assistant(): push_delta lazily opens an assistant segment,
        // so text after a tool card starts a fresh segment.
        tab.mode = Mode::Thinking;
        tab.thinking_process_shown = false;
        tab.active_tool_names.clear();

        tab.turn_seq += 1;
        let turn_id = tab.turn_seq;
        tab.active_turn_id = turn_id;
        let tab_id = tab.id;
        let abort = AbortController::new();
        tab.turn_abort = Some(abort.clone());

        let engine = tab.engine.clone();
        let prompt = text.to_string();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            match engine.turn(&prompt, abort).await {
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
                    Event::Thinking { .. } => {
                        tab.mode = Mode::Thinking;
                        if !tab.thinking_process_shown {
                            if let Some(line) = process_line_for_event(&event, None) {
                                tab.chat.push_tool(&line);
                            }
                            tab.thinking_process_shown = true;
                        }
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
                tab.thinking_process_shown = false;
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
            "sidebar" => self.handle_sidebar_command(args),
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
            "mcp" => {
                let lines: Vec<String> = match &self.active_tab().engine.mcp {
                    None => vec!["(no MCP servers configured)".to_string()],
                    Some(lc) => lc
                        .registry
                        .snapshot()
                        .iter()
                        .map(|s| {
                            let status = if s.state.is_connected() {
                                "connected"
                            } else {
                                "not connected"
                            };
                            format!(
                                "{} — {} ({} tools)",
                                s.name,
                                status,
                                s.state.tool_names().len()
                            )
                        })
                        .collect(),
                };
                for l in lines {
                    self.active_tab_mut().chat.push_system(&l);
                }
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
                    self.active_tab_mut().chat.push_system("(no skills loaded)");
                } else {
                    for l in list {
                        self.active_tab_mut().chat.push_system(&l);
                    }
                }
            }
            other => {
                self.toast = Some(Toast::info(format!("/{other} lands in a later phase")));
            }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarVisibility {
    Auto,
    Visible,
    Hidden,
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Ready => "ready",
        Mode::Thinking => "thinking",
        Mode::Streaming => "streaming",
        Mode::Error => "error",
    }
}

fn should_show_sidebar(tab_count: usize, visibility: SidebarVisibility) -> bool {
    match visibility {
        SidebarVisibility::Auto => tab_count > 1,
        SidebarVisibility::Visible => true,
        SidebarVisibility::Hidden => false,
    }
}

fn process_line_for_event(event: &Event, known_tool: Option<&str>) -> Option<String> {
    match event {
        Event::TextDelta { .. } => None,
        Event::Thinking { delta } => (!delta.is_empty()).then(|| "Thinking…".to_string()),
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
        } => Some(format!(
            "Usage ↑{input_tokens} ↓{output_tokens} cache +{cache_create}/{cache_read}"
        )),
        Event::Result { data } => {
            let stop = data.stop_reason.as_deref().unwrap_or("complete");
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

/// Rebuild a ChatView from a resumed MessageStore so the conversation history
/// is visible after /resume. User messages that carry only tool results are
/// skipped (their tool card already shows under the assistant turn); System /
/// Progress / Tombstone messages are not chat content.
fn rebuild_chat_from_store(store: &MessageStore) -> ChatView {
    let mut chat = ChatView::new();
    for msg in store.iter() {
        match msg {
            Message::User { content, .. } => {
                for block in content {
                    if let ContentBlock::Text { text } = block {
                        if !text.trim().is_empty() {
                            chat.push_user(text);
                        }
                    }
                }
            }
            Message::Assistant { content, .. } => {
                let mut thinking_shown = false;
                for block in content {
                    match block {
                        ContentBlock::Text { text } => chat.push_delta(text),
                        ContentBlock::Thinking { thinking, .. }
                            if !thinking_shown && !thinking.is_empty() =>
                        {
                            chat.push_tool("Thinking…");
                            thinking_shown = true;
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

fn setup_terminal() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    // Undo raw mode if any subsequent step fails, so we never leave the
    // terminal in a broken state on a setup error.
    if let Err(e) = stdout.execute(EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
        Ok(term) => {
            install_panic_hook();
            Ok(term)
        }
        Err(e) => {
            let _ = std::io::stdout().execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();
            Err(e)
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Restore the terminal on panic so a crash doesn't leave a garbled tty.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = std::io::stdout().execute(LeaveAlternateScreen);
        original(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Some("Thinking…")
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
            Some("Usage ↑10 ↓3 cache +1/2")
        );
    }
}
