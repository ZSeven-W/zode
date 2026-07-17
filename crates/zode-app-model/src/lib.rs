#![forbid(unsafe_code)]

mod command;
mod layout;
mod reducer;
mod state;
mod transcript;

pub use command::AppCommand;
pub use layout::LayoutClass;
pub use reducer::{
    reduce_agent_event, reduce_navigation_command, reduce_settings_command, reduce_tool_command,
    reduce_transcript_command, NavigationOutcome, ReduceOutcome, SettingsCommandOutcome,
    ToolCommandOutcome, TranscriptCommandOutcome,
};
pub use state::*;
pub use transcript::{
    default_tool_expanded, tool_category, ToolCategory, TranscriptItem, TranscriptState,
};

pub const CRATE_READY: bool = true;
