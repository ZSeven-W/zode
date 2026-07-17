use zode_app_model::ZodeAppState;
use zode_app_ui::ComposerOutcome;

pub(crate) fn project_composer_outcome(state: &mut ZodeAppState, outcome: &ComposerOutcome) {
    match outcome {
        ComposerOutcome::AttachmentsChanged(attachments) => {
            state.composer.attachments.clone_from(attachments);
        }
        ComposerOutcome::Queue(_) | ComposerOutcome::QueueFront(_) => {
            state.composer.attachments.clear();
        }
        ComposerOutcome::Send(submission) => {
            state.composer.attachments.clear();
            let Some(session) = state.current_session.clone() else {
                return;
            };
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return;
            };
            let attachments = submission
                .attachments
                .iter()
                .cloned()
                .map(zode_app_model::TranscriptItem::Attachment)
                .collect::<Vec<_>>();
            if !transcript.insert_latest_turn_user_items(attachments.clone()) {
                transcript.items.extend(attachments);
            }
        }
        ComposerOutcome::Steer(submission) => {
            state.composer.attachments.clear();
            let Some(session) = state.current_session.clone() else {
                return;
            };
            let Some(transcript) = state.transcripts.get_mut(&session) else {
                return;
            };
            transcript.items.extend(
                submission
                    .attachments
                    .iter()
                    .cloned()
                    .map(zode_app_model::TranscriptItem::Attachment),
            );
        }
        ComposerOutcome::Ignored
        | ComposerOutcome::Edited
        | ComposerOutcome::Stop
        | ComposerOutcome::SetModel(_)
        | ComposerOutcome::SetEffort(_)
        | ComposerOutcome::SetSandbox(_) => {}
    }
}

/// Mirrors the controller's committed text into presentation state while
/// retaining the draft allocation across keystrokes and ignoring preedit-only
/// updates that leave the committed text unchanged.
pub(crate) fn sync_draft(state: &mut ZodeAppState, text: &str) -> bool {
    if state.composer.draft == text {
        return false;
    }
    state.composer.draft.clear();
    state.composer.draft.push_str(text);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_sync_skips_unchanged_preedit_text_and_reuses_storage_for_commits() {
        let mut state = zode_app_model::demo_state();
        state.composer.draft = String::with_capacity(64);
        state.composer.draft.push_str("existing");
        let capacity = state.composer.draft.capacity();

        assert!(!sync_draft(&mut state, "existing"));
        assert_eq!(state.composer.draft.capacity(), capacity);

        assert!(sync_draft(&mut state, "committed"));
        assert_eq!(state.composer.draft, "committed");
        assert_eq!(state.composer.draft.capacity(), capacity);
    }
}
