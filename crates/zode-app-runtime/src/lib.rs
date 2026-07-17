#![forbid(unsafe_code)]

mod app_state_store;
mod bootstrap;
mod engine_backend;
mod event_sink;
mod local_endpoint;
mod node;
mod node_identity;
mod session_repository;
mod zode_engine_driver;

pub use app_state_store::{AppStateFile, AppStateStore, SessionUiState};
pub use bootstrap::LocalAppRuntime;
pub use engine_backend::{DriverEventStream, EngineBackend, EngineDriver, EventNormalizer};
pub use event_sink::EventSink;
pub use local_endpoint::LocalAgentEndpoint;
pub use node::NodeBackend;
pub use node_identity::NodeIdentityStore;
pub use session_repository::{
    path_to_workspace_uri, workspace_uri_to_path, LoadedSession, LocalSessionRepository,
};
pub use zode_engine_driver::{
    SessionEngine, SessionEngineFactory, SessionEngineSnapshot, ZodeEngineDriver,
    ZodeSessionEngineFactory,
};

pub const CRATE_READY: bool = true;
