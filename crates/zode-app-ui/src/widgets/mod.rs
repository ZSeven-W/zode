mod composer;
mod project_sidebar;
mod thread_header;
mod transcript;
mod window_chrome;

use jian_widgets::{Painter, Rect};
use zode_app_model::ZodeAppState;

use crate::{Insets, WorkspaceLayout, ZodeTheme};

pub use composer::{
    Composer, ComposerController, ComposerOutcome, ComposerSubmission, SandboxSelection,
};
pub use project_sidebar::{
    group_sessions, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarItem,
};
pub use thread_header::ThreadHeader;
pub use transcript::ThreadTranscript;
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
        let geometry = WorkspaceLayout::compute(viewport.size.x, viewport.size.y, insets);
        painter.begin_frame();
        WindowChrome::paint(painter, viewport, &geometry, theme);
        ProjectSidebar::paint(painter, geometry.sidebar, state, theme);
        ThreadHeader::paint(painter, geometry.top_bar, state, theme);
        ThreadTranscript::paint(painter, geometry.transcript, state, theme);
        Composer::paint(painter, geometry.composer, &state.composer, theme);
        painter.end_frame();
        geometry
    }
}
