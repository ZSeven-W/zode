use std::path::{Path, PathBuf};

use zode_core::approval::ApprovalPolicy;
use zode_core::config::ConfigManager;
use zode_core::sandbox::{SandboxConfig, SandboxMode as CoreSandboxMode};
use zode_core::{CoreError, EngineTemplate};
use zode_node_protocol::{ApprovalMode, RuntimeOptions, SandboxMode};

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
        approval_mode: match template.approval_policy() {
            ApprovalPolicy::Request => ApprovalMode::Request,
            ApprovalPolicy::Auto => ApprovalMode::Auto,
            ApprovalPolicy::Full => ApprovalMode::Full,
        },
        sandbox_mode: match sandbox.map(SandboxConfig::mode) {
            None => SandboxMode::Off,
            Some(CoreSandboxMode::ReadOnly) => SandboxMode::ReadOnly,
            Some(CoreSandboxMode::WorkspaceWrite) => SandboxMode::WorkspaceWrite,
        },
        sandbox_network: sandbox.is_some_and(SandboxConfig::allow_network),
    }
}

pub(crate) fn with_approval_mode(template: &EngineTemplate, mode: ApprovalMode) -> EngineTemplate {
    let policy = match mode {
        ApprovalMode::Request => ApprovalPolicy::Request,
        ApprovalMode::Auto => ApprovalPolicy::Auto,
        ApprovalMode::Full => ApprovalPolicy::Full,
    };
    template.with_approval_policy(policy)
}

pub(crate) fn with_sandbox(
    template: &EngineTemplate,
    cwd: &Path,
    mode: SandboxMode,
    network: bool,
) -> Result<EngineTemplate, CoreError> {
    let sandbox = match mode {
        SandboxMode::Off => None,
        SandboxMode::ReadOnly | SandboxMode::WorkspaceWrite => {
            let core_mode = match mode {
                SandboxMode::ReadOnly => CoreSandboxMode::ReadOnly,
                _ => CoreSandboxMode::WorkspaceWrite,
            };
            Some(
                template
                    .sandbox()
                    .cloned()
                    .map(|sandbox| sandbox.with_mode(core_mode).with_network(network))
                    .map(Ok)
                    .unwrap_or_else(|| SandboxConfig::new(cwd, core_mode, network, &[]))?,
            )
        }
    };
    Ok(template.with_sandbox(sandbox))
}

pub(crate) fn with_permission_preset(
    template: &EngineTemplate,
    cwd: &Path,
    approval_mode: ApprovalMode,
    sandbox_mode: SandboxMode,
    network: bool,
) -> Result<EngineTemplate, CoreError> {
    let template = with_approval_mode(template, approval_mode);
    with_sandbox(&template, cwd, sandbox_mode, network)
}

pub(crate) fn persist_sandbox(
    cwd: &Path,
    mode: SandboxMode,
    network: bool,
) -> Result<(), CoreError> {
    ConfigManager::update_project_state(cwd, |state| {
        state.insert(
            "sandbox".to_string(),
            serde_json::json!({
                "enabled": mode != SandboxMode::Off,
                "mode": match mode {
                    SandboxMode::ReadOnly => Some("read-only"),
                    SandboxMode::WorkspaceWrite => Some("workspace-write"),
                    SandboxMode::Off => None,
                },
                "network": (mode != SandboxMode::Off).then_some(network),
            }),
        );
    })
}
