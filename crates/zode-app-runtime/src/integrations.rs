use std::path::Path;

use agent::mcp::McpConfig;
use zode_core::config::{ConfigManager, ZodeConfig};
use zode_core::plugin::{PluginManager, TOOL_GROUPS};
use zode_node_protocol::{
    CapabilityManifest, IntegrationRegistryEntry, IntegrationRegistryKind,
    IntegrationRegistrySnapshot, IntegrationRegistryState, WorkspaceUri,
};

use crate::workspace_uri_to_path;

const DIRECTORY_UNAVAILABLE: &str = "在线目录不可用；当前仅显示本机已发现的集成。";

pub(crate) fn discover_registry(
    workspace_uri: WorkspaceUri,
    config_dir: Option<&Path>,
    capabilities: &CapabilityManifest,
) -> Result<IntegrationRegistrySnapshot, String> {
    let cwd = workspace_uri_to_path(&workspace_uri).map_err(|error| error.to_string())?;
    let cfg = config_dir
        .map(|config_dir| ConfigManager::load_in(&cwd, config_dir, false))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let skill_dirs = zode_core::skills::skills_dirs(&cwd);
    let skills = zode_core::skills::load_skills_from(&skill_dirs)
        .list()
        .into_iter()
        .map(|skill| (skill.name, skill.description))
        .collect::<Vec<_>>();
    let mcp = zode_core::mcp::discover_mcp_config(&cwd).unwrap_or_default();

    Ok(registry_from_discovery(
        workspace_uri,
        &cfg,
        capabilities,
        skills,
        mcp,
    ))
}

/// Persists the enabled state of one entry that the local registry has
/// already discovered. This intentionally does not install anything: the
/// current core exposes no verified marketplace or installer contract.
pub(crate) fn set_registry_entry_enabled(
    workspace_uri: WorkspaceUri,
    config_dir: Option<&Path>,
    capabilities: &CapabilityManifest,
    source_id: &str,
    enabled: bool,
) -> Result<Vec<String>, String> {
    let cwd = workspace_uri_to_path(&workspace_uri).map_err(|error| error.to_string())?;
    let snapshot = discover_registry(workspace_uri, config_dir, capabilities)?;
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.source_id == source_id)
        .ok_or_else(|| "integration is not present in the local registry".to_owned())?;
    if entry.kind == IntegrationRegistryKind::NodeCapability {
        return Err("node capabilities cannot be changed from the plugin page".into());
    }

    let config_dir = config_dir
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(ConfigManager::config_dir)
        .map_err(|error| error.to_string())?;
    let original = ConfigManager::load_global_in(&config_dir).map_err(|error| error.to_string())?;
    let mut updated = original.clone();
    let mut manager = PluginManager::from_config(&updated);
    manager.set_enabled(source_id, enabled);
    updated.plugins.disabled = manager.disabled_ids();
    ConfigManager::save_global_in(&config_dir, &updated).map_err(|error| error.to_string())?;

    let effective =
        ConfigManager::load_in(&cwd, &config_dir, false).map_err(|error| error.to_string())?;
    if PluginManager::from_config(&effective).is_enabled(source_id) != enabled {
        let _ = ConfigManager::save_global_in(&config_dir, &original);
        return Err(
            "project configuration overrides this plugin; edit the project config explicitly"
                .into(),
        );
    }
    Ok(effective.plugins.disabled)
}

fn registry_from_discovery(
    workspace_uri: WorkspaceUri,
    cfg: &ZodeConfig,
    capabilities: &CapabilityManifest,
    mut skills: Vec<(String, String)>,
    mcp: McpConfig,
) -> IntegrationRegistrySnapshot {
    let plugins = PluginManager::from_config(cfg);
    let mut entries = Vec::new();

    for (group, description, _members) in TOOL_GROUPS {
        let source_id = format!("tools:{group}");
        let feature_available = match *group {
            "browser" => cfg.browser.enabled(),
            "op" => cfg.openpencil.enabled(),
            _ => true,
        };
        entries.push(IntegrationRegistryEntry {
            state: enabled_state(feature_available && plugins.is_enabled(&source_id)),
            source_id,
            name: (*group).into(),
            description: (*description).into(),
            kind: IntegrationRegistryKind::ToolGroup,
            installed: true,
        });
    }

    skills.sort_by(|left, right| left.0.cmp(&right.0));
    skills.dedup_by(|left, right| left.0 == right.0);
    for (name, description) in skills {
        entries.push(IntegrationRegistryEntry {
            source_id: format!("skill:{name}"),
            state: enabled_state(plugins.skill_enabled(&name)),
            name,
            description,
            kind: IntegrationRegistryKind::Skill,
            installed: true,
        });
    }

    for (name, server) in mcp.servers {
        let enabled = server.enabled() && plugins.mcp_enabled(&name);
        entries.push(IntegrationRegistryEntry {
            source_id: format!("mcp:{name}"),
            name,
            description: "本机 MCP 配置（未探测连接状态）".into(),
            kind: IntegrationRegistryKind::Mcp,
            state: if enabled {
                IntegrationRegistryState::Configured
            } else {
                IntegrationRegistryState::Disabled
            },
            installed: false,
        });
    }

    let mut lsp_servers = cfg.lsp.servers.iter().collect::<Vec<_>>();
    lsp_servers.sort_by(|left, right| left.0.cmp(right.0));
    for (language, server) in lsp_servers {
        let enabled = !server.command.trim().is_empty() && plugins.lsp_enabled(language);
        entries.push(IntegrationRegistryEntry {
            source_id: format!("lsp:{language}"),
            name: language.clone(),
            description: if server.command.trim().is_empty() {
                "语言服务配置缺少启动命令".into()
            } else {
                format!("语言服务命令：{}", server.command)
            },
            kind: IntegrationRegistryKind::Lsp,
            state: if enabled {
                IntegrationRegistryState::Configured
            } else {
                IntegrationRegistryState::Disabled
            },
            installed: false,
        });
    }

    for capability in &capabilities.capabilities {
        let (source, name, description) = capability_copy(capability);
        entries.push(IntegrationRegistryEntry {
            source_id: format!("capability:{source}"),
            name: name.into(),
            description: description.into(),
            kind: IntegrationRegistryKind::NodeCapability,
            state: IntegrationRegistryState::Ready,
            installed: true,
        });
    }

    IntegrationRegistrySnapshot {
        workspace_uri,
        entries,
        directory_error: Some(DIRECTORY_UNAVAILABLE.into()),
    }
}

const fn enabled_state(enabled: bool) -> IntegrationRegistryState {
    if enabled {
        IntegrationRegistryState::Ready
    } else {
        IntegrationRegistryState::Disabled
    }
}

const fn capability_copy(
    capability: &zode_node_protocol::NodeCapability,
) -> (&'static str, &'static str, &'static str) {
    use zode_node_protocol::NodeCapability;
    match capability {
        NodeCapability::Agent => ("agent", "智能体", "运行并协调 AI 编码任务"),
        NodeCapability::Workspace => ("workspace", "工作区", "读取当前项目与会话上下文"),
        NodeCapability::FileSystem => ("filesystem", "文件系统", "读取和修改本地文件"),
        NodeCapability::Terminal => ("terminal", "终端", "运行本地命令与开发工具"),
        NodeCapability::Browser => ("browser", "浏览器", "打开网页并与页面交互"),
        NodeCapability::Camera => ("camera", "相机", "访问设备摄像头输入"),
        NodeCapability::Notifications => ("notifications", "通知", "发送本地系统通知"),
        NodeCapability::Approval => ("approval", "审批", "在敏感操作前请求确认"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use zode_core::config::{LspServerConfig, ZodeConfig};
    use zode_node_protocol::{NodeCapability, NodeId};

    use super::*;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            node_id: NodeId::new(),
            capabilities: BTreeSet::from([
                NodeCapability::Agent,
                NodeCapability::Workspace,
                NodeCapability::FileSystem,
                NodeCapability::Terminal,
                NodeCapability::Browser,
                NodeCapability::Camera,
                NodeCapability::Notifications,
                NodeCapability::Approval,
            ]),
        }
    }

    #[test]
    fn real_sources_exceed_catalog_landmarks_without_fixtures() {
        let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
        let catalog = registry_from_discovery(
            workspace_uri,
            &ZodeConfig::default(),
            &manifest(),
            Vec::new(),
            McpConfig::default(),
        );

        assert!(
            catalog
                .entries
                .iter()
                .filter(|entry| entry.installed)
                .count()
                >= 8
        );
        assert!(catalog.entries.len() >= 10);
        assert!(catalog
            .entries
            .iter()
            .all(|entry| !entry.source_id.is_empty()));
        assert!(catalog.entries.iter().any(|entry| {
            entry.kind == IntegrationRegistryKind::ToolGroup
                && entry.state == IntegrationRegistryState::Ready
        }));
        assert!(catalog.entries.iter().any(|entry| {
            entry.kind == IntegrationRegistryKind::NodeCapability
                && entry.state == IntegrationRegistryState::Ready
        }));
    }

    #[test]
    fn disabled_and_configured_states_never_claim_mcp_connections() {
        let mut cfg = ZodeConfig::default();
        cfg.plugins.disabled = vec!["tools:git".into(), "skill:review".into()];
        cfg.browser.enabled = Some(false);
        cfg.openpencil.enabled = Some(false);
        cfg.lsp.servers.insert(
            "rust".into(),
            LspServerConfig {
                command: "rust-analyzer".into(),
                args: Vec::new(),
                extensions: vec!["rs".into()],
            },
        );
        let mcp: McpConfig = serde_json::from_value(serde_json::json!({
            "servers": {
                "github": { "transport": "stdio", "command": "github-mcp" },
                "off": { "transport": "stdio", "command": "off-mcp", "enabled": false }
            }
        }))
        .unwrap();
        let catalog = registry_from_discovery(
            WorkspaceUri::new("file:///repo/zode").unwrap(),
            &cfg,
            &manifest(),
            vec![("review".into(), "Review code".into())],
            mcp,
        );

        let state = |source_id: &str| {
            catalog
                .entries
                .iter()
                .find(|entry| entry.source_id == source_id)
                .unwrap()
                .state
        };
        assert_eq!(state("tools:git"), IntegrationRegistryState::Disabled);
        assert_eq!(state("tools:browser"), IntegrationRegistryState::Disabled);
        assert_eq!(state("tools:op"), IntegrationRegistryState::Disabled);
        assert_eq!(state("skill:review"), IntegrationRegistryState::Disabled);
        assert_eq!(state("mcp:github"), IntegrationRegistryState::Configured);
        assert_eq!(state("mcp:off"), IntegrationRegistryState::Disabled);
        assert_eq!(state("lsp:rust"), IntegrationRegistryState::Configured);
        assert!(catalog.entries.iter().all(|entry| {
            entry.kind != IntegrationRegistryKind::Mcp
                || matches!(
                    entry.state,
                    IntegrationRegistryState::Configured | IntegrationRegistryState::Disabled
                )
        }));
    }

    #[test]
    fn local_registry_toggle_persists_only_a_discovered_plugin() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace_uri = crate::path_to_workspace_uri(&workspace).unwrap();

        let disabled = set_registry_entry_enabled(
            workspace_uri.clone(),
            Some(&config_dir),
            &manifest(),
            "tools:git",
            false,
        )
        .unwrap();
        assert_eq!(disabled, vec!["tools:git"]);
        assert_eq!(
            ConfigManager::load_global_in(&config_dir)
                .unwrap()
                .plugins
                .disabled,
            vec!["tools:git"]
        );

        assert!(set_registry_entry_enabled(
            workspace_uri,
            Some(&config_dir),
            &manifest(),
            "directory:invented",
            true,
        )
        .unwrap_err()
        .contains("not present"));
    }
}
