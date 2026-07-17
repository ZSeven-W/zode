pub mod browser;
pub mod builtin;
pub mod desktop;
pub mod op;
pub mod registry;
pub mod team;

pub use registry::{parse_slash, CommandAction, CommandRegistry, SlashCommand};
