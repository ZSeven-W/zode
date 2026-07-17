use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use std::time::{Duration, Instant};
use zode_app_model::{demo_state, AttachmentMetadata, TranscriptItem, TranscriptState};
use zode_app_ui::{ThreadTranscript, ZodeTheme};
use zode_node_protocol::{SessionLocator, TurnId};

#[derive(Debug, Clone, PartialEq)]
struct CapturedText {
    text: String,
    origin: Point2D,
    family: String,
    size: f32,
}

#[derive(Default)]
struct TextCapture {
    texts: Vec<CapturedText>,
    round_fills: Vec<(Rect, f32)>,
    lines: Vec<(Point2D, Point2D)>,
}

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        self.texts.push(CapturedText {
            text: layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
            origin,
            family: layout
                .runs()
                .first()
                .map(|run| run.font_family.clone())
                .unwrap_or_default(),
            size: layout
                .runs()
                .first()
                .map(|run| run.font_size)
                .unwrap_or_default(),
        });
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, from: Point2D, to: Point2D, _color: Color, _width: f32) {
        self.lines.push((from, to));
    }
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, _color: Color) {
        self.round_fills.push((rect, radius));
    }
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

fn state_with_transcript(transcript: TranscriptState) -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "paint-thread");
    state.current_session = Some(session.clone());
    state.transcripts.insert(session, transcript);
    state
}

#[test]
fn transcript_parses_agent_markdown_and_right_aligns_user_bubble() {
    let state = state_with_transcript(TranscriptState {
        items: vec![
            TranscriptItem::AssistantText(
                "### Result\n\nUse **bold** and `code` in the answer.".into(),
            ),
            TranscriptItem::UserText("ship it".into()),
        ],
        ..TranscriptState::default()
    });
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 600.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|item| item.text == "Result"));
    assert!(painter.texts.iter().any(|item| item.text == "bold"));
    assert!(!painter.texts.iter().any(|item| item.text.contains("###")));
    let user = painter
        .texts
        .iter()
        .find(|item| item.text == "ship it")
        .expect("user message should be painted");
    assert!(user.origin.x > 300.0);
}

#[test]
fn transcript_only_paints_the_tail_when_scrolled_beyond_long_history() {
    let mut items = (0..32)
        .map(|index| TranscriptItem::UserText(format!("hidden-{index}")))
        .collect::<Vec<_>>();
    items.push(TranscriptItem::UserText("tail-visible".into()));
    let state = state_with_transcript(TranscriptState {
        items,
        scroll_offset: 10_000.0,
        follow_tail: false,
        ..TranscriptState::default()
    });
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 600.0, 80.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|item| item.text == "tail-visible"));
    assert!(!painter.texts.iter().any(|item| item.text == "hidden-0"));
}

#[test]
fn completed_turn_places_a_summary_divider_between_user_and_assistant() {
    let started_at = Instant::now();
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000151").unwrap();
    let mut transcript = TranscriptState {
        items: vec![
            TranscriptItem::UserText("请继续实现".into()),
            TranscriptItem::AssistantText("已经完成。".into()),
        ],
        busy: false,
        ..TranscriptState::default()
    };
    assert!(transcript.begin_turn_at(turn_id, 0, 1, started_at));
    assert!(transcript.finish_turn_at(
        turn_id,
        false,
        started_at + Duration::from_secs(20 * 60 + 51),
    ));
    let state = state_with_transcript(transcript);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    let user_y = painter
        .texts
        .iter()
        .find(|text| text.text == "请继续实现")
        .unwrap()
        .origin
        .y;
    let summary_y = painter
        .texts
        .iter()
        .find(|text| text.text == "已处理 20m 51s")
        .unwrap()
        .origin
        .y;
    let assistant_y = painter
        .texts
        .iter()
        .find(|text| text.text == "已经完成。")
        .unwrap()
        .origin
        .y;

    assert!(user_y < summary_y && summary_y < assistant_y);
    assert_eq!(painter.lines.len(), 1, "turn summary needs one divider");
    let (bubble, radius) = painter.round_fills[0];
    assert_eq!(bubble.size.y, 40.0);
    assert_eq!(radius, 16.0);
    assert!(bubble.size.x <= 736.0 * 0.77);
}

#[test]
fn one_runtime_turn_paints_one_divider_after_its_last_initial_user_block() {
    let started_at = Instant::now();
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000152").unwrap();
    let mut transcript = TranscriptState {
        items: vec![
            TranscriptItem::UserText("第一段输入".into()),
            TranscriptItem::UserText("第二段输入".into()),
            TranscriptItem::AssistantText("处理中。".into()),
            TranscriptItem::UserText("运行中追加的引导".into()),
        ],
        ..TranscriptState::default()
    };
    assert!(transcript.begin_turn_at(turn_id, 0, 2, started_at));
    assert!(transcript.finish_turn_at(turn_id, false, started_at + Duration::from_secs(39),));
    let state = state_with_transcript(transcript);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 600.0),
        &state,
        &ZodeTheme::light(),
    );

    assert_eq!(
        painter
            .texts
            .iter()
            .filter(|text| text.text == "已处理 39s")
            .count(),
        1
    );
    assert_eq!(painter.lines.len(), 1, "one runtime turn needs one divider");
    let second_user_y = painter
        .texts
        .iter()
        .find(|text| text.text == "第二段输入")
        .unwrap()
        .origin
        .y;
    let summary_y = painter
        .texts
        .iter()
        .find(|text| text.text == "已处理 39s")
        .unwrap()
        .origin
        .y;
    let assistant_y = painter
        .texts
        .iter()
        .find(|text| text.text == "处理中。")
        .unwrap()
        .origin
        .y;
    assert!(second_user_y < summary_y && summary_y < assistant_y);
}

#[test]
fn turn_divider_follows_text_and_attachment_as_one_user_submission() {
    let started_at = Instant::now();
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000153").unwrap();
    let attachment = AttachmentMetadata {
        id: "shot".into(),
        path: None,
        display_name: "shot.png".into(),
        media_type: "image/png".into(),
        width: Some(640),
        height: Some(360),
        byte_len: 1_024,
    };
    let mut transcript = TranscriptState {
        items: vec![
            TranscriptItem::UserText("请对照这张图".into()),
            TranscriptItem::Attachment(attachment),
            TranscriptItem::AssistantText("收到。".into()),
        ],
        ..TranscriptState::default()
    };
    assert!(transcript.begin_turn_at(turn_id, 0, 2, started_at));
    assert!(transcript.finish_turn_at(turn_id, false, started_at + Duration::from_secs(39),));
    let state = state_with_transcript(transcript);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 600.0),
        &state,
        &ZodeTheme::light(),
    );

    let y = |text: &str| {
        painter
            .texts
            .iter()
            .find(|item| item.text == text)
            .unwrap_or_else(|| panic!("missing text: {text}"))
            .origin
            .y
    };
    assert!(y("请对照这张图") < y("shot.png"));
    assert!(y("shot.png") < y("已处理 39s"));
    assert!(y("已处理 39s") < y("收到。"));
    assert_eq!(painter.lines.len(), 1);
}

#[test]
fn attachment_only_submission_still_owns_a_turn_divider() {
    let started_at = Instant::now();
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000154").unwrap();
    let mut transcript = TranscriptState {
        items: vec![
            TranscriptItem::Attachment(AttachmentMetadata {
                id: "shot-only".into(),
                path: None,
                display_name: "only.png".into(),
                media_type: "image/png".into(),
                width: Some(640),
                height: Some(360),
                byte_len: 1_024,
            }),
            TranscriptItem::AssistantText("已查看。".into()),
        ],
        ..TranscriptState::default()
    };
    assert!(transcript.begin_turn_at(turn_id, 0, 1, started_at));
    assert!(transcript.finish_turn_at(turn_id, false, started_at + Duration::from_secs(2),));
    let state = state_with_transcript(transcript);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    assert!(painter.texts.iter().any(|item| item.text == "only.png"));
    assert!(painter.texts.iter().any(|item| item.text == "已处理 2s"));
    assert_eq!(painter.lines.len(), 1);
}

#[test]
fn assistant_markdown_uses_body_type_inline_pills_and_monospace_code_blocks() {
    let state = state_with_transcript(TranscriptState {
        items: vec![TranscriptItem::AssistantText(
            "正文包含 `cargo test`。\n\n```bash\ncargo test --workspace\n```".into(),
        )],
        ..TranscriptState::default()
    });
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    let body = painter
        .texts
        .iter()
        .find(|text| text.text.contains("正文包含"))
        .unwrap();
    assert_eq!(body.size, 14.0);
    let code = painter
        .texts
        .iter()
        .find(|text| text.text == "cargo test --workspace")
        .unwrap();
    assert_eq!(code.family, "monospace");
    assert_eq!(code.size, 12.0);
    assert!(
        painter.round_fills.len() >= 2,
        "inline code and fenced code both need muted surfaces"
    );
}
