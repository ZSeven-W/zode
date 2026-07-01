//! `zode doctor` — diagnose environment / configuration problems that block
//! startup or updates, and report whether a newer release is available. Prints a
//! checklist and returns a non-zero exit code only on a hard failure.

use std::io::{IsTerminal, Write};
use std::path::Path;

use zode_core::config::ConfigManager;
use zode_core::sandbox::{self, SandboxMode};
use zode_core::updater;

/// Outcome of a single check.
enum Status {
    Ok(String),
    Warn(String),
    Fail(String),
}

impl Status {
    fn glyph(&self) -> &'static str {
        match self {
            Status::Ok(_) => "✓",
            Status::Warn(_) => "⚠",
            Status::Fail(_) => "✗",
        }
    }
    fn msg(&self) -> &str {
        match self {
            Status::Ok(m) | Status::Warn(m) | Status::Fail(m) => m,
        }
    }
}

fn print_line(label: &str, s: &Status) {
    println!("  {} {label:<12}  {}", s.glyph(), s.msg());
}

/// Run all checks, print the report, and return an exit code (0 = no hard
/// failures; warnings don't fail).
pub async fn run(cwd: &Path) -> i32 {
    println!("zode doctor  ({})\n", env!("CARGO_PKG_VERSION"));
    let mut hard_fail = false;

    // Mirror a normal launch: create the starter config if missing, so the
    // report reflects the state zode actually boots with. Best-effort.
    let _ = ConfigManager::ensure_default_global();

    // Config directory + writability (updates and `/connect` need to write here).
    let cfg_dir_status = match ConfigManager::config_dir() {
        Ok(dir) => match probe_writable(&dir) {
            Ok(()) => Status::Ok(format!("{} (writable)", dir.display())),
            Err(e) => Status::Warn(format!("{} not writable: {e}", dir.display())),
        },
        Err(e) => Status::Fail(format!("cannot resolve config dir: {e}")),
    };
    print_line("config dir", &cfg_dir_status);

    // Config load — a parse error here blocks startup, so it's a hard failure.
    let cfg = ConfigManager::load(cwd);
    match &cfg {
        Ok(_) => print_line("config", &Status::Ok("loads cleanly".into())),
        Err(e) => {
            hard_fail = true;
            print_line(
                "config",
                &Status::Fail(format!("{e} — fix or remove config.json")),
            );
        }
    }

    // Provider + credentials. Missing creds is a warning (the TUI still launches
    // and can /connect), not a hard failure.
    let provider_status = match &cfg {
        Ok(c) => {
            let kind = c.provider.kind();
            let has_key = c.provider.api_key.as_deref().is_some_and(|k| !k.is_empty());
            let model = c.provider.model.as_deref().unwrap_or("(none)");
            if has_key || kind == zode_core::config::ProviderKind::Ollama {
                Status::Ok(format!("{kind:?} · {model}"))
            } else {
                Status::Warn(format!(
                    "{kind:?} · {model} — no API key; run /connect or set provider.apiKey"
                ))
            }
        }
        Err(_) => Status::Warn("skipped (config did not load)".into()),
    };
    print_line("provider", &provider_status);

    // Sandbox backend (shell commands run confined). Missing backend is a
    // warning — the user can run with --no-sandbox.
    let sandbox_status =
        match sandbox::resolve(cwd, true, SandboxMode::default(), false, &[], false, false) {
            Ok(Some(_)) => Status::Ok("backend available".into()),
            Ok(None) => Status::Ok("disabled".into()),
            Err(e) => Status::Warn(format!("{e}")),
        };
    print_line("sandbox", &sandbox_status);

    // Terminal capability (the full TUI needs a tty).
    let term = std::env::var("TERM").unwrap_or_default();
    let tty = std::io::stdout().is_terminal();
    let term_status = if tty {
        Status::Ok(format!(
            "tty · TERM={}",
            if term.is_empty() { "(unset)" } else { &term }
        ))
    } else {
        Status::Warn(
            "stdout is not a tty — the full TUI needs one (use -p / --no-tui otherwise)".into(),
        )
    };
    print_line("terminal", &term_status);

    // Network + updates (best-effort — offline is a warning, not a failure).
    print!("  … checking for updates…\r");
    let _ = std::io::stdout().flush();
    let update_status = match updater::latest_release().await {
        Ok(rel) => {
            let current = env!("CARGO_PKG_VERSION");
            if updater::is_newer(&rel.version, current) {
                let note = if rel.asset_url.is_some() {
                    ""
                } else {
                    " (no prebuilt binary for this platform)"
                };
                Status::Warn(format!(
                    "update available: {} (have {current}){note}",
                    rel.tag
                ))
            } else {
                Status::Ok(format!("up to date ({current})"))
            }
        }
        Err(e) => Status::Warn(format!("could not reach GitHub: {e}")),
    };
    print_line("updates", &update_status);

    println!();
    if hard_fail {
        println!("✗ found a problem that blocks startup — see above.");
        1
    } else {
        println!("✓ no blocking problems.");
        0
    }
}

/// Confirm `dir` (created if missing) accepts a write, then clean up the probe.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".doctor-probe.{}", std::process::id()));
    std::fs::write(&probe, b"ok")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
