use zode_app_model::{
    ActivityEntry, FileArtifact, PreviewKind, PreviewState, PreviewTarget, SecondaryPane,
    ShellRoute, ThemePreference, TranscriptItem,
};
use zode_node_protocol::{ToolCall, ToolStatus};

use super::ReferenceScene;
use crate::snapshot_support::fixture::{base_scene_state, set_transcript};

pub fn conversation_document_preview_scene(
    theme: ThemePreference,
    viewport_width: u32,
) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    set_transcript(
        &mut state,
        vec![
            TranscriptItem::UserText("把文档中心整理方案打开在右侧，我要边看边推进。".into()),
            TranscriptItem::Thinking("正在核对工作区文件路径和当前会话归属。".into()),
            TranscriptItem::Tool(ToolCall {
                id: "preview-list-docs".into(),
                name: "list_files".into(),
                status: ToolStatus::Completed,
                summary: "列出 Zode Desktop 文档".into(),
                detail: Some("openpencil-docs/zode/desktop".into()),
            }),
            TranscriptItem::AssistantText(
                "文档按 specs、plans、reports 与 references 归类，根目录索引已经指向当前实施计划。"
                    .into(),
            ),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "preview-plan".into(),
                path: "docs/2026-07-12-reference-first-plan.md".into(),
                summary: "Zode Desktop Reference-First 实施计划".into(),
                change_summary: Some("已更新".into()),
            }),
            TranscriptItem::UserText("右侧预览需要独立 tab、breadcrumb 和外部打开。".into()),
            TranscriptItem::Thinking("正在验证 Markdown 与纯文本的确定性换行。".into()),
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "preview-file-service".into(),
                title: "已通过 FileService 有界读取".into(),
                detail: Some("UTF-8 · 1 MiB 上限 · workspace containment".into()),
                completed: true,
            }]),
            TranscriptItem::Tool(ToolCall {
                id: "preview-tests".into(),
                name: "shell".into(),
                status: ToolStatus::Completed,
                summary: "运行文档预览测试".into(),
                detail: Some("document_preview · file_service · presentation".into()),
            }),
            TranscriptItem::AssistantText(
                "预览只读取当前会话绑定工作区；切换或重定向工作区后不会展示旧缓存。".into(),
            ),
            TranscriptItem::FileArtifact(FileArtifact {
                id: "preview-widget".into(),
                path: "crates/zode-app-ui/src/widgets/document_preview.rs".into(),
                summary: "右侧 700px 文档预览与选中 tab".into(),
                change_summary: Some("+401 -0".into()),
            }),
            TranscriptItem::AssistantText("预览已经打开；可以继续在左侧会话中提出修改。".into()),
        ],
        false,
    );
    let session = state
        .current_session
        .clone()
        .expect("document preview scene has a session");
    let workspace_uri = state
        .available_workspace_for_session(&session)
        .cloned()
        .expect("document preview scene has an available workspace");
    state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("document preview scene has presentation state")
        .preview = PreviewState::Ready {
        target: PreviewTarget {
            workspace_uri,
            relative_path: "docs/2026-07-12-reference-first-plan.md".into(),
        },
        title: "Zode Desktop Reference-First 实施计划".into(),
        content: concat!(
            "# Zode Desktop Reference-First 实施计划\n\n",
            "日期：2026-07-12\n\n",
            "## 目标\n\n",
            "以六张 1800×1080 桌面场景为硬验收，保留 Jian/Skia、真实 session、审批、Git diff 与 PTY。\n\n",
            "## 六个验收场景\n\n",
            "- Empty Task：英雄标记、四张建议卡、上下文栏与 Composer\n",
            "- Integrations：真实 registry 的已安装图标与分类条目\n",
            "- Settings：权限预设、常规设置与完整导航\n",
            "- Document Preview：长会话与 700px Markdown 预览\n",
            "- Artifacts：文件、附件、目标、活动与进度\n",
            "- Environment：变更、分支、子智能体、进程与来源\n\n",
            "## 完成定义\n\n",
            "三平台 golden、完整画布 overlay/heatmap、workspace tests、clippy、fmt 与 cargo-deny 全部通过。\n",
        )
        .into(),
        kind: PreviewKind::Markdown,
    };
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = Some(SecondaryPane::DocumentPreview);
    state.shell.page = ShellRoute::Conversation.legacy_page();
    ReferenceScene {
        name: "conversation-document-preview",
        state,
    }
}
