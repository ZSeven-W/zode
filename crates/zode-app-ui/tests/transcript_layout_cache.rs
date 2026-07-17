//! Covers the cumulative-layout memoization added to
//! `ThreadTranscript::visible_item_layout_with_tools` /
//! `ThreadTranscript::scroll_command`. Both call sites (and, transitively,
//! the paint path and the accessibility-tree builder) route through
//! `TranscriptState::layout_offsets`, so `layout_recompute_count` is a
//! reliable probe for "did the expensive per-item pass actually run".

use std::collections::BTreeMap;

use jian_widgets::Rect;
use zode_app_model::{demo_state, AppCommand, TranscriptItem, TranscriptState};
use zode_app_ui::ThreadTranscript;
use zode_node_protocol::SessionLocator;

fn fixture() -> TranscriptState {
    TranscriptState {
        items: (0..12)
            .map(|index| TranscriptItem::user_text(format!("message {index}")))
            .collect(),
        ..TranscriptState::default()
    }
}

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, 360.0, 240.0)
}

#[test]
fn repeated_calls_with_unchanged_inputs_recompute_only_once() {
    let transcript = fixture();
    let tool_expanded = BTreeMap::new();

    let first =
        ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);
    let second =
        ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    assert_eq!(first, second);
    assert_eq!(
        transcript.layout_recompute_count(),
        1,
        "identical viewport/content across calls (e.g. a sidebar-animation or \
         resize-settle tick that never touches this transcript) must reuse the cache"
    );
}

#[test]
fn paint_and_accessibility_share_one_computation_within_a_frame() {
    let transcript = fixture();
    let tool_expanded = BTreeMap::new();

    // Simulates the two independent production call sites that both run
    // within the same rendered frame: the accessibility-tree builder
    // (accessibility/transcript.rs) and the paint path
    // (widgets/transcript/mod.rs), both against the same viewport.
    let accessibility_pass =
        ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);
    let paint_pass =
        ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    assert_eq!(accessibility_pass, paint_pass);
    assert_eq!(
        transcript.layout_recompute_count(),
        1,
        "the second same-frame caller must reuse the first caller's computation"
    );
}

#[test]
fn appending_an_item_forces_a_recompute() {
    let mut transcript = fixture();
    let tool_expanded = BTreeMap::new();
    ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    transcript
        .items
        .push(TranscriptItem::assistant_text("streamed reply"));

    ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    assert_eq!(transcript.layout_recompute_count(), 2);
}

#[test]
fn width_change_forces_a_recompute() {
    let transcript = fixture();
    let tool_expanded = BTreeMap::new();
    ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    let wider = Rect::xywh(0.0, 0.0, 720.0, 240.0);
    ThreadTranscript::visible_item_layout_with_tools(wider, &transcript, &tool_expanded);

    assert_eq!(transcript.layout_recompute_count(), 2);
}

#[test]
fn scroll_only_change_never_recomputes() {
    let mut transcript = fixture();
    let tool_expanded = BTreeMap::new();
    ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    transcript.follow_tail = false;
    transcript.scroll_offset = 500.0;
    ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &tool_expanded);

    let session = SessionLocator::new(demo_state().host.node_id, "session");
    let command =
        ThreadTranscript::scroll_command(session, viewport(), &transcript, &tool_expanded, -20.0);
    assert!(matches!(command, AppCommand::SetTranscriptViewport { .. }));

    assert_eq!(
        transcript.layout_recompute_count(),
        1,
        "scrolling (including via scroll_command, which shares the same cache) \
         must never invalidate cumulative layout - only the visible slice changes"
    );
}

#[test]
fn tool_expand_toggle_forces_a_recompute() {
    let mut transcript = TranscriptState {
        items: vec![TranscriptItem::Tool(zode_node_protocol::ToolCall {
            id: "tool-1".into(),
            name: "read_file".into(),
            status: zode_node_protocol::ToolStatus::Completed,
            summary: "Read the source".into(),
            detail: None,
        })],
        ..TranscriptState::default()
    };
    let collapsed = BTreeMap::from([("tool-1".to_string(), false)]);
    let expanded = BTreeMap::from([("tool-1".to_string(), true)]);

    let closed =
        ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &collapsed);
    transcript.touch_layout();
    let open = ThreadTranscript::visible_item_layout_with_tools(viewport(), &transcript, &expanded);

    assert_ne!(
        closed[0].rect.size.y, open[0].rect.size.y,
        "expanding a tool card must change its estimated height"
    );
    assert_eq!(transcript.layout_recompute_count(), 2);
}
