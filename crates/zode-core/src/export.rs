//! Render a conversation [`MessageStore`] to Markdown, for the `/export`
//! command (and headless `-p`/REPL export). Mirrors Claude Code's transcript
//! export: user/assistant turns become sections, tool calls + results are
//! summarized inline, thinking is included as a blockquote.

use agent::message::{ContentBlock, Message, MessageStore, ToolResultContent};

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
        assert_eq!(store_to_markdown(&MessageStore::new()), "# Conversation\n\n");
    }
}
