use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{SecondaryPane, ZodeAppState};

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const UNAVAILABLE_SECONDARY_CLOSE_ID: WidgetId = WidgetId(111);

pub struct UnavailableSecondaryPanel;

impl UnavailableSecondaryPanel {
    pub fn paint(painter: &mut dyn Painter, rect: Rect, pane: SecondaryPane, theme: &ZodeTheme) {
        painter.fill_rect(rect, theme.tokens.background);
        let header = Rect::xywh(
            rect.origin.x,
            rect.origin.y,
            rect.size.x,
            46.0_f32.min(rect.size.y),
        );
        let close = Self::close_button(rect);
        paint_single_line(
            painter,
            Self::title(pane),
            Rect::xywh(
                header.origin.x + 16.0,
                header.origin.y,
                (close.origin.x - header.origin.x - 24.0).max(0.0),
                header.size.y,
            ),
            13.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            jian_widgets::Point2D::new(close.origin.x + 8.0, close.origin.y + 8.0),
            16.0,
            theme.tokens.muted_foreground,
            SemanticIcon::Close.stroke_width(),
        );
        painter.stroke_line(
            jian_widgets::Point2D::new(header.origin.x, header.max_y()),
            jian_widgets::Point2D::new(header.max_x(), header.max_y()),
            theme.tokens.border,
            1.0,
        );
        paint_single_line(
            painter,
            Self::message(pane),
            Rect::xywh(
                rect.origin.x + 24.0,
                header.max_y() + 24.0,
                (rect.size.x - 48.0).max(0.0),
                24.0,
            ),
            13.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }

    pub fn close_button(rect: Rect) -> Rect {
        Rect::xywh(
            (rect.max_x() - 40.0).max(rect.origin.x),
            rect.origin.y + 7.0,
            32.0_f32.min(rect.size.x.max(0.0)),
            32.0_f32.min(rect.size.y.max(0.0)),
        )
    }

    pub const fn title(pane: SecondaryPane) -> &'static str {
        match pane {
            SecondaryPane::Browser => "浏览器",
            SecondaryPane::Files => "文件",
            SecondaryPane::SideTask => "侧边任务",
            // `Subagents` never actually reaches this widget - it paints via
            // `SubagentsPanel` instead (see `workspace_shell.rs`) - this arm
            // exists only to keep the match exhaustive.
            SecondaryPane::Environment
            | SecondaryPane::Review
            | SecondaryPane::DocumentPreview
            | SecondaryPane::Terminal
            | SecondaryPane::Subagents => "侧边面板",
        }
    }

    pub const fn message(pane: SecondaryPane) -> &'static str {
        match pane {
            SecondaryPane::Browser => "浏览器会话尚未接入桌面端。",
            SecondaryPane::Files => "文件树查询尚未接入桌面端。",
            SecondaryPane::SideTask => "侧边任务尚未接入桌面端。",
            SecondaryPane::Environment
            | SecondaryPane::Review
            | SecondaryPane::DocumentPreview
            | SecondaryPane::Terminal
            | SecondaryPane::Subagents => "该面板当前不可用。",
        }
    }

    pub fn command_for_widget(
        state: &ZodeAppState,
        id: WidgetId,
    ) -> Option<zode_app_model::AppCommand> {
        (id == UNAVAILABLE_SECONDARY_CLOSE_ID
            && matches!(
                state.presentation.secondary_pane,
                Some(SecondaryPane::Browser | SecondaryPane::Files | SecondaryPane::SideTask)
            ))
        .then_some(zode_app_model::AppCommand::CloseSecondary)
    }
}
