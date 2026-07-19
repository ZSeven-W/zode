pub mod browser;
pub mod builtin;
pub mod desktop;
#[path = "loop-sched.rs"]
pub mod loop_sched;
pub mod op;
pub mod registry;
pub mod team;

pub use registry::{parse_slash, CommandAction, CommandRegistry, SlashCommand};
