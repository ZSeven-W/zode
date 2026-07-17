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
    composer_queue_reserved_height, Insets, RectExt, WorkspaceLayout, WorkspaceLayoutOptions,
    COMPACT_SIDEBAR_W, COMPOSER_ATTACHMENT_H, COMPOSER_BOTTOM, COMPOSER_CONTEXT_H, COMPOSER_H,
    COMPOSER_INPUT_H, COMPOSER_QUEUE_INSET_X, COMPOSER_QUEUE_MAX_VISIBLE, COMPOSER_QUEUE_OVERLAP,
    COMPOSER_QUEUE_PAD_Y, COMPOSER_QUEUE_ROW_H, CONTENT_GUTTER, CONTENT_W, SIDEBAR_W, TOP_BAR_H,
    TRANSCRIPT_COMPOSER_GAP, TRANSCRIPT_TOP_GAP,
};
pub use text::{paint_role_single_line, TypographyRole, TypographyStyle};
pub use theme::{animation_duration_ms, resolve_theme, ThemeMode, ZodeTheme, ZODE_PURPLE};
pub use virtual_list::{visible_range, MeasuredItem, MeasurementCache, VirtualListState};
pub use widgets::{
    group_sessions, ApprovalAction, ApprovalButtonLayout, ApprovalCard, ArchivedTaskGroupLayout,
    ArchivedTaskRowLayout, ArchivedTasksLayout, CatalogSectionLayout, CellPosition, ComingSoonPage,
    Composer, ComposerContextLayout, ComposerController, ComposerLayout, ComposerOutcome,
    ComposerQueueLayout, ComposerQueueMenuLayout, ComposerQueueRowLayout, ComposerSubmission,
    DocumentPreview, DocumentPreviewLayout, EmptyState, EmptySuggestionLayout, EnvironmentPanel,
    EnvironmentPanelLayout, EnvironmentSectionLayout, GeneralSettingsLayout, HeaderActionLayout,
    InstalledIconLayout, IntegrationRowLayout, IntegrationScopeLayout, IntegrationTabLayout,
    IntegrationsPage, IntegrationsPageLayout, PanelMenuItemLayout, PanelPicker,
    PanelPickerMenuLayout, PermissionPresetLayout, PermissionRow, PermissionRowLayout,
    ProjectChoice, ProjectPicker, ProjectPickerController, ProjectPickerLayout,
    ProjectPickerRowLayout, ProjectPickerTarget, ProjectPickerViewState, ProjectSearchOutcome,
    ProjectSessionGroup, ProjectSidebar, ReviewDraft, ReviewFileRowLayout, ReviewLine,
    ReviewLineKind, ReviewPanel, ReviewPanelLayout, ReviewSelection, SandboxSelection,
    SessionRenameController, SessionRenameOutcome, SettingControl, SettingControlLayout,
    SettingRowLayout, SettingsNavigationEntryLayout, SettingsNavigationGroupLayout,
    SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout, SidebarAction,
    SidebarControlLayout, SidebarControlTarget, SidebarItem, SidebarLabelLayout, SidebarLayout,
    SidebarNavigationRowLayout, SidebarRowLayout, SidebarRowTarget, SidebarSection,
    SidebarSectionLayout, TerminalCell, TerminalColor, TerminalGrid, TerminalLine, TerminalPanel,
    TerminalPanelController, TerminalSecondaryLayout, TerminalSecondaryPanel, TerminalSelection,
    ThreadCopyMenuLayout, ThreadHeader, ThreadHeaderLayout, ThreadMenuActionLayout,
    ThreadMenuLayout, ThreadRenameLayout, ThreadTranscript, ToolCard, ToolTone,
    TranscriptItemLayout, UnavailableSecondaryPanel, UsageChip, UsageDisplay, WelcomeTitleLayout,
    WindowChrome, WorkspaceShell, ARCHIVED_TASK_FILTER_ID, ARCHIVED_TASK_SEARCH_ID,
    DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID, DOCUMENT_PREVIEW_EXTERNAL_ID,
    DOCUMENT_PREVIEW_RETRY_ID, EMPTY_SUGGESTION_IDS, ENVIRONMENT_CLOSE_ID, ENVIRONMENT_REVIEW_ID,
    HEADER_COPY_DETAILS_ID, HEADER_COPY_MENU_ID, HEADER_COPY_SESSION_ID, HEADER_COPY_TITLE_ID,
    HEADER_MENU_ARCHIVE_ID, HEADER_MENU_CONTINUE_ID, HEADER_MENU_COPY_ID, HEADER_MENU_ID,
    HEADER_MENU_NEW_WINDOW_ID, HEADER_MENU_PIN_ID, HEADER_MENU_RENAME_ID, HEADER_MENU_SCHEDULE_ID,
    HEADER_MENU_SIDE_TASK_ID, HEADER_MORE_ID, HEADER_RENAME_CANCEL_ID, HEADER_RENAME_DIALOG_ID,
    HEADER_RENAME_INPUT_ID, HEADER_RENAME_SAVE_ID, INTEGRATIONS_PERSONAL_SCOPE_ID,
    INTEGRATIONS_PLUGINS_TAB_ID, INTEGRATIONS_PUBLIC_SCOPE_ID, INTEGRATIONS_SEARCH_ID,
    INTEGRATIONS_SKILLS_TAB_ID, PANEL_PICKER_ID, PANEL_PICKER_MENU_ID, PROJECT_DETACH_ID,
    PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID, PROJECT_PICKER_SEARCH_ID,
    PROJECT_PICKER_SURFACE_ID, PROJECT_PICKER_TRIGGER_ID, SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
    SIDEBAR_SHOW_ALL_PROJECTS_ID, SIDEBAR_TASKS_MORE_ID, SIDEBAR_TASKS_NEW_ID,
    SIDEBAR_TASKS_TOGGLE_ID, TERMINAL_SECONDARY_CLOSE_ID, UNAVAILABLE_SECONDARY_CLOSE_ID,
};

pub const CRATE_READY: bool = true;
pub(crate) use accessibility::stable_widget_id;
pub use accessibility::{
    accessibility_tree, FocusDirection, InteractionNode, WidgetId, WorkspaceSnapshot,
    BROWSER_NAV_ID, COMPOSER_ID, HEADER_ENVIRONMENT_ID, HEADER_REVIEW_ID, HELP_ID,
    HIGH_CONTRAST_ID, NEW_SESSION_ID, OPENPENCIL_NAV_ID, PLUGINS_NAV_ID, REDUCED_MOTION_ID,
    REVIEW_CLOSE_ID, SEND_ID, SETTINGS_NAV_ID, SETTINGS_ROOT_ID, SIDEBAR_ID, TERMINAL_ID,
    THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID, WORKFLOWS_NAV_ID,
};
pub(crate) use brand::BrandMark;
pub(crate) use text::paint_single_line;
