use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use serde_json::json;
use zode_app::render::render_offscreen_with_fonts;
use zode_app_model::ZodeAppState;
use zode_app_ui::{Insets, WorkspaceSnapshot};

use super::geometry::{assert_snapshot_geometry, GeometryExpectation};

const MAX_CHANGED_FRACTION: f64 = 0.03;

#[derive(Debug, Clone, Copy)]
pub struct SnapshotCase {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    geometry: &'static [GeometryExpectation],
}

impl SnapshotCase {
    pub const fn new(
        name: &'static str,
        width: u32,
        height: u32,
        scale: f32,
        geometry: &'static [GeometryExpectation],
    ) -> Self {
        Self {
            name,
            width,
            height,
            scale,
            geometry,
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
pub struct PixelDiff {
    pub changed_pixels: u64,
    pub total_pixels: u64,
    pub changed_fraction: f64,
    pub max_channel_delta: u8,
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

pub fn render_snapshot(
    state: &ZodeAppState,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<u8>, String> {
    render_offscreen_with_fonts(state, width, height, scale, fonts())
        .map_err(|error| error.to_string())
}

pub fn assert_platform_snapshot(case: SnapshotCase, state: &ZodeAppState) {
    assert_safe_scene_name(case.name);
    assert_case_geometry(case, state);
    let actual = render_snapshot(state, case.width, case.height, case.scale)
        .unwrap_or_else(|error| panic!("{} failed to render: {error}", case.name));
    let actual_image = decode_png(&actual, case.name, "actual");
    assert_eq!(
        actual_image.dimensions(),
        case.physical_dimensions(),
        "{} actual dimensions do not match logical size and scale",
        case.name,
    );

    reconcile_snapshot(
        case,
        &actual,
        &actual_image,
        &platform_golden_path(case.name),
        &snapshot_artifact_root(),
        update_requested(),
    );
}

pub fn assert_case_geometry(case: SnapshotCase, state: &ZodeAppState) {
    let snapshot =
        WorkspaceSnapshot::build(state, case.width as f32, case.height as f32, Insets::ZERO);
    assert_snapshot_geometry(case.name, &snapshot, case.scale, case.geometry);
}

fn reconcile_snapshot(
    case: SnapshotCase,
    actual_bytes: &[u8],
    actual: &RgbaImage,
    golden_path: &Path,
    artifact_root: &Path,
    update: bool,
) {
    let expected_bytes = match std::fs::read(golden_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let placeholder =
                RgbaImage::from_pixel(actual.width(), actual.height(), Rgba([0, 0, 0, 0]));
            let diff = compare_pixels(&placeholder, actual);
            persist_diagnostics(
                &artifact_root.join(platform_name()).join(case.name),
                &placeholder,
                actual,
                diff,
                DiagnosticMetadata {
                    scene_name: case.name,
                    platform: platform_name(),
                    status: "missing_expected",
                    expected_source: Some(golden_path),
                    actual_source: None,
                },
            );
            if update {
                write_golden_after_diagnostics(golden_path, actual_bytes);
                return;
            }
            panic!(
                "missing platform golden {}: {error}\ndiagnostics: {}\n{}",
                golden_path.display(),
                artifact_root
                    .join(platform_name())
                    .join(case.name)
                    .display(),
                update_instructions(),
            );
        }
        Err(error) => panic!("could not read {}: {error}", golden_path.display()),
    };
    let expected = decode_png(&expected_bytes, case.name, "golden");
    if expected.dimensions() != actual.dimensions() {
        let normalized = normalize_to_dimensions(&expected, actual.dimensions());
        let diff = compare_pixels(&normalized, actual);
        persist_diagnostics(
            &artifact_root.join(platform_name()).join(case.name),
            &normalized,
            actual,
            diff,
            DiagnosticMetadata {
                scene_name: case.name,
                platform: platform_name(),
                status: "dimension_mismatch",
                expected_source: Some(golden_path),
                actual_source: None,
            },
        );
        if update {
            write_golden_after_diagnostics(golden_path, actual_bytes);
            return;
        }
        panic!(
            "{} golden dimensions {:?} differ from actual {:?}; diagnostics: {}\n{}",
            case.name,
            expected.dimensions(),
            actual.dimensions(),
            artifact_root
                .join(platform_name())
                .join(case.name)
                .display(),
            update_instructions(),
        );
    }

    let diff = compare_pixels(&expected, actual);
    if diff.changed_pixels > 0 {
        persist_diagnostics(
            &artifact_root.join(platform_name()).join(case.name),
            &expected,
            actual,
            diff,
            DiagnosticMetadata {
                scene_name: case.name,
                platform: platform_name(),
                status: "pixel_mismatch",
                expected_source: Some(golden_path),
                actual_source: None,
            },
        );
    }
    if update && diff.changed_pixels > 0 {
        // The old expected image and every visual diagnostic have already been
        // persisted above. Bless is deliberately the final operation.
        write_golden_after_diagnostics(golden_path, actual_bytes);
        return;
    }
    assert!(
        diff.changed_fraction <= MAX_CHANGED_FRACTION,
        "{} differs from {}: {diff:?}; allowed changed_fraction <= {MAX_CHANGED_FRACTION:.2}. \
         diagnostics: {}. Review the complete rendered image before updating.\n{}",
        case.name,
        golden_path.display(),
        artifact_root
            .join(platform_name())
            .join(case.name)
            .display(),
        update_instructions(),
    );
}

fn write_golden_after_diagnostics(path: &Path, bytes: &[u8]) {
    let parent = path.parent().expect("snapshot path has a parent");
    std::fs::create_dir_all(parent)
        .unwrap_or_else(|error| panic!("could not create {}: {error}", parent.display()));
    std::fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("could not update {}: {error}", path.display()));
}

/// Writes a deterministic 50% overlay and a channel-difference heatmap for a
/// user-approved reference and one generated scene. A one-pixel capture edge
/// is removed by top-left cropping the reference to the generated dimensions.
pub fn compare_reference_images(
    scene_name: &str,
    reference_path: &Path,
    actual_path: &Path,
    output_root: &Path,
) -> Result<PixelDiff, String> {
    assert_safe_scene_name(scene_name);
    let reference_bytes = std::fs::read(reference_path)
        .map_err(|error| format!("could not read {}: {error}", reference_path.display()))?;
    let actual_bytes = std::fs::read(actual_path)
        .map_err(|error| format!("could not read {}: {error}", actual_path.display()))?;
    let reference = decode_png_result(&reference_bytes, scene_name, "reference")?;
    let actual = decode_png_result(&actual_bytes, scene_name, "actual")?;
    if reference.width() < actual.width() || reference.height() < actual.height() {
        return Err(format!(
            "{} reference {:?} is smaller than actual {:?}",
            scene_name,
            reference.dimensions(),
            actual.dimensions(),
        ));
    }
    let normalized_reference = reference
        .view(0, 0, actual.width(), actual.height())
        .to_image();
    let diff = compare_pixels(&normalized_reference, &actual);
    persist_diagnostics(
        &output_root.join(platform_name()).join(scene_name),
        &normalized_reference,
        &actual,
        diff,
        DiagnosticMetadata {
            scene_name,
            platform: platform_name(),
            status: "reference_comparison",
            expected_source: Some(reference_path),
            actual_source: Some(actual_path),
        },
    );
    Ok(diff)
}

#[derive(Debug, Clone, Copy)]
struct DiagnosticMetadata<'a> {
    scene_name: &'a str,
    platform: &'a str,
    status: &'a str,
    expected_source: Option<&'a Path>,
    actual_source: Option<&'a Path>,
}

fn persist_diagnostics(
    directory: &Path,
    expected: &RgbaImage,
    actual: &RgbaImage,
    diff: PixelDiff,
    metadata: DiagnosticMetadata<'_>,
) {
    std::fs::create_dir_all(directory).unwrap_or_else(|error| {
        panic!(
            "could not create snapshot diagnostics {}: {error}",
            directory.display()
        )
    });
    let expected_png = encode_png(expected);
    let actual_png = encode_png(actual);
    let overlay_png = encode_png(&overlay_image(expected, actual));
    let heatmap_png = encode_png(&heatmap_image(expected, actual));
    std::fs::write(directory.join("expected.png"), expected_png)
        .unwrap_or_else(|error| panic!("could not write expected diagnostic: {error}"));
    std::fs::write(directory.join("actual.png"), actual_png)
        .unwrap_or_else(|error| panic!("could not write actual diagnostic: {error}"));
    std::fs::write(directory.join("overlay.png"), overlay_png)
        .unwrap_or_else(|error| panic!("could not write overlay diagnostic: {error}"));
    std::fs::write(directory.join("heatmap.png"), heatmap_png)
        .unwrap_or_else(|error| panic!("could not write heatmap diagnostic: {error}"));
    let report = json!({
        "scene": metadata.scene_name,
        "platform": metadata.platform,
        "status": metadata.status,
        "dimensions": {
            "width": actual.width(),
            "height": actual.height(),
        },
        "threshold": {
            "maxChangedFraction": MAX_CHANGED_FRACTION,
        },
        "diff": {
            "changedPixels": diff.changed_pixels,
            "totalPixels": diff.total_pixels,
            "changedFraction": diff.changed_fraction,
            "maxChannelDelta": diff.max_channel_delta,
        },
        "sources": {
            "expected": metadata.expected_source.map(|path| path.display().to_string()),
            "actual": metadata.actual_source.map(|path| path.display().to_string()),
        },
        "artifacts": ["expected.png", "actual.png", "overlay.png", "heatmap.png"],
    });
    std::fs::write(
        directory.join("diff.json"),
        serde_json::to_vec_pretty(&report).expect("snapshot diff report serializes"),
    )
    .unwrap_or_else(|error| panic!("could not write diff diagnostic: {error}"));
}

fn decode_png(bytes: &[u8], scene_name: &str, kind: &str) -> RgbaImage {
    decode_png_result(bytes, scene_name, kind).unwrap_or_else(|error| panic!("{error}"))
}

fn decode_png_result(bytes: &[u8], scene_name: &str, kind: &str) -> Result<RgbaImage, String> {
    image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|error| format!("{scene_name} {kind} is not a decodable PNG: {error}"))
        .map(|image| image.to_rgba8())
}

fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("diagnostic image encodes as PNG");
    cursor.into_inner()
}

fn normalize_to_dimensions(image: &RgbaImage, dimensions: (u32, u32)) -> RgbaImage {
    let mut normalized = RgbaImage::from_pixel(dimensions.0, dimensions.1, Rgba([0, 0, 0, 0]));
    let width = image.width().min(dimensions.0);
    let height = image.height().min(dimensions.1);
    for y in 0..height {
        for x in 0..width {
            normalized.put_pixel(x, y, *image.get_pixel(x, y));
        }
    }
    normalized
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
        changed_fraction: if total_pixels == 0 {
            0.0
        } else {
            changed_pixels as f64 / total_pixels as f64
        },
        max_channel_delta,
    }
}

fn overlay_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    assert_eq!(expected.dimensions(), actual.dimensions());
    let mut image = RgbaImage::new(expected.width(), expected.height());
    for (pixel, (expected, actual)) in image
        .pixels_mut()
        .zip(expected.pixels().zip(actual.pixels()))
    {
        pixel.0 = std::array::from_fn(|channel| {
            ((u16::from(expected.0[channel]) + u16::from(actual.0[channel])) / 2) as u8
        });
    }
    image
}

fn heatmap_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    assert_eq!(expected.dimensions(), actual.dimensions());
    let mut image = RgbaImage::new(expected.width(), expected.height());
    for (pixel, (expected, actual)) in image
        .pixels_mut()
        .zip(expected.pixels().zip(actual.pixels()))
    {
        let delta = expected
            .0
            .into_iter()
            .zip(actual.0)
            .map(|(expected, actual)| expected.abs_diff(actual))
            .max()
            .unwrap_or(0);
        *pixel = if delta == 0 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255_u8.saturating_sub(delta), 0, 255])
        };
    }
    image
}

fn fonts() -> Vec<Vec<u8>> {
    vec![
        include_bytes!("../fonts/NotoSansSC-Regular.subset.ttf").to_vec(),
        include_bytes!("../fonts/NotoSansSC-SemiBold.subset.ttf").to_vec(),
    ]
}

fn update_requested() -> bool {
    ["ZODE_UPDATE_SNAPSHOTS", "GOLDEN_BLESS"]
        .into_iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| value == "1"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("zode-app lives below the workspace root")
        .to_path_buf()
}

fn snapshot_artifact_root() -> PathBuf {
    workspace_root()
        .join("target")
        .join("zode-app-snapshot-artifacts")
}

fn platform_golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(platform_name())
        .join(format!("{name}.png"))
}

fn update_instructions() -> &'static str {
    "Set GOLDEN_BLESS=1 (or ZODE_UPDATE_SNAPSHOTS=1) and run the exact snapshot test. \
     The old expected plus actual/overlay/heatmap/diff.json are written before the golden is \
     replaced. Only commit a platform golden after inspecting the complete canvas. For non-local \
     platforms, run the workflow_dispatch update-snapshots job on that OS and review its artifact."
}

fn assert_safe_scene_name(name: &str) {
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
        "snapshot scene names must contain only lowercase ASCII, digits, and hyphens"
    );
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
        let expected = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
        let mut actual = expected.clone();
        actual.put_pixel(1, 0, Rgba([10, 21, 30, 255]));

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

    #[test]
    fn bless_preserves_old_expected_and_diagnostics_before_replacement() {
        static GEOMETRY: &[GeometryExpectation] = &[];
        let case = SnapshotCase::new("ordering-proof", 2, 1, 1.0, GEOMETRY);
        let expected = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
        let actual = RgbaImage::from_pixel(2, 1, Rgba([90, 80, 70, 255]));
        let expected_bytes = encode_png(&expected);
        let actual_bytes = encode_png(&actual);
        let root = std::env::temp_dir().join(format!(
            "zode-snapshot-ordering-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos(),
        ));
        let golden = root.join("golden.png");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&root).expect("temporary snapshot directory is created");
        std::fs::write(&golden, &expected_bytes).expect("old golden is written");

        reconcile_snapshot(case, &actual_bytes, &actual, &golden, &artifacts, true);

        assert_eq!(
            decode_png(
                &std::fs::read(
                    artifacts
                        .join(platform_name())
                        .join(case.name)
                        .join("expected.png")
                )
                .expect("expected diagnostic exists"),
                case.name,
                "expected diagnostic",
            ),
            expected,
        );
        assert_eq!(
            decode_png(
                &std::fs::read(
                    artifacts
                        .join(platform_name())
                        .join(case.name)
                        .join("actual.png")
                )
                .expect("actual diagnostic exists"),
                case.name,
                "actual diagnostic",
            ),
            actual,
        );
        for name in ["overlay.png", "heatmap.png", "diff.json"] {
            assert!(
                artifacts
                    .join(platform_name())
                    .join(case.name)
                    .join(name)
                    .is_file(),
                "{name} is preserved",
            );
        }
        assert_eq!(std::fs::read(&golden).expect("golden exists"), actual_bytes);
        std::fs::remove_dir_all(root).expect("temporary snapshot directory is removed");
    }

    #[test]
    fn reference_comparison_uses_platform_then_scene_artifact_layout() {
        let reference = RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255]));
        let actual = RgbaImage::from_pixel(2, 1, Rgba([11, 20, 30, 255]));
        let root = std::env::temp_dir().join(format!(
            "zode-reference-layout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos(),
        ));
        let reference_path = root.join("reference.png");
        let actual_path = root.join("actual.png");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&root).expect("temporary comparison directory is created");
        std::fs::write(&reference_path, encode_png(&reference)).expect("reference is written");
        std::fs::write(&actual_path, encode_png(&actual)).expect("actual is written");

        let diff =
            compare_reference_images("layout-proof", &reference_path, &actual_path, &artifacts)
                .expect("reference comparison succeeds");

        assert_eq!(diff.changed_pixels, 2);
        let directory = artifacts.join(platform_name()).join("layout-proof");
        for name in [
            "expected.png",
            "actual.png",
            "overlay.png",
            "heatmap.png",
            "diff.json",
        ] {
            assert!(directory.join(name).is_file(), "{name} exists");
        }
        assert!(!artifacts.join("layout-proof").exists());
        std::fs::remove_dir_all(root).expect("temporary comparison directory is removed");
    }
}
