//! Built-in LSP plugin: spawns language servers and surfaces their analysis as
//! the `lsp_*` tools (diagnostics, hover, definition, references, symbols,
//! rename, format). Servers come from auto-detection (any known server already
//! on `PATH`) plus on-demand installation into `~/.zode/lsp` (npm / rustup /
//! go) for the ones with an installer — so mainstream languages are available
//! without the user hand-installing each server. Every language is a toggleable
//! plugin (`lsp:<lang>`) in the `/plugin` picker.

mod client;
pub mod install;
mod manager;
mod tools;

use std::collections::HashMap;
use std::sync::Arc;

use agent::tool::Tool;

use crate::config::LspServerConfig;

pub use manager::LspManager;

/// Build the `lsp_*` tools over a manager.
pub fn lsp_tools(mgr: &Arc<LspManager>) -> Vec<Arc<dyn Tool>> {
    tools::all_tools(mgr)
}

/// Every language server we offer the user: already on `PATH`, or installable
/// on demand (npm/rustup/go available). The `/plugin` picker lists these so a
/// language shows up even before its server is installed; the manager installs
/// it on first use.
pub fn detect_default_servers() -> HashMap<String, LspServerConfig> {
    let mut out = HashMap::new();
    for spec in install::SERVERS {
        if install::offered(spec) {
            out.insert(
                spec.lang.to_string(),
                LspServerConfig {
                    command: spec.command.to_string(),
                    args: spec.args.iter().map(|s| (*s).to_string()).collect(),
                    extensions: spec.extensions.iter().map(|s| (*s).to_string()).collect(),
                },
            );
        }
    }
    out
}

/// Effective servers: offered defaults ∪ user config, with the user's entry
/// winning on a key collision (so they can override command/args/extensions).
pub fn effective_servers(
    user: &HashMap<String, LspServerConfig>,
) -> HashMap<String, LspServerConfig> {
    let mut servers = detect_default_servers();
    for (lang, sc) in user {
        servers.insert(lang.clone(), sc.clone());
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_servers_lets_user_override() {
        let mut user = HashMap::new();
        user.insert(
            "rust".to_string(),
            LspServerConfig {
                command: "my-ra".into(),
                args: vec!["--custom".into()],
                extensions: vec!["rs".into()],
            },
        );
        let eff = effective_servers(&user);
        // The user's rust entry wins regardless of what's auto-detected.
        assert_eq!(eff.get("rust").unwrap().command, "my-ra");
    }
}
