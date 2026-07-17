use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, ActivityEntry, AttachmentMetadata, FileArtifact, GoalProgress, TranscriptItem,
    TranscriptState,
};
use zode_app_ui::{Insets, ThreadTranscript, WorkspaceSnapshot, ZodeTheme};
use zode_node_protocol::{SessionLocator, ToolCall, ToolStatus};

#[derive(Default)]
struct TextCapture(Vec<String>);

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.0.push(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        );
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color) {}
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn rich_state() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "rich");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session,
        TranscriptState {
            items: vec![
                TranscriptItem::AssistantText("## Result\n\nReady".into()),
                TranscriptItem::ActivityGroup(vec![ActivityEntry {
                    id: "activity-1".into(),
                    title: "Ran tests".into(),
                    detail: Some("12 passed".into()),
                    completed: true,
                }]),
                TranscriptItem::Tool(ToolCall {
                    id: "tool-1".into(),
                    name: "cargo_test".into(),
                    status: ToolStatus::Completed,
                    summary: "Tests passed".into(),
                    detail: None,
                }),
                TranscriptItem::FileArtifact(FileArtifact {
                    id: "file-1".into(),
                    path: "crates/zode-app-ui/src/widgets/transcript/mod.rs".into(),
                    summary: "Transcript updated".into(),
                    change_summary: Some("+12 -4".into()),
                }),
                TranscriptItem::Attachment(AttachmentMetadata {
                    id: "attachment-1".into(),
                    path: None,
                    display_name: "reference.png".into(),
                    media_type: "image/png".into(),
                    width: Some(640),
                    height: Some(360),
                    byte_len: 8_192,
                }),
                TranscriptItem::GoalProgress(GoalProgress {
                    id: "goal-1".into(),
                    title: "Visual rebuild".into(),
                    completed: 3,
                    total: 7,
                }),
                TranscriptItem::Error {
                    message: "Retry available".into(),
                    retryable: true,
                },
            ],
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    state
}

#[test]
fn rich_transcript_paints_artifact_activity_attachment_goal_and_error_cards() {
    let state = rich_state();
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 1_000.0),
        &state,
        &ZodeTheme::light(),
    );

    let text = painter.0.join("\n");
    for expected in [
        "Ran tests",
        "Transcript updated",
        "reference.png",
        "Visual rebuild",
        "Retry available",
    ] {
        assert!(text.contains(expected), "missing painted text: {expected}");
    }
}

#[test]
fn rich_transcript_accessibility_names_every_visual_variant() {
    let state = rich_state();
    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let names = snapshot
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for expected in [
        "Ran tests",
        "Transcript updated",
        "reference.png",
        "Visual rebuild",
        "Retry available",
    ] {
        assert!(names.contains(expected), "missing a11y label: {expected}");
    }
}

#[test]
fn rich_artifact_accessibility_ids_survive_earlier_transcript_insertions() {
    let mut state = rich_state();
    let before = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let ids = [
        "活动：Ran tests：12 passed",
        "文件：Transcript updated",
        "附件：reference.png",
        "目标：Visual rebuild",
    ]
    .into_iter()
    .map(|prefix| {
        before
            .nodes
            .iter()
            .find(|node| node.name.starts_with(prefix))
            .map(|node| (prefix, node.id))
            .unwrap_or_else(|| panic!("missing rich node: {prefix}"))
    })
    .collect::<Vec<_>>();

    let session = state.current_session.clone().unwrap();
    state
        .transcripts
        .get_mut(&session)
        .unwrap()
        .items
        .insert(0, TranscriptItem::UserText("prepended".into()));
    let after = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);

    for (prefix, before_id) in ids {
        let after_id = after
            .nodes
            .iter()
            .find(|node| node.name.starts_with(prefix))
            .map(|node| node.id)
            .unwrap_or_else(|| panic!("missing rich node after insert: {prefix}"));
        assert_eq!(after_id, before_id, "unstable rich id: {prefix}");
    }
}
