use std::cell::{Cell, Ref, RefCell};
use std::time::{Duration, Instant};

use zode_node_protocol::{ToolCall, ToolStatus, TurnId};

use crate::SubagentChip;

mod anchors;
pub use anchors::ConversationAnchor;

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

/// An image the agent read or wrote, rendered inline as a thumbnail with a
/// tap-to-enlarge lightbox (see `LightboxState`). Deliberately carries no
/// encoded bytes - like `AttachmentMetadata`, this model crate never holds a
/// binary payload; the desktop host resolves `path` to pixels at paint time
/// (mirroring how `BrowserFrameView` supplies decoded frame bytes as an
/// ephemeral, borrowed view rather than through app state) and corrects
/// `item_heights` once the real aspect ratio is known via the existing
/// `AppCommand::SetTranscriptItemHeight`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageItem {
    /// Stable identity for this transcript position; addressed by
    /// `LightboxState::item_id` and the inline thumbnail's image cache key.
    pub id: String,
    /// Absolute or workspace-relative source path. Always populated - unlike
    /// `AttachmentMetadata::path`, an `Image` item only ever comes from a
    /// file the agent touched on disk (see `image_from_completed_tool`), never
    /// from clipboard/paste content.
    pub path: String,
    pub media_type: String,
    /// Natural pixel size, when known. `None` until a host-side decode fills
    /// it in; the inline thumbnail falls back to a placeholder height (see
    /// `crate::widgets::transcript::image` in the UI crate) until then.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The `agent-tools-code` file tools' stable wire names whose completed
/// calls can attribute an inline image (see `image_from_completed_tool`).
/// Duplicated from `presentation.rs`'s private constants of the same
/// values rather than shared, since that module's are not `pub(crate)` and
/// the two attribution paths (Sources rows vs. inline images) are allowed to
/// evolve independently.
const IMAGE_FILE_READ_TOOL_NAME: &str = "FileRead";
const IMAGE_FILE_WRITE_TOOL_NAME: &str = "FileWrite";
const IMAGE_FILE_EDIT_TOOL_NAME: &str = "FileEdit";

/// Extensions recognized as inline-previewable images (case-insensitive).
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg"];

fn media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Pulls the `key=value` suffix `safe_tool_summary`
/// (`zode_app_runtime::engine_backend`) formats a file tool's summary with,
/// e.g. `"FileRead path=/repo/logo.png"`. Mirrors `tool_summary_value` in
/// `presentation.rs`, which extracts the same shape for Sources rows.
fn tool_summary_path(tool: &ToolCall) -> Option<&str> {
    tool.summary
        .strip_prefix(tool.name.as_str())
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.split_once('='))
        .map(|(_, value)| value)
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
    Image,
    GoalProgress,
    SubagentChip,
    Approval,
    Status,
    Error,
}

/// Local, UI-only reaction on one assistant message. Mutually exclusive:
/// setting `Up` after `Down` (or vice versa) replaces the prior value, and
/// clicking the active choice again clears it back to `None`. Not wired to
/// any backend signal today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageFeedback {
    #[default]
    None,
    Up,
    Down,
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
    UserText {
        text: String,
        /// Wall-clock send time in epoch milliseconds, set once at insertion
        /// time from the reducer's existing `now_ms` time source. `None` for
        /// messages restored from history, which the current history
        /// protocol does not persist a timestamp for (mirrors `TurnSummary`'s
        /// `Restored` status carrying no elapsed time).
        timestamp_ms: Option<i64>,
    },
    AssistantText {
        text: String,
        /// Wall-clock time the message started, epoch milliseconds. Same
        /// "unknown for restored history" caveat as `UserText::timestamp_ms`.
        timestamp_ms: Option<i64>,
        /// Local-only reaction toggle. See `MessageFeedback`.
        feedback: MessageFeedback,
    },
    Thinking(String),
    ActivityGroup(Vec<ActivityEntry>),
    Tool(ToolCall),
    FileArtifact(FileArtifact),
    Attachment(AttachmentMetadata),
    Image(ImageItem),
    GoalProgress(GoalProgress),
    /// One `Task` sub-agent lifecycle one-liner. See
    /// [`crate::SubagentChip`].
    SubagentChip(SubagentChip),
    Approval {
        id: String,
        tool: String,
    },
    Status {
        code: String,
        message: String,
    },
    Error {
        message: String,
        retryable: bool,
    },
}

impl TranscriptItem {
    /// Constructs a user message with no known send time (history restore).
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::UserText {
            text: text.into(),
            timestamp_ms: None,
        }
    }

    /// Constructs a user message stamped with its send time in epoch millis.
    pub fn user_text_at(text: impl Into<String>, timestamp_ms: i64) -> Self {
        Self::UserText {
            text: text.into(),
            timestamp_ms: Some(timestamp_ms),
        }
    }

    /// Constructs an assistant message with no known start time (history
    /// restore) and no reaction set yet.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::AssistantText {
            text: text.into(),
            timestamp_ms: None,
            feedback: MessageFeedback::None,
        }
    }

    /// Constructs an assistant message stamped with its start time in epoch
    /// milliseconds.
    pub fn assistant_text_at(text: impl Into<String>, timestamp_ms: i64) -> Self {
        Self::AssistantText {
            text: text.into(),
            timestamp_ms: Some(timestamp_ms),
            feedback: MessageFeedback::None,
        }
    }

    /// Attributes a completed `FileRead`/`FileWrite`/`FileEdit` call that
    /// touched a recognized image extension (see `IMAGE_EXTENSIONS`) into an
    /// inline `Image` item. Returns `None` for every other tool, a failed
    /// call, or a path this crate cannot recognize as an image - callers
    /// must never fabricate an image item for a non-image tool. The path is
    /// read back out of `tool.summary` (`"{name} path={value}"`, written by
    /// `zode_app_runtime::engine_backend::safe_tool_summary`) rather than
    /// plumbed through a new wire-protocol field, so this attributes purely
    /// from data the reducer already has for every tool call.
    pub fn image_from_completed_tool(tool: &ToolCall) -> Option<Self> {
        if tool.status != ToolStatus::Completed {
            return None;
        }
        if !matches!(
            tool.name.as_str(),
            IMAGE_FILE_READ_TOOL_NAME | IMAGE_FILE_WRITE_TOOL_NAME | IMAGE_FILE_EDIT_TOOL_NAME
        ) {
            return None;
        }
        let path = tool_summary_path(tool)?;
        let extension = path.rsplit('.').next()?.to_ascii_lowercase();
        if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            return None;
        }
        Some(Self::Image(ImageItem {
            id: format!("image:{}", tool.id),
            path: path.to_owned(),
            media_type: media_type_for_extension(&extension).to_owned(),
            width: None,
            height: None,
        }))
    }

    pub const fn visual_kind(&self) -> TranscriptVisualKind {
        match self {
            Self::UserText { .. } => TranscriptVisualKind::UserMarkdown,
            Self::AssistantText { .. } => TranscriptVisualKind::AssistantMarkdown,
            Self::Thinking(_) => TranscriptVisualKind::Thinking,
            Self::ActivityGroup(_) => TranscriptVisualKind::Activity,
            Self::Tool(_) => TranscriptVisualKind::Tool,
            Self::FileArtifact(_) => TranscriptVisualKind::FileArtifact,
            Self::Attachment(_) => TranscriptVisualKind::Attachment,
            Self::Image(_) => TranscriptVisualKind::Image,
            Self::GoalProgress(_) => TranscriptVisualKind::GoalProgress,
            Self::SubagentChip(_) => TranscriptVisualKind::SubagentChip,
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
            Self::Image(image) => format!("image:{}", image.id),
            Self::GoalProgress(goal) => format!("goal:{}", goal.id),
            Self::SubagentChip(chip) => {
                format!("subagent:{}:{}", chip.agent_id, chip.phase.key())
            }
            Self::Approval { id, .. } => format!("approval:{id}"),
            Self::UserText { .. } => format!("user:{index}"),
            Self::AssistantText { .. } => format!("assistant:{index}"),
            Self::Thinking(_) => format!("thinking:{index}"),
            Self::Status { code, .. } => format!("status:{index}:{code}"),
            Self::Error { .. } => format!("error:{index}"),
        }
    }
}

/// Memoized cumulative item layout, keyed by the inputs that can invalidate
/// it. Lives on `TranscriptState` (behind a `RefCell`) so the paint path and
/// the accessibility-tree builder share one computation within a frame, and
/// reuse it across frames whenever nothing relevant to this transcript
/// changed (e.g. a sidebar-animation or resize-settle tick). Plain tuples
/// rather than a UI-crate geometry type, since this crate has no rendering
/// dependency.
///
/// Validity is checked with two complementary signals (see `layout_offsets`):
/// `revision` catches content changes that a fixture might make through a
/// `TranscriptState` method, while `items_len`/`item_heights_snapshot` are a
/// safety net that also catches direct field mutation - `items` and
/// `item_heights` are `pub` (existing test fixtures build `TranscriptState`
/// literals and poke at them directly), so nothing guarantees every mutation
/// path bumps `revision`. Comparing the small `item_heights` vector is far
/// cheaper than the measurement work it lets us skip, and is exact rather
/// than best-effort.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLayoutCache {
    pub revision: u64,
    pub viewport_width_bits: u32,
    items_len: usize,
    item_heights_snapshot: Vec<f32>,
    /// (top, bottom) per item, in item order.
    pub offsets: Vec<(f32, f32)>,
    pub total_height: f32,
}

/// Ordered transcript state and its event-stream cursor.
#[derive(Debug, Clone)]
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
    /// Bumped by `touch_layout` whenever `items`/`item_heights` change in a
    /// way that can affect cumulative layout (including a tool's
    /// expanded/collapsed override, since that changes how an unmeasured
    /// item's height is estimated). `layout_offsets` compares this instead
    /// of diffing the vectors so a cache hit is O(1). `pub` only so `..`
    /// struct-update syntax keeps working from the many existing test
    /// fixtures that build a `TranscriptState` literal; prefer `touch_layout`
    /// over writing this field directly.
    pub revision: u64,
    /// Memoized layout for the current `revision`/viewport width. See
    /// `layout_offsets`. `pub` for the same struct-update-syntax reason as
    /// `revision` above.
    pub layout_cache: RefCell<Option<TranscriptLayoutCache>>,
    /// Diagnostic-only: counts genuine recomputes (cache misses) in
    /// `layout_offsets`. Negligible cost, always compiled in - both the
    /// model crate's own tests and the UI crate's tests (a separate
    /// compilation unit, where `#[cfg(test)]` items on this type would not
    /// exist) assert on it to verify cache reuse.
    pub layout_recompute_count: Cell<u64>,
    /// Memoized conversation-anchor-rail derivation (one entry per turn).
    /// `pub` for the same struct-update-syntax reason as `layout_cache`
    /// above - existing test fixtures across the workspace build
    /// `TranscriptState` literals with `..TranscriptState::default()`, which
    /// requires every field (even ones no fixture sets explicitly) to be
    /// visible. See `anchors()`.
    pub anchor_cache: RefCell<Option<anchors::AnchorCache>>,
}

/// Two transcripts are equal iff their observable content is equal.
/// `revision`, `layout_cache` and `layout_recompute_count` are derived
/// bookkeeping - excluding them keeps equality identical to the plain
/// `#[derive(PartialEq)]` this type used before the layout cache existed.
impl PartialEq for TranscriptState {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
            && self.turns == other.turns
            && self.last_sequence == other.last_sequence
            && self.busy == other.busy
            && self.scroll_offset == other.scroll_offset
            && self.follow_tail == other.follow_tail
            && self.item_heights == other.item_heights
    }
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
            revision: 0,
            layout_cache: RefCell::new(None),
            layout_recompute_count: Cell::new(0),
            anchor_cache: RefCell::new(None),
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
        self.touch_layout();
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
        self.touch_layout();
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
            if !matches!(self.items[index], TranscriptItem::UserText { .. }) {
                index += 1;
                continue;
            }
            let start = index;
            while index < self.items.len()
                && matches!(self.items[index], TranscriptItem::UserText { .. })
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
        self.touch_layout();
        true
    }

    /// Bumps the layout revision. Every mutation of `items` or `item_heights`,
    /// or of anything else that changes what `layout_offsets` would compute
    /// (such as a tool's expanded/collapsed override), must call this so
    /// cached layout gets recomputed instead of served stale.
    pub fn touch_layout(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Returns the cumulative item layout (top/bottom offsets in item order,
    /// plus total content height) for the given viewport width, recomputing
    /// it via `compute` only when something that affects the result changed
    /// since the last call. Paint and the accessibility-tree builder call
    /// this every frame with the same viewport width, so within one frame
    /// the second caller always hits the cache the first caller just
    /// populated; across frames with no relevant change (e.g. a
    /// sidebar-animation tick) every caller hits it.
    ///
    /// Validity requires `revision`, the viewport width, `items.len()` and
    /// the full `item_heights` vector to all still match what the cache was
    /// built from - see the doc comment on `TranscriptLayoutCache` for why
    /// both a revision counter and a direct comparison are needed. The
    /// `item_heights` comparison is a cheap numeric scan, far less work than
    /// the measurement it lets us skip, so paying it on every call (even a
    /// hit) is still a large net win.
    ///
    /// `compute` takes no arguments; the caller closes over `self` plus
    /// whatever rendering-only inputs (like tool-expanded overrides) it
    /// needs, because this crate has no rendering dependency and cannot
    /// perform the actual measurement.
    pub fn layout_offsets(
        &self,
        viewport_width: f32,
        compute: impl FnOnce() -> (Vec<(f32, f32)>, f32),
    ) -> Ref<'_, TranscriptLayoutCache> {
        let viewport_width_bits = viewport_width.to_bits();
        let hit = self.layout_cache.borrow().as_ref().is_some_and(|cache| {
            cache.revision == self.revision
                && cache.viewport_width_bits == viewport_width_bits
                && cache.items_len == self.items.len()
                && cache.item_heights_snapshot == self.item_heights
        });
        if !hit {
            let (offsets, total_height) = compute();
            self.layout_recompute_count
                .set(self.layout_recompute_count.get() + 1);
            *self.layout_cache.borrow_mut() = Some(TranscriptLayoutCache {
                revision: self.revision,
                viewport_width_bits,
                items_len: self.items.len(),
                item_heights_snapshot: self.item_heights.clone(),
                offsets,
                total_height,
            });
        }
        Ref::map(self.layout_cache.borrow(), |cache| {
            cache.as_ref().expect("populated above")
        })
    }

    /// Diagnostic-only: how many times `layout_offsets` has actually
    /// recomputed (as opposed to reusing the cache) since this transcript
    /// was created. Tests assert on this to verify cache reuse across calls
    /// and frames; production code never reads it.
    pub fn layout_recompute_count(&self) -> u64 {
        self.layout_recompute_count.get()
    }

    /// Returns one `ConversationAnchor` per turn (see `anchors::derive_anchors`),
    /// recomputing only on the same cache-miss signals as `layout_offsets` -
    /// the design deliberately rides that existing invalidation instead of a
    /// second revision counter. `offsets` must be the same prefix-sum vector
    /// `layout_offsets` produced for `viewport_width`; callers always fetch
    /// that first (see `TranscriptState::layout_offsets`) and pass its
    /// `.offsets` straight through.
    pub fn anchors(
        &self,
        viewport_width: f32,
        offsets: &[(f32, f32)],
    ) -> Ref<'_, Vec<ConversationAnchor>> {
        let viewport_width_bits = viewport_width.to_bits();
        let hit = self
            .anchor_cache
            .borrow()
            .as_ref()
            .is_some_and(|cache| cache.matches(self, viewport_width_bits));
        if !hit {
            let anchors = anchors::derive_anchors(self, offsets);
            *self.anchor_cache.borrow_mut() = Some(anchors::AnchorCache {
                revision: self.revision,
                viewport_width_bits,
                items_len: self.items.len(),
                item_heights_snapshot: self.item_heights.clone(),
                anchors,
            });
        }
        Ref::map(self.anchor_cache.borrow(), |cache| {
            &cache.as_ref().expect("populated above").anchors
        })
    }
}

fn remap_boundary(boundary: usize, removed: &[usize]) -> usize {
    boundary.saturating_sub(removed.partition_point(|index| *index < boundary))
}

#[cfg(test)]
#[path = "transcript/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "transcript/approval-tests.rs"]
mod approval_tests;
