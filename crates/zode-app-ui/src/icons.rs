/// Stable semantic icon registry shared by desktop surfaces.
///
/// Every path uses a 24×24 coordinate system. Callers choose geometry and
/// color; the semantic name keeps product code independent from path data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticIcon {
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
    Worktree,
    Archive,
}

impl SemanticIcon {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Settings => {
                "M12 8A4 4 0 1 0 12 16A4 4 0 1 0 12 8M19 12L22 12M2 12L5 12M12 2L12 5M12 19L12 22M17 7L19 5M5 19L7 17M17 17L19 19M5 5L7 7"
            }
            Self::Search => "M11 4A7 7 0 1 0 11 18A7 7 0 1 0 11 4M16 16L21 21",
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
            Self::Integrations => "M8 9H16V12A4 4 0 0 1 12 16V21M10 9V4M14 9V4",
            Self::Browser => "M4 5H20V19H4ZM4 9H20M7 7H7.1M10 7H10.1",
            Self::Computer => "M3 4H21V17H3ZM8 21H16M12 17V21",
            Self::Hook => "M7 4V12A5 5 0 0 0 17 12V10M14 10L17 13L20 10",
            Self::Connect => "M8 12H16M5 9L2 12L5 15M19 9L22 12L19 15M8 5H16M8 19H16",
            Self::Git => "M7 3A2 2 0 1 0 7 7A2 2 0 1 0 7 3M17 17A2 2 0 1 0 17 21A2 2 0 1 0 17 17M7 7V13C7 16 9 19 13 19H15M17 5V17M14 8L17 5L20 8",
            Self::Environment => "M4 5H20V19H4ZM7 9L10 12L7 15M12 15H17",
            Self::Worktree => "M6 4V18M18 6V20M6 9H12C15 9 18 7 18 4M6 15H12C15 15 18 17 18 20",
            Self::Archive => "M4 7H20V20H4ZM3 4H21V7H3ZM9 11H15",
        }
    }

    pub const fn viewbox(self) -> f32 {
        let _ = self;
        24.0
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticIcon;

    #[test]
    fn settings_icons_share_the_semantic_24px_registry() {
        for icon in [
            SemanticIcon::Settings,
            SemanticIcon::Search,
            SemanticIcon::Appearance,
            SemanticIcon::Integrations,
            SemanticIcon::Environment,
        ] {
            assert!(!icon.path().is_empty());
            assert_eq!(icon.viewbox(), 24.0);
        }
    }
}
