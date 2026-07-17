//! Derivation for the conversation anchor rail (one anchor per turn).
//!
//! See `docs/proposals/conversation-anchor-rail.md` for the design. This
//! module only derives data; geometry, painting, hit-testing and the a11y
//! tree all live in `zode-app-ui`.

use zode_node_protocol::ToolCall;

use super::{TranscriptItem, TranscriptState};

const USER_EXCERPT_CHARS: usize = 80;
const REPLY_EXCERPT_CHARS: usize = 150;

/// One derived turn anchor: where the turn starts, its cached y-offset in
/// the transcript's cumulative layout, short excerpts of the opening user
/// message and the first assistant reply, and any files touched by
/// Write/Edit/NotebookEdit tool calls in the turn's range.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationAnchor {
    /// Index of the turn's first (and by construction, only) `UserText` item.
    pub item_index: usize,
    /// Cumulative top offset of `item_index`, taken from the same prefix
    /// sums `TranscriptState::layout_offsets` already maintains.
    pub y_offset: f32,
    pub user_excerpt: String,
    pub reply_excerpt: String,
    /// Deduplicated basenames, in first-seen order. Empty when no
    /// file-editing tool call was recoverable in this turn's range.
    pub files: Vec<String>,
}

/// Memoized anchor list, keyed identically to `TranscriptLayoutCache` (see
/// its doc comment) so it invalidates in lockstep with the layout cache
/// instead of tracking a second revision signal, per the design doc's
/// "reuse the same invalidation, don't invent a second revision counter".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnchorCache {
    pub(super) revision: u64,
    pub(super) viewport_width_bits: u32,
    pub(super) items_len: usize,
    pub(super) item_heights_snapshot: Vec<f32>,
    pub(super) anchors: Vec<ConversationAnchor>,
}

impl AnchorCache {
    pub(super) fn matches(&self, transcript: &TranscriptState, viewport_width_bits: u32) -> bool {
        self.revision == transcript.revision
            && self.viewport_width_bits == viewport_width_bits
            && self.items_len == transcript.items.len()
            && self.item_heights_snapshot == transcript.item_heights
    }
}

/// Builds one anchor per turn. `offsets` must be the same prefix-sum vector
/// `layout_offsets` produced for `viewport_width` - the caller is
/// responsible for keeping them in sync (see `TranscriptState::anchors`).
pub(super) fn derive_anchors(
    transcript: &TranscriptState,
    offsets: &[(f32, f32)],
) -> Vec<ConversationAnchor> {
    let item_count = transcript.items.len();
    transcript
        .turns
        .iter()
        .filter_map(|turn| {
            let start = turn.start_item_index.min(item_count);
            let user_excerpt = match transcript.items.get(start) {
                Some(TranscriptItem::UserText { text, .. }) => {
                    char_excerpt(text, USER_EXCERPT_CHARS)
                }
                _ => return None,
            };
            let y_offset = offsets.get(start)?.0;
            let end = turn
                .end_item_index
                .unwrap_or(item_count)
                .min(item_count)
                .max(start);
            let response = turn.response_item_index.clamp(start, end);
            let reply_excerpt = transcript.items[response..end]
                .iter()
                .find_map(|item| match item {
                    TranscriptItem::AssistantText { text, .. } => {
                        Some(char_excerpt(text, REPLY_EXCERPT_CHARS))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            let mut files = Vec::new();
            for item in &transcript.items[start..end] {
                if let TranscriptItem::Tool(tool) = item {
                    if let Some(path) = file_tool_path(tool) {
                        let name = basename(&path);
                        if !files.contains(&name) {
                            files.push(name);
                        }
                    }
                }
            }
            Some(ConversationAnchor {
                item_index: start,
                y_offset,
                user_excerpt,
                reply_excerpt,
                files,
            })
        })
        .collect()
}

/// Recovers a file path from a `ToolCall`'s human-readable `summary` - the
/// only place a Write/Edit/NotebookEdit call's input survives into the
/// transcript (`ToolCall` has no structured path field; `summary` is built
/// once by `safe_tool_summary` in zode-app-runtime as `"{name} path={value}"`
/// when the tool's JSON input had a top-level `path` key, and may itself be
/// truncated with a trailing "…" for very long paths). Returns `None` for
/// any other tool name or when `summary` doesn't match that convention -
/// callers must degrade to no file chip rather than guess.
fn file_tool_path(tool: &ToolCall) -> Option<String> {
    if !matches!(
        tool.name.as_str(),
        "FileWrite" | "FileEdit" | "NotebookEdit"
    ) {
        return None;
    }
    let prefix = format!("{} path=", tool.name);
    let value = tool.summary.strip_prefix(&prefix)?;
    (!value.is_empty()).then(|| value.to_string())
}

fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

/// First `max_chars` characters (not bytes), with a trailing ellipsis when
/// truncated. Character-count based per the design doc, not a pixel-width
/// fit - this crate has no rendering dependency.
fn char_excerpt(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use zode_node_protocol::{ToolStatus, TurnId};

    use super::*;
    use crate::transcript::TranscriptState;

    fn turn_id(tag: &str) -> TurnId {
        TurnId::parse(&format!("00000000-0000-0000-0000-{tag:0>12}")).unwrap()
    }

    fn offsets_for(transcript: &TranscriptState, width: f32) -> Vec<(f32, f32)> {
        // A simple fixed per-item height stand-in for tests: this crate has
        // no rendering dependency to measure real text, and only relative
        // ordering / presence of an offset matters for these assertions.
        let _ = width;
        let mut top = 0.0;
        transcript
            .items
            .iter()
            .map(|_| {
                let bottom = top + 40.0;
                let entry = (top, bottom);
                top = bottom;
                entry
            })
            .collect()
    }

    fn write_tool(id: &str, path: &str) -> TranscriptItem {
        TranscriptItem::Tool(ToolCall {
            id: id.into(),
            name: "FileWrite".into(),
            status: ToolStatus::Completed,
            summary: format!("FileWrite path={path}"),
            detail: None,
        })
    }

    #[test]
    fn splits_one_anchor_per_turn_with_excerpts_and_files() {
        // Items are appended incrementally around begin/finish, mirroring
        // how the runtime actually drives a turn's boundaries - a turn's
        // `end_item_index` freezes at whatever `items.len()` is when it
        // finishes, so a fixture that pre-populates every item up front
        // (before starting any turn) would give every turn the same,
        // too-wide end boundary.
        let mut transcript = TranscriptState::default();
        transcript.items.push(TranscriptItem::user_text(
            "Please rewrite the parser module for clarity",
        ));
        let started = Instant::now();
        assert!(transcript.begin_turn_at(turn_id("1"), 0, 1, started));
        transcript
            .items
            .push(write_tool("tool-1", "crates/foo/src/parser.rs"));
        transcript.items.push(TranscriptItem::assistant_text(
            "Rewrote the parser to use a recursive-descent design.",
        ));
        assert!(transcript.finish_turn_at(turn_id("1"), false, started + Duration::from_secs(1)));

        transcript
            .items
            .push(TranscriptItem::user_text("Now add tests"));
        assert!(transcript.begin_turn_at(turn_id("2"), 3, 4, started));
        transcript
            .items
            .push(write_tool("tool-2", "crates/foo/tests/parser_tests.rs"));
        transcript.items.push(TranscriptItem::assistant_text(
            "Added coverage for the new parser paths.",
        ));
        assert!(transcript.finish_turn_at(turn_id("2"), false, started + Duration::from_secs(2)));

        let offsets = offsets_for(&transcript, 400.0);
        let anchors = derive_anchors(&transcript, &offsets);

        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].item_index, 0);
        assert_eq!(
            anchors[0].user_excerpt,
            "Please rewrite the parser module for clarity"
        );
        assert_eq!(
            anchors[0].reply_excerpt,
            "Rewrote the parser to use a recursive-descent design."
        );
        assert_eq!(anchors[0].files, vec!["parser.rs".to_string()]);
        assert_eq!(anchors[1].item_index, 3);
        assert_eq!(anchors[1].files, vec!["parser_tests.rs".to_string()]);
        assert!(anchors[1].y_offset > anchors[0].y_offset);
    }

    #[test]
    fn excerpts_truncate_with_ellipsis_and_files_dedupe() {
        let long_user = "x".repeat(200);
        let long_reply = "y".repeat(200);
        let mut transcript = TranscriptState {
            items: vec![
                TranscriptItem::user_text(long_user.clone()),
                write_tool("tool-1", "src/lib.rs"),
                write_tool("tool-2", "src/lib.rs"),
                TranscriptItem::assistant_text(long_reply.clone()),
            ],
            ..TranscriptState::default()
        };
        let started = Instant::now();
        assert!(transcript.begin_turn_at(turn_id("3"), 0, 1, started));
        assert!(transcript.finish_turn_at(turn_id("3"), false, started));

        let offsets = offsets_for(&transcript, 400.0);
        let anchors = derive_anchors(&transcript, &offsets);

        assert_eq!(anchors.len(), 1);
        assert_eq!(
            anchors[0].user_excerpt.chars().count(),
            USER_EXCERPT_CHARS + 1
        );
        assert!(anchors[0].user_excerpt.ends_with('…'));
        assert_eq!(
            anchors[0].reply_excerpt.chars().count(),
            REPLY_EXCERPT_CHARS + 1
        );
        assert!(anchors[0].reply_excerpt.ends_with('…'));
        assert_eq!(anchors[0].files, vec!["lib.rs".to_string()]);
    }

    #[test]
    fn non_file_tools_and_unparseable_summaries_yield_no_chips() {
        let mut transcript = TranscriptState {
            items: vec![
                TranscriptItem::user_text("Investigate the failing test"),
                TranscriptItem::Tool(ToolCall {
                    id: "tool-1".into(),
                    name: "Grep".into(),
                    status: ToolStatus::Completed,
                    summary: "Grep query=timeout".into(),
                    detail: None,
                }),
                TranscriptItem::Tool(ToolCall {
                    id: "tool-2".into(),
                    name: "FileEdit".into(),
                    status: ToolStatus::Completed,
                    summary: "FileEdit".into(), // no recoverable path
                    detail: None,
                }),
                TranscriptItem::assistant_text("Found the root cause."),
            ],
            ..TranscriptState::default()
        };
        let started = Instant::now();
        assert!(transcript.begin_turn_at(turn_id("4"), 0, 1, started));
        assert!(transcript.finish_turn_at(turn_id("4"), false, started));

        let offsets = offsets_for(&transcript, 400.0);
        let anchors = derive_anchors(&transcript, &offsets);

        assert_eq!(anchors.len(), 1);
        assert!(anchors[0].files.is_empty());
    }

    #[test]
    fn cache_key_tracks_the_same_signals_as_the_layout_cache() {
        let mut transcript = TranscriptState {
            items: vec![TranscriptItem::user_text("hi")],
            ..TranscriptState::default()
        };
        assert!(transcript.begin_turn_at(turn_id("5"), 0, 1, Instant::now()));

        let cache = AnchorCache {
            revision: transcript.revision,
            viewport_width_bits: 400.0_f32.to_bits(),
            items_len: transcript.items.len(),
            item_heights_snapshot: transcript.item_heights.clone(),
            anchors: Vec::new(),
        };
        assert!(cache.matches(&transcript, 400.0_f32.to_bits()));
        assert!(!cache.matches(&transcript, 800.0_f32.to_bits()));

        transcript.touch_layout();
        assert!(!cache.matches(&transcript, 400.0_f32.to_bits()));
    }
}
