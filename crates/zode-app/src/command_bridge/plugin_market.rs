use zode_app_model::{LoadState, PluginAddState, PluginAddStatus, PluginDetailMode, ZodeAppState};
use zode_node_protocol::{
    AgentCommandKind, AgentEndpoint, AgentQuery, AgentSnapshot, InstalledPluginSummary,
    SessionLocator,
};

use super::{Completion, CompletionResult};

pub(super) struct PluginMarketDispatchParts {
    pub session: SessionLocator,
    pub kind: AgentCommandKind,
    pub completion: Completion,
}

/// A stable pseudo-session for plugin-market commands, which are global
/// (config-dir-scoped) rather than tied to any one task - mirrors
/// `"integration-settings"` / `"provider-configuration-reload"`.
fn plugin_market_session(state: &ZodeAppState) -> SessionLocator {
    SessionLocator::new(state.host.node_id, "plugin-market")
}

pub(super) fn prepare_install(
    state: &mut ZodeAppState,
) -> Result<PluginMarketDispatchParts, String> {
    if !state.presentation.plugin_add.open {
        return Err("插件安装表单未打开".into());
    }
    let spec = state.presentation.plugin_add.spec.trim().to_owned();
    if spec.is_empty() {
        return Err("请输入插件仓库来源".into());
    }
    if state.presentation.plugin_add.status == PluginAddStatus::Installing {
        return Err("已有插件正在安装".into());
    }
    let reference = {
        let reference = state.presentation.plugin_add.reference.trim();
        (!reference.is_empty()).then(|| reference.to_owned())
    };
    state.presentation.plugin_add.status = PluginAddStatus::Installing;
    Ok(PluginMarketDispatchParts {
        session: plugin_market_session(state),
        kind: AgentCommandKind::InstallPlugin { spec, reference },
        completion: Completion::RefreshInstalledPlugins,
    })
}

pub(super) fn prepare_uninstall(
    state: &mut ZodeAppState,
) -> Result<PluginMarketDispatchParts, String> {
    let Some(detail) = state.presentation.plugin_detail.as_mut() else {
        return Err("没有正在查看的插件".into());
    };
    if detail.mode != PluginDetailMode::ConfirmUninstall {
        return Err("请先确认删除".into());
    }
    let plugin_id = detail.plugin_id.clone();
    detail.mode = PluginDetailMode::Uninstalling;
    Ok(PluginMarketDispatchParts {
        session: plugin_market_session(state),
        kind: AgentCommandKind::UninstallPlugin { plugin_id },
        completion: Completion::RefreshInstalledPlugins,
    })
}

pub(super) fn prepare_grant(
    state: &mut ZodeAppState,
    keys: Option<Vec<String>>,
) -> Result<PluginMarketDispatchParts, String> {
    let Some(detail) = state.presentation.plugin_detail.as_ref() else {
        return Err("没有正在查看的插件".into());
    };
    if !matches!(detail.mode, PluginDetailMode::TrustReview { .. }) {
        return Err("请先打开信任审查".into());
    }
    let plugin_id = detail.plugin_id.clone();
    Ok(PluginMarketDispatchParts {
        session: plugin_market_session(state),
        kind: AgentCommandKind::GrantPluginTrust { plugin_id, keys },
        completion: Completion::RefreshInstalledPlugins,
    })
}

pub(super) async fn complete_installed_plugins(
    endpoint: &dyn AgentEndpoint,
) -> Result<CompletionResult, String> {
    match endpoint
        .query(AgentQuery::InstalledPlugins)
        .await
        .map_err(|error| error.to_string())?
    {
        AgentSnapshot::InstalledPlugins(plugins) => Ok(CompletionResult::InstalledPlugins(plugins)),
        _ => Err("the endpoint returned the wrong installed-plugins snapshot".into()),
    }
}

pub(super) fn apply_success(state: &mut ZodeAppState, plugins: Vec<InstalledPluginSummary>) {
    let still_installed = state
        .presentation
        .plugin_detail
        .as_ref()
        .is_some_and(|detail| plugins.iter().any(|plugin| plugin.id == detail.plugin_id));
    if !still_installed {
        // Covers both a completed uninstall (the plugin is gone) and a
        // vanished plugin detected on refresh - either way the overlay has
        // nothing left to show.
        state.presentation.plugin_detail = None;
    } else if let Some(detail) = state.presentation.plugin_detail.as_mut() {
        // A successful `GrantPluginTrust` folds the review screen back to
        // the capability list; an uninstall confirmation never reaches here
        // (the plugin is gone, handled above).
        if matches!(detail.mode, PluginDetailMode::TrustReview { .. }) {
            detail.mode = PluginDetailMode::Overview;
        }
    }
    if state.presentation.plugin_add.status == PluginAddStatus::Installing {
        state.presentation.plugin_add = PluginAddState::default();
    }
    state.presentation.installed_plugins = LoadState::Ready(plugins);
}

pub(super) fn mark_failure(state: &mut ZodeAppState, kind: &AgentCommandKind, message: &str) {
    match kind {
        AgentCommandKind::InstallPlugin { .. } => {
            state.presentation.plugin_add.status = PluginAddStatus::Failed(message.to_owned());
        }
        AgentCommandKind::UninstallPlugin { .. } => {
            if let Some(detail) = state.presentation.plugin_detail.as_mut() {
                detail.mode = PluginDetailMode::Overview;
                detail.notice = Some(format!("删除失败：{message}"));
            }
        }
        AgentCommandKind::GrantPluginTrust { .. } => {
            if let Some(detail) = state.presentation.plugin_detail.as_mut() {
                detail.notice = Some(format!("信任操作失败：{message}"));
            }
        }
        _ => {}
    }
}
