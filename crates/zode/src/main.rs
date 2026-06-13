mod args;
mod headless;

use std::path::PathBuf;
use std::sync::Arc;

use agent::permission::PermissionMode;
use agent::session::Session;
use args::Args;
use clap::Parser;
use zode_core::approval::{ApprovalGate, BypassGate, StdinGate};
use zode_core::config::ConfigManager;
use zode_core::session_meta::SessionIndex;
use zode_core::ZodeEngine;

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

    let mode = if args.yolo {
        PermissionMode::Bypass
    } else {
        PermissionMode::Default
    };
    let gate: Arc<dyn ApprovalGate> = if args.yolo {
        Arc::new(BypassGate)
    } else {
        Arc::new(StdinGate)
    };

    let engine = match ZodeEngine::assemble(&cfg, cwd, mode, gate) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("zode: {e}");
            return 1;
        }
    };

    // Resume: --resume <id-prefix> or --continue (latest).
    let mut resumed_id: Option<String> = None;
    let target = if let Some(r) = &args.resume {
        SessionIndex::load()
            .ok()
            .and_then(|i| i.find_prefix(r).cloned())
    } else if args.continue_ {
        SessionIndex::load().ok().and_then(|i| i.latest().cloned())
    } else {
        None
    };
    let engine = if let Some(meta) = target {
        match SessionIndex::session_path(&meta.id) {
            Ok(path) => match Session::load(&path).await {
                Ok(store) => {
                    let short = &meta.id[..8.min(meta.id.len())];
                    eprintln!("zode: resumed session {short} ({})", meta.title);
                    resumed_id = Some(meta.id.clone());
                    engine.with_store(store)
                }
                Err(e) => {
                    eprintln!("zode: could not load session {}: {e}", meta.id);
                    engine
                }
            },
            Err(_) => engine,
        }
    } else {
        engine
    };

    if let Some(prompt) = args.print.clone() {
        return headless::run_print(&engine, &prompt).await;
    }

    // Full TUI lands in Phase 04; for now everything else is the REPL.
    headless::run_repl(engine, resumed_id).await
}
