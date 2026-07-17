use std::{num::NonZeroU32, sync::Arc};

use winit::window::Window;

use crate::render::RasterSurface;

use super::PresentationFrame;

pub(crate) struct SoftbufferPresenter {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    size: Option<(u32, u32)>,
}

impl SoftbufferPresenter {
    pub(super) fn new(window: Arc<Window>) -> Result<Self, String> {
        let context =
            softbuffer::Context::new(window.clone()).map_err(|error| error.to_string())?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            window,
            surface,
            size: None,
        })
    }

    pub(super) fn present(
        &mut self,
        raster: &mut RasterSurface,
        frame: PresentationFrame,
    ) -> Result<(), String> {
        let size = (frame.physical_width.max(1), frame.physical_height.max(1));
        if self.size != Some(size) {
            self.surface
                .resize(
                    NonZeroU32::new(size.0).expect("positive width"),
                    NonZeroU32::new(size.1).expect("positive height"),
                )
                .map_err(|error| error.to_string())?;
            self.size = Some(size);
        }
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|error| error.to_string())?;
        if !raster.copy_rgb_to(&mut buffer, frame.physical_width, frame.physical_height) {
            return Err("Skia framebuffer copy failed".into());
        }
        self.window.pre_present_notify();
        buffer.present().map_err(|error| error.to_string())
    }
}
