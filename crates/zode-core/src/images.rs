use std::path::{Path, PathBuf};

use agent::attachments::{image_from_bytes, AttachmentError};
use agent::message::{ContentBlock, ImageSource};

use crate::CoreError;

#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub path: PathBuf,
    pub display_name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub content_block: ContentBlock,
}

#[derive(Debug, Clone, Default)]
pub struct PastedImages {
    pub images: Vec<ImageAttachment>,
    pub remaining_text: String,
}

pub fn load_image_attachment(cwd: &Path, raw_path: &str) -> Result<ImageAttachment, CoreError> {
    let raw = raw_path.trim();
    if raw.is_empty() {
        return Err(CoreError::Other("image path is empty".into()));
    }
    let path = expand_image_path(cwd, raw);
    let bytes = std::fs::read(&path)
        .map_err(|_| CoreError::Other(format!("image not found: {}", path.display())))?;
    let size_bytes = bytes.len() as u64;
    let content_block = image_from_bytes(&bytes).map_err(image_error)?;
    let media_type = match &content_block {
        ContentBlock::Image {
            source: ImageSource::Base64 { media_type, .. },
        } => media_type.clone(),
        _ => {
            return Err(CoreError::Other(
                "unsupported image: expected png/jpeg/gif/webp".into(),
            ))
        }
    };
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("attached image")
        .to_string();

    Ok(ImageAttachment {
        path,
        display_name,
        media_type,
        size_bytes,
        content_block,
    })
}

pub fn split_pasted_image_paths(cwd: &Path, pasted: &str) -> Result<PastedImages, CoreError> {
    let tokens = pasted_tokens(pasted);
    let mut images = Vec::new();
    let mut remove_spans = Vec::new();

    for token in tokens {
        let normalized = normalize_pasted_path(&token.text);
        if !looks_like_image_path(&normalized) {
            continue;
        }
        match load_image_attachment(cwd, &normalized) {
            Ok(image) => {
                images.push(image);
                remove_spans.push((token.start, token.end));
            }
            Err(err) if path_is_explicit(&normalized) => return Err(err),
            Err(_) => {}
        }
    }

    let remaining_text = if remove_spans.is_empty() {
        pasted.to_string()
    } else {
        remove_spans.sort_by_key(|(start, _)| *start);
        let mut out = String::new();
        let mut cursor = 0;
        for (start, end) in remove_spans {
            out.push_str(&pasted[cursor..start]);
            cursor = end;
        }
        out.push_str(&pasted[cursor..]);
        out.trim().to_string()
    };

    Ok(PastedImages {
        images,
        remaining_text,
    })
}

fn expand_image_path(cwd: &Path, raw_path: &str) -> PathBuf {
    let normalized = normalize_pasted_path(raw_path);
    if let Some(rest) = normalized.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let path = PathBuf::from(normalized);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn image_error(err: AttachmentError) -> CoreError {
    match err {
        AttachmentError::UnknownImageFormat => {
            CoreError::Other("unsupported image: expected png/jpeg/gif/webp".into())
        }
        AttachmentError::TooLarge { size_bytes, .. } => CoreError::Other(format!(
            "image too large for inline upload: {size_bytes} bytes; Files API upload is not enabled"
        )),
    }
}

fn looks_like_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    matches!(
        Path::new(&lower).extension().and_then(|ext| ext.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp")
    )
}

fn path_is_explicit(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with("~/")
        || path.starts_with("file://")
        || path.contains(std::path::MAIN_SEPARATOR)
}

fn normalize_pasted_path(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(['\'', '"']).to_string();
    if let Some(rest) = s.strip_prefix("file://") {
        s = percent_decode_file_url(rest);
    }
    unescape_terminal_path(&s)
}

fn unescape_terminal_path(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn percent_decode_file_url(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug)]
struct PasteToken {
    text: String,
    start: usize,
    end: usize,
}

fn pasted_tokens(input: &str) -> Vec<PasteToken> {
    let mut out = Vec::new();
    let mut iter = input.char_indices().peekable();
    while let Some((start, ch)) = iter.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            let content_start = start + ch.len_utf8();
            let mut end = input.len();
            for (idx, next) in iter.by_ref() {
                if next == quote {
                    end = idx + next.len_utf8();
                    break;
                }
            }
            let content_end = end.saturating_sub(quote.len_utf8());
            out.push(PasteToken {
                text: input[content_start..content_end].to_string(),
                start,
                end,
            });
            continue;
        }

        let mut end = input.len();
        while let Some(&(idx, next)) = iter.peek() {
            if next.is_whitespace() {
                end = idx;
                break;
            }
            iter.next();
        }
        out.push(PasteToken {
            text: input[start..end].to_string(),
            start,
            end,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::message::{ContentBlock, ImageSource};

    fn write_png(path: &std::path::Path) {
        std::fs::write(
            path,
            [
                0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x00,
            ],
        )
        .unwrap();
    }

    #[test]
    fn loads_png_attachment_from_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        write_png(&path);

        let image = load_image_attachment(dir.path(), "shot.png").unwrap();

        assert_eq!(image.display_name, "shot.png");
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.size_bytes, 12);
        assert_eq!(image.path, path);
        match image.content_block {
            ContentBlock::Image {
                source: ImageSource::Base64 { media_type, data },
            } => {
                assert_eq!(media_type, "image/png");
                assert!(!data.is_empty());
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }

    #[test]
    fn pasted_image_path_becomes_attachment_and_removes_path_from_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("screen shot.png");
        write_png(&path);
        let pasted = format!("看这个 '{}'", path.display());

        let result = split_pasted_image_paths(dir.path(), &pasted).unwrap();

        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].display_name, "screen shot.png");
        assert_eq!(result.remaining_text.trim(), "看这个");
    }

    #[test]
    fn file_url_paste_becomes_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        write_png(&path);
        let pasted = format!("file://{}", path.display());

        let result = split_pasted_image_paths(dir.path(), &pasted).unwrap();

        assert_eq!(result.images.len(), 1);
        assert!(result.remaining_text.trim().is_empty());
    }

    #[test]
    fn non_image_paste_stays_as_text() {
        let dir = tempfile::tempdir().unwrap();

        let result = split_pasted_image_paths(dir.path(), "hello world").unwrap();

        assert!(result.images.is_empty());
        assert_eq!(result.remaining_text, "hello world");
    }
}
