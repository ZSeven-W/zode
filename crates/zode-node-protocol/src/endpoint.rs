use crate::{
    AgentCommand, AgentEvent, CapabilityManifest, DiffSnapshot, EndpointError,
    InstalledPluginSummary, IntegrationRegistrySnapshot, PluginTrustReview, PluginUpdateCheck,
    RuntimeOptions, SessionLocator, ThreadHistory, ThreadSummary, WorkspaceUri,
};
use async_trait::async_trait;
use futures_core::Stream;
use std::pin::Pin;

pub type AgentEventStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, EndpointError>> + Send>>;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentQuery {
    Capabilities,
    Threads,
    History {
        session: SessionLocator,
    },
    Diff {
        session: SessionLocator,
    },
    RuntimeOptions,
    SessionRuntimeOptions {
        session: SessionLocator,
    },
    ProjectPermissions {
        workspace_uri: WorkspaceUri,
    },
    Integrations {
        workspace_uri: WorkspaceUri,
    },
    InstalledPlugins,
    PluginTrustReview {
        plugin_id: String,
    },
    /// Fetches the plugin's pinned ref and compares it with the local
    /// checkout. A query rather than a command because it only reads: the
    /// worktree is untouched until `AgentCommandKind::ApplyPluginUpdate`.
    PluginUpdateCheck {
        plugin_id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentSnapshot {
    Capabilities(CapabilityManifest),
    Threads(Vec<ThreadSummary>),
    History(ThreadHistory),
    Diff(DiffSnapshot),
    RuntimeOptions(RuntimeOptions),
    SessionRuntimeOptions {
        session: SessionLocator,
        options: RuntimeOptions,
    },
    ProjectPermissions(Vec<String>),
    Integrations(IntegrationRegistrySnapshot),
    InstalledPlugins(Vec<InstalledPluginSummary>),
    PluginTrustReview(PluginTrustReview),
    PluginUpdateCheck(PluginUpdateCheck),
}

#[async_trait]
pub trait AgentEndpoint: Send + Sync {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError>;
    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError>;
    async fn subscribe(&self) -> Result<AgentEventStream, EndpointError>;
}
