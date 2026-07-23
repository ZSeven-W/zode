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

/// Read-only OpenPencil MCP tools. A small write override handles create-like
/// tools whose names otherwise match read prefixes; a curated read allowlist
/// covers current non-prefix reads. Anything not read is routed to `op_write`
/// (gated).
pub fn is_read_tool(name: &str) -> bool {
    const WRITE_TOOLS: &[&str] = &["export_nodes"];
    if WRITE_TOOLS.contains(&name) {
        return false;
    }

    const READ_TOOLS: &[&str] = &[
        "open_document",
        "get_document_info",
        "get_selection",
        "get_node",
        "get_node_children",
        "get_node_parent",
        "list_pages",
        "list_variables",
        "get_variables",
        "conversion_status",
        "lint_document",
        "list_theme_presets",
        "get_design_md",
        "export_design_md",
        "get_style_guide_tags",
        "get_style_guide",
        "get_guidelines",
        "ToolSearch",
        "get_screenshot",
        "get_active_theme",
        "list_components",
        "get_component",
        "snapshot_layout",
        "find_empty_space",
        "get_canvas_bounds",
        "find_node_by_name",
        "count_nodes",
        "list_node_kinds",
        "get_history_depth",
        "get_viewport",
        "get_selection_set",
        "get_editor_state",
        "read_nodes",
        "batch_get",
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
    #[error("operation aborted: {0}")]
    Aborted(String),
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
