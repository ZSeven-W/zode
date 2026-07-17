use accesskit::{Action, ActionData};
use zode_app_model::{
    reduce_navigation_command, reduce_settings_command, reduce_tool_command,
    reduce_transcript_command, AppCommand, NavigationOutcome, SettingsCommandOutcome, ShellPage,
    ThemePreference, ToolCommandOutcome, TranscriptCommandOutcome,
};
use zode_app_ui::{
    Key, KeyEvent, PointerButton, PointerEvent, PointerEventKind, ProjectSidebar, SettingsPanel,
    ThreadTranscript, TouchPhase, UnifiedInputEvent, WheelDeltaMode, WidgetId, COMPOSER_ID,
    HIGH_CONTRAST_ID, NEW_SESSION_ID, REDUCED_MOTION_ID, SEND_ID, SETTINGS_NAV_ID, TERMINAL_ID,
    THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

use super::DesktopApp;
use crate::{
    accessibility_host::AccessibilityBridge,
    clipboard::{execute_clipboard_command, paste_from_clipboard},
    cursor::{cursor_hint_at, cursor_icon_for_hint},
    event_map::{is_paste_shortcut, terminal_shortcut_command},
    input_dispatch::{
        dispatch_key, ime_allowed_for_focus, settings_scroll_delta_for_action,
        settings_scroll_delta_for_key, KeyDispatch, SettingsTouchOutcome,
    },
    window_state::{update_window_geometry, WindowGeometry},
};

impl DesktopApp {
    pub(super) fn enqueue_command(&mut self, command: AppCommand) {
        if matches!(command, AppCommand::CopyText(_)) {
            if let Some(clipboard) = self.clipboard.as_ref() {
                if let Err(error) = execute_clipboard_command(&command, clipboard.as_ref()) {
                    eprintln!("zode-app: clipboard write failed: {error}");
                }
            } else {
                eprintln!("zode-app: clipboard is unavailable");
            }
            return;
        }
        if self.persist_local_navigation_effect(&command) {
            return;
        }
        if self.command_bridge.is_none() {
            eprintln!("zode-app: endpoint command ignored because no endpoint is attached");
            return;
        }
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
        self.sync_composer_busy();
        self.rebuild_frame_snapshot();
        self.request_redraw();
    }

    pub(super) fn handle_unified_input(&mut self, input: UnifiedInputEvent) {
        match input {
            UnifiedInputEvent::Keyboard(event) => self.handle_key_event(event),
            UnifiedInputEvent::Ime(event) => {
                if self.app_state.shell.page == ShellPage::Terminal
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
                if self.app_state.shell.page == ShellPage::Terminal {
                    let command = self.terminal_controller.scroll_command(
                        &self.app_state.terminal,
                        &self.terminal_grid,
                        self.terminal_rect().size.y,
                        -event.delta_y * multiplier,
                    );
                    self.apply_terminal_command(command);
                } else if self.app_state.shell.page == ShellPage::Conversation {
                    let command = self.app_state.current_session.as_ref().and_then(|session| {
                        self.app_state.transcripts.get(session).map(|transcript| {
                            ThreadTranscript::scroll_command(
                                session.clone(),
                                self.frame_snapshot.layout.transcript,
                                transcript,
                                &self.app_state.tool_expanded,
                                -event.delta_y * multiplier,
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
                } else if self.app_state.shell.page == ShellPage::Settings {
                    self.apply_settings_scroll_delta(-event.delta_y * multiplier);
                }
            }
            UnifiedInputEvent::Touch(event) => {
                if self.app_state.shell.page == ShellPage::Settings {
                    match self
                        .settings_touch
                        .handle(event, self.frame_snapshot.layout.transcript)
                    {
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
        if self.app_state.shell.page == ShellPage::Terminal
            && self.focused_widget == Some(TERMINAL_ID)
        {
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
            Ok(_) => self.apply_composer_outcome(zode_app_ui::ComposerOutcome::Edited),
            Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
        }
    }

    pub(super) fn set_focused_widget(&mut self, focused: Option<WidgetId>) {
        let focused = focused.filter(|id| self.frame_snapshot.node(*id).is_some());
        self.focused_widget = focused;
        self.app_state.composer.focused = self.window_focused && focused == Some(COMPOSER_ID);
        let terminal_focused = self.window_focused
            && self.app_state.shell.page == ShellPage::Terminal
            && focused == Some(TERMINAL_ID);
        let _ = zode_app_model::reduce_terminal_command(
            &mut self.app_state,
            AppCommand::SetTerminalFocus(terminal_focused),
        );
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(ime_allowed_for_focus(
                self.app_state.shell.page,
                focused,
                self.window_focused,
            ));
        }
        self.request_redraw();
    }

    pub(super) fn activate_widget(&mut self, id: WidgetId) {
        self.set_focused_widget(Some(id));
        if let Some(command) = ProjectSidebar::command_for_widget(&self.app_state, id) {
            match reduce_navigation_command(&mut self.app_state, command.clone()) {
                NavigationOutcome::Applied => {}
                NavigationOutcome::NeedsEffect => self.enqueue_command(command),
                NavigationOutcome::Ignored => return,
            }
            self.sync_composer_busy();
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if let Some(command) = ThreadTranscript::command_for_widget(&self.app_state, id) {
            match command {
                command @ AppCommand::SetToolExpanded { .. } => {
                    if reduce_tool_command(&mut self.app_state, command)
                        == ToolCommandOutcome::Applied
                    {
                        self.rebuild_frame_snapshot();
                        self.request_redraw();
                    }
                }
                command @ AppCommand::Approve { .. } => self.enqueue_command(command),
                _ => {}
            }
            return;
        }
        if let Some(command) = SettingsPanel::command_for_widget(&self.app_state, id) {
            self.enqueue_command(command);
            return;
        }
        match id {
            SEND_ID => {
                self.set_focused_widget(Some(COMPOSER_ID));
                let outcome = self.composer.key(Key::Enter, zode_app_ui::Modifiers::NONE);
                self.apply_composer_outcome(outcome);
            }
            SETTINGS_NAV_ID if self.app_state.shell.page != ShellPage::Settings => {
                self.app_state.shell.page = ShellPage::Settings;
                self.rebuild_frame_snapshot();
                self.set_focused_widget(self.frame_snapshot.focused);
            }
            NEW_SESSION_ID => {
                let workspace = self
                    .app_state
                    .active_available_workspace()
                    .cloned()
                    .or_else(|| {
                        self.app_state
                            .projects
                            .iter()
                            .find(|project| project.available)
                            .map(|project| project.workspace_uri.clone())
                    });
                if let Some(workspace_uri) = workspace {
                    self.enqueue_command(AppCommand::NewSession { workspace_uri });
                }
            }
            THEME_SYSTEM_ID => {
                self.apply_setting(AppCommand::SetThemePreference(ThemePreference::System));
            }
            THEME_LIGHT_ID => {
                self.apply_setting(AppCommand::SetThemePreference(ThemePreference::Light));
            }
            THEME_DARK_ID => {
                self.apply_setting(AppCommand::SetThemePreference(ThemePreference::Dark));
            }
            REDUCED_MOTION_ID => {
                self.apply_setting(AppCommand::SetReducedMotion(
                    !self.app_state.ui_preferences.reduced_motion,
                ));
            }
            HIGH_CONTRAST_ID => {
                self.apply_setting(AppCommand::SetHighContrast(
                    !self.app_state.ui_preferences.high_contrast,
                ));
            }
            COMPOSER_ID | TERMINAL_ID | SETTINGS_NAV_ID => {}
            _ => {}
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
            match request.action {
                Action::Focus => self.set_focused_widget(Some(id)),
                Action::Click => self.activate_widget(id),
                Action::SetValue if id == COMPOSER_ID => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.composer.set_text(value.into_string());
                        self.apply_composer_outcome(zode_app_ui::ComposerOutcome::Edited);
                    }
                }
                Action::ScrollUp | Action::ScrollDown if id == zode_app_ui::SETTINGS_ROOT_ID => {
                    if let Some(delta) = settings_scroll_delta_for_action(
                        request.action,
                        self.frame_snapshot.layout.transcript.size.y,
                    ) {
                        self.apply_settings_scroll_delta(delta);
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn record_window_geometry(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        let minimized = window.is_minimized().unwrap_or(false);
        if minimized || size.width == 0 || size.height == 0 {
            return;
        }
        let fallback = self.window_geometry.unwrap_or(WindowGeometry {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
            maximized: false,
        });
        let position = window
            .outer_position()
            .unwrap_or(winit::dpi::PhysicalPosition::new(fallback.x, fallback.y));
        let maximized = window.is_maximized();
        let reported = WindowGeometry {
            x: position.x,
            y: position.y,
            width: size.width.max(1),
            height: size.height.max(1),
            maximized,
        };
        if let Some(saved) = self.window_geometry.as_mut() {
            update_window_geometry(saved, reported, maximized, minimized);
        } else {
            self.window_geometry = Some(reported);
        }
    }

    pub(super) fn persist_ui_state(&self) {
        let Some(store) = self.app_state_store.as_ref() else {
            return;
        };
        let preferences = self.app_state.ui_preferences.clone();
        let geometry = self.window_geometry;
        let last_session = self
            .app_state
            .current_session
            .as_ref()
            .map(|session| session.session_id.clone());
        if let Err(error) = store.update(move |state| {
            state.ui_preferences = preferences;
            state.window_geometry = geometry;
            state.last_session = last_session;
        }) {
            eprintln!("zode-app: failed to persist UI state: {error}");
        }
    }

    pub(super) fn request_redraw(&mut self) {
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(super) fn update_accessibility_window_bounds(&mut self) {
        if let (Some(a11y), Some(window)) = (self.a11y.as_mut(), self.window.as_ref()) {
            a11y.update_window_bounds(window);
        }
    }

    pub(super) fn sync_window_focus(&mut self, focused: bool) {
        self.window_focused = focused;
        self.app_state.composer.focused = focused && self.focused_widget == Some(COMPOSER_ID);
        let terminal_focused = focused
            && self.app_state.shell.page == ShellPage::Terminal
            && self.focused_widget == Some(TERMINAL_ID);
        let _ = zode_app_model::reduce_terminal_command(
            &mut self.app_state,
            AppCommand::SetTerminalFocus(terminal_focused),
        );
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(ime_allowed_for_focus(
                self.app_state.shell.page,
                self.focused_widget,
                focused,
            ));
        }
        self.request_redraw();
    }

    fn handle_pointer_event(&mut self, event: PointerEvent) {
        self.window_state.cursor_logical = event.position;
        if event.kind == PointerEventKind::Move {
            if let Some(window) = self.window.as_ref() {
                window.set_cursor(cursor_icon_for_hint(cursor_hint_at(
                    &self.frame_snapshot,
                    event.position,
                )));
            }
            if self.app_state.shell.page == ShellPage::Terminal
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
        if self.app_state.shell.page == ShellPage::Terminal
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
        if let Some(command) = terminal_shortcut_command(&event) {
            self.apply_terminal_command(command);
            return;
        }
        let terminal_focused = self.app_state.shell.page == ShellPage::Terminal
            && self.focused_widget == Some(TERMINAL_ID);
        if is_paste_shortcut(&event, terminal_focused) {
            self.handle_paste();
            return;
        }
        if event.pressed
            && event.key == Key::Escape
            && self.app_state.shell.page == ShellPage::Settings
        {
            self.app_state.shell.page = ShellPage::Conversation;
            self.rebuild_frame_snapshot();
            self.set_focused_widget(Some(COMPOSER_ID));
            return;
        }
        if event.pressed && self.app_state.shell.page == ShellPage::Settings {
            if let Some(delta) = settings_scroll_delta_for_key(
                &event.key,
                self.frame_snapshot.layout.transcript.size.y,
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

    fn apply_setting(&mut self, command: AppCommand) {
        if reduce_settings_command(&mut self.app_state, command) == SettingsCommandOutcome::Applied
        {
            self.persist_ui_state();
            self.request_redraw();
        }
    }

    fn apply_settings_scroll_delta(&mut self, delta: f32) {
        let command = SettingsPanel::scroll_command(
            self.frame_snapshot.layout.transcript,
            &self.app_state,
            delta,
        );
        if reduce_settings_command(&mut self.app_state, command) == SettingsCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }

    fn persist_local_navigation_effect(&self, command: &AppCommand) -> bool {
        let Some(store) = self.app_state_store.as_ref() else {
            return matches!(
                command,
                AppCommand::ToggleProject(_) | AppCommand::SetSessionPinned { .. }
            );
        };
        let result = match command {
            AppCommand::ToggleProject(workspace_uri) => {
                let collapsed = self
                    .app_state
                    .projects
                    .iter()
                    .find(|project| &project.workspace_uri == workspace_uri)
                    .is_some_and(|project| !project.expanded);
                let key = workspace_uri.as_str().to_owned();
                store.update(move |state| {
                    if collapsed {
                        state.collapsed_workspaces.insert(key);
                    } else {
                        state.collapsed_workspaces.remove(&key);
                    }
                })
            }
            AppCommand::SetSessionPinned { session, pinned } => {
                let key = session.session_id.clone();
                let pinned = *pinned;
                store.update(move |state| {
                    state.sessions.entry(key).or_default().pinned = pinned;
                })
            }
            _ => return false,
        };
        if let Err(error) = result {
            eprintln!("zode-app: navigation state could not be persisted: {error}");
        }
        true
    }
}
