use jian_widgets::{ImageDrawMode, Painter, Point2D, Rect};
use zode_app_model::ImageItem;

use crate::{stable_widget_id, RectExt, SemanticIcon, ZodeTheme};

use super::{draw_text, file_card::paint_icon_tile};

/// Inline thumbnails never grow taller than this, matching the reference
/// design's compact image cards; the lightbox is where a viewer sees the
/// image at its real (or zoomed) size.
pub(super) const MAX_HEIGHT: f32 = 180.0;

/// Height used while the natural aspect ratio is still unknown (before a
/// host-side decode fills in `ImageItem::width`/`height` - see the type's own
/// doc comment). Deliberately below `MAX_HEIGHT` so the common case of a
/// square-ish screenshot does not visibly grow once the real size lands via
/// `AppCommand::SetTranscriptItemHeight`.
const PLACEHOLDER_HEIGHT: f32 = 140.0;

const CARD_PADDING: f32 = 12.0;

/// Namespace for the source bytes this card's thumbnail draws, distinct from
/// `ThreadTranscript::semantic_widget_id`'s own `0x19` for this same item -
/// an image id and a widget id are different addressing spaces even when
/// both are derived from the same `ImageItem::id`.
const IMAGE_SOURCE_NAMESPACE: u8 = 0xA0;

/// Host-supplied lookup for already-loaded image bytes, threaded through the
/// transcript paint path as an ephemeral borrowed view - mirrors how
/// `BrowserFrameView` supplies a live frame without putting encoded bytes in
/// `ZodeAppState`. Returning `None` (including "no provider wired up yet")
/// paints the icon-tile placeholder below for that frame; a host that wants
/// real pixels populates its own bytes cache from `ImageItem::path` and
/// implements this trait over it.
pub trait TranscriptImageSource {
    fn lookup(&self, item: &ImageItem) -> Option<TranscriptImageBytes<'_>>;
}

pub struct TranscriptImageBytes<'a> {
    pub encoded: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Estimated card height for layout purposes, before any bytes are
/// available. Uses the real aspect ratio when attribution (or a later
/// `SetTranscriptItemHeight` correction) has already supplied one; otherwise
/// a fixed placeholder - see `PLACEHOLDER_HEIGHT`.
pub(super) fn estimated_height(item: &ImageItem, width: f32) -> f32 {
    let inner_width = (width - CARD_PADDING * 2.0).max(1.0);
    let inner_height = match (item.width, item.height) {
        (Some(natural_width), Some(natural_height)) if natural_width > 0 && natural_height > 0 => {
            let aspect = natural_height as f32 / natural_width as f32;
            (inner_width * aspect).min(MAX_HEIGHT)
        }
        _ => PLACEHOLDER_HEIGHT,
    };
    inner_height + CARD_PADDING * 2.0
}

/// Public (not `pub(super)`) so the lightbox overlay - a sibling widget
/// module, not a descendant of `transcript` - can derive the same
/// `draw_image` id for the same item's full-size view as the inline
/// thumbnail used. Re-exported at the crate root alongside
/// `TranscriptImageSource`/`TranscriptImageBytes`.
pub fn image_source_id(item: &ImageItem) -> u64 {
    stable_widget_id(IMAGE_SOURCE_NAMESPACE, &item.id).0
}

/// Public wrapper around `estimated_height` for a host that just learned an
/// item's natural pixel size (via a background decode) and wants to
/// dispatch `AppCommand::SetTranscriptItemHeight` with the exact card
/// height the paint path would compute for that size - see
/// `ImageItem`'s own doc comment and `crates/zode-app/src/app/transcript-images.rs`.
/// `card_width` must be the same full-card width `compute_offsets` uses
/// (the transcript content rect's width), not the padded inner width.
pub fn corrected_card_height(item: &ImageItem, card_width: f32) -> f32 {
    estimated_height(item, card_width)
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    rect: Rect,
    item: &ImageItem,
    image_source: Option<&dyn TranscriptImageSource>,
    theme: &ZodeTheme,
) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
    let inner = Rect::xywh(
        rect.origin.x + CARD_PADDING,
        rect.origin.y + CARD_PADDING,
        (rect.size.x - CARD_PADDING * 2.0).max(0.0),
        (rect.size.y - CARD_PADDING * 2.0).max(0.0),
    );
    if let Some(bytes) = image_source.and_then(|source| source.lookup(item)) {
        painter.save();
        painter.clip_round_rect(inner, 8.0);
        painter.draw_image_with_mode(
            inner,
            image_source_id(item),
            bytes.encoded,
            ImageDrawMode::Fit,
        );
        painter.restore();
        painter.stroke_round_rect(inner, 8.0, theme.tokens.border, 1.0);
    } else {
        paint_placeholder(painter, inner, item, theme);
    }
    paint_zoom_hint(painter, inner, theme);
}

/// Rendered when the host has not (yet) supplied decoded bytes for this
/// item's path - an icon tile plus filename, matching `attachment.rs`'s
/// existing fallback style so an undecoded image never looks broken.
fn paint_placeholder(painter: &mut dyn Painter, rect: Rect, item: &ImageItem, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 8.0, theme.tokens.muted);
    paint_icon_tile(painter, rect, SemanticIcon::Snapshot, theme);
    let name = item.path.rsplit('/').next().unwrap_or(item.path.as_str());
    draw_text(
        painter,
        name,
        Point2D::new(
            rect.origin.x + 64.0,
            rect.origin.y + rect.size.y / 2.0 - 8.0,
        ),
        13.0,
        600,
        theme.tokens.foreground,
    );
    let details = match (item.width, item.height) {
        (Some(width), Some(height)) => format!("{width}×{height}"),
        _ => "预览待生成".to_string(),
    };
    draw_text(
        painter,
        &details,
        Point2D::new(
            rect.origin.x + 64.0,
            rect.origin.y + rect.size.y / 2.0 + 12.0,
        ),
        11.0,
        400,
        theme.tokens.muted_foreground,
    );
}

/// Small "点击查看" affordance in the corner, hinting the card opens a
/// lightbox on click - always painted (not hover-gated) since transcript
/// cards have no persistent hover state on touch/trackpad-only input.
fn paint_zoom_hint(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    if rect.size.x < 90.0 || rect.size.y < 28.0 {
        return;
    }
    let badge = Rect::xywh(rect.max_x() - 74.0, rect.max_y() - 26.0, 66.0, 20.0);
    painter.fill_round_rect(badge, 10.0, theme.tokens.card.with_alpha(0.85));
    draw_text(
        painter,
        "点击查看",
        Point2D::new(badge.origin.x + 8.0, badge.origin.y + 4.0),
        10.0,
        600,
        theme.tokens.foreground,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(width: Option<u32>, height: Option<u32>) -> ImageItem {
        ImageItem {
            id: "image:1".into(),
            path: "/repo/logo.png".into(),
            media_type: "image/png".into(),
            width,
            height,
        }
    }

    #[test]
    fn placeholder_height_is_used_before_natural_size_is_known() {
        assert_eq!(
            estimated_height(&item(None, None), 400.0),
            PLACEHOLDER_HEIGHT + CARD_PADDING * 2.0
        );
    }

    #[test]
    fn estimated_height_preserves_aspect_ratio_up_to_the_cap() {
        // 400x200 at a 376 px inner width (400 - 2*12) scales to a 188 px
        // tall image, under the 180 px cap.
        let height = estimated_height(&item(Some(400), Some(200)), 400.0);
        assert!((height - (180.0 + CARD_PADDING * 2.0)).abs() < 1.0);
    }

    #[test]
    fn a_very_tall_image_is_capped_at_max_height() {
        let height = estimated_height(&item(Some(100), Some(2000)), 400.0);
        assert_eq!(height, MAX_HEIGHT + CARD_PADDING * 2.0);
    }

    #[test]
    fn image_source_id_is_stable_for_the_same_item_id() {
        let a = item(None, None);
        let mut b = item(None, None);
        b.path = "/other/path.png".into();
        assert_eq!(image_source_id(&a), image_source_id(&b));
    }
}
