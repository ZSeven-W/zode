use super::{TranscriptItem, TranscriptState};

#[test]
fn removing_an_approval_keeps_item_heights_and_turn_boundaries_aligned() {
    let mut transcript = TranscriptState {
        items: vec![
            TranscriptItem::UserText("first".into()),
            TranscriptItem::Approval {
                id: "approval-1".into(),
                tool: "shell".into(),
            },
            TranscriptItem::AssistantText("done".into()),
            TranscriptItem::UserText("second".into()),
            TranscriptItem::AssistantText("done again".into()),
        ],
        item_heights: vec![1.0, 2.0, 3.0, 4.0, 5.0],
        ..TranscriptState::default()
    };
    transcript.restore_historical_turns();

    assert!(transcript.remove_approval("approval-1"));

    assert_eq!(transcript.items.len(), 4);
    assert_eq!(transcript.item_heights, vec![1.0, 3.0, 4.0, 5.0]);
    assert_eq!(transcript.turns[0].start_item_index, 0);
    assert_eq!(transcript.turns[0].response_item_index, 1);
    assert_eq!(transcript.turns[0].end_item_index, Some(2));
    assert_eq!(transcript.turns[1].start_item_index, 2);
    assert_eq!(transcript.turns[1].response_item_index, 3);
    assert_eq!(transcript.turns[1].end_item_index, Some(4));
    assert!(!transcript.remove_approval("approval-1"));
}
