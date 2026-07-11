use crate::turns::TurnRegistry;

#[test]
fn start_rejects_second_active_turn_for_thread() {
    let mut registry = TurnRegistry::default();
    let (turn, _abort) = registry.start("thread-1", "turn-1".into()).unwrap();

    let err = registry.start("thread-1", "turn-2".into()).unwrap_err();

    assert_eq!(turn.thread_id, "thread-1");
    assert!(err.contains("turn already running"));
}

#[test]
fn interrupt_aborts_matching_active_turn() {
    let mut registry = TurnRegistry::default();
    let (turn, abort) = registry.start("thread-1", "turn-1".into()).unwrap();

    assert!(registry.interrupt("thread-1", &turn.id));

    assert!(abort.is_aborted());
}

#[test]
fn interrupt_and_finish_ignore_wrong_turn_id() {
    let mut registry = TurnRegistry::default();
    let (_turn, abort) = registry.start("thread-1", "turn-1".into()).unwrap();

    assert!(!registry.interrupt("thread-1", "stale-turn"));
    assert!(registry.finish("thread-1", "stale-turn").is_none());

    assert!(!abort.is_aborted());
    assert!(registry.start("thread-1", "turn-2".into()).is_err());
}

#[test]
fn finish_matching_turn_allows_thread_to_restart() {
    let mut registry = TurnRegistry::default();
    let (turn, _abort) = registry.start("thread-1", "turn-1".into()).unwrap();

    assert!(registry.finish("thread-1", &turn.id).is_some());

    assert!(registry.start("thread-1", "turn-2".into()).is_ok());
}
