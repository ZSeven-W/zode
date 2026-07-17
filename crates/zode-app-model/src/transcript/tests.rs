use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use super::{
    ActivityEntry, AttachmentMetadata, FileArtifact, GoalProgress, TranscriptItem,
    TranscriptTurnStatus, TranscriptVisualKind,
};
use zode_node_protocol::{ToolCall, ToolStatus, TurnId};

fn rich_transcript_fixture() -> Vec<TranscriptItem> {
    vec![
        TranscriptItem::user_text("Please update the renderer"),
        TranscriptItem::assistant_text("## Done\n\nUpdated the renderer."),
        TranscriptItem::Thinking("Checking the layout".into()),
        TranscriptItem::ActivityGroup(vec![ActivityEntry {
            id: "activity-1".into(),
            title: "Ran tests".into(),
            detail: Some("12 passed".into()),
            completed: true,
        }]),
        TranscriptItem::Tool(ToolCall {
            id: "tool-1".into(),
            name: "read_file".into(),
            status: ToolStatus::Completed,
            summary: "Read the source".into(),
            detail: None,
        }),
        TranscriptItem::FileArtifact(FileArtifact {
            id: "file-1".into(),
            path: "crates/zode-app-ui/src/widgets/transcript/mod.rs".into(),
            summary: "Updated transcript rendering".into(),
            change_summary: Some("+120 -14".into()),
        }),
        TranscriptItem::Attachment(AttachmentMetadata {
            id: "attachment-1".into(),
            path: None,
            display_name: "layout.png".into(),
            media_type: "image/png".into(),
            width: Some(1280),
            height: Some(720),
            byte_len: 42_000,
        }),
        TranscriptItem::GoalProgress(GoalProgress {
            id: "goal-1".into(),
            title: "Reference rebuild".into(),
            completed: 3,
            total: 7,
        }),
        TranscriptItem::Approval {
            id: "approval-1".into(),
            tool: "shell".into(),
        },
        TranscriptItem::Status {
            code: "running".into(),
            message: "Still working".into(),
        },
        TranscriptItem::Error {
            message: "Build failed".into(),
            retryable: true,
        },
    ]
}

#[test]
fn rich_transcript_exposes_five_visual_card_kinds() {
    let kinds = rich_transcript_fixture()
        .iter()
        .map(TranscriptItem::visual_kind)
        .collect::<BTreeSet<_>>();

    assert!(kinds.len() >= 5);
}

#[test]
fn transcript_visual_kind_maps_every_variant() {
    let kinds = rich_transcript_fixture()
        .iter()
        .map(TranscriptItem::visual_kind)
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            TranscriptVisualKind::UserMarkdown,
            TranscriptVisualKind::AssistantMarkdown,
            TranscriptVisualKind::Thinking,
            TranscriptVisualKind::Activity,
            TranscriptVisualKind::Tool,
            TranscriptVisualKind::FileArtifact,
            TranscriptVisualKind::Attachment,
            TranscriptVisualKind::GoalProgress,
            TranscriptVisualKind::Approval,
            TranscriptVisualKind::Status,
            TranscriptVisualKind::Error,
        ]
    );
}

#[test]
fn replacing_rich_activity_or_goal_invalidates_cached_height() {
    let mut transcript = super::TranscriptState {
        items: vec![
            TranscriptItem::ActivityGroup(vec![ActivityEntry {
                id: "activity-1".into(),
                title: "Running".into(),
                detail: None,
                completed: false,
            }]),
            TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Rebuild".into(),
                completed: 1,
                total: 4,
            }),
        ],
        item_heights: vec![64.0, 72.0],
        ..super::TranscriptState::default()
    };

    assert!(transcript.replace_item(
        0,
        TranscriptItem::ActivityGroup(vec![ActivityEntry {
            id: "activity-1".into(),
            title: "Complete".into(),
            detail: Some("done".into()),
            completed: true,
        }]),
    ));
    assert!(transcript.replace_item(
        1,
        TranscriptItem::GoalProgress(GoalProgress {
            id: "goal-1".into(),
            title: "Rebuild".into(),
            completed: 2,
            total: 4,
        }),
    ));
    assert_eq!(transcript.item_heights, [0.0, 0.0]);
}

#[test]
fn live_turn_freezes_real_monotonic_elapsed_at_completion() {
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000111").unwrap();
    let started_at = Instant::now();
    let mut transcript = super::TranscriptState {
        items: vec![TranscriptItem::user_text("ship it")],
        ..super::TranscriptState::default()
    };

    assert!(transcript.begin_turn_at(turn_id, 0, 1, started_at));
    assert_eq!(
        transcript.turns[0].elapsed_at(started_at + Duration::from_secs(2)),
        Some(Duration::from_secs(2))
    );
    transcript
        .items
        .push(TranscriptItem::assistant_text("done"));
    assert!(transcript.finish_turn_at(turn_id, false, started_at + Duration::from_millis(3_250),));

    let turn = &transcript.turns[0];
    assert_eq!(turn.status, TranscriptTurnStatus::Completed);
    assert_eq!(turn.start_item_index, 0);
    assert_eq!(turn.response_item_index, 1);
    assert_eq!(turn.end_item_index, Some(2));
    assert_eq!(turn.elapsed, Some(Duration::from_millis(3_250)));
    assert!(!transcript.busy);
}

#[test]
fn restored_turns_never_invent_ids_status_edges_or_elapsed_time() {
    let mut transcript = super::TranscriptState {
        items: vec![
            TranscriptItem::user_text("first block"),
            TranscriptItem::user_text("second block"),
            TranscriptItem::assistant_text("first answer"),
            TranscriptItem::user_text("next turn"),
            TranscriptItem::assistant_text("second answer"),
        ],
        ..super::TranscriptState::default()
    };

    transcript.restore_historical_turns();

    assert_eq!(transcript.turns.len(), 2);
    assert_eq!(transcript.turns[0].start_item_index, 0);
    assert_eq!(transcript.turns[0].response_item_index, 2);
    assert_eq!(transcript.turns[0].end_item_index, Some(3));
    assert_eq!(transcript.turns[1].start_item_index, 3);
    assert_eq!(transcript.turns[1].response_item_index, 4);
    assert_eq!(transcript.turns[1].end_item_index, Some(5));
    assert!(transcript.turns.iter().all(|turn| {
        turn.turn_id.is_none()
            && turn.status == TranscriptTurnStatus::Restored
            && turn.elapsed.is_none()
    }));
}

#[test]
fn undispatched_turn_can_be_discarded_without_leaving_an_orphan_boundary() {
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000222").unwrap();
    let mut transcript = super::TranscriptState {
        items: vec![TranscriptItem::user_text("queued")],
        ..super::TranscriptState::default()
    };

    assert!(transcript.begin_turn(turn_id, 0, 1));
    transcript.items.clear();
    assert!(transcript.discard_turn(turn_id));

    assert!(transcript.turns.is_empty());
    assert!(!transcript.busy);
    assert!(!transcript.discard_turn(turn_id));
}

#[test]
fn late_user_artifacts_are_inserted_before_runtime_output_and_the_divider() {
    let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000223").unwrap();
    let mut transcript = super::TranscriptState {
        items: vec![TranscriptItem::user_text("with attachment")],
        ..super::TranscriptState::default()
    };
    assert!(transcript.begin_turn(turn_id, 0, 1));
    assert!(transcript.fail_turn(turn_id));
    transcript.items.push(TranscriptItem::Error {
        message: "dispatch failed".into(),
        retryable: true,
    });

    assert!(
        transcript.insert_latest_turn_user_items(vec![TranscriptItem::Attachment(
            super::AttachmentMetadata {
                id: "shot".into(),
                path: None,
                display_name: "shot.png".into(),
                media_type: "image/png".into(),
                width: Some(640),
                height: Some(360),
                byte_len: 1_024,
            },
        )])
    );

    assert_eq!(transcript.turns[0].response_item_index, 2);
    assert_eq!(transcript.turns[0].end_item_index, Some(2));
    assert!(matches!(
        transcript.items.as_slice(),
        [
            TranscriptItem::UserText { .. },
            TranscriptItem::Attachment(_),
            TranscriptItem::Error { .. }
        ]
    ));
}

fn layout_fixture() -> super::TranscriptState {
    super::TranscriptState {
        items: vec![
            TranscriptItem::user_text("first"),
            TranscriptItem::assistant_text("second"),
            TranscriptItem::Thinking("third".into()),
        ],
        item_heights: vec![10.0, 20.0, 30.0],
        ..super::TranscriptState::default()
    }
}

fn trivial_offsets(transcript: &super::TranscriptState) -> (Vec<(f32, f32)>, f32) {
    let mut top = 0.0;
    let offsets = transcript
        .item_heights
        .iter()
        .map(|height| {
            let bottom = top + height;
            let entry = (top, bottom);
            top = bottom;
            entry
        })
        .collect();
    (offsets, top)
}

#[test]
fn layout_offsets_reuses_the_cache_when_nothing_changed() {
    let transcript = layout_fixture();

    let first = transcript.layout_offsets(400.0, || trivial_offsets(&transcript));
    assert_eq!(first.total_height, 60.0);
    drop(first);

    let second = transcript.layout_offsets(400.0, || trivial_offsets(&transcript));
    assert_eq!(second.total_height, 60.0);
    drop(second);

    assert_eq!(
        transcript.layout_recompute_count(),
        1,
        "same width and unchanged content must recompute exactly once"
    );
}

#[test]
fn layout_offsets_recomputes_after_touch_layout() {
    let mut transcript = layout_fixture();
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));

    transcript.touch_layout();
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));

    assert_eq!(transcript.layout_recompute_count(), 2);
}

#[test]
fn layout_offsets_recomputes_when_viewport_width_changes() {
    let transcript = layout_fixture();
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));
    drop(transcript.layout_offsets(500.0, || trivial_offsets(&transcript)));

    assert_eq!(transcript.layout_recompute_count(), 2);
}

/// `items`/`item_heights` are `pub` (existing fixtures build `TranscriptState`
/// literals and mutate them directly), so nothing forces every mutation
/// through `touch_layout`. `layout_offsets` must still notice a direct
/// mutation instead of serving a stale cache - this reproduces the bug two
/// accessibility tests hit when they replaced `transcript.items` in place.
#[test]
fn layout_offsets_notices_direct_item_mutation_that_bypasses_touch_layout() {
    let mut transcript = layout_fixture();
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));

    transcript.items.push(TranscriptItem::Status {
        code: "after".into(),
        message: "after".into(),
    });
    transcript.item_heights.push(40.0);

    let second = transcript.layout_offsets(400.0, || trivial_offsets(&transcript));
    assert_eq!(second.total_height, 100.0);
    drop(second);

    assert_eq!(transcript.layout_recompute_count(), 2);
}

#[test]
fn layout_offsets_ignores_scroll_only_changes() {
    let mut transcript = layout_fixture();
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));

    transcript.scroll_offset = 123.0;
    transcript.follow_tail = false;
    drop(transcript.layout_offsets(400.0, || trivial_offsets(&transcript)));

    assert_eq!(
        transcript.layout_recompute_count(),
        1,
        "scroll position must never affect cumulative layout"
    );
}

fn completed_tool(name: &str, path: &str) -> ToolCall {
    ToolCall {
        id: "tool-image-1".into(),
        name: name.into(),
        status: ToolStatus::Completed,
        summary: format!("{name} path={path}"),
        detail: None,
    }
}

#[test]
fn a_completed_file_read_of_a_recognized_image_extension_attributes_an_image_item() {
    let tool = completed_tool("FileRead", "/repo/assets/logo.PNG");
    let item = TranscriptItem::image_from_completed_tool(&tool)
        .expect("a completed FileRead over a .png path should attribute an image");
    match item {
        TranscriptItem::Image(image) => {
            assert_eq!(image.path, "/repo/assets/logo.PNG");
            assert_eq!(image.media_type, "image/png");
            assert_eq!(image.width, None);
            assert_eq!(image.height, None);
        }
        other => panic!("expected an Image item, got {other:?}"),
    }
}

#[test]
fn file_write_and_file_edit_also_attribute_images() {
    for name in ["FileWrite", "FileEdit"] {
        let tool = completed_tool(name, "/tmp/shot.jpeg");
        assert!(
            matches!(
                TranscriptItem::image_from_completed_tool(&tool),
                Some(TranscriptItem::Image(_))
            ),
            "{name} over a .jpeg path should attribute an image"
        );
    }
}

#[test]
fn a_non_image_extension_never_attributes_an_image() {
    let tool = completed_tool("FileRead", "/repo/src/main.rs");
    assert_eq!(TranscriptItem::image_from_completed_tool(&tool), None);
}

#[test]
fn a_failed_call_never_attributes_an_image_even_over_an_image_path() {
    let mut tool = completed_tool("FileRead", "/repo/assets/logo.png");
    tool.status = ToolStatus::Failed;
    assert_eq!(TranscriptItem::image_from_completed_tool(&tool), None);
}

#[test]
fn an_unrelated_tool_never_attributes_an_image_even_over_an_image_looking_summary() {
    let tool = completed_tool("WebFetch", "/repo/assets/logo.png");
    assert_eq!(TranscriptItem::image_from_completed_tool(&tool), None);
}
