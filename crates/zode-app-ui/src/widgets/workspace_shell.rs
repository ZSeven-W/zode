use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::{ShellPage, ZodeAppState};

use super::{
    Composer, EmptyState, ProjectSidebar, SettingsPanel, TerminalGrid, TerminalPanel,
    TerminalSelection, ThreadHeader, ThreadTranscript, WindowChrome,
};
use crate::{Insets, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme, COMPOSER_ID};

/// Paints the complete platform-neutral workbench shell in stable z-order.
pub struct WorkspaceShell;

impl WorkspaceShell {
    /// Paint entry point for the immutable snapshot shared with hit testing and a11y.
    pub fn paint_snapshot(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let input = TextInputState::with_text(state.composer.draft.clone());
        Self::paint_snapshot_content(painter, snapshot, state, &input, None, None, theme)
    }

    pub fn paint(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot(painter, &snapshot, state, theme)
    }

    pub fn paint_with_composer_input(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_content(painter, &snapshot, state, composer_input, None, None, theme)
    }

    pub fn paint_with_terminal(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let input = TextInputState::with_text(state.composer.draft.clone());
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_content(
            painter,
            &snapshot,
            state,
            &input,
            Some(terminal_grid),
            terminal_selection,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_with_composer_and_terminal_input(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let snapshot = WorkspaceSnapshot::build(state, viewport.size.x, viewport.size.y, insets);
        Self::paint_snapshot_with_composer_and_terminal_input(
            painter,
            &snapshot,
            state,
            composer_input,
            terminal_grid,
            terminal_selection,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_snapshot_with_composer_and_terminal_input(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: &TerminalGrid,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        Self::paint_snapshot_content(
            painter,
            snapshot,
            state,
            composer_input,
            Some(terminal_grid),
            terminal_selection,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_snapshot_content(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: Option<&TerminalGrid>,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let geometry = snapshot.layout;
        painter.begin_frame();
        WindowChrome::paint(painter, geometry.viewport, &geometry, theme);
        if state.shell.page == ShellPage::Settings {
            let workspace = SettingsPanel::active_workspace_uri(state);
            SettingsPanel::paint_page(painter, snapshot, state, workspace, theme);
        } else {
            ProjectSidebar::paint(painter, geometry.sidebar, state, theme);
            ThreadHeader::paint(painter, geometry.top_bar, state, theme);
        }
        if state.shell.page == ShellPage::Terminal {
            let fallback = TerminalGrid::new(1, 1);
            let grid = terminal_grid.unwrap_or(&fallback);
            let terminal_rect = Rect::xywh(
                geometry.transcript.origin.x,
                geometry.transcript.origin.y,
                geometry.transcript.size.x,
                geometry.composer.origin.y + geometry.composer.size.y
                    - geometry.transcript.origin.y,
            );
            TerminalPanel::paint(
                painter,
                terminal_rect,
                grid,
                &state.terminal,
                terminal_selection,
                theme,
            );
        } else if state.shell.page != ShellPage::Settings {
            if state.current_session.is_none() {
                EmptyState::paint(painter, geometry.transcript, theme);
            } else {
                ThreadTranscript::paint(painter, geometry.transcript, state, theme);
            }
            let composer_rect = snapshot
                .node(COMPOSER_ID)
                .map_or(geometry.composer, |node| node.rect);
            Composer::paint_input(
                painter,
                composer_rect,
                composer_input,
                &state.composer,
                theme,
            );
        }
        painter.end_frame();
        geometry
    }
}
