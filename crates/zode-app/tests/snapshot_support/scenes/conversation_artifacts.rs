use zode_app_model::{
    ActivityEntry, AttachmentMetadata, EnvironmentEntry, FileArtifact, GoalProgress, LoadState,
    SecondaryPane, ShellRoute, ThemePreference, TranscriptItem,
};
use zode_node_protocol::{ToolCall, ToolStatus};

use super::ReferenceScene;
use crate::snapshot_support::fixture::{base_scene_state, set_transcript};

pub fn conversation_artifacts_scene(theme: ThemePreference, viewport_width: u32) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    let composer_attachment = AttachmentMetadata {
        id: "reference-composer-image".into(),
        path: Some("design/references/zode-desktop.png".into()),
        display_name: "zode-desktop.png".into(),
        media_type: "image/png".into(),
        width: Some(1800),
        height: Some(1080),
        byte_len: 248_320,
    };
    set_transcript(
        &mut state,
        vec![
            TranscriptItem::UserText(
                "照着参考界面完成视觉重构，并保留真实 runtime 数据边界。".into(),
            ),
            TranscriptItem::Thinking("正在读取设计规格和当前组件树。".into()),
            TranscriptItem::ActivityGroup(vec![
                ActivityEntry {
                    id: "artifact-audit".into(),
                    title: "已核对六个参考场景".into(),
                    detail: Some("结构、密度、分栏与 Composer".into()),
                    completed: true,
                },
                ActivityEntry {
                    id: "artifact-layout".into(),
                    title: "已检查共享布局几何".into(),
                    detail: Some("1800×1080 · 1×".into()),
                    completed: true,
                },
            ]),
            TranscriptItem::Tool(ToolCall {
                id: "artifact-read-plan".into(),
                name: "read_file".into(),
                status: ToolStatus::Completed,
                summary: "读取 reference-first 实施计划".into(),
                detail: Some(
                    "zode/desktop/plans/2026-07-12-zode-reference-first-visual-rebuild-plan.md"
                        .into(),
                ),
            }),
            TranscriptItem::AssistantText(
                "已确认顶层骨架保持不变，接下来补齐富会话、文件卡、目标进度与附件层。".into(),
            ),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "artifact-transcript".into(),
                path: "crates/zode-app-ui/src/widgets/transcript/mod.rs".into(),
                summary: "补齐富会话卡片和虚拟列表测量".into(),
                change_summary: Some("+214 -38".into()),
            }),
            TranscriptItem::Tool(ToolCall {
                id: "artifact-tests".into(),
                name: "shell".into(),
                status: ToolStatus::Completed,
                summary: "运行模型、UI 与快照测试".into(),
                detail: Some("38 passed · 0 failed".into()),
            }),
            TranscriptItem::Status {
                code: "visual-review".into(),
                message: "正在生成 overlay 与 difference heatmap".into(),
            },
            TranscriptItem::Attachment(composer_attachment.clone()),
            TranscriptItem::GoalProgress(GoalProgress {
                id: "reference-rebuild".into(),
                title: "完成 Zode Desktop 六场景视觉重构".into(),
                completed: 5,
                total: 6,
            }),
            TranscriptItem::AssistantText(
                "第一轮实现已经通过自动门禁。现在保留实际输出供整幅画布人工检查。".into(),
            ),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "artifact-snapshots".into(),
                path: "crates/zode-app/tests/snapshots.rs".into(),
                summary: "注册六个确定性 1800×1080 场景".into(),
                change_summary: Some("+326 -142".into()),
            }),
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "artifact-final-gates".into(),
                title: "最终门禁正在进行".into(),
                detail: Some("fmt · clippy · workspace tests · snapshots".into()),
                completed: false,
            }]),
            TranscriptItem::Approval {
                id: "artifact-push".into(),
                tool: "git push".into(),
            },
        ],
        true,
    );
    let session = state
        .current_session
        .clone()
        .expect("artifact scene has a session");
    let presentation = state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("artifact scene has presentation state");
    let context = match &mut presentation.context {
        LoadState::Ready(context) => context,
        _ => panic!("artifact scene starts with loaded context"),
    };
    context.subagents = vec![EnvironmentEntry {
        id: "artifact-visual-audit".into(),
        label: "视觉完成度审查".into(),
        value: Some("已完成".into()),
    }];
    context.background_processes = vec![EnvironmentEntry {
        id: "artifact-snapshot-render".into(),
        label: "参考场景渲染".into(),
        value: Some("进行中".into()),
    }];
    state.composer.attachments = vec![composer_attachment];
    state.composer.draft = "继续检查六张完整画布的文字对齐和视觉密度".into();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(SecondaryPane::Environment);
    state.shell.page = ShellRoute::Conversation.legacy_page();
    ReferenceScene {
        name: "conversation-artifacts",
        state,
    }
}
