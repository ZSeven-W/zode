use std::fmt;
use std::path::{Path, PathBuf};

use image::RgbaImage;
use zode_app::render::render_offscreen_with_fonts;
use zode_app_model::ZodeAppState;

const MAX_CHANGED_FRACTION: f64 = 0.03;

#[derive(Debug, Clone, Copy)]
pub struct SnapshotCase {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl SnapshotCase {
    pub const fn new(name: &'static str, width: u32, height: u32, scale: f32) -> Self {
        Self {
            name,
            width,
            height,
            scale,
        }
    }

    fn physical_dimensions(self) -> (u32, u32) {
        (
            (self.width as f64 * f64::from(self.scale)).round() as u32,
            (self.height as f64 * f64::from(self.scale)).round() as u32,
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
struct PixelDiff {
    changed_pixels: u64,
    total_pixels: u64,
    changed_fraction: f64,
    max_channel_delta: u8,
}

impl fmt::Debug for PixelDiff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PixelDiff")
            .field("changed_pixels", &self.changed_pixels)
            .field("total_pixels", &self.total_pixels)
            .field("changed_fraction", &self.changed_fraction)
            .field("max_channel_delta", &self.max_channel_delta)
            .finish()
    }
}

pub fn assert_platform_snapshot(case: SnapshotCase, state: &ZodeAppState) {
    let actual = render_offscreen_with_fonts(state, case.width, case.height, case.scale, fonts())
        .unwrap_or_else(|error| panic!("{} failed to render: {error}", case.name));
    let actual_image = decode_png(&actual, case, "actual");
    let path = platform_golden_path(case.name);

    if update_requested() {
        let parent = path.parent().expect("snapshot path has a parent");
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
        std::fs::write(&path, &actual)
            .unwrap_or_else(|error| panic!("could not update {}: {error}", path.display()));
    }

    let expected = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "missing platform golden {}: {error}\n{}",
            path.display(),
            update_instructions()
        )
    });
    let expected_image = decode_png(&expected, case, "golden");
    let diff = compare_pixels(&expected_image, &actual_image);
    assert!(
        diff.changed_fraction <= MAX_CHANGED_FRACTION,
        "{} differs from {}: {diff:?}; allowed changed_fraction <= {MAX_CHANGED_FRACTION:.2}. \
         Review the rendered image before updating.\n{}",
        case.name,
        path.display(),
        update_instructions(),
    );
}

fn decode_png(bytes: &[u8], case: SnapshotCase, kind: &str) -> RgbaImage {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .unwrap_or_else(|error| panic!("{} {kind} is not a decodable PNG: {error}", case.name))
        .to_rgba8();
    let expected = case.physical_dimensions();
    assert_eq!(
        image.dimensions(),
        expected,
        "{} {kind} dimensions do not match logical size and scale",
        case.name,
    );
    image
}

fn compare_pixels(expected: &RgbaImage, actual: &RgbaImage) -> PixelDiff {
    assert_eq!(
        expected.dimensions(),
        actual.dimensions(),
        "pixel comparison requires equal dimensions"
    );
    let mut changed_pixels = 0_u64;
    let mut max_channel_delta = 0_u8;
    for (expected, actual) in expected.pixels().zip(actual.pixels()) {
        let mut pixel_changed = false;
        for (expected, actual) in expected.0.into_iter().zip(actual.0) {
            let delta = expected.abs_diff(actual);
            max_channel_delta = max_channel_delta.max(delta);
            pixel_changed |= delta > 0;
        }
        changed_pixels += u64::from(pixel_changed);
    }
    let total_pixels = u64::from(expected.width()) * u64::from(expected.height());
    PixelDiff {
        changed_pixels,
        total_pixels,
        changed_fraction: changed_pixels as f64 / total_pixels as f64,
        max_channel_delta,
    }
}

fn fonts() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../fonts/NotoSansSC-Regular.subset.ttf").to_vec(),
        include_bytes!("../fonts/NotoSansSC-SemiBold.subset.ttf").to_vec(),
    ]
}

fn update_requested() -> bool {
    std::env::var_os("ZODE_UPDATE_SNAPSHOTS").is_some_and(|value| value == "1")
}

fn platform_golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(platform_name())
        .join(format!("{name}.png"))
}

fn update_instructions() -> &'static str {
    "Set ZODE_UPDATE_SNAPSHOTS=1 and run `cargo +1.94 test -p zode-app --test snapshots`. \
     Only commit a platform golden after inspecting it for layout, Zode branding, and the \
     absence of Codex assets. For non-local platforms, run the workflow_dispatch \
     update-snapshots job on that OS and review its artifact."
}

#[cfg(target_os = "macos")]
const fn platform_name() -> &'static str {
    "macos"
}

#[cfg(target_os = "windows")]
const fn platform_name() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
const fn platform_name() -> &'static str {
    "linux"
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("zode-app screenshot goldens are only defined for macOS, Windows, and Linux");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_diff_counts_actual_changed_pixels() {
        let expected = RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
        let mut actual = expected.clone();
        actual.put_pixel(1, 0, image::Rgba([10, 21, 30, 255]));

        assert_eq!(
            compare_pixels(&expected, &actual),
            PixelDiff {
                changed_pixels: 1,
                total_pixels: 2,
                changed_fraction: 0.5,
                max_channel_delta: 1,
            }
        );
    }
}
