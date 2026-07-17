mod approval_card;
mod coming_soon_page;
mod composer;
#[path = "composer-context-menu.rs"]
mod composer_context_menu;
mod document_preview;
mod empty_state;
mod environment;
mod integrations;
mod panel_picker;
mod project_picker;
mod project_sidebar;
mod review_panel;
#[path = "session-rename.rs"]
mod session_rename;
mod settings;
mod terminal_controller;
mod terminal_grid;
mod terminal_panel;
mod terminal_secondary;
mod thread_header;
#[path = "thread-header-overlay.rs"]
mod thread_header_overlay;
mod tool_card;
pub(crate) mod transcript;
mod unavailable_secondary;
mod usage_chip;
mod window_chrome;
mod workspace_shell;

pub use approval_card::{ApprovalAction, ApprovalButtonLayout, ApprovalCard};
pub use coming_soon_page::ComingSoonPage;
pub use composer::{
    Composer, ComposerContextChipLayout, ComposerContextLayout, ComposerController,
    ComposerFooterLayout, ComposerFooterMenuLayout, ComposerFooterMenuWidget,
    ComposerFooterRowLayout, ComposerFooterSectionLayout, ComposerLayout, ComposerOutcome,
    ComposerQueueLayout, ComposerQueueMenuLayout, ComposerQueueRowLayout, ComposerSubmission,
    SandboxSelection, COMPOSER_ADD_FILE_ID, COMPOSER_ADD_GOAL_ID, COMPOSER_ADD_ID,
    COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID, COMPOSER_BRANCH_ID,
    COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_LOCATION_ID, COMPOSER_MODEL_BACK_ID,
    COMPOSER_MODEL_CONFIGURE_ID, COMPOSER_MODEL_EFFORTS_ID, COMPOSER_MODEL_EFFORT_HIGH_ID,
    COMPOSER_MODEL_EFFORT_LOW_ID, COMPOSER_MODEL_EFFORT_MEDIUM_ID, COMPOSER_MODEL_EFFORT_XHIGH_ID,
    COMPOSER_MODEL_ID, COMPOSER_MODEL_MODELS_ID, COMPOSER_MODEL_RESET_ID, COMPOSER_MODEL_SPEEDS_ID,
    COMPOSER_MODEL_SPEED_ID, COMPOSER_PERMISSION_AUTO_ID, COMPOSER_PERMISSION_CUSTOM_ID,
    COMPOSER_PERMISSION_FULL_ID, COMPOSER_PERMISSION_ID, COMPOSER_PERMISSION_REQUEST_ID,
    COMPOSER_PROJECT_ID, PROJECT_DETACH_ID,
};
pub use composer_context_menu::{
    ComposerContextMenu, ComposerContextMenuLayout, ComposerContextMenuRowLayout,
    ComposerContextMenuRowTarget, ComposerContextMenuStatusLayout, COMPOSER_BRANCH_CREATE_ID,
    COMPOSER_BRANCH_SEARCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID, COMPOSER_LOCATION_LOCAL_ID,
    COMPOSER_LOCATION_WORKTREE_ID,
};
pub use document_preview::{
    DocumentPreview, DocumentPreviewLayout, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID,
};
pub use empty_state::{EmptyState, EmptySuggestionLayout, EMPTY_SUGGESTION_IDS};
pub use environment::{
    EnvironmentActionLayout, EnvironmentPanel, EnvironmentPanelLayout, EnvironmentSectionLayout,
    ENVIRONMENT_CLOSE_ID, ENVIRONMENT_COMMIT_PUSH_ID, ENVIRONMENT_OPEN_WORKSPACE_ID,
    ENVIRONMENT_REFRESH_ID, ENVIRONMENT_REVIEW_ID,
};
pub use integrations::{
    CatalogSectionLayout, InstalledIconLayout, IntegrationRowLayout, IntegrationScopeLayout,
    IntegrationTabLayout, IntegrationsPage, IntegrationsPageLayout, INTEGRATIONS_PERSONAL_SCOPE_ID,
    INTEGRATIONS_PLUGINS_TAB_ID, INTEGRATIONS_PUBLIC_SCOPE_ID, INTEGRATIONS_SEARCH_ID,
    INTEGRATIONS_SKILLS_TAB_ID,
};
pub use panel_picker::{
    PanelMenuItemLayout, PanelPicker, PanelPickerMenuLayout, PANEL_PICKER_ID, PANEL_PICKER_MENU_ID,
};
pub use project_picker::{
    ProjectChoice, ProjectPicker, ProjectPickerController, ProjectPickerLayout,
    ProjectPickerRowLayout, ProjectPickerTarget, ProjectPickerViewState, ProjectSearchOutcome,
    WelcomeTitleLayout, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_SURFACE_ID, PROJECT_PICKER_TRIGGER_ID,
};
pub use project_sidebar::{
    group_sessions, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarControlLayout,
    SidebarControlTarget, SidebarItem, SidebarLabelLayout, SidebarLayout, SidebarMenuItemLayout,
    SidebarMenuKind, SidebarMenuLayout, SidebarNavigationRowLayout, SidebarRowLayout,
    SidebarRowTarget, SidebarSection, SidebarSectionLayout, SIDEBAR_PROJECTS_MENU_FLAT_ID,
    SIDEBAR_PROJECTS_MENU_GROUPED_ID, SIDEBAR_PROJECTS_MENU_MANUAL_ID,
    SIDEBAR_PROJECTS_MENU_PRIORITY_ID, SIDEBAR_PROJECTS_MENU_RECENT_ID, SIDEBAR_PROJECTS_MORE_ID,
    SIDEBAR_PROJECTS_NEW_ID, SIDEBAR_PROJECTS_SECTION_ID, SIDEBAR_PROJECT_MENU_ARCHIVE_ID,
    SIDEBAR_PROJECT_MENU_FINDER_ID, SIDEBAR_PROJECT_MENU_PIN_ID, SIDEBAR_PROJECT_MENU_TOGGLE_ID,
    SIDEBAR_SHOW_ALL_PROJECTS_ID, SIDEBAR_TASKS_MENU_NEW_ID, SIDEBAR_TASKS_MENU_TOGGLE_ID,
    SIDEBAR_TASKS_MORE_ID, SIDEBAR_TASKS_NEW_ID, SIDEBAR_TASKS_SECTION_ID, SIDEBAR_TASKS_TOGGLE_ID,
};
pub use review_panel::{
    ReviewDraft, ReviewFileRowLayout, ReviewLine, ReviewLineKind, ReviewPanel, ReviewPanelLayout,
    ReviewSelection,
};
pub use session_rename::{SessionRenameController, SessionRenameOutcome};
pub use settings::{
    ArchivedTaskGroupLayout, ArchivedTaskRowLayout, ArchivedTasksLayout, GeneralSettingsLayout,
    PermissionPresetLayout, PermissionRow, PermissionRowLayout, SettingControl,
    SettingControlLayout, SettingRowLayout, SettingsNavigationEntryLayout,
    SettingsNavigationGroupLayout, SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout,
    ARCHIVED_TASK_FILTER_ID, ARCHIVED_TASK_SEARCH_ID, SETTINGS_BACK_ID, SETTINGS_SEARCH_ID,
};
pub use terminal_controller::TerminalPanelController;
pub use terminal_grid::{
    CellPosition, TerminalCell, TerminalColor, TerminalGrid, TerminalLine, TerminalSelection,
};
pub use terminal_panel::TerminalPanel;
pub use terminal_secondary::{
    TerminalSecondaryLayout, TerminalSecondaryPanel, TERMINAL_SECONDARY_CLOSE_ID,
};
pub use thread_header::{
    HeaderActionLayout, ThreadCopyMenuLayout, ThreadHeader, ThreadHeaderLayout,
    ThreadMenuActionLayout, ThreadMenuLayout, ThreadRenameLayout, HEADER_COPY_DETAILS_ID,
    HEADER_COPY_MENU_ID, HEADER_COPY_SESSION_ID, HEADER_COPY_TITLE_ID, HEADER_MENU_ARCHIVE_ID,
    HEADER_MENU_CONTINUE_ID, HEADER_MENU_COPY_ID, HEADER_MENU_ID, HEADER_MENU_NEW_WINDOW_ID,
    HEADER_MENU_PIN_ID, HEADER_MENU_RENAME_ID, HEADER_MENU_SCHEDULE_ID, HEADER_MENU_SIDE_TASK_ID,
    HEADER_MORE_ID, HEADER_RENAME_CANCEL_ID, HEADER_RENAME_DIALOG_ID, HEADER_RENAME_INPUT_ID,
    HEADER_RENAME_SAVE_ID,
};
pub use tool_card::{ToolCard, ToolTone};
pub use transcript::{ThreadTranscript, TranscriptItemLayout};
pub use unavailable_secondary::{UnavailableSecondaryPanel, UNAVAILABLE_SECONDARY_CLOSE_ID};
pub use usage_chip::{UsageChip, UsageDisplay};
pub use window_chrome::WindowChrome;
pub use workspace_shell::WorkspaceShell;
