use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use zode_app_model::{
    demo_state, EnvironmentEntry, EnvironmentSnapshot, LayoutClass, LoadState, ProjectState,
    SessionDiffState, SessionPresentationState, ThemePreference, TranscriptItem, TranscriptState,
    ZodeAppState,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, NodeCapability, RuntimeOptions, SandboxMode,
    SessionLocator, ThreadStatus, ThreadSummary, ToolCall, ToolStatus, TurnId, UsageSnapshot,
    WorkspaceUri,
};

/// Shared deterministic shell state used only by integration-test scene builders.
///
/// Production bootstrap never references this module. Individual reference scenes
/// own their content and mutate this shell-only base at the test boundary.
pub(crate) fn base_scene_state(theme: ThemePreference, viewport_width: u32) -> ZodeAppState {
    let mut state = demo_state();
    let node_id = state.host.node_id;
    let workspace = workspace_uri("file:///workspace/zode");
    let opentt_workspace = workspace_uri("file:///workspace/opentt");
    let codex_workspace = workspace_uri("file:///workspace/codex");
    let openpencil_workspace = workspace_uri("file:///workspace/openpencil");
    let website_workspace = workspace_uri("file:///workspace/openpencil-website");
    let session = SessionLocator::new(node_id, "desktop-snapshot-session");
    let earlier_session = SessionLocator::new(node_id, "desktop-plan-session");
    let opentt_session = SessionLocator::new(node_id, "opentt-apk-session");
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
            workspace_uri: opentt_workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1_720_627_200_000,
        },
        ProjectState {
            workspace_uri: codex_workspace,
            expanded: true,
            available: true,
            last_opened_ms: 1_720_620_000_000,
        },
        ProjectState {
            workspace_uri: openpencil_workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1_720_584_000_000,
        },
        ProjectState {
            workspace_uri: website_workspace,
            expanded: true,
            available: true,
            last_opened_ms: 1_720_540_800_000,
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
            session: opentt_session,
            workspace_uri: opentt_workspace,
            title: "安装 APK 到电视".into(),
            updated_at_ms: 1_720_627_200_000,
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
    set_transcript(
        &mut state,
        vec![
            TranscriptItem::user_text("照着参考界面完善 Zode 桌面端，并补齐截图回归。"),
            TranscriptItem::Thinking("已读取桌面端实施计划，正在核对真实状态与布局。".into()),
            TranscriptItem::Tool(ToolCall {
                id: "read-desktop-plan".into(),
                name: "read_file".into(),
                status: ToolStatus::Completed,
                summary: "已读取桌面端实施计划与界面组件".into(),
                detail: Some(
                    "openpencil-docs/zode/desktop/plans/2026-07-12-zode-reference-first-visual-rebuild-plan.md"
                        .into(),
                ),
            }),
            TranscriptItem::assistant_text(
                "截图回归现在覆盖真实的本地状态：\n\n- 项目与任务来自会话索引\n- 环境信息展示当前分支与变更\n- 插件页只列出节点实际声明的能力",),
        ],
        false,
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
            "+fn six_reference_scenes() {\n",
            "+    assert_platform_snapshot();\n",
            "+}\n",
            "+\n",
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
                background_processes: Vec::new(),
                sources: vec![EnvironmentEntry::new(
                    "desktop-plan",
                    "Zode 桌面端实施计划",
                    None,
                )],
            }),
            preview: zode_app_model::PreviewState::Idle,
            runtime_options: LoadState::Ready(RuntimeOptions {
                models: vec!["gpt-5.6".into()],
                active_model: Some("gpt-5.6".into()),
                effort: Some("high".into()),
                approval_mode: Default::default(),
                sandbox_mode: SandboxMode::Off,
                sandbox_network: false,
            }),
            subagents: Vec::new(),
            ..SessionPresentationState::default()
        },
    );
    state
}

pub(crate) fn set_transcript(state: &mut ZodeAppState, items: Vec<TranscriptItem>, busy: bool) {
    let session = state
        .current_session
        .clone()
        .expect("reference conversation scene has a session");
    let user_starts = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, TranscriptItem::UserText { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    let mut transcript = TranscriptState {
        last_sequence: items.len() as u64,
        items,
        turns: Vec::new(),
        busy: false,
        scroll_offset: 0.0,
        follow_tail: true,
        item_heights: Vec::new(),
        ..TranscriptState::default()
    };
    for (group_index, start) in user_starts.iter().copied().enumerate() {
        let turn_id = TurnId::parse(&format!("00000000-0000-0000-0000-{:012}", group_index + 1))
            .expect("snapshot turn id is valid");
        let started_at = Instant::now();
        assert!(transcript.begin_turn_at(turn_id, start, start + 1, started_at));
        let is_live_tail = busy && group_index + 1 == user_starts.len();
        if !is_live_tail {
            let elapsed = match group_index {
                0 => Duration::from_secs(20 * 60 + 51),
                1 => Duration::from_secs(39),
                _ => Duration::from_secs(58),
            };
            assert!(transcript.finish_turn_at(turn_id, false, started_at + elapsed));
        }
    }
    transcript.busy = busy;
    state.transcripts.insert(session, transcript);
}

pub(crate) fn workspace_uri(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).expect("snapshot workspace URI is valid")
}
