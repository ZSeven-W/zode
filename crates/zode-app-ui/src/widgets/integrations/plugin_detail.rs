//! Plugin detail overlay: opened by clicking an installed-plugin row
//! ([`super::plugin_rows`]). One overlay, three modes sharing the same panel
//! and widget-id surface - `Overview` (capability list + uninstall/update),
//! `ConfirmUninstall`, and `TrustReview` (verbatim command/script text +
//! per-item or all-at-once grant) - rather than a stack of separate dialogs,
//! per the design doc's "hard gate before enabling" requirement.

use jian_widgets::Rect;
use zode_app_model::{LoadState, PluginDetailMode, PluginUpdateState, ZodeAppState};
use zode_node_protocol::{PluginCapabilityKind, PluginCapabilitySummary, PluginTrustState};

use crate::{stable_widget_id, WidgetId};

pub const PLUGIN_DETAIL_CLOSE_ID: WidgetId = WidgetId(305);
pub const PLUGIN_DETAIL_UNINSTALL_ID: WidgetId = WidgetId(306);
pub const PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID: WidgetId = WidgetId(307);
pub const PLUGIN_DETAIL_UNINSTALL_CANCEL_ID: WidgetId = WidgetId(308);
pub const PLUGIN_DETAIL_CHECK_UPDATE_ID: WidgetId = WidgetId(309);
pub const PLUGIN_DETAIL_TRUST_ALL_ID: WidgetId = WidgetId(310);
pub const PLUGIN_DETAIL_TRUST_CANCEL_ID: WidgetId = WidgetId(311);
pub const PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID: WidgetId = WidgetId(312);
pub const PLUGIN_DETAIL_APPLY_UPDATE_ID: WidgetId = WidgetId(313);

const PANEL_W: f32 = 520.0;
pub(super) const PAD: f32 = 20.0;
pub(super) const HEADER_H: f32 = 64.0;
const ROW_H: f32 = 30.0;
const BUTTON_H: f32 = 32.0;
const FOOTER_H: f32 = 64.0;
/// Height one stacked text line above the footer buttons occupies.
pub(super) const NOTICE_LINE_H: f32 = 22.0;
/// Mirrors `plugin_rows::MAX_VISIBLE_PLUGIN_ROWS`'s "show a few, don't build
/// a second scroll region" convention.
const MAX_VISIBLE_ROWS: usize = 8;

pub fn capability_toggle_widget_id(plugin_id: &str, source_id: &str) -> WidgetId {
    stable_widget_id(0x81, &(plugin_id, source_id))
}

pub fn trust_item_checkbox_widget_id(plugin_id: &str, key: &str) -> WidgetId {
    stable_widget_id(0x82, &(plugin_id, key))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityRowLayout {
    pub rect: Rect,
    pub capability: PluginCapabilitySummary,
    /// `Some` only for a skill/MCP capability whose trust key is not
    /// pending review - clicking toggles `SetIntegrationEnabled` for its
    /// `toggle_source_id`.
    pub toggle_action: Option<CapabilityToggleAction>,
    pub gated: bool,
    pub status_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapabilityToggleAction {
    pub widget_id: WidgetId,
    pub currently_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrustItemRowLayout {
    pub checkbox_id: WidgetId,
    pub key: String,
    pub content: String,
    pub selected: bool,
    pub rect: Rect,
}

/// The overview footer's update controls, derived from
/// [`PluginUpdateState`]. `apply` only exists once a check reported a
/// pending update - there is nothing to apply before that.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateControls {
    pub check: Rect,
    pub check_label: String,
    pub check_disabled: bool,
    pub apply: Option<Rect>,
    pub apply_label: String,
    pub apply_disabled: bool,
    pub status: Option<UpdateStatusLine>,
}

/// One line above the footer reporting the last check/apply outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateStatusLine {
    pub text: String,
    /// Painted in the destructive color - a failed check, never a summary.
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginDetailBody {
    Overview {
        capabilities: Vec<CapabilityRowLayout>,
        update: UpdateControls,
        uninstall: Rect,
        notice: Option<String>,
    },
    ConfirmUninstall {
        confirm: Rect,
        cancel: Rect,
    },
    Uninstalling,
    TrustReview {
        items: Vec<TrustItemRowLayout>,
        trust_all: Rect,
        grant_selected: Rect,
        grant_selected_enabled: bool,
        cancel: Rect,
        loading: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginDetailOverlayLayout {
    pub panel: Rect,
    pub plugin_id: String,
    pub repo: String,
    pub reference: String,
    pub close: Rect,
    pub body: PluginDetailBody,
}

impl PluginDetailOverlayLayout {
    pub fn new(content: Rect, state: &ZodeAppState) -> Option<Self> {
        let detail = state.presentation.plugin_detail.as_ref()?;
        let plugin = state
            .presentation
            .installed_plugins
            .ready()
            .and_then(|plugins| plugins.iter().find(|plugin| plugin.id == detail.plugin_id))?;
        let body_rows = match &detail.mode {
            PluginDetailMode::Overview => plugin.capabilities.len().min(MAX_VISIBLE_ROWS),
            PluginDetailMode::TrustReview { review, .. } => review
                .ready()
                .map(|review| review.items.len().min(MAX_VISIBLE_ROWS))
                .unwrap_or(1),
            PluginDetailMode::ConfirmUninstall | PluginDetailMode::Uninstalling => 0,
        };
        let update_status = update_status_line(&detail.update);
        let extra = match detail.mode {
            PluginDetailMode::ConfirmUninstall => 64.0,
            PluginDetailMode::Uninstalling => 40.0,
            // The status line and the notice stack above the footer buttons;
            // each one present grows the panel so neither overlaps them.
            PluginDetailMode::Overview => {
                (usize::from(update_status.is_some()) + usize::from(detail.notice.is_some())) as f32
                    * NOTICE_LINE_H
            }
            _ => 0.0,
        };
        let panel_h = HEADER_H + (body_rows as f32) * ROW_H + FOOTER_H + extra + PAD;
        let panel = Rect::xywh(
            content.origin.x + ((content.size.x - PANEL_W).max(0.0) / 2.0),
            content.origin.y + 16.0,
            PANEL_W.min(content.size.x.max(0.0)),
            panel_h.min((content.size.y - 32.0).max(160.0)),
        );
        let close = Rect::xywh(
            panel.origin.x + panel.size.x - 40.0,
            panel.origin.y + 12.0,
            28.0,
            28.0,
        );
        let body_top = panel.origin.y + HEADER_H;

        let body = match &detail.mode {
            PluginDetailMode::Overview => {
                let capabilities = plugin
                    .capabilities
                    .iter()
                    .take(MAX_VISIBLE_ROWS)
                    .enumerate()
                    .map(|(index, capability)| {
                        capability_row(
                            &detail.plugin_id,
                            capability,
                            &plugin.trust,
                            state,
                            panel,
                            body_top,
                            index,
                        )
                    })
                    .collect();
                let footer_y = panel.origin.y + panel.size.y - FOOTER_H + 12.0;
                PluginDetailBody::Overview {
                    capabilities,
                    update: update_controls(
                        &detail.update,
                        update_status,
                        panel.origin.x + PAD,
                        footer_y,
                    ),
                    uninstall: Rect::xywh(
                        panel.origin.x + panel.size.x - PAD - 88.0,
                        footer_y,
                        88.0,
                        BUTTON_H,
                    ),
                    notice: detail.notice.clone(),
                }
            }
            PluginDetailMode::ConfirmUninstall => {
                let button_y = panel.origin.y + panel.size.y - FOOTER_H + 12.0;
                PluginDetailBody::ConfirmUninstall {
                    confirm: Rect::xywh(
                        panel.origin.x + panel.size.x - PAD - 96.0,
                        button_y,
                        96.0,
                        BUTTON_H,
                    ),
                    cancel: Rect::xywh(
                        panel.origin.x + panel.size.x - PAD - 96.0 - 12.0 - 72.0,
                        button_y,
                        72.0,
                        BUTTON_H,
                    ),
                }
            }
            PluginDetailMode::Uninstalling => PluginDetailBody::Uninstalling,
            PluginDetailMode::TrustReview { review, selected } => {
                let (items, loading, error) = match review {
                    LoadState::Ready(payload) => {
                        let items = payload
                            .items
                            .iter()
                            .take(MAX_VISIBLE_ROWS)
                            .enumerate()
                            .map(|(index, item)| TrustItemRowLayout {
                                checkbox_id: trust_item_checkbox_widget_id(
                                    &detail.plugin_id,
                                    &item.key,
                                ),
                                key: item.key.clone(),
                                content: item.content.clone(),
                                selected: selected.contains(&item.key),
                                rect: Rect::xywh(
                                    panel.origin.x + PAD,
                                    body_top + index as f32 * ROW_H,
                                    panel.size.x - PAD * 2.0,
                                    ROW_H,
                                ),
                            })
                            .collect();
                        (items, false, None)
                    }
                    LoadState::Failed(message) => (Vec::new(), false, Some(message.clone())),
                    LoadState::Idle | LoadState::Loading => (Vec::new(), true, None),
                };
                let footer_y = panel.origin.y + panel.size.y - FOOTER_H + 12.0;
                let grant_selected_enabled = !selected.is_empty();
                PluginDetailBody::TrustReview {
                    items,
                    trust_all: Rect::xywh(
                        panel.origin.x + panel.size.x - PAD - 88.0,
                        footer_y,
                        88.0,
                        BUTTON_H,
                    ),
                    grant_selected: Rect::xywh(
                        panel.origin.x + panel.size.x - PAD - 88.0 - 12.0 - 96.0,
                        footer_y,
                        96.0,
                        BUTTON_H,
                    ),
                    cancel: Rect::xywh(panel.origin.x + PAD, footer_y, 72.0, BUTTON_H),
                    loading,
                    error,
                    grant_selected_enabled,
                }
            }
        };

        Some(Self {
            panel,
            plugin_id: detail.plugin_id.clone(),
            repo: plugin.repo.clone(),
            reference: plugin.reference.clone(),
            close,
            body,
        })
    }
}

const CHECK_BUTTON_W: f32 = 96.0;
const APPLY_BUTTON_W: f32 = 76.0;
const BUTTON_GAP: f32 = 8.0;

/// Maps the update state machine onto the footer's two buttons and its
/// status line. Both buttons are disabled while git is running so a second
/// press cannot race the in-flight operation.
fn update_controls(
    update: &PluginUpdateState,
    status: Option<UpdateStatusLine>,
    left_x: f32,
    footer_y: f32,
) -> UpdateControls {
    let busy = update.busy();
    let applying = matches!(update, PluginUpdateState::Applying(_));
    UpdateControls {
        check: Rect::xywh(left_x, footer_y, CHECK_BUTTON_W, BUTTON_H),
        check_label: if matches!(update, PluginUpdateState::Checking) {
            "检查中…".to_owned()
        } else {
            "检查更新".to_owned()
        },
        check_disabled: busy,
        apply: update.pending().map(|_| {
            Rect::xywh(
                left_x + CHECK_BUTTON_W + BUTTON_GAP,
                footer_y,
                APPLY_BUTTON_W,
                BUTTON_H,
            )
        }),
        apply_label: if applying {
            "更新中…".to_owned()
        } else {
            "更新".to_owned()
        },
        apply_disabled: busy,
        status,
    }
}

fn update_status_line(update: &PluginUpdateState) -> Option<UpdateStatusLine> {
    let (text, error) = match update {
        PluginUpdateState::Idle => return None,
        PluginUpdateState::Checking => ("正在检查更新…".to_owned(), false),
        PluginUpdateState::UpToDate => ("已是最新版本".to_owned(), false),
        PluginUpdateState::Available(available) => {
            (format!("发现更新：{}", available.summary), false)
        }
        PluginUpdateState::Applying(available) => {
            (format!("正在更新到 {}…", available.summary), false)
        }
        PluginUpdateState::CheckFailed(reason) => (format!("检查更新失败：{reason}"), true),
    };
    Some(UpdateStatusLine { text, error })
}

#[allow(clippy::too_many_arguments)]
fn capability_row(
    plugin_id: &str,
    capability: &PluginCapabilitySummary,
    trust: &PluginTrustState,
    state: &ZodeAppState,
    panel: Rect,
    body_top: f32,
    index: usize,
) -> CapabilityRowLayout {
    let rect = Rect::xywh(
        panel.origin.x + PAD,
        body_top + index as f32 * ROW_H,
        panel.size.x - PAD * 2.0,
        ROW_H,
    );
    let gated = matches!(
        trust,
        PluginTrustState::NeedsReview(keys) | PluginTrustState::Drifted(keys) if keys.contains(&capability.key)
    );
    // A button is rendered for every skill/MCP row, gated or not - a gated
    // row's button opens the trust-review screen instead of toggling
    // `SetIntegrationEnabled` directly (see `command_for_widget`'s dispatch
    // for `capability_toggle_widget_id`, which re-derives this same `gated`
    // check to route the click). Hooks get no button at all: they have no
    // `toggle_source_id` because the engine does not yet load hooks from an
    // installed plugin's tree (see `zode-app-runtime::plugin_market`'s doc
    // comment on `Capability::Hook`).
    let toggle_action = capability.toggle_source_id.as_ref().map(|source_id| {
        let currently_enabled = catalog_enabled(state, source_id).unwrap_or(false);
        CapabilityToggleAction {
            widget_id: capability_toggle_widget_id(plugin_id, source_id),
            currently_enabled,
        }
    });
    let status_label = if capability.kind == PluginCapabilityKind::Hook {
        "只读展示".to_owned()
    } else if gated {
        "待审查".to_owned()
    } else if toggle_action.is_some_and(|action| action.currently_enabled) {
        "已启用".to_owned()
    } else {
        "已停用".to_owned()
    };
    CapabilityRowLayout {
        rect,
        capability: capability.clone(),
        toggle_action,
        gated,
        status_label,
    }
}

fn catalog_enabled(state: &ZodeAppState, source_id: &str) -> Option<bool> {
    let catalog = state.presentation.integrations.ready()?;
    catalog
        .all_entries()
        .find(|entry| entry.source_id.as_deref() == Some(source_id))
        .map(|entry| entry.availability != zode_app_model::Availability::Disabled)
}
