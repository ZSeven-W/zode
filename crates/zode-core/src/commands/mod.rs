pub mod builtin;
pub mod op;
pub mod registry;

pub use registry::{parse_slash, CommandAction, CommandRegistry, SlashCommand};
