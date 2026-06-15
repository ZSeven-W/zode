//! User/plugin slash commands: Markdown prompt files (`commands/<name>.md`)
//! from the agent ecosystem (Claude/codex/opencode plugins + project/global
//! `.claude/commands`, `.zode/commands`, …). Each becomes a dynamic slash
//! command whose body is submitted as a templated turn (the TUI wires this like
//! agents/skills). Optional frontmatter `description:` gives the palette hint.

use std::path::{Path, PathBuf};

use crate::agents::split_frontmatter_pub;
use crate::config::ConfigManager;

/// A discovered command: `name` (file stem), one-line `description`, and the
/// prompt `body` (frontmatter stripped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCommand {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// `commands/` dirs across the ecosystem, low → high precedence (zode last).
///
/// We deliberately do NOT scan foreign agents' installed *plugin* trees
/// (`~/.claude/plugins`, `~/.codex/plugins`, opencode plugins) for commands.
/// Those plugins (codex, claude-hud, ralph-loop, claude-plugins-official, …)
/// ship dozens of product-specific slash commands — `/rescue` delegates to the
/// Codex subagent, `/setup` configures claude-hud's statusline, etc. — that
/// drive that product's own tooling and cannot work under zode. Loading them
/// floods zode's palette with broken entries (the user's "还是有很多"). Same
/// policy as foreign MCP servers: another product's installed integrations are
/// off by default; to use one under zode, copy it into a `.zode/commands` dir.
///
/// We DO still load the user's own direct command dirs (`.claude/commands`,
/// `.codex/commands`, global + project) for cross-agent portability, and
/// zode's own plugin tree + command dirs. Non-portable entries among those
/// (referencing a foreign host var) are dropped in [`load_user_commands`].
pub fn commands_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = dirs::home_dir();
    if let Some(h) = &home {
        // Direct global command dirs the user maintains (not plugin trees).
        dirs.push(h.join(".claude").join("commands"));
        dirs.push(h.join(".codex").join("commands"));
    }
    dirs.push(cwd.join(".claude").join("commands"));
    dirs.push(cwd.join(".codex").join("commands"));
    // zode's own (highest): plugin tree + direct, global then project.
    if let Ok(global) = ConfigManager::config_dir() {
        collect_named_dirs(&global.join("plugins"), "commands", &mut dirs);
        dirs.push(global.join("commands"));
    }
    collect_named_dirs(&cwd.join(".zode").join("plugins"), "commands", &mut dirs);
    dirs.push(cwd.join(".zode").join("commands"));
    dirs
}

/// Recursively collect dirs named `name` under `root` (depth ≤6).
fn collect_named_dirs(root: &Path, name: &str, out: &mut Vec<PathBuf>) {
    fn walk(dir: &Path, name: &str, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            if p.file_name().and_then(|n| n.to_str()) == Some(name) {
                out.push(p);
            } else {
                walk(&p, name, depth + 1, out);
            }
        }
    }
    walk(root, name, 0, out);
}

/// Load all command files. Same-name later (higher-precedence) dirs win. `.md`
/// files only; a missing frontmatter is fine (whole file is the body).
pub fn load_user_commands(cwd: &Path) -> Vec<UserCommand> {
    let mut by_name: std::collections::BTreeMap<String, UserCommand> = Default::default();
    for dir in commands_dirs(cwd) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Skip claude SKILL.md-style files that live elsewhere; command
            // files are arbitrary names.
            if stem.eq_ignore_ascii_case("README") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let (description, body) = parse_command(&text);
            // Drop commands that hard-code another agent's host variables (e.g.
            // a Claude plugin command invoking `${CLAUDE_PLUGIN_ROOT}/…`): that
            // path is unset under zode, so the command can only fail. Keep the
            // ecosystem's portable commands; skip the non-portable ones.
            if let Some(var) = crate::portability::foreign_host_var(&body) {
                tracing::debug!(
                    "skip non-portable command {} ({}): references ${}",
                    stem,
                    p.display(),
                    var
                );
                continue;
            }
            by_name.insert(
                stem.to_string(),
                UserCommand {
                    name: stem.to_string(),
                    description,
                    body,
                },
            );
        }
    }
    by_name.into_values().collect()
}

/// Split optional frontmatter (for a `description:`) from the prompt body.
fn parse_command(text: &str) -> (String, String) {
    if let Some((front, body)) = split_frontmatter_pub(text) {
        let mut description = String::new();
        for line in front.lines() {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim() == "description" {
                    description = v.trim().trim_matches('"').to_string();
                }
            }
        }
        if description.is_empty() {
            description = first_line(body);
        }
        (description, body.trim().to_string())
    } else {
        (first_line(text), text.trim().to_string())
    }
}

fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(60)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_description_and_body() {
        let (desc, body) =
            parse_command("---\ndescription: Run the linter\n---\nRun `cargo clippy` and fix.\n");
        assert_eq!(desc, "Run the linter");
        assert_eq!(body, "Run `cargo clippy` and fix.");
    }

    #[test]
    fn falls_back_to_first_line() {
        let (desc, body) = parse_command("# Tidy imports\nGroup and sort imports.\n");
        assert_eq!(desc, "Tidy imports");
        assert!(body.contains("Group and sort"));
    }

    /// `commands_dirs` must never scan another agent's installed plugin trees —
    /// those ship product-specific commands (codex `/rescue`, claude-hud
    /// `/setup`, …) that flood the palette and don't work under zode.
    #[test]
    fn excludes_foreign_plugin_trees() {
        let cwd = Path::new("/tmp/zode-cmd-test");
        for d in commands_dirs(cwd) {
            let s = d.to_string_lossy().replace('\\', "/");
            assert!(
                !(s.contains("/.claude/plugins")
                    || s.contains("/.codex/plugins")
                    || s.contains("/opencode/plugin")),
                "foreign plugin command tree must not be scanned: {s}"
            );
        }
    }

    /// Foreign plugin commands aren't loaded even when present; portable direct
    /// commands are kept, and ones hard-coding a foreign host var are dropped.
    #[test]
    #[serial_test::serial]
    fn skips_foreign_plugins_and_non_portable() {
        let home = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());

        // A codex-style plugin command in the foreign plugin tree.
        let plugin_cmds = home
            .path()
            .join(".claude")
            .join("plugins")
            .join("openai-codex")
            .join("commands");
        std::fs::create_dir_all(&plugin_cmds).unwrap();
        std::fs::write(plugin_cmds.join("rescue.md"), "Delegate to Codex.").unwrap();

        // The user's own direct command dir: one portable, one non-portable.
        let cwd = tempfile::tempdir().unwrap();
        let direct = cwd.path().join(".claude").join("commands");
        std::fs::create_dir_all(&direct).unwrap();
        std::fs::write(direct.join("tidy.md"), "Tidy the imports.").unwrap();
        std::fs::write(direct.join("hud.md"), "Run ${CLAUDE_PLUGIN_ROOT}/setup.sh.").unwrap();

        let names: Vec<String> = load_user_commands(cwd.path())
            .into_iter()
            .map(|c| c.name)
            .collect();

        match prev {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }

        assert!(
            names.contains(&"tidy".to_string()),
            "portable kept: {names:?}"
        );
        assert!(
            !names.contains(&"rescue".to_string()),
            "foreign plugin command must not load: {names:?}"
        );
        assert!(
            !names.contains(&"hud".to_string()),
            "non-portable command must be dropped: {names:?}"
        );
    }
}
