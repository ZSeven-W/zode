use super::*;

#[tokio::test]
async fn aborted_stream_finishes_as_interrupted_without_an_error_event() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "aborted-stream");
    let turn_id = TurnId::new();
    let (sender, stream) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session, turn_id), events)
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "aborted turn start").await;
    sender
        .send(Err(AgentError::Aborted("user cancelled".into())))
        .unwrap();
    drop(sender);

    let emitted = collect_until_finished(&mut receiver).await;
    assert!(!emitted
        .iter()
        .any(|event| matches!(event.kind, AgentEventKind::Error { .. })));
    assert!(matches!(
        emitted.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: true })
    ));
    assert_eq!(driver.finishes.lock().unwrap().len(), 1);
    assert!(driver.finishes.lock().unwrap()[0].interrupted);
}

#[tokio::test]
async fn fatal_stream_error_is_terminal_and_still_finishes_exactly_once() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "fatal-stream");
    let turn_id = TurnId::new();
    let (sender, stream) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session, turn_id), events)
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "fatal turn start").await;
    sender
        .send(Err(AgentError::other("safe provider failure")))
        .unwrap();
    drop(sender);

    let emitted = collect_until_finished(&mut receiver).await;
    assert_eq!(emitted.len(), 3);
    assert!(matches!(
        &emitted[0].kind,
        AgentEventKind::Error {
            message,
            retryable: false
        } if message.contains("safe provider failure")
    ));
    assert!(matches!(emitted[1].kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        emitted[2].kind,
        AgentEventKind::TurnFinished { interrupted: false }
    ));
    assert_eq!(driver.finishes.lock().unwrap().len(), 1);
    assert!(!driver.finishes.lock().unwrap()[0].interrupted);
}
