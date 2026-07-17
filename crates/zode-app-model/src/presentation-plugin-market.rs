use std::collections::BTreeSet;

use zode_node_protocol::{InstalledPluginSummary, PluginTrustReview};

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
    /// A plain client-side notice - currently only used by the "检查更新"
    /// M1 stub (real update/drift detection is M2, see
    /// `docs/proposals/plugin-marketplace.md`).
    pub notice: Option<String>,
}

impl PluginDetailState {
    pub fn overview(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            mode: PluginDetailMode::Overview,
            notice: None,
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
