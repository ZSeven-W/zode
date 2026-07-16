//! zode-core — UI-agnostic product logic for the Zode CLI.
//!
//! Wraps the vendored `agent` runtime: config loading, provider
//! construction, the query engine assembly, product tools, commands,
//! and context/skills/MCP wiring. The TUI (`zode-tui`) and the binary
//! (`zode`) depend on this crate; this crate never depends on them.

pub mod agents;
pub mod approval;
pub mod bg_shells;
pub mod browser;
pub mod clipboard;
pub mod commands;
pub mod compact_memory;
pub mod config;
pub mod cost;
pub mod currency;
pub mod diff;
pub mod engine;
pub mod error;
pub mod export;
pub mod fs_escalate;
pub mod gated_tool;
pub mod git_stat;
pub mod goal;
pub mod history;
pub mod hooks_config;
pub mod i18n;
pub mod i18n_data;
pub mod images;
pub mod instructions;
pub mod lsp;
pub mod mcp;
pub mod models_dev;
pub mod noema;
pub mod noema_extract;
pub mod openpencil;
pub mod plugin;
pub mod portability;
pub mod provider;
pub mod question;
pub mod reminders;
pub mod sandbox;
pub mod session_meta;
pub mod skills;
pub mod subagents;
pub mod task_factory;
pub mod tool_trace;
pub mod tools;
pub mod updater;
pub mod user_commands;
pub mod verification;
pub mod workflows;
pub mod workflows_js;

pub use agent_tools_code::{TodoItem, TodoStatus};
pub use engine::{EngineTemplate, ToolAccessMode, ZodeEngine};
pub use error::CoreError;
pub use git_stat::GitFileStat;
pub use models_dev::{Catalog, CatalogModel, CatalogProvider};
pub use plugin::{Plugin, PluginKind, PluginManager};
pub use subagents::{SubAgent, SubAgentLine, SubAgentRegistry, SubAgentStatus};
