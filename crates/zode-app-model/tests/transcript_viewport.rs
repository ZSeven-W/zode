use zode_app_model::{
    demo_state, reduce_transcript_command, AppCommand, TranscriptCommandOutcome, TranscriptItem,
    TranscriptState,
};
use zode_node_protocol::SessionLocator;

#[test]
fn transcript_defaults_to_following_the_tail() {
    let transcript = TranscriptState::default();
    assert_eq!(transcript.scroll_offset, 0.0);
    assert!(transcript.follow_tail);
    assert!(transcript.item_heights.is_empty());
}

#[test]
fn viewport_and_item_measurements_update_only_the_target_session() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "thread-a");
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::UserText("one".into()),
                TranscriptItem::AssistantText("two".into()),
            ],
            ..TranscriptState::default()
        },
    );

    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetTranscriptViewport {
                session: session.clone(),
                scroll_offset: 120.0,
                follow_tail: false,
            },
        ),
        TranscriptCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetTranscriptItemHeight {
                session: session.clone(),
                index: 1,
                height: 88.0,
            },
        ),
        TranscriptCommandOutcome::Applied,
    );

    let transcript = &state.transcripts[&session];
    assert_eq!(transcript.scroll_offset, 120.0);
    assert!(!transcript.follow_tail);
    assert_eq!(transcript.item_heights, vec![0.0, 88.0]);
}

#[test]
fn invalid_measurements_and_unknown_sessions_are_ignored() {
    let mut state = demo_state();
    let missing = SessionLocator::new(state.host.node_id, "missing");
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetTranscriptItemHeight {
                session: missing,
                index: 0,
                height: f32::NAN,
            },
        ),
        TranscriptCommandOutcome::Ignored,
    );
}
