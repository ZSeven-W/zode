use zode_app_model::{
    integration_catalog, IntegrationsTab, LoadState, ShellRoute, ThemePreference,
};
use zode_node_protocol::{
    IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState, NodeCapability,
};

use super::ReferenceScene;
use crate::snapshot_support::fixture::base_scene_state;

pub fn integrations_catalog_scene(theme: ThemePreference, viewport_width: u32) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    let workspace_uri = state
        .active_available_workspace()
        .cloned()
        .expect("integrations scene has an available workspace");
    let mut entries = TOOL_GROUPS
        .into_iter()
        .map(|(name, description)| IntegrationRegistryEntry {
            source_id: format!("tools:{name}"),
            name: name.into(),
            description: description.into(),
            kind: IntegrationRegistryKind::ToolGroup,
            state: IntegrationRegistryState::Ready,
            installed: true,
        })
        .collect::<Vec<_>>();
    entries.extend(
        state
            .host
            .capabilities
            .capabilities
            .iter()
            .map(capability_entry),
    );
    state.presentation.integrations =
        LoadState::Ready(integration_catalog(IntegrationRegistrySnapshot {
            workspace_uri,
            entries,
            directory_error: Some("在线目录不可用；当前仅显示本机已发现的集成。".into()),
        }));
    let route = ShellRoute::Integrations(IntegrationsTab::Plugins);
    state.presentation.route = route;
    state.presentation.secondary_pane = None;
    state.shell.page = route.legacy_page();
    ReferenceScene {
        name: "integrations-catalog",
        state,
    }
}

const TOOL_GROUPS: [(&str, &str); 10] = [
    ("filesystem", "读取、编辑和管理工作区文件"),
    ("search", "查找文件与搜索内容"),
    ("shell", "运行前台与后台命令"),
    ("git", "检查并操作 Git 仓库"),
    ("web", "获取 URL 与搜索网页"),
    ("notebook", "编辑 Jupyter notebooks"),
    ("todo", "跟踪当前任务列表"),
    ("subagent", "委派独立子任务"),
    ("op", "驱动 OpenPencil 设计"),
    ("browser", "控制内置浏览器"),
];

fn capability_entry(capability: &NodeCapability) -> IntegrationRegistryEntry {
    let (source, name, description) = match capability {
        NodeCapability::Agent => ("agent", "智能体", "运行并协调 AI 编码任务"),
        NodeCapability::Workspace => ("workspace", "工作区", "读取当前项目与会话上下文"),
        NodeCapability::FileSystem => ("filesystem", "文件系统", "读取和修改本地文件"),
        NodeCapability::Terminal => ("terminal", "终端", "运行本地命令与开发工具"),
        NodeCapability::Browser => ("browser", "浏览器", "打开网页并与页面交互"),
        NodeCapability::Camera => ("camera", "相机", "访问设备摄像头输入"),
        NodeCapability::Notifications => ("notifications", "通知", "发送本地系统通知"),
        NodeCapability::Approval => ("approval", "审批", "在敏感操作前请求确认"),
    };
    IntegrationRegistryEntry {
        source_id: format!("capability:{source}"),
        name: name.into(),
        description: description.into(),
        kind: IntegrationRegistryKind::NodeCapability,
        state: IntegrationRegistryState::Ready,
        installed: true,
    }
}
