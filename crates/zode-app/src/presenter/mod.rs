use std::sync::Arc;

use winit::window::Window;

use crate::render::RasterSurface;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;
mod softbuffer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PresentationFrame {
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,
    pub sidebar_width: f32,
    pub native_sidebar_material: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalLayerGeometry {
    pub width: f64,
    pub height: f64,
    pub sidebar_width: f64,
}

#[cfg(target_os = "macos")]
impl PresentationFrame {
    pub(crate) fn logical_geometry(self) -> LogicalLayerGeometry {
        let scale = if self.scale_factor.is_finite() && self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        };
        let width = f64::from(self.physical_width) / scale;
        let height = f64::from(self.physical_height) / scale;
        let sidebar_width = f64::from(self.sidebar_width).clamp(0.0, width);
        LogicalLayerGeometry {
            width,
            height,
            sidebar_width,
        }
    }
}

pub(crate) enum LivePresenter {
    #[cfg(target_os = "macos")]
    MacMaterial(macos::MacMaterialPresenter),
    Softbuffer(softbuffer::SoftbufferPresenter),
}

impl LivePresenter {
    pub(crate) fn new(window: Arc<Window>) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let native = macos::MacMaterialPresenter::new(window.clone()).map(Self::MacMaterial);
            fallback_on_error(
                native,
                || softbuffer::SoftbufferPresenter::new(window).map(Self::Softbuffer),
                |error| {
                    eprintln!(
                        "zode-app: native macOS material presenter is unavailable; using softbuffer: {error}"
                    );
                },
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            softbuffer::SoftbufferPresenter::new(window).map(Self::Softbuffer)
        }
    }

    pub(crate) const fn supports_native_sidebar_material(&self) -> bool {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacMaterial(_) => true,
            Self::Softbuffer(_) => false,
        }
    }

    pub(crate) fn present(
        &mut self,
        raster: &mut RasterSurface,
        frame: PresentationFrame,
    ) -> Result<(), String> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacMaterial(presenter) => presenter.present(raster, frame),
            Self::Softbuffer(presenter) => presenter.present(raster, frame),
        }
    }
}

#[cfg(target_os = "macos")]
fn fallback_on_error<T, E>(
    primary: Result<T, E>,
    fallback: impl FnOnce() -> Result<T, E>,
    on_error: impl FnOnce(&E),
) -> Result<T, E> {
    match primary {
        Ok(value) => Ok(value),
        Err(error) => {
            on_error(&error);
            fallback()
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::{fallback_on_error, PresentationFrame};

    #[cfg(target_os = "macos")]
    #[test]
    fn geometry_uses_logical_points_and_clamps_sidebar() {
        let frame = PresentationFrame {
            physical_width: 1800,
            physical_height: 1080,
            scale_factor: 2.0,
            sidebar_width: 960.0,
            native_sidebar_material: true,
        };

        let geometry = frame.logical_geometry();
        assert_eq!(geometry.width, 900.0);
        assert_eq!(geometry.height, 540.0);
        assert_eq!(geometry.sidebar_width, 900.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn failed_primary_initialization_uses_fallback() {
        let mut fallback_called = false;
        let mut error_seen = false;

        let result = fallback_on_error::<u8, &str>(
            Err("native init failed"),
            || {
                fallback_called = true;
                Ok(7)
            },
            |_| error_seen = true,
        );

        assert_eq!(result, Ok(7));
        assert!(fallback_called);
        assert!(error_seen);
    }
}
