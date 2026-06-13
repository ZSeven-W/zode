//! zode-core — UI-agnostic product logic for the Zode CLI.
//!
//! Wraps the vendored `agent` runtime: config loading, provider
//! construction, the query engine assembly, product tools, commands,
//! and context/skills/MCP wiring. The TUI (`zode-tui`) and the binary
//! (`zode`) depend on this crate; this crate never depends on them.

pub mod approval;
pub mod commands;
pub mod config;
pub mod engine;
pub mod error;
pub mod gated_tool;
pub mod history;
pub mod provider;
pub mod session_meta;
pub mod tools;

pub use engine::ZodeEngine;
pub use error::CoreError;
