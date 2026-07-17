use jian_widgets::{ImageDrawMode, Painter, Point2D, Rect};

use crate::{ThemeMode, ZodeTheme};

const DARK_LOGO: &[u8] = include_bytes!("../../../assets/logo.png");
const LIGHT_LOGO: &[u8] = include_bytes!("../../../assets/logo-light.png");
const DARK_LOGO_ID: u64 = 0x248d_4a5b_143e_0780;
const LIGHT_LOGO_ID: u64 = 0x69f3_60d5_c3e4_1b23;

pub(crate) struct BrandMark;

impl BrandMark {
    pub(crate) const SIZE: f32 = 48.0;

    pub(crate) fn paint(painter: &mut dyn Painter, center: Point2D, theme: &ZodeTheme) {
        let (image_id, encoded) = match theme.mode() {
            ThemeMode::Light => (LIGHT_LOGO_ID, LIGHT_LOGO),
            ThemeMode::Dark => (DARK_LOGO_ID, DARK_LOGO),
        };
        let rect = Rect::xywh(
            center.x - Self::SIZE / 2.0,
            center.y - Self::SIZE / 2.0,
            Self::SIZE,
            Self::SIZE,
        );
        painter.draw_image_with_mode(rect, image_id, encoded, ImageDrawMode::Fit);
    }
}
