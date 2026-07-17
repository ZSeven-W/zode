use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize};
use zode_app_model::{
    ProjectDisplayMode, ProjectSortMode, ThemePreference, UiPreferences,
    DEFAULT_PRIMARY_SIDEBAR_WIDTH, DEFAULT_SECONDARY_SIDEBAR_WIDTH,
};
use zode_core::{
    config::ConfigManager,
    persistence::{write_atomic, AdvisoryFileLock},
    CoreError,
};
use zode_node_protocol::WorkspaceUri;

const APP_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiState {
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    pub unread: bool,
    pub failed: bool,
}

/// Last known windowed bounds plus whether the native window was maximized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

/// Persisted scope for the new-task composer. `None` means no explicit choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskContext {
    Project {
        #[serde(rename = "workspaceUri")]
        workspace_uri: WorkspaceUri,
    },
    Projectless,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppStateFile {
    pub version: u32,
    pub last_session: Option<String>,
    pub sessions: BTreeMap<String, SessionUiState>,
    pub collapsed_workspaces: BTreeSet<String>,
    pub pinned_workspaces: BTreeSet<WorkspaceUri>,
    pub project_display_mode: ProjectDisplayMode,
    pub project_sort_mode: ProjectSortMode,
    #[serde(deserialize_with = "deserialize_ui_preferences")]
    pub ui_preferences: UiPreferences,
    pub window_geometry: Option<WindowGeometry>,
    #[serde(default)]
    pub task_context: Option<TaskContext>,
}

impl Default for AppStateFile {
    fn default() -> Self {
        Self {
            version: APP_STATE_VERSION,
            last_session: None,
            sessions: BTreeMap::new(),
            collapsed_workspaces: BTreeSet::new(),
            pinned_workspaces: BTreeSet::new(),
            project_display_mode: ProjectDisplayMode::default(),
            project_sort_mode: ProjectSortMode::default(),
            ui_preferences: UiPreferences::default(),
            window_geometry: None,
            task_context: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UiPreferencesCompat {
    theme: ThemePreference,
    reduced_motion: bool,
    high_contrast: bool,
    task_suggestions: bool,
    sidebar_tasks_expanded: bool,
    primary_sidebar_open: bool,
    primary_sidebar_width: u16,
    secondary_sidebar_width: u16,
}

impl Default for UiPreferencesCompat {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            reduced_motion: false,
            high_contrast: false,
            task_suggestions: true,
            sidebar_tasks_expanded: true,
            primary_sidebar_open: true,
            primary_sidebar_width: DEFAULT_PRIMARY_SIDEBAR_WIDTH,
            secondary_sidebar_width: DEFAULT_SECONDARY_SIDEBAR_WIDTH,
        }
    }
}

fn deserialize_ui_preferences<'de, D>(deserializer: D) -> Result<UiPreferences, D::Error>
where
    D: Deserializer<'de>,
{
    let preferences = UiPreferencesCompat::deserialize(deserializer)?;
    Ok(UiPreferences {
        theme: preferences.theme,
        reduced_motion: preferences.reduced_motion,
        high_contrast: preferences.high_contrast,
        task_suggestions: preferences.task_suggestions,
        sidebar_tasks_expanded: preferences.sidebar_tasks_expanded,
        primary_sidebar_open: preferences.primary_sidebar_open,
        primary_sidebar_width: preferences.primary_sidebar_width,
        secondary_sidebar_width: preferences.secondary_sidebar_width,
    })
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
        self.load_locked()
    }

    pub fn save(&self, state: &AppStateFile) -> Result<(), CoreError> {
        let _lock = AdvisoryFileLock::acquire(&self.path)?;
        self.save_locked(state)
    }

    /// Mutate the current state while holding one lock across read and publish.
    pub fn update(&self, update: impl FnOnce(&mut AppStateFile)) -> Result<(), CoreError> {
        let _lock = AdvisoryFileLock::acquire(&self.path)?;
        let mut state = self.load_locked()?;
        update(&mut state);
        self.save_locked(&state)
    }

    fn load_locked(&self) -> Result<AppStateFile, CoreError> {
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

    fn save_locked(&self, state: &AppStateFile) -> Result<(), CoreError> {
        if state.version != APP_STATE_VERSION {
            return Err(CoreError::Other(format!(
                "unsupported app state version: {}",
                state.version
            )));
        }
        let bytes = serde_json::to_vec_pretty(state)?;
        write_atomic(&self.path, &bytes)
    }
}
