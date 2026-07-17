use std::time::{Duration, Instant};

use zode_node_protocol::{ToolCall, TurnId};

/// Lightweight metadata for an attachment that can safely live in immutable UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMetadata {
    /// Stable identity within the composer or transcript. Display names are not unique.
    pub id: String,
    /// Optional durable source path. Clipboard images intentionally have no path.
    pub path: Option<String>,
    pub display_name: String,
    pub media_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Encoded payload size in bytes. The payload itself never enters app state.
    pub byte_len: u64,
}

/// One real activity reported by the runtime. Empty placeholder activities are not produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
    pub completed: bool,
}

/// A durable file result that can later open in the document preview pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileArtifact {
    pub id: String,
    pub path: String,
    pub summary: String,
    pub change_summary: Option<String>,
}

/// Explicit progress supplied by a goal source. Status text is never inferred into this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalProgress {
    pub id: String,
    pub title: String,
    pub completed: u32,
    pub total: u32,
}

/// Lifecycle state for one user-initiated agent turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptTurnStatus {
    /// The turn was started locally and has not reached a terminal edge.
    Running,
    /// The runtime emitted a non-interrupted `TurnFinished` edge.
    Completed,
    /// The runtime emitted an interrupted `TurnFinished` edge.
    Interrupted,
    /// Dispatch failed before the runtime could emit `TurnFinished`.
    Failed,
    /// A persisted transcript exposed content but no trustworthy lifecycle timing.
    Restored,
}

/// UI-consumable metadata for one transcript turn.
///
/// Item indexes are half-open boundaries. `start_item_index` points at the
/// first user-projected item, `response_item_index` points at the first item
/// owned by the runtime response, and `end_item_index` is set at completion.
/// Historical turns intentionally have no id or elapsed duration because the
/// current history protocol does not persist a `TurnFinished` timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub turn_id: Option<TurnId>,
    pub start_item_index: usize,
    pub response_item_index: usize,
    pub end_item_index: Option<usize>,
    pub status: TranscriptTurnStatus,
    pub elapsed: Option<Duration>,
    started_at: Option<Instant>,
    terminal_error: bool,
}

impl TurnSummary {
    /// Returns frozen terminal elapsed time or a live monotonic duration.
    pub fn elapsed_at(&self, now: Instant) -> Option<Duration> {
        self.elapsed.or_else(|| {
            (self.status == TranscriptTurnStatus::Running)
                .then_some(self.started_at)
                .flatten()
                .map(|started_at| now.saturating_duration_since(started_at))
        })
    }

    fn live(
        turn_id: TurnId,
        start_item_index: usize,
        response_item_index: usize,
        started_at: Instant,
    ) -> Self {
        Self {
            turn_id: Some(turn_id),
            start_item_index,
            response_item_index,
            end_item_index: None,
            status: TranscriptTurnStatus::Running,
            elapsed: None,
            started_at: Some(started_at),
            terminal_error: false,
        }
    }

    fn restored(
        start_item_index: usize,
        response_item_index: usize,
        end_item_index: usize,
    ) -> Self {
        Self {
            turn_id: None,
            start_item_index,
            response_item_index,
            end_item_index: Some(end_item_index),
            status: TranscriptTurnStatus::Restored,
            elapsed: None,
            started_at: None,
            terminal_error: false,
        }
    }
}

impl GoalProgress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed.min(self.total) as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

/// Stable visual vocabulary consumed by paint, measurement and accessibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TranscriptVisualKind {
    UserMarkdown,
    AssistantMarkdown,
    Thinking,
    Activity,
    Tool,
    FileArtifact,
    Attachment,
    GoalProgress,
    Approval,
    Status,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Read,
    Create,
    Modify,
    Delete,
    Orchestrate,
    Other,
}

pub fn tool_category(name: &str) -> ToolCategory {
    let name = name.to_ascii_lowercase();
    if starts_with_any(
        &name,
        &[
            "read", "get", "list", "search", "find", "count", "snapshot", "export",
        ],
    ) {
        ToolCategory::Read
    } else if starts_with_any(&name, &["create", "insert", "add", "mkdir"]) {
        ToolCategory::Create
    } else if starts_with_any(&name, &["delete", "remove", "unlink", "rmdir"]) {
        ToolCategory::Delete
    } else if starts_with_any(&name, &["task", "agent", "orchestrate", "plan", "workflow"]) {
        ToolCategory::Orchestrate
    } else if starts_with_any(
        &name,
        &["edit", "modify", "update", "write", "move", "copy"],
    ) {
        ToolCategory::Modify
    } else {
        ToolCategory::Other
    }
}

pub fn default_tool_expanded(name: &str) -> bool {
    !matches!(
        tool_category(name),
        ToolCategory::Read | ToolCategory::Create
    )
}

fn starts_with_any(name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

/// A renderable item in one conversation transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptItem {
    UserText(String),
    AssistantText(String),
    Thinking(String),
    ActivityGroup(Vec<ActivityEntry>),
    Tool(ToolCall),
    FileArtifact(FileArtifact),
    Attachment(AttachmentMetadata),
    GoalProgress(GoalProgress),
    Approval { id: String, tool: String },
    Status { code: String, message: String },
    Error { message: String, retryable: bool },
}

impl TranscriptItem {
    pub const fn visual_kind(&self) -> TranscriptVisualKind {
        match self {
            Self::UserText(_) => TranscriptVisualKind::UserMarkdown,
            Self::AssistantText(_) => TranscriptVisualKind::AssistantMarkdown,
            Self::Thinking(_) => TranscriptVisualKind::Thinking,
            Self::ActivityGroup(_) => TranscriptVisualKind::Activity,
            Self::Tool(_) => TranscriptVisualKind::Tool,
            Self::FileArtifact(_) => TranscriptVisualKind::FileArtifact,
            Self::Attachment(_) => TranscriptVisualKind::Attachment,
            Self::GoalProgress(_) => TranscriptVisualKind::GoalProgress,
            Self::Approval { .. } => TranscriptVisualKind::Approval,
            Self::Status { .. } => TranscriptVisualKind::Status,
            Self::Error { .. } => TranscriptVisualKind::Error,
        }
    }

    /// Stable key for one transcript position. Rich artifacts use runtime identities;
    /// append-only text/status entries use their immutable transcript index.
    pub fn stable_key(&self, index: usize) -> String {
        match self {
            Self::ActivityGroup(entries) => entries
                .first()
                .map(|entry| format!("activity:{}", entry.id))
                .unwrap_or_else(|| format!("activity:{index}")),
            Self::Tool(tool) => format!("tool:{}", tool.id),
            Self::FileArtifact(file) => format!("file:{}", file.id),
            Self::Attachment(attachment) => format!("attachment:{}", attachment.id),
            Self::GoalProgress(goal) => format!("goal:{}", goal.id),
            Self::Approval { id, .. } => format!("approval:{id}"),
            Self::UserText(_) => format!("user:{index}"),
            Self::AssistantText(_) => format!("assistant:{index}"),
            Self::Thinking(_) => format!("thinking:{index}"),
            Self::Status { code, .. } => format!("status:{index}:{code}"),
            Self::Error { .. } => format!("error:{index}"),
        }
    }
}

/// Ordered transcript state and its event-stream cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptState {
    pub items: Vec<TranscriptItem>,
    /// Turn boundaries and lifecycle metadata kept separately from renderable items.
    pub turns: Vec<TurnSummary>,
    pub last_sequence: u64,
    pub busy: bool,
    pub scroll_offset: f32,
    pub follow_tail: bool,
    /// Last measured height for each item. Zero means the UI should estimate.
    pub item_heights: Vec<f32>,
}

impl Default for TranscriptState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            turns: Vec::new(),
            last_sequence: 0,
            busy: false,
            scroll_offset: 0.0,
            follow_tail: true,
            item_heights: Vec::new(),
        }
    }
}

impl TranscriptState {
    /// Starts one live turn using a monotonic clock.
    pub fn begin_turn(
        &mut self,
        turn_id: TurnId,
        start_item_index: usize,
        response_item_index: usize,
    ) -> bool {
        self.begin_turn_at(
            turn_id,
            start_item_index,
            response_item_index,
            Instant::now(),
        )
    }

    /// Deterministic-clock variant used by reducers and tests.
    pub fn begin_turn_at(
        &mut self,
        turn_id: TurnId,
        start_item_index: usize,
        response_item_index: usize,
        started_at: Instant,
    ) -> bool {
        let valid_boundary =
            start_item_index <= response_item_index && response_item_index <= self.items.len();
        let already_running = self
            .turns
            .iter()
            .any(|turn| turn.status == TranscriptTurnStatus::Running);
        if !valid_boundary || already_running {
            return false;
        }
        self.turns.push(TurnSummary::live(
            turn_id,
            start_item_index,
            response_item_index,
            started_at,
        ));
        self.busy = true;
        true
    }

    /// Completes a live turn at the current monotonic instant.
    pub fn finish_turn(&mut self, turn_id: TurnId, interrupted: bool) -> bool {
        self.finish_turn_at(turn_id, interrupted, Instant::now())
    }

    /// Deterministic-clock completion used by the agent-event reducer.
    pub fn finish_turn_at(
        &mut self,
        turn_id: TurnId,
        interrupted: bool,
        finished_at: Instant,
    ) -> bool {
        let status = if interrupted {
            TranscriptTurnStatus::Interrupted
        } else {
            TranscriptTurnStatus::Completed
        };
        self.complete_turn_at(turn_id, status, finished_at)
    }

    /// Marks a turn whose start command failed before a runtime completion edge.
    pub fn fail_turn(&mut self, turn_id: TurnId) -> bool {
        self.complete_turn_at(turn_id, TranscriptTurnStatus::Failed, Instant::now())
    }

    /// Records a terminal error until `TurnFinished` freezes the turn bounds.
    pub fn record_terminal_error(&mut self, turn_id: TurnId) -> bool {
        let Some(turn) = self.turns.iter_mut().rev().find(|turn| {
            turn.turn_id == Some(turn_id) && turn.status == TranscriptTurnStatus::Running
        }) else {
            return false;
        };
        turn.terminal_error = true;
        true
    }

    /// Removes an optimistic turn that never reached the runtime command pump.
    ///
    /// Callers must use this only when they also roll back the projected user
    /// items. A dispatched turn should instead finish or fail so its lifecycle
    /// remains visible in the transcript.
    pub fn discard_turn(&mut self, turn_id: TurnId) -> bool {
        let Some(index) = self
            .turns
            .iter()
            .rposition(|turn| turn.turn_id == Some(turn_id))
        else {
            return false;
        };
        self.turns.remove(index);
        self.busy = self
            .turns
            .iter()
            .any(|turn| turn.status == TranscriptTurnStatus::Running);
        true
    }

    /// Inserts late-projected user artifacts at the latest live turn boundary.
    /// Composer attachment metadata arrives after command preparation. It must
    /// still appear before runtime output (including a synchronous dispatch
    /// error) and before the visual turn divider.
    pub fn insert_latest_turn_user_items(&mut self, items: Vec<TranscriptItem>) -> bool {
        if items.is_empty() {
            return true;
        }
        let Some(turn_index) = self.turns.iter().rposition(|turn| {
            turn.turn_id.is_some()
                && matches!(
                    turn.status,
                    TranscriptTurnStatus::Running | TranscriptTurnStatus::Failed
                )
        }) else {
            return false;
        };
        let insertion_index = self.turns[turn_index]
            .response_item_index
            .min(self.items.len());
        let inserted = items.len();
        self.items.splice(insertion_index..insertion_index, items);
        if !self.item_heights.is_empty() {
            let height_index = insertion_index.min(self.item_heights.len());
            self.item_heights.splice(
                height_index..height_index,
                std::iter::repeat_n(0.0, inserted),
            );
        }
        let turn = &mut self.turns[turn_index];
        turn.response_item_index += inserted;
        if let Some(end) = turn.end_item_index.as_mut() {
            if *end >= insertion_index {
                *end += inserted;
            }
        }
        true
    }

    /// Removes one approval projection and keeps all turn boundaries aligned.
    pub fn remove_approval(&mut self, approval_id: &str) -> bool {
        let removed = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                matches!(item, TranscriptItem::Approval { id, .. } if id == approval_id)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if removed.is_empty() {
            return false;
        }

        let old_items = std::mem::take(&mut self.items);
        self.items = old_items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| (!removed.contains(&index)).then_some(item))
            .collect();
        if !self.item_heights.is_empty() {
            let old_heights = std::mem::take(&mut self.item_heights);
            self.item_heights = old_heights
                .into_iter()
                .enumerate()
                .filter_map(|(index, height)| (!removed.contains(&index)).then_some(height))
                .collect();
        }

        for turn in &mut self.turns {
            turn.start_item_index = remap_boundary(turn.start_item_index, &removed);
            turn.response_item_index = remap_boundary(turn.response_item_index, &removed);
            turn.end_item_index = turn
                .end_item_index
                .map(|boundary| remap_boundary(boundary, &removed));
        }
        true
    }

    fn complete_turn_at(
        &mut self,
        turn_id: TurnId,
        status: TranscriptTurnStatus,
        finished_at: Instant,
    ) -> bool {
        let end_item_index = self.items.len();
        let Some(turn) = self.turns.iter_mut().rev().find(|turn| {
            turn.turn_id == Some(turn_id) && turn.status == TranscriptTurnStatus::Running
        }) else {
            return false;
        };
        turn.status = if turn.terminal_error {
            TranscriptTurnStatus::Failed
        } else {
            status
        };
        turn.end_item_index = Some(end_item_index);
        turn.elapsed = turn
            .started_at
            .take()
            .map(|started_at| finished_at.saturating_duration_since(started_at));
        self.busy = false;
        true
    }

    /// Rebuilds conservative turn groupings from display-safe persisted items.
    ///
    /// History does not contain turn ids or terminal timestamps, so restored
    /// entries are explicitly marked `Restored` with `elapsed == None`.
    pub fn restore_historical_turns(&mut self) {
        self.turns.clear();
        let mut groups = Vec::new();
        let mut index = 0;
        while index < self.items.len() {
            if !matches!(self.items[index], TranscriptItem::UserText(_)) {
                index += 1;
                continue;
            }
            let start = index;
            while index < self.items.len()
                && matches!(self.items[index], TranscriptItem::UserText(_))
            {
                index += 1;
            }
            groups.push((start, index));
        }
        for (group_index, (start, response)) in groups.iter().copied().enumerate() {
            let end = groups
                .get(group_index + 1)
                .map_or(self.items.len(), |(next_start, _)| *next_start);
            self.turns.push(TurnSummary::restored(start, response, end));
        }
    }

    /// Replaces an item in place and invalidates its measured height. Runtime
    /// updates for tools, activities and goals must use this path.
    pub fn replace_item(&mut self, index: usize, item: TranscriptItem) -> bool {
        let Some(slot) = self.items.get_mut(index) else {
            return false;
        };
        *slot = item;
        if let Some(height) = self.item_heights.get_mut(index) {
            *height = 0.0;
        }
        true
    }
}

fn remap_boundary(boundary: usize, removed: &[usize]) -> usize {
    boundary.saturating_sub(removed.partition_point(|index| *index < boundary))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        time::{Duration, Instant},
    };

    use super::{
        ActivityEntry, AttachmentMetadata, FileArtifact, GoalProgress, TranscriptItem,
        TranscriptTurnStatus, TranscriptVisualKind,
    };
    use zode_node_protocol::{ToolCall, ToolStatus, TurnId};

    fn rich_transcript_fixture() -> Vec<TranscriptItem> {
        vec![
            TranscriptItem::UserText("Please update the renderer".into()),
            TranscriptItem::AssistantText("## Done\n\nUpdated the renderer.".into()),
            TranscriptItem::Thinking("Checking the layout".into()),
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "activity-1".into(),
                title: "Ran tests".into(),
                detail: Some("12 passed".into()),
                completed: true,
            }]),
            TranscriptItem::Tool(ToolCall {
                id: "tool-1".into(),
                name: "read_file".into(),
                status: ToolStatus::Completed,
                summary: "Read the source".into(),
                detail: None,
            }),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "file-1".into(),
                path: "crates/zode-app-ui/src/widgets/transcript/mod.rs".into(),
                summary: "Updated transcript rendering".into(),
                change_summary: Some("+120 -14".into()),
            }),
            TranscriptItem::Attachment(AttachmentMetadata {
                id: "attachment-1".into(),
                path: None,
                display_name: "layout.png".into(),
                media_type: "image/png".into(),
                width: Some(1280),
                height: Some(720),
                byte_len: 42_000,
            }),
            TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Reference rebuild".into(),
                completed: 3,
                total: 7,
            }),
            TranscriptItem::Approval {
                id: "approval-1".into(),
                tool: "shell".into(),
            },
            TranscriptItem::Status {
                code: "running".into(),
                message: "Still working".into(),
            },
            TranscriptItem::Error {
                message: "Build failed".into(),
                retryable: true,
            },
        ]
    }

    #[test]
    fn rich_transcript_exposes_five_visual_card_kinds() {
        let kinds = rich_transcript_fixture()
            .iter()
            .map(TranscriptItem::visual_kind)
            .collect::<BTreeSet<_>>();

        assert!(kinds.len() >= 5);
    }

    #[test]
    fn transcript_visual_kind_maps_every_variant() {
        let kinds = rich_transcript_fixture()
            .iter()
            .map(TranscriptItem::visual_kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TranscriptVisualKind::UserMarkdown,
                TranscriptVisualKind::AssistantMarkdown,
                TranscriptVisualKind::Thinking,
                TranscriptVisualKind::Activity,
                TranscriptVisualKind::Tool,
                TranscriptVisualKind::FileArtifact,
                TranscriptVisualKind::Attachment,
                TranscriptVisualKind::GoalProgress,
                TranscriptVisualKind::Approval,
                TranscriptVisualKind::Status,
                TranscriptVisualKind::Error,
            ]
        );
    }

    #[test]
    fn replacing_rich_activity_or_goal_invalidates_cached_height() {
        let mut transcript = super::TranscriptState {
            items: vec![
                TranscriptItem::ActivityGroup(vec![ActivityEntry {
                    id: "activity-1".into(),
                    title: "Running".into(),
                    detail: None,
                    completed: false,
                }]),
                TranscriptItem::GoalProgress(GoalProgress {
                    id: "goal-1".into(),
                    title: "Rebuild".into(),
                    completed: 1,
                    total: 4,
                }),
            ],
            item_heights: vec![64.0, 72.0],
            ..super::TranscriptState::default()
        };

        assert!(transcript.replace_item(
            0,
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "activity-1".into(),
                title: "Complete".into(),
                detail: Some("done".into()),
                completed: true,
            }]),
        ));
        assert!(transcript.replace_item(
            1,
            TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Rebuild".into(),
                completed: 2,
                total: 4,
            }),
        ));
        assert_eq!(transcript.item_heights, [0.0, 0.0]);
    }

    #[test]
    fn live_turn_freezes_real_monotonic_elapsed_at_completion() {
        let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000111").unwrap();
        let started_at = Instant::now();
        let mut transcript = super::TranscriptState {
            items: vec![TranscriptItem::UserText("ship it".into())],
            ..super::TranscriptState::default()
        };

        assert!(transcript.begin_turn_at(turn_id, 0, 1, started_at));
        assert_eq!(
            transcript.turns[0].elapsed_at(started_at + Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        transcript
            .items
            .push(TranscriptItem::AssistantText("done".into()));
        assert!(transcript.finish_turn_at(
            turn_id,
            false,
            started_at + Duration::from_millis(3_250),
        ));

        let turn = &transcript.turns[0];
        assert_eq!(turn.status, TranscriptTurnStatus::Completed);
        assert_eq!(turn.start_item_index, 0);
        assert_eq!(turn.response_item_index, 1);
        assert_eq!(turn.end_item_index, Some(2));
        assert_eq!(turn.elapsed, Some(Duration::from_millis(3_250)));
        assert!(!transcript.busy);
    }

    #[test]
    fn restored_turns_never_invent_ids_status_edges_or_elapsed_time() {
        let mut transcript = super::TranscriptState {
            items: vec![
                TranscriptItem::UserText("first block".into()),
                TranscriptItem::UserText("second block".into()),
                TranscriptItem::AssistantText("first answer".into()),
                TranscriptItem::UserText("next turn".into()),
                TranscriptItem::AssistantText("second answer".into()),
            ],
            ..super::TranscriptState::default()
        };

        transcript.restore_historical_turns();

        assert_eq!(transcript.turns.len(), 2);
        assert_eq!(transcript.turns[0].start_item_index, 0);
        assert_eq!(transcript.turns[0].response_item_index, 2);
        assert_eq!(transcript.turns[0].end_item_index, Some(3));
        assert_eq!(transcript.turns[1].start_item_index, 3);
        assert_eq!(transcript.turns[1].response_item_index, 4);
        assert_eq!(transcript.turns[1].end_item_index, Some(5));
        assert!(transcript.turns.iter().all(|turn| {
            turn.turn_id.is_none()
                && turn.status == TranscriptTurnStatus::Restored
                && turn.elapsed.is_none()
        }));
    }

    #[test]
    fn undispatched_turn_can_be_discarded_without_leaving_an_orphan_boundary() {
        let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000222").unwrap();
        let mut transcript = super::TranscriptState {
            items: vec![TranscriptItem::UserText("queued".into())],
            ..super::TranscriptState::default()
        };

        assert!(transcript.begin_turn(turn_id, 0, 1));
        transcript.items.clear();
        assert!(transcript.discard_turn(turn_id));

        assert!(transcript.turns.is_empty());
        assert!(!transcript.busy);
        assert!(!transcript.discard_turn(turn_id));
    }

    #[test]
    fn late_user_artifacts_are_inserted_before_runtime_output_and_the_divider() {
        let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000223").unwrap();
        let mut transcript = super::TranscriptState {
            items: vec![TranscriptItem::UserText("with attachment".into())],
            ..super::TranscriptState::default()
        };
        assert!(transcript.begin_turn(turn_id, 0, 1));
        assert!(transcript.fail_turn(turn_id));
        transcript.items.push(TranscriptItem::Error {
            message: "dispatch failed".into(),
            retryable: true,
        });

        assert!(
            transcript.insert_latest_turn_user_items(vec![TranscriptItem::Attachment(
                super::AttachmentMetadata {
                    id: "shot".into(),
                    path: None,
                    display_name: "shot.png".into(),
                    media_type: "image/png".into(),
                    width: Some(640),
                    height: Some(360),
                    byte_len: 1_024,
                },
            )])
        );

        assert_eq!(transcript.turns[0].response_item_index, 2);
        assert_eq!(transcript.turns[0].end_item_index, Some(2));
        assert!(matches!(
            transcript.items.as_slice(),
            [
                TranscriptItem::UserText(_),
                TranscriptItem::Attachment(_),
                TranscriptItem::Error { .. }
            ]
        ));
    }
}

#[cfg(test)]
#[path = "transcript/approval-tests.rs"]
mod approval_tests;
