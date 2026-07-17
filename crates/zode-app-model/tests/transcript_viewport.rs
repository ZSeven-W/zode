use zode_app_model::{
    demo_state, reduce_transcript_command, AppCommand, MessageFeedback, TranscriptCommandOutcome,
    TranscriptItem, TranscriptState,
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
                TranscriptItem::user_text("one"),
                TranscriptItem::assistant_text("two"),
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

#[test]
fn message_feedback_updates_only_the_targeted_assistant_item() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "thread-b");
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::user_text("one"),
                TranscriptItem::assistant_text("two"),
            ],
            ..TranscriptState::default()
        },
    );

    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetMessageFeedback {
                session: session.clone(),
                index: 1,
                feedback: MessageFeedback::Up,
            },
        ),
        TranscriptCommandOutcome::Applied,
    );
    assert!(matches!(
        state.transcripts[&session].items[1],
        TranscriptItem::AssistantText {
            feedback: MessageFeedback::Up,
            ..
        }
    ));

    // Clicking the same reaction again clears it back to `None` - the
    // toggle-off logic lives at the click site (`command_for_widget`), so
    // this just confirms the reducer applies whatever value it is given.
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetMessageFeedback {
                session: session.clone(),
                index: 1,
                feedback: MessageFeedback::None,
            },
        ),
        TranscriptCommandOutcome::Applied,
    );
    assert!(matches!(
        state.transcripts[&session].items[1],
        TranscriptItem::AssistantText {
            feedback: MessageFeedback::None,
            ..
        }
    ));
}

#[test]
fn message_feedback_is_ignored_for_non_assistant_items_and_out_of_range_indices() {
    let mut state = demo_state();
    let session = SessionLocator::new(state.host.node_id, "thread-c");
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![TranscriptItem::user_text("only item")],
            ..TranscriptState::default()
        },
    );

    // Index 0 is a UserText item, not AssistantText.
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetMessageFeedback {
                session: session.clone(),
                index: 0,
                feedback: MessageFeedback::Up,
            },
        ),
        TranscriptCommandOutcome::Ignored,
    );
    // Index 5 does not exist.
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetMessageFeedback {
                session: session.clone(),
                index: 5,
                feedback: MessageFeedback::Up,
            },
        ),
        TranscriptCommandOutcome::Ignored,
    );

    let missing = SessionLocator::new(state.host.node_id, "missing");
    assert_eq!(
        reduce_transcript_command(
            &mut state,
            AppCommand::SetMessageFeedback {
                session: missing,
                index: 0,
                feedback: MessageFeedback::Up,
            },
        ),
        TranscriptCommandOutcome::Ignored,
    );
}
