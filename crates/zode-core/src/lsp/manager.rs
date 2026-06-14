//! Routes files to language servers. Owns one [`LspClient`] per language,
//! spawned lazily on first use (so configuring rust-analyzer doesn't pay its
//! startup cost until an `lsp_*` tool actually touches a `.rs` file).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::{LspConfig, LspServerConfig};
use crate::lsp::client::LspClient;
use crate::lsp::install;

#[derive(Debug)]
pub struct LspManager {
    root: PathBuf,
    /// Enabled language → server config.
    servers: HashMap<String, LspServerConfig>,
    /// File extension (lowercased, no dot) → language key.
    ext_index: HashMap<String, String>,
    /// Lazily-spawned clients, keyed by language.
    clients: Mutex<HashMap<String, Arc<LspClient>>>,
}

impl LspManager {
    pub fn new(cfg: LspConfig, root: PathBuf) -> Self {
        let mut ext_index = HashMap::new();
        for (lang, sc) in &cfg.servers {
            for ext in &sc.extensions {
                let key = ext.trim_start_matches('.').to_ascii_lowercase();
                ext_index.insert(key, lang.clone());
            }
        }
        Self {
            root,
            servers: cfg.servers,
            ext_index,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve a possibly-relative file argument against the workspace root.
    pub fn resolve(&self, file: &str) -> PathBuf {
        let p = Path::new(file);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn lang_for(&self, path: &Path) -> Option<String> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.ext_index.get(&ext).cloned()
    }

    /// Get (spawning if needed) the client that handles `path`. Errors when no
    /// configured+enabled server claims the file's extension, or the server
    /// fails to start.
    pub async fn client_for(&self, path: &Path) -> Result<Arc<LspClient>, String> {
        let lang = self.lang_for(path).ok_or_else(|| {
            format!(
                "no language server configured for {} (enable one via /plugin)",
                path.display()
            )
        })?;
        // Held across spawn so two concurrent calls don't start the server
        // twice; LSP calls aren't hot, so the brief serialization is fine.
        let mut clients = self.clients.lock().await;
        if let Some(c) = clients.get(&lang) {
            return Ok(c.clone());
        }
        let cfg = self
            .servers
            .get(&lang)
            .ok_or_else(|| format!("server config for {lang} missing"))?
            .clone();
        // For a built-in language, resolve the runnable command — installing it
        // on demand into ~/.zode/lsp if needed (blocking, hence spawn_blocking).
        // User-defined languages use their configured command as-is.
        let resolved = match install::spec_for_lang(&lang) {
            Some(spec) => {
                let path = tokio::task::spawn_blocking(move || install::ensure(spec))
                    .await
                    .map_err(|e| format!("install task failed: {e}"))??;
                LspServerConfig {
                    command: path.to_string_lossy().into_owned(),
                    ..cfg
                }
            }
            None => cfg,
        };
        let client = Arc::new(LspClient::start(lang.clone(), &resolved, self.root.clone()).await?);
        clients.insert(lang, client.clone());
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LspConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "rust".to_string(),
            LspServerConfig {
                command: "rust-analyzer".into(),
                args: vec![],
                extensions: vec!["rs".into()],
            },
        );
        LspConfig { servers }
    }

    #[test]
    fn routes_extension_to_language() {
        let m = LspManager::new(cfg(), PathBuf::from("/proj"));
        assert_eq!(
            m.lang_for(Path::new("/proj/src/main.rs")).as_deref(),
            Some("rust")
        );
        assert_eq!(m.lang_for(Path::new("/proj/README.md")), None);
    }

    #[test]
    fn resolve_joins_relative_to_root() {
        let m = LspManager::new(cfg(), PathBuf::from("/proj"));
        assert_eq!(m.resolve("src/main.rs"), PathBuf::from("/proj/src/main.rs"));
        assert_eq!(m.resolve("/abs/x.rs"), PathBuf::from("/abs/x.rs"));
    }
}
