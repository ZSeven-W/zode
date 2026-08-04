//! `MemoryImport` — migrate memories from external files into zode's
//! built-in (noema) memory.
//!
//! Zode's memory is deliberately AUTOMATIC: relevant memories are recalled
//! into each turn and durable facts are auto-remembered — there is no
//! model-facing save/recall surface. The one thing the automatic pipeline
//! cannot do is a one-shot migration from another tool's memory files
//! (Claude Code memory directories, MEMORY.md indexes, plain notes). Told to
//! "migrate memories" without this tool, a model was observed burning 49
//! ToolSearch calls hunting for a memory tool that did not exist.
//!
//! The tool description doubles as documentation of the built-in memory
//! model, so a search for "memory" teaches the model how memory works here
//! instead of leaving it guessing.

use std::path::{Path, PathBuf};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::noema::{ZodeMemoryScope, ZodeNoema};

/// Hard caps so one import cannot flood the store or the transcript.
const MAX_FILES: usize = 50;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 200;
const MAX_DIR_DEPTH: usize = 3;
/// A bulletless file becomes one entry, capped to this many chars.
const MAX_WHOLE_FILE_ENTRY_CHARS: usize = 2000;
/// At most this many error strings ride back in the report.
const MAX_REPORTED_ERRORS: usize = 5;

#[derive(Debug)]
pub struct MemoryImportTool {
    noema: ZodeNoema,
}

impl MemoryImportTool {
    pub fn new(noema: ZodeNoema) -> Self {
        Self { noema }
    }
}

#[derive(Debug, Deserialize)]
struct MemoryImportInput {
    /// File or directory to import from (absolute, or relative to cwd).
    path: String,
    /// "user" (default — applies across projects) or "project".
    #[serde(default)]
    scope: Option<String>,
}

#[async_trait]
impl Tool for MemoryImportTool {
    fn name(&self) -> &str {
        "MemoryImport"
    }
    fn description(&self) -> &str {
        "Migrate memories from external files into zode's built-in memory. Zode's memory is \
         AUTOMATIC — relevant memories are recalled into every turn and durable facts are \
         remembered on their own, so there is no save/recall tool and none is needed. Use this \
         tool ONLY to import existing memory files from other tools (a Claude Code memory \
         directory, a MEMORY.md, plain notes): point `path` at the file or directory; markdown \
         bullets become one memory each, bulletless files import whole. `scope` is 'user' \
         (default, cross-project) or 'project'."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File or directory of memory notes"},
                "scope": {"type": "string", "enum": ["user", "project"], "default": "user"}
            },
            "required": ["path"]
        })
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(
        &self,
        ctx: &ToolUseContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        let parsed: MemoryImportInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("MemoryImport invalid input: {e}")))?;
        let scope = match parsed.scope.as_deref().map(str::trim) {
            None | Some("") | Some("user") => ZodeMemoryScope::User,
            Some("project") => ZodeMemoryScope::Project,
            Some(other) => {
                return Err(AgentError::other(format!(
                    "MemoryImport: unknown scope '{other}' (use 'user' or 'project')"
                )))
            }
        };
        let root = {
            let p = PathBuf::from(parsed.path.trim());
            if p.is_absolute() {
                p
            } else {
                ctx.cwd.join(p)
            }
        };
        let noema = self.noema.clone();
        let cwd = ctx.cwd.clone();
        // Noema writes files per entry — keep it off the async workers.
        let report =
            tokio::task::spawn_blocking(move || import_from_path(&noema, &root, scope, &cwd))
                .await
                .map_err(|e| AgentError::other(format!("MemoryImport task failed: {e}")))??;
        Ok(report)
    }
}

fn import_from_path(
    noema: &ZodeNoema,
    root: &Path,
    scope: ZodeMemoryScope,
    cwd: &Path,
) -> Result<serde_json::Value, AgentError> {
    if !root.exists() {
        return Err(AgentError::other(format!(
            "MemoryImport: {} does not exist",
            root.display()
        )));
    }
    let files = collect_note_files(root);
    if files.is_empty() {
        return Err(AgentError::other(format!(
            "MemoryImport: no .md/.txt notes found under {}",
            root.display()
        )));
    }
    let truncated_files = files.len() > MAX_FILES;
    let mut entries: Vec<String> = Vec::new();
    let mut skipped_files = 0usize;
    for file in files.iter().take(MAX_FILES) {
        let Ok(meta) = std::fs::metadata(file) else {
            skipped_files += 1;
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            skipped_files += 1;
            continue;
        }
        let Ok(content) = std::fs::read_to_string(file) else {
            skipped_files += 1;
            continue;
        };
        entries.extend(split_memory_entries(&content));
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }
    let truncated_entries = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    if entries.is_empty() {
        return Err(AgentError::other(
            "MemoryImport: the notes contained no importable entries".to_string(),
        ));
    }

    let mut imported = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for entry in &entries {
        match noema.remember(entry, scope, Some(cwd)) {
            Ok(_) => imported += 1,
            Err(e) => {
                failed += 1;
                if errors.len() < MAX_REPORTED_ERRORS && !errors.contains(&e) {
                    errors.push(e);
                }
            }
        }
    }
    let mut report = json!({
        "files_read": files.len().min(MAX_FILES),
        "entries_found": entries.len(),
        "imported": imported,
        "failed": failed,
        "note": "imported memories are recalled automatically in future turns — no further action needed",
    });
    if skipped_files > 0 {
        report["files_skipped"] = json!(skipped_files);
    }
    if truncated_files || truncated_entries {
        report["truncated"] = json!(format!(
            "capped at {MAX_FILES} files / {MAX_ENTRIES} entries per call — run again on the remainder"
        ));
    }
    if !errors.is_empty() {
        report["errors"] = json!(errors);
    }
    Ok(report)
}

/// All importable note files under `root` (or `root` itself when it is a
/// file), bounded by depth. Sorted for a deterministic import order.
fn collect_note_files(root: &Path) -> Vec<PathBuf> {
    fn is_note(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md" | "markdown" | "txt")
        )
    }
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > MAX_DIR_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1, out);
            } else if is_note(&path) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    if root.is_file() {
        // An explicitly named file imports regardless of extension.
        out.push(root.to_path_buf());
    } else {
        walk(root, 0, &mut out);
    }
    out.sort();
    out
}

/// Split note content into memory entries.
///
/// - YAML frontmatter (`--- … ---` at the top) is dropped.
/// - Markdown headers are dropped.
/// - Each top-level bullet (`- ` / `* `) starts an entry; indented
///   continuation lines fold into it.
/// - A file with no bullets becomes ONE entry (trimmed, capped).
fn split_memory_entries(content: &str) -> Vec<String> {
    let body = strip_frontmatter(content);
    let mut entries: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut had_bullets = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let is_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        let top_level = line.len() == trimmed.len();
        if is_bullet && top_level {
            had_bullets = true;
            if let Some(entry) = current.take() {
                push_entry(&mut entries, entry);
            }
            current = Some(trimmed[2..].trim().to_string());
        } else if let Some(entry) = current.as_mut() {
            if trimmed.is_empty() {
                push_entry(&mut entries, current.take().unwrap_or_default());
            } else if !top_level {
                // Indented continuation of the current bullet.
                entry.push(' ');
                entry.push_str(trimmed);
            } else {
                // Back to prose — close the bullet.
                push_entry(&mut entries, current.take().unwrap_or_default());
            }
        }
    }
    if let Some(entry) = current.take() {
        push_entry(&mut entries, entry);
    }
    if !had_bullets {
        let whole: String = body
            .trim()
            .chars()
            .take(MAX_WHOLE_FILE_ENTRY_CHARS)
            .collect();
        if !whole.is_empty() {
            return vec![whole];
        }
    }
    entries
}

fn push_entry(entries: &mut Vec<String>, entry: String) {
    let entry = entry.trim().to_string();
    if !entry.is_empty() {
        entries.push(entry);
    }
}

/// Drop a leading `---\n…\n---` YAML frontmatter block.
fn strip_frontmatter(content: &str) -> &str {
    let Some(rest) = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
    else {
        return content;
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            // Skip the remainder of the closing fence line (`\n` or `\r\n`).
            match after.find('\n') {
                Some(nl) => &after[nl + 1..],
                None => "",
            }
        }
        None => content,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn bullets_become_entries_and_continuations_fold_in() {
        let content = "\
# Header dropped
- first fact
- second fact
  with a continuation
- third

prose outside bullets is ignored when bullets exist
- fourth";
        let entries = split_memory_entries(content);
        assert_eq!(
            entries,
            vec![
                "first fact",
                "second fact with a continuation",
                "third",
                "fourth",
            ]
        );
    }

    #[test]
    fn frontmatter_is_stripped_and_bulletless_files_import_whole() {
        let content = "---\nname: x\ntype: user\n---\nThe user prefers tabs over spaces.\n";
        let entries = split_memory_entries(content);
        assert_eq!(entries, vec!["The user prefers tabs over spaces."]);
    }

    #[test]
    fn empty_and_whitespace_content_yields_nothing() {
        assert!(split_memory_entries("").is_empty());
        assert!(split_memory_entries("\n\n  \n").is_empty());
        // A bulletless file imports whole — even if it is only a header —
        // rather than silently losing content.
        assert_eq!(split_memory_entries("# t\n"), vec!["# t"]);
    }

    #[test]
    fn collect_walks_dirs_and_accepts_explicit_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "- a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "- b").unwrap();
        std::fs::write(dir.path().join("skip.png"), "x").unwrap();
        let files = collect_note_files(dir.path());
        assert_eq!(files.len(), 2);
        // Explicit file: extension not required.
        let odd = dir.path().join("notes.custom");
        std::fs::write(&odd, "- c").unwrap();
        assert_eq!(collect_note_files(&odd), vec![odd]);
    }

    #[tokio::test]
    async fn disabled_memory_reports_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.md"), "- fact one").unwrap();
        let tool = MemoryImportTool::new(ZodeNoema::disabled());
        let ctx = ToolUseContext {
            cwd: dir.path().to_path_buf(),
            abort: agent::abort::AbortController::new(),
            file_cache: Arc::new(agent::file_cache::FileStateCache::new(
                std::num::NonZeroUsize::new(8).unwrap(),
                1024 * 1024,
            )),
            permissions: Arc::new(agent::permission::PermissionManager::new()),
            hooks: Arc::new(agent::hook::HookRunner::new()),
            task_depth: 0,
        };
        let out = tool
            .call(&ctx, json!({"path": dir.path().display().to_string()}))
            .await
            .unwrap();
        // Entries were found but every save failed with the disabled notice.
        assert_eq!(out["imported"], 0);
        assert_eq!(out["failed"], 1);
        assert!(out["errors"][0].as_str().unwrap().contains("disabled"));
    }
}
