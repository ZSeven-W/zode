//! Painting half of the plugin detail overlay - the layout half (rects,
//! widget ids, and the state-to-controls mapping) lives in
//! [`super::plugin_detail`], which this module reads but never re-derives.

use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_node_protocol::PluginCapabilityKind;

use super::plugin_detail::{
    CapabilityRowLayout, PluginDetailBody, PluginDetailOverlayLayout, TrustItemRowLayout, HEADER_H,
    NOTICE_LINE_H, PAD,
};
use crate::theme::paint_elevated_surface;
use crate::{paint_single_line, Button, ButtonVariant, ZodeTheme};

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
            update,
            uninstall,
            notice,
        } => {
            for row in capabilities {
                paint_capability_row(painter, row, theme);
            }
            // Stacked upward from the footer: the update status sits closest
            // to its buttons, any one-off notice above it.
            let mut line_y = update.check.origin.y - NOTICE_LINE_H;
            if let Some(status) = &update.status {
                paint_single_line(
                    painter,
                    &status.text,
                    Rect::xywh(
                        layout.panel.origin.x + PAD,
                        line_y,
                        layout.panel.size.x - PAD * 2.0,
                        18.0,
                    ),
                    11.0,
                    450,
                    if status.error {
                        theme.tokens.destructive
                    } else {
                        theme.tokens.muted_foreground
                    },
                    HorizontalAlign::Start,
                );
                line_y -= NOTICE_LINE_H;
            }
            if let Some(notice) = notice {
                paint_single_line(
                    painter,
                    notice,
                    Rect::xywh(
                        layout.panel.origin.x + PAD,
                        line_y,
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
                update.check,
                8.0,
                &update.check_label,
                None,
                ButtonVariant::Secondary,
                update.check_disabled,
                &theme.tokens,
            );
            if let Some(apply) = update.apply {
                Button::paint(
                    painter,
                    apply,
                    8.0,
                    &update.apply_label,
                    None,
                    ButtonVariant::Primary,
                    update.apply_disabled,
                    &theme.tokens,
                );
            }
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
