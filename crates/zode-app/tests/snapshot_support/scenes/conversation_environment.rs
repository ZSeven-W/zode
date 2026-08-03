use zode_app_model::{
    ActivityEntry, EnvironmentEntry, FileArtifact, GoalProgress, LoadState, ShellRoute,
    ThemePreference, TranscriptItem,
};
use zode_node_protocol::{
    BackgroundProcessSnapshot, BackgroundProcessStatus, SubagentSnapshot, SubagentStatus, ToolCall,
    ToolStatus, TurnId,
};

use super::ReferenceScene;
use crate::snapshot_support::fixture::{base_scene_state, set_transcript};

pub fn conversation_environment_scene(
    theme: ThemePreference,
    viewport_width: u32,
) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    set_transcript(
        &mut state,
        vec![
            TranscriptItem::user_text("继续推动 Zode 桌面端实施，并把环境状态保持可核对。"),
            TranscriptItem::Thinking("正在读取分支、工作区变更和后台任务。".into()),
            TranscriptItem::Tool(ToolCall {
                id: "environment-git-status".into(),
                name: "shell".into(),
                status: ToolStatus::Completed,
                summary: "读取 Git 状态与当前分支".into(),
                detail: Some("codex/zode-jian-desktop".into()),
            }),
            TranscriptItem::assistant_text(
                "环境面板只展示真实查询到的分组；缺少数据时省略对应区域。",
            ),
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "environment-review".into(),
                title: "视觉审查子任务已完成".into(),
                detail: Some("未发现 Codex 品牌资源残留".into()),
                completed: true,
            }]),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "environment-panel".into(),
                path: "crates/zode-app-ui/src/widgets/environment/mod.rs".into(),
                summary: "按真实来源分组的环境检查器".into(),
                change_summary: Some("+247 -39".into()),
            }),
            TranscriptItem::Tool(ToolCall {
                id: "environment-tests".into(),
                name: "shell".into(),
                status: ToolStatus::Completed,
                summary: "运行环境面板与无障碍测试".into(),
                detail: Some("29 passed · 0 failed".into()),
            }),
            TranscriptItem::Status {
                code: "ci".into(),
                message: "GitHub Actions 三平台快照正在等待最终 golden".into(),
            },
            TranscriptItem::assistant_text(concat!(
                "当前工作区有 2 个文件变更，分支与比较范围已经投影到右侧。\n\n",
                "- 变更：2 个文件\n",
                "- 分支：codex/zode-jian-desktop\n",
                "- 后台：1 个确定性离屏渲染"
            )),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "environment-scene".into(),
                path: "crates/zode-app/tests/snapshot_support/scenes/conversation_environment.rs"
                    .into(),
                summary: "补齐环境场景的会话与检查器密度".into(),
                change_summary: Some("+58 -4".into()),
            }),
            TranscriptItem::GoalProgress(GoalProgress {
                id: "environment-goal".into(),
                title: "完成 reference-first 视觉重构".into(),
                completed: 5,
                total: 6,
            }),
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "environment-snapshot".into(),
                title: "正在生成 conversation-environment 快照".into(),
                detail: Some("1800×1080 · light · deterministic fonts".into()),
                completed: false,
            }]),
            TranscriptItem::assistant_text("下一步是整幅画布 overlay 检查与最终门禁。"),
        ],
        true,
    );
    let session = state
        .current_session
        .clone()
        .expect("environment scene has a session");
    let presentation = state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("environment scene has presentation state");
    presentation.subagents = vec![
        SubagentSnapshot {
            id: "snapshot-infra".into(),
            agent_type: "快照基础设施".into(),
            display_name: "快照基础设施".into(),
            depth: 0,
            status: SubagentStatus::Running,
            tokens: 1_800,
            turn_id: TurnId::new(),
            completed_at_ms: None,
            result_summary: None,
            model: None,
        },
        SubagentSnapshot {
            id: "visual-audit".into(),
            agent_type: "视觉完成度审查".into(),
            display_name: "视觉完成度审查".into(),
            depth: 0,
            status: SubagentStatus::Completed,
            tokens: 6_400,
            turn_id: TurnId::new(),
            completed_at_ms: Some(1_752_700_000_000),
            result_summary: Some("未发现视觉问题".into()),
            model: None,
        },
    ];
    presentation.background_processes = vec![BackgroundProcessSnapshot {
        id: "snapshot-render".into(),
        command: "离屏快照渲染".into(),
        status: BackgroundProcessStatus::Running,
        started_at_ms: 0,
        tool_call_id: None,
    }];
    let context = match &mut presentation.context {
        LoadState::Ready(context) => context,
        _ => panic!("environment scene starts with loaded context"),
    };
    // Dead field (superseded by `transcript_sources`, which derives Sources
    // rows from the transcript's real `FileArtifact`/`Attachment`/tool-call
    // items instead - this scene's `TranscriptItem::FileArtifact` entries
    // above already drive the real Sources rows). Kept populated only so a
    // future reader does not mistake the empty vec for "this scene has no
    // sources".
    context.sources = vec![
        EnvironmentEntry::new(
            "source-reference",
            "Codex Desktop 视觉参考",
            Some("6 张已批准截图".into()),
        ),
        EnvironmentEntry::new(
            "source-plan",
            "Reference-First 实施计划",
            Some("2026-07-12".into()),
        ),
    ];
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.pinned_summary_overlay_open = true;
    state.shell.page = ShellRoute::Conversation.legacy_page();
    ReferenceScene {
        name: "conversation-environment",
        state,
    }
}
