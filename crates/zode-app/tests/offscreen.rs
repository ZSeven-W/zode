use image::GenericImageView;
use zode_app::render::{render_offscreen, render_offscreen_with_fonts};

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
fn offscreen_rejects_invalid_dimensions_and_accepts_optional_fonts() {
    assert!(render_offscreen(&zode_app_model::demo_state(), 0, 200, 1.0).is_err());
    assert!(render_offscreen(&zode_app_model::demo_state(), 200, 200, 0.0).is_err());
    let png = render_offscreen_with_fonts(&zode_app_model::demo_state(), 320, 240, 1.0, Vec::new())
        .unwrap();
    assert!(image::load_from_memory(&png).is_ok());
}
