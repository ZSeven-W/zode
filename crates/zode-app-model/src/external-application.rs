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

/// PNG icon data resolved from an installed desktop application's bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalApplicationIcon {
    pub application: ExternalApplication,
    encoded_png: Vec<u8>,
    image_id: u64,
}

impl ExternalApplicationIcon {
    pub fn new(application: ExternalApplication, encoded_png: Vec<u8>) -> Self {
        let hash = fnv1a64(FNV1A_OFFSET_BASIS, b"zode-external-application-icon-v1");
        let hash = fnv1a64(hash, application.icon_cache_salt());
        let image_id = fnv1a64(hash, &encoded_png);
        Self {
            application,
            encoded_png,
            image_id,
        }
    }

    /// Stable cache key for the native image, including both its owning
    /// application and the current PNG contents.
    pub fn image_id(&self) -> u64 {
        self.image_id
    }

    pub fn encoded_png(&self) -> &[u8] {
        &self.encoded_png
    }
}

const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a64(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }
    hash
}

/// Installed applications and the native icons that could be resolved for them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalApplicationCatalog {
    pub applications: Vec<ExternalApplication>,
    pub icons: Vec<ExternalApplicationIcon>,
}

impl From<Vec<ExternalApplication>> for ExternalApplicationCatalog {
    fn from(applications: Vec<ExternalApplication>) -> Self {
        Self {
            applications,
            icons: Vec::new(),
        }
    }
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

    const fn icon_cache_salt(self) -> &'static [u8] {
        match self {
            Self::VisualStudioCode => b"visual-studio-code",
            Self::Cursor => b"cursor",
            Self::Zed => b"zed",
            Self::Finder => b"finder",
            Self::Terminal => b"terminal",
            Self::ITerm2 => b"iterm2",
            Self::Warp => b"warp",
            Self::Xcode => b"xcode",
            Self::AndroidStudio => b"android-studio",
        }
    }
}

/// Transient state for the thread-header "Open with" split button.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenWithState {
    pub menu_open: bool,
    pub applications: LoadState<Vec<ExternalApplication>>,
    pub icons: Vec<ExternalApplicationIcon>,
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

    pub fn icon(&self, application: ExternalApplication) -> Option<&ExternalApplicationIcon> {
        self.icons
            .iter()
            .find(|icon| icon.application == application)
    }

    pub fn icon_png(&self, application: ExternalApplication) -> Option<&[u8]> {
        self.icon(application)
            .map(ExternalApplicationIcon::encoded_png)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExternalApplication, ExternalApplicationIcon, OpenWithState};
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

    #[test]
    fn native_icons_are_looked_up_by_application() {
        let state = OpenWithState {
            icons: vec![ExternalApplicationIcon::new(
                ExternalApplication::Zed,
                vec![1, 2, 3],
            )],
            ..OpenWithState::default()
        };

        assert_eq!(
            state.icon_png(ExternalApplication::Zed),
            Some(&[1, 2, 3][..])
        );
        assert_eq!(state.icon_png(ExternalApplication::Finder), None);
    }

    #[test]
    fn native_icon_image_ids_are_stable_and_content_addressed() {
        let icon = ExternalApplicationIcon::new;
        let first = icon(ExternalApplication::Zed, vec![1, 2, 3, 4]);
        let identical = icon(ExternalApplication::Zed, vec![1, 2, 3, 4]);
        let changed_bytes = icon(ExternalApplication::Zed, vec![1, 2, 3, 5]);
        let changed_application = icon(ExternalApplication::Finder, vec![1, 2, 3, 4]);

        assert_eq!(first.image_id(), identical.image_id());
        assert_ne!(first.image_id(), changed_bytes.image_id());
        assert_ne!(first.image_id(), changed_application.image_id());
    }
}
