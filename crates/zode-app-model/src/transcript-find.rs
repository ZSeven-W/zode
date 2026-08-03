//! In-conversation find state (Codex `threadFindBar` parity).
//!
//! Matching is a plain case-insensitive substring scan over the *visible*
//! text of each transcript item - the same strings the transcript renderer
//! puts on screen - so the counter never claims a hit the user cannot see.
//! No regex: the bar is a reading aid, not a query language.
//!
//! The match list is derived, never stored as authoritative state: it is
//! memoized behind a `RefCell` keyed on the query plus the same
//! `revision`/`items.len()` signals `TranscriptState::layout_offsets` already
//! uses, so a streaming turn that appends items invalidates it automatically.
//! That means no mutation site anywhere in the app has to remember to tell
//! the find bar that the transcript changed.

use std::cell::{Ref, RefCell};

use crate::{TranscriptItem, TranscriptState, TranscriptVisualKind};

/// One match inside one transcript item's visible text.
///
/// `field` indexes [`item_search_fields`] for that item, so an item with
/// several rendered strings (a tool's summary and detail, an activity
/// group's entries) can distinguish which one matched. `kind` is the item's
/// existing visual vocabulary, which is exactly the axis a later
/// source-type filter (Codex's "只看用户/助手/工具") would filter on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptFindMatch {
    pub item_index: usize,
    pub field: usize,
    pub kind: TranscriptVisualKind,
    /// Byte offset of the match start within the matched field's text.
    /// Byte, not character, offsets: they are what slicing the field for a
    /// run-level highlight needs, and the folding below preserves them
    /// exactly (see [`fold_char`]).
    pub start: usize,
    /// Byte offset one past the match end, in the same field's text.
    pub end: usize,
}

/// The visible strings of one transcript item, in render order.
///
/// Mirrors what `zode_app_ui`'s transcript renderer actually paints for each
/// variant. Anything the renderer derives rather than shows verbatim (turn
/// labels, relative timestamps, status chips) is deliberately absent - the
/// find bar searches conversation content, not chrome.
pub fn item_search_fields(item: &TranscriptItem) -> Vec<&str> {
    match item {
        TranscriptItem::UserText { text, .. }
        | TranscriptItem::AssistantText { text, .. }
        | TranscriptItem::Thinking(text) => vec![text.as_str()],
        TranscriptItem::ActivityGroup(entries) => entries
            .iter()
            .flat_map(|entry| std::iter::once(entry.title.as_str()).chain(entry.detail.as_deref()))
            .collect(),
        TranscriptItem::Tool(tool) => std::iter::once(tool.summary.as_str())
            .chain(tool.detail.as_deref())
            .collect(),
        TranscriptItem::FileArtifact(file) => std::iter::once(file.summary.as_str())
            .chain(std::iter::once(file.path.as_str()))
            .chain(file.change_summary.as_deref())
            .collect(),
        TranscriptItem::Attachment(attachment) => vec![attachment.display_name.as_str()],
        TranscriptItem::Image(image) => vec![image.path.as_str()],
        TranscriptItem::GoalProgress(goal) => vec![goal.title.as_str()],
        // The phase verb is derived from `phase` rather than shown verbatim,
        // so it stays out of the search corpus for the same reason turn
        // labels and status chips do.
        TranscriptItem::SubagentChip(chip) => std::iter::once(chip.display_name.as_str())
            .chain(chip.summary.as_deref())
            .chain(chip.model.as_deref())
            .collect(),
        TranscriptItem::Approval { tool, .. } => vec![tool.as_str()],
        TranscriptItem::Status { message, .. } | TranscriptItem::Error { message, .. } => {
            vec![message.as_str()]
        }
    }
}

/// Every non-overlapping case-insensitive occurrence of `query`, in
/// transcript order then field order then position order. An empty or
/// whitespace-only query matches nothing (rather than everything), so the
/// counter reads `0/0` while the user is still typing the first character.
pub fn find_matches(transcript: &TranscriptState, query: &str) -> Vec<TranscriptFindMatch> {
    let needle: Vec<char> = query.trim().chars().map(fold_char).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (item_index, item) in transcript.items.iter().enumerate() {
        let kind = item.visual_kind();
        for (field, text) in item_search_fields(item).into_iter().enumerate() {
            for (start, end) in field_matches(text, &needle) {
                matches.push(TranscriptFindMatch {
                    item_index,
                    field,
                    kind,
                    start,
                    end,
                });
            }
        }
    }
    matches
}

/// Case-folds one character for comparison while keeping it a single
/// character, so byte offsets into the original text stay exact. Full
/// Unicode case folding can expand one character into several (the German
/// sharp s folds to a two-character `ss`),
/// which would desynchronize those offsets; taking the first folded
/// character is the standard "good enough for search" compromise and is
/// exact for ASCII and CJK, the text this transcript actually carries.
fn fold_char(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

/// Non-overlapping matches of `needle` (already folded) inside `text`,
/// returned as byte ranges. A naive scan: transcripts are at most a few
/// hundred kilobytes and this only runs on a cache miss.
fn field_matches(text: &str, needle: &[char]) -> Vec<(usize, usize)> {
    let folded: Vec<(usize, char)> = text
        .char_indices()
        .map(|(offset, character)| (offset, fold_char(character)))
        .collect();
    if folded.len() < needle.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut index = 0;
    while index + needle.len() <= folded.len() {
        let hit = folded[index..index + needle.len()]
            .iter()
            .zip(needle)
            .all(|((_, character), wanted)| character == wanted);
        if hit {
            let start = folded[index].0;
            let end = folded
                .get(index + needle.len())
                .map_or(text.len(), |(offset, _)| *offset);
            matches.push((start, end));
            index += needle.len();
        } else {
            index += 1;
        }
    }
    matches
}

/// Memoized match list plus the inputs that can invalidate it. Keyed like
/// `TranscriptLayoutCache`: `revision` catches edits made through
/// `TranscriptState`'s own methods, `items_len` is the safety net for the
/// fixtures and call sites that push onto the `pub` `items` vector directly.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptFindCache {
    query: String,
    revision: u64,
    items_len: usize,
    matches: Vec<TranscriptFindMatch>,
}

/// Per-session find-bar state. Shaped like `GlobalSearchState` (open flag +
/// query + active index) with the derived match list memoized alongside.
#[derive(Debug, Clone, Default)]
pub struct TranscriptFindState {
    pub open: bool,
    pub query: String,
    /// Which match is current. Stored unclamped and clamped on read, so a
    /// turn that appends or removes matches never needs to rewrite it.
    pub active: usize,
    cache: RefCell<Option<TranscriptFindCache>>,
}

/// Two find states are equal iff their user-visible intent is equal. The
/// memo is derived bookkeeping - including it would make `ZodeAppState`
/// equality (which drives accessibility-tree and persistence dirty checks)
/// report a change every time a frame happened to repopulate the cache.
impl PartialEq for TranscriptFindState {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open && self.query == other.query && self.active == other.active
    }
}

impl Eq for TranscriptFindState {}

impl TranscriptFindState {
    /// The current match list, recomputed only when the query or the
    /// transcript changed since the last call.
    pub fn matches(&self, transcript: &TranscriptState) -> Ref<'_, Vec<TranscriptFindMatch>> {
        let hit = self.cache.borrow().as_ref().is_some_and(|cache| {
            cache.query == self.query
                && cache.revision == transcript.revision
                && cache.items_len == transcript.items.len()
        });
        if !hit {
            *self.cache.borrow_mut() = Some(TranscriptFindCache {
                query: self.query.clone(),
                revision: transcript.revision,
                items_len: transcript.items.len(),
                matches: find_matches(transcript, &self.query),
            });
        }
        Ref::map(self.cache.borrow(), |cache| {
            &cache.as_ref().expect("populated above").matches
        })
    }

    pub fn match_count(&self, transcript: &TranscriptState) -> usize {
        self.matches(transcript).len()
    }

    /// The current match, with `active` clamped into range. `None` when
    /// nothing matches.
    pub fn active_match(&self, transcript: &TranscriptState) -> Option<TranscriptFindMatch> {
        let matches = self.matches(transcript);
        matches.get(self.active_index(matches.len())?).copied()
    }

    /// `active` clamped into `0..count`, or `None` when there are no matches.
    pub fn active_index(&self, count: usize) -> Option<usize> {
        count.checked_sub(1).map(|last| self.active.min(last))
    }

    /// The bar's "N/M" counter. Reads `0/0` when nothing matches, so an
    /// empty query and a query with no hits look the same - both mean
    /// "there is nothing to step through".
    pub fn counter_label(&self, transcript: &TranscriptState) -> String {
        let count = self.match_count(transcript);
        let current = self.active_index(count).map_or(0, |index| index + 1);
        format!("{current}/{count}")
    }

    /// Whether `item_index` holds the current match. Drives the transcript's
    /// distinct current-match band.
    pub fn is_active_item(&self, transcript: &TranscriptState, item_index: usize) -> bool {
        self.active_match(transcript)
            .is_some_and(|found| found.item_index == item_index)
    }

    /// Whether `item_index` holds any match at all. Drives the transcript's
    /// secondary band on every other matched item.
    pub fn is_matched_item(&self, transcript: &TranscriptState, item_index: usize) -> bool {
        self.matches(transcript)
            .iter()
            .any(|found| found.item_index == item_index)
    }

    /// Steps `active` one match forward or backward, wrapping at both ends.
    /// Returns `false` when there is nothing to step through.
    pub fn step(&mut self, transcript: &TranscriptState, forward: bool) -> bool {
        let count = self.match_count(transcript);
        let Some(current) = self.active_index(count) else {
            return false;
        };
        self.active = if forward {
            (current + 1) % count
        } else {
            current.checked_sub(1).unwrap_or(count - 1)
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use zode_node_protocol::{ToolCall, ToolStatus};

    use super::*;
    use crate::{ActivityEntry, TranscriptItem, TranscriptState};

    fn transcript(items: Vec<TranscriptItem>) -> TranscriptState {
        TranscriptState {
            items,
            ..TranscriptState::default()
        }
    }

    #[test]
    fn matching_is_case_insensitive_and_spans_messages_and_tools() {
        let transcript = transcript(vec![
            TranscriptItem::user_text("Please Fix the PARSER"),
            TranscriptItem::Tool(ToolCall {
                id: "tool-1".into(),
                name: "FileEdit".into(),
                status: ToolStatus::Completed,
                summary: "FileEdit path=src/parser.rs".into(),
                detail: Some("rewrote the parser entry point".into()),
            }),
            TranscriptItem::assistant_text("Nothing here"),
        ]);

        let matches = find_matches(&transcript, "parser");

        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].item_index, 0);
        assert_eq!(matches[0].kind, TranscriptVisualKind::UserMarkdown);
        assert_eq!(
            &"Please Fix the PARSER"[matches[0].start..matches[0].end],
            "PARSER"
        );
        assert_eq!(matches[1].item_index, 1);
        assert_eq!(matches[1].field, 0);
        assert_eq!(matches[2].item_index, 1);
        assert_eq!(matches[2].field, 1, "the tool detail is its own field");
    }

    #[test]
    fn overlapping_occurrences_are_counted_once_each() {
        let transcript = transcript(vec![TranscriptItem::user_text("aaaa")]);
        let matches = find_matches(&transcript, "aa");
        assert_eq!(matches.len(), 2);
        assert_eq!((matches[0].start, matches[0].end), (0, 2));
        assert_eq!((matches[1].start, matches[1].end), (2, 4));
    }

    #[test]
    fn byte_offsets_stay_valid_for_multibyte_text() {
        let text = "打开工作区，工作区已更新";
        let transcript = transcript(vec![TranscriptItem::assistant_text(text)]);
        let matches = find_matches(&transcript, "工作区");
        assert_eq!(matches.len(), 2);
        for found in &matches {
            assert_eq!(&text[found.start..found.end], "工作区");
        }
    }

    #[test]
    fn blank_queries_match_nothing() {
        let transcript = transcript(vec![TranscriptItem::user_text("anything")]);
        assert!(find_matches(&transcript, "").is_empty());
        assert!(find_matches(&transcript, "   ").is_empty());
    }

    #[test]
    fn activity_entries_expose_every_visible_line() {
        let item = TranscriptItem::ActivityGroup(vec![
            ActivityEntry {
                id: "a".into(),
                title: "读取配置".into(),
                detail: Some("config.json".into()),
                completed: true,
            },
            ActivityEntry {
                id: "b".into(),
                title: "写入配置".into(),
                detail: None,
                completed: false,
            },
        ]);
        assert_eq!(
            item_search_fields(&item),
            vec!["读取配置", "config.json", "写入配置"]
        );
    }

    #[test]
    fn subagent_chips_are_searchable_by_name_summary_and_model() {
        let item = TranscriptItem::SubagentChip(crate::SubagentChip {
            agent_id: "1".into(),
            display_name: "审查代码".into(),
            agent_type: "reviewer".into(),
            phase: crate::SubagentChipPhase::Finished,
            summary: Some("已读取三个文件".into()),
            model: Some("claude-opus-5".into()),
        });
        assert_eq!(
            item_search_fields(&item),
            vec!["审查代码", "已读取三个文件", "claude-opus-5"]
        );

        let transcript = transcript(vec![item]);
        let matches = find_matches(&transcript, "OPUS");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, TranscriptVisualKind::SubagentChip);
        assert_eq!(matches[0].field, 2);
    }

    #[test]
    fn the_memo_recomputes_when_the_query_or_the_transcript_changes() {
        let mut transcript = transcript(vec![TranscriptItem::user_text("alpha")]);
        let mut find = TranscriptFindState {
            query: "alpha".into(),
            ..TranscriptFindState::default()
        };
        assert_eq!(find.match_count(&transcript), 1);

        transcript
            .items
            .push(TranscriptItem::assistant_text("alpha alpha"));
        transcript.touch_layout();
        assert_eq!(find.match_count(&transcript), 3);

        find.query = "beta".into();
        assert_eq!(find.match_count(&transcript), 0);
    }

    #[test]
    fn stepping_wraps_at_both_ends() {
        let transcript = transcript(vec![
            TranscriptItem::user_text("hit"),
            TranscriptItem::assistant_text("hit hit"),
        ]);
        let mut find = TranscriptFindState {
            query: "hit".into(),
            ..TranscriptFindState::default()
        };
        assert_eq!(find.match_count(&transcript), 3);

        assert!(find.step(&transcript, true));
        assert_eq!(find.active, 1);
        assert!(find.step(&transcript, true));
        assert!(find.step(&transcript, true));
        assert_eq!(find.active, 0, "forward wraps past the last match");

        assert!(find.step(&transcript, false));
        assert_eq!(find.active, 2, "backward wraps past the first match");
    }

    #[test]
    fn an_out_of_range_active_index_is_clamped_rather_than_lost() {
        let transcript = transcript(vec![TranscriptItem::user_text("hit hit")]);
        let find = TranscriptFindState {
            query: "hit".into(),
            active: 7,
            ..TranscriptFindState::default()
        };
        assert_eq!(find.counter_label(&transcript), "2/2");
        assert_eq!(find.active_match(&transcript).unwrap().start, 4);
    }

    #[test]
    fn an_empty_match_list_reports_a_zero_counter() {
        let transcript = transcript(vec![TranscriptItem::user_text("hello")]);
        let mut find = TranscriptFindState {
            query: "missing".into(),
            ..TranscriptFindState::default()
        };
        assert_eq!(find.counter_label(&transcript), "0/0");
        assert!(find.active_match(&transcript).is_none());
        assert!(!find.step(&transcript, true));
    }
}
