//! Screen capture via ScreenCaptureKit (`SCScreenshotManager`), the doc's
//! required replacement for the deprecated `CGWindowListCreateImage` path.
//! `captureImageInRect:completionHandler:` is display-agnostic (it takes a
//! rect in global logical points), so we simply pass the main display's
//! full bounds. The captured `CGImage` is encoded to JPEG (via ImageIO)
//! *inside* the completion handler, before it returns — this sidesteps any
//! question about whether the raw `CGImage`/`NSError` pointers, or a
//! `CFRetained` wrapping them, are safe to move across the callback
//! boundary: only the resulting `Vec<u8>` (definitely `Send`) crosses the
//! channel back to the calling task.

use block2::RcBlock;
use objc2_core_foundation::{CFString, CGRect};
use objc2_core_graphics::{CGDisplayBounds, CGImage, CGMainDisplayID};
use objc2_foundation::NSError;
use objc2_image_io::CGImageDestination;
use objc2_screen_capture_kit::SCScreenshotManager;

use super::super::backend::ComputerError;

/// Encode `image` to JPEG bytes via ImageIO. Runs synchronously inside the
/// ScreenCaptureKit completion handler.
fn encode_jpeg(image: &CGImage) -> Result<Vec<u8>, ComputerError> {
    let data = objc2_core_foundation::CFMutableData::new(None, 0)
        .ok_or_else(|| ComputerError::Protocol("allocate JPEG buffer failed".into()))?;
    let jpeg_type = CFString::from_str("public.jpeg");
    let dest = unsafe { CGImageDestination::with_data(&data, &jpeg_type, 1, None) }
        .ok_or_else(|| ComputerError::Protocol("create image destination failed".into()))?;
    unsafe { dest.add_image(image, None) };
    if !unsafe { dest.finalize() } {
        return Err(ComputerError::Protocol("JPEG encode failed".into()));
    }
    Ok(data.to_vec())
}

/// Capture the main display and return JPEG-encoded bytes.
pub(super) async fn capture_main_display() -> Result<Vec<u8>, ComputerError> {
    let rect: CGRect = CGDisplayBounds(CGMainDisplayID());
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
    let tx = std::sync::Mutex::new(Some(tx));

    {
        // Scoped so `block` (a `!Send` `RcBlock`) drops before the `.await`
        // below — ScreenCaptureKit copies/retains its own reference to the
        // block for the async operation's lifetime (the standard Cocoa
        // completion-handler contract), so the caller doesn't need to keep
        // one alive, and the enclosing future must not hold a `!Send` value
        // across an await point (required for `ComputerBackend`'s `Send`
        // future bound).
        let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            let result = if !image.is_null() {
                // SAFETY: `image` is non-null and valid for the duration of
                // this callback per SCScreenshotManager's documented contract.
                let image = unsafe { &*image };
                encode_jpeg(image).map_err(|e| e.to_string())
            } else if !error.is_null() {
                let error = unsafe { &*error };
                Err(format!("ScreenCaptureKit error: {error}"))
            } else {
                Err("ScreenCaptureKit returned neither an image nor an error".into())
            };
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(result);
            }
        });

        unsafe {
            SCScreenshotManager::captureImageInRect_completionHandler(rect, Some(&block));
        }
    }

    match rx.await {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(msg)) => Err(ComputerError::Protocol(msg)),
        Err(_) => Err(ComputerError::Protocol(
            "ScreenCaptureKit completion handler dropped without a result".into(),
        )),
    }
}
