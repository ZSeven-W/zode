//! Desktop-side effect handling for the Computer Use settings page
//! (`SettingsCategory::ComputerUse`): live TCC permission status, opening
//! System Settings, and reading/writing the config-backed tool-group toggle
//! and allowed-apps list.
//!
//! `consume_computer_use_command` mirrors `external_preview.rs`'s
//! `consume_*_command` shape - these commands never reach
//! `reduce_settings_command` because they need real OS/file I/O, which
//! `zode-app-model` deliberately cannot perform.
//!
//! Scope note: this wave reads and writes `computer.*` config plus reports
//! live permission grant state. It does not enforce `allowedApps`/`anyApp`
//! anywhere - that consult belongs in the `computer_act` approval gate and
//! is tracked as a follow-up (see `zode_core::config::ComputerConfig::any_app`).

use zode_app_model::{
    AppCommand, ComputerPermissionState, ComputerUseSnapshot, LoadState, ZodeAppState,
};
use zode_core::config::{ConfigManager, ZodeConfig};

use crate::services::ExternalOpenService;

pub(super) fn consume_computer_use_command(
    state: &mut ZodeAppState,
    external_open: &dyn ExternalOpenService,
    command: &AppCommand,
) -> bool {
    match command {
        AppCommand::OpenComputerUsePermissionSettings(pane) => {
            if let Err(error) = external_open.open_system_settings_pane(*pane) {
                eprintln!("zode-app: opening system settings failed: {error}");
            }
            true
        }
        AppCommand::SetComputerToolEnabled(enabled) => {
            let enabled = *enabled;
            update_config(state, move |config| {
                config.computer.enabled = Some(enabled);
            });
            true
        }
        AppCommand::SetComputerAnyApp(any_app) => {
            let any_app = *any_app;
            update_config(state, move |config| {
                config.computer.any_app = Some(any_app);
            });
            true
        }
        AppCommand::AddComputerAllowedApp(app) => {
            let app = app.trim().to_owned();
            if app.is_empty() {
                return true;
            }
            update_config(state, |config| {
                if !config
                    .computer
                    .allowed_apps
                    .iter()
                    .any(|existing| existing == &app)
                {
                    config.computer.allowed_apps.push(app);
                }
            });
            state.computer_use.allowed_app_input.clear();
            true
        }
        AppCommand::RemoveComputerAllowedApp(app) => {
            let app = app.clone();
            update_config(state, move |config| {
                config
                    .computer
                    .allowed_apps
                    .retain(|existing| existing != &app);
            });
            true
        }
        AppCommand::SetComputerAllowedAppInput(value) => {
            state.computer_use.allowed_app_input = value.clone();
            true
        }
        _ => false,
    }
}

/// Loads the global config, applies `mutate`, persists it, then refreshes
/// `state.local_settings.computer` from the saved result - a failed
/// read/write surfaces as `LoadState::Failed` instead of silently drifting
/// from disk.
fn update_config(state: &mut ZodeAppState, mutate: impl FnOnce(&mut ZodeConfig)) {
    let mut config = match ConfigManager::load_global() {
        Ok(config) => config,
        Err(error) => {
            state.local_settings.computer = LoadState::Failed(error.to_string());
            return;
        }
    };
    mutate(&mut config);
    if let Err(error) = ConfigManager::save_global(&config) {
        state.local_settings.computer = LoadState::Failed(error.to_string());
        return;
    }
    state.local_settings.computer = LoadState::Ready(snapshot_from_config(&config));
}

/// Global config + live permission status, as `SettingsCategory::ComputerUse`
/// shows it. `computer.*` is desktop-wide (unlike per-project config), so -
/// symmetrically with `update_config`'s writes - this always reads the
/// global config file, not a project-merged view.
pub(super) fn load_computer_use_snapshot() -> LoadState<ComputerUseSnapshot> {
    match ConfigManager::load_global() {
        Ok(config) => LoadState::Ready(snapshot_from_config(&config)),
        Err(error) => LoadState::Failed(error.to_string()),
    }
}

fn snapshot_from_config(config: &ZodeConfig) -> ComputerUseSnapshot {
    let status = zode_app_runtime::computer_permission_status();
    ComputerUseSnapshot {
        accessibility: map_permission(status.accessibility),
        screen_recording: map_permission(status.screen_recording),
        tool_group_enabled: config.computer.enabled(),
        allowed_apps: config.computer.allowed_apps.clone(),
        any_app: config.computer.any_app(),
    }
}

fn map_permission(state: zode_app_runtime::ComputerPermissionState) -> ComputerPermissionState {
    match state {
        zode_app_runtime::ComputerPermissionState::Granted => ComputerPermissionState::Granted,
        zode_app_runtime::ComputerPermissionState::NotGranted => {
            ComputerPermissionState::NotGranted
        }
        zode_app_runtime::ComputerPermissionState::Unsupported => {
            ComputerPermissionState::Unsupported
        }
    }
}
