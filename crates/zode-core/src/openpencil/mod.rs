//! OpenPencil control surface (`op-bridge`): drive a live OpenPencil instance
//! from zode. `sh` for lifecycle (locate/install/launch), `http` for ops.

pub mod client;
pub mod connection;
pub mod design;
pub mod install;
pub mod launcher;
pub mod locate;
pub mod tools;

use thiserror::Error;

/// User consent for a lifecycle action (install / launch). The prompt SHOULD
/// include the exact command. Returns true to proceed.
#[async_trait::async_trait]
pub trait Consent: Send + Sync + std::fmt::Debug {
    async fn confirm(&self, prompt: &str) -> bool;
}

/// Read-only OpenPencil MCP tools. A curated allowlist (the prefix heuristic
/// alone misclassifies `read_nodes`/`batch_get`/`export_design_md`/
/// `search_all_unique_properties`). Anything not read is routed to `op_write`
/// (gated). TODO: derive from `tools/list` metadata when OpenPencil exposes it.
pub fn is_read_tool(name: &str) -> bool {
    const READ_TOOLS: &[&str] = &[
        "get_document_info",
        "get_selection",
        "get_node",
        "get_node_children",
        "get_node_parent",
        "list_pages",
        "list_variables",
        "list_components",
        "list_node_kinds",
        "get_active_theme",
        "get_component",
        "snapshot_layout",
        "get_canvas_bounds",
        "find_node_by_name",
        "count_nodes",
        "get_history_depth",
        "get_viewport",
        "get_selection_set",
        "read_nodes",
        "batch_get",
        "export_design_md",
        "search_all_unique_properties",
    ];
    READ_TOOLS.contains(&name)
        || [
            "get_",
            "list_",
            "snapshot_",
            "count_",
            "find_",
            "read_",
            "export_",
            "search_",
        ]
        .iter()
        .any(|p| name.starts_with(p))
}

/// Failures across the op-bridge.
#[derive(Debug, Error)]
pub enum OpError {
    #[error("the `op` CLI is not installed")]
    NotInstalled,
    #[error("install declined by user")]
    InstallDeclined,
    #[error("install failed: {0}")]
    Install(String),
    #[error("no live OpenPencil instance and none could be launched: {0}")]
    NoInstance(String),
    #[error("launch declined by user")]
    LaunchDeclined,
    #[error("http error: {0}")]
    Http(String),
    #[error("OpenPencil returned an error: {0}")]
    Rpc(String),
    #[error("could not parse response: {0}")]
    Parse(String),
}
