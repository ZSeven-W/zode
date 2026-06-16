mod args;
mod headless;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use agent::session::Session;
use args::Args;
use clap::Parser;
use zode_core::approval::{ApprovalGate, BypassGate, StdinGate};
use zode_core::config::ConfigManager;
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::{EngineTemplate, ZodeEngine};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZODE_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let exit = run(args).await;
    std::process::exit(exit);
}

async fn run(args: Args) -> i32 {
    let cwd = match &args.cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let mut cfg = match ConfigManager::load(&cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("zode: config error: {e}");
            return 1;
        }
    };
    // --provider selects a named provider; --model overrides the model.
    if let Some(name) = &args.provider {
        match cfg.providers.get(name).cloned() {
            Some(p) => cfg.provider = p,
            None => {
                eprintln!("zode: no provider named '{name}' in config.providers");
                return 1;
            }
        }
    }
    if let Some(m) = &args.model {
        cfg.provider.model = Some(m.clone());
    }
    cfg.apply_env_fallbacks();

    // The OS sandbox for shell commands is ON BY DEFAULT (workspace-write,
    // network denied). `--no-sandbox` disables it; `--sandbox` forces it on;
    // otherwise config decides (default true). `resolve` degrades gracefully
    // (returns None) on an unsupported OS or a missing backend.
    let sandbox = {
        use zode_core::sandbox::SandboxMode;
        let enabled = if args.no_sandbox {
            false
        } else if args.sandbox {
            true
        } else {
            cfg.sandbox.enabled.unwrap_or(true)
        };
        let mode = if args.sandbox_read_only {
            SandboxMode::ReadOnly
        } else {
            cfg.sandbox
                .mode
                .as_deref()
                .map(SandboxMode::parse)
                .unwrap_or_default()
        };
        let allow_network = args.sandbox_allow_network || cfg.sandbox.network.unwrap_or(false);
        let roots: Vec<std::path::PathBuf> = cfg
            .sandbox
            .writable_roots
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        let exclude_slash_tmp = cfg.sandbox.exclude_slash_tmp.unwrap_or(false);
        let exclude_tmpdir_env_var = cfg.sandbox.exclude_tmpdir_env_var.unwrap_or(false);
        zode_core::sandbox::resolve(
            &cwd,
            enabled,
            mode,
            allow_network,
            &roots,
            exclude_slash_tmp,
            exclude_tmpdir_env_var,
        )
    };

    let today = today_date();

    // --print: headless single turn (stdin gate, or bypass on --yolo).
    if let Some(prompt) = args.print.clone() {
        let Some(engine) = build(&cfg, cwd, headless_gate(args.yolo), sandbox, &today).await else {
            return 1;
        };
        return headless::run_print(&engine, &prompt).await;
    }

    // Plain REPL when asked, or when stdout isn't a tty (piped/CI).
    if args.no_tui || !std::io::stdout().is_terminal() {
        // Resume in the session's original directory when it still exists.
        let resume_meta = resolve_resume_target(&args);
        let eff_cwd = resume_dir(&resume_meta).unwrap_or(cwd);
        let Some(engine) = build(&cfg, eff_cwd, headless_gate(args.yolo), sandbox, &today).await
        else {
            return 1;
        };
        let (engine, resumed_id) = attach_session(engine, resume_meta).await;
        return headless::run_repl(engine, resumed_id).await;
    }

    // Full TUI: approvals are gated through a queue the UI drains. Each tab
    // gets a QueueGate labeled with its id (so prompts carry their source tab)
    // over this one channel; --yolo makes the gate bypass. The queue is kept
    // even under --yolo so `/yolo` can be toggled back off at runtime.
    let (queue, approval_rx) = zode_core::approval::approval_queue();
    // Parallel channel for the AskUserQuestion tool — the UI drains it like the
    // approval channel, but it carries a single-choice question, not an allow/deny.
    let (question_queue, question_rx) = zode_core::question::question_queue();
    // The TUI keeps a template so Ctrl+T / resume / hot-switch can (re)assemble
    // engines.
    let template = EngineTemplate::new(cfg.clone(), cwd, Some(queue), args.yolo, sandbox, today)
        .with_question_queue(Some(question_queue));
    // Tab 0 is assembled here; the app assigns it id 0, so label it "0".
    // Resume in the session's original directory when it still exists.
    let resume_meta = resolve_resume_target(&args);
    let engine = match template
        .assemble_tab(resume_dir(&resume_meta), Some("0".to_string()))
        .await
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("zode: {e}");
            return 1;
        }
    };
    let (engine, resumed_id) = attach_session(engine, resume_meta).await;
    let ui = zode_tui::UiConfig {
        theme_id: cfg.theme.clone(),
        yolo: args.yolo,
        sandbox: args.sandbox,
        provider_names: cfg.providers.keys().cloned().collect(),
    };
    match zode_tui::TuiApp::new(engine, template, ui, approval_rx, question_rx, resumed_id)
        .run()
        .await
    {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("zode tui: {e}");
            1
        }
    }
}

/// Gate for the headless surfaces: bypass on --yolo, else a stdin prompt.
fn headless_gate(yolo: bool) -> Arc<dyn ApprovalGate> {
    if yolo {
        Arc::new(BypassGate)
    } else {
        Arc::new(StdinGate::new())
    }
}

/// Assemble the engine, reporting and returning None on error.
async fn build(
    cfg: &zode_core::config::ZodeConfig,
    cwd: PathBuf,
    gate: Arc<dyn ApprovalGate>,
    sandbox: Option<zode_core::sandbox::SandboxConfig>,
    date: &str,
) -> Option<ZodeEngine> {
    // Headless surfaces have no UI to answer questions (None) and don't enter
    // plan mode (false).
    match ZodeEngine::assemble(cfg, cwd, gate, sandbox, date, None, false).await {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("zode: {e}");
            None
        }
    }
}

/// Today's date as YYYY-MM-DD (UTC) for the system prompt's env block.
fn today_date() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

/// Resolve which session `--resume`/`--continue` targets, if any. Done BEFORE
/// engine assembly so the engine can be built in the session's own cwd.
fn resolve_resume_target(args: &Args) -> Option<SessionMeta> {
    if let Some(r) = &args.resume {
        SessionIndex::load()
            .ok()
            .and_then(|i| i.find_prefix(r).cloned())
    } else if args.continue_ {
        SessionIndex::load().ok().and_then(|i| i.latest().cloned())
    } else {
        None
    }
}

/// The session's recorded cwd, but only if that directory still exists (else
/// the caller falls back to the launch cwd).
fn resume_dir(meta: &Option<SessionMeta>) -> Option<PathBuf> {
    meta.as_ref().and_then(|m| {
        let p = PathBuf::from(&m.cwd);
        p.is_dir().then_some(p)
    })
}

/// Load the resolved session's store into `engine`. Returns the (possibly
/// updated) engine and the resumed session id.
async fn attach_session(
    engine: ZodeEngine,
    meta: Option<SessionMeta>,
) -> (ZodeEngine, Option<String>) {
    let Some(meta) = meta else {
        return (engine, None);
    };
    match SessionIndex::session_path(&meta.id) {
        Ok(path) => match Session::load(&path).await {
            Ok(store) => {
                let short: String = meta.id.chars().take(8).collect();
                eprintln!("zode: resumed session {short} ({})", meta.title);
                (engine.with_store(store), Some(meta.id))
            }
            Err(e) => {
                eprintln!("zode: could not load session {}: {e}", meta.id);
                (engine, None)
            }
        },
        Err(_) => (engine, None),
    }
}
