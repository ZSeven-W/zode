use std::collections::BTreeSet;
use std::path::Path;

use image::GenericImageView;
use zode_app::render::{render_offscreen, render_offscreen_with_fonts};
use zode_app_model::{SystemTheme, ThemePreference};

const SNAPSHOT_REGULAR: &[u8] = include_bytes!("fonts/NotoSansSC-Regular.subset.ttf");
const SNAPSHOT_SEMIBOLD: &[u8] = include_bytes!("fonts/NotoSansSC-SemiBold.subset.ttf");
const SNAPSHOT_GLYPHS: &str = include_str!("fonts/glyphs.txt");
const SNAPSHOT_FAMILY: &str = "Zode Snapshot Sans SC";

#[test]
fn offscreen_shell_is_reference_size_and_non_empty() {
    let png = render_offscreen(&zode_app_model::demo_state(), 1221, 992, 1.0).unwrap();
    let image = image::load_from_memory(&png).unwrap();

    assert_eq!((image.width(), image.height()), (1221, 992));
    assert!(png.len() > 10_000, "shell PNG is suspiciously empty");
    let main_background = image.get_pixel(500, 400);
    assert_ne!(image.get_pixel(20, 400), main_background);
    assert!(
        (800..image.height()).any(|y| {
            (300..image.width())
                .step_by(4)
                .any(|x| image.get_pixel(x, y) != main_background)
        }),
        "bottom interaction region is missing from the shell render"
    );
}

#[test]
fn offscreen_scale_changes_physical_pixels_not_logical_layout() {
    let png = render_offscreen(&zode_app_model::demo_state(), 300, 200, 2.0).unwrap();
    let image = image::load_from_memory(&png).unwrap();

    assert_eq!((image.width(), image.height()), (600, 400));
}

#[test]
fn offscreen_rejects_invalid_dimensions() {
    assert!(render_offscreen(&zode_app_model::demo_state(), 0, 200, 1.0).is_err());
    assert!(render_offscreen(&zode_app_model::demo_state(), 200, 200, 0.0).is_err());
}

#[test]
fn offscreen_accepts_empty_and_invalid_font_inputs() {
    let png = render_offscreen_with_fonts(&zode_app_model::demo_state(), 320, 240, 1.0, Vec::new())
        .unwrap();
    assert!(image::load_from_memory(&png).is_ok());

    let png = render_offscreen_with_fonts(
        &zode_app_model::demo_state(),
        320,
        240,
        1.0,
        vec![b"not a font".to_vec(), Vec::new()],
    )
    .unwrap();
    assert!(image::load_from_memory(&png).is_ok());
}

#[test]
fn offscreen_accepts_the_snapshot_font_family() {
    let png = render_offscreen_with_fonts(
        &zode_app_model::demo_state(),
        320,
        240,
        1.0,
        vec![SNAPSHOT_REGULAR.to_vec(), SNAPSHOT_SEMIBOLD.to_vec()],
    )
    .unwrap();

    assert!(image::load_from_memory(&png).is_ok());
}

#[test]
fn snapshot_font_assets_use_a_repository_private_family() {
    let regular = jian_skia::parse_imported_font_meta(SNAPSHOT_REGULAR).unwrap();
    let semibold = jian_skia::parse_imported_font_meta(SNAPSHOT_SEMIBOLD).unwrap();

    assert_eq!(regular.family, SNAPSHOT_FAMILY);
    assert_eq!(semibold.family, SNAPSHOT_FAMILY);
    assert_eq!(regular.weight, 400);
    assert_eq!(semibold.weight, 600);
}

#[test]
fn snapshot_font_manifest_covers_visible_app_sources() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir.parent().unwrap();
    let mut required = BTreeSet::new();
    for root in [
        manifest_dir.join("src"),
        manifest_dir.join("tests/snapshot_support"),
        crates_dir.join("zode-app-model/src"),
        crates_dir.join("zode-app-ui/src"),
    ] {
        collect_non_ascii_rust_chars(&root, &mut required);
    }

    let manifest = SNAPSHOT_GLYPHS.chars().collect::<BTreeSet<_>>();
    let missing = required.difference(&manifest).collect::<String>();
    assert!(
        missing.is_empty(),
        "snapshot font manifest is missing visible source characters: {missing}"
    );
}

#[test]
fn snapshot_font_assets_cover_the_checked_in_manifest() {
    let manager = skia_safe::FontMgr::new();
    for (name, bytes) in [
        ("regular", SNAPSHOT_REGULAR),
        ("semibold", SNAPSHOT_SEMIBOLD),
    ] {
        let typeface = manager
            .new_from_data(bytes, None)
            .unwrap_or_else(|| panic!("{name} snapshot font must parse"));
        let missing = SNAPSHOT_GLYPHS
            .chars()
            .filter(|character| !character.is_whitespace())
            .filter(|character| typeface.unichar_to_glyph(*character as i32) == 0)
            .collect::<String>();
        assert!(
            missing.is_empty(),
            "{name} snapshot font is missing manifest glyphs: {missing}"
        );
    }
}

fn collect_non_ascii_rust_chars(path: &Path, output: &mut BTreeSet<char>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            collect_non_ascii_rust_chars(&entry.unwrap().path(), output);
        }
        return;
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return;
    }
    for character in std::fs::read_to_string(path).unwrap().chars() {
        if !character.is_ascii() && !character.is_whitespace() && character != '\u{fffd}' {
            output.insert(character);
        }
    }
}

#[test]
fn offscreen_render_honors_explicit_theme_over_the_observed_system_theme() {
    let mut state = zode_app_model::demo_state();
    state.host.system_theme = SystemTheme::Light;
    state.ui_preferences.theme = ThemePreference::Dark;

    let png = render_offscreen(&state, 1221, 992, 1.0).unwrap();
    let image = image::load_from_memory(&png).unwrap();
    let pixel = image.get_pixel(500, 400);

    assert!(pixel.0[0] < 64 && pixel.0[1] < 64 && pixel.0[2] < 64);
    assert_eq!(state.host.system_theme, SystemTheme::Light);
}
