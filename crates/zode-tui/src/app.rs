//! TUI main loop. Initializes the terminal, runs a tokio::select! over
//! terminal input + agent events + a tick, and drives one turn at a time.

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
use zode_core::commands::{parse_slash, CommandRegistry};
use zode_core::config::ConfigManager;
use zode_core::ZodeEngine;

use crate::event::AppEvent;
use crate::theme::{Theme, ThemeStore};
use crate::ui::chat::ChatView;
use crate::ui::input::InputBox;
use crate::ui::status::{Mode, StatusBar};

pub struct UiConfig {
    pub theme_id: Option<String>,
    pub yolo: bool,
    pub sandbox: bool,
}

pub struct TuiApp {
    engine: Arc<ZodeEngine>,
    chat: ChatView,
    input: InputBox,
    status: StatusBar,
    theme_store: ThemeStore,
    theme: Theme,
    commands: CommandRegistry,
    /// Abort handle for the in-flight turn, if any.
    turn_abort: Option<AbortController>,
    should_quit: bool,
}

impl TuiApp {
    pub fn new(engine: ZodeEngine, ui: UiConfig) -> Self {
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
            commands: CommandRegistry::with_builtins(),
            turn_abort: None,
            should_quit: false,
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
                        self.handle_term(ev, &agent_tx);
                    }
                }
                Some(app_ev) = agent_rx.recv() => {
                    self.handle_agent_event(app_ev);
                }
                _ = ticker.tick() => {
                    self.status.tick();
                }
            }
        }
        Ok(())
    }

    fn draw(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // chat
                Constraint::Length(3), // input
                Constraint::Length(1), // status
            ])
            .split(f.area());
        self.chat.render(f, chunks[0], &self.theme);
        self.input.render(f, chunks[1], &self.theme);
        self.status.render(f, chunks[2], &self.theme);
    }

    fn handle_term(&mut self, ev: CtEvent, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        let CtEvent::Key(key) = ev else {
            return;
        };
        // Ignore key-release events (crossterm reports them on some terminals).
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if let Some(abort) = self.turn_abort.take() {
                    abort.abort_with_reason("user interrupted");
                    self.chat.push_system("(interrupted)");
                    self.status.mode = Mode::Ready;
                } else {
                    self.should_quit = true;
                }
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => self.chat = ChatView::new(),
            (KeyCode::PageUp, _) => self.chat.scroll_up(5),
            (KeyCode::PageDown, _) => self.chat.scroll_down(5),
            (KeyCode::Enter, m)
                if !m.contains(KeyModifiers::SHIFT) && !m.contains(KeyModifiers::ALT) =>
            {
                let text = self.input.take();
                if !text.trim().is_empty() {
                    self.submit(&text, agent_tx);
                }
            }
            (KeyCode::Enter, _) => self.input.insert_newline(),
            _ => self.input.input(key),
        }
    }

    fn submit(&mut self, text: &str, agent_tx: &mpsc::UnboundedSender<AppEvent>) {
        if let Some((name, args)) = parse_slash(text) {
            self.handle_slash(name, args);
            return;
        }
        self.chat.push_user(text);
        self.chat.begin_assistant();
        self.status.mode = Mode::Thinking;

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
                            Ok(ev) => {
                                if tx.send(AppEvent::Agent(ev)).is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::TurnDone(Err(e.to_string())));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(AppEvent::TurnDone(Ok(())));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::TurnDone(Err(e.to_string())));
                }
            }
        });
    }

    fn handle_agent_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Agent(Event::TextDelta { delta }) => {
                self.status.mode = Mode::Streaming;
                self.chat.push_delta(&delta);
            }
            AppEvent::Agent(Event::ToolUse { name, .. }) => self.chat.push_tool(&name),
            AppEvent::Agent(Event::Usage {
                input_tokens,
                output_tokens,
                ..
            }) => {
                self.status.input_tokens = self.status.input_tokens.saturating_add(input_tokens);
                self.status.output_tokens = self.status.output_tokens.saturating_add(output_tokens);
            }
            AppEvent::Agent(Event::Error { code, message }) => {
                self.chat.push_system(&format!("error [{code}]: {message}"));
                self.status.mode = Mode::Error;
            }
            AppEvent::Agent(_) => {}
            AppEvent::TurnDone(res) => {
                self.chat.end_turn();
                self.turn_abort = None;
                self.status.mode = match res {
                    Ok(()) => Mode::Ready,
                    Err(e) => {
                        self.chat.push_system(&format!("turn failed: {e}"));
                        Mode::Error
                    }
                };
            }
        }
    }

    fn handle_slash(&mut self, name: &str, args: &str) {
        match name {
            "exit" => self.should_quit = true,
            "help" => {
                for c in self.commands.all() {
                    self.chat
                        .push_system(&format!("/{:<10} {}", c.name, c.description));
                }
            }
            "clear" => self.chat = ChatView::new(),
            "theme" => self.handle_theme(args),
            other => self
                .chat
                .push_system(&format!("/{other} is wired in a later phase")),
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
    stdout.execute(EnterAlternateScreen)?;
    install_panic_hook();
    Terminal::new(CrosstermBackend::new(stdout))
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
