//! Runtime skins: the agent (or any plugin) writes a skin and installs
//! it while zode is running; frontends observe the skin service and re-render
//! on the next frame — no restart.
//!
//! A skin is the same JSON schema as '~/.zode/themes/*.json' (name,
//! description, 256-color palette indices, optional icons/spinner). The TUI
//! re-parses it through its theme loader; other frontends can render the raw
//! JSON however they like.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::error::CoreError;

/// Shared skin slot: current skin JSON + a version bumped on every install.
pub struct SkinState {
    current: RwLock<Option<String>>,
    version: AtomicU64,
}

impl SkinState {
    pub fn new() -> Arc<Self> {
        Arc::new(SkinState {
            current: RwLock::new(None),
            version: AtomicU64::new(0),
        })
    }

    /// Install a skin from its JSON text. Validated structurally here (must
    /// be a JSON object); full schema/color validation happens on the
    /// consumer side (the theme parser).
    pub fn install(&self, json: &str) -> Result<(), CoreError> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| CoreError::Other(format!("skin json: {error}")))?;
        if !value.is_object() {
            return Err(CoreError::Other("skin must be a JSON object".to_string()));
        }
        *self.current.write().unwrap() = Some(json.to_string());
        self.version.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// The installed skin JSON, if any.
    pub fn current(&self) -> Option<String> {
        self.current.read().unwrap().clone()
    }

    /// Install counter (frontends poll this to detect hot swaps).
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
}
