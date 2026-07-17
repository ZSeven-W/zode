use jian_widgets::Rect;
use thiserror::Error;
use zode_app_model::ZodeAppState;
use zode_app_ui::{Insets, WorkspaceShell};

use super::{FramePainter, NativeBackend, RasterSurface};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("snapshot dimensions and scale must be finite and greater than zero")]
    InvalidDimensions,
    #[error("physical snapshot dimensions exceed the supported range")]
    DimensionsTooLarge,
    #[error("Skia could not encode the snapshot as PNG")]
    PngEncode,
}

pub fn render_offscreen(
    state: &ZodeAppState,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<u8>, RenderError> {
    render_offscreen_with_fonts(state, width, height, scale, Vec::new())
}

/// Paint the shell at logical `width` x `height`, returning a PNG at
/// `scale` physical pixels per logical pixel. Font bytes are registered before
/// the backend is constructed. The first valid face supplies one explicit
/// family override for both paint and measurement during this render.
pub fn render_offscreen_with_fonts(
    state: &ZodeAppState,
    width: u32,
    height: u32,
    scale: f32,
    fonts: Vec<Vec<u8>>,
) -> Result<Vec<u8>, RenderError> {
    if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
        return Err(RenderError::InvalidDimensions);
    }
    let physical_width = physical_extent(width, scale)?;
    let physical_height = physical_extent(height, scale)?;
    let font_family_override = snapshot_font_family(&fonts);
    if font_family_override.is_some() {
        jian_skia::register_bundled_fonts(fonts);
    }

    let mut surface = RasterSurface::new(physical_width, physical_height)?;
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::WHITE);
    canvas.scale((scale, scale));

    let theme = crate::preferences::theme_for_state(state);
    {
        let mut backend = NativeBackend::with_font_family(scale, font_family_override);
        let mut painter = FramePainter::new(&mut backend, canvas);
        WorkspaceShell::paint(
            &mut painter,
            Rect::xywh(0.0, 0.0, width as f32, height as f32),
            Insets::ZERO,
            state,
            &theme,
        );
    }
    surface.encode_png()
}

fn snapshot_font_family(fonts: &[Vec<u8>]) -> Option<String> {
    fonts.iter().find_map(|bytes| {
        jian_skia::parse_imported_font_meta(bytes).map(|metadata| metadata.family)
    })
}

fn physical_extent(logical: u32, scale: f32) -> Result<u32, RenderError> {
    let value = (logical as f64 * f64::from(scale)).round();
    if !value.is_finite() || value < 1.0 || value > f64::from(i32::MAX) {
        return Err(RenderError::DimensionsTooLarge);
    }
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::snapshot_font_family;

    const SNAPSHOT_REGULAR: &[u8] =
        include_bytes!("../../tests/fonts/NotoSansSC-Regular.subset.ttf");
    const SNAPSHOT_SEMIBOLD: &[u8] =
        include_bytes!("../../tests/fonts/NotoSansSC-SemiBold.subset.ttf");

    #[test]
    fn empty_and_invalid_fonts_do_not_select_an_override() {
        assert_eq!(snapshot_font_family(&[]), None);
        assert_eq!(
            snapshot_font_family(&[b"not a font".to_vec(), Vec::new()]),
            None
        );
    }

    #[test]
    fn first_valid_font_selects_the_snapshot_family() {
        let fonts = vec![
            b"not a font".to_vec(),
            SNAPSHOT_REGULAR.to_vec(),
            SNAPSHOT_SEMIBOLD.to_vec(),
        ];

        assert_eq!(
            snapshot_font_family(&fonts).as_deref(),
            Some("Zode Snapshot Sans SC")
        );
    }
}
