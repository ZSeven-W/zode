use zode_app_model::{AppCommand, SettingsCategory, ShellRoute};
use zode_app_ui::ComposerOutcome;
use zode_node_protocol::UserContent;

use super::{submission_text, DesktopApp};

impl DesktopApp {
    /// Preserve an attempted submission and route to the truthful local
    /// configuration summary instead of silently dropping the draft.
    pub(super) fn redirect_unconfigured_submission(&mut self, outcome: &ComposerOutcome) -> bool {
        if !self.app_state.provider_setup_required
            || self.app_state.composer.editing_queued_message.is_some()
        {
            return false;
        }
        let submission = match outcome {
            ComposerOutcome::Send(submission)
            | ComposerOutcome::Queue(submission)
            | ComposerOutcome::QueueFront(submission)
            | ComposerOutcome::Steer(submission) => submission,
            _ => return false,
        };

        self.composer.set_text(submission_text(&submission.content));
        for (content, metadata) in submission
            .content
            .iter()
            .filter_map(|content| match content {
                UserContent::Image {
                    mime_type,
                    data_base64,
                    ..
                } => Some((mime_type, data_base64)),
                UserContent::Text { .. } => None,
            })
            .zip(submission.attachments.iter().cloned())
        {
            let _ = self.composer.paste_image_with_metadata(
                content.0.clone(),
                content.1.clone(),
                metadata,
            );
        }
        self.app_state.composer.draft = self.composer.text().to_owned();
        self.app_state.composer.attachments = self.composer.attachment_metadata().to_vec();
        self.enqueue_command(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::ProviderModels,
        )));
        true
    }
}
