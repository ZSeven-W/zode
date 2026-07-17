#![forbid(unsafe_code)]

mod error;
mod types;

pub use error::ProtocolError;
pub use types::*;

pub const CRATE_READY: bool = true;
