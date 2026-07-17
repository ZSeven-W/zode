#![forbid(unsafe_code)]

mod accessibility;
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
    Insets, RectExt, WorkspaceLayout, WorkspaceLayoutOptions, COMPACT_SIDEBAR_W, COMPOSER_BOTTOM,
    COMPOSER_H, CONTENT_GUTTER, CONTENT_W, SIDEBAR_W, TOP_BAR_H, TRANSCRIPT_COMPOSER_GAP,
    TRANSCRIPT_TOP_GAP,
};
pub use theme::{animation_duration_ms, resolve_theme, ThemeMode, ZodeTheme, ZODE_PURPLE};
pub use virtual_list::{visible_range, MeasuredItem, MeasurementCache, VirtualListState};
pub use widgets::{
    group_sessions, ApprovalAction, ApprovalCard, CellPosition, Composer, ComposerController,
    ComposerOutcome, ComposerSubmission, PermissionRow, ProjectSessionGroup, ProjectSidebar,
    ReviewDraft, ReviewLine, ReviewLineKind, ReviewPanel, ReviewSelection, SandboxSelection,
    SettingControl, SettingsPanel, SidebarAction, SidebarItem, TerminalCell, TerminalColor,
    TerminalGrid, TerminalLine, TerminalPanel, TerminalPanelController, TerminalSelection,
    ThreadHeader, ThreadTranscript, ToolCard, ToolTone, UsageChip, UsageDisplay, WindowChrome,
    WorkspaceShell,
};

pub const CRATE_READY: bool = true;
pub use accessibility::{
    accessibility_tree, FocusDirection, InteractionNode, WidgetId, WorkspaceSnapshot,
};
