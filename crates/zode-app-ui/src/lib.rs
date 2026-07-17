#![forbid(unsafe_code)]

mod layout;
mod theme;
mod widgets;

pub use layout::{
    Insets, RectExt, WorkspaceLayout, COMPACT_SIDEBAR_W, COMPOSER_BOTTOM, COMPOSER_H,
    CONTENT_GUTTER, CONTENT_W, SIDEBAR_W, TOP_BAR_H, TRANSCRIPT_COMPOSER_GAP, TRANSCRIPT_TOP_GAP,
};
pub use theme::{ZodeTheme, ZODE_PURPLE};
pub use widgets::{
    group_sessions, Composer, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarItem,
    ThreadHeader, ThreadTranscript, WindowChrome, WorkspaceShell,
};

pub const CRATE_READY: bool = true;
