use jian_widgets::Rect;
use zode_app_model::{reduce_terminal_command, AppCommand, ShellPage, TerminalCommandOutcome};
use zode_app_ui::{KeyEvent, TerminalPanel, TerminalPanelController, TERMINAL_ID};

use super::DesktopApp;
use crate::event_map::terminal_shortcut_command;

impl DesktopApp {
    pub(super) fn apply_terminal_command(&mut self, command: AppCommand) {
        if command == AppCommand::OpenTerminal {
            let _ = reduce_terminal_command(&mut self.app_state, command);
            if self.terminal_runtime.active_id().is_none()
                && self.app_state.terminal.unavailable_reason.is_none()
            {
                let cwd = self.terminal_cwd();
                let (cols, rows) = self.terminal_grid.size();
                match self.terminal_runtime.open(
                    &cwd,
                    u16::try_from(cols).unwrap_or(u16::MAX),
                    u16::try_from(rows).unwrap_or(u16::MAX),
                ) {
                    Ok(id) => self.app_state.terminal.active_id = Some(id),
                    Err(error) => {
                        self.app_state.terminal.unavailable_reason = Some(error.to_string())
                    }
                }
            }
            self.rebuild_frame_snapshot();
            self.set_focused_widget(Some(TERMINAL_ID));
        } else {
            match reduce_terminal_command(&mut self.app_state, command.clone()) {
                TerminalCommandOutcome::NeedsEffect => {
                    let result = match command {
                        AppCommand::WriteTerminal { id, bytes } => {
                            self.terminal_runtime.write(id, bytes)
                        }
                        AppCommand::ResizeTerminal { id, cols, rows } => {
                            self.terminal_runtime.resize(id, cols, rows)
                        }
                        AppCommand::CloseTerminal(id) => {
                            let result = self.terminal_runtime.close(id);
                            if result.is_ok() {
                                self.app_state.terminal.active_id = None;
                                self.app_state.terminal.open = false;
                                self.app_state.terminal.focused = false;
                            }
                            result
                        }
                        _ => Ok(()),
                    };
                    if let Err(error) = result {
                        self.app_state.terminal.unavailable_reason = Some(error.to_string());
                    }
                }
                TerminalCommandOutcome::Applied => {}
                TerminalCommandOutcome::Ignored => return,
            }
        }
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn terminal_cwd(&self) -> std::path::PathBuf {
        self.app_state
            .active_available_workspace()
            .and_then(|workspace| crate::services::workspace_root(workspace).ok())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    pub(super) fn drain_terminal_output(&mut self) {
        let mut changed = false;
        for output in self.terminal_runtime.drain_output() {
            match output {
                Ok(bytes) => {
                    self.terminal_grid.feed(&bytes);
                    changed = true;
                }
                Err(error) => self.app_state.terminal.unavailable_reason = Some(error.to_string()),
            }
        }
        if changed && self.app_state.terminal.follow_tail {
            self.app_state.terminal.scroll_offset = TerminalPanel::tail_offset(
                self.terminal_grid.line_count(),
                self.terminal_rect().size.y,
            );
        }
        match self.terminal_runtime.reap_finished() {
            Ok(Some(id)) if self.app_state.terminal.active_id == Some(id) => {
                self.app_state.terminal.active_id = None;
                self.app_state.terminal.open = false;
                self.app_state.terminal.focused = false;
            }
            Ok(_) => {}
            Err(error) => self.app_state.terminal.unavailable_reason = Some(error.to_string()),
        }
    }

    pub(super) fn terminal_rect(&self) -> Rect {
        let geometry = self.frame_snapshot.layout;
        Rect::xywh(
            geometry.transcript.origin.x,
            geometry.transcript.origin.y,
            geometry.transcript.size.x,
            geometry.composer.origin.y + geometry.composer.size.y - geometry.transcript.origin.y,
        )
    }

    pub(super) fn handle_terminal_key(&mut self, event: &KeyEvent) -> bool {
        if let Some(command) = terminal_shortcut_command(event) {
            self.apply_terminal_command(command);
            return true;
        }
        if self.app_state.shell.page != ShellPage::Terminal || !self.app_state.terminal.focused {
            return false;
        }
        if TerminalPanelController::is_copy_shortcut(event) {
            if let Some(command) = self.terminal_controller.copy_command(&self.terminal_grid) {
                self.enqueue_command(command);
            }
            return true;
        }
        if let Some(command) = self
            .terminal_controller
            .key_command(&self.app_state.terminal, event)
        {
            self.apply_terminal_command(command);
        }
        true
    }

    pub(super) fn resize_terminal_grid(&mut self) {
        let rect = self.terminal_rect();
        let cols = ((rect.size.x - 16.0).max(8.0) / 8.0).floor() as usize;
        let rows = (rect.size.y.max(20.0) / 20.0).floor() as usize;
        if self.terminal_grid.size() == (cols, rows) {
            return;
        }
        self.terminal_grid.resize(cols, rows);
        if self.app_state.terminal.follow_tail {
            self.app_state.terminal.scroll_offset =
                TerminalPanel::tail_offset(self.terminal_grid.line_count(), rect.size.y);
        }
        if let Some(id) = self.app_state.terminal.active_id {
            self.apply_terminal_command(AppCommand::ResizeTerminal {
                id,
                cols: u16::try_from(cols).unwrap_or(u16::MAX),
                rows: u16::try_from(rows).unwrap_or(u16::MAX),
            });
        }
    }
}
