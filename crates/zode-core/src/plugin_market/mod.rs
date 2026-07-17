//! Plugin marketplace: git-based plugin install/uninstall, capability
//! detection (skills / MCP servers / hooks), and the trust-review data layer
//! for capabilities that would execute code (hooks, MCP launch commands).
//! See `docs/proposals/plugin-marketplace.md` for the approved design.
//!
//! Named `plugin_market`, not `plugins`, to stay clearly distinct from
//! [`crate::plugin`] — that module is the built-in tool-group enable/disable
//! model (`PluginManager`, ids like `tools:git` / `mcp:foo` / `skill:review`),
//! unrelated to installing third-party git repos. The two do connect at one
//! point: once a plugin here is cloned into `~/.zode/plugins/<id>/`, its
//! skills and MCP servers are picked up automatically by the EXISTING
//! discovery in `crate::skills::skills_dirs` and
//! `crate::mcp::discover_mcp_config` (both already walk `~/.zode/plugins/*`
//! via `collect_plugin_skill_dirs` / `collect_plugin_mcp`), and enabling or
//! disabling an individual skill/server still flows through
//! `crate::plugin::PluginManager` under its usual `skill:<name>` /
//! `mcp:<server>` id. No parallel enable mechanism is introduced here.
//!
//! This wave (M1 + the M2 trust data layer) is `zode-core` only: installer,
//! manifest, capability scan, and trust store. The install/uninstall UI, the
//! plugin detail page, and the trust-review dialog are later, UI-crate waves.

pub mod installer;
pub mod manifest;
pub mod scan;
pub mod trust;

use thiserror::Error;

/// Failures across install / uninstall / scan / trust.
#[derive(Debug, Error)]
pub enum PluginMarketError {
    #[error("invalid plugin spec: {0}")]
    InvalidSpec(String),
    #[error("unsafe path in plugin spec: {0}")]
    UnsafePath(String),
    #[error("git is not installed (or not on PATH)")]
    GitMissing,
    #[error("git operation timed out")]
    Timeout,
    #[error("repository not found: {0}")]
    NotFound(String),
    #[error("authentication required: {0}")]
    AuthRequired(String),
    #[error("git failed: {0}")]
    GitFailed(String),
    #[error("plugin already installed: {0}")]
    AlreadyInstalled(String),
    #[error("plugin not installed: {0}")]
    NotInstalled(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest: {0}")]
    Manifest(String),
}
