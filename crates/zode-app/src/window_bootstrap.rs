use winit::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    window::{Icon, Window, WindowAttributes},
};

use crate::window_state::{
    WindowGeometry, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT,
    MIN_WINDOW_WIDTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupWindowPlacement {
    Default,
    Positioned(WindowGeometry),
    Unpositioned(WindowGeometry),
}

impl StartupWindowPlacement {
    pub fn maximized(self) -> bool {
        match self {
            Self::Default => false,
            Self::Positioned(geometry) | Self::Unpositioned(geometry) => geometry.maximized,
        }
    }
}

/// Builds an initially hidden window so platform accessibility can attach first.
///
/// Persisted geometry is expressed in native physical pixels. A first launch has
/// no persisted geometry and deliberately uses logical units so the default
/// window has the same apparent size on both 1x and HiDPI displays.
pub fn hidden_window_attributes(geometry: Option<WindowGeometry>) -> WindowAttributes {
    let placement = geometry.map_or(
        StartupWindowPlacement::Default,
        StartupWindowPlacement::Positioned,
    );
    hidden_window_attributes_for_placement(placement)
}

pub fn hidden_window_attributes_for_placement(
    placement: StartupWindowPlacement,
) -> WindowAttributes {
    let mut attributes = Window::default_attributes()
        .with_title("Zode")
        .with_window_icon(zode_window_icon())
        .with_min_inner_size(LogicalSize::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT))
        .with_visible(false);
    attributes = match placement {
        StartupWindowPlacement::Default => attributes.with_inner_size(LogicalSize::new(
            f64::from(DEFAULT_WINDOW_WIDTH),
            f64::from(DEFAULT_WINDOW_HEIGHT),
        )),
        StartupWindowPlacement::Positioned(geometry) => attributes
            .with_position(PhysicalPosition::new(geometry.x, geometry.y))
            .with_inner_size(PhysicalSize::new(
                geometry.width.max(1),
                geometry.height.max(1),
            )),
        StartupWindowPlacement::Unpositioned(geometry) => attributes.with_inner_size(
            PhysicalSize::new(geometry.width.max(1), geometry.height.max(1)),
        ),
    };
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes = attributes
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_traffic_light_inset(4.0);
    }
    #[cfg(not(target_os = "macos"))]
    {
        attributes = attributes.with_decorations(false);
    }
    attributes
}

fn zode_window_icon() -> Option<Icon> {
    let image = image::load_from_memory_with_format(
        include_bytes!("../../../assets/brand/zode-512.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::hidden_window_attributes;

    #[test]
    fn native_window_uses_the_zode_brand_icon() {
        let attributes = hidden_window_attributes(None);
        assert!(attributes.window_icon.is_some());
    }
}
