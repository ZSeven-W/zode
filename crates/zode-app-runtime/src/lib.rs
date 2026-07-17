#![forbid(unsafe_code)]

mod event_sink;
mod engine_backend;
mod local_endpoint;
mod node;
mod node_identity;

pub use event_sink::EventSink;
pub use engine_backend::EventNormalizer;
pub use local_endpoint::LocalAgentEndpoint;
pub use node::NodeBackend;
pub use node_identity::NodeIdentityStore;

pub const CRATE_READY: bool = true;
