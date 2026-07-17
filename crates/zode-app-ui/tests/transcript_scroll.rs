use zode_app_ui::{visible_range, MeasuredItem, VirtualListState};

#[test]
fn visible_range_keeps_one_overscan_item_above_viewport() {
    let items = vec![
        MeasuredItem::new(0.0, 40.0),
        MeasuredItem::new(40.0, 100.0),
        MeasuredItem::new(100.0, 180.0),
        MeasuredItem::new(180.0, 260.0),
    ];

    assert_eq!(visible_range(&items, 110.0, 80.0), 1..4);
}

#[test]
fn visible_range_is_empty_for_an_empty_transcript() {
    assert_eq!(visible_range(&[], 0.0, 400.0), 0..0);
}

#[test]
fn streaming_keeps_tail_only_when_user_was_at_tail() {
    let mut detached = VirtualListState::at_offset(100.0, false);
    detached.content_grew(300.0, 320.0);
    assert_eq!(detached.offset(), 100.0);

    let mut following = VirtualListState::at_offset(100.0, true);
    following.content_grew(300.0, 320.0);
    assert_eq!(following.offset(), 120.0);
}

#[test]
fn user_scroll_controls_follow_tail_state() {
    let mut list = VirtualListState::at_offset(100.0, true);
    list.scroll_by(-20.0, 200.0);
    assert_eq!(list.offset(), 80.0);
    assert!(!list.follows_tail());

    list.scroll_by(200.0, 200.0);
    assert_eq!(list.offset(), 200.0);
    assert!(list.follows_tail());
}
