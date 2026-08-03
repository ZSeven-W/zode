//! Rendering-level contract for the in-conversation find bar: the floating
//! surface must win hit tests over the transcript it covers, the counter must
//! reach the accessibility tree, and matched items must band with theme
//! tokens (current vs. secondary).

use accesskit::Role;
use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, TranscriptItem, TranscriptState, ZodeAppState};
use zode_app_ui::{
    accessibility_tree, Insets, ThreadTranscript, TranscriptFindBar, WorkspaceSnapshot, ZodeTheme,
    TRANSCRIPT_FIND_CLOSE_ID, TRANSCRIPT_FIND_INPUT_ID, TRANSCRIPT_FIND_NEXT_ID,
    TRANSCRIPT_FIND_PREVIOUS_ID, TRANSCRIPT_FIND_SURFACE_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Default)]
struct RoundFillCapture {
    fills: Vec<(Rect, Color)>,
}

impl Painter for RoundFillCapture {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, color: Color) {
        self.fills.push((rect, color));
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

fn state_with_find(query: &str) -> (ZodeAppState, SessionLocator) {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "find-task");
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        title: "查找".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::user_text("请修复 parser 里的空指针"),
                TranscriptItem::assistant_text("与 parser 无关的一段回复"),
                TranscriptItem::assistant_text("完全无关的一段回复"),
            ],
            follow_tail: false,
            ..TranscriptState::default()
        },
    );
    state.current_session = Some(session.clone());
    let find = &mut state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .find;
    find.open = true;
    find.query = query.to_owned();
    (state, session)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn the_bar_wins_hit_tests_over_the_transcript_it_floats_above() {
    let (state, _) = state_with_find("parser");
    let snapshot = WorkspaceSnapshot::build(&state, 1_600.0, 1_000.0, Insets::ZERO);
    let bar = TranscriptFindBar::layout(snapshot.layout.transcript, &state)
        .expect("the bar is open on a wide window");

    for id in [
        TRANSCRIPT_FIND_INPUT_ID,
        TRANSCRIPT_FIND_PREVIOUS_ID,
        TRANSCRIPT_FIND_NEXT_ID,
        TRANSCRIPT_FIND_CLOSE_ID,
    ] {
        let rect = match id {
            TRANSCRIPT_FIND_INPUT_ID => bar.input,
            TRANSCRIPT_FIND_PREVIOUS_ID => bar.previous,
            TRANSCRIPT_FIND_NEXT_ID => bar.next,
            _ => bar.close,
        };
        assert_eq!(
            snapshot.hit_test(center(rect)),
            Some(id),
            "the bar control at {rect:?} must not fall through to the transcript"
        );
    }
    assert!(
        snapshot.hit_test(center(bar.surface)).is_some(),
        "the surface itself blocks clicks from reaching messages behind it"
    );
}

#[test]
fn the_accessibility_tree_carries_the_counter_and_the_step_controls() {
    let (state, _) = state_with_find("parser");
    let snapshot = WorkspaceSnapshot::build(&state, 1_600.0, 1_000.0, Insets::ZERO);
    let update = accessibility_tree(&snapshot, 1.0);
    let labels = update
        .nodes
        .iter()
        .map(|(_, node)| {
            (
                node.role(),
                node.label().map(str::to_owned).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    assert!(
        labels
            .iter()
            .any(|(role, label)| *role == Role::Group && label == "在对话中查找，1/2"),
        "the group name folds in the live counter: {labels:?}"
    );
    assert!(labels
        .iter()
        .any(|(role, label)| *role == Role::Button && label == "下一个匹配"));
    assert!(labels
        .iter()
        .any(|(role, label)| *role == Role::Button && label == "上一个匹配"));
    assert!(labels
        .iter()
        .any(|(role, label)| *role == Role::Button && label == "关闭查找"));
    assert!(labels.iter().any(|(role, _)| *role == Role::SearchInput));
}

#[test]
fn a_query_with_no_matches_leaves_the_steppers_unclickable() {
    let (state, _) = state_with_find("没有这个词");
    let snapshot = WorkspaceSnapshot::build(&state, 1_600.0, 1_000.0, Insets::ZERO);
    let bar = TranscriptFindBar::layout(snapshot.layout.transcript, &state).unwrap();

    assert_eq!(bar.counter_label, "0/0");
    assert!(!bar.navigable);
    // No actions means `hit_test` skips the node, so the click falls through
    // to the bar's own blocking surface rather than acting.
    assert_eq!(
        snapshot.hit_test(center(bar.next)),
        Some(TRANSCRIPT_FIND_SURFACE_ID)
    );
    assert_eq!(
        snapshot.hit_test(center(bar.close)),
        Some(TRANSCRIPT_FIND_CLOSE_ID),
        "close stays reachable even with nothing to step through"
    );
}

#[test]
fn matched_items_band_with_current_and_secondary_theme_tokens() {
    let (state, _) = state_with_find("parser");
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 700.0, 900.0);
    let mut painter = RoundFillCapture::default();
    ThreadTranscript::paint_with_hovered(&mut painter, rect, rect, &state, None, &theme);

    let current = painter
        .fills
        .iter()
        .filter(|(_, color)| *color == theme.tokens.row_selected_primary)
        .count();
    let secondary = painter
        .fills
        .iter()
        .filter(|(_, color)| *color == theme.tokens.row_selected)
        .count();
    assert_eq!(
        current, 1,
        "exactly one item carries the current-match band"
    );
    assert_eq!(
        secondary, 1,
        "the other matched item carries the secondary band"
    );
}

#[test]
fn a_closed_bar_paints_no_bands_at_all() {
    let (mut state, session) = state_with_find("parser");
    state
        .presentation
        .sessions
        .get_mut(&session)
        .unwrap()
        .find
        .open = false;
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 700.0, 900.0);
    let mut painter = RoundFillCapture::default();
    ThreadTranscript::paint_with_hovered(&mut painter, rect, rect, &state, None, &theme);

    assert!(!painter
        .fills
        .iter()
        .any(|(_, color)| *color == theme.tokens.row_selected_primary
            || *color == theme.tokens.row_selected));
    assert!(TranscriptFindBar::layout(rect, &state).is_none());
}
