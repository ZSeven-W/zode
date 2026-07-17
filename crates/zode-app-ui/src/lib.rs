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
    composer_queue_reserved_height, constrained_primary_sidebar_width,
    constrained_secondary_sidebar_width, Insets, PinnedSummaryMode, PrimarySidebarLayoutOptions,
    RectExt, SecondarySidebarLayoutOptions, SidebarLayoutOptions, WorkspaceLayout,
    WorkspaceLayoutOptions, COMPACT_SIDEBAR_W, COMPOSER_ATTACHMENT_H, COMPOSER_BOTTOM,
    COMPOSER_CONTEXT_H, COMPOSER_CONTEXT_INSET_X, COMPOSER_CONTEXT_OVERLAP,
    COMPOSER_CONTEXT_VISIBLE_H, COMPOSER_H, COMPOSER_INPUT_H, COMPOSER_QUEUE_INSET_X,
    COMPOSER_QUEUE_MAX_VISIBLE, COMPOSER_QUEUE_OVERLAP, COMPOSER_QUEUE_PAD_Y, COMPOSER_QUEUE_ROW_H,
    CONTENT_GUTTER, CONTENT_W, PRIMARY_SIDEBAR_DEFAULT_W, PRIMARY_SIDEBAR_MAX_VIEWPORT_RATIO,
    PRIMARY_SIDEBAR_MIN_MAIN_W, PRIMARY_SIDEBAR_MIN_W, SECONDARY_PANE_BREAKPOINT,
    SECONDARY_SIDEBAR_MAX_VIEWPORT_RATIO, SECONDARY_SIDEBAR_MAX_W, SECONDARY_SIDEBAR_MIN_PRIMARY_W,
    SECONDARY_SIDEBAR_MIN_W, SIDEBAR_W, TOP_BAR_H, TRANSCRIPT_COMPOSER_GAP, TRANSCRIPT_TOP_GAP,
};
pub use text::{paint_role_single_line, TypographyRole, TypographyStyle};
pub use theme::{animation_duration_ms, resolve_theme, ThemeMode, ZodeTheme, ZODE_PURPLE};
pub use virtual_list::{visible_range, MeasuredItem, MeasurementCache, VirtualListState};
pub use widgets::{
    current_local_workspace, group_sessions, ApprovalAction, ApprovalButtonLayout, ApprovalCard,
    ArchivedTaskGroupLayout, ArchivedTaskRowLayout, ArchivedTasksLayout, CatalogSectionLayout,
    CellPosition, ComingSoonPage, Composer, ComposerContextChipLayout, ComposerContextLayout,
    ComposerContextMenu, ComposerContextMenuLayout, ComposerContextMenuRowLayout,
    ComposerContextMenuRowTarget, ComposerContextMenuStatusLayout, ComposerController,
    ComposerFooterLayout, ComposerFooterMenuLayout, ComposerFooterMenuWidget,
    ComposerFooterRowLayout, ComposerFooterSectionLayout, ComposerLayout, ComposerOutcome,
    ComposerQueueLayout, ComposerQueueMenuLayout, ComposerQueueRowLayout, ComposerSubmission,
    DocumentPreview, DocumentPreviewLayout, EmptyState, EmptySuggestionLayout,
    EnvironmentActionLayout, EnvironmentPanel, EnvironmentPanelLayout, EnvironmentSectionLayout,
    GeneralSettingsLayout, HeaderActionLayout, InstalledIconLayout, IntegrationRowLayout,
    IntegrationScopeLayout, IntegrationTabLayout, IntegrationsPage, IntegrationsPageLayout,
    OpenWithMenu, OpenWithMenuItemLayout, OpenWithMenuLayout, OpenWithSplitLayout, PanelPicker,
    PanelPickerHomeItemLayout, PanelPickerHomeLayout, PermissionPresetLayout, PermissionRow,
    PermissionRowLayout, ProjectChoice, ProjectPicker, ProjectPickerController,
    ProjectPickerLayout, ProjectPickerRowLayout, ProjectPickerTarget, ProjectPickerViewState,
    ProjectSearchOutcome, ProjectSessionGroup, ProjectSidebar, ReviewDraft, ReviewFileRowLayout,
    ReviewLine, ReviewLineKind, ReviewPanel, ReviewPanelLayout, ReviewSelection, SandboxSelection,
    SessionRenameController, SessionRenameOutcome, SettingControl, SettingControlLayout,
    SettingRowLayout, SettingsNavigationEntryLayout, SettingsNavigationGroupLayout,
    SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout, SidebarAction,
    SidebarControlLayout, SidebarControlTarget, SidebarItem, SidebarLabelLayout, SidebarLayout,
    SidebarMenuItemLayout, SidebarMenuKind, SidebarMenuLayout, SidebarNavigationRowLayout,
    SidebarRowLayout, SidebarRowTarget, SidebarSection, SidebarSectionLayout, TerminalCell,
    TerminalColor, TerminalGrid, TerminalLine, TerminalPanel, TerminalPanelController,
    TerminalSecondaryLayout, TerminalSecondaryPanel, TerminalSelection, ThreadCopyMenuLayout,
    ThreadHeader, ThreadHeaderLayout, ThreadMenuActionLayout, ThreadMenuLayout, ThreadRenameLayout,
    ThreadTranscript, ToolCard, ToolTone, TranscriptItemLayout, UnavailableSecondaryPanel,
    UsageChip, UsageDisplay, WelcomeTitleLayout, WindowChrome, WorkspaceShell,
    ARCHIVED_TASK_FILTER_ID, ARCHIVED_TASK_SEARCH_ID, COMPOSER_ADD_FILE_ID, COMPOSER_ADD_GOAL_ID,
    COMPOSER_ADD_ID, COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID, COMPOSER_BRANCH_CREATE_ID,
    COMPOSER_BRANCH_ID, COMPOSER_BRANCH_SEARCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID,
    COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_LOCATION_ID, COMPOSER_LOCATION_LOCAL_ID,
    COMPOSER_LOCATION_WORKTREE_ID, COMPOSER_MODEL_BACK_ID, COMPOSER_MODEL_CONFIGURE_ID,
    COMPOSER_MODEL_EFFORTS_ID, COMPOSER_MODEL_EFFORT_HIGH_ID, COMPOSER_MODEL_EFFORT_LOW_ID,
    COMPOSER_MODEL_EFFORT_MEDIUM_ID, COMPOSER_MODEL_EFFORT_XHIGH_ID, COMPOSER_MODEL_ID,
    COMPOSER_MODEL_MODELS_ID, COMPOSER_MODEL_RESET_ID, COMPOSER_MODEL_SPEEDS_ID,
    COMPOSER_MODEL_SPEED_ID, COMPOSER_PERMISSION_AUTO_ID, COMPOSER_PERMISSION_CUSTOM_ID,
    COMPOSER_PERMISSION_FULL_ID, COMPOSER_PERMISSION_ID, COMPOSER_PERMISSION_REQUEST_ID,
    COMPOSER_PROJECT_ID, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID, EMPTY_SUGGESTION_IDS,
    ENVIRONMENT_CLOSE_ID, ENVIRONMENT_COMMIT_PUSH_ID, ENVIRONMENT_OPEN_WORKSPACE_ID,
    ENVIRONMENT_PANEL_ID, ENVIRONMENT_REFRESH_ID, ENVIRONMENT_REVIEW_ID, HEADER_COPY_DETAILS_ID,
    HEADER_COPY_MENU_ID, HEADER_COPY_SESSION_ID, HEADER_COPY_TITLE_ID, HEADER_MENU_ARCHIVE_ID,
    HEADER_MENU_CONTINUE_ID, HEADER_MENU_COPY_ID, HEADER_MENU_ID, HEADER_MENU_NEW_WINDOW_ID,
    HEADER_MENU_PIN_ID, HEADER_MENU_RENAME_ID, HEADER_MENU_SCHEDULE_ID, HEADER_MENU_SIDE_TASK_ID,
    HEADER_MORE_ID, HEADER_RENAME_CANCEL_ID, HEADER_RENAME_DIALOG_ID, HEADER_RENAME_INPUT_ID,
    HEADER_RENAME_SAVE_ID, INTEGRATIONS_PERSONAL_SCOPE_ID, INTEGRATIONS_PLUGINS_TAB_ID,
    INTEGRATIONS_PUBLIC_SCOPE_ID, INTEGRATIONS_SEARCH_ID, INTEGRATIONS_SKILLS_TAB_ID,
    OPEN_WITH_DROPDOWN_ID, OPEN_WITH_MENU_ID, OPEN_WITH_PRIMARY_ID, PANEL_PICKER_ID,
    PROJECT_DETACH_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_SURFACE_ID, PROJECT_PICKER_TRIGGER_ID,
    SECONDARY_HOME_BROWSER_ID, SECONDARY_HOME_FILES_ID, SECONDARY_HOME_REVIEW_ID,
    SECONDARY_HOME_SIDE_TASK_ID, SECONDARY_HOME_TERMINAL_ID, SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
    SIDEBAR_PROJECTS_MENU_FLAT_ID, SIDEBAR_PROJECTS_MENU_GROUPED_ID,
    SIDEBAR_PROJECTS_MENU_MANUAL_ID, SIDEBAR_PROJECTS_MENU_PRIORITY_ID,
    SIDEBAR_PROJECTS_MENU_RECENT_ID, SIDEBAR_PROJECTS_MORE_ID, SIDEBAR_PROJECTS_NEW_ID,
    SIDEBAR_PROJECTS_SECTION_ID, SIDEBAR_PROJECT_MENU_ARCHIVE_ID, SIDEBAR_PROJECT_MENU_FINDER_ID,
    SIDEBAR_PROJECT_MENU_PIN_ID, SIDEBAR_PROJECT_MENU_TOGGLE_ID, SIDEBAR_SHOW_ALL_PROJECTS_ID,
    SIDEBAR_TASKS_MENU_NEW_ID, SIDEBAR_TASKS_MENU_TOGGLE_ID, SIDEBAR_TASKS_MORE_ID,
    SIDEBAR_TASKS_NEW_ID, SIDEBAR_TASKS_SECTION_ID, SIDEBAR_TASKS_TOGGLE_ID,
    TERMINAL_SECONDARY_CLOSE_ID, UNAVAILABLE_SECONDARY_CLOSE_ID,
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
