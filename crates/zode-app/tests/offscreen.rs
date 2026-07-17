use image::GenericImageView;
use zode_app::render::{render_offscreen, render_offscreen_with_fonts};
use zode_app_model::{SystemTheme, ThemePreference};

const SNAPSHOT_REGULAR: &[u8] = include_bytes!("fonts/NotoSansSC-Regular.subset.ttf");
const SNAPSHOT_SEMIBOLD: &[u8] = include_bytes!("fonts/NotoSansSC-SemiBold.subset.ttf");

#[test]
fn offscreen_shell_is_reference_size_and_non_empty() {
    let png = render_offscreen(&zode_app_model::demo_state(), 1221, 992, 1.0).unwrap();
    let image = image::load_from_memory(&png).unwrap();

    assert_eq!((image.width(), image.height()), (1221, 992));
    assert!(png.len() > 10_000, "shell PNG is suspiciously empty");
    assert_ne!(image.get_pixel(20, 400), image.get_pixel(500, 400));
    assert_ne!(image.get_pixel(1070, 940), image.get_pixel(500, 400));
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
