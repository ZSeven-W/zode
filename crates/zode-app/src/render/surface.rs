use jian_skia::SkiaSurface;

use super::RenderError;

/// CPU-backed Skia surface used by snapshots and golden tests.
pub struct RasterSurface {
    inner: SkiaSurface,
}

impl RasterSurface {
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let width = i32::try_from(width).map_err(|_| RenderError::DimensionsTooLarge)?;
        let height = i32::try_from(height).map_err(|_| RenderError::DimensionsTooLarge)?;
        Ok(Self {
            inner: SkiaSurface::new_raster(width, height),
        })
    }

    pub fn canvas(&mut self) -> &skia_safe::Canvas {
        self.inner.canvas()
    }

    pub fn encode_png(&mut self) -> Result<Vec<u8>, RenderError> {
        self.inner.encode_png().ok_or(RenderError::PngEncode)
    }
}
