use accesskit::{Action, ActionData};
use zode_app_model::{
    reduce_navigation_command, reduce_presentation_command, reduce_tool_command,
    reduce_transcript_command, AppCommand, NavigationOutcome, SettingsCommandOutcome, ShellRoute,
    ToolCommandOutcome, TranscriptCommandOutcome, ZodeAppState,
};
use zode_app_ui::{
    Composer, EmptyState, Key, KeyEvent, PointerButton, PointerEvent, PointerEventKind,
    ThreadHeader, ThreadTranscript, TouchPhase, UnifiedInputEvent, WheelDeltaMode, WidgetId,
    COMPOSER_ID, SEND_ID, SETTINGS_SEARCH_ID, TERMINAL_ID,
};

use super::{
    external_preview::consume_external_preview_command,
    panel_menu::close_panel_menu_command,
    session_menu::{consume_session_window_command, session_menu_outside_click_command},
    settings::{reduce_local_settings_command, settings_interaction_viewport},
    DesktopApp,
};
use crate::{
    accessibility_host::AccessibilityBridge,
    clipboard::{execute_clipboard_command, paste_from_clipboard},
    cursor::{cursor_hint_at, cursor_icon_for_hint},
    event_map::{is_paste_shortcut, terminal_shortcut_command},
    ime::set_cursor_area,
    input_dispatch::{
        dispatch_key, ime_allowed_for_focus, settings_scroll_delta_for_action,
        settings_scroll_delta_for_key, KeyDispatch, SettingsTouchOutcome,
    },
};

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;

#[path = "widget_commands.rs"]
mod widget_commands;
use widget_commands::widget_command;

pub(super) fn project_composer_outcome(
    state: &mut ZodeAppState,
    outcome: &zode_app_ui::ComposerOutcome,
) {
    match outcome {
        zode_app_ui::ComposerOutcome::AttachmentsChanged(attachments) => {
            state.composer.attachments.clone_from(attachments);
        }
        zode_app_ui::ComposerOutcome::Queue(_) => {
            state.composer.attachments.clear();
        }
        zode_app_ui::ComposerOutcome::Send(submission)
        | zode_app_ui::ComposerOutcome::Steer(submission) => {
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
        zode_app_ui::ComposerOutcome::Ignored
        | zode_app_ui::ComposerOutcome::Edited
        | zode_app_ui::ComposerOutcome::Stop
        | zode_app_ui::ComposerOutcome::SetModel(_)
        | zode_app_ui::ComposerOutcome::SetEffort(_)
        | zode_app_ui::ComposerOutcome::SetSandbox(_) => {}
    }
}

fn normalize_conversation_route(
    state: &mut zode_app_model::ZodeAppState,
    command: &AppCommand,
) -> bool {
    if !matches!(
        command,
        AppCommand::SelectSession(_) | AppCommand::NewSession { .. } | AppCommand::BeginTask { .. }
    ) {
        return false;
    }
    let _ = reduce_presentation_command(state, AppCommand::CloseSecondary);
    let _ = reduce_presentation_command(state, AppCommand::Navigate(ShellRoute::Conversation));
    true
}

fn available_new_session_command(
    state: &zode_app_model::ZodeAppState,
    command: &AppCommand,
) -> bool {
    matches!(
        command,
        AppCommand::NewSession { workspace_uri } if state.available_workspace(workspace_uri)
    )
}

impl DesktopApp {
    pub(super) fn enqueue_command(&mut self, command: AppCommand) {
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
            super::queue::QueueIntentResult::Handled => return,
            super::queue::QueueIntentResult::Continue(command) => command,
        };
        if self.apply_presentation_command(command.clone()) {
            return;
        }
        if reduce_local_settings_command(&mut self.app_state, command.clone())
            == SettingsCommandOutcome::Applied
        {
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

    pub(super) fn handle_unified_input(&mut self, input: UnifiedInputEvent) {
        match input {
            UnifiedInputEvent::Keyboard(event) => self.handle_key_event(event),
            UnifiedInputEvent::Ime(event) => {
                if self.handle_session_rename_ime(event.clone()) {
                    return;
                }
                if self.handle_settings_search_ime(&event) {
                    return;
                }
                if self.handle_project_picker_ime(event.clone()) {
                    return;
                }
                if self.app_state.terminal_surface_visible()
                    && self.focused_widget == Some(TERMINAL_ID)
                {
                    if let (Some(id), zode_app_ui::ImeEvent::Commit(text)) =
                        (self.app_state.terminal.active_id, event)
                    {
                        self.apply_terminal_command(AppCommand::WriteTerminal {
                            id,
                            bytes: text.into_bytes(),
                        });
                    }
                } else if self.focused_widget == Some(COMPOSER_ID) {
                    let outcome = self.composer.ime(event);
                    self.apply_composer_outcome(outcome);
                }
            }
            UnifiedInputEvent::Pointer(event) => self.handle_pointer_event(event),
            UnifiedInputEvent::Wheel(event) => {
                let multiplier = match event.mode {
                    WheelDeltaMode::Line => 20.0,
                    WheelDeltaMode::Pixel => 1.0,
                };
                let delta = -event.delta_y * multiplier;
                if self.handle_sidebar_scroll_delta(delta) {
                    return;
                }
                let terminal_under_pointer = self.app_state.terminal_surface_visible()
                    && self
                        .terminal_rect()
                        .contains(self.window_state.cursor_logical);
                if terminal_under_pointer {
                    let command = self.terminal_controller.scroll_command(
                        &self.app_state.terminal,
                        &self.terminal_grid,
                        self.terminal_rect().size.y,
                        delta,
                    );
                    self.apply_terminal_command(command);
                } else if self.app_state.presentation.route == ShellRoute::Conversation {
                    let empty = std::collections::BTreeMap::new();
                    let command = self.app_state.current_session.as_ref().and_then(|session| {
                        self.app_state.transcripts.get(session).map(|transcript| {
                            let tool_expanded =
                                self.app_state.tool_expanded.get(session).unwrap_or(&empty);
                            ThreadTranscript::scroll_command(
                                session.clone(),
                                self.frame_snapshot.layout.transcript,
                                transcript,
                                tool_expanded,
                                delta,
                            )
                        })
                    });
                    if let Some(command) = command {
                        if reduce_transcript_command(&mut self.app_state, command)
                            == TranscriptCommandOutcome::Applied
                        {
                            self.rebuild_frame_snapshot();
                            self.request_redraw();
                        }
                    }
                } else if matches!(self.app_state.presentation.route, ShellRoute::Settings(_)) {
                    self.apply_settings_scroll_delta(delta);
                }
            }
            UnifiedInputEvent::Touch(event) => {
                if matches!(self.app_state.presentation.route, ShellRoute::Settings(_)) {
                    let viewport = settings_interaction_viewport(&self.frame_snapshot);
                    match self.settings_touch.handle(event, viewport) {
                        SettingsTouchOutcome::Captured => return,
                        SettingsTouchOutcome::Scroll(delta) => {
                            self.apply_settings_scroll_delta(delta);
                            return;
                        }
                        SettingsTouchOutcome::Tap(position) => {
                            self.handle_pointer_event(PointerEvent {
                                position,
                                kind: PointerEventKind::Press,
                                button: Some(PointerButton::Primary),
                            });
                            return;
                        }
                        SettingsTouchOutcome::Ignored => {}
                    }
                }
                let kind = match event.phase {
                    TouchPhase::Started => PointerEventKind::Press,
                    TouchPhase::Moved => PointerEventKind::Move,
                    TouchPhase::Ended | TouchPhase::Cancelled => PointerEventKind::Release,
                };
                self.handle_pointer_event(PointerEvent {
                    position: event.position,
                    kind,
                    button: Some(PointerButton::Primary),
                });
            }
        }
    }

    pub(super) fn handle_paste(&mut self) {
        let Some(clipboard) = self.clipboard.clone() else {
            return;
        };
        if self.paste_session_rename_from_clipboard(clipboard.as_ref()) {
            return;
        }
        if self.app_state.project_picker.open
            && self.focused_widget == Some(zode_app_ui::PROJECT_PICKER_SEARCH_ID)
        {
            match clipboard.read_text() {
                Ok(Some(text)) if !text.is_empty() => {
                    let _ = self.paste_project_search_text(&text);
                }
                Ok(_) => {}
                Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
            }
            return;
        }
        if self.focused_widget == Some(SETTINGS_SEARCH_ID) {
            match clipboard.read_text() {
                Ok(Some(text)) if !text.is_empty() => {
                    let _ = self.paste_settings_search_text(&text);
                }
                Ok(_) => {}
                Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
            }
            return;
        }
        if self.app_state.terminal_surface_visible() && self.focused_widget == Some(TERMINAL_ID) {
            match clipboard.read_text() {
                Ok(Some(text)) if !text.is_empty() => {
                    if let Some(id) = self.app_state.terminal.active_id {
                        self.apply_terminal_command(AppCommand::WriteTerminal {
                            id,
                            bytes: text.into_bytes(),
                        });
                    }
                }
                Ok(_) => {}
                Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
            }
            return;
        }
        match paste_from_clipboard(clipboard.as_ref(), &mut self.composer) {
            Ok(0) => {}
            Ok(_) => {
                let attachments = self.composer.attachment_metadata().to_vec();
                let outcome = if attachments.is_empty() {
                    zode_app_ui::ComposerOutcome::Edited
                } else {
                    zode_app_ui::ComposerOutcome::AttachmentsChanged(attachments)
                };
                self.apply_composer_outcome(outcome);
            }
            Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
        }
    }

    pub(super) fn set_focused_widget(&mut self, focused: Option<WidgetId>) {
        let focused = focused.filter(|id| self.frame_snapshot.node(*id).is_some());
        self.focused_widget = focused;
        self.app_state.composer.focused = self.window_focused && focused == Some(COMPOSER_ID);
        let terminal_focused = self.window_focused
            && self.app_state.terminal_surface_visible()
            && focused == Some(TERMINAL_ID);
        let _ = zode_app_model::reduce_terminal_command(
            &mut self.app_state,
            AppCommand::SetTerminalFocus(terminal_focused),
        );
        self.sync_ime_for_focus(focused, self.window_focused);
        self.request_redraw();
    }

    fn sync_ime_for_focus(&self, focused: Option<WidgetId>, window_focused: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let ime_page = if self.app_state.terminal_surface_visible() && focused == Some(TERMINAL_ID)
        {
            zode_app_model::ShellPage::Terminal
        } else {
            self.app_state.presentation.route.legacy_page()
        };
        let allowed = ime_allowed_for_focus(ime_page, focused, window_focused);
        if allowed {
            if let Some(rect) = focused.and_then(|id| self.frame_snapshot.node(id).map(|n| n.rect))
            {
                set_cursor_area(window, rect);
            }
        }
        window.set_ime_allowed(allowed);
    }

    pub(super) fn activate_widget(&mut self, id: WidgetId) {
        if let Some(prompt) = EmptyState::suggestion_prompt(id) {
            self.composer.set_text(prompt);
            self.set_focused_widget(Some(COMPOSER_ID));
            self.apply_composer_outcome(zode_app_ui::ComposerOutcome::Edited);
            return;
        }
        self.set_focused_widget(Some(id));
        match id {
            SEND_ID => {
                self.set_focused_widget(Some(COMPOSER_ID));
                let busy = self
                    .app_state
                    .current_session
                    .as_ref()
                    .and_then(|session| self.app_state.transcripts.get(session))
                    .is_some_and(|transcript| transcript.busy);
                self.composer.set_busy(busy);
                let outcome = if busy {
                    self.composer.stop()
                } else {
                    self.composer.key(Key::Enter, zode_app_ui::Modifiers::NONE)
                };
                self.apply_composer_outcome(outcome);
            }
            COMPOSER_ID
            | TERMINAL_ID
            | SETTINGS_SEARCH_ID
            | zode_app_ui::PROJECT_PICKER_SEARCH_ID
            | zode_app_ui::HEADER_RENAME_INPUT_ID => {}
            _ => {
                if let Some(command) = widget_command(&self.app_state, id) {
                    let focus_composer =
                        matches!(command, AppCommand::BeginEditQueuedMessage { .. });
                    let opening_queue_menu = match &command {
                        AppCommand::ToggleQueuedMessageMenu { id, .. }
                            if self.app_state.composer.queue_menu != Some(*id) =>
                        {
                            Some(*id)
                        }
                        _ => None,
                    };
                    self.enqueue_command(command);
                    if focus_composer {
                        self.set_focused_widget(Some(COMPOSER_ID));
                    } else if let Some(message_id) = opening_queue_menu {
                        self.focus_open_queue_menu(message_id);
                    }
                }
            }
        }
    }

    pub(super) fn drain_accessibility_actions(&mut self) {
        let requests = self
            .a11y
            .as_mut()
            .map(AccessibilityBridge::drain_actions)
            .unwrap_or_default();
        for request in requests {
            let id = WidgetId(request.target_node.0);
            if self.frame_snapshot.node(id).is_none() {
                continue;
            }
            if !self.project_picker_allows_accessibility_action(id) {
                continue;
            }
            match request.action {
                Action::Focus => self.set_focused_widget(Some(id)),
                Action::Click => self.activate_widget(id),
                Action::SetValue if id == COMPOSER_ID => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.composer.set_text(value.into_string());
                        self.apply_composer_outcome(zode_app_ui::ComposerOutcome::Edited);
                    }
                }
                Action::SetValue if id == zode_app_ui::PROJECT_PICKER_SEARCH_ID => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.set_project_search_value(value.into_string());
                    }
                }
                Action::SetValue if id == SETTINGS_SEARCH_ID => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.set_settings_search_value(value.into_string());
                    }
                }
                Action::SetValue if id == zode_app_ui::HEADER_RENAME_INPUT_ID => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.set_session_rename_value(value.into_string());
                    }
                }
                Action::ScrollUp | Action::ScrollDown if id == zode_app_ui::SETTINGS_ROOT_ID => {
                    if let Some(delta) = settings_scroll_delta_for_action(
                        request.action,
                        settings_interaction_viewport(&self.frame_snapshot).size.y,
                    ) {
                        self.apply_settings_scroll_delta(delta);
                    }
                }
                Action::ScrollUp | Action::ScrollDown if id == zode_app_ui::SIDEBAR_ID => {
                    let _ = self.handle_sidebar_accessibility_scroll(request.action);
                }
                _ => {}
            }
        }
    }

    pub(super) fn sync_window_focus(&mut self, focused: bool) {
        self.window_focused = focused;
        self.app_state.composer.focused = focused && self.focused_widget == Some(COMPOSER_ID);
        let terminal_focused = focused
            && self.app_state.terminal_surface_visible()
            && self.focused_widget == Some(TERMINAL_ID);
        let _ = zode_app_model::reduce_terminal_command(
            &mut self.app_state,
            AppCommand::SetTerminalFocus(terminal_focused),
        );
        self.sync_ime_for_focus(self.focused_widget, focused);
        self.request_redraw();
    }

    fn handle_pointer_event(&mut self, event: PointerEvent) {
        self.window_state.cursor_logical = event.position;
        if event.kind == PointerEventKind::Move {
            let hovered = self.frame_snapshot.hit_test(event.position);
            if self.hovered_widget != hovered {
                self.hovered_widget = hovered;
                self.request_sidebar_hover_context(hovered);
                self.request_redraw();
            }
            if let Some(window) = self.window.as_ref() {
                window.set_cursor(cursor_icon_for_hint(cursor_hint_at(
                    &self.frame_snapshot,
                    event.position,
                )));
            }
            if self.app_state.terminal_surface_visible()
                && self.terminal_controller.pointer_move(
                    self.terminal_rect(),
                    event.position,
                    &self.terminal_grid,
                    self.app_state.terminal.scroll_offset,
                )
            {
                self.request_redraw();
            }
            return;
        }
        if event.kind == PointerEventKind::Release {
            if event.button == Some(PointerButton::Primary) {
                self.terminal_controller.pointer_up();
            }
            return;
        }
        if event.button != Some(PointerButton::Primary) {
            return;
        }
        if self.app_state.project_picker.open {
            if !self.project_picker_contains_point(event.position) {
                self.close_project_picker_from_outside();
                return;
            }
            if self.frame_snapshot.hit_test(event.position).is_none() {
                return;
            }
        }
        if let Some(command) = session_menu_outside_click_command(
            &self.app_state,
            &self.frame_snapshot,
            event.position,
        ) {
            self.enqueue_command(command);
            return;
        }
        if self.handle_panel_menu_pointer(event.position) {
            return;
        }
        if self.app_state.session_menu.is_some() {
            let actionable = self
                .frame_snapshot
                .hit_test(event.position)
                .is_some_and(|id| ThreadHeader::command_for_widget(&self.app_state, id).is_some());
            if !actionable {
                return;
            }
        }
        if let Some(open) = self.app_state.composer.queue_menu {
            let menu = Composer::queue_layout(self.frame_snapshot.layout.composer, &self.app_state)
                .and_then(|layout| layout.menu);
            let inside_menu = menu
                .as_ref()
                .is_some_and(|menu| menu.rect.contains(event.position));
            if !inside_menu {
                if let Some(session) = self.app_state.current_session.clone() {
                    self.close_queue_menu_and_restore_focus(session, open);
                }
                return;
            }
            let actionable = self
                .frame_snapshot
                .hit_test(event.position)
                .is_some_and(|id| Composer::command_for_widget(&self.app_state, id).is_some());
            if !actionable {
                return;
            }
        }
        if self.app_state.terminal_surface_visible()
            && self.frame_snapshot.hit_test(event.position) == Some(TERMINAL_ID)
        {
            self.set_focused_widget(Some(TERMINAL_ID));
            if let Some(command) = self.terminal_controller.pointer_down(
                self.terminal_rect(),
                event.position,
                &self.terminal_grid,
                self.app_state.terminal.scroll_offset,
            ) {
                self.apply_terminal_command(command);
            }
            return;
        }
        if let Some(id) = self.frame_snapshot.hit_test(event.position) {
            self.activate_widget(id);
        } else {
            self.begin_window_gesture();
        }
    }

    fn handle_key_event(&mut self, event: KeyEvent) {
        if self.handle_session_action_key(&event) {
            return;
        }
        if self.handle_project_picker_key(&event) {
            return;
        }
        if self.handle_settings_search_key(&event) {
            return;
        }
        if self.handle_sidebar_shortcut(&event) {
            return;
        }
        if let Some(command) = terminal_shortcut_command(&event) {
            self.apply_terminal_command(command);
            return;
        }
        let terminal_focused =
            self.app_state.terminal_surface_visible() && self.focused_widget == Some(TERMINAL_ID);
        if is_paste_shortcut(&event, terminal_focused) {
            self.handle_paste();
            return;
        }
        if event.pressed && event.key == Key::Escape {
            if let Some(command) = close_panel_menu_command(&self.app_state) {
                self.enqueue_command(command);
                return;
            }
            if let (Some(session), Some(id)) = (
                self.app_state.current_session.clone(),
                self.app_state.composer.queue_menu,
            ) {
                self.close_queue_menu_and_restore_focus(session, id);
                return;
            }
            if let Some(session) = self
                .app_state
                .current_session
                .clone()
                .filter(|_| self.app_state.composer.editing_queued_message.is_some())
            {
                self.enqueue_command(AppCommand::CancelQueuedMessageEdit { session });
                return;
            }
        }
        if event.pressed && event.key == Key::Tab && self.app_state.composer.queue_menu.is_some() {
            let backwards = event.modifiers.contains(zode_app_ui::Modifiers::SHIFT);
            if self.trap_queue_menu_tab(backwards) {
                return;
            }
        }
        if event.pressed
            && event.key == Key::Escape
            && matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
        {
            self.enqueue_command(AppCommand::Navigate(ShellRoute::Conversation));
            self.set_focused_widget(Some(COMPOSER_ID));
            return;
        }
        if event.pressed && matches!(self.app_state.presentation.route, ShellRoute::Settings(_)) {
            if let Some(delta) = settings_scroll_delta_for_key(
                &event.key,
                settings_interaction_viewport(&self.frame_snapshot).size.y,
            ) {
                self.apply_settings_scroll_delta(delta);
                return;
            }
        }

        match dispatch_key(
            &self.frame_snapshot,
            self.focused_widget,
            &event,
            terminal_focused,
        ) {
            KeyDispatch::FocusChanged(focused) => self.set_focused_widget(focused),
            KeyDispatch::TerminalPty => {
                let _ = self.handle_terminal_key(&event);
            }
            KeyDispatch::Widget => {
                let activate = event.pressed
                    && (event.key == Key::Enter
                        || matches!(&event.key, Key::Character(value) if value == " "));
                if activate && self.focused_widget != Some(COMPOSER_ID) {
                    if let Some(focused) = self.focused_widget {
                        self.activate_widget(focused);
                    }
                } else if self.focused_widget == Some(COMPOSER_ID) && event.pressed {
                    let outcome = self.composer.key(event.key, event.modifiers);
                    self.apply_composer_outcome(outcome);
                }
            }
        }
    }

    fn apply_local_navigation_command(&mut self, command: &AppCommand) -> bool {
        let previous_session = self.app_state.current_session.clone();
        let previous_queue_edit = self.app_state.composer.editing_queued_message;
        let project_picker_was_open = self.app_state.project_picker.open;
        let outcome = reduce_navigation_command(&mut self.app_state, command.clone());
        let session_focus = self.sync_session_action_after_navigation(command);
        self.sync_queue_editor_after_state_change(previous_session.clone(), previous_queue_edit);
        self.prune_queued_payloads();
        let handled = match outcome {
            NavigationOutcome::Applied => true,
            NavigationOutcome::NeedsEffect if matches!(command, AppCommand::CreateProject) => {
                self.request_workspace_pick();
                true
            }
            NavigationOutcome::NeedsEffect
                if matches!(
                    command,
                    AppCommand::ToggleProject(_)
                        | AppCommand::SetSessionPinned { .. }
                        | AppCommand::SetSessionArchived { .. }
                        | AppCommand::RequestDeleteSession(_)
                ) =>
            {
                let _ = self.persist_local_navigation_effect(command);
                true
            }
            NavigationOutcome::NeedsEffect | NavigationOutcome::Ignored => false,
        };
        if !handled {
            return false;
        }
        normalize_conversation_route(&mut self.app_state, command);
        if matches!(
            command,
            AppCommand::BeginTask { .. }
                | AppCommand::SelectSession(_)
                | AppCommand::SetSessionArchived { archived: true, .. }
        ) {
            self.persist_ui_state();
        }
        let focus_after = session_focus.or_else(|| {
            self.sync_project_picker_after_navigation(command, project_picker_was_open)
        });
        self.refresh_if_session_changed(previous_session);
        self.sync_composer_busy();
        self.rebuild_frame_snapshot();
        if let Some(id) = focus_after {
            self.set_focused_widget(Some(id));
        } else {
            self.request_redraw();
        }
        true
    }
}
