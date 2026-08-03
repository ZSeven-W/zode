//! Reducer arm for the in-conversation find bar.
//!
//! Every mutation the bar can make is local to one session: the open flag,
//! the query, the active match index, and the transcript scroll offset that
//! brings the current match on screen. Nothing here reaches the endpoint.

use zode_node_protocol::SessionLocator;

use super::TranscriptCommandOutcome;
use crate::{AppCommand, ZodeAppState};

/// Space left above a match once the transcript jumps to it, so the item
/// does not sit flush against the top edge of the viewport. A fixed inset
/// rather than a share of the viewport height because this crate has no
/// rendering dependency and therefore does not know the viewport - see
/// `item_top` for how the offset itself is recovered.
const JUMP_MARGIN: f32 = 24.0;

pub(super) fn reduce_transcript_find_command(
    state: &mut ZodeAppState,
    command: &AppCommand,
) -> Option<TranscriptCommandOutcome> {
    let session = match command {
        AppCommand::OpenTranscriptFind { session }
        | AppCommand::CloseTranscriptFind { session }
        | AppCommand::SetTranscriptFindQuery { session, .. }
        | AppCommand::StepTranscriptFindMatch { session, .. } => session.clone(),
        _ => return None,
    };
    if !state.transcripts.contains_key(&session) {
        return Some(TranscriptCommandOutcome::Ignored);
    }

    match command {
        AppCommand::OpenTranscriptFind { .. } => {
            let find = &mut state.presentation.sessions.entry(session).or_default().find;
            if find.open {
                return Some(TranscriptCommandOutcome::Ignored);
            }
            find.open = true;
            Some(TranscriptCommandOutcome::Applied)
        }
        AppCommand::CloseTranscriptFind { .. } => {
            let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                return Some(TranscriptCommandOutcome::Ignored);
            };
            if !presentation.find.open {
                return Some(TranscriptCommandOutcome::Ignored);
            }
            // Closing clears the query too, so the highlights vanish with the
            // bar and reopening starts from a clean field - matching how
            // `close_session_action_surfaces` resets `GlobalSearchState`.
            presentation.find = crate::TranscriptFindState::default();
            Some(TranscriptCommandOutcome::Applied)
        }
        AppCommand::SetTranscriptFindQuery { query, .. } => {
            let find = &mut state
                .presentation
                .sessions
                .entry(session.clone())
                .or_default()
                .find;
            if !find.open || &find.query == query {
                return Some(TranscriptCommandOutcome::Ignored);
            }
            find.query.clone_from(query);
            find.active = 0;
            scroll_to_active_match(state, &session);
            Some(TranscriptCommandOutcome::Applied)
        }
        AppCommand::StepTranscriptFindMatch { forward, .. } => {
            let Some(transcript) = state.transcripts.get(&session) else {
                return Some(TranscriptCommandOutcome::Ignored);
            };
            let Some(presentation) = state.presentation.sessions.get_mut(&session) else {
                return Some(TranscriptCommandOutcome::Ignored);
            };
            if !presentation.find.open || !presentation.find.step(transcript, *forward) {
                return Some(TranscriptCommandOutcome::Ignored);
            }
            scroll_to_active_match(state, &session);
            Some(TranscriptCommandOutcome::Applied)
        }
        _ => None,
    }
}

/// Scrolls `session`'s transcript so the current match's item is on screen.
///
/// Unlike the anchor rail - which resolves a click into a
/// `SetTranscriptViewport` command in the UI crate, where the viewport rect
/// is known - this runs in the reducer and has no geometry. It reads the
/// item's top offset out of the layout memo the last paint populated, which
/// is exactly the prefix-sum vector the rail would have used, and lets the
/// paint path clamp the result against the real content height (it already
/// clamps `scroll_offset` on every frame). A transcript that has never been
/// painted has no memo; the jump is then skipped rather than guessed at.
fn scroll_to_active_match(state: &mut ZodeAppState, session: &SessionLocator) {
    let Some(transcript) = state.transcripts.get(session) else {
        return;
    };
    let Some(presentation) = state.presentation.sessions.get(session) else {
        return;
    };
    let Some(found) = presentation.find.active_match(transcript) else {
        return;
    };
    let Some(top) = item_top(transcript, found.item_index) else {
        return;
    };
    let Some(transcript) = state.transcripts.get_mut(session) else {
        return;
    };
    transcript.scroll_offset = (top - JUMP_MARGIN).max(0.0);
    transcript.follow_tail = false;
}

fn item_top(transcript: &crate::TranscriptState, item_index: usize) -> Option<f32> {
    transcript
        .layout_cache
        .borrow()
        .as_ref()
        .and_then(|cache| cache.offsets.get(item_index).map(|(top, _)| *top))
        .filter(|top| top.is_finite())
}

#[cfg(test)]
mod tests {
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use crate::{
        demo_state, reduce_transcript_command, AppCommand, TranscriptCommandOutcome,
        TranscriptItem, TranscriptState, ZodeAppState,
    };

    fn state_with_transcript(items: Vec<TranscriptItem>) -> (ZodeAppState, SessionLocator) {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "task-1");
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
            title: "Task".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Idle,
        });
        state.transcripts.insert(
            session.clone(),
            TranscriptState {
                items,
                ..TranscriptState::default()
            },
        );
        state.current_session = Some(session.clone());
        (state, session)
    }

    fn find(state: &ZodeAppState, session: &SessionLocator) -> crate::TranscriptFindState {
        state.presentation.sessions[session].find.clone()
    }

    fn open(state: &mut ZodeAppState, session: &SessionLocator) {
        assert_eq!(
            reduce_transcript_command(
                state,
                AppCommand::OpenTranscriptFind {
                    session: session.clone(),
                },
            ),
            TranscriptCommandOutcome::Applied
        );
    }

    fn type_query(state: &mut ZodeAppState, session: &SessionLocator, query: &str) {
        assert_eq!(
            reduce_transcript_command(
                state,
                AppCommand::SetTranscriptFindQuery {
                    session: session.clone(),
                    query: query.into(),
                },
            ),
            TranscriptCommandOutcome::Applied
        );
    }

    fn step(state: &mut ZodeAppState, session: &SessionLocator, forward: bool) {
        assert_eq!(
            reduce_transcript_command(
                state,
                AppCommand::StepTranscriptFindMatch {
                    session: session.clone(),
                    forward,
                },
            ),
            TranscriptCommandOutcome::Applied
        );
    }

    #[test]
    fn open_and_close_toggle_the_bar_and_clear_the_query() {
        let (mut state, session) = state_with_transcript(vec![TranscriptItem::user_text("hit")]);
        open(&mut state, &session);
        assert!(find(&state, &session).open);
        type_query(&mut state, &session, "hit");

        assert_eq!(
            reduce_transcript_command(
                &mut state,
                AppCommand::CloseTranscriptFind {
                    session: session.clone(),
                },
            ),
            TranscriptCommandOutcome::Applied
        );
        let closed = find(&state, &session);
        assert!(!closed.open);
        assert!(closed.query.is_empty());
    }

    #[test]
    fn a_closed_bar_ignores_typing_and_navigation() {
        let (mut state, session) = state_with_transcript(vec![TranscriptItem::user_text("hit")]);
        state
            .presentation
            .sessions
            .entry(session.clone())
            .or_default();
        assert_eq!(
            reduce_transcript_command(
                &mut state,
                AppCommand::SetTranscriptFindQuery {
                    session: session.clone(),
                    query: "hit".into(),
                },
            ),
            TranscriptCommandOutcome::Ignored
        );
        assert_eq!(
            reduce_transcript_command(
                &mut state,
                AppCommand::StepTranscriptFindMatch {
                    session: session.clone(),
                    forward: true,
                },
            ),
            TranscriptCommandOutcome::Ignored
        );
    }

    #[test]
    fn typing_resets_the_active_match_to_the_first_hit() {
        let (mut state, session) = state_with_transcript(vec![
            TranscriptItem::user_text("hit"),
            TranscriptItem::assistant_text("hit hit"),
        ]);
        open(&mut state, &session);
        type_query(&mut state, &session, "hit");
        step(&mut state, &session, true);
        assert_eq!(find(&state, &session).active, 1);

        type_query(&mut state, &session, "hit h");
        assert_eq!(find(&state, &session).active, 0);
    }

    #[test]
    fn next_and_previous_wrap_around_the_match_list() {
        let (mut state, session) = state_with_transcript(vec![
            TranscriptItem::user_text("hit"),
            TranscriptItem::assistant_text("hit hit"),
        ]);
        open(&mut state, &session);
        type_query(&mut state, &session, "hit");
        let transcript = &state.transcripts[&session];
        assert_eq!(find(&state, &session).counter_label(transcript), "1/3");

        step(&mut state, &session, true);
        step(&mut state, &session, true);
        step(&mut state, &session, true);
        assert_eq!(find(&state, &session).active, 0);

        step(&mut state, &session, false);
        assert_eq!(find(&state, &session).active, 2);
    }

    #[test]
    fn stepping_a_query_with_no_hits_is_ignored() {
        let (mut state, session) = state_with_transcript(vec![TranscriptItem::user_text("hit")]);
        open(&mut state, &session);
        type_query(&mut state, &session, "missing");
        assert_eq!(
            reduce_transcript_command(
                &mut state,
                AppCommand::StepTranscriptFindMatch {
                    session: session.clone(),
                    forward: true,
                },
            ),
            TranscriptCommandOutcome::Ignored
        );
    }

    #[test]
    fn new_transcript_items_are_picked_up_without_retyping_the_query() {
        let (mut state, session) = state_with_transcript(vec![TranscriptItem::user_text("hit")]);
        open(&mut state, &session);
        type_query(&mut state, &session, "hit");
        assert_eq!(
            find(&state, &session).match_count(&state.transcripts[&session]),
            1
        );

        let transcript = state.transcripts.get_mut(&session).unwrap();
        transcript.items.push(TranscriptItem::assistant_text(
            "another hit, and one more hit",
        ));
        transcript.touch_layout();

        assert_eq!(
            find(&state, &session).match_count(&state.transcripts[&session]),
            3
        );
        assert_eq!(
            find(&state, &session).counter_label(&state.transcripts[&session]),
            "1/3"
        );
    }

    #[test]
    fn jumping_scrolls_the_transcript_to_the_matched_item() {
        let (mut state, session) = state_with_transcript(vec![
            TranscriptItem::user_text("nothing"),
            TranscriptItem::assistant_text("nothing"),
            TranscriptItem::user_text("hit"),
        ]);
        // Stand in for a painted frame: the reducer reads item tops out of
        // the layout memo, which only a real paint would otherwise populate.
        {
            let transcript = &state.transcripts[&session];
            let _ = transcript.layout_offsets(400.0, || {
                (vec![(0.0, 100.0), (100.0, 200.0), (200.0, 300.0)], 300.0)
            });
        }
        state.transcripts.get_mut(&session).unwrap().follow_tail = true;

        open(&mut state, &session);
        type_query(&mut state, &session, "hit");

        let transcript = &state.transcripts[&session];
        assert!(!transcript.follow_tail);
        assert_eq!(transcript.scroll_offset, 200.0 - super::JUMP_MARGIN);
    }

    #[test]
    fn commands_for_an_unknown_session_never_create_presentation_state() {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "ghost");
        assert_eq!(
            reduce_transcript_command(
                &mut state,
                AppCommand::OpenTranscriptFind {
                    session: session.clone(),
                },
            ),
            TranscriptCommandOutcome::Ignored
        );
        assert!(state.presentation.sessions.is_empty());
    }
}
