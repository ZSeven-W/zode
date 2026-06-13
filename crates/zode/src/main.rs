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
use zode_core::session_meta::SessionIndex;
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

    let sandbox = if args.sandbox {
        match zode_core::sandbox::SandboxConfig::for_current_os(&cwd) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("zode: {e}");
                return 1;
            }
        }
    } else {
        None
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
        let Some(engine) = build(&cfg, cwd, headless_gate(args.yolo), sandbox, &today).await else {
            return 1;
        };
        let (engine, resumed_id) = resume_into(engine, &args).await;
        return headless::run_repl(engine, resumed_id).await;
    }

    // Full TUI: approvals are gated through a queue the UI drains.
    let (queue, approval_rx) = zode_core::approval::approval_queue();
    let gate: Arc<dyn ApprovalGate> = if args.yolo {
        Arc::new(BypassGate)
    } else {
        Arc::new(zode_core::approval::QueueGate::new(queue))
    };
    // The TUI keeps a template so Ctrl+T / resume can assemble more engines,
    // all sharing this one gate (so every tab's approvals reach the same UI).
    let template = EngineTemplate::new(cfg.clone(), cwd, gate, sandbox, today);
    let engine = match template.assemble().await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("zode: {e}");
            return 1;
        }
    };
    let (engine, resumed_id) = resume_into(engine, &args).await;
    let ui = zode_tui::UiConfig {
        theme_id: cfg.theme.clone(),
        yolo: args.yolo,
        sandbox: args.sandbox,
        provider_names: cfg.providers.keys().cloned().collect(),
    };
    match zode_tui::TuiApp::new(engine, template, ui, approval_rx, resumed_id)
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
    match ZodeEngine::assemble(cfg, cwd, gate, sandbox, date).await {
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

/// Apply --resume/--continue: load the target session's store into `engine`.
/// Returns the (possibly updated) engine and the resumed session id.
async fn resume_into(engine: ZodeEngine, args: &Args) -> (ZodeEngine, Option<String>) {
    let target = if let Some(r) = &args.resume {
        SessionIndex::load()
            .ok()
            .and_then(|i| i.find_prefix(r).cloned())
    } else if args.continue_ {
        SessionIndex::load().ok().and_then(|i| i.latest().cloned())
    } else {
        None
    };
    let Some(meta) = target else {
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
