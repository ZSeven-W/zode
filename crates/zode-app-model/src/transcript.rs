use zode_node_protocol::ToolCall;

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
