#![forbid(unsafe_code)]

mod persistence;
pub mod session_store;
mod app_state_store;
mod bootstrap;
mod engine_backend;
mod event_sink;
mod integrations;
mod local_endpoint;
mod node;
mod node_identity;
mod runtime_policy;
mod session_repository;
mod zode_engine_driver;

pub use app_state_store::{
    AppStateFile, AppStateStore, SessionUiState, TaskContext, WindowGeometry,
};
pub use bootstrap::LocalAppRuntime;
pub use engine_backend::{
    persist_project_allow, DriverEventStream, EngineBackend, EngineDriver, EventNormalizer,
    PersistedApproval,
};
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
