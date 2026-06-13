//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

use std::collections::VecDeque;
use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
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
use zode_core::approval::{ApprovalReceiver, ApprovalRequest};
use zode_core::commands::parse_slash;
use zode_core::config::ConfigManager;
use zode_core::ZodeEngine;

use crate::event::AppEvent;
use crate::theme::{Theme, ThemeStore};
use crate::ui::autocomplete::Autocomplete;
use crate::ui::chat::ChatView;
use crate::ui::dialog::permission::PermissionDialog;
use crate::ui::dialog::settings::{SettingsAction, SettingsDialog, SettingsLevel};
use crate::ui::input::InputBox;
use crate::ui::status::{Mode, StatusBar};
use crate::ui::toast::Toast;

pub struct UiConfig {
    pub theme_id: Option<String>,
    pub yolo: bool,
    pub sandbox: bool,
    /// Named providers (config.providers keys) for the settings dialog.
    pub provider_names: Vec<String>,
}

pub struct TuiApp {
    engine: Arc<ZodeEngine>,
    chat: ChatView,
    input: InputBox,
    status: StatusBar,
    theme_store: ThemeStore,
    theme: Theme,
    /// Abort handle for the in-flight turn, if any.
    turn_abort: Option<AbortController>,
    /// Monotonic turn counter; `active_turn_id` is the turn whose events we
    /// currently accept. Aborting/superseding bumps it so stale events from
    /// a still-draining task are dropped (agent events carry no turn id).
    turn_seq: u64,
    active_turn_id: u64,
    should_quit: bool,
    /// Approval requests from gated tools (one dialog shown at a time).
    approval_rx: ApprovalReceiver,
    active_dialog: Option<PermissionDialog>,
    pending_requests: VecDeque<ApprovalRequest>,
    autocomplete: Autocomplete,
    settings: Option<SettingsDialog>,
    show_help: bool,
    toast: Option<Toast>,
    provider_names: Vec<String>,
}

impl TuiApp {
    pub fn new(engine: ZodeEngine, ui: UiConfig, approval_rx: ApprovalReceiver) -> Self {
        let mut theme_store = ThemeStore::with_builtins();
        if let Ok(dir) = ConfigManager::config_dir() {
            theme_store.merge_user(crate::theme::loader::load_dir(&dir.join("themes")));
        }
        let theme = theme_store.resolve(ui.theme_id.as_deref());
        let mut status = StatusBar::new(engine.model.clone());
        status.yolo = ui.yolo;
        status.sandbox = ui.sandbox;
        Self {
            chat: ChatView::new(),
            input: InputBox::new(),
            status,
            theme_store,
            theme,
            turn_abort: None,
            turn_seq: 0,
            active_turn_id: 0,
            should_quit: false,
            approval_rx,
            active_dialog: None,
            pending_requests: VecDeque::new(),
            autocomplete: Autocomplete::new(),
            settings: None,
            show_help: false,
            toast: None,
            provider_names: ui.provider_names,
            engine: Arc::new(engine),
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
                    if self.active_dialog.is_none() {
                        self.active_dialog = Some(PermissionDialog::new(req));
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // chat
                Constraint::Length(3), // input
                Constraint::Length(1), // status
            ])
            .split(area);
        self.chat.render(f, chunks[0], &theme);
        self.input.render(f, chunks[1], &theme);
        self.status.render(f, chunks[2], &theme);
        // Autocomplete popup floats above the input row.
        self.autocomplete.render(f, chunks[1], &theme);
        // Overlays, top-most last.
        if let Some(dialog) = &self.active_dialog {
            dialog.render(f, area, &theme);
        }
        if let Some(settings) = &mut self.settings {
            settings.render(f, area, &theme);
        }
        if self.show_help {
            crate::ui::help::render_help(f, area, &theme);
        }
        if let Some(toast) = &self.toast {
            toast.render(f, area, &theme);
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
                self.active_dialog = self.pending_requests.pop_front().map(PermissionDialog::new);
            }
            return;
        }

        // 2. Settings dialog captures input.
        if self.settings.is_some() {
            self.handle_settings_key(key.code);
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
                if let Some(abort) = self.turn_abort.take() {
                    abort.abort_with_reason("user interrupted");
                    self.active_turn_id = 0;
                    self.chat.end_turn();
                    self.chat.push_system("(interrupted)");
                    self.status.mode = Mode::Ready;
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
                self.chat = ChatView::new();
                return;
            }
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                self.open_settings();
                return;
            }
            (KeyCode::F(1), _) => {
                self.show_help = true;
                return;
            }
            (KeyCode::PageUp, _) => {
                self.chat.scroll_up(5);
                return;
            }
            (KeyCode::PageDown, _) => {
                self.chat.scroll_down(5);
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
                // Hot provider switch needs engine reassembly (heavy) — v1
                // applies it on next launch.
                if let Some(a) = self.turn_abort.take() {
                    a.abort_with_reason("provider switch");
                    self.active_turn_id = 0;
                }
                self.toast = Some(Toast::info(format!("provider '{name}' applies on restart")));
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
            self.handle_slash(name, args).await;
            return;
        }
        self.chat.push_user(text);
        // No begin_assistant(): push_delta lazily opens an assistant segment,
        // so text after a tool card starts a fresh segment.
        self.status.mode = Mode::Thinking;

        self.turn_seq += 1;
        let turn_id = self.turn_seq;
        self.active_turn_id = turn_id;
        let abort = AbortController::new();
        self.turn_abort = Some(abort.clone());

        let engine = self.engine.clone();
        let prompt = text.to_string();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            match engine.turn(&prompt, abort).await {
                Ok(mut stream) => {
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(event) => {
                                if tx.send(AppEvent::Agent { turn_id, event }).is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::TurnDone {
                                    turn_id,
                                    result: Err(e.to_string()),
                                });
                                return;
                            }
                        }
                    }
                    let _ = tx.send(AppEvent::TurnDone {
                        turn_id,
                        result: Ok(()),
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::TurnDone {
                        turn_id,
                        result: Err(e.to_string()),
                    });
                }
            }
        });
    }

    fn handle_agent_event(&mut self, ev: AppEvent) {
        // Drop events from aborted/superseded turns.
        let turn_id = match &ev {
            AppEvent::Agent { turn_id, .. } | AppEvent::TurnDone { turn_id, .. } => *turn_id,
        };
        if turn_id != self.active_turn_id {
            return;
        }
        match ev {
            AppEvent::Agent { event, .. } => match event {
                Event::TextDelta { delta } => {
                    self.status.mode = Mode::Streaming;
                    self.chat.push_delta(&delta);
                }
                Event::ToolUse { name, .. } => self.chat.push_tool(&name),
                Event::Usage {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    self.status.input_tokens =
                        self.status.input_tokens.saturating_add(input_tokens);
                    self.status.output_tokens =
                        self.status.output_tokens.saturating_add(output_tokens);
                }
                Event::Error { code, message } => {
                    self.chat.push_system(&format!("error [{code}]: {message}"));
                    self.status.mode = Mode::Error;
                }
                _ => {}
            },
            AppEvent::TurnDone { result, .. } => {
                self.chat.end_turn();
                self.turn_abort = None;
                self.active_turn_id = 0;
                self.status.mode = match result {
                    Ok(()) => Mode::Ready,
                    Err(e) => {
                        self.chat.push_system(&format!("turn failed: {e}"));
                        Mode::Error
                    }
                };
            }
        }
    }

    async fn handle_slash(&mut self, name: &str, args: &str) {
        match name {
            "exit" => self.should_quit = true,
            "help" => self.show_help = true,
            "clear" => {
                self.chat = ChatView::new();
                if let Ok(mut store) = self.engine.store.lock() {
                    *store = agent::message::MessageStore::new();
                }
            }
            "theme" => self.handle_theme(args),
            "undo" => {
                let r = self.engine.clone().undo().await;
                self.toast = Some(match r {
                    Ok(p) => Toast::info(format!("undid {}", p.display())),
                    Err(e) => Toast::error(e.to_string()),
                });
            }
            "redo" => {
                let r = self.engine.clone().redo().await;
                self.toast = Some(match r {
                    Ok(p) => Toast::info(format!("redid {}", p.display())),
                    Err(e) => Toast::error(e.to_string()),
                });
            }
            other => {
                self.toast = Some(Toast::info(format!("/{other} lands in a later phase")));
            }
        }
    }

    fn handle_theme(&mut self, args: &str) {
        if args.is_empty() {
            let ids: Vec<String> = self
                .theme_store
                .list()
                .iter()
                .map(|t| format!("{} ({})", t.id, t.name))
                .collect();
            self.chat
                .push_system(&format!("themes: {}", ids.join(", ")));
            return;
        }
        if self.theme_store.contains(args) {
            self.theme = self.theme_store.resolve(Some(args));
            if let Ok(mut cfg) = ConfigManager::load_global() {
                cfg.theme = Some(args.to_string());
                let _ = ConfigManager::save_global(&cfg);
            }
            self.chat.push_system(&format!("theme → {args}"));
        } else {
            self.chat.push_system(&format!("unknown theme: {args}"));
        }
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
