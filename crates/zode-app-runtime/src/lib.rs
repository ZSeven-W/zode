#![forbid(unsafe_code)]

mod app_state_store;
mod bootstrap;
mod computer_status;
mod engine_backend;
mod event_sink;
pub mod git_status;
mod integrations;
mod local_endpoint;
mod node;
mod node_identity;
pub mod persistence;
mod plugin_market;
mod runtime_policy;
mod session_repository;
pub mod session_store;
mod zode_engine_driver;

pub use app_state_store::{
    AppStateFile, AppStateStore, SessionUiState, TaskContext, WindowGeometry,
};
pub use bootstrap::LocalAppRuntime;
pub use computer_status::{
    computer_permission_status, ComputerPermissionState, ComputerPermissionStatus,
};
pub use engine_backend::{
    persist_project_allow, DriverEventStream, EngineBackend, EngineDriver, EventNormalizer,
    PersistedApproval, SubagentModels,
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
