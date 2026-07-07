mod client;
mod protocol;

pub use client::{ClientOptions, SdkError, StdioZodeClient, ZodeClient};
pub use protocol::ProtocolMethod;

#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
