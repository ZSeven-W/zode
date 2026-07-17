#![forbid(unsafe_code)]

mod command;
mod layout;
mod reducer;
mod state;
mod transcript;

pub use command::AppCommand;
pub use layout::LayoutClass;
pub use reducer::{reduce_agent_event, ReduceOutcome};
pub use state::*;
pub use transcript::{TranscriptItem, TranscriptState};

pub const CRATE_READY: bool = true;
