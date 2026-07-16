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
    Intermediate,
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

    // 2. Project root down to cwd: every directory on the path contributes,
    //    root first (later files win the model's attention). If cwd is not
    //    under the root (no .git anywhere), the chain is just [cwd].
    let root = project_root(cwd);
    let mut chain: Vec<&Path> = Vec::new();
    let mut cur = Some(cwd);
    while let Some(c) = cur {
        chain.push(c);
        if c == root {
            break;
        }
        cur = c.parent();
    }
    if *chain.last().unwrap() != root {
        // cwd is outside the detected root (project_root fell back): keep cwd only.
        chain = vec![cwd];
    }
    chain.reverse();
    let last = chain.len().saturating_sub(1);
    for (i, dir) in chain.iter().enumerate() {
        if let Some(p) = pick_in_dir(dir) {
            if seen.contains(&p) {
                continue;
            }
            if let Some((content, truncated)) = read_capped(&p) {
                seen.push(p.clone());
                let level = if i == 0 {
                    Level::ProjectRoot
                } else if i == last {
                    Level::Cwd
                } else {
                    Level::Intermediate
                };
                out.push(InstructionFile {
                    path: p,
                    content,
                    level,
                    truncated,
                });
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

/// Feature flags for optional system-prompt sections. A struct (not bools)
/// so call sites stay readable as sections accumulate.
#[derive(Debug, Clone, Default)]
pub struct PromptFlags {
    pub skill_discipline: bool,
    pub openspec: bool,
    pub ask_user_question: bool,
    /// TodoWrite is registered — include the task-tracking discipline section.
    pub todo: bool,
    /// The `run_check` tool is registered — include the verification
    /// sentence that advertises it. Plan mode strips `run_check` from the
    /// tool registry (it's `SafetyClass::Mutating`, filtered out by
    /// `filter_read_only`), so the prompt must not tell the model to call a
    /// tool it doesn't have.
    pub verify_tool: bool,
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

/// Appended when the `AskUserQuestion` tool is available, so multiple-choice
/// questions route to the interactive arrow-key picker instead of plain
/// `A)/B)` text the user must retype.
const ASK_USER_QUESTION: &str = "\n### Asking the user\n\
When you need the user to decide — ambiguous requirements, a fork you can't \
resolve from the code, or a skill (such as brainstorming) that has you ask \
multiple-choice questions — call the `AskUserQuestion` tool instead of writing \
the choices as plain `A)/B)` text. BATCH your decisions: pass `questions` (4-8 \
questions, each with 2-10 `options` and an optional short `header`) in ONE call \
— never call the tool repeatedly with a single question, as that clutters the \
transcript. The user gets an arrow-key picker that always also offers a \
free-text 'Other'. You get back `{ answers: [{ question, header, answer }] }`, \
where each `answer` is the chosen option text or the user's custom text. Use \
plain prose only for open-ended questions that have no discrete options.\n";

/// Parallel tool calls + anti-promise completion discipline. Unconditional —
/// does not name `run_check` (which plan mode removes from the tool
/// registry; see `VERIFY_TOOL` below for the gated sentence that does).
const WORKING_STYLE: &str = "\n### Working style\n\
When a response needs several INDEPENDENT tool calls (reading multiple files, \
separate searches), issue them together in one response — they run in parallel \
and you get every result at once. Only sequence calls whose input depends on an \
earlier result. Never end your turn on an unexecuted plan or promise: if your \
last paragraph says you WILL do something next, do it now with tool calls \
instead of stopping.\n";

/// Verification sentence naming the `run_check` tool. Split out of
/// `WORKING_STYLE` because plan mode strips `run_check` from the tool
/// registry (`SafetyClass::Mutating`, dropped by `filter_read_only`) —
/// advertising a tool the model doesn't have invites a failed call. Gated
/// on `PromptFlags::verify_tool`.
const VERIFY_TOOL: &str = "Before claiming work is complete or fixed, verify \
it — run the relevant test/build/command (the `run_check` tool runs a \
command and evaluates explicit assertions, recording the evidence) and \
report the actual result. If verification fails, say so plainly; never \
claim success without fresh evidence.\n";

/// Final-message and reporting norms.
const REPORTING: &str = "\n### Reporting\n\
Your final message of a turn is what the user reads — put the complete answer, \
findings, and conclusions there; text between tool calls should be brief status \
only. Lead with the outcome, then supporting detail. Reference code as \
`path:line` so locations are clickable. Report results faithfully: failing \
tests, skipped steps, and partial work are stated as such, never papered over.\n";

/// Included only when the TodoWrite tool is registered.
const TODO_DISCIPLINE: &str = "\n### Task tracking\n\
For any task with 3+ distinct steps, maintain a plan with the TodoWrite tool: \
create the list before starting, keep exactly one item in_progress while you \
work on it, and mark items completed as you finish — re-emit the FULL list on \
every call (it replaces the previous one). Keep items short and user-facing. \
Skip the todo list for single-step or purely conversational requests.\n";

/// Compaction awareness: the model otherwise treats a `[Context summary]`
/// message as odd user input, or wraps up early fearing context loss.
const LONG_SESSIONS: &str = "\n### Long sessions\n\
When the conversation grows long it is automatically compacted: older turns \
are replaced by a `[Context summary]` message and recently touched files are \
re-attached. Treat a context summary as trusted prior conversation and keep \
working — do not wrap up early because the session is long.\n";

/// Steer the model toward bounded reads. Bash output caps are the hard guard;
/// this guidance reduces how often the cap needs to trigger.
const TOKEN_HYGIENE: &str = "\n### File reading\n\
Use `FileRead` with offset/limit for large files and `Grep` to search. Do not \
cat or dump whole files into the conversation. When using Bash to inspect files, \
avoid dumping whole files; narrow output with `grep`/`head`/`tail`. Large Bash \
output is truncated.\n";

/// Appended when at least one language server is enabled. The `lsp_*` tools are
/// registered either way, but nothing in the prompt said what they were for —
/// so the model reached for grep and `cargo check` and left them unused.
const LSP_TOOLS: &str = "\n### Language servers\n\
Language servers are running for the languages listed below. For code questions \
in those languages, prefer the `lsp_*` tools over text search: `lsp_definition` \
and `lsp_references` resolve a symbol exactly (grep matches text, not bindings), \
`lsp_hover` gives the real type signature, `lsp_symbols` outlines a file, and \
`lsp_diagnostics` reports compiler/linter errors for one file without building \
the whole project. Positions are 0-based. `lsp_rename` and `lsp_format` return a \
PREVIEW of the server's edits — apply them yourself with FileEdit. A server \
starts on first use and may take a few seconds to index.\n";

/// The language-server section, or empty when no server is enabled.
pub fn lsp_prompt_note(langs: &[String]) -> String {
    if langs.is_empty() {
        return String::new();
    }
    format!("{LSP_TOOLS}Enabled: {}.\n", langs.join(", "))
}

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
    flags: &PromptFlags,
) -> String {
    let mut s = String::new();
    s.push_str(IDENTITY);
    s.push_str("\n\n## Environment\n");
    s.push_str(&format!("- cwd: {}\n", env.cwd));
    s.push_str(&format!("- platform: {}\n", env.platform));
    s.push_str(&format!("- date (session start): {}\n", env.date));
    if !env.model.is_empty() {
        s.push_str(&format!("- model: {}\n", env.model));
    }
    if let Some(b) = &env.git_branch {
        s.push_str(&format!("- git branch (at session start): {b}\n"));
    }
    s.push_str(TOKEN_HYGIENE);
    s.push_str(WORKING_STYLE);
    if flags.verify_tool {
        s.push_str(VERIFY_TOOL);
    }
    s.push_str(REPORTING);
    if flags.todo {
        s.push_str(TODO_DISCIPLINE);
    }
    s.push_str(LONG_SESSIONS);

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
        if flags.skill_discipline {
            s.push_str(SKILL_DISCIPLINE);
        }
    }
    if flags.ask_user_question {
        s.push_str(ASK_USER_QUESTION);
    }
    if flags.openspec {
        s.push_str(OPENSPEC_AWARENESS);
    }
    s
}

/// The current git branch of `cwd`, or `None` outside a repo. Shells out
/// to `git` (blocking) — callers on an async path should run it via
/// `spawn_blocking` (see [`gather_env_with_branch`]).
pub fn detect_git_branch(cwd: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Gather environment info for the prompt, detecting the git branch inline
/// (blocking). Kept for the synchronous hot-swap path + tests; the async
/// assemble path precomputes the branch off-thread via
/// [`gather_env_with_branch`].
pub fn gather_env(cwd: &Path, date: &str) -> EnvInfo {
    gather_env_with_branch(cwd, date, detect_git_branch(cwd))
}

/// Like [`gather_env`] but with the (possibly off-thread-computed) git
/// branch supplied, so an async caller doesn't block a runtime worker on
/// the `git` subprocess.
pub fn gather_env_with_branch(cwd: &Path, date: &str, git_branch: Option<String>) -> EnvInfo {
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
            &PromptFlags {
                skill_discipline: true,
                ..Default::default()
            },
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
    fn lsp_note_lists_enabled_languages_and_is_absent_when_none() {
        assert_eq!(lsp_prompt_note(&[]), "");
        let note = lsp_prompt_note(&["go".to_string(), "rust".to_string()]);
        assert!(note.contains("lsp_definition"));
        assert!(note.contains("Enabled: go, rust."));
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
        let prompt = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                skill_discipline: true,
                ..Default::default()
            },
        );
        assert!(!prompt.contains("Available Skills"));
        // An empty model is omitted from the Environment block.
        assert!(!prompt.contains("- model:"));
        assert!(!prompt.contains("Project Instructions"));
    }

    #[test]
    fn system_prompt_steers_away_from_cat() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-07-05".into(),
            git_branch: None,
            model: String::new(),
        };
        let prompt = build_system_prompt(&[], "", &env, &PromptFlags::default());
        assert!(prompt.contains("FileRead"));
        assert!(
            prompt.to_lowercase().contains("do not cat")
                || prompt.contains("avoid dumping whole files")
        );
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
        let p = build_system_prompt(
            &[],
            "- foo: a skill",
            &env,
            &PromptFlags {
                skill_discipline: true,
                ..Default::default()
            },
        );
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
        // no skills index → no block
        let p = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                skill_discipline: true,
                ..Default::default()
            },
        );
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
        let p = build_system_prompt(&[], "- foo: a skill", &env, &PromptFlags::default());
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
        let p = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                openspec: true,
                ..Default::default()
            },
        );
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
        let p = build_system_prompt(&[], "", &env, &PromptFlags::default());
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
        let p = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                ask_user_question: true,
                ..Default::default()
            },
        );
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
        let p = build_system_prompt(&[], "", &env, &PromptFlags::default());
        assert!(!p.contains("AskUserQuestion"));
        assert!(!p.contains("Asking the user"));
    }

    #[test]
    fn system_prompt_includes_working_style_and_reporting() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-07-16".into(),
            git_branch: None,
            model: String::new(),
        };
        let p = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                verify_tool: true,
                ..Default::default()
            },
        );
        // Parallel tool-call steering.
        assert!(p.contains("issue them together in one response"));
        // Anti-promise completion discipline (unconditional).
        assert!(p.contains("Never end your turn on an unexecuted plan"));
        // run_check advertising — gated on verify_tool, on here.
        assert!(p.contains("run_check"));
        // Communication norms.
        assert!(p.contains("final message of a turn is what the user reads"));
        assert!(p.contains("`path:line`"));
        // Compaction awareness.
        assert!(p.contains("[Context summary]"));

        // Plan mode (verify_tool: false) must not advertise a tool it
        // removed from the registry.
        let without_verify = build_system_prompt(&[], "", &env, &PromptFlags::default());
        assert!(!without_verify.contains("run_check"));
        // The unconditional working-style text is still present.
        assert!(without_verify.contains("issue them together in one response"));
        assert!(without_verify.contains("Never end your turn on an unexecuted plan"));
    }

    #[test]
    fn system_prompt_gates_todo_discipline_on_flag() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "linux".into(),
            date: "2026-07-16".into(),
            git_branch: None,
            model: String::new(),
        };
        let with = build_system_prompt(
            &[],
            "",
            &env,
            &PromptFlags {
                todo: true,
                ..Default::default()
            },
        );
        let without = build_system_prompt(&[], "", &env, &PromptFlags::default());
        assert!(with.contains("TodoWrite"));
        assert!(with.contains("one item in_progress"));
        assert!(!without.contains("TodoWrite"));
    }

    #[test]
    fn env_block_labels_branch_and_date_as_session_start() {
        let env = EnvInfo {
            cwd: "/p".into(),
            platform: "macos".into(),
            date: "2026-07-16".into(),
            git_branch: Some("main".into()),
            model: String::new(),
        };
        let p = build_system_prompt(&[], "", &env, &PromptFlags::default());
        assert!(p.contains("- date (session start): 2026-07-16"));
        assert!(p.contains("- git branch (at session start): main"));
    }

    #[test]
    fn discovers_intermediate_dir_instructions_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "root rules").unwrap();
        let mid = dir.path().join("crates");
        let leaf = mid.join("foo");
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(mid.join("AGENTS.md"), "mid rules").unwrap();
        std::fs::write(leaf.join("CLAUDE.md"), "leaf rules").unwrap();
        let files = discover_instructions(&leaf);
        let bodies: Vec<&str> = files.iter().map(|f| f.content.as_str()).collect();
        let root_i = bodies
            .iter()
            .position(|c| c.contains("root rules"))
            .unwrap();
        let mid_i = bodies.iter().position(|c| c.contains("mid rules")).unwrap();
        let leaf_i = bodies
            .iter()
            .position(|c| c.contains("leaf rules"))
            .unwrap();
        assert!(
            root_i < mid_i && mid_i < leaf_i,
            "root → intermediate → cwd order"
        );
        assert_eq!(files[mid_i].level, Level::Intermediate);
    }
}
