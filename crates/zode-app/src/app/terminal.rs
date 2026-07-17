use jian_widgets::Rect;
use zode_app_model::{reduce_terminal_command, AppCommand, SecondaryPane, TerminalCommandOutcome};
use zode_app_ui::{
    KeyEvent, TerminalPanel, TerminalPanelController, TerminalSecondaryPanel, TERMINAL_ID,
};
use zode_node_protocol::NodeCapability;

use super::DesktopApp;
use crate::event_map::terminal_shortcut_command;

impl DesktopApp {
    pub(super) fn apply_terminal_command(&mut self, command: AppCommand) {
        let invalidates_snapshot = matches!(
            &command,
            AppCommand::SetTerminalScroll { .. } | AppCommand::CloseTerminal(_)
        );
        let redraw_immediately = !matches!(
            &command,
            AppCommand::WriteTerminal { .. } | AppCommand::ResizeTerminal { .. }
        );
        if command == AppCommand::OpenTerminal {
            let _ = reduce_terminal_command(&mut self.app_state, command);
            self.ensure_terminal_runtime();
            self.rebuild_frame_snapshot();
            self.resize_terminal_grid();
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
                                self.terminal_workspace = None;
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
        if invalidates_snapshot {
            self.invalidate_frame_snapshot();
        }
        if redraw_immediately {
            self.request_redraw();
        }
    }

    pub(super) fn ensure_terminal_runtime(&mut self) {
        if !self
            .app_state
            .host
            .capabilities
            .capabilities
            .contains(&NodeCapability::Terminal)
        {
            self.app_state.terminal.unavailable_reason =
                Some("Terminal is unavailable on this node.".into());
            return;
        }
        let target = self.terminal_target();
        let target_workspace = target.as_ref().map(|(workspace, _)| workspace);
        if self.terminal_runtime.active_id().is_some()
            && self.terminal_workspace.as_ref() != target_workspace
        {
            if let Some(id) = self.terminal_runtime.active_id() {
                let _ = self.terminal_runtime.close(id);
            }
            self.app_state.terminal.active_id = None;
            self.terminal_workspace = None;
        }
        if self.terminal_runtime.active_id().is_some() {
            self.app_state.terminal.unavailable_reason = None;
            return;
        }
        let (cols, rows) = self.terminal_grid.size();
        match target {
            Some((workspace, cwd)) => {
                self.app_state.terminal.unavailable_reason = None;
                match self.terminal_runtime.open(
                    &cwd,
                    u16::try_from(cols).unwrap_or(u16::MAX),
                    u16::try_from(rows).unwrap_or(u16::MAX),
                ) {
                    Ok(id) => {
                        self.app_state.terminal.active_id = Some(id);
                        self.terminal_workspace = Some(workspace);
                    }
                    Err(error) => {
                        self.app_state.terminal.unavailable_reason = Some(error.to_string())
                    }
                }
            }
            None => {
                self.app_state.terminal.unavailable_reason =
                    Some("当前任务没有可用工作目录。".into());
            }
        }
    }

    fn terminal_target(&self) -> Option<(zode_node_protocol::WorkspaceUri, std::path::PathBuf)> {
        let workspace = terminal_workspace(&self.app_state)?;
        let cwd = crate::services::workspace_root(workspace).ok()?;
        if cwd.is_dir() {
            return Some((workspace.clone(), cwd));
        }
        if !self.app_state.is_projectless_workspace(workspace) {
            return None;
        }
        let root = self.app_state.projectless_workspace_root.as_ref()?;
        let cwd = crate::services::workspace_root(root).ok()?;
        cwd.is_dir().then(|| (root.clone(), cwd))
    }

    pub(super) fn drain_terminal_output(&mut self) -> bool {
        let mut changed = false;
        for output in self.terminal_runtime.drain_output() {
            match output {
                Ok(bytes) => {
                    self.terminal_grid.feed(&bytes);
                    changed = true;
                }
                Err(error) => {
                    self.app_state.terminal.unavailable_reason = Some(error.to_string());
                    changed = true;
                }
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
                self.terminal_workspace = None;
                self.app_state.terminal.open = false;
                self.app_state.terminal.focused = false;
                changed = true;
            }
            Ok(_) => {}
            Err(error) => {
                self.app_state.terminal.unavailable_reason = Some(error.to_string());
                changed = true;
            }
        }
        changed
    }

    pub(super) fn terminal_rect(&self) -> Rect {
        let geometry = self.frame_snapshot.layout;
        if self.app_state.presentation.secondary_pane == Some(SecondaryPane::Terminal) {
            let panel = if geometry.review_panel.size.x > 0.0 {
                geometry.review_panel
            } else {
                geometry.primary_surface
            };
            return TerminalSecondaryPanel::layout(panel).content;
        }
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
        if !self.app_state.terminal_surface_visible() || !self.app_state.terminal.focused {
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
        self.invalidate_frame_snapshot();
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

fn terminal_workspace(
    state: &zode_app_model::ZodeAppState,
) -> Option<&zode_node_protocol::WorkspaceUri> {
    state
        .current_session
        .as_ref()
        .and_then(|session| state.available_workspace_for_session(session))
        .or_else(|| state.active_available_workspace())
        .or(state.projectless_workspace_root.as_ref())
}

#[cfg(test)]
mod tests {
    use super::terminal_workspace;
    use zode_app_model::{demo_state, ProjectState, TranscriptState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    #[test]
    fn terminal_workspace_never_falls_back_to_the_process_directory() {
        let mut state = demo_state();
        assert_eq!(terminal_workspace(&state), None);

        let root = WorkspaceUri::new("file:///tmp/zode-task-workspaces").unwrap();
        state.projectless_workspace_root = Some(root.clone());
        assert_eq!(terminal_workspace(&state), Some(&root));

        let child = WorkspaceUri::new("file:///tmp/zode-task-workspaces/task-1").unwrap();
        let session = SessionLocator::new(state.host.node_id, "task-1");
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri: child.clone(),
            title: "task".into(),
            updated_at_ms: 1,
            status: ThreadStatus::Idle,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.current_session = Some(session);
        assert_eq!(terminal_workspace(&state), Some(&child));

        let project = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.current_session = None;
        state.projects.push(ProjectState {
            workspace_uri: project.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 1,
        });
        state.active_workspace = Some(project.clone());
        assert_eq!(terminal_workspace(&state), Some(&project));
    }
}
