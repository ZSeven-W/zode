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

    pub fn read_rgba8(&mut self, buffer: &mut [u8]) -> bool {
        self.inner.read_rgba8(buffer)
    }

    /// Copy the native N32 raster directly into softbuffer's `0x00RRGGBB`
    /// representation. The live desktop path stays on the CPU, so peeking the
    /// raster avoids a second full-frame allocation and color conversion.
    pub fn copy_rgb_to(&mut self, output: &mut [u32]) -> bool {
        let expected_row_bytes = self.size().0 as usize * std::mem::size_of::<u32>();
        let Some(pixmap) = self.inner.canvas().peek_pixels() else {
            return false;
        };
        if pixmap.row_bytes() != expected_row_bytes {
            return false;
        }
        let Some(pixels) = pixmap.pixels::<u32>() else {
            return false;
        };
        if pixels.len() != output.len() {
            return false;
        }
        for (target, source) in output.iter_mut().zip(pixels) {
            // Skia's N32 raster is laid out as RGBA bytes on our supported
            // little-endian desktop targets, while softbuffer expects a
            // numeric 0x00RRGGBB word.
            *target = ((source & 0x0000_00ff) << 16)
                | (source & 0x0000_ff00)
                | ((source & 0x00ff_0000) >> 16);
        }
        true
    }

    /// Copy native premultiplied N32 pixels into Core Graphics BGRA bytes.
    /// Alpha is preserved so the macOS foreground layer can reveal the
    /// native sidebar material without changing opaque main-content RGB.
    pub fn copy_bgra_premultiplied_to(&mut self, output: &mut [u8]) -> bool {
        let expected_row_bytes = self.size().0 as usize * std::mem::size_of::<u32>();
        let Some(pixmap) = self.inner.canvas().peek_pixels() else {
            return false;
        };
        if pixmap.row_bytes() != expected_row_bytes {
            return false;
        }
        let Some(pixels) = pixmap.pixels::<u32>() else {
            return false;
        };
        if pixels.len().saturating_mul(4) != output.len() {
            return false;
        }
        for (target, source) in output.chunks_exact_mut(4).zip(pixels) {
            // Skia N32 is numeric 0xAABBGGRR on our little-endian desktop
            // target. Core Graphics PremultipliedFirst + Order32Little uses
            // the matching in-memory BGRA order.
            target[0] = ((source >> 16) & 0xff) as u8;
            target[1] = ((source >> 8) & 0xff) as u8;
            target[2] = (source & 0xff) as u8;
            target[3] = ((source >> 24) & 0xff) as u8;
        }
        true
    }

    pub fn size(&self) -> (u32, u32) {
        (self.inner.width() as u32, self.inner.height() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::RasterSurface;

    #[test]
    fn native_raster_copies_into_softbuffer_rgb_words() {
        let mut surface = RasterSurface::new(3, 2).expect("surface");
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 0x12, 0x34, 0x56));
        let mut output = vec![0; 6];

        assert!(surface.copy_rgb_to(&mut output));
        assert_eq!(output, vec![0x0012_3456; 6]);
    }

    #[test]
    fn direct_rgb_copy_matches_rgba_readback_for_odd_width_and_blending() {
        let mut surface = RasterSurface::new(5, 3).expect("surface");
        surface.canvas().clear(skia_safe::Color::WHITE);
        let mut paint = skia_safe::Paint::new(skia_safe::Color4f::new(0.2, 0.6, 0.9, 0.45), None);
        paint.set_anti_alias(false);
        surface
            .canvas()
            .draw_rect(skia_safe::Rect::from_xywh(1.0, 0.0, 3.0, 2.0), &paint);

        let mut rgba = vec![0; 5 * 3 * 4];
        let mut direct = vec![0; 5 * 3];
        assert!(surface.read_rgba8(&mut rgba));
        assert!(surface.copy_rgb_to(&mut direct));

        let expected = rgba
            .chunks_exact(4)
            .map(|pixel| {
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2])
            })
            .collect::<Vec<_>>();
        assert_eq!(direct, expected);
    }

    #[test]
    fn bgra_copy_preserves_premultiplied_alpha() {
        let mut surface = RasterSurface::new(2, 1).expect("surface");
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        let mut paint = skia_safe::Paint::new(
            skia_safe::Color4f::new(128.0 / 255.0, 64.0 / 255.0, 32.0 / 255.0, 0.5),
            None,
        );
        paint.set_anti_alias(false);
        surface
            .canvas()
            .draw_rect(skia_safe::Rect::from_xywh(0.0, 0.0, 1.0, 1.0), &paint);
        let mut output = vec![0; 8];

        assert!(surface.copy_bgra_premultiplied_to(&mut output));
        assert_eq!(output[3], 128);
        assert_eq!(output[4..8], [0, 0, 0, 0]);
        assert!(output[0] <= 17, "blue must remain premultiplied");
        assert!(output[1] <= 33, "green must remain premultiplied");
        assert!(output[2] <= 65, "red must remain premultiplied");
    }

    #[test]
    fn bgra_copy_keeps_opaque_main_rgb_exact() {
        let mut surface = RasterSurface::new(1, 1).expect("surface");
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 0x12, 0x34, 0x56));
        let mut output = vec![0; 4];

        assert!(surface.copy_bgra_premultiplied_to(&mut output));
        assert_eq!(output, [0x56, 0x34, 0x12, 0xff]);
    }
}
