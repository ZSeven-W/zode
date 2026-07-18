//! Renders the ghost-cursor bitmap (orange arrow + glow) into CGImages, one
//! per pre-rotated heading step, so the run loop only swaps layer contents.

#![cfg(target_os = "macos")]

use core_graphics::base::kCGImageAlphaPremultipliedLast;
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::image::CGImage;

/// Logical cursor canvas (points). Hotspot is the canvas center.
pub const CANVAS: f64 = 56.0;
/// Number of pre-rotated frames (2π / 16 ≈ 22.5° steps).
pub const ROTATIONS: usize = 16;

/// Render all rotation frames at `scale` (2.0 for retina).
pub fn cursor_frames(scale: f64) -> Vec<CGImage> {
    (0..ROTATIONS)
        .map(|i| render(scale, (i as f64) * std::f64::consts::TAU / ROTATIONS as f64))
        .collect()
}

/// Frame index whose rotation is nearest to `heading` (radians).
pub fn frame_for(heading: f64) -> usize {
    let tau = std::f64::consts::TAU;
    let h = ((heading % tau) + tau) % tau;
    ((h / tau * ROTATIONS as f64).round() as usize) % ROTATIONS
}

fn render(scale: f64, angle: f64) -> CGImage {
    let px = (CANVAS * scale) as usize;
    let cs = CGColorSpace::create_device_rgb();
    let ctx =
        CGContext::create_bitmap_context(None, px, px, 8, 0, &cs, kCGImageAlphaPremultipliedLast);
    ctx.scale(scale, scale);
    let c = CANVAS / 2.0;

    // Glow: three concentric circles fading out (approximates the bloom).
    for (r, a) in [(22.0, 0.18), (15.0, 0.22), (9.0, 0.28)] {
        ctx.set_rgb_fill_color(1.0, 0.47, 0.09, a);
        ctx.fill_ellipse_in_rect(CGRect::new(
            &CGPoint::new(c - r, c - r),
            &CGSize::new(r * 2.0, r * 2.0),
        ));
    }

    // Arrow polygon from pi-computer-use: (14,0) (-8,-9) (-3,0) (-8,9),
    // rotated by `angle` around the hotspot.
    let pts = [(14.0f64, 0.0f64), (-8.0, -9.0), (-3.0, 0.0), (-8.0, 9.0)];
    let (sin, cos) = angle.sin_cos();
    let rot = |p: (f64, f64)| (c + p.0 * cos - p.1 * sin, c + p.0 * sin + p.1 * cos);

    let p0 = rot(pts[0]);
    ctx.begin_path();
    ctx.move_to_point(p0.0, p0.1);
    for p in &pts[1..] {
        let q = rot(*p);
        ctx.add_line_to_point(q.0, q.1);
    }
    ctx.close_path();
    ctx.set_rgb_fill_color(1.0, 0.47, 0.09, 1.0);
    ctx.fill_path();

    // White outline for contrast on dark backgrounds.
    ctx.begin_path();
    ctx.move_to_point(p0.0, p0.1);
    for p in &pts[1..] {
        let q = rot(*p);
        ctx.add_line_to_point(q.0, q.1);
    }
    ctx.close_path();
    ctx.set_rgb_stroke_color(1.0, 1.0, 1.0, 1.0);
    ctx.set_line_width(2.0);
    ctx.stroke_path();

    ctx.create_image().expect("cursor bitmap")
}
