mod approval_card;
mod composer;
mod project_sidebar;
mod settings_panel;
mod thread_header;
mod tool_card;
mod transcript;
mod usage_chip;
mod window_chrome;

use jian_core::text_input::TextInputState;
use jian_widgets::{Painter, Rect};
use zode_app_model::ZodeAppState;

use crate::{Insets, WorkspaceLayout, ZodeTheme};

pub use approval_card::{ApprovalAction, ApprovalCard};
pub use composer::{
    Composer, ComposerController, ComposerOutcome, ComposerSubmission, SandboxSelection,
};
pub use project_sidebar::{
    group_sessions, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarItem,
};
pub use settings_panel::{PermissionRow, SettingsPanel};
pub use thread_header::ThreadHeader;
pub use tool_card::{ToolCard, ToolTone};
pub use transcript::ThreadTranscript;
pub use usage_chip::{UsageChip, UsageDisplay};
pub use window_chrome::WindowChrome;

/// Paints the complete platform-neutral workbench shell in stable z-order.
pub struct WorkspaceShell;

impl WorkspaceShell {
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
        let geometry = WorkspaceLayout::compute(viewport.size.x, viewport.size.y, insets);
        painter.begin_frame();
        WindowChrome::paint(painter, viewport, &geometry, theme);
        ProjectSidebar::paint(painter, geometry.sidebar, state, theme);
        ThreadHeader::paint(painter, geometry.top_bar, state, theme);
        ThreadTranscript::paint(painter, geometry.transcript, state, theme);
        Composer::paint_input(
            painter,
            geometry.composer,
            composer_input,
            &state.composer,
            theme,
        );
        painter.end_frame();
        geometry
    }
}
