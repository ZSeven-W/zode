mod lucide_paths;

use lucide_paths as lucide;

/// Stable semantic icon registry shared by desktop surfaces.
///
/// Every path uses a 24×24 coordinate system. Callers choose geometry and
/// color; the semantic name keeps product code independent from path data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticIcon {
    NewTask,
    Scheduled,
    Settings,
    Search,
    User,
    Appearance,
    Microphone,
    Configuration,
    Sparkles,
    Pet,
    Keyboard,
    Usage,
    Account,
    Snapshot,
    Integrations,
    Browser,
    Computer,
    Hook,
    Connect,
    Git,
    Environment,
    Panel,
    Terminal,
    Worktree,
    Archive,
    Sites,
    PullRequest,
    Chat,
    ExploreCode,
    BuildFeature,
    ReviewChange,
    FixIssue,
    Folder,
    Send,
    Stop,
    Queue,
    Guide,
    Delete,
    More,
    Edit,
    CloseQueue,
    Branch,
    Host,
    Diff,
    Compare,
    Back,
    FileText,
    Close,
    Check,
    ChevronRight,
    ChevronDown,
    Refresh,
    ExternalOpen,
    Help,
    Pin,
    Plus,
    ShieldAlert,
}

impl SemanticIcon {
    pub const ALL: [Self; 57] = [
        Self::NewTask,
        Self::Scheduled,
        Self::Settings,
        Self::Search,
        Self::User,
        Self::Appearance,
        Self::Microphone,
        Self::Configuration,
        Self::Sparkles,
        Self::Pet,
        Self::Keyboard,
        Self::Usage,
        Self::Account,
        Self::Snapshot,
        Self::Integrations,
        Self::Browser,
        Self::Computer,
        Self::Hook,
        Self::Connect,
        Self::Git,
        Self::Environment,
        Self::Panel,
        Self::Terminal,
        Self::Worktree,
        Self::Archive,
        Self::Sites,
        Self::PullRequest,
        Self::Chat,
        Self::ExploreCode,
        Self::BuildFeature,
        Self::ReviewChange,
        Self::FixIssue,
        Self::Folder,
        Self::Send,
        Self::Stop,
        Self::Queue,
        Self::Guide,
        Self::Delete,
        Self::More,
        Self::Edit,
        Self::CloseQueue,
        Self::Branch,
        Self::Host,
        Self::Diff,
        Self::Compare,
        Self::Back,
        Self::FileText,
        Self::Close,
        Self::Check,
        Self::ChevronRight,
        Self::ChevronDown,
        Self::Refresh,
        Self::ExternalOpen,
        Self::Help,
        Self::Pin,
        Self::Plus,
        Self::ShieldAlert,
    ];

    pub const fn path(self) -> &'static str {
        match self {
            Self::NewTask => lucide::SQUARE_PEN,
            Self::Scheduled => lucide::CLOCK,
            Self::Settings => lucide::SETTINGS,
            Self::Search => lucide::SEARCH,
            Self::User => "M12 4A4 4 0 1 0 12 12A4 4 0 1 0 12 4M4 21C4 16 7 14 12 14C17 14 20 16 20 21",
            Self::Appearance => "M12 3V5M12 19V21M3 12H5M19 12H21M5.6 5.6L7 7M17 17L18.4 18.4M18.4 5.6L17 7M7 17L5.6 18.4M12 8A4 4 0 1 0 12 16A4 4 0 1 0 12 8",
            Self::Microphone => "M9 5V12A3 3 0 0 0 15 12V5M6 11A6 6 0 0 0 18 11M12 17V21",
            Self::Configuration => "M4 7H14M18 7H20M4 12H8M12 12H20M4 17H15M19 17H20M14 5V9M8 10V14M15 15V19",
            Self::Sparkles => "M12 3L13.4 8.6L19 10L13.4 11.4L12 17L10.6 11.4L5 10L10.6 8.6ZM19 16L19.7 18.3L22 19L19.7 19.7L19 22L18.3 19.7L16 19L18.3 18.3Z",
            Self::Pet => "M8 11A2 2 0 1 0 8 7A2 2 0 1 0 8 11M16 11A2 2 0 1 0 16 7A2 2 0 1 0 16 11M5 16A2 2 0 1 0 5 12A2 2 0 1 0 5 16M19 16A2 2 0 1 0 19 12A2 2 0 1 0 19 16M8 20C8 16 10 14 12 14C14 14 16 16 16 20C14 22 10 22 8 20",
            Self::Keyboard => "M3 6H21V18H3ZM6 10H7M10 10H11M14 10H15M18 10H19M6 14H8M10 14H16M18 14H19",
            Self::Usage => "M4 20V12M10 20V7M16 20V10M22 20V4",
            Self::Account => "M4 5H20V19H4ZM8 10A2 2 0 1 0 8 6A2 2 0 1 0 8 10M6 16C6 13 7 12 9 12C11 12 12 13 12 16M14 8H18M14 12H18",
            Self::Snapshot => "M4 7H8L10 4H14L16 7H20V20H4ZM12 10A3 3 0 1 0 12 16A3 3 0 1 0 12 10",
            Self::Integrations => lucide::AT_SIGN,
            Self::Browser => "M4 5H20V19H4ZM4 9H20M7 7H7.1M10 7H10.1",
            Self::Computer => "M3 4H21V17H3ZM8 21H16M12 17V21",
            Self::Hook => "M7 4V12A5 5 0 0 0 17 12V10M14 10L17 13L20 10",
            Self::Connect => "M8 12H16M5 9L2 12L5 15M19 9L22 12L19 15M8 5H16M8 19H16",
            Self::Git => "M7 3A2 2 0 1 0 7 7A2 2 0 1 0 7 3M17 17A2 2 0 1 0 17 21A2 2 0 1 0 17 17M7 7V13C7 16 9 19 13 19H15M17 5V17M14 8L17 5L20 8",
            Self::Environment => "M4 5H20V19H4ZM7 9L10 12L7 15M12 15H17",
            Self::Panel => "M3 4H21V20H3ZM15 4V20M18 8H18.1M18 12H18.1M18 16H18.1",
            Self::Terminal => "M4 5H20V19H4ZM7 9L10 12L7 15M12 15H17",
            Self::Worktree => "M6 4V18M18 6V20M6 9H12C15 9 18 7 18 4M6 15H12C15 15 18 17 18 20",
            Self::Archive => lucide::ARCHIVE,
            Self::Sites => lucide::BLOCKS,
            Self::PullRequest => lucide::GIT_PULL_REQUEST,
            Self::Chat => lucide::MESSAGE_CIRCLE_PLUS,
            Self::ExploreCode => "M12 3L14 9L20 11L14 13L12 19L10 13L4 11L10 9Z",
            Self::BuildFeature => "M4 18L11 11M9 5L19 15M13 3L21 11L17 15L9 7Z",
            Self::ReviewChange => "M4 12L9 17L20 6",
            Self::FixIssue => {
                "M8 8H16V18H8ZM9 5L12 8L15 5M4 10H8M16 10H20M4 15H8M16 15H20"
            }
            Self::Folder => lucide::FOLDER,
            Self::Send => "M12 19V5M6 11L12 5L18 11",
            Self::Stop => "M7 7H17V17H7Z",
            Self::Queue => "M4 5V10C4 12.2 5.8 14 8 14H20M16 10L20 14L16 18",
            Self::Guide => "M9 6L4 11L9 16M4 11H14C18 11 20 13 20 17",
            Self::Delete => "M5 7H19M9 7V4H15V7M7 7L8 20H16L17 7M10 11V16M14 11V16",
            Self::More => lucide::ELLIPSIS,
            Self::Edit => lucide::PENCIL,
            Self::CloseQueue => "M4 5V10C4 12.2 5.8 14 8 14H20M16 10L20 14L16 18",
            Self::Branch => "M6 3A2 2 0 1 0 6 7A2 2 0 1 0 6 3M18 17A2 2 0 1 0 18 21A2 2 0 1 0 18 17M6 7V13C6 16 8 19 12 19H16M18 5V17M15 8L18 5L21 8",
            Self::Host => "M3 5H21V17H3ZM8 21H16M12 17V21",
            Self::Diff => "M5 7H11M8 4V10M14 7H20M5 17H11M14 17H20M17 14V20",
            Self::Compare => "M7 7H19M16 4L19 7L16 10M17 17H5M8 14L5 17L8 20",
            Self::Back => "M15 18L9 12L15 6",
            Self::FileText => "M6 3H14L19 8V21H6ZM14 3V8H19M9 13H16M9 17H16",
            Self::Close => lucide::X,
            Self::Check => lucide::CHECK,
            Self::ChevronRight => lucide::CHEVRON_RIGHT,
            Self::ChevronDown => lucide::CHEVRON_DOWN,
            Self::Refresh => lucide::REFRESH_CW,
            Self::ExternalOpen => lucide::EXTERNAL_LINK,
            Self::Help => lucide::CIRCLE_QUESTION_MARK,
            Self::Pin => lucide::PIN,
            Self::Plus => lucide::PLUS,
            Self::ShieldAlert => lucide::SHIELD_ALERT,
        }
    }

    pub const fn viewbox(self) -> f32 {
        let _ = self;
        24.0
    }

    pub const fn stroke_width(self) -> f32 {
        let _ = self;
        1.25
    }
}

#[cfg(test)]
mod tests {
    use super::{lucide, SemanticIcon};

    #[test]
    fn all_icons_share_the_regular_semantic_24px_registry() {
        for icon in SemanticIcon::ALL {
            assert!(!icon.path().is_empty());
            assert_eq!(icon.viewbox(), 24.0);
            assert_eq!(icon.stroke_width(), 1.25);
        }
    }

    #[test]
    fn queue_icons_share_the_reference_turn_marker() {
        const TURN_MARKER: &str = "M4 5V10C4 12.2 5.8 14 8 14H20M16 10L20 14L16 18";

        assert_eq!(SemanticIcon::Queue.path(), TURN_MARKER);
        assert_eq!(SemanticIcon::CloseQueue.path(), TURN_MARKER);
    }

    #[test]
    fn sidebar_and_menu_icons_use_the_pinned_lucide_registry() {
        let mappings = [
            (SemanticIcon::NewTask, lucide::SQUARE_PEN),
            (SemanticIcon::Scheduled, lucide::CLOCK),
            (SemanticIcon::Search, lucide::SEARCH),
            (SemanticIcon::Integrations, lucide::AT_SIGN),
            (SemanticIcon::Sites, lucide::BLOCKS),
            (SemanticIcon::PullRequest, lucide::GIT_PULL_REQUEST),
            (SemanticIcon::Chat, lucide::MESSAGE_CIRCLE_PLUS),
            (SemanticIcon::Folder, lucide::FOLDER),
            (SemanticIcon::Pin, lucide::PIN),
            (SemanticIcon::Archive, lucide::ARCHIVE),
            (SemanticIcon::More, lucide::ELLIPSIS),
            (SemanticIcon::Edit, lucide::PENCIL),
            (SemanticIcon::Plus, lucide::PLUS),
            (SemanticIcon::ShieldAlert, lucide::SHIELD_ALERT),
        ];

        for (icon, path) in mappings {
            assert_eq!(icon.path(), path);
        }
    }
}
