use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, TranscriptItem, TranscriptState};
use zode_app_ui::{ThreadTranscript, ZodeTheme};
use zode_node_protocol::SessionLocator;

#[derive(Debug, Clone, PartialEq)]
struct CapturedText {
    text: String,
    origin: Point2D,
}

#[derive(Default)]
struct TextCapture {
    texts: Vec<CapturedText>,
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
        });
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
