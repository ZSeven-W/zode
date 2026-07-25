use crate::error::{error, ServerResult};
use serde_json::{json, Value};
use std::path::Path;
use zode_app_server_protocol::rpc::INVALID_PARAMS;
use zode_app_server_protocol::types::{
    ConfigListResponse, ConfigReadResponse, HookSummary, HooksListResponse, McpServerStatus,
    McpServerStatusListResponse, ModelListResponse, ModelProviderSummary, ModelSummary,
    PluginListResponse, PluginSummary, SkillReadParams, SkillReadResponse, SkillSummary,
    SkillsListResponse,
};
use zode_core::config::ConfigManager;
use zode_core::{Catalog, PluginManager};

pub fn model_list() -> ModelListResponse {
    let providers = Catalog::load_blocking()
        .providers()
        .iter()
        .map(|provider| ModelProviderSummary {
            id: provider.id.clone(),
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            kind: format!("{:?}", provider.kind),
            models: provider
                .models
                .iter()
                .map(|model| ModelSummary {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    context: model.context,
                    max_output: model.max_output,
                })
                .collect(),
        })
        .collect();
    ModelListResponse { providers }
}

pub fn config_read() -> ServerResult<ConfigReadResponse> {
    let config = ConfigManager::load_global()
        .map_err(|err| error(INVALID_PARAMS, format!("config/read: {err}")))
        .and_then(|config| {
            serde_json::to_value(config)
                .map_err(|err| error(INVALID_PARAMS, format!("config/read: {err}")))
        })?;
    Ok(ConfigReadResponse { config })
}

pub fn config_list() -> ServerResult<ConfigListResponse> {
    let config = config_read()?.config;
    let mut keys = config
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    Ok(ConfigListResponse { keys })
}

pub fn skills_list(cwd: &Path) -> ServerResult<SkillsListResponse> {
    let (all_skill_meta, _enabled) = skills(cwd)?;
    Ok(SkillsListResponse {
        skills: all_skill_meta
            .into_iter()
            .map(|(name, description)| SkillSummary { name, description })
            .collect(),
    })
}

pub fn skills_read(cwd: &Path, params: SkillReadParams) -> ServerResult<SkillReadResponse> {
    let (_all, enabled) = skills(cwd)?;
    let skill = enabled
        .get(&params.name)
        .ok_or_else(|| error(INVALID_PARAMS, format!("skill not found: {}", params.name)))?;
    Ok(SkillReadResponse {
        name: skill.name,
        description: skill.description,
        prompt: skill.prompt,
        input_schema: skill.input_schema,
    })
}

pub fn hooks_list(cwd: &Path) -> HooksListResponse {
    HooksListResponse {
        hooks: zode_core::hooks_config::load_hook_entries(cwd)
            .into_iter()
            .map(|hook| HookSummary {
                event: hook.event,
                tool: hook.tool,
                script: hook.script,
            })
            .collect(),
    }
}

pub fn mcp_server_status_list(cwd: &Path) -> McpServerStatusListResponse {
    let servers = zode_core::mcp::discover_mcp_config(cwd)
        .map(|config| {
            config
                .servers
                .into_iter()
                .map(|(name, spec)| McpServerStatus {
                    name,
                    enabled: spec.enabled(),
                    connected: false,
                })
                .collect()
        })
        .unwrap_or_default();
    McpServerStatusListResponse { servers }
}

pub fn plugin_list(cwd: &Path) -> ServerResult<PluginListResponse> {
    let config = ConfigManager::load(cwd).unwrap_or_default();
    let manager = PluginManager::from_config(&config);
    let mcp_servers = mcp_server_status_list(cwd)
        .servers
        .into_iter()
        .map(|server| (server.name, server.connected))
        .collect::<Vec<_>>();
    let (skills, _enabled) = skills(cwd)?;
    let packages = zode_core::plugin_package::installed_package_entries();
    let plugins = manager
        .list(&packages, &mcp_servers, &skills, &[])
        .into_iter()
        .map(|plugin| PluginSummary {
            id: plugin.id,
            kind: plugin.kind.prefix().to_string(),
            name: plugin.name,
            description: plugin.description,
            detail: plugin.detail,
            enabled: plugin.enabled,
        })
        .collect();
    Ok(PluginListResponse { plugins })
}

pub fn unsupported_empty(method: &str) -> Value {
    json!({
        "unsupported": method,
        "reason": "This zode server build only exposes read-only capability discovery for this group."
    })
}

fn skills(cwd: &Path) -> ServerResult<(Vec<(String, String)>, agent::skills::SkillRegistry)> {
    let config = ConfigManager::load(cwd).unwrap_or_default();
    let manager = PluginManager::from_config(&config);
    let dirs = zode_core::skills::skills_dirs(cwd);
    Ok(zode_core::skills::load_skills_meta_and_registry(
        &dirs,
        |name| manager.skill_enabled(name),
    ))
}
