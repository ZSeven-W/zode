use std::collections::BTreeSet;

use zode_app_model::{
    demo_state, EnvironmentEntry, EnvironmentSnapshot, IntegrationsTab, LayoutClass, LoadState,
    ProjectState, SecondaryPane, SessionDiffState, SessionPresentationState, SettingsCategory,
    ShellRoute, ThemePreference, TranscriptItem, TranscriptState, ZodeAppState,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, NodeCapability, RuntimeOptions, SandboxMode,
    SessionLocator, ThreadStatus, ThreadSummary, ToolCall, ToolStatus, UsageSnapshot, WorkspaceUri,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRoute {
    Empty,
    Conversation,
    Settings,
    Integrations,
    Environment,
    Review,
}

pub fn fixture_state(
    route: SnapshotRoute,
    theme: ThemePreference,
    viewport_width: u32,
) -> ZodeAppState {
    let mut state = demo_state();
    let node_id = state.host.node_id;
    let workspace = workspace_uri("file:///workspace/zode");
    let openpencil_workspace = workspace_uri("file:///workspace/openpencil");
    let session = SessionLocator::new(node_id, "desktop-snapshot-session");
    let earlier_session = SessionLocator::new(node_id, "desktop-plan-session");
    let openpencil_session = SessionLocator::new(node_id, "openpencil-alignment-session");

    state.host.capabilities.capabilities = BTreeSet::from([
        NodeCapability::Agent,
        NodeCapability::Workspace,
        NodeCapability::FileSystem,
        NodeCapability::Terminal,
        NodeCapability::Browser,
        NodeCapability::Notifications,
        NodeCapability::Approval,
    ]);
    state.projects = vec![
        ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1_720_670_400_000,
        },
        ProjectState {
            workspace_uri: openpencil_workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1_720_584_000_000,
        },
    ];
    state.threads = vec![
        ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace.clone(),
            title: "Zode 桌面端截图回归".into(),
            updated_at_ms: 1_720_670_400_000,
            status: ThreadStatus::Idle,
        },
        ThreadSummary {
            session: earlier_session,
            workspace_uri: workspace.clone(),
            title: "梳理桌面端实施计划".into(),
            updated_at_ms: 1_720_584_000_000,
            status: ThreadStatus::Idle,
        },
        ThreadSummary {
            session: openpencil_session,
            workspace_uri: openpencil_workspace,
            title: "核对样式系统边界".into(),
            updated_at_ms: 1_720_497_600_000,
            status: ThreadStatus::Idle,
        },
    ];
    state.active_workspace = Some(workspace.clone());
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::UserText("照着参考界面完善 Zode 桌面端，并补齐截图回归。".into()),
                TranscriptItem::Thinking(
                    "已读取桌面端实施计划，正在核对真实状态与布局。".into(),
                ),
                TranscriptItem::Tool(ToolCall {
                    id: "read-desktop-plan".into(),
                    name: "read_file".into(),
                    status: ToolStatus::Completed,
                    summary: "已读取桌面端实施计划与界面组件".into(),
                    detail: Some(
                        "openpencil-docs/zode/desktop/plans/2026-07-10-zode-jian-desktop-app.md"
                            .into(),
                    ),
                }),
                TranscriptItem::AssistantText(
                    "截图回归现在覆盖真实的本地状态：\n\n- 项目与任务来自会话索引\n- 环境信息展示当前分支与变更\n- 插件页只列出节点实际声明的能力"
                        .into(),
                ),
            ],
            last_sequence: 4,
            busy: false,
            scroll_offset: 0.0,
            follow_tail: false,
            item_heights: Vec::new(),
        },
    );
    state.usage.insert(
        session.clone(),
        UsageSnapshot {
            input_tokens: 2_148,
            output_tokens: 936,
            context_used: Some(0.18),
            cost_usd: None,
        },
    );
    state.project_permissions.insert(
        workspace.clone(),
        LoadState::Ready(vec!["read_file".into(), "shell".into()]),
    );
    state.composer.model = Some("gpt-5.6".into());
    state.composer.effort = Some("high".into());
    state.composer.sandbox_label = "完全访问".into();
    state.composer.focused = true;
    state.ui_preferences.theme = theme;
    state.shell.layout = LayoutClass::for_width(viewport_width as f32);

    let diff = DiffSnapshot {
        session: session.clone(),
        files: vec![
            DiffFile {
                path: "crates/zode-app/tests/snapshots.rs".into(),
                status: DiffFileStatus::Added,
                additions: 86,
                deletions: 0,
            },
            DiffFile {
                path: "crates/zode-app-ui/src/widgets/composer.rs".into(),
                status: DiffFileStatus::Modified,
                additions: 12,
                deletions: 4,
            },
        ],
        unified: concat!(
            "diff --git a/crates/zode-app/tests/snapshots.rs b/crates/zode-app/tests/snapshots.rs\n",
            "new file mode 100644\n",
            "--- /dev/null\n",
            "+++ b/crates/zode-app/tests/snapshots.rs\n",
            "@@ -0,0 +1,5 @@\n",
            "+#[test]\n",
            "+fn reference_snapshots_match_platform_goldens() {\n",
            "+    assert_platform_snapshot();\n",
            "+}\n",
            "+\n",
            "diff --git a/crates/zode-app-ui/src/widgets/composer.rs b/crates/zode-app-ui/src/widgets/composer.rs\n",
            "--- a/crates/zode-app-ui/src/widgets/composer.rs\n",
            "+++ b/crates/zode-app-ui/src/widgets/composer.rs\n",
            "@@ -12,3 +12,3 @@\n",
            "-    let label = \"main\";\n",
            "+    let label = current_branch;\n",
            "     paint_label(label);\n",
        )
        .into(),
    };
    state.presentation.sessions.insert(
        session,
        SessionPresentationState {
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(diff),
            },
            context: LoadState::Ready(EnvironmentSnapshot {
                workspace_uri: workspace,
                branch: Some("codex/zode-jian-desktop".into()),
                subagents: Vec::new(),
                background_processes: Vec::new(),
                sources: vec![EnvironmentEntry {
                    id: "desktop-plan".into(),
                    label: "Zode 桌面端实施计划".into(),
                    value: None,
                }],
            }),
            preview: zode_app_model::PreviewState::Idle,
            runtime_options: LoadState::Ready(RuntimeOptions {
                models: vec!["gpt-5.6".into()],
                active_model: Some("gpt-5.6".into()),
                effort: Some("high".into()),
                sandbox_mode: SandboxMode::Off,
                sandbox_network: false,
            }),
        },
    );

    let (shell_route, secondary_pane) = match route {
        SnapshotRoute::Empty | SnapshotRoute::Conversation => (ShellRoute::Conversation, None),
        SnapshotRoute::Settings => (ShellRoute::Settings(SettingsCategory::General), None),
        SnapshotRoute::Integrations => (ShellRoute::Integrations(IntegrationsTab::Plugins), None),
        SnapshotRoute::Environment => (ShellRoute::Conversation, Some(SecondaryPane::Environment)),
        SnapshotRoute::Review => (ShellRoute::Conversation, Some(SecondaryPane::Review)),
    };
    state.presentation.route = shell_route;
    state.presentation.secondary_pane = secondary_pane;
    state.shell.page = shell_route.legacy_page();
    if route == SnapshotRoute::Empty {
        state.current_session = None;
        state.transcripts.clear();
    }
    state
}

fn workspace_uri(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).expect("snapshot workspace URI is valid")
}
