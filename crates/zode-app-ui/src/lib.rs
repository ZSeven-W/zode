#![forbid(unsafe_code)]

mod input;
mod layout;
mod theme;
mod virtual_list;
mod widgets;

pub use input::{
    ImeEvent, Key, KeyEvent, Modifiers, PointerButton, PointerEvent, TouchEvent, TouchPhase,
    WheelEvent,
};
pub use layout::{
    Insets, RectExt, WorkspaceLayout, COMPACT_SIDEBAR_W, COMPOSER_BOTTOM, COMPOSER_H,
    CONTENT_GUTTER, CONTENT_W, SIDEBAR_W, TOP_BAR_H, TRANSCRIPT_COMPOSER_GAP, TRANSCRIPT_TOP_GAP,
};
pub use theme::{ZodeTheme, ZODE_PURPLE};
pub use virtual_list::{visible_range, MeasuredItem, MeasurementCache, VirtualListState};
pub use widgets::{
    group_sessions, ApprovalAction, ApprovalCard, Composer, ComposerController, ComposerOutcome,
    ComposerSubmission, PermissionRow, ProjectSessionGroup, ProjectSidebar, SandboxSelection,
    SettingsPanel, SidebarAction, SidebarItem, ThreadHeader, ThreadTranscript, ToolCard, ToolTone,
    UsageChip, UsageDisplay, WindowChrome, WorkspaceShell,
};

pub const CRATE_READY: bool = true;
