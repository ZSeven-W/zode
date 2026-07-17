//! Plugin detail overlay: opened by clicking an installed-plugin row
//! ([`super::plugin_rows`]). One overlay, three modes sharing the same panel
//! and widget-id surface - `Overview` (capability list + uninstall/update),
//! `ConfirmUninstall`, and `TrustReview` (verbatim command/script text +
//! per-item or all-at-once grant) - rather than a stack of separate dialogs,
//! per the design doc's "hard gate before enabling" requirement.

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{LoadState, PluginDetailMode, ZodeAppState};
use zode_node_protocol::{PluginCapabilityKind, PluginCapabilitySummary, PluginTrustState};

use crate::theme::paint_elevated_surface;
use crate::{paint_single_line, stable_widget_id, Button, ButtonVariant, WidgetId, ZodeTheme};

pub const PLUGIN_DETAIL_CLOSE_ID: WidgetId = WidgetId(305);
pub const PLUGIN_DETAIL_UNINSTALL_ID: WidgetId = WidgetId(306);
pub const PLUGIN_DETAIL_UNINSTALL_CONFIRM_ID: WidgetId = WidgetId(307);
pub const PLUGIN_DETAIL_UNINSTALL_CANCEL_ID: WidgetId = WidgetId(308);
pub const PLUGIN_DETAIL_CHECK_UPDATE_ID: WidgetId = WidgetId(309);
pub const PLUGIN_DETAIL_TRUST_ALL_ID: WidgetId = WidgetId(310);
pub const PLUGIN_DETAIL_TRUST_CANCEL_ID: WidgetId = WidgetId(311);
pub const PLUGIN_DETAIL_TRUST_GRANT_SELECTED_ID: WidgetId = WidgetId(312);

const PANEL_W: f32 = 520.0;
const PAD: f32 = 20.0;
const HEADER_H: f32 = 64.0;
const ROW_H: f32 = 30.0;
const BUTTON_H: f32 = 32.0;
const FOOTER_H: f32 = 64.0;
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

#[derive(Debug, Clone, PartialEq)]
pub enum PluginDetailBody {
    Overview {
        capabilities: Vec<CapabilityRowLayout>,
        check_update: Rect,
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
        let extra = match detail.mode {
            PluginDetailMode::ConfirmUninstall => 64.0,
            PluginDetailMode::Uninstalling => 40.0,
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
                    check_update: Rect::xywh(panel.origin.x + PAD, footer_y, 96.0, BUTTON_H),
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

pub fn paint(
    painter: &mut dyn Painter,
    scrim: Rect,
    layout: &PluginDetailOverlayLayout,
    theme: &ZodeTheme,
) {
    painter.fill_rect(scrim, theme.tokens.background.with_alpha(0.5));
    paint_elevated_surface(painter, layout.panel, 14.0, theme);
    painter.fill_round_rect(layout.panel, 14.0, theme.tokens.card);
    painter.stroke_round_rect(layout.panel, 14.0, theme.tokens.border, 1.0);

    paint_single_line(
        painter,
        &layout.repo,
        Rect::xywh(
            layout.panel.origin.x + PAD,
            layout.panel.origin.y + 16.0,
            layout.panel.size.x - PAD * 2.0 - 40.0,
            22.0,
        ),
        15.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        &format!("ref: {}", layout.reference),
        Rect::xywh(
            layout.panel.origin.x + PAD,
            layout.panel.origin.y + 38.0,
            layout.panel.size.x - PAD * 2.0 - 40.0,
            16.0,
        ),
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    Button::paint(
        painter,
        layout.close,
        8.0,
        "关闭",
        None,
        ButtonVariant::Ghost,
        false,
        &theme.tokens,
    );

    match &layout.body {
        PluginDetailBody::Overview {
            capabilities,
            check_update,
            uninstall,
            notice,
        } => {
            for row in capabilities {
                paint_capability_row(painter, row, theme);
            }
            if let Some(notice) = notice {
                paint_single_line(
                    painter,
                    notice,
                    Rect::xywh(
                        layout.panel.origin.x + PAD,
                        check_update.origin.y - 22.0,
                        layout.panel.size.x - PAD * 2.0,
                        18.0,
                    ),
                    11.0,
                    450,
                    theme.tokens.muted_foreground,
                    HorizontalAlign::Start,
                );
            }
            Button::paint(
                painter,
                *check_update,
                8.0,
                "检查更新",
                None,
                ButtonVariant::Secondary,
                false,
                &theme.tokens,
            );
            Button::paint(
                painter,
                *uninstall,
                8.0,
                "删除",
                None,
                ButtonVariant::Destructive,
                false,
                &theme.tokens,
            );
        }
        PluginDetailBody::ConfirmUninstall { confirm, cancel } => {
            paint_single_line(
                painter,
                "确认删除该插件？将删除本地文件与信任记录。",
                Rect::xywh(
                    layout.panel.origin.x + PAD,
                    layout.panel.origin.y + HEADER_H,
                    layout.panel.size.x - PAD * 2.0,
                    40.0,
                ),
                13.0,
                450,
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
            Button::paint(
                painter,
                *cancel,
                8.0,
                "取消",
                None,
                ButtonVariant::Secondary,
                false,
                &theme.tokens,
            );
            Button::paint(
                painter,
                *confirm,
                8.0,
                "确认删除",
                None,
                ButtonVariant::Destructive,
                false,
                &theme.tokens,
            );
        }
        PluginDetailBody::Uninstalling => {
            paint_single_line(
                painter,
                "正在删除…",
                Rect::xywh(
                    layout.panel.origin.x + PAD,
                    layout.panel.origin.y + HEADER_H,
                    layout.panel.size.x - PAD * 2.0,
                    24.0,
                ),
                13.0,
                450,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
        }
        PluginDetailBody::TrustReview {
            items,
            trust_all,
            grant_selected,
            grant_selected_enabled,
            cancel,
            loading,
            error,
        } => {
            paint_single_line(
                painter,
                "以下能力将执行代码，请核对原文后再启用：",
                Rect::xywh(
                    layout.panel.origin.x + PAD,
                    layout.panel.origin.y + HEADER_H - 20.0,
                    layout.panel.size.x - PAD * 2.0,
                    18.0,
                ),
                12.0,
                500,
                theme.tokens.muted_foreground,
                HorizontalAlign::Start,
            );
            if *loading {
                paint_single_line(
                    painter,
                    "正在加载审查内容…",
                    Rect::xywh(
                        layout.panel.origin.x + PAD,
                        layout.panel.origin.y + HEADER_H,
                        layout.panel.size.x - PAD * 2.0,
                        20.0,
                    ),
                    12.0,
                    450,
                    theme.tokens.muted_foreground,
                    HorizontalAlign::Start,
                );
            }
            if let Some(error) = error {
                paint_single_line(
                    painter,
                    error,
                    Rect::xywh(
                        layout.panel.origin.x + PAD,
                        layout.panel.origin.y + HEADER_H,
                        layout.panel.size.x - PAD * 2.0,
                        20.0,
                    ),
                    12.0,
                    450,
                    theme.tokens.destructive,
                    HorizontalAlign::Start,
                );
            }
            for item in items {
                paint_trust_item(painter, item, theme);
            }
            Button::paint(
                painter,
                *cancel,
                8.0,
                "取消",
                None,
                ButtonVariant::Secondary,
                false,
                &theme.tokens,
            );
            Button::paint(
                painter,
                *grant_selected,
                8.0,
                "信任所选",
                None,
                ButtonVariant::Secondary,
                !grant_selected_enabled,
                &theme.tokens,
            );
            Button::paint(
                painter,
                *trust_all,
                8.0,
                "全部信任",
                None,
                ButtonVariant::Primary,
                false,
                &theme.tokens,
            );
        }
    }
}

fn paint_capability_row(painter: &mut dyn Painter, row: &CapabilityRowLayout, theme: &ZodeTheme) {
    let kind_label = match row.capability.kind {
        PluginCapabilityKind::Skill => "技能",
        PluginCapabilityKind::Mcp => "MCP",
        PluginCapabilityKind::Hook => "Hook",
    };
    paint_single_line(
        painter,
        &format!("[{kind_label}] {}", row.capability.label),
        Rect::xywh(
            row.rect.origin.x,
            row.rect.origin.y,
            row.rect.size.x * 0.6,
            row.rect.size.y,
        ),
        12.0,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let action_rect = Rect::xywh(
        row.rect.origin.x + row.rect.size.x - 88.0,
        row.rect.origin.y + (row.rect.size.y - 24.0) / 2.0,
        88.0,
        24.0,
    );
    if let Some(action) = row.toggle_action {
        let label = if row.gated {
            "审查"
        } else if action.currently_enabled {
            "停用"
        } else {
            "启用"
        };
        Button::paint(
            painter,
            action_rect,
            8.0,
            label,
            None,
            if row.gated {
                ButtonVariant::Destructive
            } else {
                ButtonVariant::Secondary
            },
            false,
            &theme.tokens,
        );
    } else {
        paint_single_line(
            painter,
            &row.status_label,
            action_rect,
            11.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::Center,
        );
    }
}

fn paint_trust_item(painter: &mut dyn Painter, item: &TrustItemRowLayout, theme: &ZodeTheme) {
    let checkbox = Rect::xywh(item.rect.origin.x, item.rect.origin.y + 4.0, 16.0, 16.0);
    painter.fill_round_rect(
        checkbox,
        4.0,
        if item.selected {
            theme.tokens.primary
        } else {
            theme.tokens.muted
        },
    );
    painter.stroke_round_rect(checkbox, 4.0, theme.tokens.border, 1.0);
    let key_rect = Rect::xywh(
        checkbox.origin.x + 24.0,
        item.rect.origin.y,
        item.rect.size.x - 24.0,
        16.0,
    );
    paint_single_line(
        painter,
        &item.key,
        key_rect,
        11.0,
        550,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let content_rect = Rect::xywh(
        checkbox.origin.x + 24.0,
        item.rect.origin.y + 15.0,
        item.rect.size.x - 24.0,
        14.0,
    );
    // Verbatim, unsummarized text - see the module doc comment and the
    // design doc's "show the exact text" security requirement. Long content
    // is left-aligned and clipped by the row's own bounds rather than
    // truncated with an ellipsis that could hide a trailing malicious
    // argument; a real scrollable/wrapping viewer is a follow-up.
    painter.save();
    painter.clip_rect(content_rect);
    paint_single_line(
        painter,
        &item.content,
        content_rect,
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    painter.restore();
}
