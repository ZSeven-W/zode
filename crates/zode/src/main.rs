mod args;
mod doctor;
mod headless;
mod server;

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
    let args = Args::parse();
    let stdout_is_tty = std::io::stdout().is_terminal();
    init_tracing(&args, stdout_is_tty);
    let exit = run(args).await;
    std::process::exit(exit);
}

fn init_tracing(args: &Args, stdout_is_tty: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("ZODE_LOG").unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(default_tracing_filter(args, stdout_is_tty))
    });
    if tracing_writes_to_terminal(args, stdout_is_tty) {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::sink)
            .init();
    }
}

fn default_tracing_filter(args: &Args, stdout_is_tty: bool) -> &'static str {
    if launches_full_tui(args, stdout_is_tty) {
        "off"
    } else {
        "warn"
    }
}

fn tracing_writes_to_terminal(args: &Args, stdout_is_tty: bool) -> bool {
    !launches_full_tui(args, stdout_is_tty)
}

fn launches_full_tui(args: &Args, stdout_is_tty: bool) -> bool {
    args.command.is_none() && args.print.is_none() && !args.no_tui && stdout_is_tty
}

async fn run(args: Args) -> i32 {
    let cwd = match &args.cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    // Subcommands short-circuit the normal launch.
    if let Some(command) = &args.command {
        match command {
            args::Command::Doctor => return doctor::run(&cwd).await,
            args::Command::Server(server_args) => return server::run(server_args, &cwd).await,
        }
    }
    // First run: drop a starter config the user can edit. Best-effort — a
    // failure here (e.g. read-only home) must never stop zode from launching.
    if let Err(e) = ConfigManager::ensure_default_global() {
        tracing::warn!("could not write starter config: {e}");
    }
    let mut cfg = match ConfigManager::load(&cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("zode: config error: {e}");
            return 1;
        }
    };
    // --provider selects a named provider group; --model picks the model within
    // it (or overrides the active model when no --provider is given). Resolving
    // the (provider, model) pair TOGETHER applies the correct per-model override
    // — `--provider X --model Y` must use Y's settings, not the group default's.
    match (&args.provider, &args.model) {
        (Some(name), Some(m)) => match cfg.resolve_named_provider_model(name, m) {
            Some(p) => cfg.provider = p,
            None => {
                eprintln!("zode: no provider named '{name}' in config.providers");
                return 1;
            }
        },
        (Some(name), None) => match cfg.resolve_named_provider(name) {
            Some(p) => cfg.provider = p,
            None => {
                eprintln!("zode: no provider named '{name}' in config.providers");
                return 1;
            }
        },
        (None, Some(m)) => cfg.provider.model = Some(m.clone()),
        (None, None) => {}
    }
    // If the active provider has no key but a matching `providers` entry does,
    // adopt it — so a configured `providers` map works without `--provider`
    // (config wins over the env-var fallback below).
    cfg.resolve_provider_from_map();
    cfg.apply_env_fallbacks();

    // The OS sandbox for shell commands is ON BY DEFAULT (workspace-write,
    // network denied). `--no-sandbox` disables it; `--sandbox` forces it on;
    // otherwise config decides (default true). `resolve` FAILS CLOSED: if the
    // sandbox is wanted but can't be established (missing backend / unsupported
    // OS) it errors instead of silently running unconfined — we stop here and
    // tell the user to install a backend or pass `--no-sandbox`.
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
        let restrict_reads =
            args.sandbox_strict_read || cfg.sandbox.restrict_reads.unwrap_or(false);
        match zode_core::sandbox::resolve(
            &cwd,
            enabled,
            mode,
            allow_network,
            &roots,
            exclude_slash_tmp,
            exclude_tmpdir_env_var,
        ) {
            // Strict-read is applied post-resolve so it rides along on the
            // resolved config without widening `resolve`'s signature.
            Ok(sandbox) => sandbox.map(|c| c.with_restrict_reads(restrict_reads)),
            Err(e) => {
                eprintln!("zode: {e}");
                return 1;
            }
        }
    };
    // Prove the sandbox actually ENFORCES on this host before trusting it —
    // some systems have a backend that runs but does not confine (e.g. a
    // kernel without unprivileged user namespaces). FAIL-CLOSED like resolve:
    // stop and tell the user rather than run with a false sense of isolation.
    if let Some(sb) = &sandbox {
        if let Err(e) = sb.verify().await {
            eprintln!("zode: {e}");
            return 1;
        }
    }

    // --browser / --no-browser: session-only override, never persisted.
    if args.no_browser {
        cfg.browser.enabled = Some(false);
    } else if args.browser {
        cfg.browser.enabled = Some(true);
    }

    let today = today_date();

    // --print: headless single turn (stdin gate, or bypass on --yolo). A
    // short-lived one-shot has nothing to gain from a background update, so it's
    // only started for the interactive surfaces below.
    if let Some(prompt) = args.print.clone() {
        // Resume in the session's original directory when `-p` is combined
        // with `--continue`/`--resume`, so the one-shot appends to (and
        // persists) that conversation instead of starting cold.
        let resume_meta = resolve_resume_target(&args);
        let eff_cwd = resume_dir(&resume_meta).unwrap_or(cwd);
        let Some(engine) = build(&cfg, eff_cwd, headless_gate(args.yolo), sandbox, &today).await
        else {
            return 1;
        };
        let (engine, resumed_id) = attach_session(engine, resume_meta).await;
        return headless::run_print(&engine, &prompt, resumed_id).await;
    }

    // Silently check GitHub Releases in the background and swap in a newer build
    // for the next launch (best-effort; never blocks or interrupts the session).
    spawn_auto_update(&cfg);

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
    // The TUI also keeps a clone of the question queue so the `/op` command can
    // raise install/launch consent prompts through the same modal.
    let op_question_queue = question_queue.clone();
    // Make the config launchable even when the user hasn't finished provider
    // setup, so they always reach the UI (and can run `/connect`) instead of
    // being blocked at startup. TUI-only: headless surfaces above keep the
    // strict MissingApiKey / no-model errors. `needs_setup` drives a hint.
    let needs_setup = cfg.prepare_for_interactive_launch();
    // The TUI keeps a template so Ctrl+T / resume / hot-switch can (re)assemble
    // engines.
    let template = EngineTemplate::new(cfg.clone(), cwd, Some(queue), args.yolo, sandbox, today)
        .with_question_queue(Some(question_queue));
    // Tab 0 is assembled here; the app assigns it id 0, so label it "0".
    // Resume in the session's original directory when it still exists.
    let resume_meta = resolve_resume_target(&args);
    let engine_template = tui_engine_template(&template, resume_meta.as_ref());
    let engine = match engine_template
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
        initial_access: engine_template.tool_access(),
        sandbox: args.sandbox,
        provider_names: cfg.providers.keys().cloned().collect(),
        needs_setup,
    };
    match zode_tui::TuiApp::new(
        engine,
        template,
        ui,
        approval_rx,
        question_rx,
        op_question_queue,
        resumed_id,
    )
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

/// Spawn the silent background self-updater. No-op when disabled via
/// `autoUpdate: false` or `ZODE_NO_UPDATE`. Fully best-effort: it logs at debug
/// and never surfaces UI, per the "silently pull in the background" intent.
fn spawn_auto_update(cfg: &zode_core::config::ZodeConfig) {
    if !cfg.auto_update() || std::env::var_os("ZODE_NO_UPDATE").is_some() {
        return;
    }
    tokio::spawn(async {
        match zode_core::updater::auto_update_if_available(env!("CARGO_PKG_VERSION")).await {
            Ok(Some(tag)) => tracing::info!("zode self-updated to {tag} (restart to apply)"),
            Ok(None) => {}
            Err(e) => tracing::debug!("auto-update skipped: {e}"),
        }
    });
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
    // Headless surfaces have no UI to answer questions (None), no consent
    // channel for the op-bridge (None), don't enter plan mode (false), and
    // build a fresh, single-use browser session (None; no tab to share it with).
    match ZodeEngine::assemble(cfg, cwd, gate, sandbox, date, None, None, false, None).await {
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

/// Derive the engine used for the initial TUI tab without mutating the clean
/// process-wide defaults retained by `TuiApp`. Saved sessions deliberately
/// fail safe to prompted tool access because access is not persisted in
/// `SessionMeta`.
fn tui_engine_template(template: &EngineTemplate, meta: Option<&SessionMeta>) -> EngineTemplate {
    match meta {
        Some(meta) => template
            .with_model(meta.model.clone())
            .with_tool_access(zode_core::ToolAccessMode::Prompt)
            .with_plan_mode(false),
        None => template.clone(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn full_tui_defaults_tracing_filter_off() {
        let args = Args::parse_from(["zode"]);

        assert_eq!(default_tracing_filter(&args, true), "off");
    }

    #[test]
    fn non_tui_surfaces_keep_warn_tracing_filter() {
        let print = Args::parse_from(["zode", "--print", "hi"]);
        let no_tui = Args::parse_from(["zode", "--no-tui"]);
        let piped = Args::parse_from(["zode"]);
        let doctor = Args::parse_from(["zode", "doctor"]);

        assert_eq!(default_tracing_filter(&print, true), "warn");
        assert_eq!(default_tracing_filter(&no_tui, true), "warn");
        assert_eq!(default_tracing_filter(&piped, false), "warn");
        assert_eq!(default_tracing_filter(&doctor, true), "warn");
    }

    #[test]
    fn full_tui_tracing_does_not_write_to_terminal() {
        let tui = Args::parse_from(["zode"]);
        let print = Args::parse_from(["zode", "--print", "hi"]);

        assert!(!tracing_writes_to_terminal(&tui, true));
        assert!(tracing_writes_to_terminal(&print, true));
    }

    #[test]
    fn initial_tui_resume_uses_saved_model_and_prompt_without_dirtying_clean_template() {
        use zode_core::config::{ProviderConfig, ProviderKind, ZodeConfig};

        let cwd = tempfile::tempdir().unwrap();
        let template = EngineTemplate::new(
            ZodeConfig {
                provider: ProviderConfig {
                    r#type: Some(ProviderKind::Ollama),
                    base_url: Some("http://localhost:11434".into()),
                    model: Some("global-model".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            cwd.path().to_path_buf(),
            None,
            true,
            None,
            "2026-07-13".into(),
        );
        let meta = SessionMeta {
            id: "saved".into(),
            title: "Saved".into(),
            cwd: cwd.path().display().to_string(),
            model: "saved-model".into(),
            updated_at: 1,
        };

        let effective = tui_engine_template(&template, Some(&meta));

        assert_eq!(effective.model(), Some("saved-model"));
        assert_eq!(effective.tool_access(), zode_core::ToolAccessMode::Prompt);
        assert!(!effective.plan_mode());
        assert_eq!(template.model(), Some("global-model"));
        assert_eq!(template.tool_access(), zode_core::ToolAccessMode::Auto);
        assert!(!template.plan_mode());
    }
}
