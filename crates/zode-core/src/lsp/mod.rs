//! Built-in LSP plugin: spawns language servers and surfaces their analysis as
//! the `lsp_*` tools (diagnostics, hover, definition, references, symbols,
//! rename, format). Servers come from auto-detection (any known server on
//! `PATH`) unioned with the user's `lsp.servers` config; each language is a
//! toggleable plugin (`lsp:<lang>`) in the `/plugin` picker.

mod client;
mod manager;
mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent::tool::Tool;

use crate::config::LspServerConfig;

pub use manager::LspManager;

/// Build the `lsp_*` tools over a manager.
pub fn lsp_tools(mgr: &Arc<LspManager>) -> Vec<Arc<dyn Tool>> {
    tools::all_tools(mgr)
}

/// Known language servers, probed on `PATH`. Tuple: (LSP languageId, command,
/// args, file extensions). The languageId doubles as the plugin key (`lsp:rust`).
const KNOWN_SERVERS: &[(&str, &str, &[&str], &[&str])] = &[
    ("rust", "rust-analyzer", &[], &["rs"]),
    ("go", "gopls", &[], &["go"]),
    (
        "cpp",
        "clangd",
        &[],
        &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx"],
    ),
    ("python", "pyright-langserver", &["--stdio"], &["py", "pyi"]),
    (
        "typescript",
        "typescript-language-server",
        &["--stdio"],
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
    ),
    ("zig", "zls", &[], &["zig"]),
];

/// Auto-detect language servers present on `PATH`. Returns config entries for
/// each one found, so the `/plugin` picker can list them without the user
/// hand-writing `lsp.servers`.
pub fn detect_default_servers() -> HashMap<String, LspServerConfig> {
    let mut out = HashMap::new();
    for (lang, cmd, args, exts) in KNOWN_SERVERS {
        if on_path(cmd) {
            out.insert(
                (*lang).to_string(),
                LspServerConfig {
                    command: (*cmd).to_string(),
                    args: args.iter().map(|s| (*s).to_string()).collect(),
                    extensions: exts.iter().map(|s| (*s).to_string()).collect(),
                },
            );
        }
    }
    out
}

/// Effective servers: auto-detected ∪ user config, with the user's entry
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

/// Whether `cmd` resolves to a file on `PATH`.
fn on_path(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let full: PathBuf = dir.join(cmd);
        full.is_file()
    })
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

    #[test]
    fn on_path_finds_a_ubiquitous_binary() {
        // `sh` is on PATH on every unix; a nonsense name is not.
        assert!(on_path("sh"));
        assert!(!on_path("definitely-not-a-real-binary-zzqq"));
    }
}
