//! Plain readline REPL (`--no-tui` and piped stdout): slash commands plus
//! turn streaming over the same engine, journal, and checkpoint machinery as
//! `-p`.

use std::io::Write;

use agent::abort::AbortController;
use agent::message::MessageStore;
use agent::stream::Event;
use futures::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use uuid::Uuid;
use zode_core::commands::{parse_slash, CommandAction, CommandRegistry};
use zode_core::config::ConfigManager;
use zode_core::run_event::{RunEvent, RunEventContext, RunStatus, TurnOutcome, TurnRecorder};
use zode_core::session_meta::{title_from_prompt, SessionIndex, SessionMeta};
use zode_core::sessions::{DurableSessionMeta, SessionStore};
use zode_core::ZodeEngine;

use crate::headless::{now_secs, tool_result_line};

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
    if let Err(error) = ensure_session_sidecar(&engine, &session_id, "(untitled)").await {
        tracing::warn!("could not initialize durable session sidecar: {error}");
    }

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
                        CmdFlow::Save => {
                            save_session(&engine, &session_id).await;
                            continue;
                        }
                        CmdFlow::Cleared => {
                            // Persist the emptied transcript FIRST, then drop
                            // the compacted archive; /clear + /exit must not
                            // leave the old conversation on disk.
                            save_session(&engine, &session_id).await;
                            if let Ok(store) = SessionStore::open_default() {
                                if let Ok(path) = store.compacted_archive_path(&session_id) {
                                    let _ = tokio::fs::remove_file(path).await;
                                }
                            }
                            continue;
                        }
                    }
                }

                if !titled {
                    stamp_title(&engine, &session_id, &line);
                    titled = true;
                }
                // Pre-turn safety compaction using the accurate provider-
                // reported occupancy from the last turn — the runtime's own
                // byte-estimate auto-compaction under-counts CJK, so a long
                // REPL session could otherwise hard-400 at the input limit.
                let last = engine.last_prompt_tokens().await;
                if engine.auto_compact_if_needed(last).await {
                    save_session(&engine, &session_id).await;
                }
                run_turn(&engine, &session_id, &line).await;
                engine.extract_post_turn_inline().await;
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
    /// The command mutated the message store (e.g. /compact); persist the
    /// session before continuing so the change survives a resume.
    Save,
    /// `/clear`: persist the emptied transcript AND drop the compacted
    /// archive — the old conversation must not survive on disk.
    Cleared,
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
            // Conversation-scoped tracker state (restore latch, read set,
            // reminder baselines, display overlay) and the ledger describe
            // the discarded conversation — same cleanup as the TUI /clear.
            engine.clear_conversation_state();
            engine.ledger.clear();
            if let Ok(mut s) = engine.compact_state.lock() {
                *s = agent::compact::AutoCompactState::default();
            }
            println!("(context cleared)");
            return CmdFlow::Cleared;
        }
        "model" => {
            if args.is_empty() {
                println!("model: {}", engine.model);
            } else {
                println!("(model switch needs reassembly — restart with --model {args})");
            }
        }
        "config" => println!("model={} cwd={}", engine.model, engine.cwd.display()),
        "compact" => match engine.compact(AbortController::new()).await {
            Ok(o) => {
                println!(
                    "(compacted {} message{} · ~{} → ~{} tokens)",
                    o.replaced,
                    if o.replaced == 1 { "" } else { "s" },
                    o.pre_tokens,
                    o.post_tokens,
                );
                return CmdFlow::Save;
            }
            Err(e) => println!("(compact failed: {e})"),
        },
        "cost" => println!("{}", engine.cost.report().await),
        "undo" => match engine.undo().await {
            Ok(p) => println!("(undid edit to {})", p.display()),
            Err(e) => println!("({e})"),
        },
        "redo" => match engine.redo().await {
            Ok(p) => println!("(redid edit to {})", p.display()),
            Err(e) => println!("({e})"),
        },
        "skills" => {
            let list = engine.skills.list();
            if list.is_empty() {
                println!("(no skills loaded)");
            } else {
                for s in list {
                    println!("  {} — {}", s.name, s.description);
                }
            }
        }
        "plugin" => {
            let plugins = engine.plugin_list();
            if args.is_empty() {
                for p in &plugins {
                    let mark = if p.enabled { "[on] " } else { "[off]" };
                    println!("  {mark} {:<22} {}", p.id, p.description);
                }
                println!("(toggle with /plugin <id>; applies on restart)");
            } else if !plugins.iter().any(|p| p.id == args) {
                println!("unknown plugin: {args}");
            } else {
                match toggle_plugin(args) {
                    Ok(on) => println!(
                        "{args} {} (restart to apply)",
                        if on { "enabled" } else { "disabled" }
                    ),
                    Err(e) => println!("({e})"),
                }
            }
        }
        "mcp" => match &engine.mcp {
            None => println!("(no MCP servers configured)"),
            Some(lc) => {
                for s in lc.registry.snapshot() {
                    let status = if s.state.is_connected() {
                        "connected"
                    } else {
                        "not connected"
                    };
                    println!(
                        "  {} — {} ({} tools)",
                        s.name,
                        status,
                        s.state.tool_names().len()
                    );
                }
            }
        },
        "memory" => {
            println!("{}", engine.noema.handle_command(args, Some(&engine.cwd)));
        }
        "diff" => println!("{}", zode_core::diff::working_tree_diff(&engine.cwd).await),
        "agents" => {
            for (n, desc) in &engine.agent_types {
                println!("  {n:<12} {desc}");
            }
        }
        "external-agents" => {
            let subcommand = args.trim().to_ascii_lowercase();
            match subcommand.as_str() {
                "" | "list" => {
                    let detected = zode_core::external_agents::detect_installed_presets();
                    match ConfigManager::load(&engine.cwd) {
                        Err(e) => println!("(could not load config: {e})"),
                        Ok(_cfg) if detected.is_empty() => {
                            println!("(no supported external agent CLIs found on PATH)")
                        }
                        Ok(cfg) => {
                            for item in detected {
                                let status = match cfg.external_agents.agents.get(&item.name) {
                                    Some(entry) if entry.enabled == Some(false) => "disabled",
                                    Some(_) if cfg.external_agents.enabled() => "registered",
                                    Some(_) => "registered, globally disabled",
                                    None => "available",
                                };
                                println!(
                                    "  [{status}] {:<14} {}",
                                    item.name,
                                    item.command.display()
                                );
                            }
                            println!(
                                "(use /external-agents discover to register available presets)"
                            );
                        }
                    }
                }
                "discover" | "register" => {
                    match zode_core::external_agents::detect_and_register_global(&engine.cwd) {
                        Err(e) => println!("(external-agent registration failed: {e})"),
                        Ok(report) if report.detected.is_empty() => println!(
                            "(no supported external agent CLIs found on PATH; config unchanged)"
                        ),
                        Ok(report) => {
                            if !report.added.is_empty() {
                                println!("registered: {}", report.added.join(", "));
                            }
                            if !report.already_registered.is_empty() {
                                println!(
                                    "already registered: {}",
                                    report.already_registered.join(", ")
                                );
                            }
                            if !report.effective_enabled {
                                println!("(external agents remain disabled by project config)");
                            }
                            if report.config_changed {
                                println!("(restart zode to activate the updated agent registry)");
                            }
                        }
                    }
                }
                _ => println!("usage: /external-agents [list|discover]"),
            }
        }
        "workflows" => {
            if engine.workflows.is_empty() {
                println!("(no workflows; add ~/.zode/workflows/<name>.md or use define_workflow)");
            } else {
                for (n, desc) in &engine.workflows {
                    println!("  {n:<16} {desc}");
                }
            }
        }
        "hooks" => {
            let entries = zode_core::hooks_config::load_hook_entries(&engine.cwd);
            if entries.is_empty() {
                println!("(no hooks configured)");
            } else {
                for e in entries {
                    match &e.tool {
                        Some(t) => println!("  {} [{}] → {}", e.event, t, e.script),
                        None => println!("  {} → {}", e.event, e.script),
                    }
                }
            }
        }
        "export" => match zode_core::export::try_resolve_export_path(&engine.cwd, args) {
            Some(path) => match std::fs::write(&path, engine.export_markdown()) {
                Ok(()) => println!("(exported to {})", path.display()),
                Err(e) => println!("(export failed: {e})"),
            },
            None => println!(
                "(export path escapes the workspace — use an absolute path to export elsewhere)"
            ),
        },
        "currency" => {
            let code = args.trim();
            if code.is_empty() {
                println!("currency: {}", engine.cost.currency_code());
            } else {
                println!("currency → {}", engine.cost.set_currency(code));
            }
        }
        _ => match cmd.action {
            CommandAction::Ui => println!("/{name} is available in the TUI only"),
            _ => println!("/{name}: not handled in the REPL yet"),
        },
    }
    CmdFlow::Continue
}

async fn run_turn(engine: &ZodeEngine, session_id: &str, prompt: &str) {
    let turn_id = Uuid::new_v4().simple().to_string();
    let request_id = Uuid::new_v4().simple().to_string();
    let mut recorder = TurnRecorder::new(
        SessionStore::open_default().ok(),
        RunEventContext::new(session_id.to_string(), Some(turn_id), Some(request_id)),
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
    let abort = AbortController::new();
    let mut stream = match engine.turn(prompt, abort.clone()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zode: {e}");
            recorder.record(RunEvent::Error {
                code: "turn_start_error".into(),
                message: e.to_string(),
            });
            recorder.complete(
                Some(&engine.checkpoints),
                false,
                &TurnOutcome {
                    status: RunStatus::Failed,
                    stop_reason: None,
                    partial: false,
                },
            );
            return;
        }
    };
    let mut out = std::io::stdout();
    let mut tool_names = std::collections::HashMap::<String, String>::new();
    let mut failed = false;
    let mut stop_reason: Option<String> = None;
    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("\nstream error: {e}");
                recorder.record(RunEvent::Error {
                    code: "stream_error".into(),
                    message: e.to_string(),
                });
                failed = true;
                break;
            }
        };
        // Feed every event to the cost tracker (it counts only Usage events).
        engine.cost.observe(&event).await;
        recorder.record_agent(&event);
        if let Event::Result { data } = &event {
            stop_reason = data.stop_reason.clone();
        }
        match event {
            Event::TextDelta { delta } => {
                let _ = write!(out, "{delta}");
                let _ = out.flush();
            }
            Event::ToolUse { id, name, .. } => {
                tool_names.insert(id, name.clone());
                eprintln!("\n· {name}");
            }
            Event::ToolResult { id, ok, output } => {
                if let Some(line) = tool_result_line(
                    tool_names.get(&id).map(String::as_str),
                    ok,
                    &output,
                    Some(engine.cwd.as_path()),
                ) {
                    eprintln!("\n· {line}");
                }
            }
            Event::Error { code, message } => eprintln!("\n[{code}] {message}"),
            _ => {}
        }
    }
    let _ = writeln!(out);
    let status = RunStatus::derive(abort.is_aborted(), failed, stop_reason.as_deref());
    recorder.complete(
        Some(&engine.checkpoints),
        true,
        &TurnOutcome {
            status,
            stop_reason,
            partial: failed,
        },
    );
}

/// Snapshot the store (MessageStore: Clone) then persist. The std mutex
/// guard is dropped before the await, so it never crosses an await point.
async fn save_session(engine: &ZodeEngine, id: &str) {
    let snapshot = match engine.store.lock() {
        Ok(store) => store.clone(),
        Err(_) => return,
    };
    let store = match SessionStore::open_default() {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!("session store unavailable: {error}");
            return;
        }
    };
    let meta = match store.load_meta(id) {
        Ok(meta) => meta,
        Err(_) => {
            let index_meta = SessionIndex::load()
                .ok()
                .and_then(|index| index.sessions.into_iter().find(|meta| meta.id == id))
                .unwrap_or(SessionMeta {
                    id: id.to_string(),
                    title: "(session)".to_string(),
                    cwd: engine.cwd.display().to_string(),
                    model: engine.model.clone(),
                    updated_at: now_secs(),
                });
            let meta = DurableSessionMeta::new(index_meta);
            if !store.has_sidecar(id) {
                if let Err(error) = store.create(meta.clone()) {
                    tracing::warn!("session creation failed: {error}");
                    return;
                }
            }
            meta
        }
    };
    // Overlay originals ride along (same rationale as the TUI persist path).
    let overlay = engine.compacted_overlay_snapshot();
    if let Err(error) = store.save_with_originals(&meta, &snapshot, &overlay).await {
        tracing::warn!("session save failed: {error}");
    }
}

async fn ensure_session_sidecar(
    engine: &ZodeEngine,
    id: &str,
    title: &str,
) -> Result<(), zode_core::CoreError> {
    let store = SessionStore::open_default()?;
    if store.has_sidecar(id) {
        return Ok(());
    }
    if SessionIndex::session_path(id)?.is_file() {
        store.ensure_sidecar(id).await?;
        return Ok(());
    }
    store.create(DurableSessionMeta::new(SessionMeta {
        id: id.to_string(),
        title: title.to_string(),
        cwd: engine.cwd.display().to_string(),
        model: engine.model.clone(),
        updated_at: now_secs(),
    }))
}

fn stamp_title(engine: &ZodeEngine, id: &str, prompt: &str) {
    if let Ok(store) = SessionStore::open_default() {
        if store.has_sidecar(id) {
            if let Err(error) = store.update_meta(id, |meta| {
                meta.title = title_from_prompt(prompt);
                meta.cwd = engine.cwd.display().to_string();
                meta.model = engine.model.clone();
            }) {
                tracing::warn!("durable session title update failed: {error}");
            }
        }
    }
    let meta = SessionMeta {
        id: id.to_string(),
        title: title_from_prompt(prompt),
        cwd: engine.cwd.display().to_string(),
        model: engine.model.clone(),
        updated_at: now_secs(),
    };
    if let Err(error) = SessionIndex::update(|idx| {
        idx.upsert(meta);
        Ok(())
    }) {
        tracing::warn!("session index title update failed: {error}");
    }
}

/// Flip a plugin id in the global config's `plugins.disabled` list and persist
/// it. Returns the new enabled state. Applies on the next launch (the running
/// engine's tool set is already assembled).
fn toggle_plugin(id: &str) -> Result<bool, zode_core::CoreError> {
    // Installed packages are owned by the install registry, not by
    // `plugins.disabled`; route them there so the directory move happens too.
    if let Some(name) = id.strip_prefix("plugin:") {
        let manager = zode_core::plugin_package::PluginPackageManager::open_default()?;
        let currently = manager
            .registry()?
            .plugins
            .get(name)
            .map(|record| record.enabled)
            .unwrap_or(true);
        return manager
            .set_enabled(name, !currently)
            .map(|record| record.enabled);
    }
    let mut cfg = ConfigManager::load_global()?;
    let was_disabled = cfg.plugins.disabled.iter().any(|d| d == id);
    if was_disabled {
        cfg.plugins.disabled.retain(|d| d != id);
    } else {
        cfg.plugins.disabled.push(id.to_string());
    }
    ConfigManager::save_global(&cfg)?;
    Ok(was_disabled)
}

fn history_path() -> Option<std::path::PathBuf> {
    ConfigManager::config_dir()
        .ok()
        .map(|d| d.join("input_history"))
}
