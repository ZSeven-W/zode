use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::TerminalState;

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

use super::{TerminalGrid, TerminalPanel, TerminalSelection};

pub const TERMINAL_SECONDARY_CLOSE_ID: WidgetId = WidgetId(110);
const HEADER_HEIGHT: f32 = 46.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalSecondaryLayout {
    pub rect: Rect,
    pub header: Rect,
    pub close_button: Rect,
    pub content: Rect,
}

pub struct TerminalSecondaryPanel;

impl TerminalSecondaryPanel {
    pub fn layout(rect: Rect) -> TerminalSecondaryLayout {
        let header_height = HEADER_HEIGHT.min(rect.size.y.max(0.0));
        TerminalSecondaryLayout {
            rect,
            header: Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, header_height),
            close_button: Rect::xywh(
                (rect.max_x() - 40.0).max(rect.origin.x),
                rect.origin.y + (header_height - 32.0).max(0.0) / 2.0,
                32.0_f32.min(rect.size.x.max(0.0)),
                32.0_f32.min(header_height),
            ),
            content: Rect::xywh(
                rect.origin.x,
                rect.origin.y + header_height,
                rect.size.x,
                (rect.size.y - header_height).max(0.0),
            ),
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        grid: &TerminalGrid,
        state: &TerminalState,
        selection: Option<TerminalSelection>,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(rect);
        painter.fill_rect(layout.rect, theme.tokens.background);
        paint_single_line(
            painter,
            "终端",
            Rect::xywh(
                layout.header.origin.x + 16.0,
                layout.header.origin.y,
                (layout.close_button.origin.x - layout.header.origin.x - 24.0).max(0.0),
                layout.header.size.y,
            ),
            13.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            jian_widgets::Point2D::new(
                layout.close_button.origin.x + 8.0,
                layout.close_button.origin.y + 8.0,
            ),
            16.0,
            theme.tokens.muted_foreground,
            SemanticIcon::Close.stroke_width(),
        );
        painter.stroke_line(
            jian_widgets::Point2D::new(layout.header.origin.x, layout.header.max_y()),
            jian_widgets::Point2D::new(layout.header.max_x(), layout.header.max_y()),
            theme.tokens.border,
            1.0,
        );
        TerminalPanel::paint(painter, layout.content, grid, state, selection, theme);
    }

    pub fn command_for_widget(
        state: &zode_app_model::ZodeAppState,
        id: WidgetId,
    ) -> Option<zode_app_model::AppCommand> {
        (id == TERMINAL_SECONDARY_CLOSE_ID
            && state.presentation.secondary_pane == Some(zode_app_model::SecondaryPane::Terminal))
        .then_some(zode_app_model::AppCommand::CloseSecondary)
    }
}
