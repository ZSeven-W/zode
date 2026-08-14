mod acp;
mod args;
mod browser_native_host;
mod dashboard;
mod doctor;
mod headless;
mod headless_output;
mod plugin_cli;
mod repl;
mod server;
mod session_cli;
mod session_setup;
mod ui_frontends;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use args::{Args, OutputFormat, PermissionModeArg};
use clap::Parser;
use session_setup::{attach_session, prepare_headless_session, resolve_resume_target, resume_dir};
use zode_core::approval::{AcceptEditsGate, ApprovalGate, BypassGate, DenyGate, StdinGate};
use zode_core::config::ConfigManager;
use zode_core::session_meta::SessionMeta;
use zode_core::{EngineTemplate, ZodeEngine};

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    if let Some(exit) = zode_core::sandbox::windows::intercept_private_entrypoint(
        &std::env::args_os().collect::<Vec<_>>(),
    ) {
        std::process::exit(exit as i32);
    }
    let raw_args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let native_invocation = raw_args
        .get(1)
        .is_some_and(|arg| zode_core::browser::bridge::native_host::is_invocation_arg(arg));
    let mut args = if native_invocation {
        if let Err(error) = browser_native_host::read_start_request() {
            let _ = browser_native_host::write_error(&error.to_string());
            std::process::exit(1);
        }
        Args::parse_from(["zode", "--browser-native-host"])
    } else {
        Args::parse()
    };
    if native_invocation {
        args.cwd = zode_core::browser::bridge::native_host::preferred_cwd()
            .map(|path| path.display().to_string());
    }
    let stdout_is_tty = std::io::stdout().is_terminal();
    init_tracing(&args, stdout_is_tty);
    let exit = run(args).await;
    zode_core::telemetry::shutdown();
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
    args.command.is_none()
        && args.print.is_none()
        && args.prompt_file.is_none()
        && args.prompt_json.is_none()
        && !args.no_tui
        && !args.browser_native_host
        && stdout_is_tty
}

async fn run(args: Args) -> i32 {
    let cwd = match &args.cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    // UI deps snapshot (frontends mount as harness plugins).
    let ui_deps_cwd = cwd.clone();
    // Subcommands short-circuit the normal launch.
    if let Some(command) = &args.command {
        match command {
            args::Command::Doctor => return doctor::run(&cwd).await,
            args::Command::Update { check } => return run_self_update(*check).await,
            args::Command::Server(server_args) => return server::run(server_args, &cwd).await,
            args::Command::Acp => return acp::run(&cwd).await,
            args::Command::Dashboard { json } => return dashboard::run(*json).await,
            args::Command::Plugin { action } => return plugin_cli::run(action, &cwd),
            args::Command::Session { action } => return session_cli::run(action, &cwd).await,
            args::Command::Sandbox { action } => match action {
                args::SandboxCommand::Cleanup => {
                    #[cfg(windows)]
                    match zode_core::sandbox::windows::cleanup_acl_journal() {
                        Ok(()) => return 0,
                        Err(error) => {
                            eprintln!("zode: sandbox cleanup failed: {error}");
                            return 1;
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        eprintln!("zode: sandbox cleanup is only needed on Windows");
                        return 0;
                    }
                }
            },
        }
    }
    let headless_prompt = match load_headless_prompt(&args) {
        Ok(prompt) => prompt,
        Err(error) => {
            emit_startup_error(args.output_format, "invalid_prompt", &error);
            return 2;
        }
    };
    if headless_prompt.is_none() && args.output_format != OutputFormat::Plain {
        eprintln!("zode: --output-format requires -p, --prompt-file, or --prompt-json");
        return 2;
    }
    // Exact session targeting / forking is only wired on the headless prompt
    // path. Reject rather than silently start a fresh random-id session (the
    // REPL/TUI resume only via --resume/--continue), matching the
    // --permission-mode guard below.
    if headless_prompt.is_none() && (args.session_id.is_some() || args.fork_session.is_some()) {
        eprintln!(
            "zode: --session-id and --fork-session require -p, --prompt-file, or --prompt-json"
        );
        return 2;
    }
    if args.yolo && args.permission_mode != PermissionModeArg::Default {
        emit_startup_error(
            args.output_format,
            "invalid_permission_mode",
            "--yolo cannot be combined with --permission-mode",
        );
        return 2;
    }
    // First run: drop a starter config the user can edit. Best-effort — a
    // failure here (e.g. read-only home) must never stop zode from launching.
    if let Err(e) = ConfigManager::ensure_default_global() {
        tracing::warn!("could not write starter config: {e}");
    }
    let mut cfg = match ConfigManager::load(&cwd) {
        Ok(c) => c,
        Err(e) => {
            emit_startup_error(args.output_format, "config_error", &e.to_string());
            return 1;
        }
    };
    if let Some(max_turns) = args.max_turns {
        cfg.max_iterations = Some(max_turns);
    }
    if let Some(path) = &args.rules {
        match zode_core::permission_rules::load_rule_specs(std::path::Path::new(path)) {
            Ok(rules) => cfg.permissions.rules.extend(rules),
            Err(error) => {
                emit_startup_error(
                    args.output_format,
                    "invalid_permission_rules",
                    &error.to_string(),
                );
                return 2;
            }
        }
    }
    // --provider selects a named provider group; --model picks the model within
    // it (or overrides the active model when no --provider is given). Resolving
    // the (provider, model) pair TOGETHER applies the correct per-model override
    // — `--provider X --model Y` must use Y's settings, not the group default's.
    match (&args.provider, &args.model) {
        (Some(name), Some(m)) => match cfg.resolve_named_provider_model(name, m) {
            Some(p) => cfg.provider = p,
            None => {
                emit_startup_error(
                    args.output_format,
                    "provider_not_found",
                    &format!("no provider named '{name}' in config.providers"),
                );
                return 1;
            }
        },
        (Some(name), None) => match cfg.resolve_named_provider(name) {
            Some(p) => cfg.provider = p,
            None => {
                emit_startup_error(
                    args.output_format,
                    "provider_not_found",
                    &format!("no provider named '{name}' in config.providers"),
                );
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
    // A named `--sandbox-profile` is a config preset, not a one-shot flag:
    // fold it into cfg.sandbox so every later consumer — the EngineTemplate
    // and the TUI `/sandbox` toggle, which both re-resolve from cfg.sandbox —
    // honors the profile's mode/network/roots/windowsTier. The transient CLI
    // overrides below stay session-only (the toggle owns mode/network).
    if let Some(name) = args.sandbox_profile.as_deref() {
        match zode_core::sandbox::select_profile(&cfg.sandbox, name) {
            Ok(profile) => {
                cfg.sandbox = zode_core::sandbox::overlay_profile(&cfg.sandbox, &profile)
            }
            Err(error) => {
                emit_startup_error(
                    args.output_format,
                    "sandbox_profile_not_found",
                    &error.to_string(),
                );
                return 2;
            }
        }
    }
    let sandbox = {
        let overrides = zode_core::sandbox::SandboxOverrides {
            disable: args.no_sandbox,
            force_enable: args.sandbox,
            read_only: args.sandbox_read_only,
            allow_network: args.sandbox_allow_network,
            strict_read: args.sandbox_strict_read,
        };
        match zode_core::sandbox::resolve_with_overrides(&cfg.sandbox, &cwd, &overrides, &[]) {
            Ok(sandbox) => sandbox,
            Err(e) => {
                emit_startup_error(args.output_format, "sandbox_error", &e.to_string());
                return 1;
            }
        }
    };
    // Prove the sandbox actually ENFORCES on this host before trusting it —
    // some systems have a backend that runs but does not confine (e.g. a
    // kernel without unprivileged user namespaces). FAIL-CLOSED like resolve:
    // stop and tell the user rather than run with a false sense of isolation.
    if let Some(sb) = &sandbox {
        if let Some(notice) = sb.windows_tier_notice() {
            if args.output_format == OutputFormat::Plain {
                eprintln!("zode: {notice}");
            }
        }
        if let Err(e) = sb.verify().await {
            emit_startup_error(
                args.output_format,
                "sandbox_verification_failed",
                &e.to_string(),
            );
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
    if let Some(prompt) = headless_prompt {
        let model = cfg
            .provider
            .model
            .clone()
            .unwrap_or_else(|| zode_core::config::DEFAULT_STARTER_MODEL.into());
        let prepared = match prepare_headless_session(&args, &prompt, &cwd, &model).await {
            Ok(prepared) => prepared,
            Err(error) => {
                emit_startup_error(args.output_format, "session_error", &error.to_string());
                return crate::headless_output::EXIT_SESSION;
            }
        };
        let eff_cwd = {
            let recorded = PathBuf::from(&prepared.meta.cwd);
            if recorded.is_dir() {
                recorded
            } else {
                cwd
            }
        };
        let tool_filter = match headless_tool_filter(&args) {
            Ok(filter) => filter,
            Err(error) => {
                emit_startup_error(
                    args.output_format,
                    "invalid_tool_filter",
                    &error.to_string(),
                );
                return 2;
            }
        };
        let engine = match build(
            &cfg,
            eff_cwd,
            headless_gate(&args),
            sandbox,
            &today,
            tool_filter.as_ref(),
        )
        .await
        {
            Ok(engine) => engine,
            Err(error) => {
                emit_startup_error(args.output_format, "engine_build_error", &error.to_string());
                return 10;
            }
        };
        let engine = engine.with_store(prepared.messages);
        let exit: std::sync::Arc<std::sync::Mutex<i32>> = Default::default();
        let root = zode_core::cordis_rs::Context::root();
        let host = zode_core::ui::UiHost::new(
            &root,
            Arc::new(zode_core::ui::UiDeps {
                cwd: ui_deps_cwd.clone(),
                cfg: cfg.clone(),
            }),
        )
        .expect("ui host");
        host.register(Arc::new(crate::ui_frontends::HeadlessUi {
            engine: Arc::new(engine),
            prompt,
            meta: std::sync::Mutex::new(Some(prepared.meta)),
            output_format: args.output_format,
            exit: exit.clone(),
        }));
        if let Err(error) = host.run("headless").await {
            eprintln!("zode: {error}");
            return 1;
        }
        return *exit.lock().unwrap();
    }

    // Silently check GitHub Releases in the background and swap in a newer build
    // for the next launch (best-effort; never blocks or interrupts the session).
    // The cell carries the applied tag so the TUI can show a one-time
    // "restart to apply" notice; the REPL stays log-only.
    let update_applied: std::sync::Arc<std::sync::OnceLock<String>> = Default::default();
    spawn_auto_update(&cfg, update_applied.clone());

    // Plain REPL when asked, or when stdout isn't a tty (piped/CI).
    if !args.browser_native_host && (args.no_tui || !std::io::stdout().is_terminal()) {
        // Resume in the session's original directory when it still exists.
        let resume_meta = resolve_resume_target(&args);
        let eff_cwd = resume_dir(&resume_meta).unwrap_or(cwd);
        let tool_filter = match headless_tool_filter(&args) {
            Ok(filter) => filter,
            Err(error) => {
                eprintln!("zode: {error}");
                return 2;
            }
        };
        let engine = match build(
            &cfg,
            eff_cwd,
            headless_gate(&args),
            sandbox,
            &today,
            tool_filter.as_ref(),
        )
        .await
        {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("zode: {error}");
                return 1;
            }
        };
        let (engine, resumed_id) = attach_session(engine, resume_meta).await;
        let exit: std::sync::Arc<std::sync::Mutex<i32>> = Default::default();
        let root = zode_core::cordis_rs::Context::root();
        let host = zode_core::ui::UiHost::new(
            &root,
            Arc::new(zode_core::ui::UiDeps {
                cwd: ui_deps_cwd.clone(),
                cfg: cfg.clone(),
            }),
        )
        .expect("ui host");
        host.register(Arc::new(crate::ui_frontends::ReadlineUi {
            engine: std::sync::Mutex::new(Some(engine)),
            resumed_id,
            exit: exit.clone(),
        }));
        if let Err(error) = host.run("readline").await {
            eprintln!("zode: {error}");
            return 1;
        }
        return *exit.lock().unwrap();
    }

    // The TUI's approval flow is the interactive queue gate; a headless
    // permission policy has no mapping onto it. Reject instead of silently
    // running with more capability than the flag promised (--yolo remains
    // the TUI's bypass).
    if args.permission_mode != PermissionModeArg::Default {
        eprintln!("zode: --permission-mode requires -p, --prompt-file, --prompt-json, or --no-tui");
        return 2;
    }
    let tui_tool_filter = match headless_tool_filter(&args) {
        Ok(filter) => filter,
        Err(error) => {
            eprintln!("zode: {error}");
            return 2;
        }
    };

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
    // A `/yolo` toggle persisted in the global config (written by the TUI like
    // the sandbox toggle; project config/state can override per workspace)
    // re-applies on the next TUI launch; the explicit `--yolo` flag also turns
    // it on. TUI-ONLY on purpose: the headless surfaces above keep
    // flag-explicit gating, so a `-p` script run never silently bypasses
    // approvals because of an interactive toggle.
    let yolo = args.yolo || cfg.yolo.unwrap_or(false);
    // The TUI keeps a template so Ctrl+T / resume / hot-switch can (re)assemble
    // engines.
    let template = EngineTemplate::new(cfg.clone(), cwd, Some(queue), yolo, sandbox, today)
        .with_question_queue(Some(question_queue))
        .with_tool_filter(tui_tool_filter);
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
        yolo,
        initial_access: engine_template.tool_access(),
        sandbox: args.sandbox,
        provider_names: cfg.providers.keys().cloned().collect(),
        needs_setup,
        update_applied: Some(update_applied),
    };
    // Mount the TUI as a harness UI plugin: the host owns its lifecycle,
    // so a future runtime swap (e.g. TUI -> app-server) is just a fiber
    // dispose + mount away.
    let exit: std::sync::Arc<std::sync::Mutex<i32>> = Default::default();
    let root = zode_core::cordis_rs::Context::root();
    let host = zode_core::ui::UiHost::new(
        &root,
        Arc::new(zode_core::ui::UiDeps {
            cwd: ui_deps_cwd.clone(),
            cfg: cfg.clone(),
        }),
    )
    .expect("ui host");
    host.register(Arc::new(crate::ui_frontends::TuiUi {
        parts: std::sync::Mutex::new(Some(crate::ui_frontends::TuiParts {
            engine,
            template,
            ui,
            approval_rx,
            question_rx,
            op_question_queue,
            resumed_id,
        })),
        browser_native_host: args.browser_native_host,
        exit: exit.clone(),
    }));
    if let Err(error) = host.run("tui").await {
        eprintln!("zode: {error}");
        return 1;
    }
    let code = *exit.lock().unwrap();
    code
}

/// `zode update` / `zode upgrade`: explicit self-update with console output.
/// Unlike the background auto-updater it ignores `autoUpdate: false` and
/// `ZODE_NO_UPDATE` (running the command IS the consent), but still refuses to
/// clobber a dev build. `--check` reports without downloading.
async fn run_self_update(check_only: bool) -> i32 {
    use zode_core::updater;
    let current = env!("CARGO_PKG_VERSION");
    let rel = match updater::latest_release().await {
        Ok(rel) => rel,
        Err(e) => {
            eprintln!("zode: update check failed: {e}");
            return 1;
        }
    };
    if !updater::is_newer(&rel.version, current) {
        println!(
            "zode {current} is up to date (latest release: {}).",
            rel.tag
        );
        return 0;
    }
    println!("zode {current} → {} available.", rel.tag);
    if check_only {
        println!("run `zode update` to install it.");
        return 0;
    }
    match std::env::current_exe() {
        Ok(exe) if updater::looks_like_dev_build(&exe) => {
            eprintln!(
                "zode: this looks like a dev build ({}) — refusing to overwrite it; \
                 use `cargo build` or download the release manually",
                exe.display()
            );
            return 1;
        }
        Err(e) => {
            eprintln!("zode: cannot locate the current executable: {e}");
            return 1;
        }
        Ok(_) => {}
    }
    let Some(url) = rel.asset_url.as_deref() else {
        eprintln!(
            "zode: release {} has no prebuilt binary for this platform — update manually",
            rel.tag
        );
        return 1;
    };
    println!("downloading {} …", rel.tag);
    match updater::download_and_apply(url).await {
        Ok(()) => {
            println!("updated to {} — takes effect on the next launch.", rel.tag);
            0
        }
        Err(e) => {
            eprintln!("zode: update failed: {e}");
            1
        }
    }
}

/// Spawn the silent background self-updater. No-op when disabled via
/// `autoUpdate: false` or `ZODE_NO_UPDATE`. Fully best-effort: it logs at debug
/// and never surfaces UI, per the "silently pull in the background" intent.
fn spawn_auto_update(
    cfg: &zode_core::config::ZodeConfig,
    applied: std::sync::Arc<std::sync::OnceLock<String>>,
) {
    if !cfg.auto_update() || std::env::var_os("ZODE_NO_UPDATE").is_some() {
        return;
    }
    tokio::spawn(async move {
        match zode_core::updater::auto_update_if_available(env!("CARGO_PKG_VERSION")).await {
            Ok(Some(tag)) => {
                tracing::info!("zode self-updated to {tag} (restart to apply)");
                let _ = applied.set(tag);
            }
            Ok(None) => {}
            Err(e) => tracing::debug!("auto-update skipped: {e}"),
        }
    });
}

/// Gate for the headless surfaces: bypass on --yolo, else a stdin prompt.
fn headless_gate(args: &Args) -> Arc<dyn ApprovalGate> {
    if args.yolo || args.permission_mode == PermissionModeArg::Bypass {
        return Arc::new(BypassGate);
    }
    match args.permission_mode {
        PermissionModeArg::Default => Arc::new(StdinGate::new()),
        PermissionModeArg::DontAsk => Arc::new(DenyGate),
        PermissionModeArg::AcceptEdits => Arc::new(AcceptEditsGate::default()),
        PermissionModeArg::Bypass => Arc::new(BypassGate),
    }
}

fn headless_tool_filter(
    args: &Args,
) -> Result<Option<zode_core::tool_filter::ToolFilter>, zode_core::CoreError> {
    let filter =
        zode_core::tool_filter::ToolFilter::new(args.tools.clone(), args.disallowed_tools.clone())?;
    Ok((!filter.is_empty()).then_some(filter))
}

/// Assemble the engine, reporting and returning None on error.
async fn build(
    cfg: &zode_core::config::ZodeConfig,
    cwd: PathBuf,
    gate: Arc<dyn ApprovalGate>,
    sandbox: Option<zode_core::sandbox::SandboxConfig>,
    date: &str,
    tool_filter: Option<&zode_core::tool_filter::ToolFilter>,
) -> Result<ZodeEngine, zode_core::CoreError> {
    // Headless surfaces have no UI to answer questions (None), no consent
    // channel for the op-bridge (None), don't enter plan mode (false), and
    // build a fresh, single-use browser session (None; no tab to share it with).
    ZodeEngine::assemble_with_tool_filter_and_mcp(
        cfg,
        cwd,
        gate,
        sandbox,
        date,
        None,
        None,
        false,
        None,
        tool_filter,
        None,
    )
    .await
}

fn load_headless_prompt(args: &Args) -> Result<Option<String>, String> {
    if let Some(prompt) = &args.print {
        return Ok(Some(prompt.clone()));
    }
    if let Some(path) = &args.prompt_file {
        let prompt = if path == "-" {
            use std::io::Read;
            let mut prompt = String::new();
            std::io::stdin()
                .read_to_string(&mut prompt)
                .map_err(|error| format!("read prompt from stdin: {error}"))?;
            prompt
        } else {
            std::fs::read_to_string(path)
                .map_err(|error| format!("read prompt file '{path}': {error}"))?
        };
        return nonempty_prompt(prompt);
    }
    if let Some(raw) = &args.prompt_json {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| format!("parse --prompt-json: {error}"))?;
        let prompt = match value {
            serde_json::Value::String(prompt) => prompt,
            serde_json::Value::Object(object) => object
                .get("prompt")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "--prompt-json object requires string field 'prompt'".to_string())?,
            _ => return Err("--prompt-json must be a string or object".into()),
        };
        return nonempty_prompt(prompt);
    }
    Ok(None)
}

fn nonempty_prompt(prompt: String) -> Result<Option<String>, String> {
    if prompt.trim().is_empty() {
        Err("headless prompt is empty".into())
    } else {
        Ok(Some(prompt))
    }
}

fn emit_startup_error(format: OutputFormat, code: &str, message: &str) {
    if format == OutputFormat::Json {
        let result = crate::headless_output::HeadlessResult::startup_failure(code, message);
        println!("{}", serde_json::to_string(&result).unwrap_or_default());
    } else if format == OutputFormat::StreamJson {
        let mut context = zode_core::run_event::RunEventContext::new("", None, None);
        for event in [
            zode_core::run_event::RunEvent::Error {
                code: code.to_string(),
                message: message.to_string(),
            },
            zode_core::run_event::RunEvent::RunCompleted {
                status: zode_core::run_event::RunStatus::Failed,
                stop_reason: Some("startup_error".into()),
                partial: false,
            },
        ] {
            println!(
                "{}",
                serde_json::to_string(&context.envelope(event)).unwrap_or_default()
            );
        }
    } else {
        eprintln!("zode: {message}");
    }
}

/// Today's date as YYYY-MM-DD (UTC) for the system prompt's env block.
fn today_date() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
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
