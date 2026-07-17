use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use zode_core::{
    config::ConfigManager,
    persistence::{write_atomic, AdvisoryFileLock},
    CoreError,
};

const APP_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiState {
    pub pinned: bool,
    pub unread: bool,
    pub failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateFile {
    pub version: u32,
    pub last_session: Option<String>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionUiState>,
    #[serde(default)]
    pub collapsed_workspaces: BTreeSet<String>,
}

impl Default for AppStateFile {
    fn default() -> Self {
        Self {
            version: APP_STATE_VERSION,
            last_session: None,
            sessions: BTreeMap::new(),
            collapsed_workspaces: BTreeSet::new(),
        }
    }
}

impl AppStateFile {
    pub fn reconcile<I, S>(&mut self, existing: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let existing: BTreeSet<String> = existing
            .into_iter()
            .map(|id| id.as_ref().to_owned())
            .collect();
        self.sessions.retain(|id, _| existing.contains(id));
        if self
            .last_session
            .as_ref()
            .is_some_and(|id| !existing.contains(id))
        {
            self.last_session = None;
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppStateStore {
    path: PathBuf,
}

impl AppStateStore {
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            path: config_dir.as_ref().join("app-state.json"),
        }
    }

    pub fn from_default_config() -> Result<Self, CoreError> {
        Ok(Self::new(ConfigManager::config_dir()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AppStateFile, CoreError> {
        let _lock = AdvisoryFileLock::acquire(&self.path)?;
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppStateFile::default());
            }
            Err(error) => return Err(CoreError::Io(error)),
        };
        let state: AppStateFile = serde_json::from_slice(&bytes)?;
        if state.version != APP_STATE_VERSION {
            return Err(CoreError::Other(format!(
                "unsupported app state version: {}",
                state.version
            )));
        }
        Ok(state)
    }

    pub fn save(&self, state: &AppStateFile) -> Result<(), CoreError> {
        if state.version != APP_STATE_VERSION {
            return Err(CoreError::Other(format!(
                "unsupported app state version: {}",
                state.version
            )));
        }
        let _lock = AdvisoryFileLock::acquire(&self.path)?;
        let bytes = serde_json::to_vec_pretty(state)?;
        write_atomic(&self.path, &bytes)
    }
}
