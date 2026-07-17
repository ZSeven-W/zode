//! Full-size image preview overlay opened from a transcript `Image` card
//! (see `widgets::transcript::image`). Painted last, above every other
//! shell layer, mirroring `GlobalSearch`'s scrim + centered surface
//! convention (`workspace_shell::paint_snapshot_content`).

use jian_widgets::{Color, HorizontalAlign, ImageDrawMode, Painter, Point2D, Rect};
use zode_app_model::{AppCommand, ImageItem, LightboxState, ZodeAppState};

use crate::{
    paint_elevated_surface, paint_single_line,
    widgets::transcript::{image_source_id, TranscriptImageSource},
    RectExt, SemanticIcon, WidgetId, ZodeTheme,
};

pub const LIGHTBOX_SCRIM_ID: WidgetId = WidgetId(260);
pub const LIGHTBOX_CLOSE_ID: WidgetId = WidgetId(261);
pub const LIGHTBOX_ZOOM_OUT_ID: WidgetId = WidgetId(262);
pub const LIGHTBOX_ZOOM_IN_ID: WidgetId = WidgetId(263);

const TOOLBAR_HEIGHT: f32 = 44.0;
const TOOLBAR_GAP: f32 = 16.0;
const EDGE_INSET: f32 = 48.0;
const STEP_BUTTON_SIZE: f32 = 32.0;
const CLOSE_BUTTON_SIZE: f32 = 36.0;
const ZOOM_LABEL_WIDTH: f32 = 56.0;

#[derive(Debug, Clone, PartialEq)]
pub struct LightboxLayout {
    pub scrim: Rect,
    pub image_rect: Rect,
    pub toolbar: Rect,
    pub zoom_out: Rect,
    pub zoom_label: Rect,
    pub zoom_in: Rect,
    pub close: Rect,
    pub zoom_percent: u32,
    pub item: ImageItem,
}

pub struct Lightbox;

impl Lightbox {
    /// `None` when no lightbox is open, or the addressed session/item no
    /// longer exists (a defensive check - see `session_has_image_item` in
    /// `zode-app-model`, which already refuses to *open* one for a missing
    /// item; this covers the item disappearing while already open, which
    /// cannot currently happen since transcripts only append, but keeps
    /// paint/layout/hit-testing agreeing on "is a lightbox visible" without
    /// a second source of truth).
    pub fn layout(viewport: Rect, state: &ZodeAppState) -> Option<LightboxLayout> {
        if viewport.size.x <= 0.0 || viewport.size.y <= 0.0 {
            return None;
        }
        let lightbox = state.lightbox.as_ref()?;
        let item = find_image_item(state, lightbox)?.clone();
        let zoom_percent = lightbox.zoom_percent();

        let available = Rect::xywh(
            viewport.origin.x + EDGE_INSET,
            viewport.origin.y + EDGE_INSET,
            (viewport.size.x - EDGE_INSET * 2.0).max(1.0),
            (viewport.size.y - EDGE_INSET * 2.0 - TOOLBAR_HEIGHT - TOOLBAR_GAP).max(1.0),
        );
        let image_rect = fitted_image_rect(viewport, available, &item, zoom_percent);
        let toolbar_width = STEP_BUTTON_SIZE * 2.0 + ZOOM_LABEL_WIDTH + 16.0;
        let toolbar = Rect::xywh(
            viewport.origin.x + (viewport.size.x - toolbar_width) / 2.0,
            available.max_y() + TOOLBAR_GAP,
            toolbar_width,
            TOOLBAR_HEIGHT,
        );
        let zoom_out = Rect::xywh(
            toolbar.origin.x,
            toolbar.origin.y + (TOOLBAR_HEIGHT - STEP_BUTTON_SIZE) / 2.0,
            STEP_BUTTON_SIZE,
            STEP_BUTTON_SIZE,
        );
        let zoom_label = Rect::xywh(
            zoom_out.max_x(),
            toolbar.origin.y,
            ZOOM_LABEL_WIDTH,
            TOOLBAR_HEIGHT,
        );
        let zoom_in = Rect::xywh(
            zoom_label.max_x(),
            zoom_out.origin.y,
            STEP_BUTTON_SIZE,
            STEP_BUTTON_SIZE,
        );
        let close = Rect::xywh(
            viewport.max_x() - EDGE_INSET / 2.0 - CLOSE_BUTTON_SIZE,
            viewport.origin.y + EDGE_INSET / 2.0,
            CLOSE_BUTTON_SIZE,
            CLOSE_BUTTON_SIZE,
        );
        Some(LightboxLayout {
            scrim: viewport,
            image_rect,
            toolbar,
            zoom_out,
            zoom_label,
            zoom_in,
            close,
            zoom_percent,
            item,
        })
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        state.lightbox.as_ref()?;
        if id == LIGHTBOX_SCRIM_ID || id == LIGHTBOX_CLOSE_ID {
            return Some(AppCommand::CloseLightbox);
        }
        if id == LIGHTBOX_ZOOM_IN_ID {
            return Some(AppCommand::StepLightboxZoom { increase: true });
        }
        if id == LIGHTBOX_ZOOM_OUT_ID {
            return Some(AppCommand::StepLightboxZoom { increase: false });
        }
        None
    }

    pub fn paint(
        painter: &mut dyn Painter,
        layout: &LightboxLayout,
        image_source: Option<&dyn TranscriptImageSource>,
        hovered: Option<WidgetId>,
        theme: &ZodeTheme,
    ) {
        painter.fill_rect(layout.scrim, Color::BLACK.with_alpha(0.75));
        if let Some(bytes) = image_source.and_then(|source| source.lookup(&layout.item)) {
            painter.draw_image_with_mode(
                layout.image_rect,
                image_source_id(&layout.item),
                bytes.encoded,
                ImageDrawMode::Fit,
            );
        } else {
            paint_undecoded_placeholder(painter, layout.image_rect, &layout.item, theme);
        }

        paint_elevated_surface(painter, layout.toolbar, 22.0, theme);
        painter.fill_round_rect(layout.toolbar, 22.0, theme.tokens.popover);
        painter.stroke_round_rect(layout.toolbar, 22.0, theme.tokens.border, 1.0);
        paint_step_button(
            painter,
            layout.zoom_out,
            "-",
            layout.zoom_percent > zode_app_model::LIGHTBOX_ZOOM_STEPS[0],
            hovered == Some(LIGHTBOX_ZOOM_OUT_ID),
            theme,
        );
        paint_single_line(
            painter,
            &format!("{}%", layout.zoom_percent),
            layout.zoom_label,
            13.0,
            600,
            theme.tokens.popover_foreground,
            HorizontalAlign::Center,
        );
        let max_zoom = *zode_app_model::LIGHTBOX_ZOOM_STEPS
            .last()
            .expect("zoom step table is non-empty");
        paint_step_button(
            painter,
            layout.zoom_in,
            "+",
            layout.zoom_percent < max_zoom,
            hovered == Some(LIGHTBOX_ZOOM_IN_ID),
            theme,
        );

        paint_elevated_surface(painter, layout.close, CLOSE_BUTTON_SIZE / 2.0, theme);
        painter.fill_round_rect(
            layout.close,
            CLOSE_BUTTON_SIZE / 2.0,
            if hovered == Some(LIGHTBOX_CLOSE_ID) {
                theme.tokens.accent
            } else {
                theme.tokens.popover
            },
        );
        painter.stroke_round_rect(
            layout.close,
            CLOSE_BUTTON_SIZE / 2.0,
            theme.tokens.border,
            1.0,
        );
        let icon_size = 16.0;
        painter.stroke_svg_path(
            SemanticIcon::Close.path(),
            Point2D::new(
                layout.close.origin.x + (layout.close.size.x - icon_size) / 2.0,
                layout.close.origin.y + (layout.close.size.y - icon_size) / 2.0,
            ),
            icon_size,
            theme.tokens.popover_foreground,
            SemanticIcon::Close.stroke_width(),
        );
    }
}

fn find_image_item<'a>(state: &'a ZodeAppState, lightbox: &LightboxState) -> Option<&'a ImageItem> {
    let transcript = state.transcripts.get(&lightbox.session)?;
    transcript.items.iter().find_map(|item| match item {
        zode_app_model::TranscriptItem::Image(image) if image.id == lightbox.item_id => Some(image),
        _ => None,
    })
}

/// Scales `item`'s natural size by `zoom_percent`, then uniformly shrinks
/// (never grows) the result so it never exceeds `available` - there is no
/// scroll region yet, so an unclamped zoom past 100% on a small window would
/// otherwise just paint outside the visible surface. When the natural size
/// is not yet known (no host decode has run - see `ImageItem`'s own doc
/// comment), the box simply fills `available`; the zoom stepper still moves,
/// it just has nothing to visibly scale until a real size lands.
fn fitted_image_rect(viewport: Rect, available: Rect, item: &ImageItem, zoom_percent: u32) -> Rect {
    let Some((natural_width, natural_height)) = item
        .width
        .zip(item.height)
        .filter(|(width, height)| *width > 0 && *height > 0)
    else {
        return available;
    };
    let scale = zoom_percent as f32 / 100.0;
    let desired_width = natural_width as f32 * scale;
    let desired_height = natural_height as f32 * scale;
    let clamp = (available.size.x / desired_width)
        .min(available.size.y / desired_height)
        .min(1.0);
    let width = (desired_width * clamp).max(1.0);
    let height = (desired_height * clamp).max(1.0);
    Rect::xywh(
        viewport.origin.x + (viewport.size.x - width) / 2.0,
        available.origin.y + (available.size.y - height) / 2.0,
        width,
        height,
    )
}

fn paint_step_button(
    painter: &mut dyn Painter,
    rect: Rect,
    glyph: &str,
    enabled: bool,
    hovered: bool,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(
        rect,
        rect.size.x / 2.0,
        if hovered && enabled {
            theme.tokens.accent
        } else {
            theme.tokens.popover
        },
    );
    let color = if enabled {
        theme.tokens.popover_foreground
    } else {
        theme.tokens.muted_foreground
    };
    paint_single_line(
        painter,
        glyph,
        rect,
        16.0,
        600,
        color,
        HorizontalAlign::Center,
    );
}

fn paint_undecoded_placeholder(
    painter: &mut dyn Painter,
    rect: Rect,
    item: &ImageItem,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
    let name = item.path.rsplit('/').next().unwrap_or(item.path.as_str());
    paint_single_line(
        painter,
        name,
        Rect::xywh(
            rect.origin.x + 16.0,
            rect.origin.y + rect.size.y / 2.0 - 10.0,
            (rect.size.x - 32.0).max(0.0),
            20.0,
        ),
        14.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Center,
    );
}
