//! Three-level instruction file discovery + system prompt builder.
//!
//! Discovery order (low → high priority, appended in order so later text
//! wins the model's attention): global `~/.zode` → project root
//! (AGENTS.md > CLAUDE.md) → cwd (if different from the root). Mirrors the
//! design spec §8 hierarchy.

use std::path::{Path, PathBuf};

use crate::config::ConfigManager;

pub const MAX_INSTRUCTION_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Global,
    ProjectRoot,
    Cwd,
}

#[derive(Debug, Clone)]
pub struct InstructionFile {
    pub path: PathBuf,
    pub content: String,
    pub level: Level,
    pub truncated: bool,
}

/// Walk up from `start` to find a directory containing `.git`; fall back to
/// `start` itself.
fn project_root(start: &Path) -> PathBuf {
    let mut cur = start;
    loop {
        if cur.join(".git").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return start.to_path_buf(),
        }
    }
}

/// Read a file, truncating to MAX_INSTRUCTION_BYTES (on a char boundary).
fn read_capped(path: &Path) -> Option<(String, bool)> {
    let raw = std::fs::read_to_string(path).ok()?;
    if raw.len() > MAX_INSTRUCTION_BYTES {
        let mut s: String = raw.chars().take(MAX_INSTRUCTION_BYTES).collect();
        s.push_str("\n…(truncated)");
        Some((s, true))
    } else {
        Some((raw, false))
    }
}

/// First of AGENTS.md / CLAUDE.md present in `dir`.
fn pick_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn discover_instructions(cwd: &Path) -> Vec<InstructionFile> {
    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();

    // 1. Global (~/.zode/AGENTS.md or instructions.md).
    if let Ok(global_dir) = ConfigManager::config_dir() {
        for name in ["AGENTS.md", "instructions.md"] {
            let p = global_dir.join(name);
            if let Some((content, truncated)) = read_capped(&p) {
                seen.push(p.clone());
                out.push(InstructionFile {
                    path: p,
                    content,
                    level: Level::Global,
                    truncated,
                });
                break;
            }
        }
    }

    // 2. Project root.
    let root = project_root(cwd);
    if let Some(p) = pick_in_dir(&root) {
        if !seen.contains(&p) {
            if let Some((content, truncated)) = read_capped(&p) {
                seen.push(p.clone());
                out.push(InstructionFile {
                    path: p,
                    content,
                    level: Level::ProjectRoot,
                    truncated,
                });
            }
        }
    }

    // 3. cwd (if different from the project root).
    if cwd != root {
        if let Some(p) = pick_in_dir(cwd) {
            if !seen.contains(&p) {
                if let Some((content, truncated)) = read_capped(&p) {
                    out.push(InstructionFile {
                        path: p,
                        content,
                        level: Level::Cwd,
                        truncated,
                    });
                }
            }
        }
    }

    out
}

#[derive(Debug, Clone)]
pub struct EnvInfo {
    pub cwd: String,
    pub platform: String,
    pub date: String,
    pub git_branch: Option<String>,
}

const IDENTITY: &str = "\
You are Zode, an AI-native coding assistant running in a terminal. You help \
with software engineering tasks: reading and editing code, running shell \
commands, searching, and using git. Be concise and precise. Prefer the \
provided tools over guessing. Confirm before destructive actions. When you \
edit files, make minimal, correct changes that match the surrounding style.";

/// Assemble the full system prompt: identity → environment → project
/// instructions (with source attribution) → skills index.
pub fn build_system_prompt(
    instructions: &[InstructionFile],
    skills_index: &str,
    env: &EnvInfo,
) -> String {
    let mut s = String::new();
    s.push_str(IDENTITY);
    s.push_str("\n\n## Environment\n");
    s.push_str(&format!("- cwd: {}\n", env.cwd));
    s.push_str(&format!("- platform: {}\n", env.platform));
    s.push_str(&format!("- date: {}\n", env.date));
    if let Some(b) = &env.git_branch {
        s.push_str(&format!("- git branch: {b}\n"));
    }

    if !instructions.is_empty() {
        s.push_str("\n## Project Instructions\n");
        for f in instructions {
            s.push_str(&format!("\n### From {}\n{}\n", f.path.display(), f.content));
        }
    }

    if !skills_index.is_empty() {
        s.push_str("\n## Available Skills\n");
        s.push_str("Invoke a skill by name with the Skill tool to load its full instructions:\n");
        s.push_str(skills_index);
        s.push('\n');
    }
    s
}

/// Gather environment info for the prompt. `git_branch` is best-effort.
pub fn gather_env(cwd: &Path, date: &str) -> EnvInfo {
    let git_branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    EnvInfo {
        cwd: cwd.display().to_string(),
        platform: std::env::consts::OS.to_string(),
        date: date.to_string(),
        git_branch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_project_agents_md_over_claude_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "use rust").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "ignored").unwrap();
        let files = discover_instructions(dir.path());
        assert!(files.iter().any(|f| f.content.contains("use rust")));
        assert!(!files.iter().any(|f| f.content.contains("ignored")));
    }

    #[test]
    #[serial_test::serial]
    fn empty_when_no_files() {
        let dir = tempfile::tempdir().unwrap();
        // Point the global dir somewhere empty too.
        std::env::set_var("ZODE_CONFIG_DIR", dir.path().join("empty-cfg"));
        let files = discover_instructions(dir.path());
        std::env::remove_var("ZODE_CONFIG_DIR");
        assert!(files.is_empty());
    }

    #[test]
    fn oversized_file_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let big = "x".repeat(MAX_INSTRUCTION_BYTES + 100);
        std::fs::write(dir.path().join("AGENTS.md"), &big).unwrap();
        let files = discover_instructions(dir.path());
        let f = files
            .iter()
            .find(|f| f.level == Level::ProjectRoot)
            .unwrap();
        assert!(f.truncated);
        assert!(f.content.len() <= MAX_INSTRUCTION_BYTES + 32);
    }

    #[test]
    fn system_prompt_includes_identity_env_and_instructions() {
        let files = vec![InstructionFile {
            path: PathBuf::from("/p/AGENTS.md"),
            content: "Always write tests.".into(),
            level: Level::ProjectRoot,
            truncated: false,
        }];
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "macos".into(),
            date: "2026-06-13".into(),
            git_branch: Some("main".into()),
        };
        let prompt = build_system_prompt(&files, "- code-review: review code", &env);
        assert!(prompt.contains("Zode"));
        assert!(prompt.contains("/p"));
        assert!(prompt.contains("Always write tests."));
        assert!(prompt.contains("code-review"));
        assert!(prompt.contains("AGENTS.md"));
        assert!(prompt.contains("main"));
    }

    #[test]
    fn system_prompt_without_skills_omits_section() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-13".into(),
            git_branch: None,
        };
        let prompt = build_system_prompt(&[], "", &env);
        assert!(!prompt.contains("Available Skills"));
        assert!(!prompt.contains("Project Instructions"));
    }
}
