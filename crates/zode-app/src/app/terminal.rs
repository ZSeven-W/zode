use jian_widgets::Rect;
use zode_app_model::{
    reduce_terminal_command, AppCommand, SecondaryPane, TerminalCommandOutcome, TerminalState,
    ZodeAppState,
};
use zode_app_ui::{
    KeyEvent, TerminalGrid, TerminalPanel, TerminalPanelController, TerminalSecondaryPanel,
    WorkspaceLayout, TERMINAL_ID,
};
use zode_node_protocol::NodeCapability;

use super::DesktopApp;
use crate::event_map::terminal_shortcut_command;

impl DesktopApp {
    pub(super) fn apply_terminal_command(&mut self, command: AppCommand) {
        let scrolls_snapshot = matches!(&command, AppCommand::SetTerminalScroll { .. });
        let mut invalidates_snapshot = matches!(&command, AppCommand::CloseTerminal(_));
        let mut redraw_immediately = !matches!(
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
                    if record_terminal_effect_failure(
                        &mut self.app_state.terminal,
                        result.err().map(|error| error.to_string()),
                    ) {
                        invalidates_snapshot = true;
                        redraw_immediately = true;
                    }
                }
                TerminalCommandOutcome::Applied => {}
                TerminalCommandOutcome::Ignored => return,
            }
        }
        if scrolls_snapshot {
            self.invalidate_frame_snapshot_for_scroll();
        } else if invalidates_snapshot {
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
        terminal_rect_for_layout(&self.app_state, self.frame_snapshot.layout)
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
        let Some((cols, rows)) = resize_terminal_grid_to_rect(
            &mut self.terminal_grid,
            &mut self.app_state.terminal,
            rect,
        ) else {
            return;
        };
        if let Some(id) = self.app_state.terminal.active_id {
            self.apply_terminal_command(AppCommand::ResizeTerminal {
                id,
                cols: u16::try_from(cols).unwrap_or(u16::MAX),
                rows: u16::try_from(rows).unwrap_or(u16::MAX),
            });
        }
    }
}

fn terminal_rect_for_layout(state: &ZodeAppState, geometry: WorkspaceLayout) -> Rect {
    if state.presentation.secondary_pane == Some(SecondaryPane::Terminal) {
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

fn resize_terminal_grid_to_rect(
    grid: &mut TerminalGrid,
    terminal: &mut TerminalState,
    rect: Rect,
) -> Option<(usize, usize)> {
    let cols = ((rect.size.x - 16.0).max(8.0) / 8.0).floor() as usize;
    let rows = (rect.size.y.max(20.0) / 20.0).floor() as usize;
    if grid.size() == (cols, rows) {
        return None;
    }
    grid.resize(cols, rows);
    if terminal.follow_tail {
        terminal.scroll_offset = TerminalPanel::tail_offset(grid.line_count(), rect.size.y);
    }
    Some((cols, rows))
}

fn record_terminal_effect_failure(terminal: &mut TerminalState, error: Option<String>) -> bool {
    let Some(error) = error else {
        return false;
    };
    terminal.unavailable_reason = Some(error);
    true
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
    use super::{
        record_terminal_effect_failure, resize_terminal_grid_to_rect, terminal_rect_for_layout,
        terminal_workspace,
    };
    use zode_app_model::{demo_state, ProjectState, SecondaryPane, TranscriptState};
    use zode_app_ui::{Insets, TerminalGrid, TerminalPanel, WorkspaceSnapshot, TERMINAL_ID};
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

    #[test]
    fn terminal_resize_updates_paint_state_without_changing_the_interaction_snapshot() {
        let mut state = demo_state();
        state.presentation.secondary_pane = Some(SecondaryPane::Terminal);
        state.presentation.secondary_sidebar_open = true;
        state.terminal.follow_tail = true;
        let before = WorkspaceSnapshot::build(&state, 1_000.0, 700.0, Insets::ZERO);
        let rect = terminal_rect_for_layout(&state, before.layout);
        assert_eq!(
            rect,
            jian_widgets::Rect::xywh(
                before.layout.transcript.origin.x,
                before.layout.transcript.origin.y,
                before.layout.transcript.size.x,
                before.layout.composer.origin.y + before.layout.composer.size.y
                    - before.layout.transcript.origin.y,
            )
        );

        let mut grid = TerminalGrid::new(8, 2);
        for _ in 0..80 {
            grid.feed(b"line\r\n");
        }
        let resized = resize_terminal_grid_to_rect(&mut grid, &mut state.terminal, rect)
            .expect("resized viewport changes terminal dimensions");
        let expected = (
            ((rect.size.x - 16.0).max(8.0) / 8.0).floor() as usize,
            (rect.size.y.max(20.0) / 20.0).floor() as usize,
        );
        assert_eq!(resized, expected);
        assert_eq!(grid.size(), expected);
        assert_eq!(
            state.terminal.scroll_offset,
            TerminalPanel::tail_offset(grid.line_count(), rect.size.y)
        );
        assert!(state.terminal.scroll_offset > 0.0);

        let after = WorkspaceSnapshot::build(&state, 1_000.0, 700.0, Insets::ZERO);
        assert_eq!(after, before);
        assert_eq!(
            resize_terminal_grid_to_rect(&mut grid, &mut state.terminal, rect),
            None
        );
    }

    #[test]
    fn terminal_effect_failure_changes_the_accessible_terminal_snapshot() {
        let mut state = demo_state();
        state.presentation.secondary_pane = Some(SecondaryPane::Terminal);
        state.presentation.secondary_sidebar_open = true;
        let before = WorkspaceSnapshot::build(&state, 1_000.0, 700.0, Insets::ZERO);
        assert_eq!(
            before
                .node(TERMINAL_ID)
                .and_then(|node| node.value.as_deref()),
            None
        );

        assert!(record_terminal_effect_failure(
            &mut state.terminal,
            Some("resize failed".into()),
        ));
        let after = WorkspaceSnapshot::build(&state, 1_000.0, 700.0, Insets::ZERO);
        assert_eq!(
            after
                .node(TERMINAL_ID)
                .and_then(|node| node.value.as_deref()),
            Some("resize failed")
        );
        assert_ne!(after, before);
        assert!(!record_terminal_effect_failure(&mut state.terminal, None));
    }
}
