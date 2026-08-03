use std::collections::BTreeSet;

use zode_node_protocol::{InstalledPluginSummary, PluginTrustReview, PluginUpdateAvailable};

use crate::LoadState;

/// Live state for the "添加插件" inline form on the Integrations page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginAddState {
    pub open: bool,
    pub spec: String,
    pub reference: String,
    pub status: PluginAddStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PluginAddStatus {
    #[default]
    Idle,
    Installing,
    Failed(String),
}

/// The plugin detail overlay opened by clicking an installed plugin row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDetailState {
    pub plugin_id: String,
    pub mode: PluginDetailMode,
    /// A plain client-side notice line above the overlay's footer buttons -
    /// the slot every one-off outcome (uninstall failure, trust-grant
    /// failure, a finished update) writes to.
    pub notice: Option<String>,
    pub update: PluginUpdateState,
}

impl PluginDetailState {
    pub fn overview(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            mode: PluginDetailMode::Overview,
            notice: None,
            update: PluginUpdateState::Idle,
        }
    }
}

/// The update check/apply state machine driving the overlay's "检查更新" and
/// "更新" buttons.
///
/// ```text
///   Idle --CheckPluginUpdate--> Checking
///   Checking --query ok, remote unchanged--> UpToDate
///   Checking --query ok, remote ahead-----> Available
///   Checking --query failed---------------> CheckFailed
///   UpToDate | Available | CheckFailed --CheckPluginUpdate--> Checking
///   Available --ApplyPluginUpdate--> Applying
///   Applying --ok-----> UpToDate    (+ notice)
///   Applying --failed-> Available   (+ notice, checkout rolled back)
/// ```
///
/// Closing or reopening the overlay rebuilds `PluginDetailState`, so the
/// machine always restarts at `Idle` for a freshly opened plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PluginUpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(PluginUpdateAvailable),
    /// Carries the update being applied so the overlay keeps showing what is
    /// about to land while the git fetch/reset runs.
    Applying(PluginUpdateAvailable),
    CheckFailed(String),
}

impl PluginUpdateState {
    /// True while a git operation is in flight - the overlay disables both
    /// buttons so a second click cannot race the first.
    pub fn busy(&self) -> bool {
        matches!(self, Self::Checking | Self::Applying(_))
    }

    /// The update an "更新" press would apply, if one is pending.
    pub fn pending(&self) -> Option<&PluginUpdateAvailable> {
        match self {
            Self::Available(available) | Self::Applying(available) => Some(available),
            _ => None,
        }
    }
}

/// The overlay's internal screen. Kept as one overlay with several modes
/// (rather than a stack of separate overlays) so the uninstall confirmation
/// and the trust-review gate share the same widget-id surface and paint path
/// as the capability list they interrupt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PluginDetailMode {
    #[default]
    Overview,
    ConfirmUninstall,
    Uninstalling,
    TrustReview {
        review: LoadState<PluginTrustReview>,
        /// Keys the user has checked for "逐项信任" - a directly pressed
        /// "全部信任" grants every pending item regardless of this set.
        selected: BTreeSet<String>,
    },
}

/// Looks up one installed plugin summary by id from the loaded catalog.
pub fn installed_plugin<'a>(
    installed: &'a [InstalledPluginSummary],
    plugin_id: &str,
) -> Option<&'a InstalledPluginSummary> {
    installed.iter().find(|plugin| plugin.id == plugin_id)
}
