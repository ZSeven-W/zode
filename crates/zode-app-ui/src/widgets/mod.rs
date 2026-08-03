mod approval_card;
mod browser_panel;
mod collapsed_sidebar_chrome;
mod coming_soon_page;
mod composer;
#[path = "composer-context-menu.rs"]
mod composer_context_menu;
mod document_preview;
mod empty_state;
mod environment;
mod global_search;
mod integrations;
mod lightbox;
#[path = "open-with-menu.rs"]
mod open_with_menu;
mod panel_picker;
mod primary_sidebar_preview;
mod project_picker;
mod project_sidebar;
mod review_panel;
#[path = "session-rename.rs"]
mod session_rename;
mod settings;
pub(crate) mod subagent_avatar;
mod subagents_panel;
mod terminal_controller;
mod terminal_grid;
mod terminal_panel;
mod terminal_secondary;
mod thread_header;
#[path = "thread-header-overlay.rs"]
mod thread_header_overlay;
mod tool_card;
pub(crate) mod transcript;
#[path = "transcript-find-bar.rs"]
mod transcript_find_bar;
mod unavailable_secondary;
mod usage_chip;
mod window_chrome;
mod workspace_shell;

pub use approval_card::{ApprovalAction, ApprovalButtonLayout, ApprovalCard};
pub use browser_panel::{
    BrowserFrameView, BrowserPanel, BrowserPanelLayout, BROWSER_PANEL_CLOSE_ID,
};
pub use collapsed_sidebar_chrome::{
    CollapsedSidebarChrome, CollapsedSidebarChromeLayout, COLLAPSED_SIDEBAR_BACK_ID,
    COLLAPSED_SIDEBAR_CHROME_TRAILING_EDGE, COLLAPSED_SIDEBAR_FORWARD_ID,
};
pub use coming_soon_page::ComingSoonPage;
pub use composer::{
    Composer, ComposerContextChipLayout, ComposerContextLayout, ComposerController,
    ComposerFooterLayout, ComposerFooterMenuLayout, ComposerFooterMenuWidget,
    ComposerFooterRowLayout, ComposerFooterSectionLayout, ComposerLayout, ComposerOutcome,
    ComposerQueueLayout, ComposerQueueMenuLayout, ComposerQueueRowLayout, ComposerSubmission,
    SandboxSelection, COMPOSER_ADD_FILE_ID, COMPOSER_ADD_GOAL_ID, COMPOSER_ADD_ID,
    COMPOSER_ADD_PLAN_ID, COMPOSER_ADD_WECHAT_ID, COMPOSER_BRANCH_ID,
    COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_LOCATION_ID, COMPOSER_MODEL_ADD_ID,
    COMPOSER_MODEL_BACK_ID, COMPOSER_MODEL_CONFIGURE_ID, COMPOSER_MODEL_EFFORTS_ID,
    COMPOSER_MODEL_EFFORT_HIGH_ID, COMPOSER_MODEL_EFFORT_LOW_ID, COMPOSER_MODEL_EFFORT_MEDIUM_ID,
    COMPOSER_MODEL_EFFORT_XHIGH_ID, COMPOSER_MODEL_ID, COMPOSER_MODEL_MODELS_ID,
    COMPOSER_MODEL_RESET_ID, COMPOSER_MODEL_SPEEDS_ID, COMPOSER_MODEL_SPEED_ID,
    COMPOSER_PERMISSION_AUTO_ID, COMPOSER_PERMISSION_CUSTOM_ID, COMPOSER_PERMISSION_FULL_ID,
    COMPOSER_PERMISSION_ID, COMPOSER_PERMISSION_REQUEST_ID, COMPOSER_PROJECT_ID, PROJECT_DETACH_ID,
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
    ENVIRONMENT_PANEL_ID, ENVIRONMENT_REFRESH_ID, ENVIRONMENT_REVIEW_ID,
};
pub use global_search::{
    GlobalSearch, GlobalSearchChoice, GlobalSearchController, GlobalSearchLayout,
    GlobalSearchOutcome, GlobalSearchRowLayout, GlobalSearchTarget, GlobalSearchViewState,
    GLOBAL_SEARCH_INPUT_ID, GLOBAL_SEARCH_NEW_TASK_ID, GLOBAL_SEARCH_OPEN_FOLDER_ID,
    GLOBAL_SEARCH_SCRIM_ID, GLOBAL_SEARCH_SETTINGS_ID, GLOBAL_SEARCH_SURFACE_ID,
};
pub use integrations::{
    CapabilityRowLayout, CatalogSectionLayout, InstalledIconLayout, IntegrationRowLayout,
    IntegrationScopeLayout, IntegrationTabLayout, IntegrationsPage, IntegrationsPageLayout,
    PluginAddFormLayout, PluginDetailBody, PluginDetailOverlayLayout, PluginRowLayout,
    TrustItemRowLayout, UpdateControls, UpdateStatusLine, INTEGRATIONS_ADD_PLUGIN_ID,
    INTEGRATIONS_PERSONAL_SCOPE_ID, INTEGRATIONS_PLUGINS_TAB_ID, INTEGRATIONS_PUBLIC_SCOPE_ID,
    INTEGRATIONS_SEARCH_ID, INTEGRATIONS_SKILLS_TAB_ID, PLUGIN_ADD_CANCEL_ID,
    PLUGIN_ADD_REFERENCE_INPUT_ID, PLUGIN_ADD_SPEC_INPUT_ID, PLUGIN_ADD_SUBMIT_ID,
    PLUGIN_DETAIL_APPLY_UPDATE_ID, PLUGIN_DETAIL_CHECK_UPDATE_ID, PLUGIN_DETAIL_CLOSE_ID,
    PLUGIN_DETAIL_TRUST_ALL_ID, PLUGIN_DETAIL_TRUST_CANCEL_ID,
    PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID, PLUGIN_DETAIL_UNINSTALL_CANCEL_ID,
    PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID, PLUGIN_DETAIL_UNINSTALL_ID,
};
pub use lightbox::{
    Lightbox, LightboxLayout, LIGHTBOX_CLOSE_ID, LIGHTBOX_SCRIM_ID, LIGHTBOX_ZOOM_IN_ID,
    LIGHTBOX_ZOOM_OUT_ID,
};
pub use open_with_menu::{
    current_local_workspace, OpenWithMenu, OpenWithMenuItemLayout, OpenWithMenuLayout,
    OpenWithSplitLayout, OPEN_WITH_DROPDOWN_ID, OPEN_WITH_MENU_ID, OPEN_WITH_PRIMARY_ID,
};
pub use panel_picker::{
    PanelPicker, PanelPickerHomeItemLayout, PanelPickerHomeLayout, PANEL_PICKER_ID,
    SECONDARY_HOME_BROWSER_ID, SECONDARY_HOME_FILES_ID, SECONDARY_HOME_REVIEW_ID,
    SECONDARY_HOME_SIDE_TASK_ID, SECONDARY_HOME_TERMINAL_ID,
};
pub use primary_sidebar_preview::PrimarySidebarPreview;
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
    SIDEBAR_SEARCH_ID, SIDEBAR_SHOW_ALL_PROJECTS_ID, SIDEBAR_TASKS_MENU_NEW_ID,
    SIDEBAR_TASKS_MENU_TOGGLE_ID, SIDEBAR_TASKS_MORE_ID, SIDEBAR_TASKS_NEW_ID,
    SIDEBAR_TASKS_SECTION_ID, SIDEBAR_TASKS_TOGGLE_ID, SIDEBAR_TOGGLE_ID,
};
pub use review_panel::{
    ReviewDraft, ReviewFileRowLayout, ReviewLine, ReviewLineKind, ReviewPanel, ReviewPanelLayout,
    ReviewSelection,
};
pub use session_rename::{SessionRenameController, SessionRenameOutcome};
pub use settings::{
    provider_model_widget_id, provider_remove_widget_id, provider_widget_id, AllowedAppRowLayout,
    ArchivedTaskGroupLayout, ArchivedTaskRowLayout, ArchivedTasksLayout, ComputerUseLayout,
    GeneralSettingsLayout, PermissionPresetLayout, PermissionRow, PermissionRowLayout,
    PermissionStatusRowLayout, ProviderEditorLayout, ProviderFieldLayout, ProviderKindLayout,
    ProviderModelLayout, ProviderModelsLayout, ProviderRowLayout, SettingControl,
    SettingControlLayout, SettingRowLayout, SettingsNavigationEntryLayout,
    SettingsNavigationGroupLayout, SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout,
    ARCHIVED_TASK_FILTER_ID, ARCHIVED_TASK_SEARCH_ID, COMPUTER_ALLOWED_APP_ADD_ID,
    COMPUTER_ALLOWED_APP_INPUT_ID, PROVIDER_ADD_ID, PROVIDER_API_KEY_INPUT_ID,
    PROVIDER_BASE_URL_INPUT_ID, PROVIDER_CANCEL_ID, PROVIDER_DEFAULT_MODEL_INPUT_ID,
    PROVIDER_ID_INPUT_ID, PROVIDER_KIND_ANTHROPIC_ID, PROVIDER_KIND_OLLAMA_ID,
    PROVIDER_KIND_OPENAI_ID, PROVIDER_MODEL_IDS_INPUT_ID, PROVIDER_SAVE_ID, SETTINGS_BACK_ID,
    SETTINGS_SEARCH_ID,
};
pub use subagents_panel::{
    SubagentRowLayout, SubagentsPanel, SubagentsPanelLayout, SUBAGENTS_PANEL_CLOSE_ID,
    SUBAGENTS_PANEL_SHOW_MORE_ID,
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
pub use transcript::{
    corrected_card_height, AnchorRail, AnchorTick, ThreadTranscript, TranscriptImageBytes,
    TranscriptImageSource, TranscriptItemLayout, TRANSCRIPT_BACK_TO_BOTTOM_ID,
};
pub use transcript_find_bar::{
    TranscriptFindBar, TranscriptFindBarLayout, TranscriptFindController, TranscriptFindOutcome,
    FIND_BAR_HEIGHT, TRANSCRIPT_FIND_CLOSE_ID, TRANSCRIPT_FIND_INPUT_ID, TRANSCRIPT_FIND_NEXT_ID,
    TRANSCRIPT_FIND_PREVIOUS_ID, TRANSCRIPT_FIND_SURFACE_ID,
};
pub use unavailable_secondary::{UnavailableSecondaryPanel, UNAVAILABLE_SECONDARY_CLOSE_ID};
pub use usage_chip::{UsageChip, UsageDisplay};
pub use window_chrome::WindowChrome;
pub use workspace_shell::WorkspaceShell;
