mod approval_card;
mod coming_soon_page;
mod composer;
mod document_preview;
mod empty_state;
mod environment;
mod integrations_page;
mod project_sidebar;
mod review_panel;
mod settings;
mod terminal_controller;
mod terminal_grid;
mod terminal_panel;
mod thread_header;
mod tool_card;
pub(crate) mod transcript;
mod usage_chip;
mod window_chrome;
mod workspace_shell;

pub use approval_card::{ApprovalAction, ApprovalButtonLayout, ApprovalCard};
pub use coming_soon_page::ComingSoonPage;
pub use composer::{
    Composer, ComposerController, ComposerLayout, ComposerOutcome, ComposerSubmission,
    SandboxSelection,
};
pub use document_preview::{
    DocumentPreview, DocumentPreviewLayout, DOCUMENT_PREVIEW_CLOSE_ID, DOCUMENT_PREVIEW_CONTENT_ID,
    DOCUMENT_PREVIEW_EXTERNAL_ID, DOCUMENT_PREVIEW_RETRY_ID,
};
pub use empty_state::EmptyState;
pub use environment::{
    EnvironmentPanel, EnvironmentPanelLayout, EnvironmentSectionLayout, ENVIRONMENT_CLOSE_ID,
    ENVIRONMENT_REVIEW_ID,
};
pub use integrations_page::{
    CapabilityCard, CapabilityCardLayout, IntegrationTabLayout, IntegrationsPage,
    IntegrationsPageLayout, INTEGRATIONS_PLUGINS_TAB_ID, INTEGRATIONS_SKILLS_TAB_ID,
};
pub use project_sidebar::{
    group_sessions, ProjectSessionGroup, ProjectSidebar, SidebarAction, SidebarItem,
    SidebarNavigationRowLayout, SidebarRowLayout, SidebarRowTarget,
};
pub use review_panel::{
    ReviewDraft, ReviewFileRowLayout, ReviewLine, ReviewLineKind, ReviewPanel, ReviewPanelLayout,
    ReviewSelection,
};
pub use settings::{
    GeneralSettingsLayout, PermissionPresetLayout, PermissionRow, PermissionRowLayout,
    SettingControl, SettingControlLayout, SettingRowLayout, SettingsNavigationEntryLayout,
    SettingsNavigationGroupLayout, SettingsNavigationLayout, SettingsPanel, SettingsPanelLayout,
};
pub use terminal_controller::TerminalPanelController;
pub use terminal_grid::{
    CellPosition, TerminalCell, TerminalColor, TerminalGrid, TerminalLine, TerminalSelection,
};
pub use terminal_panel::TerminalPanel;
pub use thread_header::{HeaderActionLayout, ThreadHeader, ThreadHeaderLayout};
pub use tool_card::{ToolCard, ToolTone};
pub use transcript::{ThreadTranscript, TranscriptItemLayout};
pub use usage_chip::{UsageChip, UsageDisplay};
pub use window_chrome::WindowChrome;
pub use workspace_shell::WorkspaceShell;
