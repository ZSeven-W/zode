use zode_app_model::ZodeAppState;
use zode_app_ui::ComposerOutcome;

pub(crate) fn project_composer_outcome(state: &mut ZodeAppState, outcome: &ComposerOutcome) {
    match outcome {
        ComposerOutcome::AttachmentsChanged(attachments) => {
            state.composer.attachments.clone_from(attachments);
        }
        ComposerOutcome::Queue(_) => {
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
