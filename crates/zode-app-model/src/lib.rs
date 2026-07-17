#![forbid(unsafe_code)]

mod command;
#[path = "external-application.rs"]
mod external_application;
mod layout;
mod lightbox;
mod message_queue;
#[path = "open-with-navigation.rs"]
mod open_with_navigation;
mod presentation;
mod reducer;
#[path = "session-navigation.rs"]
mod session_navigation;
#[path = "settings-reducer.rs"]
mod settings_reducer;
mod sidebar_navigation;
mod state;
mod task_navigation;
mod transcript;

pub use command::{AppCommand, QueueReorderOp};
pub use external_application::{
    ExternalApplication, ExternalApplicationCatalog, ExternalApplicationIcon, OpenWithState,
};
pub use layout::LayoutClass;
pub use lightbox::{LightboxState, LIGHTBOX_DEFAULT_ZOOM_INDEX, LIGHTBOX_ZOOM_STEPS};
pub use message_queue::{MessageQueueState, QueuedMessage, QueuedMessageId};
pub use presentation::*;
pub use reducer::{
    apply_session_runtime_options, reduce_agent_event, reduce_agent_event_at,
    reduce_navigation_command, reduce_presentation_command, reduce_queue_command,
    reduce_terminal_command, reduce_tool_command, reduce_transcript_command, NavigationOutcome,
    PresentationCommandOutcome, QueueCommandOutcome, ReduceOutcome, TerminalCommandOutcome,
    ToolCommandOutcome, TranscriptCommandOutcome,
};
pub use settings_reducer::{reduce_settings_command, SettingsCommandOutcome};
pub use state::*;
pub use transcript::{
    default_tool_expanded, tool_category, ActivityEntry, AttachmentMetadata, ConversationAnchor,
    FileArtifact, GoalProgress, ImageItem, MessageFeedback, ToolCategory, TranscriptItem,
    TranscriptState, TranscriptTurnStatus, TranscriptVisualKind, TurnSummary,
};

pub const CRATE_READY: bool = true;
