use zode_app_model::{
    ActivityEntry, MessageQueueState, ShellRoute, ThemePreference, TranscriptItem,
};
use zode_node_protocol::{ThreadStatus, ToolCall, ToolStatus};

use super::ReferenceScene;
use crate::snapshot_support::fixture::{base_scene_state, set_transcript};

pub fn conversation_queue_scene(theme: ThemePreference, viewport_width: u32) -> ReferenceScene {
    let mut state = base_scene_state(theme, viewport_width);
    set_transcript(
        &mut state,
        vec![
            TranscriptItem::UserText("支持任务运行中的消息排队，并保持当前响应不中断。".into()),
            TranscriptItem::Thinking("正在梳理会话级 FIFO 队列与显式引导语义。".into()),
            TranscriptItem::Tool(ToolCall {
                id: "queue-model-audit".into(),
                name: "read_file".into(),
                status: ToolStatus::Completed,
                summary: "已核对消息队列模型与会话归属".into(),
                detail: Some("队列内容按会话隔离，运行结束后逐条提交".into()),
            }),
            TranscriptItem::AssistantText(
                "当前任务仍在运行。新消息会进入队列；只有点击“引导”才会提交到当前轮次。".into(),
            ),
            TranscriptItem::ActivityGroup(vec![
                ActivityEntry {
                    id: "queue-fifo".into(),
                    title: "FIFO 自动续跑".into(),
                    detail: Some("当前轮次结束后提交下一条".into()),
                    completed: true,
                },
                ActivityEntry {
                    id: "queue-actions".into(),
                    title: "队列行操作".into(),
                    detail: Some("引导、删除、编辑与关闭排队".into()),
                    completed: true,
                },
            ]),
            TranscriptItem::Status {
                code: "queue-ui".into(),
                message: "正在完成队列界面与交互回归".into(),
            },
        ],
        true,
    );

    let session = state
        .current_session
        .clone()
        .expect("queue scene has a current session");
    let current_thread = state
        .threads
        .iter_mut()
        .find(|thread| thread.session == session)
        .expect("queue scene has a current thread");
    current_thread.status = ThreadStatus::Running;

    let mut queue = MessageQueueState::default();
    queue
        .enqueue(
            "给 Zode 的截图，是让你在界面上进行像素级致敬，并对比整体密度".into(),
            Vec::new(),
        )
        .expect("first queue message is valid");
    queue
        .enqueue(
            "这里可以点击并选择项目，或者新建项目；进入 projectless 模式".into(),
            Vec::new(),
        )
        .expect("second queue message is valid");
    let menu_message = queue
        .enqueue(
            "菜单栏除新建任务外应整体可滚动，悬浮会话时展示操作".into(),
            Vec::new(),
        )
        .expect("third queue message is valid");
    queue
        .enqueue("缩放 APK 大小的时候，有卡顿需要继续优化".into(), Vec::new())
        .expect("fourth queue message is valid");
    state.message_queues.insert(session, queue);
    state.composer.queue_menu = Some(menu_message);
    state.composer.draft.clear();
    state.presentation.route = ShellRoute::Conversation;
    state.presentation.secondary_pane = None;
    state.shell.page = ShellRoute::Conversation.legacy_page();

    ReferenceScene {
        name: "conversation-queue",
        state,
    }
}
