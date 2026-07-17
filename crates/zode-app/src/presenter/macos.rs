use std::{
    ffi::c_void,
    ptr::{self, slice_from_raw_parts_mut, NonNull},
    sync::Arc,
};

use objc2::{rc::Retained, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindowOrderingMode,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_quartz_core::{kCAGravityResize, CALayer, CATransaction};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

use crate::render::RasterSurface;

use super::PresentationFrame;

pub(crate) struct MacMaterialPresenter {
    window: Arc<Window>,
    effect_view: Retained<NSVisualEffectView>,
    foreground_layer: Retained<CALayer>,
    color_space: CFRetained<CGColorSpace>,
}

impl MacMaterialPresenter {
    pub(super) fn new(window: Arc<Window>) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "AppKit presenter must initialize on the main thread".to_owned())?;
        let RawWindowHandle::AppKit(handle) = window
            .window_handle()
            .map_err(|error| error.to_string())?
            .as_raw()
        else {
            return Err("window does not expose an AppKit NSView".into());
        };
        // SAFETY: The raw handle is valid while `window` is retained by this presenter.
        let content_view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| "AppKit content view is null".to_owned())?;
        content_view.setWantsLayer(true);
        let root_layer = content_view
            .layer()
            .ok_or_else(|| "AppKit content view could not become layer-backed".to_owned())?;
        root_layer.setOpaque(false);

        let zero_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 0.0));
        let effect_view =
            NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), zero_frame);
        effect_view.setMaterial(NSVisualEffectMaterial::Sidebar);
        effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect_view.setState(NSVisualEffectState::FollowsWindowActiveState);
        effect_view.setHidden(true);

        let foreground_layer = CALayer::new();
        foreground_layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        foreground_layer.setGeometryFlipped(true);
        foreground_layer.setOpaque(false);
        foreground_layer.setZPosition(1.0);
        foreground_layer.setContentsGravity(unsafe { kCAGravityResize });

        let color_space = CGColorSpace::new_device_rgb()
            .ok_or_else(|| "Core Graphics RGB color space is unavailable".to_owned())?;

        let effect_as_view = effect_view.as_super();
        content_view.addSubview_positioned_relativeTo(
            effect_as_view,
            NSWindowOrderingMode::Below,
            None,
        );
        root_layer.addSublayer(&foreground_layer);

        Ok(Self {
            window,
            effect_view,
            foreground_layer,
            color_space,
        })
    }

    pub(super) fn present(
        &mut self,
        raster: &mut RasterSurface,
        frame: PresentationFrame,
    ) -> Result<(), String> {
        let geometry = frame.logical_geometry();
        let viewport = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(geometry.width, geometry.height),
        );
        let sidebar = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(geometry.sidebar_width, geometry.height),
        );

        let pixel_bytes = usize::try_from(frame.physical_width)
            .ok()
            .and_then(|width| {
                usize::try_from(frame.physical_height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "macOS presentation dimensions are too large".to_owned())?;
        let mut pixels = vec![0; pixel_bytes];
        if !raster.copy_bgra_premultiplied_to(&mut pixels) {
            return Err("Skia BGRA framebuffer copy failed".into());
        }
        let image = make_image(
            pixels,
            frame.physical_width as usize,
            frame.physical_height as usize,
            &self.color_space,
        )?;

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.effect_view.setFrame(sidebar);
        self.effect_view
            .setHidden(!frame.native_sidebar_material || geometry.sidebar_width <= 0.0);
        self.foreground_layer.setFrame(viewport);
        self.foreground_layer.setContentsScale(frame.scale_factor);
        // SAFETY: CALayer accepts a CGImage as its contents object.
        unsafe { self.foreground_layer.setContents(Some(image.as_ref())) };
        CATransaction::commit();

        self.window.pre_present_notify();
        Ok(())
    }
}

impl Drop for MacMaterialPresenter {
    fn drop(&mut self) {
        self.foreground_layer.removeFromSuperlayer();
        self.effect_view.as_super().removeFromSuperview();
    }
}

fn make_image(
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    color_space: &CGColorSpace,
) -> Result<CFRetained<CGImage>, String> {
    unsafe extern "C-unwind" fn release(_info: *mut c_void, data: NonNull<c_void>, size: usize) {
        let slice = slice_from_raw_parts_mut(data.cast::<u8>().as_ptr(), size);
        // SAFETY: This reconstructs the exact boxed slice transferred below.
        drop(unsafe { Box::from_raw(slice) });
    }

    let byte_len = pixels.len();
    let boxed: *mut [u8] = Box::into_raw(pixels.into_boxed_slice());
    let data = boxed.cast::<c_void>();
    // SAFETY: The provider owns the boxed pixel allocation until `release` runs.
    // SAFETY: The pointer and byte length describe `boxed` exactly.
    let provider =
        unsafe { CGDataProvider::with_data(ptr::null_mut(), data, byte_len, Some(release)) };
    let Some(provider) = provider else {
        // SAFETY: Ownership was not transferred because provider creation failed.
        drop(unsafe { Box::from_raw(boxed) });
        return Err("Core Graphics data provider creation failed".to_owned());
    };
    let bitmap_info = CGBitmapInfo(
        CGImageAlphaInfo::PremultipliedFirst.0
            | CGImageComponentInfo::Integer.0
            | CGImageByteOrderInfo::Order32Little.0
            | CGImagePixelFormatInfo::Packed.0,
    );
    // SAFETY: The provider contains `width * height` premultiplied BGRA pixels.
    unsafe {
        CGImage::new(
            width,
            height,
            8,
            32,
            width * 4,
            Some(color_space),
            bitmap_info,
            Some(&provider),
            ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| "Core Graphics image creation failed".to_owned())
}
