//! Headless modes: `-p/--print` (single turn) and `--no-tui` (readline
//! REPL). Both consume the agent Event stream without any TUI.

use std::io::Write;
use std::path::Path;
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
/// `resumed_id` is `Some` when `-p` ran with `--continue`/`--resume`; the
/// appended turn is then persisted so the conversation survives.
pub async fn run_print(engine: &ZodeEngine, prompt: &str, resumed_id: Option<String>) -> i32 {
    // A `-p` run over a large `--continue`/`--resume` context could exceed
    // the provider's input limit on the very first turn — compact it down
    // first (byte estimate; no prior Usage in a fresh process).
    engine.auto_compact_if_needed(None).await;
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
    let mut tool_names = std::collections::HashMap::<String, String>::new();
    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("\nzode stream error: {e}");
                exit = 1;
                break;
            }
        };
        // Feed every event to the cost tracker (it counts only Usage events).
        engine.cost.observe(&event).await;
        match event {
            Event::TextDelta { delta } => {
                let _ = write!(stdout, "{delta}");
                let _ = stdout.flush();
            }
            Event::ToolUse { id, name, .. } => {
                tool_names.insert(id, name.clone());
                eprintln!("· {name}");
            }
            Event::ToolResult { id, ok, output } => {
                if let Some(line) = tool_result_line(
                    tool_names.get(&id).map(String::as_str),
                    ok,
                    &output,
                    Some(engine.cwd.as_path()),
                ) {
                    eprintln!("· {line}");
                }
            }
            Event::Error { code, message } => {
                eprintln!("\nzode error [{code}]: {message}");
                exit = 1;
            }
            _ => {}
        }
    }
    let _ = writeln!(stdout);
    // Mine the completed turn for durable memories (no-op unless autoExtract
    // is on). Awaited so the process doesn't exit before the write lands.
    engine.extract_post_turn_inline().await;
    // Persist the appended turn when resuming, so `-p --continue` builds a
    // durable conversation instead of discarding each turn.
    if let Some(id) = &resumed_id {
        save_session(engine, id).await;
    }
    // Token/cache usage to stderr (keeps stdout = the model's answer).
    eprintln!("{}", engine.cost.report().await);
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
                        CmdFlow::Save => {
                            save_session(&engine, &session_id).await;
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
                run_turn(&engine, &line).await;
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
    let mut tool_names = std::collections::HashMap::<String, String>::new();
    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("\nstream error: {e}");
                break;
            }
        };
        // Feed every event to the cost tracker (it counts only Usage events).
        engine.cost.observe(&event).await;
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
}

fn tool_result_line(
    name: Option<&str>,
    ok: bool,
    output: &serde_json::Value,
    cwd: Option<&Path>,
) -> Option<String> {
    let name = name.unwrap_or("tool");
    if !ok {
        return Some(format!("{name} failed: {}", compact_tool_payload(output)));
    }
    if output
        .get("passed")
        .and_then(|v| v.as_bool())
        .is_some_and(|passed| !passed)
    {
        return Some(format!("{name} failed: {}", compact_tool_payload(output)));
    }
    if let Some(summary) = file_mutation_result_location_summary(name, output, cwd) {
        return Some(format!("{name} done {summary}"));
    }
    if let Some(exit_code) = output.get("exit_code") {
        let mut line = format!("{name} exit_code={}", display_json_atom(exit_code));
        if let Some(stderr) = output.get("stderr").and_then(|v| v.as_str()) {
            if !stderr.trim().is_empty() {
                line.push_str(&format!(" stderr={}", compact_text(stderr, 160)));
            }
        }
        if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
            if !stdout.trim().is_empty()
                && output
                    .get("stderr")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                line.push_str(&format!(" stdout={}", compact_text(stdout, 160)));
            }
        }
        return Some(line);
    }
    if name == "run_check" {
        if let Some(command) = output.get("command").and_then(|v| v.as_str()) {
            return Some(format!("{name} passed: {}", compact_text(command, 120)));
        }
    }
    None
}

fn file_mutation_result_location_summary(
    name: &str,
    output: &serde_json::Value,
    cwd: Option<&Path>,
) -> Option<String> {
    if !matches!(
        name,
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

fn compact_tool_payload(output: &serde_json::Value) -> String {
    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return compact_text(error, 220);
    }
    if let Some(failures) = output.get("failures").and_then(|v| v.as_array()) {
        let joined = failures
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if !joined.is_empty() {
            return compact_text(&joined, 220);
        }
    }
    compact_text(&output.to_string(), 220)
}

fn display_json_atom(value: &serde_json::Value) -> String {
    value
        .as_i64()
        .map(|n| n.to_string())
        .or_else(|| value.as_str().map(str::to_string))
        .unwrap_or_else(|| value.to_string())
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let mut one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return one_line;
    }
    let keep = max_chars.saturating_sub(3);
    one_line = one_line.chars().take(keep).collect();
    one_line.push_str("...");
    one_line
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
    let fallback = SessionMeta {
        id: id.to_string(),
        title: "(session)".to_string(),
        cwd: engine.cwd.display().to_string(),
        model: engine.model.clone(),
        updated_at: now_secs(),
    };
    if let Err(error) = SessionIndex::update(|idx| {
        if !idx.touch_updated(id, fallback.updated_at) {
            idx.upsert(fallback);
        }
        Ok(())
    }) {
        tracing::warn!("session index update failed after transcript save: {error}");
    }
}

fn stamp_title(engine: &ZodeEngine, id: &str, prompt: &str) {
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Flip a plugin id in the global config's `plugins.disabled` list and persist
/// it. Returns the new enabled state. Applies on the next launch (the running
/// engine's tool set is already assembled).
fn toggle_plugin(id: &str) -> Result<bool, zode_core::CoreError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_result_line_surfaces_failed_error() {
        let line = tool_result_line(
            Some("FileEdit"),
            false,
            &json!({"error": "old_string not found\nretry with FileRead"}),
            None,
        )
        .unwrap();
        assert!(line.contains("FileEdit failed"), "{line}");
        assert!(line.contains("old_string not found"), "{line}");
        assert!(line.contains("FileRead"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_bash_exit_code_and_stderr() {
        let line = tool_result_line(
            Some("Bash"),
            true,
            &json!({"exit_code": 52, "stdout": "", "stderr": "Empty reply from server\n"}),
            None,
        )
        .unwrap();
        assert!(line.contains("Bash exit_code=52"), "{line}");
        assert!(line.contains("Empty reply from server"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_failed_run_check() {
        let line = tool_result_line(
            Some("run_check"),
            true,
            &json!({"passed": false, "failures": ["expected stdout to contain ready"]}),
            None,
        )
        .unwrap();
        assert!(line.contains("run_check failed"), "{line}");
        assert!(line.contains("expected stdout"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_file_result_location_and_cwd() {
        let cwd = std::path::Path::new("/work/project");
        let line = tool_result_line(
            Some("FileWrite"),
            true,
            &json!({"path": "/work/project/created.txt", "status": "ok", "size_bytes": 1}),
            Some(cwd),
        )
        .unwrap();
        assert!(line.contains("FileWrite done"), "{line}");
        assert!(line.contains("path=/work/project/created.txt"), "{line}");
        assert!(line.contains("cwd=/work/project"), "{line}");
    }
}
