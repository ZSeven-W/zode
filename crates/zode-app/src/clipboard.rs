use std::{borrow::Cow, io::Cursor, sync::Mutex};

use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, ImageFormat, RgbaImage};
use zode_app_model::{AppCommand, AttachmentMetadata};
use zode_app_ui::ComposerController;

use crate::services::ServiceError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

pub trait ClipboardService: Send + Sync {
    fn read_text(&self) -> Result<Option<String>, ServiceError>;
    fn write_text(&self, text: &str) -> Result<(), ServiceError>;
    fn read_image(&self) -> Result<Option<ClipboardImage>, ServiceError>;
}

pub struct NativeClipboardService {
    clipboard: Mutex<arboard::Clipboard>,
}

impl NativeClipboardService {
    pub fn new() -> Result<Self, ServiceError> {
        let clipboard =
            arboard::Clipboard::new().map_err(|error| ServiceError::Platform(error.to_string()))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, arboard::Clipboard>, ServiceError> {
        self.clipboard
            .lock()
            .map_err(|_| ServiceError::Platform("clipboard lock is poisoned".into()))
    }
}

impl ClipboardService for NativeClipboardService {
    fn read_text(&self) -> Result<Option<String>, ServiceError> {
        match self.lock()?.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(ServiceError::Platform(error.to_string())),
        }
    }

    fn write_text(&self, text: &str) -> Result<(), ServiceError> {
        self.lock()?
            .set_text(text)
            .map_err(|error| ServiceError::Platform(error.to_string()))
    }

    fn read_image(&self) -> Result<Option<ClipboardImage>, ServiceError> {
        match self.lock()?.get_image() {
            Ok(image) => Ok(Some(ClipboardImage {
                width: u32::try_from(image.width)
                    .map_err(|_| ServiceError::Platform("clipboard image is too wide".into()))?,
                height: u32::try_from(image.height)
                    .map_err(|_| ServiceError::Platform("clipboard image is too tall".into()))?,
                rgba8: match image.bytes {
                    Cow::Borrowed(bytes) => bytes.to_vec(),
                    Cow::Owned(bytes) => bytes,
                },
            })),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(error) => Err(ServiceError::Platform(error.to_string())),
        }
    }
}

pub fn execute_clipboard_command(
    command: &AppCommand,
    clipboard: &dyn ClipboardService,
) -> Result<bool, ServiceError> {
    let AppCommand::CopyText(text) = command else {
        return Ok(false);
    };
    clipboard.write_text(text)?;
    Ok(true)
}

pub fn paste_from_clipboard(
    clipboard: &dyn ClipboardService,
    composer: &mut ComposerController,
) -> Result<usize, ServiceError> {
    let text = clipboard.read_text()?.filter(|text| !text.is_empty());
    let image = clipboard
        .read_image()?
        .map(|image| {
            let encoded = encode_png(&image)?;
            Ok::<_, ServiceError>((image.width, image.height, encoded))
        })
        .transpose()?;
    let mut pasted = 0;
    if let Some(text) = text {
        composer.paste_text(&text);
        pasted += 1;
    }
    if let Some((width, height, encoded)) = image {
        composer.paste_image_with_metadata(
            "image/png",
            encoded.data_base64,
            AttachmentMetadata {
                id: String::new(),
                path: None,
                display_name: "clipboard.png".into(),
                media_type: "image/png".into(),
                width: Some(width),
                height: Some(height),
                byte_len: encoded.byte_len,
            },
        );
        pasted += 1;
    }
    Ok(pasted)
}

struct EncodedImage {
    data_base64: String,
    byte_len: u64,
}

fn encode_png(image: &ClipboardImage) -> Result<EncodedImage, ServiceError> {
    if image.width == 0 || image.height == 0 {
        return Err(ServiceError::Platform(
            "clipboard image dimensions must be positive".into(),
        ));
    }
    let pixels = RgbaImage::from_raw(image.width, image.height, image.rgba8.clone())
        .ok_or_else(|| ServiceError::Platform("clipboard RGBA byte length is invalid".into()))?;
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| ServiceError::Platform(error.to_string()))?;
    let bytes = output.into_inner();
    Ok(EncodedImage {
        byte_len: bytes.len() as u64,
        data_base64: STANDARD.encode(bytes),
    })
}
