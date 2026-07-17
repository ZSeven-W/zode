use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{Arc, Mutex},
};

use objc2::{define_class, msg_send, rc::Retained, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindowOrderingMode,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_foundation::NSPoint;
use objc2_quartz_core::{kCAGravityResize, CALayer, CATransaction};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

use crate::render::RasterSurface;

use super::PresentationFrame;

const PIXEL_BUFFER_POOL_CAPACITY: usize = 3;

#[derive(Default)]
struct PixelBufferPool {
    buffers: Mutex<Vec<Box<[u8]>>>,
}

impl PixelBufferPool {
    fn take(&self, byte_len: usize) -> Box<[u8]> {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(index) = buffers.iter().position(|buffer| buffer.len() == byte_len) {
            return buffers.swap_remove(index);
        }
        if buffers.len() == PIXEL_BUFFER_POOL_CAPACITY {
            buffers.pop();
        }
        drop(buffers);
        vec![0; byte_len].into_boxed_slice()
    }

    fn recycle(&self, pixels: Box<[u8]>) {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if buffers.len() < PIXEL_BUFFER_POOL_CAPACITY {
            buffers.push(pixels);
        }
    }

    #[cfg(test)]
    fn retained_count(&self) -> usize {
        self.buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

struct ProviderBuffer {
    pool: Arc<PixelBufferPool>,
    pixels: Box<[u8]>,
}

impl ProviderBuffer {
    fn recycle(self) {
        let Self { pool, pixels } = self;
        pool.recycle(pixels);
    }
}

unsafe extern "C-unwind" fn release_pixel_buffer(
    info: *mut c_void,
    _data: NonNull<c_void>,
    _size: usize,
) {
    let Some(info) = NonNull::new(info.cast::<ProviderBuffer>()) else {
        return;
    };
    // SAFETY: `info` is the unique ProviderBuffer allocation transferred to
    // Core Graphics by `transfer_buffer_to_provider`.
    let owner = unsafe { Box::from_raw(info.as_ptr()) };
    owner.recycle();
}

define_class!(
    // SAFETY:
    // - NSVisualEffectView supports subclassing without extra invariants.
    // - `PassthroughVisualEffectView` has no ivars and does not implement Drop.
    #[unsafe(super(NSVisualEffectView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZodePassthroughVisualEffectView"]
    struct PassthroughVisualEffectView;

    impl PassthroughVisualEffectView {
        // SAFETY: The signature matches NSView's `hitTest:` method exactly.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl PassthroughVisualEffectView {
    fn new(mtm: MainThreadMarker, frame: CGRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: The signature matches NSVisualEffectView's designated frame initializer.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

pub(crate) struct MacMaterialPresenter {
    window: Arc<Window>,
    effect_view: Retained<NSVisualEffectView>,
    foreground_layer: Retained<CALayer>,
    color_space: CFRetained<CGColorSpace>,
    pixel_pool: Arc<PixelBufferPool>,
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
        let effect_view = PassthroughVisualEffectView::new(mtm, zero_frame).into_super();
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
            pixel_pool: Arc::new(PixelBufferPool::default()),
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
        let mut pixels = self.pixel_pool.take(pixel_bytes);
        if !raster.copy_bgra_premultiplied_to(
            pixels.as_mut(),
            frame.physical_width,
            frame.physical_height,
        ) {
            self.pixel_pool.recycle(pixels);
            return Err("Skia BGRA framebuffer copy failed".into());
        }
        let image = make_image(
            pixels,
            frame.physical_width as usize,
            frame.physical_height as usize,
            &self.color_space,
            Arc::clone(&self.pixel_pool),
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
    pixels: Box<[u8]>,
    width: usize,
    height: usize,
    color_space: &CGColorSpace,
    pool: Arc<PixelBufferPool>,
) -> Result<CFRetained<CGImage>, String> {
    let provider = transfer_buffer_to_provider(pixels, pool, |info, data, byte_len| {
        // SAFETY: The provider receives the exact allocation described by the
        // ProviderBuffer owner and returns it through `release_pixel_buffer`.
        unsafe { CGDataProvider::with_data(info, data, byte_len, Some(release_pixel_buffer)) }
    })?;
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

fn transfer_buffer_to_provider<T>(
    pixels: Box<[u8]>,
    pool: Arc<PixelBufferPool>,
    create: impl FnOnce(*mut c_void, *mut c_void, usize) -> Option<T>,
) -> Result<T, String> {
    let mut owner = Box::new(ProviderBuffer { pool, pixels });
    let data = owner.pixels.as_mut_ptr().cast::<c_void>();
    let byte_len = owner.pixels.len();
    let info = Box::into_raw(owner).cast::<c_void>();
    let Some(provider) = create(info, data, byte_len) else {
        // SAFETY: Provider creation failed, so Core Graphics did not take the
        // unique ProviderBuffer owner and its release callback cannot run.
        let owner = unsafe { Box::from_raw(info.cast::<ProviderBuffer>()) };
        owner.recycle();
        return Err("Core Graphics data provider creation failed".to_owned());
    };
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::{
        release_pixel_buffer, transfer_buffer_to_provider, PixelBufferPool,
        PIXEL_BUFFER_POOL_CAPACITY,
    };
    use crate::render::RasterSurface;
    use objc2_core_graphics::CGDataProvider;
    use std::sync::Arc;

    #[test]
    fn pool_reuses_matching_pixel_buffer() {
        let pool = PixelBufferPool::default();
        let pixels = pool.take(64);
        let pointer = pixels.as_ptr();
        pool.recycle(pixels);

        let reused = pool.take(64);

        assert_eq!(reused.as_ptr(), pointer);
        assert_eq!(reused.len(), 64);
    }

    #[test]
    fn pool_keeps_non_matching_pixel_buffer_for_its_original_size() {
        let pool = PixelBufferPool::default();
        let small = pool.take(64);
        let small_pointer = small.as_ptr();
        pool.recycle(small);

        let large = pool.take(128);

        assert_ne!(large.as_ptr(), small_pointer);
        assert_eq!(large.len(), 128);
        assert_eq!(pool.retained_count(), 1);
        let small_again = pool.take(64);
        assert_eq!(small_again.as_ptr(), small_pointer);
    }

    #[test]
    fn pool_retains_at_most_three_frames() {
        let pool = PixelBufferPool::default();
        let buffers = (0..=PIXEL_BUFFER_POOL_CAPACITY)
            .map(|index| pool.take(64 + index))
            .collect::<Vec<_>>();

        for buffer in buffers {
            pool.recycle(buffer);
        }

        assert_eq!(pool.retained_count(), PIXEL_BUFFER_POOL_CAPACITY);
    }

    #[test]
    fn bgra_copy_overwrites_every_byte_in_reused_buffer() {
        let pool = PixelBufferPool::default();
        let mut stale = pool.take(8);
        stale.fill(0xa5);
        let pointer = stale.as_ptr();
        pool.recycle(stale);
        let mut reused = pool.take(8);
        assert_eq!(reused.as_ptr(), pointer);
        let mut surface = RasterSurface::new(2, 1).expect("surface");
        surface
            .canvas()
            .clear(skia_safe::Color::from_argb(255, 0x12, 0x34, 0x56));

        assert!(surface.copy_bgra_premultiplied_to(reused.as_mut(), 2, 1));
        assert_eq!(
            reused.as_ref(),
            [0x56, 0x34, 0x12, 0xff, 0x56, 0x34, 0x12, 0xff]
        );
    }

    #[test]
    fn provider_release_callback_returns_buffer_to_pool() {
        let pool = Arc::new(PixelBufferPool::default());
        let pixels = pool.take(64);
        let pointer = pixels.as_ptr();
        let provider =
            transfer_buffer_to_provider(pixels, Arc::clone(&pool), |info, data, byte_len| {
                // SAFETY: The test transfers the ProviderBuffer owner to Core
                // Graphics and uses the production release callback.
                unsafe {
                    CGDataProvider::with_data(info, data, byte_len, Some(release_pixel_buffer))
                }
            })
            .expect("provider");
        assert_eq!(pool.retained_count(), 0);

        drop(provider);

        let recovered = pool.take(64);
        assert_eq!(recovered.as_ptr(), pointer);
    }

    #[test]
    fn provider_creation_failure_returns_buffer_to_pool() {
        let pool = Arc::new(PixelBufferPool::default());
        let pixels = pool.take(64);
        let pointer = pixels.as_ptr();

        let result = transfer_buffer_to_provider::<()>(
            pixels,
            Arc::clone(&pool),
            |_info, _data, _byte_len| None,
        );

        assert_eq!(
            result.unwrap_err(),
            "Core Graphics data provider creation failed"
        );
        let recovered = pool.take(64);
        assert_eq!(recovered.as_ptr(), pointer);
    }
}
