use std::collections::BTreeMap;

use zode_app_model::{
    reduce_queue_command, AppCommand, MessageQueueState, QueueCommandOutcome, QueuedMessage,
    QueuedMessageId, TranscriptItem,
};
use zode_node_protocol::{SessionLocator, UserContent};

use super::DesktopApp;
use crate::command_bridge::{
    prepare_queued_start, prepare_queued_steer, project_command_error, reject_dispatch,
};

pub(super) enum QueueIntentResult {
    Handled,
    Continue(AppCommand),
}

/// Controller-private storage for full queued message payloads. The immutable
/// application model keeps only text and attachment metadata, so image bytes
/// never enter UI snapshots or reducers.
#[derive(Debug, Default)]
pub(super) struct QueuedPayloadStore {
    sessions: BTreeMap<SessionLocator, BTreeMap<QueuedMessageId, Vec<UserContent>>>,
}

impl QueuedPayloadStore {
    fn insert(&mut self, session: SessionLocator, id: QueuedMessageId, payload: Vec<UserContent>) {
        self.sessions
            .entry(session)
            .or_default()
            .insert(id, payload);
    }

    fn get(&self, session: &SessionLocator, id: QueuedMessageId) -> Option<&[UserContent]> {
        self.sessions
            .get(session)
            .and_then(|payloads| payloads.get(&id))
            .map(Vec::as_slice)
    }

    fn edit_text(&mut self, session: &SessionLocator, id: QueuedMessageId, text: String) -> bool {
        let Some(payload) = self
            .sessions
            .get_mut(session)
            .and_then(|payloads| payloads.get_mut(&id))
        else {
            return false;
        };
        replace_payload_text(payload, text);
        true
    }

    fn remove(
        &mut self,
        session: &SessionLocator,
        id: QueuedMessageId,
    ) -> Option<Vec<UserContent>> {
        let (removed, empty) = {
            let payloads = self.sessions.get_mut(session)?;
            let removed = payloads.remove(&id);
            (removed, payloads.is_empty())
        };
        if empty {
            self.sessions.remove(session);
        }
        removed
    }

    fn clear(&mut self, session: &SessionLocator) -> bool {
        self.sessions.remove(session).is_some()
    }

    fn prune(&mut self, queues: &BTreeMap<SessionLocator, MessageQueueState>) {
        self.sessions.retain(|session, payloads| {
            let Some(queue) = queues.get(session) else {
                return false;
            };
            payloads.retain(|id, _| queue.items.iter().any(|message| message.id == *id));
            !payloads.is_empty()
        });
    }
}

impl DesktopApp {
    /// Applies queue-only commands before the endpoint bridge sees them. Full
    /// payloads stay in this controller; reducers receive the same command by
    /// reference and retain only a lightweight preview.
    pub(super) fn apply_queue_intent(&mut self, command: AppCommand) -> QueueIntentResult {
        match command {
            AppCommand::EnqueueMessage {
                session,
                content,
                attachments,
            } => {
                let command = AppCommand::EnqueueMessage {
                    session: session.clone(),
                    content,
                    attachments,
                };
                if let QueueCommandOutcome::Enqueued(id) =
                    reduce_queue_command(&mut self.app_state, &command)
                {
                    let AppCommand::EnqueueMessage { content, .. } = command else {
                        unreachable!("the queue command was just constructed")
                    };
                    self.queued_payloads.insert(session.clone(), id, content);
                    let should_dispatch = self.session_is_idle(&session);
                    self.finish_queue_ui_update();
                    if should_dispatch {
                        self.dispatch_queued_intent(session, None, false);
                    }
                }
                QueueIntentResult::Handled
            }
            AppCommand::EditQueuedMessageText { session, id, text } => {
                let command = AppCommand::EditQueuedMessageText {
                    session: session.clone(),
                    id,
                    text: text.clone(),
                };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.queued_payloads.edit_text(&session, id, text);
                    self.composer.finish_queue_edit();
                    self.sync_composer_projection();
                    let should_dispatch = self.session_is_idle(&session);
                    self.finish_queue_ui_update();
                    if should_dispatch {
                        self.dispatch_queued_intent(session, None, false);
                    }
                }
                QueueIntentResult::Handled
            }
            AppCommand::RemoveQueuedMessage { session, id } => {
                let was_editing = self.app_state.current_session.as_ref() == Some(&session)
                    && self.app_state.composer.editing_queued_message == Some(id);
                let command = AppCommand::RemoveQueuedMessage {
                    session: session.clone(),
                    id,
                };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.queued_payloads.remove(&session, id);
                    if was_editing {
                        self.composer.finish_queue_edit();
                        self.sync_composer_projection();
                    }
                    let should_dispatch = self.session_is_idle(&session);
                    self.finish_queue_ui_update();
                    if should_dispatch {
                        self.dispatch_queued_intent(session, None, false);
                    }
                }
                QueueIntentResult::Handled
            }
            AppCommand::ClearMessageQueue { session } => {
                let was_editing = self.app_state.current_session.as_ref() == Some(&session)
                    && self.app_state.composer.editing_queued_message.is_some();
                let command = AppCommand::ClearMessageQueue {
                    session: session.clone(),
                };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.queued_payloads.clear(&session);
                    if was_editing {
                        self.composer.finish_queue_edit();
                        self.sync_composer_projection();
                    }
                    self.finish_queue_ui_update();
                }
                QueueIntentResult::Handled
            }
            AppCommand::ToggleQueuedMessageMenu { session, id } => {
                let command = AppCommand::ToggleQueuedMessageMenu { session, id };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.finish_queue_ui_update();
                }
                QueueIntentResult::Handled
            }
            AppCommand::BeginEditQueuedMessage { session, id } => {
                let target_has_attachments = self
                    .app_state
                    .message_queues
                    .get(&session)
                    .and_then(|queue| queue.items.iter().find(|message| message.id == id))
                    .is_some_and(|message| !message.attachments.is_empty());
                let command = AppCommand::BeginEditQueuedMessage {
                    session: session.clone(),
                    id,
                };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.composer.begin_queue_edit_for_message(
                        self.app_state.composer.draft.clone(),
                        target_has_attachments,
                    );
                    self.sync_composer_projection();
                    self.finish_queue_ui_update();
                }
                QueueIntentResult::Handled
            }
            AppCommand::CancelQueuedMessageEdit { session } => {
                let command = AppCommand::CancelQueuedMessageEdit {
                    session: session.clone(),
                };
                if reduce_queue_command(&mut self.app_state, &command)
                    == QueueCommandOutcome::Applied
                {
                    self.composer.finish_queue_edit();
                    self.sync_composer_projection();
                    let should_dispatch = self.session_is_idle(&session);
                    self.finish_queue_ui_update();
                    if should_dispatch {
                        self.dispatch_queued_intent(session, None, false);
                    }
                }
                QueueIntentResult::Handled
            }
            AppCommand::GuideQueuedMessage { session, id } => {
                self.dispatch_queued_intent(session, Some(id), true);
                QueueIntentResult::Handled
            }
            AppCommand::DispatchNextQueuedMessage { session } => {
                self.dispatch_queued_intent(session, None, false);
                QueueIntentResult::Handled
            }
            command => QueueIntentResult::Continue(command),
        }
    }

    fn dispatch_queued_intent(
        &mut self,
        session: SessionLocator,
        requested_id: Option<QueuedMessageId>,
        explicit_guide: bool,
    ) {
        if self.command_bridge.is_none() {
            if explicit_guide {
                project_command_error(
                    &mut self.app_state,
                    "队列暂时无法提交：endpoint 未连接".into(),
                );
                self.finish_queue_ui_update();
            }
            return;
        }

        let editing_id = (self.app_state.current_session.as_ref() == Some(&session))
            .then_some(self.app_state.composer.editing_queued_message)
            .flatten();
        if explicit_guide && requested_id.is_some_and(|id| Some(id) == editing_id) {
            project_queue_notice(
                &mut self.app_state,
                &session,
                "queue.guide_editing",
                "正在编辑的队列消息不能引导；请先保存或取消编辑".into(),
            );
            self.finish_queue_ui_update();
            return;
        }
        if explicit_guide && self.provisional_sessions.contains(&session) {
            project_queue_notice(
                &mut self.app_state,
                &session,
                "queue.guide_session_pending",
                "新任务仍在创建中；确认后才能引导队列消息".into(),
            );
            self.finish_queue_ui_update();
            return;
        }
        let preview = self
            .app_state
            .message_queues
            .get(&session)
            .and_then(|queue| select_queued_preview(queue, requested_id, editing_id))
            .cloned();
        let Some(preview) = preview else {
            return;
        };
        let Some(payload) = self.payload_for_preview(&session, &preview) else {
            if explicit_guide {
                project_command_error(
                    &mut self.app_state,
                    "队列附件载荷已不可用；消息仍保留在队列中".into(),
                );
                self.finish_queue_ui_update();
            }
            return;
        };

        let transcript_len = self
            .app_state
            .transcripts
            .get(&session)
            .map_or(0, |transcript| transcript.items.len());
        let active = self.app_state.active_turns.contains_key(&session)
            && self
                .app_state
                .transcripts
                .get(&session)
                .is_some_and(|transcript| transcript.busy);
        let prepared = if explicit_guide && active {
            prepare_queued_steer(&mut self.app_state, session.clone(), payload)
        } else {
            prepare_queued_start(&mut self.app_state, session.clone(), payload)
        };
        let dispatch = match prepared {
            Ok(dispatch) => dispatch,
            Err(message) => {
                if explicit_guide {
                    project_command_error(&mut self.app_state, message);
                    self.finish_queue_ui_update();
                }
                return;
            }
        };

        let accepted = self
            .command_bridge
            .as_ref()
            .expect("the bridge was checked before preparing the queue dispatch")
            .dispatch(dispatch);
        match accepted {
            Ok(()) => self.accept_queued_message(&session, preview),
            Err(dispatch) => {
                if let Some(transcript) = self.app_state.transcripts.get_mut(&session) {
                    transcript.items.truncate(transcript_len);
                }
                reject_dispatch(
                    &mut self.app_state,
                    dispatch,
                    "the endpoint command pump is unavailable".into(),
                );
            }
        }
        self.finish_queue_ui_update();
    }

    fn payload_for_preview(
        &self,
        session: &SessionLocator,
        preview: &QueuedMessage,
    ) -> Option<Vec<UserContent>> {
        if let Some(payload) = self.queued_payloads.get(session, preview.id) {
            return Some(payload.to_vec());
        }
        if !preview.attachments.is_empty() {
            return None;
        }
        (!preview.text.trim().is_empty()).then(|| {
            vec![UserContent::Text {
                text: preview.text.clone(),
            }]
        })
    }

    fn accept_queued_message(&mut self, session: &SessionLocator, preview: QueuedMessage) {
        let was_editing = self.app_state.current_session.as_ref() == Some(session)
            && self.app_state.composer.editing_queued_message == Some(preview.id);
        if let Some(transcript) = self.app_state.transcripts.get_mut(session) {
            transcript.items.extend(
                preview
                    .attachments
                    .iter()
                    .cloned()
                    .map(TranscriptItem::Attachment),
            );
        }
        let remove = AppCommand::RemoveQueuedMessage {
            session: session.clone(),
            id: preview.id,
        };
        let _ = reduce_queue_command(&mut self.app_state, &remove);
        self.queued_payloads.remove(session, preview.id);
        if was_editing {
            self.composer.finish_queue_edit();
            self.sync_composer_projection();
        }
    }

    fn session_is_idle(&self, session: &SessionLocator) -> bool {
        !self.app_state.active_turns.contains_key(session)
            && self
                .app_state
                .transcripts
                .get(session)
                .is_some_and(|transcript| !transcript.busy)
    }

    pub(super) fn prune_queued_payloads(&mut self) {
        self.queued_payloads.prune(&self.app_state.message_queues);
        self.reconcile_provisional_sessions();
    }

    pub(super) fn mark_provisional_first_submit(&mut self) {
        let Some(session) = self.app_state.current_session.clone() else {
            return;
        };
        if projected_first_submit_is_provisional(&self.app_state, &session) {
            self.provisional_sessions.insert(session);
        }
    }

    pub(super) fn reconcile_provisional_sessions(&mut self) {
        let state = &self.app_state;
        self.provisional_sessions
            .retain(|session| provisional_session_is_pending(state, session));
    }

    pub(super) fn sync_queue_editor_after_state_change(
        &mut self,
        previous_session: Option<SessionLocator>,
        was_editing: Option<QueuedMessageId>,
    ) {
        let session_changed = self.app_state.current_session != previous_session;
        if was_editing.is_none()
            || (!session_changed && self.app_state.composer.editing_queued_message.is_some())
        {
            return;
        }
        if session_changed && self.app_state.composer.editing_queued_message.is_some() {
            self.app_state.composer.finish_queue_edit();
        }
        self.composer.finish_queue_edit();
        self.sync_composer_projection();
        if let Some(session) = previous_session.filter(|session| self.session_is_idle(session)) {
            self.dispatch_queued_intent(session, None, false);
        }
    }

    fn sync_composer_projection(&mut self) {
        self.app_state.composer.draft = self.composer.text().to_owned();
        self.app_state.composer.attachments = self.composer.attachment_metadata().to_vec();
    }

    fn finish_queue_ui_update(&mut self) {
        self.rebuild_frame_snapshot();
        self.window_state.dirty = true;
        self.request_redraw();
    }
}

fn select_queued_preview(
    queue: &MessageQueueState,
    requested_id: Option<QueuedMessageId>,
    editing_id: Option<QueuedMessageId>,
) -> Option<&QueuedMessage> {
    match requested_id {
        Some(id) if Some(id) == editing_id => None,
        Some(id) => queue.items.iter().find(|message| message.id == id),
        None => queue.peek_next(editing_id),
    }
}

fn provisional_session_is_pending(
    state: &zode_app_model::ZodeAppState,
    session: &SessionLocator,
) -> bool {
    let Some(thread) = state
        .threads
        .iter()
        .find(|thread| &thread.session == session)
    else {
        return false;
    };
    if !state.transcripts.contains_key(session)
        || thread.status == zode_node_protocol::ThreadStatus::Failed
    {
        return false;
    }
    state
        .presentation
        .sessions
        .get(session)
        .is_none_or(|presentation| {
            matches!(
                presentation.runtime_options,
                zode_app_model::LoadState::Idle | zode_app_model::LoadState::Loading
            )
        })
}

fn projected_first_submit_is_provisional(
    state: &zode_app_model::ZodeAppState,
    session: &SessionLocator,
) -> bool {
    provisional_session_is_pending(state, session)
        && (state.active_turns.contains_key(session)
            || state
                .transcripts
                .get(session)
                .is_some_and(|transcript| transcript.busy))
}

fn replace_payload_text(payload: &mut Vec<UserContent>, text: String) {
    let first_text = payload
        .iter()
        .position(|content| matches!(content, UserContent::Text { .. }));
    payload.retain(|content| !matches!(content, UserContent::Text { .. }));
    if !text.trim().is_empty() {
        let index = first_text.unwrap_or(0).min(payload.len());
        payload.insert(index, UserContent::Text { text });
    }
}

fn project_queue_notice(
    state: &mut zode_app_model::ZodeAppState,
    session: &SessionLocator,
    code: &str,
    message: String,
) {
    if let Some(transcript) = state.transcripts.get_mut(session) {
        transcript.items.push(TranscriptItem::Status {
            code: code.into(),
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zode_app_model::{
        demo_state, LoadState, MessageQueueState, ProjectState, QueuedMessageId,
        SessionPresentationState, TranscriptState,
    };
    use zode_node_protocol::{
        RuntimeOptions, SandboxMode, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
        UserContent, WorkspaceUri,
    };

    use super::{
        projected_first_submit_is_provisional, provisional_session_is_pending,
        select_queued_preview, QueuedPayloadStore,
    };

    #[test]
    fn payload_store_edits_text_without_losing_private_image_bytes_then_removes_exactly() {
        let session = SessionLocator::new(zode_node_protocol::NodeId::new(), "session-a");
        let first = QueuedMessageId::from_u64(1);
        let second = QueuedMessageId::from_u64(2);
        let mut store = QueuedPayloadStore::default();
        store.insert(
            session.clone(),
            first,
            vec![
                UserContent::Text { text: "old".into() },
                UserContent::Image {
                    mime_type: "image/png".into(),
                    data_base64: "cHJpdmF0ZS1ieXRlcw==".into(),
                    display_name: "reference.png".into(),
                },
            ],
        );
        store.insert(
            session.clone(),
            second,
            vec![UserContent::Text {
                text: "second".into(),
            }],
        );

        assert!(store.edit_text(&session, first, "edited".into()));
        assert!(matches!(
            store.get(&session, first),
            Some([
                UserContent::Text { text },
                UserContent::Image { data_base64, display_name, .. }
            ]) if text == "edited"
                && data_base64 == "cHJpdmF0ZS1ieXRlcw=="
                && display_name == "reference.png"
        ));

        let removed = store.remove(&session, first).expect("first payload exists");
        assert!(matches!(
            removed.get(1),
            Some(UserContent::Image { data_base64, .. })
                if data_base64 == "cHJpdmF0ZS1ieXRlcw=="
        ));
        assert!(store.get(&session, first).is_none());
        assert!(store.get(&session, second).is_some());
    }

    #[test]
    fn payload_store_clear_and_prune_drop_only_payloads_absent_from_model_queues() {
        let node = zode_node_protocol::NodeId::new();
        let kept_session = SessionLocator::new(node, "kept");
        let cleared_session = SessionLocator::new(node, "cleared");
        let stale_session = SessionLocator::new(node, "stale");
        let mut queue = MessageQueueState::default();
        let kept_id = queue.enqueue("kept".into(), Vec::new()).unwrap();
        let stale_id = QueuedMessageId::from_u64(99);
        let mut queues = BTreeMap::new();
        queues.insert(kept_session.clone(), queue);

        let mut store = QueuedPayloadStore::default();
        for (session, id, text) in [
            (kept_session.clone(), kept_id, "kept"),
            (kept_session.clone(), stale_id, "orphan"),
            (cleared_session.clone(), kept_id, "clear me"),
            (stale_session.clone(), kept_id, "stale session"),
        ] {
            store.insert(session, id, vec![UserContent::Text { text: text.into() }]);
        }

        assert!(store.clear(&cleared_session));
        assert!(!store.clear(&cleared_session));
        store.prune(&queues);

        assert!(store.get(&kept_session, kept_id).is_some());
        assert!(store.get(&kept_session, stale_id).is_none());
        assert!(store.get(&cleared_session, kept_id).is_none());
        assert!(store.get(&stale_session, kept_id).is_none());
    }

    #[test]
    fn an_editing_message_cannot_be_selected_by_guide_or_automatic_dispatch() {
        let mut queue = MessageQueueState::default();
        let editing = queue.enqueue("old payload".into(), Vec::new()).unwrap();
        let tail = queue.enqueue("tail".into(), Vec::new()).unwrap();

        assert!(select_queued_preview(&queue, Some(editing), Some(editing)).is_none());
        assert!(select_queued_preview(&queue, None, Some(editing)).is_none());
        assert_eq!(
            select_queued_preview(&queue, Some(tail), Some(editing)).map(|message| message.id),
            Some(tail)
        );
    }

    #[test]
    fn provisional_first_submit_stays_blocked_until_runtime_confirmation() {
        let mut state = demo_state();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        let session = SessionLocator::new(state.host.node_id, "provisional");
        state.projects.push(ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: workspace,
            title: "新任务".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Running,
        });
        state.transcripts.insert(
            session.clone(),
            TranscriptState {
                busy: true,
                ..TranscriptState::default()
            },
        );
        state.active_turns.insert(session.clone(), TurnId::new());

        assert!(projected_first_submit_is_provisional(&state, &session));
        assert!(provisional_session_is_pending(&state, &session));

        // Even a very fast TurnFinished event must not make Guide race the
        // still-pending runtime confirmation in the command pump.
        state.active_turns.remove(&session);
        state.transcripts.get_mut(&session).unwrap().busy = false;
        assert!(!projected_first_submit_is_provisional(&state, &session));
        assert!(provisional_session_is_pending(&state, &session));

        // Selecting the optimistic thread can create an Idle presentation
        // entry; that alone is not endpoint confirmation.
        state
            .presentation
            .sessions
            .insert(session.clone(), SessionPresentationState::default());
        assert!(provisional_session_is_pending(&state, &session));

        state
            .presentation
            .sessions
            .get_mut(&session)
            .unwrap()
            .runtime_options = LoadState::Ready(RuntimeOptions {
            models: vec!["model".into()],
            active_model: Some("model".into()),
            effort: None,
            sandbox_mode: SandboxMode::WorkspaceWrite,
            sandbox_network: false,
        });
        assert!(!provisional_session_is_pending(&state, &session));
    }
}
