use zode_app_model::{demo_state, GoalProgress, TranscriptItem, TranscriptState};
use zode_node_protocol::SessionLocator;

fn session(state: &zode_app_model::ZodeAppState, id: &str) -> SessionLocator {
    SessionLocator::new(state.host.node_id, id)
}

#[test]
fn brand_new_tab_has_no_conversation_and_shows_the_context_bar() {
    let state = demo_state();
    assert!(state.current_session.is_none());
    assert!(!state.current_session_has_conversation());
    assert!(state.current_goal_progress().is_none());
    assert!(state.composer_context_bar_visible());
}

#[test]
fn a_session_with_an_empty_transcript_still_counts_as_no_conversation() {
    let mut state = demo_state();
    let locator = session(&state, "fresh");
    state.current_session = Some(locator.clone());
    state
        .transcripts
        .insert(locator, TranscriptState::default());

    assert!(!state.current_session_has_conversation());
    assert!(state.composer_context_bar_visible());
}

#[test]
fn first_user_message_hides_the_context_bar_immediately() {
    let mut state = demo_state();
    let locator = session(&state, "just-sent");
    state.current_session = Some(locator.clone());
    state.transcripts.insert(
        locator,
        TranscriptState {
            items: vec![TranscriptItem::user_text("build the renderer")],
            ..TranscriptState::default()
        },
    );

    assert!(state.current_session_has_conversation());
    assert!(!state.composer_context_bar_visible());
}

#[test]
fn restored_historical_session_hides_the_context_bar() {
    let mut state = demo_state();
    let locator = session(&state, "history");
    state.current_session = Some(locator.clone());
    state.transcripts.insert(
        locator,
        TranscriptState {
            items: vec![
                TranscriptItem::user_text("earlier question"),
                TranscriptItem::assistant_text("earlier answer"),
            ],
            ..TranscriptState::default()
        },
    );

    assert!(state.current_session_has_conversation());
    assert!(!state.composer_context_bar_visible());
}

#[test]
fn goal_progress_keeps_the_bar_visible_even_mid_conversation() {
    let mut state = demo_state();
    let locator = session(&state, "goal-context");
    state.current_session = Some(locator.clone());
    state.transcripts.insert(
        locator,
        TranscriptState {
            items: vec![TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Visual rebuild".into(),
                completed: 3,
                total: 7,
            })],
            busy: true,
            ..TranscriptState::default()
        },
    );

    assert!(state.current_session_has_conversation());
    assert!(state.current_goal_progress().is_some());
    assert!(state.composer_context_bar_visible());
}

#[test]
fn goal_progress_only_counts_while_the_session_is_busy() {
    let mut state = demo_state();
    let locator = session(&state, "goal-finished");
    state.current_session = Some(locator.clone());
    state.transcripts.insert(
        locator,
        TranscriptState {
            items: vec![TranscriptItem::GoalProgress(GoalProgress {
                id: "goal-1".into(),
                title: "Visual rebuild".into(),
                completed: 7,
                total: 7,
            })],
            busy: false,
            ..TranscriptState::default()
        },
    );

    assert!(state.current_goal_progress().is_none());
    assert!(!state.composer_context_bar_visible());
}

#[test]
fn the_predicate_only_looks_at_the_active_tabs_transcript() {
    let mut state = demo_state();
    let busy_other = session(&state, "other-tab-busy");
    state.transcripts.insert(
        busy_other,
        TranscriptState {
            items: vec![TranscriptItem::user_text("unrelated tab already talking")],
            ..TranscriptState::default()
        },
    );
    let active = session(&state, "active-tab-fresh");
    state.current_session = Some(active.clone());
    state.transcripts.insert(active, TranscriptState::default());

    assert!(
        !state.current_session_has_conversation(),
        "a busy background tab must not leak into the active tab's bar visibility"
    );
    assert!(state.composer_context_bar_visible());
}
