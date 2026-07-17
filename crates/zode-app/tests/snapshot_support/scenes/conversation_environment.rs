use zode_app_model::{
    ActivityEntry, EnvironmentEntry, FileArtifact, GoalProgress, LoadState, ShellRoute,
    ThemePreference, TranscriptItem,
};
use zode_node_protocol::{ToolCall, ToolStatus};

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
            TranscriptItem::UserText("继续推动 Zode 桌面端实施，并把环境状态保持可核对。".into()),
            TranscriptItem::Thinking("正在读取分支、工作区变更和后台任务。".into()),
            TranscriptItem::Tool(ToolCall {
                id: "environment-git-status".into(),
                name: "shell".into(),
                status: ToolStatus::Completed,
                summary: "读取 Git 状态与当前分支".into(),
                detail: Some("codex/zode-jian-desktop".into()),
            }),
            TranscriptItem::AssistantText(
                "环境面板只展示真实查询到的分组；缺少数据时省略对应区域。".into(),
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
            TranscriptItem::AssistantText(
                concat!(
                    "当前工作区有 2 个文件变更，分支与比较范围已经投影到右侧。\n\n",
                    "- 变更：2 个文件\n",
                    "- 分支：codex/zode-jian-desktop\n",
                    "- 后台：1 个确定性离屏渲染"
                )
                .into(),
            ),
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
            TranscriptItem::AssistantText("下一步是整幅画布 overlay 检查与最终门禁。".into()),
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
    let context = match &mut presentation.context {
        LoadState::Ready(context) => context,
        _ => panic!("environment scene starts with loaded context"),
    };
    context.subagents = vec![
        EnvironmentEntry {
            id: "snapshot-infra".into(),
            label: "快照基础设施".into(),
            value: Some("进行中".into()),
        },
        EnvironmentEntry {
            id: "visual-audit".into(),
            label: "视觉完成度审查".into(),
            value: Some("已完成".into()),
        },
    ];
    context.background_processes = vec![EnvironmentEntry {
        id: "snapshot-render".into(),
        label: "离屏快照渲染".into(),
        value: Some("target/zode-app-snapshot-artifacts".into()),
    }];
    context.sources = vec![
        EnvironmentEntry {
            id: "source-reference".into(),
            label: "Codex Desktop 视觉参考".into(),
            value: Some("6 张已批准截图".into()),
        },
        EnvironmentEntry {
            id: "source-plan".into(),
            label: "Reference-First 实施计划".into(),
            value: Some("2026-07-12".into()),
        },
    ];
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.pinned_summary_overlay_open = true;
    state.shell.page = ShellRoute::Conversation.legacy_page();
    ReferenceScene {
        name: "conversation-environment",
        state,
    }
}
