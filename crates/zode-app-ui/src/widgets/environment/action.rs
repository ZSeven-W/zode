use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::EnvironmentAction;

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub(super) const ACTION_ROW_HEIGHT: f32 = 31.0;
const ICON_SIZE: f32 = 17.0;
const ICON_TEXT_GAP: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentActionLayout {
    pub id: WidgetId,
    pub action: EnvironmentAction,
    pub rect: Rect,
    pub label: String,
    pub value: Option<String>,
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    actions: &[EnvironmentActionLayout],
    theme: &ZodeTheme,
) {
    for layout in actions {
        let enabled = layout.action.enabled();
        let foreground = if enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground.with_alpha(0.72)
        };
        let icon = icon_for(layout.action.kind);
        let icon_origin = jian_widgets::Point2D::new(
            layout.rect.origin.x,
            layout.rect.origin.y + (layout.rect.size.y - ICON_SIZE) / 2.0,
        );
        painter.stroke_svg_path(
            icon.path(),
            icon_origin,
            ICON_SIZE,
            foreground,
            icon.stroke_width(),
        );
        let text_x = layout.rect.origin.x + ICON_SIZE + ICON_TEXT_GAP;
        let trailing_width = if layout.action.unavailable_reason.is_some() {
            112.0
        } else if layout.value.is_some() {
            104.0
        } else if trailing_icon(layout.action.kind).is_some() {
            18.0
        } else {
            0.0
        };
        paint_single_line(
            painter,
            &layout.label,
            Rect::xywh(
                text_x,
                layout.rect.origin.y,
                (layout.rect.max_x() - text_x - trailing_width - 8.0).max(0.0),
                layout.rect.size.y,
            ),
            14.0,
            400,
            foreground,
            HorizontalAlign::Start,
        );
        let trailing_color =
            theme
                .tokens
                .muted_foreground
                .with_alpha(if enabled { 1.0 } else { 0.72 });
        if let Some(trailing) = layout
            .action
            .unavailable_reason
            .map(|reason| reason.message())
            .or(layout.value.as_deref())
        {
            paint_single_line(
                painter,
                trailing,
                Rect::xywh(
                    layout.rect.max_x() - trailing_width,
                    layout.rect.origin.y,
                    trailing_width,
                    layout.rect.size.y,
                ),
                if enabled { 14.0 } else { 11.0 },
                400,
                trailing_color,
                HorizontalAlign::End,
            );
        } else if let Some(icon) = trailing_icon(layout.action.kind) {
            let size = 15.0;
            painter.stroke_svg_path(
                icon.path(),
                jian_widgets::Point2D::new(
                    layout.rect.max_x() - size,
                    layout.rect.origin.y + (layout.rect.size.y - size) / 2.0,
                ),
                size,
                trailing_color,
                icon.stroke_width(),
            );
        }
    }
}

fn icon_for(kind: zode_app_model::EnvironmentActionKind) -> SemanticIcon {
    match kind {
        zode_app_model::EnvironmentActionKind::RefreshStatus => SemanticIcon::Diff,
        zode_app_model::EnvironmentActionKind::CompareWorkspaceToHead => SemanticIcon::Compare,
        zode_app_model::EnvironmentActionKind::OpenWorkspace => SemanticIcon::Host,
        zode_app_model::EnvironmentActionKind::CommitOrPush => SemanticIcon::Git,
    }
}

fn trailing_icon(kind: zode_app_model::EnvironmentActionKind) -> Option<SemanticIcon> {
    match kind {
        zode_app_model::EnvironmentActionKind::OpenWorkspace => Some(SemanticIcon::ChevronDown),
        zode_app_model::EnvironmentActionKind::CompareWorkspaceToHead => {
            Some(SemanticIcon::ExternalOpen)
        }
        zode_app_model::EnvironmentActionKind::RefreshStatus
        | zode_app_model::EnvironmentActionKind::CommitOrPush => None,
    }
}
