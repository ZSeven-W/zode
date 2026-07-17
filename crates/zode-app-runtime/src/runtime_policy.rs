use std::path::{Path, PathBuf};

use zode_core::config::ConfigManager;
use zode_core::sandbox::{SandboxConfig, SandboxMode as CoreSandboxMode};
use zode_core::{CoreError, EngineTemplate};
use zode_node_protocol::{RuntimeOptions, SandboxMode};

pub(crate) fn apply_workspace_policy(
    template: &EngineTemplate,
    cwd: &Path,
    config_dir: &Path,
) -> Result<EngineTemplate, CoreError> {
    let cfg = ConfigManager::load_in(cwd, config_dir, false)?;
    let sandbox = if cfg.sandbox.enabled.unwrap_or(true) {
        let mode = cfg
            .sandbox
            .mode
            .as_deref()
            .map(CoreSandboxMode::parse)
            .unwrap_or_default();
        let writable_roots = cfg
            .sandbox
            .writable_roots
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        zode_core::sandbox::resolve(
            cwd,
            true,
            mode,
            cfg.sandbox.network.unwrap_or(false),
            &writable_roots,
            cfg.sandbox.exclude_slash_tmp.unwrap_or(false),
            cfg.sandbox.exclude_tmpdir_env_var.unwrap_or(false),
        )?
        .map(|sandbox| sandbox.with_restrict_reads(cfg.sandbox.restrict_reads.unwrap_or(false)))
    } else {
        None
    };
    Ok(template
        .with_permissions(cfg.permissions)
        .with_sandbox(sandbox))
}

pub(crate) fn runtime_options(template: &EngineTemplate) -> RuntimeOptions {
    let sandbox = template.sandbox();
    RuntimeOptions {
        models: template.model_ids(),
        active_model: template.model().map(str::to_string),
        effort: template.effort().map(str::to_string),
        sandbox_mode: match sandbox.map(SandboxConfig::mode) {
            None => SandboxMode::Off,
            Some(CoreSandboxMode::ReadOnly) => SandboxMode::ReadOnly,
            Some(CoreSandboxMode::WorkspaceWrite) => SandboxMode::WorkspaceWrite,
        },
        sandbox_network: sandbox.is_some_and(SandboxConfig::allow_network),
    }
}
