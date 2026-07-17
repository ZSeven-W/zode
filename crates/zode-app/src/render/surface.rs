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
    ///
    /// `present_width`/`present_height` is the window's actual physical
    /// size, which may be smaller than this surface's allocated size (the
    /// surface is kept larger than the window during a live resize drag to
    /// avoid reallocating on every tick - see `app/window.rs`). Only that
    /// sub-rectangle, honoring the surface's real row stride, is copied into
    /// `output`; `output` must be exactly `present_width * present_height`
    /// words.
    pub fn copy_rgb_to(
        &mut self,
        output: &mut [u32],
        present_width: u32,
        present_height: u32,
    ) -> bool {
        let Some((stride_pixels, pixels, color_type)) =
            self.present_pixels(present_width, present_height)
        else {
            return false;
        };
        let (present_width, present_height) = (present_width as usize, present_height as usize);
        if output.len() != present_width * present_height {
            return false;
        }
        for row in 0..present_height {
            let src_start = row * stride_pixels;
            let src_row = &pixels[src_start..src_start + present_width];
            let dst_row = &mut output[row * present_width..(row + 1) * present_width];
            for (target, source) in dst_row.iter_mut().zip(src_row) {
                // Skia's native N32 color type differs by platform. Softbuffer
                // always expects a numeric 0x00RRGGBB word.
                *target = match color_type {
                    skia_safe::ColorType::RGBA8888 => {
                        ((source & 0x0000_00ff) << 16)
                            | (source & 0x0000_ff00)
                            | ((source & 0x00ff_0000) >> 16)
                    }
                    skia_safe::ColorType::BGRA8888 => source & 0x00ff_ffff,
                    _ => return false,
                };
            }
        }
        true
    }

    /// Copy native premultiplied N32 pixels into Core Graphics BGRA bytes.
    /// Alpha is preserved so the macOS foreground layer can reveal the
    /// native sidebar material without changing opaque main-content RGB.
    ///
    /// See `copy_rgb_to` for the `present_width`/`present_height` contract;
    /// `output` must be exactly `present_width * present_height * 4` bytes.
    pub fn copy_bgra_premultiplied_to(
        &mut self,
        output: &mut [u8],
        present_width: u32,
        present_height: u32,
    ) -> bool {
        let Some((stride_pixels, pixels, color_type)) =
            self.present_pixels(present_width, present_height)
        else {
            return false;
        };
        let (present_width, present_height) = (present_width as usize, present_height as usize);
        if output.len() != present_width * present_height * 4 {
            return false;
        }
        for row in 0..present_height {
            let src_start = row * stride_pixels;
            let src_row = &pixels[src_start..src_start + present_width];
            let dst_start = row * present_width * 4;
            let dst_row = &mut output[dst_start..dst_start + present_width * 4];
            for (target, source) in dst_row.chunks_exact_mut(4).zip(src_row) {
                // Core Graphics PremultipliedFirst + Order32Little expects BGRA
                // bytes. Convert from whichever N32 layout Skia selected.
                match color_type {
                    skia_safe::ColorType::RGBA8888 => {
                        target[0] = ((source >> 16) & 0xff) as u8;
                        target[1] = ((source >> 8) & 0xff) as u8;
                        target[2] = (source & 0xff) as u8;
                        target[3] = ((source >> 24) & 0xff) as u8;
                    }
                    skia_safe::ColorType::BGRA8888 => {
                        target.copy_from_slice(&source.to_le_bytes());
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    /// Shared validation for the two `copy_*_to` methods: confirms the
    /// requested present rectangle fits inside the allocated surface, that
    /// the surface is tightly packed (row stride == allocated width, the
    /// invariant Skia raster surfaces uphold), and returns the row stride in
    /// pixels plus the raw pixel slice and native color type to copy from.
    fn present_pixels(
        &mut self,
        present_width: u32,
        present_height: u32,
    ) -> Option<(usize, &[u32], skia_safe::ColorType)> {
        if present_width == 0 || present_height == 0 {
            return None;
        }
        let (surface_width, surface_height) = self.size();
        if present_width > surface_width || present_height > surface_height {
            return None;
        }
        let pixmap = self.inner.canvas().peek_pixels()?;
        let stride_pixels = pixmap.row_bytes() / std::mem::size_of::<u32>();
        if stride_pixels != surface_width as usize {
            return None;
        }
        let color_type = pixmap.color_type();
        let pixels = pixmap.pixels::<u32>()?;
        let present_height = present_height as usize;
        if pixels.len()
            < stride_pixels.saturating_mul(present_height.saturating_sub(1))
                + present_width as usize
        {
            return None;
        }
        Some((stride_pixels, pixels, color_type))
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

        assert!(surface.copy_rgb_to(&mut output, 3, 2));
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
        assert!(surface.copy_rgb_to(&mut direct, 5, 3));

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

        assert!(surface.copy_bgra_premultiplied_to(&mut output, 2, 1));
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

        assert!(surface.copy_bgra_premultiplied_to(&mut output, 1, 1));
        assert_eq!(output, [0x56, 0x34, 0x12, 0xff]);
    }

    /// Regression guard for the sticky-raster-surface optimization: the
    /// surface is deliberately allocated larger than the window during a
    /// live resize, so the copy must use the surface's real row stride and
    /// only touch the requested sub-rectangle - no bleed from the unused
    /// right/bottom margin into what gets presented.
    #[test]
    fn rgb_copy_from_oversized_surface_only_reads_the_present_sub_rect() {
        let mut surface = RasterSurface::new(8, 8).expect("surface");
        // Fill everything with a marker color, then paint only the
        // "window" sub-rect (0,0)-(5,4) a different color. If the copy
        // used the wrong stride it would either read into the marker
        // region or misalign rows.
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 0xaa, 0xaa, 0xaa));
        let mut paint = skia_safe::Paint::new(skia_safe::Color4f::new(0.0, 1.0, 0.0, 1.0), None);
        paint.set_anti_alias(false);
        surface
            .canvas()
            .draw_rect(skia_safe::Rect::from_xywh(0.0, 0.0, 5.0, 4.0), &paint);

        let mut output = vec![0u32; 5 * 4];
        assert!(surface.copy_rgb_to(&mut output, 5, 4));
        assert!(
            output.iter().all(|&pixel| pixel == 0x0000_ff00),
            "expected pure green sub-rect, got {output:?}"
        );
    }

    #[test]
    fn bgra_copy_from_oversized_surface_only_reads_the_present_sub_rect() {
        let mut surface = RasterSurface::new(8, 8).expect("surface");
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 0xaa, 0xaa, 0xaa));
        let mut paint = skia_safe::Paint::new(skia_safe::Color4f::new(0.0, 1.0, 0.0, 1.0), None);
        paint.set_anti_alias(false);
        surface
            .canvas()
            .draw_rect(skia_safe::Rect::from_xywh(0.0, 0.0, 5.0, 4.0), &paint);

        let mut output = vec![0u8; 5 * 4 * 4];
        assert!(surface.copy_bgra_premultiplied_to(&mut output, 5, 4));
        assert!(
            output
                .chunks_exact(4)
                .all(|pixel| pixel == [0x00, 0xff, 0x00, 0xff]),
            "expected pure opaque green sub-rect, got {output:?}"
        );
    }

    #[test]
    fn copy_rejects_present_size_larger_than_the_allocated_surface() {
        let mut surface = RasterSurface::new(4, 4).expect("surface");
        surface.canvas().clear(skia_safe::Color::WHITE);
        let mut rgb_output = vec![0u32; 5 * 4];
        let mut bgra_output = vec![0u8; 5 * 4 * 4];

        assert!(!surface.copy_rgb_to(&mut rgb_output, 5, 4));
        assert!(!surface.copy_bgra_premultiplied_to(&mut bgra_output, 5, 4));
    }
}
