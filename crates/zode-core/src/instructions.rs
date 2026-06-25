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

/// True when the project uses OpenSpec: an `openspec/` directory exists at the
/// project root (walk up to `.git`, falling back to `cwd`). Root-aware, so it
/// still fires when zode is launched from a subdir. Detection is project-scoped
/// (the directory), NOT the `openspec` CLI on PATH — a globally installed CLI
/// must never inject the block into unrelated projects.
pub fn openspec_detected(cwd: &Path) -> bool {
    project_root(cwd).join("openspec").is_dir()
}

/// Read a file, truncating to MAX_INSTRUCTION_BYTES. The cap is in BYTES
/// (chars().take(N) would cap by char count and overshoot for multi-byte
/// text); we cut at the largest char boundary <= the byte cap.
fn read_capped(path: &Path) -> Option<(String, bool)> {
    let raw = std::fs::read_to_string(path).ok()?;
    if raw.len() > MAX_INSTRUCTION_BYTES {
        let mut end = MAX_INSTRUCTION_BYTES;
        while end > 0 && !raw.is_char_boundary(end) {
            end -= 1;
        }
        let mut s = raw[..end].to_string();
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
    /// The active model id (e.g. "deepseek-v4-pro"), so the agent can answer
    /// "what model are you?". Set by the engine after `gather_env`.
    pub model: String,
}

/// Appended when the project uses OpenSpec + the toggle is on. Generic — names
/// no specific change/proposal, only openspec conventions + CLI verbs, so it
/// adapts to any openspec project.
const OPENSPEC_AWARENESS: &str = "\n### OpenSpec workflow\n\
This project uses OpenSpec. For non-trivial changes, work spec-first: read \
`openspec/project.md` and the relevant `openspec/specs/` first; create or extend \
a change proposal under `openspec/changes/<name>/` (proposal.md, tasks.md, and \
spec deltas); run `openspec validate` before implementing; implement the tasks; \
then `openspec archive <name>` once done. Use `openspec list`/`openspec show` to \
inspect existing specs and changes.\n";

/// Appended after the skills index (when skills exist + the toggle is on) to
/// nudge the "use skills first" discipline. Generic — names no specific skill,
/// so it adapts to whatever is installed.
const SKILL_DISCIPLINE: &str = "\n### Using skills\n\
Before starting non-trivial work, scan the Available Skills above; if one plausibly \
applies, invoke it with the Skill tool FIRST and state which you're using. For \
multi-step features or changes, prefer a plan-first flow — use any available \
planning/brainstorming skill before writing code, and follow test-driven development \
if a testing skill applies.\n";

/// Appended when the `AskUserQuestion` tool is available, so single-choice
/// questions route to the interactive arrow-key picker instead of plain
/// `A)/B)` text the user must retype.
const ASK_USER_QUESTION: &str = "\n### Asking the user\n\
When you need the user to make a single choice — ambiguous requirements, a fork \
you can't resolve from the code, or a skill (such as brainstorming) that has you \
ask one multiple-choice question at a time — call the `AskUserQuestion` tool with \
the options instead of writing them as plain `A)/B)` text. The user gets an \
arrow-key picker and you get their selection back. Use plain prose only for \
open-ended questions that have no discrete options.\n";

const IDENTITY: &str = "\
You are Zode, an AI-native coding assistant developed by ZSeven-W, running in \
a terminal. You help \
with software engineering tasks: reading and editing code, running shell \
commands, searching, and using git. Be concise and precise. Prefer the \
provided tools over guessing. Confirm before destructive actions. When you \
edit files, make minimal, correct changes that match the surrounding style.\n\n\
When you write code, correctness comes first. Before finalizing, check the \
edge cases that commonly break solutions: empty input, single element, \
boundaries (first/last, off-by-one), negative numbers and zero, duplicates, \
and overflow/precision. Mentally trace your code against every example given \
in the request and fix any mismatch before answering. Match the requested \
function/return shape exactly. When the user asks only for code, reply with \
just the code in one fenced block and no commentary.\n\n\
Follow the user's instructions precisely. Use exactly the tools or skills they \
name (and no others); when they say not to use a tool, or not to explain, \
comply and output only what was asked. Honor output-format and length \
constraints to the letter — exact wording, casing, ordering, and structure. If \
the instructions change or override an earlier one, follow the most recent; if \
they tell you to ignore prior text, ignore it.";

/// Assemble the full system prompt: identity → environment → project
/// instructions (with source attribution) → skills index → openspec block.
pub fn build_system_prompt(
    instructions: &[InstructionFile],
    skills_index: &str,
    env: &EnvInfo,
    skill_discipline: bool,
    openspec: bool,
    ask_user_question: bool,
) -> String {
    let mut s = String::new();
    s.push_str(IDENTITY);
    s.push_str("\n\n## Environment\n");
    s.push_str(&format!("- cwd: {}\n", env.cwd));
    s.push_str(&format!("- platform: {}\n", env.platform));
    s.push_str(&format!("- date: {}\n", env.date));
    if !env.model.is_empty() {
        s.push_str(&format!("- model: {}\n", env.model));
    }
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
        if skill_discipline {
            s.push_str(SKILL_DISCIPLINE);
        }
    }
    if ask_user_question {
        s.push_str(ASK_USER_QUESTION);
    }
    if openspec {
        s.push_str(OPENSPEC_AWARENESS);
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
        // Filled in by the engine, which knows the resolved model.
        model: String::new(),
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
    fn truncation_respects_byte_cap_with_multibyte() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        // 3-byte chars: char-count cap would overshoot the byte cap ~3x.
        let big = "你".repeat(MAX_INSTRUCTION_BYTES);
        std::fs::write(dir.path().join("AGENTS.md"), &big).unwrap();
        let files = discover_instructions(dir.path());
        let f = files
            .iter()
            .find(|f| f.level == Level::ProjectRoot)
            .unwrap();
        assert!(f.truncated);
        // Body (minus the short marker) stays within the byte cap, and the
        // string is valid UTF-8 (constructing it didn't panic).
        assert!(f.content.len() <= MAX_INSTRUCTION_BYTES + 20);
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
            model: "deepseek-v4-pro".into(),
        };
        let prompt = build_system_prompt(
            &files,
            "- code-review: review code",
            &env,
            true,
            false,
            false,
        );
        assert!(prompt.contains("Zode"));
        assert!(prompt.contains("/p"));
        assert!(prompt.contains("Always write tests."));
        assert!(prompt.contains("code-review"));
        assert!(prompt.contains("AGENTS.md"));
        assert!(prompt.contains("main"));
        // The active model is surfaced so the agent can answer "what model?".
        assert!(prompt.contains("deepseek-v4-pro"));
    }

    #[test]
    fn system_prompt_without_skills_omits_section() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-13".into(),
            git_branch: None,
            model: String::new(),
        };
        let prompt = build_system_prompt(&[], "", &env, true, false, false);
        assert!(!prompt.contains("Available Skills"));
        // An empty model is omitted from the Environment block.
        assert!(!prompt.contains("- model:"));
        assert!(!prompt.contains("Project Instructions"));
    }

    #[test]
    fn system_prompt_includes_skill_discipline_when_skills_and_enabled() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-21".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "- foo: a skill", &env, true, false, false);
        assert!(p.contains("Using skills"));
        assert!(p.contains("invoke it with the Skill tool"));
        // install-agnostic: no hardcoded skill names
        assert!(!p.to_lowercase().contains("superpowers"));
        assert!(!p.to_lowercase().contains("openspec"));
    }

    #[test]
    fn system_prompt_omits_discipline_without_skills() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-21".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, true, false, false); // no skills index → no block
        assert!(!p.contains("Using skills"));
    }

    #[test]
    fn system_prompt_omits_discipline_when_disabled() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-21".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "- foo: a skill", &env, false, false, false);
        assert!(!p.contains("Using skills"));
    }

    #[test]
    fn openspec_detected_true_at_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir(dir.path().join("openspec")).unwrap();
        assert!(openspec_detected(dir.path()));
    }

    #[test]
    fn openspec_detected_root_aware_from_subdir() {
        // Codex NOTE: launching from a nested subdir must still detect repo-root openspec/.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir(dir.path().join("openspec")).unwrap();
        let nested = dir.path().join("crates").join("foo").join("src");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(openspec_detected(&nested));
    }

    #[test]
    fn openspec_detected_false_without_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(!openspec_detected(dir.path()));
    }

    #[test]
    fn system_prompt_includes_openspec_when_enabled() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-21".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, false, true, false);
        assert!(p.contains("OpenSpec workflow"));
        assert!(p.contains("openspec validate"));
        assert!(p.contains("openspec archive"));
        // generic: no specific change/proposal name baked in (placeholder only)
        assert!(p.contains("<name>"));
    }

    #[test]
    fn system_prompt_omits_openspec_when_disabled() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-21".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, false, false, false);
        assert!(!p.contains("OpenSpec workflow"));
    }

    #[test]
    fn system_prompt_includes_ask_user_question_when_available() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-25".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, false, false, true);
        assert!(p.contains("Asking the user"));
        assert!(p.contains("AskUserQuestion"));
        assert!(p.contains("arrow-key picker"));
    }

    #[test]
    fn system_prompt_omits_ask_user_question_when_unavailable() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-06-25".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, false, false, false);
        assert!(!p.contains("AskUserQuestion"));
        assert!(!p.contains("Asking the user"));
    }
}
