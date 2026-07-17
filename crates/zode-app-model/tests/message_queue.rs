use zode_app_model::{
    demo_state, reduce_navigation_command, reduce_queue_command, AppCommand, AttachmentMetadata,
    MessageQueueState, NavigationOutcome, QueueCommandOutcome, TranscriptState,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, UserContent, WorkspaceUri};

fn add_session(state: &mut zode_app_model::ZodeAppState, id: &str) -> SessionLocator {
    let session = SessionLocator::new(state.host.node_id, id);
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new(format!("file:///repo/{id}")).unwrap(),
        title: id.into(),
        updated_at_ms: 1,
        status: ThreadStatus::Running,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    session
}

fn text(value: &str) -> UserContent {
    UserContent::Text { text: value.into() }
}

fn image(name: &str) -> UserContent {
    UserContent::Image {
        mime_type: "image/png".into(),
        data_base64: "cG5n".into(),
        display_name: name.into(),
    }
}

fn attachment(name: &str) -> AttachmentMetadata {
    AttachmentMetadata {
        id: format!("attachment-{name}"),
        path: None,
        display_name: name.into(),
        media_type: "image/png".into(),
        width: Some(100),
        height: Some(80),
        byte_len: 4,
    }
}

#[test]
fn enqueue_preserves_content_and_allocates_stable_session_local_ids() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first");
    let second = add_session(&mut state, "second");

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: first.clone(),
                content: vec![text("one"), image("one.png")],
                attachments: vec![attachment("one.png")],
            },
        ),
        QueueCommandOutcome::Enqueued(state.message_queues[&first].items.first().unwrap().id),
    );
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: first.clone(),
                content: vec![text("two")],
                attachments: Vec::new(),
            },
        ),
        QueueCommandOutcome::Enqueued(state.message_queues[&first].items[1].id),
    );
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: second.clone(),
                content: vec![text("other")],
                attachments: Vec::new(),
            },
        ),
        QueueCommandOutcome::Enqueued(state.message_queues[&second].items[0].id),
    );

    let first_queue = &state.message_queues[&first];
    assert_eq!(first_queue.items[0].id.as_u64(), 1);
    assert_eq!(first_queue.items[1].id.as_u64(), 2);
    assert_eq!(first_queue.items[0].text, "one");
    assert_eq!(
        first_queue.items[0].attachments,
        vec![attachment("one.png")]
    );
    assert_eq!(state.message_queues[&second].items[0].id.as_u64(), 1);
    assert_eq!(state.message_queues[&second].items[0].text, "other");
}

#[test]
fn editing_text_preserves_non_text_content_and_the_message_identity() {
    let mut state = demo_state();
    let session = add_session(&mut state, "edit");
    let mut queue = MessageQueueState::default();
    let id = queue
        .enqueue("before\ntail".into(), vec![attachment("reference.png")])
        .unwrap();
    state.message_queues.insert(session.clone(), queue);
    state.current_session = Some(session.clone());
    state.composer.editing_queued_message = Some(id);
    state.composer.draft_before_queue_edit = Some("working draft".into());
    state.composer.draft = "after".into();

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EditQueuedMessageText {
                session: session.clone(),
                id,
                text: "after".into(),
            },
        ),
        QueueCommandOutcome::Applied,
    );

    let message = &state.message_queues[&session].items[0];
    assert_eq!(message.id, id);
    assert_eq!(message.text, "after");
    assert_eq!(message.attachments, vec![attachment("reference.png")]);
    assert_eq!(state.composer.editing_queued_message, None);
    assert_eq!(state.composer.draft, "working draft");
    assert_eq!(state.composer.draft_before_queue_edit, None);
}

#[test]
fn remove_clear_and_take_next_are_scoped_and_do_not_reuse_ids() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first");
    let second = add_session(&mut state, "second");

    let mut first_queue = MessageQueueState::default();
    let first_id = first_queue.enqueue("first".into(), Vec::new()).unwrap();
    let second_id = first_queue.enqueue("second".into(), Vec::new()).unwrap();
    state.message_queues.insert(first.clone(), first_queue);

    let mut second_queue = MessageQueueState::default();
    second_queue.enqueue("other".into(), Vec::new()).unwrap();
    state.message_queues.insert(second.clone(), second_queue);

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::RemoveQueuedMessage {
                session: first.clone(),
                id: second_id,
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::DispatchNextQueuedMessage {
                session: first.clone(),
            },
        ),
        QueueCommandOutcome::Ignored,
    );
    let next_id = state.message_queues[&first].peek_next(None).unwrap().id;
    let dispatched = state
        .message_queues
        .get_mut(&first)
        .unwrap()
        .remove_exact(next_id)
        .unwrap();
    assert_eq!(dispatched.id, first_id);
    assert_eq!(dispatched.text, "first");
    assert!(state.message_queues[&first].items.is_empty());
    assert_eq!(state.message_queues[&second].items.len(), 1);

    let next_id = state
        .message_queues
        .get_mut(&first)
        .unwrap()
        .enqueue("third".into(), Vec::new())
        .unwrap();
    assert_eq!(next_id.as_u64(), 3);
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::ClearMessageQueue {
                session: first.clone(),
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert!(state.message_queues[&first].items.is_empty());
    assert_eq!(state.message_queues[&second].items.len(), 1);
}

#[test]
fn unknown_or_empty_queue_commands_are_ignored_without_creating_state() {
    let mut state = demo_state();
    let missing = SessionLocator::new(state.host.node_id, "missing");

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: missing.clone(),
                content: vec![text("never")],
                attachments: Vec::new(),
            },
        ),
        QueueCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::EnqueueMessage {
                session: missing.clone(),
                content: Vec::new(),
                attachments: Vec::new(),
            },
        ),
        QueueCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::DispatchNextQueuedMessage {
                session: missing.clone(),
            },
        ),
        QueueCommandOutcome::Ignored,
    );
    assert!(!state.message_queues.contains_key(&missing));
}

#[test]
fn deleting_a_session_removes_its_queue() {
    let mut state = demo_state();
    let session = add_session(&mut state, "delete");
    let mut queue = MessageQueueState::default();
    queue.enqueue("queued".into(), Vec::new()).unwrap();
    state.message_queues.insert(session.clone(), queue);

    assert_eq!(
        reduce_navigation_command(
            &mut state,
            AppCommand::RequestDeleteSession(session.clone()),
        ),
        NavigationOutcome::NeedsEffect,
    );
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::DeleteSession(session.clone())),
        NavigationOutcome::NeedsEffect,
    );
    assert!(!state.message_queues.contains_key(&session));
}

#[test]
fn queue_menu_and_edit_state_are_current_session_ephemeral_state() {
    let mut state = demo_state();
    let first = add_session(&mut state, "first");
    let second = add_session(&mut state, "second");
    state.current_session = Some(first.clone());
    let mut queue = MessageQueueState::default();
    let id = queue.enqueue("queued text".into(), Vec::new()).unwrap();
    state.message_queues.insert(first.clone(), queue);
    state.composer.draft = "original draft".into();

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::ToggleQueuedMessageMenu {
                session: first.clone(),
                id,
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert_eq!(state.composer.queue_menu, Some(id));
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::BeginEditQueuedMessage {
                session: first.clone(),
                id,
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert_eq!(state.composer.queue_menu, None);
    assert_eq!(state.composer.editing_queued_message, Some(id));
    assert_eq!(state.composer.draft, "queued text");

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::CancelQueuedMessageEdit { session: second },
        ),
        QueueCommandOutcome::Ignored,
    );
    assert_eq!(state.composer.editing_queued_message, Some(id));
    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::CancelQueuedMessageEdit {
                session: first.clone(),
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert_eq!(state.composer.editing_queued_message, None);
    assert_eq!(state.composer.draft, "original draft");
    assert_eq!(state.composer.draft_before_queue_edit, None);

    state.composer.queue_menu = Some(id);
    state.composer.editing_queued_message = Some(id);
    state.composer.draft_before_queue_edit = Some("original draft".into());
    state.composer.draft = "queued text".into();
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(first)),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.composer.queue_menu, Some(id));
    assert_eq!(state.composer.editing_queued_message, Some(id));

    let second = state
        .threads
        .iter()
        .find(|thread| thread.session.session_id == "second")
        .unwrap()
        .session
        .clone();
    assert_eq!(
        reduce_navigation_command(&mut state, AppCommand::SelectSession(second)),
        NavigationOutcome::Applied,
    );
    assert_eq!(state.composer.queue_menu, None);
    assert_eq!(state.composer.editing_queued_message, None);
    assert_eq!(state.composer.draft, "original draft");
    assert_eq!(state.composer.draft_before_queue_edit, None);
    assert!(!state.composer.send_hovered);
}

#[test]
fn removing_an_edited_message_closes_its_ephemeral_ui() {
    let mut state = demo_state();
    let session = add_session(&mut state, "current");
    state.current_session = Some(session.clone());
    let mut queue = MessageQueueState::default();
    let first = queue.enqueue("first".into(), Vec::new()).unwrap();
    let second = queue.enqueue("second".into(), Vec::new()).unwrap();
    state.message_queues.insert(session.clone(), queue);
    state.composer.queue_menu = Some(second);
    state.composer.editing_queued_message = Some(second);
    state.composer.draft_before_queue_edit = Some("original draft".into());
    state.composer.draft = "second".into();

    assert_eq!(
        reduce_queue_command(
            &mut state,
            &AppCommand::RemoveQueuedMessage {
                session: session.clone(),
                id: second,
            },
        ),
        QueueCommandOutcome::Applied,
    );
    assert_eq!(state.composer.queue_menu, None);
    assert_eq!(state.composer.editing_queued_message, None);
    assert_eq!(state.composer.draft, "original draft");

    assert_eq!(state.message_queues[&session].items[0].id, first);
}
