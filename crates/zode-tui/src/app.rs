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
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::sync::mpsc;
use uuid::Uuid;
use zode_core::approval::{ApprovalReceiver, ApprovalRequest};
use zode_core::bg_shells::BgShell;
use zode_core::commands::parse_slash;
use zode_core::config::ConfigManager;
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::{EngineTemplate, ZodeEngine};

use crate::event::AppEvent;
use crate::tab::SessionTab;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::Autocomplete;
use crate::ui::chat::ChatView;
use crate::ui::dialog::permission::PermissionDialog;
use crate::ui::dialog::session_picker::SessionPicker;
use crate::ui::dialog::settings::{SettingsAction, SettingsDialog, SettingsLevel};
use crate::ui::dialog::tasks_panel::TasksPanel;
use crate::ui::input::InputBox;
use crate::ui::status::{Mode, StatusBar};
use crate::ui::tabs::render_tabs;
use crate::ui::toast::Toast;

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
    input: InputBox,
    status: StatusBar,
    theme_store: ThemeStore,
    theme: Theme,
    should_quit: bool,
    /// Approval requests from gated tools (one dialog shown at a time).
    approval_rx: ApprovalReceiver,
    active_dialog: Option<PermissionDialog>,
    pending_requests: VecDeque<ApprovalRequest>,
    autocomplete: Autocomplete,
    settings: Option<SettingsDialog>,
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
            input: InputBox::new(),
            status,
            theme_store,
            theme,
            should_quit: false,
            approval_rx,
            active_dialog: None,
            pending_requests: VecDeque::new(),
            autocomplete: Autocomplete::new(),
            settings: None,
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
                    self.delete_session(&meta.id);
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
    /// untouched (they re-create the file on the next save).
    fn delete_session(&mut self, id: &str) {
        if let Ok(path) = SessionIndex::session_path(id) {
            let _ = std::fs::remove_file(path);
        }
        if let Ok(mut idx) = SessionIndex::load() {
            if idx.remove(id) {
                let _ = idx.save();
            }
        }
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
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_agent_event(app_ev);
                }
                Some(req) = self.approval_rx.next() => {
                    // Focus the tab that requested this approval (its gate is
                    // labeled with its id) so the prompt appears over the right
                    // conversation, not whatever tab happens to be active.
                    if let Some(src) = req.source.as_deref().and_then(|s| s.parse::<usize>().ok()) {
                        if let Some(pos) = self.tabs.iter().position(|t| t.id == src) {
                            self.active = pos;
                        }
                    }
                    // An approval is the highest-priority modal: dismiss any
                    // settings/help/picker/panel overlay so it can't hide the
                    // prompt that is now capturing input.
                    self.settings = None;
                    self.session_picker = None;
                    self.tasks_panel = None;
                    self.show_help = false;
                    if self.active_dialog.is_none() {
                        let cwd = self.active_tab().engine.cwd.clone();
                        self.active_dialog = Some(PermissionDialog::new(req, cwd));
                    } else {
                        self.pending_requests.push_back(req);
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
        }
        // A tab bar row appears only when more than one tab exists, so the
        // single-tab layout is unchanged.
        let show_tabs = self.tabs.len() > 1;
        let constraints = if show_tabs {
            vec![
                Constraint::Length(1), // tab bar
                Constraint::Min(3),    // chat
                Constraint::Length(3), // input
                Constraint::Length(1), // status
            ]
        } else {
            vec![
                Constraint::Min(3),    // chat
                Constraint::Length(3), // input
                Constraint::Length(1), // status
            ]
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);
        let (chat_area, input_area, status_area) = if show_tabs {
            render_tabs(f, chunks[0], &self.tabs, self.active, &theme);
            (chunks[1], chunks[2], chunks[3])
        } else {
            (chunks[0], chunks[1], chunks[2])
        };
        self.tabs[self.active].chat.render(f, chat_area, &theme);
        self.input.render(f, input_area, &theme);
        self.status.render(f, status_area, &theme);
        // Autocomplete popup floats above the input row.
        self.autocomplete.render(f, input_area, &theme);
        // Overlays, lowest first. The permission dialog renders LAST (above
        // settings/help) because it captures input with the highest
        // precedence — it must never be hidden behind another overlay.
        if let Some(settings) = &mut self.settings {
            settings.render(f, area, &theme);
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
                let cwd = self.active_tab().engine.cwd.clone();
                self.active_dialog = self
                    .pending_requests
                    .pop_front()
                    .map(|r| PermissionDialog::new(r, cwd));
            }
            return;
        }

        // 2. Settings dialog captures input.
        if self.settings.is_some() {
            self.handle_settings_key(key.code);
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
                let tab = &mut self.tabs[self.active];
                if let Some(abort) = tab.turn_abort.take() {
                    abort.abort_with_reason("user interrupted");
                    tab.active_turn_id = 0;
                    tab.chat.end_turn();
                    tab.chat.push_system("(interrupted)");
                    tab.mode = Mode::Ready;
                } else {
                    self.should_quit = true;
                }
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
        let theme_ids = self
            .theme_store
            .list()
            .iter()
            .map(|t| t.id.clone())
            .collect();
        self.settings = Some(SettingsDialog::new(theme_ids, self.provider_names.clone()));
    }

    fn handle_settings_key(&mut self, code: KeyCode) {
        let Some(d) = &mut self.settings else {
            return;
        };
        match code {
            KeyCode::Up => d.prev(),
            KeyCode::Down => d.next(),
            KeyCode::Esc => {
                if d.level() == SettingsLevel::Top {
                    self.settings = None;
                } else {
                    d.back();
                }
            }
            KeyCode::Enter => {
                if d.level() == SettingsLevel::Top {
                    d.enter();
                } else if let Some(action) = d.confirm() {
                    self.settings = None;
                    self.apply_settings(action);
                }
            }
            _ => {}
        }
    }

    fn apply_settings(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::SetTheme(id) => {
                self.theme = self.theme_store.resolve(Some(&id));
                if let Ok(mut cfg) = ConfigManager::load_global() {
                    cfg.theme = Some(id.clone());
                    let _ = ConfigManager::save_global(&cfg);
                }
                self.toast = Some(Toast::info(format!("theme → {id}")));
            }
            SettingsAction::SetProvider(name) => {
                // Hot provider switch needs engine reassembly (heavy) and we
                // don't persist it here, so be honest: tell the user how to
                // switch rather than implying it's already applied (v1).
                self.toast = Some(Toast::info(format!(
                    "relaunch with --provider {name} to switch"
                )));
            }
            SettingsAction::SetMode(m) => {
                self.toast = Some(Toast::info(format!(
                    "permission mode '{m}' (informational in v1)"
                )));
            }
        }
    }

    fn apply_completion(&mut self) {
        if let Some(usage) = self.autocomplete.confirm() {
            self.input.take();
            self.input.insert_str(usage);
        }
        self.autocomplete.dismiss();
    }

    async fn submit(&mut self, text: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if let Some((name, args)) = parse_slash(text) {
            self.handle_slash(name, args, agent_tx).await;
            return;
        }
        // One turn per tab. A second turn on the same engine would run a second
        // QueryLoop mutating the same MessageStore concurrently — reject it and
        // restore the draft so the user can resend after the turn (or Ctrl+C).
        if self.active_tab().is_busy() {
            self.input.insert_str(text);
            self.toast = Some(Toast::info("turn in progress — Ctrl+C to interrupt"));
            return;
        }
        // Stamp the session title from the first user prompt of this tab.
        if !self.active_tab().titled {
            self.active_tab_mut().stamp_title(text);
        }

        let tab = &mut self.tabs[self.active];
        tab.chat.push_user(text);
        // No begin_assistant(): push_delta lazily opens an assistant segment,
        // so text after a tool card starts a fresh segment.
        tab.mode = Mode::Thinking;

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
                                if tx
                                    .send(AppEvent::Agent {
                                        tab_id,
                                        turn_id,
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
            AppEvent::Agent { event, .. } => match event {
                Event::TextDelta { delta } => {
                    tab.mode = Mode::Streaming;
                    tab.chat.push_delta(&delta);
                }
                Event::ToolUse { name, .. } => tab.chat.push_tool(&name),
                Event::Usage {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    tab.input_tokens = tab.input_tokens.saturating_add(input_tokens);
                    tab.output_tokens = tab.output_tokens.saturating_add(output_tokens);
                }
                Event::Error { code, message } => {
                    tab.chat.push_system(&format!("error [{code}]: {message}"));
                    tab.mode = Mode::Error;
                }
                _ => {}
            },
            AppEvent::TurnDone { result, .. } => {
                tab.chat.end_turn();
                tab.turn_abort = None;
                tab.active_turn_id = 0;
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
            "tasks" => self.open_tasks_panel().await,
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
            let ids: Vec<String> = self
                .theme_store
                .list()
                .iter()
                .map(|t| format!("{} ({})", t.id, t.name))
                .collect();
            self.active_tab_mut()
                .chat
                .push_system(&format!("themes: {}", ids.join(", ")));
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
                for block in content {
                    match block {
                        ContentBlock::Text { text } => chat.push_delta(text),
                        ContentBlock::ToolUse { name, .. } => chat.push_tool(name),
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
