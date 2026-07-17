use zode_app_model::{
    reduce_tool_command, reduce_transcript_command, AppCommand, SettingsCommandOutcome,
    ToolCommandOutcome, TranscriptCommandOutcome,
};

use crate::clipboard::execute_clipboard_command;

use super::super::{
    computer_use::consume_computer_use_command, external_preview::consume_external_preview_command,
    provider_models::ProviderModelsCommandOutcome, session_menu::consume_session_window_command,
    settings::reduce_local_settings_command, DesktopApp,
};
use super::{available_new_session_command, normalize_conversation_route};

impl DesktopApp {
    pub(in crate::app) fn enqueue_command(&mut self, command: AppCommand) {
        if self.apply_environment_action_command(&command) {
            return;
        }
        if consume_session_window_command(
            &mut self.app_state,
            self.session_window.as_ref(),
            &command,
        ) {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if consume_external_preview_command(
            &mut self.app_state,
            self.external_open.as_ref(),
            &command,
        ) {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if consume_computer_use_command(&mut self.app_state, self.external_open.as_ref(), &command)
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if matches!(command, AppCommand::CopyText(_)) {
            if let Some(clipboard) = self.clipboard.as_ref() {
                if let Err(error) = execute_clipboard_command(&command, clipboard.as_ref()) {
                    eprintln!("zode-app: clipboard write failed: {error}");
                }
            } else {
                eprintln!("zode-app: clipboard is unavailable");
            }
            if self.app_state.session_copy_menu.is_some() {
                self.app_state.close_session_action_surfaces();
                self.rebuild_frame_snapshot();
                self.request_redraw();
            }
            return;
        }
        let command = match self.apply_queue_intent(command) {
            super::super::queue::QueueIntentResult::Handled => return,
            super::super::queue::QueueIntentResult::Continue(command) => command,
        };
        if self.apply_presentation_command(command.clone()) {
            return;
        }
        let command = match self
            .provider_models_controller
            .consume(&mut self.app_state, command)
        {
            ProviderModelsCommandOutcome::NotHandled(command) => command,
            ProviderModelsCommandOutcome::Handled => {
                self.rebuild_frame_snapshot();
                self.request_redraw();
                return;
            }
            ProviderModelsCommandOutcome::Forward(command) => command,
        };
        let updates_window_theme = matches!(&command, AppCommand::SetThemePreference(_));
        let updates_reduced_motion = matches!(&command, AppCommand::SetReducedMotion(_));
        if reduce_local_settings_command(&mut self.app_state, command.clone())
            == SettingsCommandOutcome::Applied
        {
            if updates_window_theme {
                self.sync_window_theme();
            }
            if updates_reduced_motion {
                self.sync_primary_sidebar_transition();
                self.sync_right_panel_transition();
            }
            self.persist_ui_state();
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if reduce_tool_command(&mut self.app_state, command.clone()) == ToolCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        // Handles the back-to-bottom button (`SetTranscriptViewport`) and the
        // per-message reaction toggle (`SetMessageFeedback`) - both are
        // transcript-widget-local state, like `SetToolExpanded` above, and
        // never reach the endpoint command pump.
        if reduce_transcript_command(&mut self.app_state, command.clone())
            == TranscriptCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if self.apply_local_navigation_command(&command) {
            return;
        }
        let normalized = available_new_session_command(&self.app_state, &command)
            && normalize_conversation_route(&mut self.app_state, &command);
        if self.command_bridge.is_none() {
            eprintln!("zode-app: endpoint command ignored because no endpoint is attached");
            if normalized {
                self.rebuild_frame_snapshot();
                self.request_redraw();
            }
            return;
        }
        let previous_session = self.app_state.current_session.clone();
        match crate::command_bridge::prepare_dispatch(&mut self.app_state, command) {
            Ok(Some(dispatch)) => {
                if let Some(bridge) = self.command_bridge.as_ref() {
                    if let Err(dispatch) = bridge.dispatch(dispatch) {
                        crate::command_bridge::reject_dispatch(
                            &mut self.app_state,
                            dispatch,
                            "the endpoint command pump is unavailable".into(),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(error) => crate::command_bridge::project_command_error(
                &mut self.app_state,
                format!("invalid endpoint command: {error}"),
            ),
        }
        self.refresh_if_session_changed(previous_session);
        self.sync_composer_busy();
        self.rebuild_frame_snapshot();
        self.request_redraw();
    }
}
