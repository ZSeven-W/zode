//! Render a conversation [`MessageStore`] to Markdown, for the `/export`
//! command (and headless `-p`/REPL export). Mirrors Claude Code's transcript
//! export: user/assistant turns become sections, tool calls + results are
//! summarized inline, thinking is included as a blockquote.

use std::path::{Path, PathBuf};

use agent::message::{ContentBlock, Message, MessageStore, ToolResultContent};

/// Default file name used when `/export` is given no file (or a directory).
const DEFAULT_EXPORT_NAME: &str = "zode-conversation.md";

/// Resolve the `/export [path]` argument to a target file. Empty → a default
/// file in `cwd`; a relative path → joined onto `cwd` (the active workspace,
/// which may differ from the process launch dir for resumed/`--cwd` sessions);
/// an absolute path → used as-is (writing outside the workspace is then the
/// user's EXPLICIT intent).
///
/// A relative path that climbs out of `cwd` via `..` is rejected: it silently
/// lands the transcript somewhere the user probably didn't mean (and the
/// workspace-confinement expectations elsewhere assume relative = inside the
/// workspace). Use an absolute path to export elsewhere.
///
/// When the argument points at a directory — either an existing dir, or a path
/// written with a trailing separator (e.g. `~/notes/`) — the default file name
/// is appended inside it. Without this, `std::fs::write` would fail with
/// "Is a directory (os error 21)" (the user's `导出聊天记录失败`).
pub fn resolve_export_path(cwd: &Path, arg: &str) -> PathBuf {
    try_resolve_export_path(cwd, arg).unwrap_or_else(|| cwd.join(DEFAULT_EXPORT_NAME))
}

/// Like [`resolve_export_path`] but surfacing the escaping-relative-path case
/// as `None` so callers can tell the user instead of silently defaulting.
pub fn try_resolve_export_path(cwd: &Path, arg: &str) -> Option<PathBuf> {
    let arg = arg.trim();
    if arg.is_empty() {
        return Some(cwd.join(DEFAULT_EXPORT_NAME));
    }
    // A trailing separator means "into this directory" even if it doesn't exist
    // yet; detect it before `PathBuf` normalizes the slash away.
    let looks_like_dir = arg.ends_with('/') || arg.ends_with(std::path::MAIN_SEPARATOR);
    let p = PathBuf::from(arg);
    let mut target = if p.is_absolute() {
        p
    } else {
        // Reject relative paths that escape the workspace (any `..` that
        // climbs above cwd after lexical normalization).
        let mut depth: i64 = 0;
        for comp in p.components() {
            match comp {
                std::path::Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                }
                std::path::Component::Normal(_) => depth += 1,
                _ => {}
            }
        }
        cwd.join(p)
    };
    if looks_like_dir || target.is_dir() {
        target.push(DEFAULT_EXPORT_NAME);
    }
    Some(target)
}

/// Truncate a one-line preview of a tool result / value for readability.
fn preview(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() > max {
        let cut: String = one_line.chars().take(max).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

fn render_blocks(blocks: &[ContentBlock], out: &mut String) {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                out.push_str(text);
                out.push_str("\n\n");
            }
            ContentBlock::Thinking { thinking, .. } => {
                out.push_str("> 💭 ");
                out.push_str(&preview(thinking, 280));
                out.push_str("\n\n");
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let args = serde_json::to_string(input).unwrap_or_default();
                out.push_str(&format!("> 🔧 **{name}**(`{}`)\n\n", preview(&args, 200)));
            }
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                let body = match content {
                    ToolResultContent::Text(t) => t.clone(),
                    ToolResultContent::Blocks(_) => "[blocks]".to_string(),
                };
                let mark = if *is_error { "⚠ " } else { "↳ " };
                out.push_str(&format!("> {mark}{}\n\n", preview(&body, 200)));
            }
            ContentBlock::Image { .. } => out.push_str("> 🖼 [image]\n\n"),
            ContentBlock::Document { .. } => out.push_str("> 📄 [document]\n\n"),
        }
    }
}

/// Render the whole conversation to Markdown. Progress/Tombstone messages are
/// internal and omitted.
pub fn store_to_markdown(store: &MessageStore) -> String {
    store_to_markdown_with_trace(store, None)
}

/// Render the conversation and optionally include the durable JSONL tool trace
/// file path. Markdown remains human-readable while full stdout/stderr stays in
/// the trace artifact for debugging and benchmark replay.
pub fn store_to_markdown_with_trace(store: &MessageStore, trace_path: Option<&Path>) -> String {
    let mut out = String::from("# Conversation\n\n");
    for msg in store.iter() {
        match msg {
            Message::User { content, .. } => {
                out.push_str("## 🧑 User\n\n");
                render_blocks(content, &mut out);
            }
            Message::Assistant { content, .. } => {
                out.push_str("## 🤖 Assistant\n\n");
                render_blocks(content, &mut out);
            }
            Message::System { text, .. } => {
                out.push_str("## ⚙ System\n\n");
                out.push_str(text);
                out.push_str("\n\n");
            }
            _ => {} // Progress / Tombstone: internal, skip.
        }
    }
    if let Some(path) = trace_path {
        out.push_str("## Trace\n\n");
        out.push_str("Full tool trace: ");
        out.push_str(&path.display().to_string());
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::message::{Header, Message};
    use serde_json::json;

    #[test]
    fn renders_user_assistant_and_tools() {
        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text {
                    text: "fix the bug".into(),
                }],
            })
            .unwrap();
        store
            .push(Message::Assistant {
                header: Header::new(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "let me look".into(),
                        signature: None,
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "FileRead".into(),
                        input: json!({ "path": "a.rs" }),
                    },
                    ContentBlock::Text {
                        text: "fixed it".into(),
                    },
                ],
            })
            .unwrap();
        let md = store_to_markdown(&store);
        assert!(md.contains("# Conversation"));
        assert!(md.contains("## 🧑 User"));
        assert!(md.contains("fix the bug"));
        assert!(md.contains("## 🤖 Assistant"));
        assert!(md.contains("🔧 **FileRead**"));
        assert!(md.contains("fixed it"));
        assert!(md.contains("💭"));
    }

    #[test]
    fn empty_store_is_just_the_header() {
        assert_eq!(
            store_to_markdown(&MessageStore::new()),
            "# Conversation\n\n"
        );
    }

    #[test]
    fn export_can_reference_full_tool_trace_file() {
        let md = store_to_markdown_with_trace(
            &MessageStore::new(),
            Some(Path::new("/work/proj/.zode/traces/session.jsonl")),
        );
        assert!(md.contains("Full tool trace"));
        assert!(md.contains("/work/proj/.zode/traces/session.jsonl"));
    }

    #[test]
    fn export_path_resolves_relative_to_cwd() {
        let cwd = Path::new("/work/proj");
        assert_eq!(
            resolve_export_path(cwd, ""),
            PathBuf::from("/work/proj/zode-conversation.md")
        );
        assert_eq!(
            resolve_export_path(cwd, "out.md"),
            PathBuf::from("/work/proj/out.md")
        );
        assert_eq!(
            resolve_export_path(cwd, "/tmp/x.md"),
            PathBuf::from("/tmp/x.md")
        );
    }

    #[test]
    fn export_path_rejects_relative_escape_but_allows_absolute() {
        let cwd = Path::new("/work/proj");
        // Climbing out of the workspace via .. is rejected…
        assert_eq!(try_resolve_export_path(cwd, "../secret.md"), None);
        assert_eq!(try_resolve_export_path(cwd, "a/../../secret.md"), None);
        // …but staying inside after normalization is fine…
        assert_eq!(
            try_resolve_export_path(cwd, "a/../out.md"),
            Some(PathBuf::from("/work/proj/a/../out.md"))
        );
        // …and an absolute path is explicit intent, used as-is.
        assert_eq!(
            try_resolve_export_path(cwd, "/tmp/x.md"),
            Some(PathBuf::from("/tmp/x.md"))
        );
    }

    #[test]
    fn export_path_appends_default_for_trailing_separator() {
        // A trailing slash means "into this directory" even before it exists.
        assert_eq!(
            resolve_export_path(Path::new("/work/proj"), "notes/"),
            PathBuf::from("/work/proj/notes/zode-conversation.md")
        );
    }

    #[test]
    fn export_path_appends_default_for_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Passing an existing directory must resolve to a file inside it, not
        // the directory itself (which would fail with EISDIR on write).
        let resolved = resolve_export_path(dir.path(), dir.path().to_str().unwrap());
        assert_eq!(resolved, dir.path().join("zode-conversation.md"));
    }
}
