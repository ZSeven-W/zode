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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptState {
    pub items: Vec<TranscriptItem>,
    pub last_sequence: u64,
    pub busy: bool,
}
