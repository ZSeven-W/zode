mod approval_card;
mod composer;
mod project_sidebar;
mod review_panel;
mod settings_panel;
mod terminal_controller;
mod terminal_grid;
mod terminal_panel;
mod thread_header;
mod tool_card;
mod transcript;
mod usage_chip;
mod window_chrome;

use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::{ShellPage, ZodeAppState};

use crate::{Insets, WorkspaceLayout, WorkspaceSnapshot, ZodeTheme};

pub use approval_card::{ApprovalAction, ApprovalCard};
pub use composer::{
    Composer, ComposerController, ComposerOutcome, ComposerSubmission, SandboxSelection,
};
pub use project_sidebar::{
    group_sessions, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarItem,
};
pub use review_panel::{ReviewDraft, ReviewLine, ReviewLineKind, ReviewPanel, ReviewSelection};
pub use settings_panel::{PermissionRow, SettingControl, SettingsPanel};
pub use terminal_controller::TerminalPanelController;
pub use terminal_grid::{
    CellPosition, TerminalCell, TerminalColor, TerminalGrid, TerminalLine, TerminalSelection,
};
pub use terminal_panel::TerminalPanel;
pub use thread_header::ThreadHeader;
pub use tool_card::{ToolCard, ToolTone};
pub use transcript::ThreadTranscript;
pub use usage_chip::{UsageChip, UsageDisplay};
pub use window_chrome::WindowChrome;

/// Paints the complete platform-neutral workbench shell in stable z-order.
pub struct WorkspaceShell;

impl WorkspaceShell {
    /// Paint entry point for the immutable snapshot shared with hit testing and a11y.
    pub fn paint_snapshot(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        _state: &ZodeAppState,
        _theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        painter.begin_frame();
        painter.end_frame();
        snapshot.layout
    }

    pub fn paint(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let input = TextInputState::with_text(state.composer.draft.clone());
        Self::paint_with_composer_input(painter, viewport, insets, state, &input, theme)
    }

    pub fn paint_with_composer_input(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        Self::paint_content(
            painter,
            viewport,
            insets,
            state,
            composer_input,
            None,
            None,
            theme,
        )
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
        Self::paint_content(
            painter,
            viewport,
            insets,
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
        Self::paint_content(
            painter,
            viewport,
            insets,
            state,
            composer_input,
            Some(terminal_grid),
            terminal_selection,
            theme,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_content(
        painter: &mut dyn Painter,
        viewport: Rect,
        insets: Insets,
        state: &ZodeAppState,
        composer_input: &TextInputState,
        terminal_grid: Option<&TerminalGrid>,
        terminal_selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) -> WorkspaceLayout {
        let geometry = WorkspaceLayout::compute(viewport.size.x, viewport.size.y, insets);
        painter.begin_frame();
        WindowChrome::paint(painter, viewport, &geometry, theme);
        ProjectSidebar::paint(painter, geometry.sidebar, state, theme);
        ThreadHeader::paint(painter, geometry.top_bar, state, theme);
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
        } else {
            ThreadTranscript::paint(painter, geometry.transcript, state, theme);
            Composer::paint_input(
                painter,
                geometry.composer,
                composer_input,
                &state.composer,
                theme,
            );
        }
        painter.end_frame();
        geometry
    }
}
