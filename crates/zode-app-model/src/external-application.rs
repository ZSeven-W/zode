use crate::LoadState;

/// Desktop applications that can open a local Zode workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalApplication {
    VisualStudioCode,
    Cursor,
    Zed,
    Finder,
    Terminal,
    ITerm2,
    Warp,
    Xcode,
    AndroidStudio,
}

impl ExternalApplication {
    pub const ALL: [Self; 9] = [
        Self::VisualStudioCode,
        Self::Cursor,
        Self::Zed,
        Self::Finder,
        Self::Terminal,
        Self::ITerm2,
        Self::Warp,
        Self::Xcode,
        Self::AndroidStudio,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::VisualStudioCode => "VS Code",
            Self::Cursor => "Cursor",
            Self::Zed => "Zed",
            Self::Finder => "Finder",
            Self::Terminal => "Terminal",
            Self::ITerm2 => "iTerm2",
            Self::Warp => "Warp",
            Self::Xcode => "Xcode",
            Self::AndroidStudio => "Android Studio",
        }
    }

    pub const fn bundle_names(self) -> &'static [&'static str] {
        match self {
            Self::VisualStudioCode => &["Visual Studio Code", "Visual Studio Code - Insiders"],
            Self::Cursor => &["Cursor"],
            Self::Zed => &["Zed", "Zed Preview"],
            Self::Finder => &["Finder"],
            Self::Terminal => &["Terminal"],
            Self::ITerm2 => &["iTerm", "iTerm2"],
            Self::Warp => &["Warp"],
            Self::Xcode => &["Xcode", "Xcode-beta"],
            Self::AndroidStudio => &["Android Studio", "Android Studio Preview"],
        }
    }
}

/// Transient state for the thread-header "Open with" split button.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenWithState {
    pub menu_open: bool,
    pub applications: LoadState<Vec<ExternalApplication>>,
    /// Last explicitly selected application. Finder remains the cold-start fallback.
    pub preferred: Option<ExternalApplication>,
}

impl OpenWithState {
    pub fn primary_application(&self) -> ExternalApplication {
        let installed = self.applications.ready();
        self.preferred
            .filter(|preferred| installed.is_none_or(|apps| apps.contains(preferred)))
            .or_else(|| {
                installed
                    .filter(|apps| apps.contains(&ExternalApplication::Finder))
                    .map(|_| ExternalApplication::Finder)
            })
            .or_else(|| installed.and_then(|apps| apps.first().copied()))
            .unwrap_or(ExternalApplication::Finder)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExternalApplication, OpenWithState};
    use crate::LoadState;

    #[test]
    fn finder_is_the_cold_start_and_ready_catalog_fallback() {
        let mut state = OpenWithState::default();
        assert_eq!(state.primary_application(), ExternalApplication::Finder);

        state.applications = LoadState::Ready(vec![
            ExternalApplication::VisualStudioCode,
            ExternalApplication::Finder,
        ]);
        assert_eq!(state.primary_application(), ExternalApplication::Finder);
    }

    #[test]
    fn a_preferred_application_must_still_be_installed() {
        let mut state = OpenWithState {
            applications: LoadState::Ready(vec![
                ExternalApplication::Finder,
                ExternalApplication::Zed,
            ]),
            preferred: Some(ExternalApplication::Zed),
            ..OpenWithState::default()
        };
        assert_eq!(state.primary_application(), ExternalApplication::Zed);

        state.applications = LoadState::Ready(vec![ExternalApplication::Finder]);
        assert_eq!(state.primary_application(), ExternalApplication::Finder);
    }
}
