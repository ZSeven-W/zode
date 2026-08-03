use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, SubagentChip, SubagentChipPhase, TranscriptItem, TranscriptState,
};
use zode_app_ui::{Insets, ThreadTranscript, WorkspaceSnapshot, ZodeTheme};
use zode_node_protocol::SessionLocator;

#[derive(Default)]
struct TextCapture {
    texts: Vec<String>,
    round_fills: Vec<Rect>,
}

impl Painter for TextCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _origin: Point2D) {
        self.texts.push(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
        );
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, _color: Color) {
        self.round_fills.push(rect);
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

fn chip(phase: SubagentChipPhase, summary: Option<&str>) -> TranscriptItem {
    TranscriptItem::SubagentChip(SubagentChip {
        agent_id: "1".into(),
        display_name: "审查代码".into(),
        agent_type: "reviewer".into(),
        phase,
        summary: summary.map(str::to_owned),
        model: Some("claude-opus-5".into()),
    })
}

fn state_with(items: Vec<TranscriptItem>) -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "chips");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session,
        TranscriptState {
            items,
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    state
}

#[test]
fn a_running_chip_paints_its_headline_and_model_disclosure() {
    let state = state_with(vec![chip(SubagentChipPhase::Started, None)]);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    let painted = painter.texts.join("\n");
    assert!(
        painted.contains("审查代码 · 开始工作"),
        "missing chip headline: {painted}"
    );
    assert!(
        painted.contains("使用 claude-opus-5"),
        "missing model disclosure: {painted}"
    );
    assert_eq!(
        painter.round_fills.len(),
        1,
        "the chip paints exactly one leading dot"
    );
}

#[test]
fn a_finished_chip_shows_what_the_agent_reported() {
    let state = state_with(vec![chip(
        SubagentChipPhase::Finished,
        Some("已读取三个文件"),
    )]);
    let mut painter = TextCapture::default();

    ThreadTranscript::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 736.0, 400.0),
        &state,
        &ZodeTheme::light(),
    );

    let painted = painter.texts.join("\n");
    assert!(painted.contains("审查代码 · 已完成"), "{painted}");
    assert!(painted.contains("已读取三个文件"), "{painted}");
}

#[test]
fn chips_are_reachable_from_the_accessibility_tree_with_the_model_named() {
    let state = state_with(vec![chip(
        SubagentChipPhase::Finished,
        Some("已读取三个文件"),
    )]);

    let snapshot = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let names = snapshot
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        names.contains("审查代码 · 已完成；已读取三个文件；使用 claude-opus-5"),
        "missing chip a11y label: {names}"
    );
}

#[test]
fn a_chip_keeps_its_identity_when_earlier_items_are_inserted() {
    let mut state = state_with(vec![chip(SubagentChipPhase::Progress, None)]);
    let before = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);
    let before_id = chip_node_id(&before);

    let session = state.current_session.clone().unwrap();
    state
        .transcripts
        .get_mut(&session)
        .unwrap()
        .items
        .insert(0, TranscriptItem::user_text("再查一遍"));
    let after = WorkspaceSnapshot::build(&state, 1_440.0, 1_080.0, Insets::ZERO);

    assert_eq!(
        chip_node_id(&after),
        before_id,
        "a chip is keyed by its agent and phase, not by transcript position"
    );
}

fn chip_node_id(snapshot: &WorkspaceSnapshot) -> zode_app_ui::WidgetId {
    snapshot
        .nodes
        .iter()
        .find(|node| node.name.starts_with("审查代码 · 有进展"))
        .map(|node| node.id)
        .expect("the chip must be in the accessibility tree")
}
