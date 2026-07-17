#![forbid(unsafe_code)]

mod accessibility;
mod brand;
mod icons;
mod input;
mod layout;
mod text;
mod theme;
mod virtual_list;
mod widgets;

pub use icons::SemanticIcon;
pub use input::{
    ImeEvent, Key, KeyEvent, Modifiers, PointerButton, PointerEvent, PointerEventKind, TouchEvent,
    TouchPhase, UnifiedInputEvent, WheelDeltaMode, WheelEvent,
};
pub use layout::{
    Insets, RectExt, WorkspaceLayout, WorkspaceLayoutOptions, COMPACT_SIDEBAR_W,
    COMPOSER_ATTACHMENT_H, COMPOSER_BOTTOM, COMPOSER_CONTEXT_H, COMPOSER_H, COMPOSER_INPUT_H,
    CONTENT_GUTTER, CONTENT_W, SIDEBAR_W, TOP_BAR_H, TRANSCRIPT_COMPOSER_GAP, TRANSCRIPT_TOP_GAP,
};
pub use text::{paint_role_single_line, TypographyRole, TypographyStyle};
pub use theme::{animation_duration_ms, resolve_theme, ThemeMode, ZodeTheme, ZODE_PURPLE};
pub use virtual_list::{visible_range, MeasuredItem, MeasurementCache, VirtualListState};
pub use widgets::{
    group_sessions, ApprovalAction, ApprovalButtonLayout, ApprovalCard, CatalogSectionLayout,
    CellPosition, ComingSoonPage, Composer, ComposerController, ComposerLayout, ComposerOutcome,
    ComposerSubmission, DocumentPreview, DocumentPreviewLayout, EnvironmentPanel,
    EnvironmentPanelLayout, EnvironmentSectionLayout, GeneralSettingsLayout, HeaderActionLayout,
    InstalledIconLayout, IntegrationRowLayout, IntegrationTabLayout, IntegrationsPage,
    IntegrationsPageLayout, PermissionPresetLayout, PermissionRow, PermissionRowLayout,
    ProjectSessionGroup, ProjectSidebar, ReviewDraft, ReviewFileRowLayout, ReviewLine,
    ReviewLineKind, ReviewPanel, ReviewPanelLayout, ReviewSelection, SandboxSelection,
    SettingControl, SettingControlLayout, SettingRowLayout, SettingsNavigationEntryLayout,
    SettingsNavigationGroupLayout, SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout,
    SidebarAction, SidebarItem, SidebarNavigationRowLayout, SidebarRowLayout, SidebarRowTarget,
    TerminalCell, TerminalColor, TerminalGrid, TerminalLine, TerminalPanel,
    TerminalPanelController, TerminalSelection, ThreadHeader, ThreadHeaderLayout, ThreadTranscript,
    ToolCard, ToolTone, TranscriptItemLayout, UsageChip, UsageDisplay, WindowChrome,
    WorkspaceShell, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID, ENVIRONMENT_CLOSE_ID,
    ENVIRONMENT_REVIEW_ID, INTEGRATIONS_PLUGINS_TAB_ID, INTEGRATIONS_SKILLS_TAB_ID,
    SETTINGS_BACK_ID,
};

pub const CRATE_READY: bool = true;
pub(crate) use accessibility::stable_widget_id;
pub use accessibility::{
    accessibility_tree, FocusDirection, InteractionNode, WidgetId, WorkspaceSnapshot,
    BROWSER_NAV_ID, COMPOSER_ID, HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID, HIGH_CONTRAST_ID,
    NEW_SESSION_ID, OPENPENCIL_NAV_ID, PLUGINS_NAV_ID, REDUCED_MOTION_ID, REVIEW_CLOSE_ID, SEND_ID,
    SETTINGS_NAV_ID, SETTINGS_ROOT_ID, SIDEBAR_ID, TERMINAL_ID, THEME_DARK_ID, THEME_LIGHT_ID,
    THEME_SYSTEM_ID, WORKFLOWS_NAV_ID,
};
pub(crate) use brand::BrandMark;
pub(crate) use text::paint_single_line;
