use jian_widgets::Rect;
use thiserror::Error;
use zode_app_model::{SystemTheme, ZodeAppState};
use zode_app_ui::{Insets, WorkspaceShell, ZodeTheme};

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
/// the backend is constructed so native hosts can inject their bundled family.
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
    if !fonts.is_empty() {
        jian_skia::register_bundled_fonts(fonts);
    }

    let mut surface = RasterSurface::new(physical_width, physical_height)?;
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::WHITE);
    canvas.scale((scale, scale));

    let mut backend = NativeBackend::new(scale);
    let mut painter = FramePainter::new(&mut backend, canvas);
    let theme = match state.host.system_theme {
        SystemTheme::Light => ZodeTheme::light(),
        SystemTheme::Dark => ZodeTheme::dark(),
    };
    WorkspaceShell::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, width as f32, height as f32),
        Insets::ZERO,
        state,
        &theme,
    );
    drop(painter);
    surface.encode_png()
}

fn physical_extent(logical: u32, scale: f32) -> Result<u32, RenderError> {
    let value = (logical as f64 * f64::from(scale)).round();
    if !value.is_finite() || value < 1.0 || value > f64::from(i32::MAX) {
        return Err(RenderError::DimensionsTooLarge);
    }
    Ok(value as u32)
}
