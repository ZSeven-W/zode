//! Capability detection: walk one installed plugin's tree and classify what
//! it declares (skills / MCP servers / hooks) into a `Vec<Capability>` for
//! the manifest. Skill and MCP discovery REUSE the exact walk+parse logic the
//! engine already uses for the live registries (`crate::skills`,
//! `crate::mcp`) rather than duplicating it — a plugin installed here is
//! discovered by those same functions once it's on disk, so the scan must
//! agree with them on what counts as a skill / an MCP declaration.

use std::path::{Path, PathBuf};

use super::manifest::Capability;
use crate::hooks_config::HooksConfig;

/// Bounded walk depth for the hooks.json search — matches the depth bound
/// `crate::skills::collect_plugin_skill_dirs` / `crate::mcp::collect_plugin_mcp`
/// already use for the same kind of plugin-tree walk.
const MAX_DEPTH: usize = 6;

/// Scan `plugin_dir` (an installed plugin's own root) for every capability it
/// declares. Best-effort: unreadable/unparseable files are skipped, matching
/// the existing discovery functions' failure mode — a partially-broken
/// plugin still installs with whatever did parse, rather than hard-erroring.
pub fn scan_capabilities(plugin_dir: &Path) -> Vec<Capability> {
    let mut caps = Vec::new();
    caps.extend(scan_skills(plugin_dir));
    caps.extend(scan_mcp(plugin_dir));
    caps.extend(scan_hooks(plugin_dir));
    caps
}

fn relative_to(plugin_dir: &Path, path: &Path) -> String {
    path.strip_prefix(plugin_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn scan_skills(plugin_dir: &Path) -> Vec<Capability> {
    let mut dirs = Vec::new();
    crate::skills::collect_plugin_skill_dirs(plugin_dir, &mut dirs);
    let mut caps = Vec::new();
    for dir in dirs {
        let Ok(outcome) = agent::skills::load_dir(&dir) else {
            continue;
        };
        for skill in outcome.skills {
            let path = relative_to(plugin_dir, &dir.join(&skill.name));
            caps.push(Capability::Skill {
                name: skill.name,
                path,
            });
        }
    }
    caps
}

fn scan_mcp(plugin_dir: &Path) -> Vec<Capability> {
    let mut servers = serde_json::Map::new();
    crate::mcp::collect_plugin_mcp(plugin_dir, &mut servers);
    let mut caps = Vec::new();
    for (name, spec) in servers {
        let Some(obj) = spec.as_object() else {
            continue;
        };
        let (command, args) = match obj.get("transport").and_then(|v| v.as_str()) {
            Some("stdio") => (
                obj.get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                obj.get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            // Remote transport: the URL IS the exec surface (zode will POST
            // requests to it) — still worth a trust review, just with no args.
            _ => (
                obj.get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                Vec::new(),
            ),
        };
        caps.push(Capability::Mcp {
            name,
            command,
            args,
        });
    }
    caps
}

fn scan_hooks(plugin_dir: &Path) -> Vec<Capability> {
    let mut hook_files = Vec::new();
    walk_for_hooks_json(plugin_dir, 0, &mut hook_files);
    let mut caps = Vec::new();
    for path in hook_files {
        let Ok(s) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(cfg) = serde_json::from_str::<HooksConfig>(&s) else {
            continue;
        };
        let parent = path.parent().unwrap_or(plugin_dir);
        for entry in cfg.hooks {
            let script_path = resolve_script_path(parent, &entry.script);
            caps.push(Capability::Hook {
                event: entry.event,
                tool: entry.tool,
                script: relative_to(plugin_dir, &script_path),
            });
        }
    }
    caps
}

fn walk_for_hooks_json(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_for_hooks_json(&p, depth + 1, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some("hooks.json") {
            out.push(p);
        }
    }
}

/// A hook script path is either absolute, `~/`-relative, or (the common case
/// for a plugin bundling its own script) relative to the `hooks.json` that
/// declared it.
fn resolve_script_path(hooks_json_dir: &Path, script: &str) -> PathBuf {
    if let Some(rest) = script.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let p = Path::new(script);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        hooks_json_dir.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn scans_a_skill() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("skills/review/SKILL.md"),
            "---\ndescription: review code\n---\nDo the review.",
        );
        let caps = scan_capabilities(dir.path());
        assert!(caps
            .iter()
            .any(|c| matches!(c, Capability::Skill { name, .. } if name == "review")));
    }

    #[test]
    fn scans_an_mcp_server() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join(".mcp.json"),
            r#"{"mcpServers":{"srv":{"command":"npx","args":["-y","tool"]}}}"#,
        );
        let caps = scan_capabilities(dir.path());
        assert!(caps.iter().any(
            |c| matches!(c, Capability::Mcp { name, command, .. } if name == "srv" && command == "npx")
        ));
    }

    #[test]
    fn scans_a_hook_and_resolves_its_script_relative_to_hooks_json() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("hooks/hooks.json"),
            r#"{"hooks":[{"event":"before_tool_use","tool":"Bash","script":"check.sh"}]}"#,
        );
        write(&dir.path().join("hooks/check.sh"), "#!/bin/sh\necho hi\n");
        let caps = scan_capabilities(dir.path());
        let hook = caps
            .iter()
            .find(|c| matches!(c, Capability::Hook { .. }))
            .expect("hook capability found");
        match hook {
            Capability::Hook { script, event, .. } => {
                assert_eq!(event, "before_tool_use");
                assert_eq!(script, "hooks/check.sh");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn empty_tree_has_no_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_capabilities(dir.path()).is_empty());
    }
}
