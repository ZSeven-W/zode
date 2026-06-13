//! Headless modes: `-p/--print` (single turn) and `--no-tui` (readline
//! REPL). Both consume the agent Event stream without any TUI.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::abort::AbortController;
use agent::message::MessageStore;
use agent::session::Session;
use agent::stream::Event;
use futures::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use uuid::Uuid;
use zode_core::commands::{parse_slash, CommandAction, CommandRegistry};
use zode_core::config::ConfigManager;
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::ZodeEngine;

/// Run a single prompt, stream to stdout, return a process exit code.
pub async fn run_print(engine: &ZodeEngine, prompt: &str) -> i32 {
    let abort = AbortController::new();
    let mut stream = match engine.turn(prompt, abort).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zode: {e}");
            return 1;
        }
    };

    let mut exit = 0;
    let mut stdout = std::io::stdout();
    while let Some(item) = stream.next().await {
        match item {
            Ok(Event::TextDelta { delta }) => {
                let _ = write!(stdout, "{delta}");
                let _ = stdout.flush();
            }
            Ok(Event::ToolUse { name, .. }) => eprintln!("· {name}"),
            Ok(Event::ToolResult { ok, .. }) if !ok => eprintln!("· tool failed"),
            Ok(Event::Error { code, message }) => {
                eprintln!("\nzode error [{code}]: {message}");
                exit = 1;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("\nzode stream error: {e}");
                exit = 1;
                break;
            }
        }
    }
    let _ = writeln!(stdout);
    exit
}

/// Plain readline REPL. `resumed_id` is Some when --continue/--resume
/// loaded a prior session into the engine's store.
pub async fn run_repl(engine: ZodeEngine, resumed_id: Option<String>) -> i32 {
    let registry = CommandRegistry::with_builtins();
    let (session_id, mut titled) = match resumed_id {
        Some(id) => (id, true),
        None => (Uuid::new_v4().simple().to_string(), false),
    };

    let mut rl = match DefaultEditor::new() {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("zode: readline init: {e}");
            return 1;
        }
    };
    let history = history_path();
    if let Some(h) = &history {
        let _ = rl.load_history(h);
    }

    println!(
        "zode {} — {}  (/help, /exit)",
        env!("CARGO_PKG_VERSION"),
        engine.model
    );

    loop {
        match rl.readline("› ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());

                if let Some((name, cmd_args)) = parse_slash(&line) {
                    match dispatch_command(&registry, &engine, name, cmd_args).await {
                        CmdFlow::Exit => break,
                        CmdFlow::Continue => continue,
                    }
                }

                if !titled {
                    stamp_title(&engine, &session_id, &line);
                    titled = true;
                }
                run_turn(&engine, &line).await;
                save_session(&engine, &session_id).await;
            }
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl+C — press Ctrl+D or type /exit to quit)");
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("zode: {e}");
                break;
            }
        }
    }

    if let Some(h) = &history {
        let _ = rl.save_history(h);
    }
    0
}

enum CmdFlow {
    Exit,
    Continue,
}

async fn dispatch_command(
    registry: &CommandRegistry,
    engine: &ZodeEngine,
    name: &str,
    args: &str,
) -> CmdFlow {
    let Some(cmd) = registry.get(name) else {
        println!("unknown command: /{name}  (try /help)");
        return CmdFlow::Continue;
    };
    match cmd.name {
        "exit" => return CmdFlow::Exit,
        "help" => {
            for c in registry.all() {
                println!("  /{:<10} {}", c.name, c.description);
            }
        }
        "clear" => {
            // std::sync::Mutex; no await while held.
            if let Ok(mut store) = engine.store.lock() {
                *store = MessageStore::new();
            }
            println!("(context cleared)");
        }
        "model" => {
            if args.is_empty() {
                println!("model: {}", engine.model);
            } else {
                println!("(model switch needs reassembly — restart with --model {args})");
            }
        }
        "config" => println!("model={} cwd={}", engine.model, engine.cwd.display()),
        "compact" => {
            // Auto-compaction is enabled per turn (QueryLoop auto_compact).
            // A manual, hook-driven /compact lands in a later phase.
            println!("(auto-compaction is enabled; manual /compact lands later)");
        }
        "cost" => println!("(cost tracking surfaces in Phase 07)"),
        _ => match cmd.action {
            CommandAction::Ui => println!("/{name} is available in the TUI only"),
            _ => println!("/{name}: not handled in the REPL yet"),
        },
    }
    CmdFlow::Continue
}

async fn run_turn(engine: &ZodeEngine, prompt: &str) {
    let abort = AbortController::new();
    let mut stream = match engine.turn(prompt, abort).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zode: {e}");
            return;
        }
    };
    let mut out = std::io::stdout();
    while let Some(item) = stream.next().await {
        match item {
            Ok(Event::TextDelta { delta }) => {
                let _ = write!(out, "{delta}");
                let _ = out.flush();
            }
            Ok(Event::ToolUse { name, .. }) => eprintln!("\n· {name}"),
            Ok(Event::Error { code, message }) => eprintln!("\n[{code}] {message}"),
            Ok(_) => {}
            Err(e) => {
                eprintln!("\nstream error: {e}");
                break;
            }
        }
    }
    let _ = writeln!(out);
}

/// Snapshot the store (MessageStore: Clone) then persist. The std mutex
/// guard is dropped before the await, so it never crosses an await point.
async fn save_session(engine: &ZodeEngine, id: &str) {
    let Ok(path) = SessionIndex::session_path(id) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let snapshot = match engine.store.lock() {
        Ok(store) => store.clone(),
        Err(_) => return,
    };
    if let Err(e) = Session::save(&path, &snapshot).await {
        tracing::warn!("session save failed: {e}");
        return;
    }
    // Keep the index's recency current so `--continue` resumes this
    // session, not an older one. The entry exists (stamp_title on the first
    // turn for new sessions; created at resume for old ones); create a
    // minimal entry if somehow missing.
    let mut idx = SessionIndex::load().unwrap_or_default();
    if !idx.touch_updated(id, now_secs()) {
        idx.upsert(SessionMeta {
            id: id.to_string(),
            title: "(session)".to_string(),
            cwd: engine.cwd.display().to_string(),
            model: engine.model.clone(),
            updated_at: now_secs(),
        });
    }
    let _ = idx.save();
}

fn stamp_title(engine: &ZodeEngine, id: &str, prompt: &str) {
    let mut idx = SessionIndex::load().unwrap_or_default();
    idx.upsert(SessionMeta {
        id: id.to_string(),
        title: title_from_prompt(prompt),
        cwd: engine.cwd.display().to_string(),
        model: engine.model.clone(),
        updated_at: now_secs(),
    });
    let _ = idx.save();
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn history_path() -> Option<std::path::PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|d| d.join("input_history"))
}
