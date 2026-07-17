#![forbid(unsafe_code)]

mod command;
mod layout;
mod message_queue;
mod presentation;
mod reducer;
mod state;
mod transcript;

pub use command::AppCommand;
pub use layout::LayoutClass;
pub use message_queue::{MessageQueueState, QueuedMessage, QueuedMessageId};
pub use presentation::*;
pub use reducer::{
    apply_session_runtime_options, reduce_agent_event, reduce_navigation_command,
    reduce_presentation_command, reduce_queue_command, reduce_settings_command,
    reduce_terminal_command, reduce_tool_command, reduce_transcript_command, NavigationOutcome,
    PresentationCommandOutcome, QueueCommandOutcome, ReduceOutcome, SettingsCommandOutcome,
    TerminalCommandOutcome, ToolCommandOutcome, TranscriptCommandOutcome,
};
pub use state::*;
pub use transcript::{
    default_tool_expanded, tool_category, ActivityEntry, AttachmentMetadata, FileArtifact,
    GoalProgress, ToolCategory, TranscriptItem, TranscriptState, TranscriptVisualKind,
};

pub const CRATE_READY: bool = true;
