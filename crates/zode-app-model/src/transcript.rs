use zode_node_protocol::ToolCall;

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
    Tool(ToolCall),
    Approval { id: String, tool: String },
    Status { code: String, message: String },
    Error { message: String, retryable: bool },
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
