use zode_node_protocol::ToolCall;

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
            last_sequence: 0,
            busy: false,
            scroll_offset: 0.0,
            follow_tail: true,
            item_heights: Vec::new(),
        }
    }
}

impl TranscriptState {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        ActivityEntry, AttachmentMetadata, FileArtifact, GoalProgress, TranscriptItem,
        TranscriptVisualKind,
    };
    use zode_node_protocol::{ToolCall, ToolStatus};

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
}
