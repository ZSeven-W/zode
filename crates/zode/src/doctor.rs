//! `zode doctor` — diagnose environment / configuration problems that block
//! startup or updates, and report whether a newer release is available.
//! Renders a claude-doctor-style grouped report (section header + status
//! glyph, tree-branch detail lines) and returns a non-zero exit code only on
//! a hard failure.

use std::io::{IsTerminal, Write};
use std::path::Path;

use agent::mcp::{McpConfig, McpServerConfig};
use zode_core::config::{ConfigManager, LspServerConfig};
use zode_core::lsp::install::{self, Install};
use zode_core::plugin::PluginManager;
use zode_core::sandbox::{self, SandboxMode};
use zode_core::updater;

/// Severity of a check line. A section reports the worst of its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    fn glyph(self) -> &'static str {
        match self {
            Level::Ok => "✓",
            Level::Warn => "⚠",
            Level::Fail => "✗",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Level::Ok => GREEN,
            Level::Warn => YELLOW,
            Level::Fail => RED,
        }
    }
}

/// One "Key: value" detail line under a section.
struct Line {
    level: Level,
    text: String,
}

impl Line {
    fn ok(text: impl Into<String>) -> Self {
        Line {
            level: Level::Ok,
            text: text.into(),
        }
    }
    fn warn(text: impl Into<String>) -> Self {
        Line {
            level: Level::Warn,
            text: text.into(),
        }
    }
    fn fail(text: impl Into<String>) -> Self {
        Line {
            level: Level::Fail,
            text: text.into(),
        }
    }
}

/// A titled group of check lines, rendered claude-doctor style:
///
/// ```text
///  Config ✓
///  ├ Directory: ~/.zode (writable)
///  └ Provider: Anthropic · some-model
/// ```
struct Section {
    title: &'static str,
    lines: Vec<Line>,
}

impl Section {
    fn level(&self) -> Level {
        self.lines
            .iter()
            .map(|l| l.level)
            .max()
            .unwrap_or(Level::Ok)
    }
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

fn paint(ansi: bool, code: &str, s: &str) -> String {
    if ansi {
        format!("{code}{s}{RESET}")
    } else {
        s.to_string()
    }
}

/// Render the report body as rows (no line endings). `ansi` gates color.
fn render(sections: &[Section], ansi: bool) -> Vec<String> {
    let mut rows = Vec::new();
    for sec in sections {
        let lv = sec.level();
        rows.push(format!(
            " {} {}",
            paint(ansi, BOLD, sec.title),
            paint(ansi, lv.color(), lv.glyph())
        ));
        for (i, line) in sec.lines.iter().enumerate() {
            let branch = if i + 1 == sec.lines.len() {
                "└"
            } else {
                "├"
            };
            let text = match line.level {
                Level::Ok => line.text.clone(),
                lv => paint(ansi, lv.color(), &line.text),
            };
            rows.push(format!(" {} {text}", paint(ansi, DIM, branch)));
        }
        rows.push(String::new());
    }
    rows
}

/// Join rows into the final output. Every row ends with `eol`: on a tty
/// that is `\r\n`, because a previous program may have left the terminal
/// in raw mode where a bare `\n` no longer implies a carriage return —
/// plain `println!` output then staircases across the screen.
fn assemble(rows: &[String], eol: &str) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(row);
        out.push_str(eol);
    }
    out
}

/// One-line transport summary of an MCP server spec.
fn describe_mcp(spec: &McpServerConfig) -> String {
    match spec {
        McpServerConfig::Stdio { command, args, .. } if args.is_empty() => {
            format!("stdio · {command}")
        }
        McpServerConfig::Stdio { command, args, .. } => {
            format!("stdio · {command} {}", args.join(" "))
        }
        McpServerConfig::Sse { url, .. } => format!("sse · {url}"),
        McpServerConfig::WebSocket { url, .. } => format!("websocket · {url}"),
        // The enum is #[non_exhaustive]; a future transport still lists.
        _ => "(unknown transport)".into(),
    }
}

/// Lines for the "MCP servers" section from the merged discovery result.
/// `gate` is the plugin-manager check (`/mcp` toggles, `plugins.disabled`);
/// a server connects only when its own `enabled` flag AND the gate allow it.
fn mcp_lines(config: Option<&McpConfig>, gate: impl Fn(&str) -> bool) -> Vec<Line> {
    let Some(config) = config else {
        return vec![Line::ok(
            "None configured (add servers to ~/.zode/mcp.json or .mcp.json)",
        )];
    };
    let on = |name: &str, spec: &McpServerConfig| spec.enabled() && gate(name);
    let enabled = config.servers.iter().filter(|(n, s)| on(n, s)).count();
    let mut lines = vec![Line::ok(format!(
        "Configured: {} ({enabled} enabled)",
        config.servers.len()
    ))];
    for (name, spec) in &config.servers {
        let suffix = if on(name, spec) {
            ""
        } else {
            " (disabled — enable in /mcp)"
        };
        lines.push(Line::ok(format!("{name}: {}{suffix}", describe_mcp(spec))));
    }
    lines
}

/// Readiness of one LSP language, probed without spawning anything.
enum LspState {
    /// Runnable now, at this path.
    Ready(String),
    /// Not installed yet; installs on first use via this tool.
    AutoInstall(&'static str),
    /// Configured but the command can't be found (and no installer).
    Missing(String),
}

/// Probe one effective LSP entry: built-in languages go through the same
/// resolve/installable logic the manager uses on first tool call; a
/// user-overridden or user-defined command is checked as-is.
fn lsp_probe(lang: &str, sc: &LspServerConfig) -> LspState {
    if let Some(spec) = install::spec_for_lang(lang) {
        if sc.command == spec.command {
            if let Some(p) = install::resolve(spec) {
                return LspState::Ready(p.display().to_string());
            }
            if install::installable(spec) {
                let tool = match spec.install {
                    Install::Npm { .. } => "npm",
                    Install::Rustup { .. } => "rustup",
                    Install::Go { .. } => "go",
                    Install::Manual => "manual",
                };
                return LspState::AutoInstall(tool);
            }
            return LspState::Missing(sc.command.clone());
        }
    }
    let p = std::path::Path::new(&sc.command);
    if (p.is_absolute() && p.is_file()) || install::on_path(&sc.command) {
        LspState::Ready(sc.command.clone())
    } else {
        LspState::Missing(sc.command.clone())
    }
}

/// Lines for the "LSP" section from probed `(language, state)` rows.
fn lsp_lines(rows: &[(String, LspState)]) -> Vec<Line> {
    if rows.is_empty() {
        return vec![Line::ok(
            "None available (no server on PATH and no installer — npm/rustup/go — found)",
        )];
    }
    let ready = rows
        .iter()
        .filter(|(_, s)| matches!(s, LspState::Ready(_)))
        .count();
    let mut lines = vec![Line::ok(format!(
        "Languages: {} ({ready} ready)",
        rows.len()
    ))];
    for (lang, state) in rows {
        lines.push(match state {
            LspState::Ready(path) => Line::ok(format!("{lang}: ready · {path}")),
            LspState::AutoInstall(tool) => {
                Line::ok(format!("{lang}: installs on first use ({tool})"))
            }
            LspState::Missing(cmd) => Line::warn(format!("{lang}: command not found: {cmd}")),
        });
    }
    lines
}

/// Lines for the "Skills" section from `(name, enabled)` pairs.
fn skill_lines(skills: &[(String, bool)]) -> Vec<Line> {
    if skills.is_empty() {
        return vec![Line::ok(
            "None found (add skills to ~/.zode/skills or .zode/skills)",
        )];
    }
    let enabled = skills.iter().filter(|(_, on)| *on).count();
    let mut lines = vec![Line::ok(format!(
        "Loaded: {} ({enabled} enabled)",
        skills.len()
    ))];
    for (name, on) in skills {
        let suffix = if *on {
            ""
        } else {
            " (disabled — enable in /plugin)"
        };
        lines.push(Line::ok(format!("{name}{suffix}")));
    }
    lines
}

/// Exit code: non-zero only when a hard failure (something that blocks
/// startup) was found; warnings don't fail.
fn exit_code(sections: &[Section]) -> i32 {
    if sections.iter().any(|s| s.level() == Level::Fail) {
        1
    } else {
        0
    }
}

/// Run all checks, print the report, and return the exit code.
pub async fn run(cwd: &Path) -> i32 {
    let tty = std::io::stdout().is_terminal();

    // Mirror a normal launch: create the starter config if missing, so the
    // report reflects the state zode actually boots with. Best-effort.
    let _ = ConfigManager::ensure_default_global();

    let mut sections = Vec::new();

    // ---- Diagnostics: what is running, where.
    let binary = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".into());
    sections.push(Section {
        title: "Diagnostics",
        lines: vec![
            Line::ok(format!("Version: {}", env!("CARGO_PKG_VERSION"))),
            Line::ok(format!(
                "Platform: {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            Line::ok(format!("Binary: {binary}")),
        ],
    });

    // ---- Config: dir writability (updates and `/connect` write here), file
    // parse (an error blocks startup — hard failure), provider credentials
    // (missing key is a warning: the TUI still launches and can /connect).
    let dir_line = match ConfigManager::config_dir() {
        Ok(dir) => match probe_writable(&dir) {
            Ok(()) => Line::ok(format!("Directory: {} (writable)", dir.display())),
            Err(e) => Line::warn(format!("Directory: {} not writable: {e}", dir.display())),
        },
        Err(e) => Line::fail(format!("Directory: cannot resolve config dir: {e}")),
    };
    let cfg = ConfigManager::load(cwd);
    let file_line = match &cfg {
        Ok(_) => Line::ok("File: loads cleanly"),
        Err(e) => Line::fail(format!("File: {e} — fix or remove config.json")),
    };
    let provider_line = match &cfg {
        Ok(c) => {
            let kind = c.provider.kind();
            let has_key = c.provider.api_key.as_deref().is_some_and(|k| !k.is_empty());
            let model = c.provider.model.as_deref().unwrap_or("(none)");
            if has_key || kind == zode_core::config::ProviderKind::Ollama {
                Line::ok(format!("Provider: {kind:?} · {model}"))
            } else {
                Line::warn(format!(
                    "Provider: {kind:?} · {model} — no API key; run /connect or set provider.apiKey"
                ))
            }
        }
        Err(_) => Line::warn("Provider: skipped (config did not load)"),
    };
    sections.push(Section {
        title: "Config",
        lines: vec![dir_line, file_line, provider_line],
    });

    // ---- Sandbox backend (shell commands run confined). Missing backend is
    // a warning — the user can run with --no-sandbox.
    let sandbox_line =
        match sandbox::resolve(cwd, true, SandboxMode::default(), false, &[], false, false) {
            Ok(Some(_)) => Line::ok("Backend: available"),
            Ok(None) => Line::ok("Backend: disabled"),
            Err(e) => Line::warn(format!("Backend: {e}")),
        };
    sections.push(Section {
        title: "Sandbox",
        lines: vec![sandbox_line],
    });

    // ---- MCP servers + Skills: what a launch would discover. Reported
    // without connecting/spawning anything — connection status belongs to
    // the running TUI (`/mcp`); doctor stays fast and side-effect-free.
    // The plugin gate mirrors assembly (`plugins.disabled` in config).
    let plugins = cfg.as_ref().ok().map(PluginManager::from_config);
    let mcp_config = zode_core::mcp::discover_mcp_config(cwd);
    sections.push(Section {
        title: "MCP servers",
        lines: mcp_lines(mcp_config.as_ref(), |name| {
            plugins.as_ref().is_none_or(|p| p.mcp_enabled(name))
        }),
    });
    let mut skills: Vec<(String, bool)> =
        zode_core::skills::load_skills_from(&zode_core::skills::skills_dirs(cwd))
            .list()
            .iter()
            .map(|s| {
                let on = plugins.as_ref().is_none_or(|p| p.skill_enabled(&s.name));
                (s.name.clone(), on)
            })
            .collect();
    skills.sort();
    sections.push(Section {
        title: "Skills",
        lines: skill_lines(&skills),
    });

    // ---- LSP: the same auto-detection a launch performs (servers on PATH
    // or installable via npm/rustup/go, plus user config), with each
    // language's readiness — makes "why didn't my language server start"
    // inspectable without spawning or installing anything.
    let user_lsp = cfg
        .as_ref()
        .map(|c| c.lsp.servers.clone())
        .unwrap_or_default();
    let mut lsp_rows: Vec<(String, LspState)> = zode_core::lsp::effective_servers(&user_lsp)
        .into_iter()
        .map(|(lang, sc)| {
            let state = lsp_probe(&lang, &sc);
            (lang, state)
        })
        .collect();
    lsp_rows.sort_by(|a, b| a.0.cmp(&b.0));
    sections.push(Section {
        title: "LSP",
        lines: lsp_lines(&lsp_rows),
    });

    // ---- Terminal capability (the full TUI needs a tty).
    let term = std::env::var("TERM").unwrap_or_default();
    let terminal_lines = if tty {
        vec![
            Line::ok("Output: tty"),
            Line::ok(format!(
                "TERM: {}",
                if term.is_empty() { "(unset)" } else { &term }
            )),
        ]
    } else {
        vec![Line::warn(
            "Output: stdout is not a tty — the full TUI needs one (use -p / --no-tui otherwise)",
        )]
    };
    sections.push(Section {
        title: "Terminal",
        lines: terminal_lines,
    });

    // ---- Network + updates (best-effort — offline is a warning, not a
    // failure). Show transient progress on a tty, then erase it.
    if tty {
        print!("… checking for updates…\r");
        let _ = std::io::stdout().flush();
    }
    let update_line = match updater::latest_release().await {
        Ok(rel) => {
            let current = env!("CARGO_PKG_VERSION");
            if updater::is_newer(&rel.version, current) {
                let note = if rel.asset_url.is_some() {
                    ""
                } else {
                    " (no prebuilt binary for this platform)"
                };
                Line::warn(format!(
                    "Update available: {} (have {current}){note}",
                    rel.tag
                ))
            } else {
                Line::ok(format!("Up to date ({current})"))
            }
        }
        Err(e) => Line::warn(format!("Could not reach GitHub: {e}")),
    };
    if tty {
        // Erase the progress line so the report starts on a clean row.
        print!("\r\x1b[2K");
    }
    sections.push(Section {
        title: "Updates",
        lines: vec![update_line],
    });

    // ---- Assemble and print.
    let code = exit_code(&sections);
    let mut rows = vec![
        format!(
            "{} ({})",
            paint(tty, BOLD, "zode doctor"),
            env!("CARGO_PKG_VERSION")
        ),
        String::new(),
    ];
    rows.extend(render(&sections, tty));
    rows.push(if code == 0 {
        paint(tty, GREEN, "✓ no blocking problems.")
    } else {
        paint(
            tty,
            RED,
            "✗ found a problem that blocks startup — see above.",
        )
    });
    let eol = if tty { "\r\n" } else { "\n" };
    print!("{}", assemble(&rows, eol));
    let _ = std::io::stdout().flush();
    code
}

/// Confirm `dir` (created if missing) accepts a write, then clean up the probe.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".doctor-probe.{}", std::process::id()));
    std::fs::write(&probe, b"ok")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(lines: Vec<Line>) -> Section {
        Section {
            title: "Test",
            lines,
        }
    }

    #[test]
    fn section_level_is_the_worst_of_its_lines() {
        assert_eq!(section(vec![]).level(), Level::Ok);
        assert_eq!(section(vec![Line::ok("a")]).level(), Level::Ok);
        assert_eq!(
            section(vec![Line::ok("a"), Line::warn("b")]).level(),
            Level::Warn
        );
        assert_eq!(
            section(vec![Line::warn("a"), Line::fail("b"), Line::ok("c")]).level(),
            Level::Fail
        );
    }

    #[test]
    fn render_uses_tree_branches_and_section_glyph() {
        let sec = section(vec![
            Line::ok("First: a"),
            Line::ok("Mid: b"),
            Line::warn("Last: c"),
        ]);
        let rows = render(&[sec], false);
        assert_eq!(rows[0], " Test ⚠", "header carries the worst-level glyph");
        assert_eq!(rows[1], " ├ First: a");
        assert_eq!(rows[2], " ├ Mid: b");
        assert_eq!(rows[3], " └ Last: c", "final line uses the corner branch");
        assert_eq!(rows[4], "", "sections end with a blank spacer row");
    }

    #[test]
    fn render_gates_ansi_on_the_flag() {
        let sec = section(vec![Line::warn("W: x")]);
        let plain = render(&[sec], false);
        assert!(plain.iter().all(|r| !r.contains('\x1b')));
        let sec = section(vec![Line::warn("W: x")]);
        let colored = render(&[sec], true);
        assert!(colored.iter().any(|r| r.contains(YELLOW)));
        assert!(colored.iter().any(|r| r.contains(BOLD)));
    }

    #[test]
    fn assemble_applies_the_eol_to_every_row() {
        // Regression: on a tty the report must end rows with \r\n — a
        // previous program can leave the terminal in raw mode where bare
        // \n doesn't return the carriage, staircasing the output.
        let rows = vec!["a".to_string(), "b".to_string()];
        assert_eq!(assemble(&rows, "\r\n"), "a\r\nb\r\n");
        assert_eq!(assemble(&rows, "\n"), "a\nb\n");
    }

    #[test]
    fn mcp_lines_report_transport_and_effective_enablement() {
        let cfg = agent::mcp::parse_json_str(
            r#"{"servers":{
                "alpha": {"transport":"stdio","command":"npx","args":["-y","alpha-mcp"]},
                "beta":  {"transport":"sse","url":"https://beta.example/mcp","enabled":false},
                "gamma": {"transport":"stdio","command":"gamma"}
            }}"#,
        )
        .unwrap();
        // Plugin gate turns gamma off even though its own flag is true.
        let lines = mcp_lines(Some(&cfg), |name| name != "gamma");
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts[0], "Configured: 3 (1 enabled)");
        assert_eq!(texts[1], "alpha: stdio · npx -y alpha-mcp");
        assert_eq!(
            texts[2],
            "beta: sse · https://beta.example/mcp (disabled — enable in /mcp)"
        );
        assert_eq!(texts[3], "gamma: stdio · gamma (disabled — enable in /mcp)");
    }

    #[test]
    fn mcp_lines_without_config_point_at_the_config_files() {
        let lines = mcp_lines(None, |_| true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("None configured"));
    }

    #[test]
    fn skill_lines_report_counts_and_disabled_markers() {
        assert!(skill_lines(&[])[0].text.contains("None found"));
        let skills = vec![
            ("commit-helper".to_string(), true),
            ("scratch".to_string(), false),
        ];
        let lines = skill_lines(&skills);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts[0], "Loaded: 2 (1 enabled)");
        assert_eq!(texts[1], "commit-helper");
        assert_eq!(texts[2], "scratch (disabled — enable in /plugin)");
    }

    #[test]
    fn lsp_lines_report_readiness_and_pending_installs() {
        let rows = vec![
            (
                "rust".to_string(),
                LspState::Ready("/home/u/.cargo/bin/rust-analyzer".into()),
            ),
            ("typescript".to_string(), LspState::AutoInstall("npm")),
            ("weird".to_string(), LspState::Missing("weird-ls".into())),
        ];
        let lines = lsp_lines(&rows);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts[0], "Languages: 3 (1 ready)");
        assert_eq!(texts[1], "rust: ready · /home/u/.cargo/bin/rust-analyzer");
        assert_eq!(texts[2], "typescript: installs on first use (npm)");
        assert_eq!(texts[3], "weird: command not found: weird-ls");
        assert_eq!(lines[3].level, Level::Warn);
        assert!(lsp_lines(&[])[0].text.contains("None available"));
    }

    #[test]
    fn exit_code_fails_only_on_hard_failures() {
        assert_eq!(exit_code(&[section(vec![Line::ok("a")])]), 0);
        assert_eq!(exit_code(&[section(vec![Line::warn("a")])]), 0);
        assert_eq!(
            exit_code(&[section(vec![Line::ok("a")]), section(vec![Line::fail("b")])]),
            1
        );
    }
}
