//! Read-only browser spectator panel (M1 route A - see
//! `docs/proposals/builtin-browser.md`). Shows a toolbar (URL, target
//! badge), a live-frame canvas fed by CDP screencast frames, and a status
//! line. No takeover/passthrough here - that's M2.

use jian_widgets::{HorizontalAlign, ImageDrawMode, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, BrowserPanelState, SecondaryPane, ZodeAppState};

use crate::{paint_single_line, RectExt, SemanticIcon, WidgetId, ZodeTheme};

pub const BROWSER_PANEL_CLOSE_ID: WidgetId = WidgetId(251);

const HEADER_HEIGHT: f32 = 46.0;
const URL_ROW_HEIGHT: f32 = 28.0;
const STATUS_ROW_HEIGHT: f32 = 22.0;

/// A decoded-ready frame borrowed from the desktop app shell for exactly
/// one paint call. `image_id` must change whenever `encoded` does - the
/// underlying image cache is keyed by identity, not content, so reusing an
/// id across frames would leave the canvas stuck on the first frame (see
/// `NativeBackend::image_source` in the `zode-app` render layer).
#[derive(Debug, Clone, Copy)]
pub struct BrowserFrameView<'a> {
    pub image_id: u64,
    pub encoded: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserPanelLayout {
    pub rect: Rect,
    pub header: Rect,
    pub close_button: Rect,
    pub url_row: Rect,
    pub canvas: Rect,
    pub status_row: Rect,
}

pub struct BrowserPanel;

impl BrowserPanel {
    pub fn layout(rect: Rect) -> BrowserPanelLayout {
        let header_height = HEADER_HEIGHT.min(rect.size.y.max(0.0));
        let header = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, header_height);
        let close_button = Rect::xywh(
            (rect.max_x() - 40.0).max(rect.origin.x),
            rect.origin.y + (header_height - 32.0).max(0.0) / 2.0,
            32.0_f32.min(rect.size.x.max(0.0)),
            32.0_f32.min(header_height),
        );

        let after_header = (rect.size.y - header_height).max(0.0);
        let url_row_height = URL_ROW_HEIGHT.min(after_header);
        let url_row = Rect::xywh(rect.origin.x, header.max_y(), rect.size.x, url_row_height);

        let after_url = (after_header - url_row_height).max(0.0);
        let status_row_height = STATUS_ROW_HEIGHT.min(after_url);
        let status_row = Rect::xywh(
            rect.origin.x,
            rect.max_y() - status_row_height,
            rect.size.x,
            status_row_height,
        );

        let canvas_height = (after_url - status_row_height).max(0.0);
        let canvas = Rect::xywh(rect.origin.x, url_row.max_y(), rect.size.x, canvas_height);

        BrowserPanelLayout {
            rect,
            header,
            close_button,
            url_row,
            canvas,
            status_row,
        }
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        frame: Option<BrowserFrameView<'_>>,
        state: &BrowserPanelState,
        theme: &ZodeTheme,
    ) {
        let layout = Self::layout(rect);
        painter.fill_rect(layout.rect, theme.tokens.background);
        Self::paint_header(painter, &layout, state, theme);
        Self::paint_url_row(painter, layout.url_row, state, theme);
        Self::paint_canvas(painter, layout.canvas, frame, state, theme);
        Self::paint_status_row(painter, layout.status_row, state, theme);
    }

    fn paint_header(
        painter: &mut dyn Painter,
        layout: &BrowserPanelLayout,
        state: &BrowserPanelState,
        theme: &ZodeTheme,
    ) {
        let badge_text = if state.is_bridge_target {
            "扩展连接"
        } else {
            "本地浏览器"
        };
        let badge_width =
            76.0_f32.min((layout.close_button.origin.x - layout.header.origin.x).max(0.0));
        let badge_rect = Rect::xywh(
            layout.close_button.origin.x - badge_width,
            layout.header.origin.y,
            badge_width,
            layout.header.size.y,
        );
        paint_single_line(
            painter,
            "浏览器",
            Rect::xywh(
                layout.header.origin.x + 16.0,
                layout.header.origin.y,
                (badge_rect.origin.x - layout.header.origin.x - 8.0).max(0.0),
                layout.header.size.y,
            ),
            13.0,
            600,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            badge_text,
            badge_rect,
            11.0,
            500,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            Point2D::new(
                layout.close_button.origin.x + 8.0,
                layout.close_button.origin.y + 8.0,
            ),
            16.0,
            theme.tokens.muted_foreground,
            SemanticIcon::Close.stroke_width(),
        );
        painter.stroke_line(
            Point2D::new(layout.header.origin.x, layout.header.max_y()),
            Point2D::new(layout.header.max_x(), layout.header.max_y()),
            theme.tokens.border,
            1.0,
        );
    }

    fn paint_url_row(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &BrowserPanelState,
        theme: &ZodeTheme,
    ) {
        if rect.size.y <= 0.0 {
            return;
        }
        let url = state.current_url.as_deref().unwrap_or("about:blank");
        paint_single_line(
            painter,
            url,
            Rect::xywh(
                rect.origin.x + 16.0,
                rect.origin.y,
                (rect.size.x - 32.0).max(0.0),
                rect.size.y,
            ),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }

    fn paint_canvas(
        painter: &mut dyn Painter,
        rect: Rect,
        frame: Option<BrowserFrameView<'_>>,
        state: &BrowserPanelState,
        theme: &ZodeTheme,
    ) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        painter.save();
        painter.clip_rect(rect);
        painter.fill_rect(rect, theme.tokens.card);
        match (state.unavailable_reason.as_deref(), frame) {
            (Some(reason), _) => Self::paint_message(painter, rect, reason, theme),
            (None, Some(frame)) if !frame.encoded.is_empty() => {
                painter.draw_image_with_mode(
                    rect,
                    frame.image_id,
                    frame.encoded,
                    ImageDrawMode::Fit,
                );
            }
            (None, _) => Self::paint_message(painter, rect, "暂无画面", theme),
        }
        painter.restore();
    }

    fn paint_message(painter: &mut dyn Painter, rect: Rect, message: &str, theme: &ZodeTheme) {
        paint_single_line(
            painter,
            message,
            rect,
            13.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Center,
        );
    }

    fn paint_status_row(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &BrowserPanelState,
        theme: &ZodeTheme,
    ) {
        if rect.size.y <= 0.0 {
            return;
        }
        let text = if state.unavailable_reason.is_some() {
            "不可用"
        } else if state.has_frame {
            "实时预览"
        } else {
            "等待画面…"
        };
        paint_single_line(
            painter,
            text,
            Rect::xywh(
                rect.origin.x + 16.0,
                rect.origin.y,
                (rect.size.x - 32.0).max(0.0),
                rect.size.y,
            ),
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        (id == BROWSER_PANEL_CLOSE_ID
            && state.presentation.secondary_pane == Some(SecondaryPane::Browser))
        .then_some(AppCommand::CloseSecondary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_clamps_rows_to_a_tiny_rect() {
        let layout = BrowserPanel::layout(Rect::xywh(0.0, 0.0, 40.0, 10.0));
        assert!(layout.header.size.y <= 10.0);
        assert!(layout.url_row.size.y >= 0.0);
        assert!(layout.status_row.size.y >= 0.0);
        assert!(layout.canvas.size.y >= 0.0);
    }

    #[test]
    fn layout_reserves_header_url_and_status_before_canvas() {
        let layout = BrowserPanel::layout(Rect::xywh(0.0, 0.0, 400.0, 400.0));
        assert_eq!(layout.header.size.y, HEADER_HEIGHT);
        assert_eq!(layout.url_row.size.y, URL_ROW_HEIGHT);
        assert_eq!(layout.status_row.size.y, STATUS_ROW_HEIGHT);
        assert_eq!(
            layout.canvas.size.y,
            400.0 - HEADER_HEIGHT - URL_ROW_HEIGHT - STATUS_ROW_HEIGHT
        );
        assert!(layout.canvas.origin.y >= layout.url_row.max_y());
        assert!(layout.status_row.origin.y >= layout.canvas.max_y());
    }

    #[test]
    fn close_command_only_fires_while_the_browser_pane_is_active() {
        let mut state = zode_app_model::demo_state();
        assert_eq!(
            BrowserPanel::command_for_widget(&state, BROWSER_PANEL_CLOSE_ID),
            None
        );
        state.presentation.secondary_pane = Some(SecondaryPane::Browser);
        assert_eq!(
            BrowserPanel::command_for_widget(&state, BROWSER_PANEL_CLOSE_ID),
            Some(AppCommand::CloseSecondary)
        );
        assert_eq!(BrowserPanel::command_for_widget(&state, WidgetId(1)), None);
    }
}
