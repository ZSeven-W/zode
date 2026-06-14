//! zode-core — UI-agnostic product logic for the Zode CLI.
//!
//! Wraps the vendored `agent` runtime: config loading, provider
//! construction, the query engine assembly, product tools, commands,
//! and context/skills/MCP wiring. The TUI (`zode-tui`) and the binary
//! (`zode`) depend on this crate; this crate never depends on them.

pub mod approval;
pub mod bg_shells;
pub mod commands;
pub mod config;
pub mod cost;
pub mod engine;
pub mod error;
pub mod gated_tool;
pub mod history;
pub mod hooks_config;
pub mod instructions;
pub mod lsp;
pub mod mcp;
pub mod plugin;
pub mod provider;
pub mod sandbox;
pub mod session_meta;
pub mod skills;
pub mod task_factory;
pub mod tools;

pub use engine::{EngineTemplate, ZodeEngine};
pub use error::CoreError;
pub use plugin::{Plugin, PluginKind, PluginManager};
