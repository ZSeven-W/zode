//! Unified diff rendering for file-edit previews (permission dialog + tool
//! result expansion). Uses the `similar` crate.

use std::path::{Component, Path, PathBuf};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

use crate::theme::Theme;

pub const MAX_DIFF_LINES: usize = 400;
const MAX_TOTAL_LINES: usize = 8000;
/// Don't read a file larger than this for a preview (avoid a huge alloc).
const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// Build colored diff lines for old → new. Oversized inputs collapse to a
/// summary line; very long diffs are truncated.
pub fn diff_lines(old: &str, new: &str, theme: &Theme) -> Vec<Line<'static>> {
    let total = old.lines().count() + new.lines().count();
    if total > MAX_TOTAL_LINES {
        return vec![Line::from(Span::styled(
            format!("(diff too large: {total} lines — not shown)"),
            Style::default().fg(theme.fg_subtle),
        ))];
    }

    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    for change in diff.iter_all_changes() {
        if out.len() >= MAX_DIFF_LINES {
            out.push(Line::from(Span::styled(
                "… (diff truncated)".to_string(),
                Style::default().fg(theme.fg_subtle),
            )));
            break;
        }
        let (sign, color) = match change.tag() {
            ChangeTag::Delete => ("-", Color::Red),
            ChangeTag::Insert => ("+", Color::Green),
            ChangeTag::Equal => (" ", theme.fg_subtle),
        };
        let text = change.value().trim_end_matches('\n').to_string();
        out.push(Line::from(Span::styled(
            format!("{sign}{text}"),
            Style::default().fg(color),
        )));
    }
    out
}

/// Best-effort diff from a FileWrite/FileEdit tool input. Reads the current
/// file as "old"; computes "new" from the input fields. None if the input
/// isn't a recognized file edit.
/// Lexically normalize a path (resolve `.`/`..` without touching the
/// filesystem, so it works for not-yet-created files).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True if `path` is inside `base` after lexical `..`/`.` resolution.
fn is_within(base: &Path, path: &Path) -> bool {
    normalize(path).starts_with(normalize(base))
}

pub fn diff_from_tool_input(
    input: &serde_json::Value,
    base_cwd: &Path,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let raw = input.get("path")?.as_str()?;
    // Resolve relative paths against the engine cwd (matches where the tool
    // writes); a raw relative path would resolve against the process cwd.
    let path = {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            p
        } else {
            base_cwd.join(p)
        }
    };

    // Containment: the fs tools confine writes to the workspace (cwd). Don't
    // read a file outside it for a preview, even though the model could name
    // an absolute or `..` path the tool would later reject.
    if !is_within(base_cwd, &path) {
        return Some(vec![Line::from(Span::styled(
            "(path outside workspace — preview skipped)".to_string(),
            Style::default().fg(Color::Yellow),
        ))]);
    }

    // Bound the read: skip a preview for oversized files (stat first).
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_PREVIEW_BYTES {
            return Some(vec![Line::from(Span::styled(
                "(file too large for a diff preview)".to_string(),
                Style::default().fg(theme.fg_subtle),
            ))]);
        }
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();

    let mut note: Option<&str> = None;
    let new = if let Some(content) = input.get("content").and_then(|v| v.as_str()) {
        content.to_string() // FileWrite: full new body
    } else if let (Some(o), Some(n)) = (
        input.get("old_string").and_then(|v| v.as_str()),
        input.get("new_string").and_then(|v| v.as_str()),
    ) {
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // Mirror the tool's rules so the preview warns when the edit will be
        // rejected: empty old_string, 0 matches, or >1 without replace_all.
        // (replace("", _) would otherwise splice new_string between chars.)
        if o.is_empty() {
            note = Some("(empty old_string — the edit will fail)");
            old.clone()
        } else {
            let count = old.matches(o).count();
            if count == 0 {
                note = Some("(old_string not found — the edit will fail)");
            } else if count > 1 && !replace_all {
                note = Some("(old_string matches multiple times — needs replace_all)");
            }
            if replace_all {
                old.replace(o, n)
            } else {
                old.replacen(o, n, 1)
            }
        }
    } else {
        return None;
    };

    let mut lines = Vec::new();
    if let Some(n) = note {
        lines.push(Line::from(Span::styled(
            n.to_string(),
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.extend(diff_lines(&old, &new, theme));
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStore;

    fn joined(lines: &[Line]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn marks_add_and_remove() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let lines = diff_lines("a\nb\nc\n", "a\nX\nc\n", &theme);
        let j = joined(&lines);
        assert!(j.contains("-b"), "{j}");
        assert!(j.contains("+X"), "{j}");
        assert!(j.contains(" a"), "{j}"); // context
    }

    #[test]
    fn oversized_is_summarized() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let big = "x\n".repeat(MAX_TOTAL_LINES + 1);
        let lines = diff_lines("", &big, &theme);
        assert!(joined(&lines).contains("too large"));
    }

    #[test]
    fn file_edit_input_produces_diff() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "hello world\n").unwrap();
        let input = serde_json::json!({
            "path": f.to_str().unwrap(),
            "old_string": "world",
            "new_string": "there"
        });
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        let j = joined(&lines);
        assert!(j.contains("-hello world"));
        assert!(j.contains("+hello there"));
    }

    #[test]
    fn relative_path_resolves_against_base_cwd() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rel.txt"), "old\n").unwrap();
        // A RELATIVE path must resolve under base_cwd, not the process cwd.
        let input = serde_json::json!({"path": "rel.txt", "content": "new\n"});
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        let j = joined(&lines);
        assert!(j.contains("-old"), "{j}");
        assert!(j.contains("+new"), "{j}");
    }

    #[test]
    fn missing_old_string_is_noted() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let input = serde_json::json!({
            "path": "a.txt", "old_string": "NOPE", "new_string": "x"
        });
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        assert!(joined(&lines).contains("not found"));
    }

    #[test]
    fn path_outside_workspace_is_skipped() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        // A `..`-escaping relative path must not be read for preview.
        let input = serde_json::json!({"path": "../escape.txt", "content": "x"});
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        assert!(joined(&lines).contains("outside workspace"));
    }

    #[test]
    fn empty_old_string_is_noted_not_spliced() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let input = serde_json::json!({
            "path": "a.txt", "old_string": "", "new_string": "X", "replace_all": true
        });
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        let j = joined(&lines);
        assert!(j.contains("empty old_string"));
        // Must NOT splice X between every character.
        assert!(!j.contains("XhXeXlX"));
    }

    #[test]
    fn new_file_under_cwd_is_allowed() {
        // A not-yet-created file under cwd must still preview (empty -> new),
        // not be skipped as outside-workspace.
        let theme = ThemeStore::with_builtins().resolve(None);
        let dir = tempfile::tempdir().unwrap();
        let input = serde_json::json!({"path": "newfile.txt", "content": "hello\n"});
        let lines = diff_from_tool_input(&input, dir.path(), &theme).unwrap();
        let j = joined(&lines);
        assert!(!j.contains("outside workspace"), "{j}");
        assert!(j.contains("+hello"), "{j}");
    }

    #[test]
    fn within_check_lexical() {
        let base = std::path::Path::new("/work/proj");
        assert!(is_within(base, std::path::Path::new("/work/proj/src/a.rs")));
        assert!(is_within(base, std::path::Path::new("/work/proj/./a.rs")));
        assert!(!is_within(
            base,
            std::path::Path::new("/work/proj/../other/a.rs")
        ));
        assert!(!is_within(base, std::path::Path::new("/etc/passwd")));
    }

    #[test]
    fn non_edit_input_is_none() {
        let theme = ThemeStore::with_builtins().resolve(None);
        assert!(diff_from_tool_input(
            &serde_json::json!({"command": "ls"}),
            std::path::Path::new("/"),
            &theme
        )
        .is_none());
    }
}
