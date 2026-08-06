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

/// One row of a computed diff, independent of styling. Line numbers are
/// 1-based: deletions carry the OLD file number, insertions and context the
/// NEW one. `Note` rows (warnings / truncation markers) carry no number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffRow {
    pub kind: DiffRowKind,
    pub line_no: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffRowKind {
    Add,
    Del,
    Ctx,
    Note,
}

impl DiffRow {
    fn note(text: impl Into<String>) -> Self {
        Self {
            kind: DiffRowKind::Note,
            line_no: None,
            text: text.into(),
        }
    }
}

/// Compute numbered diff rows for old → new. Oversized inputs collapse to a
/// single note row; very long diffs are truncated with a trailing note.
pub fn diff_rows(old: &str, new: &str) -> Vec<DiffRow> {
    let total = old.lines().count() + new.lines().count();
    if total > MAX_TOTAL_LINES {
        return vec![DiffRow::note(format!(
            "(diff too large: {total} lines — not shown)"
        ))];
    }

    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    for change in diff.iter_all_changes() {
        if out.len() >= MAX_DIFF_LINES {
            out.push(DiffRow::note("… (diff truncated)"));
            break;
        }
        let (kind, line_no) = match change.tag() {
            ChangeTag::Delete => (DiffRowKind::Del, change.old_index()),
            ChangeTag::Insert => (DiffRowKind::Add, change.new_index()),
            ChangeTag::Equal => (DiffRowKind::Ctx, change.new_index()),
        };
        out.push(DiffRow {
            kind,
            line_no: line_no.map(|n| n + 1),
            text: change.value().trim_end_matches('\n').to_string(),
        });
    }
    out
}

/// Style diff rows into transcript lines: a right-aligned line-number gutter,
/// the `-`/`+` sign, and the row text — red deletions, green insertions.
/// Rows are hard-truncated to `width` (no wrapping, so the gutter stays
/// aligned).
pub fn render_diff_rows(rows: &[DiffRow], theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let gutter = rows
        .iter()
        .filter_map(|r| r.line_no)
        .max()
        .map(|n| n.to_string().len())
        .unwrap_or(1)
        .max(2);
    rows.iter()
        .map(|row| {
            if row.kind == DiffRowKind::Note {
                return Line::from(Span::styled(
                    format!("  {}", row.text),
                    Style::default().fg(Color::Yellow),
                ));
            }
            let (sign, style) = match row.kind {
                DiffRowKind::Del => ("-", Style::default().fg(Color::Red)),
                DiffRowKind::Add => ("+", Style::default().fg(Color::Green)),
                _ => (" ", Style::default().fg(theme.fg_subtle)),
            };
            let no = row.line_no.map(|n| n.to_string()).unwrap_or_default();
            let body_width = (width as usize).saturating_sub(gutter + 3).max(8);
            let text = truncate_display(&row.text, body_width);
            Line::from(vec![
                Span::styled(
                    format!("{no:>gutter$} "),
                    Style::default().fg(theme.fg_subtle),
                ),
                Span::styled(format!("{sign} {text}"), style),
            ])
        })
        .collect()
}

/// Truncate to a display-cell budget, appending `…` when cut.
fn truncate_display(s: &str, max_cells: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut cells = 0usize;
    let mut out = String::new();
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cells + w > max_cells.saturating_sub(1) {
            // Reserve one cell for the ellipsis unless the string fits whole.
            if s.chars().count() > out.chars().count() {
                out.push('…');
            }
            return out;
        }
        cells += w;
        out.push(ch);
    }
    out
}

/// Build colored diff lines for old → new (permission-dialog style, no line
/// numbers). Oversized inputs collapse to a summary line; very long diffs are
/// truncated.
pub fn diff_lines(old: &str, new: &str, theme: &Theme) -> Vec<Line<'static>> {
    diff_rows(old, new)
        .into_iter()
        .map(|row| {
            let (sign, color) = match row.kind {
                DiffRowKind::Del => ("-", Color::Red),
                DiffRowKind::Add => ("+", Color::Green),
                DiffRowKind::Ctx => (" ", theme.fg_subtle),
                DiffRowKind::Note => {
                    return Line::from(Span::styled(
                        row.text,
                        Style::default().fg(theme.fg_subtle),
                    ));
                }
            };
            Line::from(Span::styled(
                format!("{sign}{}", row.text),
                Style::default().fg(color),
            ))
        })
        .collect()
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

/// Resolved preview source for a FileWrite/FileEdit tool input.
enum PreviewSource {
    /// Preview intentionally skipped, with the reason to display.
    Skipped(&'static str),
    /// Old and new bodies, plus an optional warning about a doomed edit.
    Diff {
        note: Option<&'static str>,
        old: String,
        new: String,
    },
}

/// Compute the old/new bodies a FileWrite/FileEdit input describes. Reads the
/// current file as "old" — callers must run BEFORE the tool applies (approval
/// prompt, ToolUse event), or the diff comes out empty. None if the input
/// isn't a recognized file edit.
fn preview_source(input: &serde_json::Value, base_cwd: &Path) -> Option<PreviewSource> {
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
        return Some(PreviewSource::Skipped(
            "(path outside workspace — preview skipped)",
        ));
    }

    // Bound the read: skip a preview for oversized files (stat first).
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_PREVIEW_BYTES {
            return Some(PreviewSource::Skipped(
                "(file too large for a diff preview)",
            ));
        }
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();

    let mut note: Option<&'static str> = None;
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
    Some(PreviewSource::Diff { note, old, new })
}

pub fn diff_from_tool_input(
    input: &serde_json::Value,
    base_cwd: &Path,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    match preview_source(input, base_cwd)? {
        PreviewSource::Skipped(reason) => Some(vec![Line::from(Span::styled(
            reason.to_string(),
            Style::default().fg(Color::Yellow),
        ))]),
        PreviewSource::Diff { note, old, new } => {
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
    }
}

/// Numbered diff rows for a FileWrite/FileEdit tool input — the transcript
/// variant of [`diff_from_tool_input`]. Same containment/size bounds; skip
/// reasons and doomed-edit warnings come back as [`DiffRowKind::Note`] rows.
pub fn diff_rows_from_tool_input(
    input: &serde_json::Value,
    base_cwd: &Path,
) -> Option<Vec<DiffRow>> {
    match preview_source(input, base_cwd)? {
        PreviewSource::Skipped(reason) => Some(vec![DiffRow::note(reason)]),
        PreviewSource::Diff { note, old, new } => {
            let mut rows = Vec::new();
            if let Some(n) = note {
                rows.push(DiffRow::note(n));
            }
            rows.extend(diff_rows(&old, &new));
            Some(rows)
        }
    }
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
    fn rows_number_del_with_old_and_add_with_new() {
        // old line 2 ("b") is replaced; the deletion carries the OLD number,
        // the insertion the NEW one, context lines the new numbering.
        let rows = diff_rows("a\nb\nc\n", "a\nX\nc\n");
        let del = rows
            .iter()
            .find(|r| r.kind == DiffRowKind::Del)
            .expect("del row");
        assert_eq!((del.line_no, del.text.as_str()), (Some(2), "b"));
        let add = rows
            .iter()
            .find(|r| r.kind == DiffRowKind::Add)
            .expect("add row");
        assert_eq!((add.line_no, add.text.as_str()), (Some(2), "X"));
        let last_ctx = rows
            .iter()
            .rev()
            .find(|r| r.kind == DiffRowKind::Ctx)
            .expect("ctx row");
        assert_eq!((last_ctx.line_no, last_ctx.text.as_str()), (Some(3), "c"));
    }

    #[test]
    fn render_rows_shows_gutter_and_sign() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let rows = diff_rows("a\nb\n", "a\nB\n");
        let flat: Vec<String> = render_diff_rows(&rows, &theme, 80)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(flat.iter().any(|l| l == " 2 - b"), "{flat:?}");
        assert!(flat.iter().any(|l| l == " 2 + B"), "{flat:?}");
        assert!(flat.iter().any(|l| l == " 1   a"), "{flat:?}");
    }

    #[test]
    fn render_rows_truncate_never_wraps() {
        let theme = ThemeStore::with_builtins().resolve(None);
        let long = "x".repeat(300);
        let rows = diff_rows("", &format!("{long}\n"));
        let lines = render_diff_rows(&rows, &theme, 40);
        for l in &lines {
            let w: usize = l
                .spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(w <= 40, "line width {w} exceeds budget");
        }
        assert!(joined(&lines).contains('…'));
    }

    #[test]
    fn rows_from_file_edit_input() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "hello world\n").unwrap();
        let input = serde_json::json!({
            "path": f.to_str().unwrap(),
            "old_string": "world",
            "new_string": "there"
        });
        let rows = diff_rows_from_tool_input(&input, dir.path()).unwrap();
        assert!(rows
            .iter()
            .any(|r| r.kind == DiffRowKind::Del && r.text == "hello world"));
        assert!(rows
            .iter()
            .any(|r| r.kind == DiffRowKind::Add && r.text == "hello there"));
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
