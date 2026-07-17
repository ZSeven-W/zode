//! Window capture: CGWindowListCreateImage → RGBA bitmap → PNG bytes. Requires
//! Screen Recording permission at runtime; without it the captured image is
//! blank rather than an error (documented M1 limitation).

#![cfg(target_os = "macos")]

use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    create_image, kCGWindowImageBoundsIgnoreFraming, kCGWindowListOptionIncludingWindow,
};

use crate::desktop::backend::DesktopError;

/// CGRectNull — tells `create_image` to use the window's own bounds.
fn null_rect() -> CGRect {
    CGRect::new(
        &CGPoint::new(f64::INFINITY, f64::INFINITY),
        &CGSize::new(0.0, 0.0),
    )
}

/// Capture the given CGWindowID and encode it as PNG bytes.
pub fn capture_window_png(window_id: u32) -> Result<Vec<u8>, DesktopError> {
    let image = create_image(
        null_rect(),
        kCGWindowListOptionIncludingWindow,
        window_id,
        kCGWindowImageBoundsIgnoreFraming,
    )
    .ok_or_else(|| DesktopError::Protocol("CGWindowListCreateImage returned null".into()))?;

    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(DesktopError::Protocol("captured image is empty".into()));
    }

    // Draw into a known RGBA8888 bitmap so we control the byte layout.
    let cs = CGColorSpace::create_device_rgb();
    let bytes_per_row = width * 4;
    let mut ctx = CGContext::create_bitmap_context(
        None,
        width,
        height,
        8,
        bytes_per_row,
        &cs,
        core_graphics::base::kCGImageAlphaPremultipliedLast,
    );
    ctx.draw_image(
        CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        ),
        &image,
    );

    let stride = ctx.bytes_per_row();
    let data = ctx.data();
    // Repack to tight RGBA rows (bitmap context may pad each row).
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = row * stride;
        rgba.extend_from_slice(&data[start..start + width * 4]);
    }

    encode_png(width as u32, height as u32, &rgba)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, DesktopError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| DesktopError::Protocol(format!("png header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| DesktopError::Protocol(format!("png data: {e}")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_valid_png_bytes() {
        // 2x2 opaque RGBA → a real PNG the attachment layer can sniff.
        let rgba = vec![255u8; 2 * 2 * 4];
        let png = encode_png(2, 2, &rgba).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert!(png.len() > 20);
    }
}
