use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_TURN_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_FILES_PER_TURN: usize = 8;
pub const MAX_RAW_CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_IN_FLIGHT_PER_CONNECTION: usize = 2;
pub const MAX_PENDING_FILES_PER_CONNECTION: usize = 16;
pub const MAX_PENDING_BYTES_PER_CONNECTION: usize = 40 * 1024 * 1024;
pub const UPLOAD_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Utf8Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginUpload {
    pub file_name: String,
    pub media_type: String,
    pub declared_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadTicket {
    pub upload_id: String,
    pub file_name: String,
    pub media_type: String,
    pub declared_size: usize,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkAck {
    pub next_sequence: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FinishedPayload {
    Utf8Text(String),
    Image(Vec<u8>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct FinishedUpload {
    pub attachment_id: String,
    pub upload_id: String,
    pub task_id: String,
    pub file_name: String,
    pub media_type: String,
    pub size: usize,
    pub payload: FinishedPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedReceipt {
    pub attachment_id: String,
    pub upload_id: String,
    pub task_id: String,
    pub file_name: String,
    pub media_type: String,
    pub size: usize,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedTurnAttachment {
    TextBlock {
        text: String,
    },
    Image {
        display_name: String,
        media_type: String,
        bytes: Vec<u8>,
    },
}

impl FinishedUpload {
    /// Convert a consumed upload into the narrow intermediate representation
    /// used by the turn integration. Image bytes can be handed directly to
    /// `zode_core::images::image_attachment_from_bytes`; text is wrapped in a
    /// server-generated boundary whose displayed file name has already been
    /// stripped of path and control characters.
    #[cfg(test)]
    pub fn into_prepared(self) -> PreparedTurnAttachment {
        match self.payload {
            FinishedPayload::Utf8Text(contents) => {
                let boundary = format!("ZODE-ATTACHMENT-{}", self.attachment_id);
                let file_name = escape_attribute(&self.file_name);
                let media_type = escape_attribute(&self.media_type);
                PreparedTurnAttachment::TextBlock {
                    text: format!(
                        "<attached_file name=\"{file_name}\" media_type=\"{media_type}\" boundary=\"{boundary}\">\n--- BEGIN {boundary} ---\n{contents}\n--- END {boundary} ---\n</attached_file>"
                    ),
                }
            }
            FinishedPayload::Image(bytes) => PreparedTurnAttachment::Image {
                display_name: self.file_name,
                media_type: self.media_type,
                bytes,
            },
        }
    }

    /// Prepare without consuming the stored upload. The registry uses this
    /// during its transactional prepare phase, then removes the whole batch
    /// only after every fallible conversion succeeds.
    pub fn to_prepared(&self) -> PreparedTurnAttachment {
        match &self.payload {
            FinishedPayload::Utf8Text(contents) => {
                let boundary = format!("ZODE-ATTACHMENT-{}", self.attachment_id);
                let file_name = escape_attribute(&self.file_name);
                let media_type = escape_attribute(&self.media_type);
                PreparedTurnAttachment::TextBlock {
                    text: format!(
                        "<attached_file name=\"{file_name}\" media_type=\"{media_type}\" boundary=\"{boundary}\">\n--- BEGIN {boundary} ---\n{contents}\n--- END {boundary} ---\n</attached_file>"
                    ),
                }
            }
            FinishedPayload::Image(bytes) => PreparedTurnAttachment::Image {
                display_name: self.file_name.clone(),
                media_type: self.media_type.clone(),
                bytes: bytes.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    UnsupportedMediaType {
        media_type: String,
    },
    UnsupportedFileType {
        file_name: String,
    },
    FileTooLarge {
        declared: usize,
        limit: usize,
    },
    TooManyFiles {
        limit: usize,
    },
    TurnTooLarge {
        declared_total: usize,
        limit: usize,
    },
    TooManyInFlight {
        limit: usize,
    },
    TooManyPendingFiles {
        limit: usize,
    },
    PendingBytesExceeded {
        declared_total: usize,
        limit: usize,
    },
    UploadNotFound,
    AttachmentNotFound,
    WrongConnection,
    WrongTask,
    UnexpectedSequence {
        expected: u64,
        actual: u64,
    },
    EmptyChunk,
    ChunkTooLarge {
        actual: usize,
        limit: usize,
    },
    DeclaredSizeExceeded {
        declared: usize,
        actual: usize,
    },
    SizeMismatch {
        declared: usize,
        actual: usize,
    },
    InvalidUtf8,
    ForbiddenContent {
        kind: &'static str,
    },
    ImageTypeMismatch {
        declared: String,
        detected: Option<String>,
    },
    DuplicateAttachment,
}

impl fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMediaType { media_type } => {
                write!(formatter, "unsupported attachment media type: {media_type}")
            }
            Self::UnsupportedFileType { file_name } => {
                write!(formatter, "unsupported attachment file type: {file_name}")
            }
            Self::FileTooLarge { declared, limit } => {
                write!(
                    formatter,
                    "attachment is {declared} bytes; limit is {limit}"
                )
            }
            Self::TooManyFiles { limit } => {
                write!(formatter, "a turn accepts at most {limit} attachments")
            }
            Self::TurnTooLarge {
                declared_total,
                limit,
            } => write!(
                formatter,
                "turn attachments declare {declared_total} bytes; limit is {limit}"
            ),
            Self::TooManyInFlight { limit } => write!(
                formatter,
                "a connection may upload at most {limit} attachments concurrently"
            ),
            Self::TooManyPendingFiles { limit } => write!(
                formatter,
                "a connection may retain at most {limit} pending attachments"
            ),
            Self::PendingBytesExceeded {
                declared_total,
                limit,
            } => write!(
                formatter,
                "connection pending attachments declare {declared_total} bytes; limit is {limit}"
            ),
            Self::UploadNotFound => formatter.write_str("upload not found or expired"),
            Self::AttachmentNotFound => formatter.write_str("attachment not found or expired"),
            Self::WrongConnection => {
                formatter.write_str("attachment belongs to another connection")
            }
            Self::WrongTask => formatter.write_str("attachment belongs to another task"),
            Self::UnexpectedSequence { expected, actual } => write!(
                formatter,
                "unexpected chunk sequence {actual}; expected {expected}"
            ),
            Self::EmptyChunk => formatter.write_str("attachment chunk is empty"),
            Self::ChunkTooLarge { actual, limit } => {
                write!(formatter, "chunk is {actual} bytes; limit is {limit}")
            }
            Self::DeclaredSizeExceeded { declared, actual } => write!(
                formatter,
                "received {actual} bytes, exceeding declared size {declared}"
            ),
            Self::SizeMismatch { declared, actual } => write!(
                formatter,
                "received {actual} bytes, but upload declared {declared}"
            ),
            Self::InvalidUtf8 => formatter.write_str("text attachment is not valid UTF-8"),
            Self::ForbiddenContent { kind } => {
                write!(formatter, "{kind} attachments are not supported")
            }
            Self::ImageTypeMismatch { declared, detected } => write!(
                formatter,
                "image bytes do not match declared type {declared}; detected {}",
                detected.as_deref().unwrap_or("unknown")
            ),
            Self::DuplicateAttachment => {
                formatter.write_str("attachment IDs must be unique within a turn")
            }
        }
    }
}

impl std::error::Error for UploadError {}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsumeFinishedError<E> {
    Upload(UploadError),
    Prepare(E),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupStats {
    pub uploads: usize,
    pub attachments: usize,
}

#[derive(Debug)]
struct InFlightUpload {
    connection_id: u64,
    task_id: String,
    file_name: String,
    media_type: String,
    declared_size: usize,
    kind: AttachmentKind,
    next_sequence: u64,
    bytes: Vec<u8>,
    expires_at: Instant,
}

#[derive(Debug)]
struct StoredFinished {
    connection_id: u64,
    expires_at: Instant,
    upload: FinishedUpload,
}

#[derive(Debug, Default)]
pub struct AttachmentRegistry {
    uploads: HashMap<String, InFlightUpload>,
    finished: HashMap<String, StoredFinished>,
    upload_to_attachment: HashMap<String, String>,
}

impl AttachmentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(
        &mut self,
        connection_id: u64,
        task_id: impl Into<String>,
        request: BeginUpload,
        now: Instant,
    ) -> Result<UploadTicket, UploadError> {
        self.cleanup_expired(now);
        let task_id = task_id.into();
        let file_name = sanitize_file_name(&request.file_name);
        if is_forbidden_extension(&file_name) {
            return Err(UploadError::UnsupportedFileType { file_name });
        }
        let (media_type, kind) = classify_media_type(&request.media_type)?;
        let file_limit = match kind {
            AttachmentKind::Utf8Text => MAX_TEXT_BYTES,
            AttachmentKind::Image => MAX_IMAGE_BYTES,
        };
        if request.declared_size > file_limit {
            return Err(UploadError::FileTooLarge {
                declared: request.declared_size,
                limit: file_limit,
            });
        }

        let in_flight = self
            .uploads
            .values()
            .filter(|upload| upload.connection_id == connection_id)
            .count();
        if in_flight >= MAX_IN_FLIGHT_PER_CONNECTION {
            return Err(UploadError::TooManyInFlight {
                limit: MAX_IN_FLIGHT_PER_CONNECTION,
            });
        }

        let (pending_files, pending_bytes) = self.pending_for_connection(connection_id);
        if pending_files >= MAX_PENDING_FILES_PER_CONNECTION {
            return Err(UploadError::TooManyPendingFiles {
                limit: MAX_PENDING_FILES_PER_CONNECTION,
            });
        }
        let pending_total = pending_bytes.saturating_add(request.declared_size);
        if pending_total > MAX_PENDING_BYTES_PER_CONNECTION {
            return Err(UploadError::PendingBytesExceeded {
                declared_total: pending_total,
                limit: MAX_PENDING_BYTES_PER_CONNECTION,
            });
        }

        let (files, bytes) = self.reserved_for_task(connection_id, &task_id);
        if files >= MAX_FILES_PER_TURN {
            return Err(UploadError::TooManyFiles {
                limit: MAX_FILES_PER_TURN,
            });
        }
        let declared_total = bytes.saturating_add(request.declared_size);
        if declared_total > MAX_TURN_BYTES {
            return Err(UploadError::TurnTooLarge {
                declared_total,
                limit: MAX_TURN_BYTES,
            });
        }

        let upload_id = opaque_id("upload");
        let ticket = UploadTicket {
            upload_id: upload_id.clone(),
            file_name: file_name.clone(),
            media_type: media_type.clone(),
            declared_size: request.declared_size,
            kind,
        };
        self.uploads.insert(
            upload_id,
            InFlightUpload {
                connection_id,
                task_id,
                file_name,
                media_type,
                declared_size: request.declared_size,
                kind,
                next_sequence: 0,
                bytes: Vec::new(),
                expires_at: now + UPLOAD_TTL,
            },
        );
        Ok(ticket)
    }

    pub fn push_chunk(
        &mut self,
        connection_id: u64,
        upload_id: &str,
        sequence: u64,
        data: &[u8],
        now: Instant,
    ) -> Result<ChunkAck, UploadError> {
        self.cleanup_expired(now);
        let upload = self
            .uploads
            .get_mut(upload_id)
            .ok_or(UploadError::UploadNotFound)?;
        check_connection(upload.connection_id, connection_id)?;
        if sequence != upload.next_sequence {
            return Err(UploadError::UnexpectedSequence {
                expected: upload.next_sequence,
                actual: sequence,
            });
        }
        if data.is_empty() {
            return Err(UploadError::EmptyChunk);
        }
        if data.len() > MAX_RAW_CHUNK_BYTES {
            return Err(UploadError::ChunkTooLarge {
                actual: data.len(),
                limit: MAX_RAW_CHUNK_BYTES,
            });
        }
        let actual = upload.bytes.len().saturating_add(data.len());
        if actual > upload.declared_size {
            return Err(UploadError::DeclaredSizeExceeded {
                declared: upload.declared_size,
                actual,
            });
        }
        upload.bytes.extend_from_slice(data);
        upload.next_sequence += 1;
        upload.expires_at = now + UPLOAD_TTL;
        Ok(ChunkAck {
            next_sequence: upload.next_sequence,
        })
    }

    pub fn finish(
        &mut self,
        connection_id: u64,
        upload_id: &str,
        now: Instant,
    ) -> Result<FinishedReceipt, UploadError> {
        self.cleanup_expired(now);
        let upload = self
            .uploads
            .get(upload_id)
            .ok_or(UploadError::UploadNotFound)?;
        check_connection(upload.connection_id, connection_id)?;
        if upload.bytes.len() != upload.declared_size {
            return Err(UploadError::SizeMismatch {
                declared: upload.declared_size,
                actual: upload.bytes.len(),
            });
        }
        if let Some(kind) = sniff_forbidden_content(&upload.bytes) {
            return Err(UploadError::ForbiddenContent { kind });
        }
        match upload.kind {
            AttachmentKind::Utf8Text => {
                std::str::from_utf8(&upload.bytes).map_err(|_| UploadError::InvalidUtf8)?;
            }
            AttachmentKind::Image => {
                let detected = sniff_image_media_type(&upload.bytes);
                if detected != Some(upload.media_type.as_str()) {
                    return Err(UploadError::ImageTypeMismatch {
                        declared: upload.media_type.clone(),
                        detected: detected.map(str::to_owned),
                    });
                }
            }
        }

        let upload = self
            .uploads
            .remove(upload_id)
            .expect("validated upload must still exist");
        let InFlightUpload {
            task_id,
            file_name,
            media_type,
            declared_size,
            kind,
            bytes,
            ..
        } = upload;
        let payload = match kind {
            AttachmentKind::Utf8Text => FinishedPayload::Utf8Text(
                String::from_utf8(bytes).expect("UTF-8 was validated before upload removal"),
            ),
            AttachmentKind::Image => FinishedPayload::Image(bytes),
        };
        let attachment_id = opaque_id("attachment");
        let receipt = FinishedReceipt {
            attachment_id: attachment_id.clone(),
            upload_id: upload_id.to_string(),
            task_id: task_id.clone(),
            file_name: file_name.clone(),
            media_type: media_type.clone(),
            size: declared_size,
            kind,
        };
        let finished = FinishedUpload {
            attachment_id: attachment_id.clone(),
            upload_id: upload_id.to_string(),
            task_id,
            file_name,
            media_type,
            size: declared_size,
            payload,
        };
        self.upload_to_attachment
            .insert(upload_id.to_string(), attachment_id.clone());
        self.finished.insert(
            attachment_id,
            StoredFinished {
                connection_id,
                expires_at: now + UPLOAD_TTL,
                upload: finished,
            },
        );
        Ok(receipt)
    }

    /// Cancel either an in-flight upload or a finished-but-unconsumed upload.
    /// The original upload ID stays a valid cancellation handle after finish;
    /// unknown/already-cancelled IDs are deliberately idempotent.
    pub fn cancel_upload(
        &mut self,
        connection_id: u64,
        upload_or_attachment_id: &str,
    ) -> Result<bool, UploadError> {
        if let Some(upload) = self.uploads.get(upload_or_attachment_id) {
            check_connection(upload.connection_id, connection_id)?;
            self.uploads.remove(upload_or_attachment_id);
            return Ok(true);
        }

        let attachment_id = self
            .upload_to_attachment
            .get(upload_or_attachment_id)
            .map(String::as_str)
            .unwrap_or(upload_or_attachment_id)
            .to_string();
        if let Some(stored) = self.finished.get(&attachment_id) {
            check_connection(stored.connection_id, connection_id)?;
            let stored = self
                .finished
                .remove(&attachment_id)
                .expect("validated attachment must still exist");
            self.upload_to_attachment.remove(&stored.upload.upload_id);
            return Ok(true);
        }
        Ok(false)
    }

    #[cfg(test)]
    pub fn consume_finished(
        &mut self,
        connection_id: u64,
        task_id: &str,
        attachment_ids: &[String],
        now: Instant,
    ) -> Result<Vec<FinishedUpload>, UploadError> {
        self.cleanup_expired(now);
        self.validate_finished_batch(connection_id, task_id, attachment_ids)?;

        Ok(self.remove_finished_batch(attachment_ids))
    }

    /// Validate and prepare a full attachment batch before consuming any of
    /// it. This lets callers perform fallible image encoding transactionally:
    /// if one conversion fails, every attachment remains available for retry.
    pub fn consume_finished_with<T, E>(
        &mut self,
        connection_id: u64,
        task_id: &str,
        attachment_ids: &[String],
        now: Instant,
        mut prepare: impl FnMut(&FinishedUpload) -> Result<T, E>,
    ) -> Result<Vec<T>, ConsumeFinishedError<E>> {
        self.cleanup_expired(now);
        self.validate_finished_batch(connection_id, task_id, attachment_ids)
            .map_err(ConsumeFinishedError::Upload)?;

        let mut prepared = Vec::with_capacity(attachment_ids.len());
        for attachment_id in attachment_ids {
            let stored = self
                .finished
                .get(attachment_id)
                .expect("attachment batch was validated before preparation");
            prepared.push(prepare(&stored.upload).map_err(ConsumeFinishedError::Prepare)?);
        }

        let _ = self.remove_finished_batch(attachment_ids);
        Ok(prepared)
    }

    fn validate_finished_batch(
        &self,
        connection_id: u64,
        task_id: &str,
        attachment_ids: &[String],
    ) -> Result<(), UploadError> {
        if attachment_ids.len() > MAX_FILES_PER_TURN {
            return Err(UploadError::TooManyFiles {
                limit: MAX_FILES_PER_TURN,
            });
        }
        let mut unique = HashSet::with_capacity(attachment_ids.len());
        let mut declared_total = 0usize;
        for attachment_id in attachment_ids {
            if !unique.insert(attachment_id.as_str()) {
                return Err(UploadError::DuplicateAttachment);
            }
            let stored = self
                .finished
                .get(attachment_id)
                .ok_or(UploadError::AttachmentNotFound)?;
            declared_total = declared_total.saturating_add(stored.upload.size);
        }
        if declared_total > MAX_TURN_BYTES {
            return Err(UploadError::TurnTooLarge {
                declared_total,
                limit: MAX_TURN_BYTES,
            });
        }
        for attachment_id in attachment_ids {
            let stored = self
                .finished
                .get(attachment_id)
                .expect("attachment set was validated before ownership checks");
            check_owner(
                stored.connection_id,
                &stored.upload.task_id,
                connection_id,
                task_id,
            )?;
        }

        Ok(())
    }

    fn remove_finished_batch(&mut self, attachment_ids: &[String]) -> Vec<FinishedUpload> {
        let mut consumed = Vec::with_capacity(attachment_ids.len());
        for attachment_id in attachment_ids {
            let stored = self
                .finished
                .remove(attachment_id)
                .expect("attachment batch was validated atomically");
            self.upload_to_attachment.remove(&stored.upload.upload_id);
            consumed.push(stored.upload);
        }
        consumed
    }

    pub fn remove_task(&mut self, task_id: &str) -> CleanupStats {
        let uploads_before = self.uploads.len();
        self.uploads.retain(|_, upload| upload.task_id != task_id);
        let uploads = uploads_before - self.uploads.len();

        let mut removed_upload_ids = Vec::new();
        let attachments_before = self.finished.len();
        self.finished.retain(|_, stored| {
            if stored.upload.task_id == task_id {
                removed_upload_ids.push(stored.upload.upload_id.clone());
                false
            } else {
                true
            }
        });
        for upload_id in removed_upload_ids {
            self.upload_to_attachment.remove(&upload_id);
        }

        CleanupStats {
            uploads,
            attachments: attachments_before - self.finished.len(),
        }
    }

    pub fn disconnect(&mut self, connection_id: u64) -> CleanupStats {
        let uploads_before = self.uploads.len();
        self.uploads
            .retain(|_, upload| upload.connection_id != connection_id);
        let uploads = uploads_before - self.uploads.len();

        let mut removed_upload_ids = Vec::new();
        let attachments_before = self.finished.len();
        self.finished.retain(|_, stored| {
            if stored.connection_id == connection_id {
                removed_upload_ids.push(stored.upload.upload_id.clone());
                false
            } else {
                true
            }
        });
        for upload_id in removed_upload_ids {
            self.upload_to_attachment.remove(&upload_id);
        }
        CleanupStats {
            uploads,
            attachments: attachments_before - self.finished.len(),
        }
    }

    pub fn cleanup_expired(&mut self, now: Instant) -> CleanupStats {
        let uploads_before = self.uploads.len();
        self.uploads.retain(|_, upload| now < upload.expires_at);
        let uploads = uploads_before - self.uploads.len();

        let mut removed_upload_ids = Vec::new();
        let attachments_before = self.finished.len();
        self.finished.retain(|_, stored| {
            if now >= stored.expires_at {
                removed_upload_ids.push(stored.upload.upload_id.clone());
                false
            } else {
                true
            }
        });
        for upload_id in removed_upload_ids {
            self.upload_to_attachment.remove(&upload_id);
        }
        CleanupStats {
            uploads,
            attachments: attachments_before - self.finished.len(),
        }
    }

    pub fn reserved_for_task(&self, connection_id: u64, task_id: &str) -> (usize, usize) {
        let upload_reservations = self
            .uploads
            .values()
            .filter(|upload| upload.connection_id == connection_id && upload.task_id == task_id);
        let finished_reservations = self.finished.values().filter(|stored| {
            stored.connection_id == connection_id && stored.upload.task_id == task_id
        });
        let mut files = 0usize;
        let mut bytes = 0usize;
        for size in upload_reservations
            .map(|upload| upload.declared_size)
            .chain(finished_reservations.map(|stored| stored.upload.size))
        {
            files += 1;
            bytes = bytes.saturating_add(size);
        }
        (files, bytes)
    }

    pub fn pending_for_connection(&self, connection_id: u64) -> (usize, usize) {
        let upload_reservations = self
            .uploads
            .values()
            .filter(|upload| upload.connection_id == connection_id);
        let finished_reservations = self
            .finished
            .values()
            .filter(|stored| stored.connection_id == connection_id);
        let mut files = 0usize;
        let mut bytes = 0usize;
        for size in upload_reservations
            .map(|upload| upload.declared_size)
            .chain(finished_reservations.map(|stored| stored.upload.size))
        {
            files += 1;
            bytes = bytes.saturating_add(size);
        }
        (files, bytes)
    }
}

fn check_owner(
    actual_connection: u64,
    actual_task: &str,
    connection_id: u64,
    task_id: &str,
) -> Result<(), UploadError> {
    check_connection(actual_connection, connection_id)?;
    if actual_task != task_id {
        return Err(UploadError::WrongTask);
    }
    Ok(())
}

fn check_connection(actual: u64, requested: u64) -> Result<(), UploadError> {
    if actual != requested {
        return Err(UploadError::WrongConnection);
    }
    Ok(())
}

fn classify_media_type(raw: &str) -> Result<(String, AttachmentKind), UploadError> {
    if raw.is_empty()
        || raw
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(unsupported_media_type(raw));
    }

    let mut segments = raw.split(';');
    let base = segments.next().unwrap_or_default();
    let parameter = segments.next();
    if segments.next().is_some() {
        return Err(unsupported_media_type(raw));
    }
    let Some((type_name, subtype)) = base.split_once('/') else {
        return Err(unsupported_media_type(raw));
    };
    if subtype.contains('/') || !is_mime_token(type_name) || !is_mime_token(subtype) {
        return Err(unsupported_media_type(raw));
    }

    let has_utf8_charset = match parameter {
        None => false,
        Some(value) if value.eq_ignore_ascii_case("charset=utf-8") => true,
        Some(_) => return Err(unsupported_media_type(raw)),
    };
    let media_type = base.to_ascii_lowercase();

    let kind = match media_type.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => AttachmentKind::Image,
        "text/rtf" | "application/rtf" => return Err(unsupported_media_type(raw)),
        media if media.starts_with("text/") => AttachmentKind::Utf8Text,
        "application/json"
        | "application/xml"
        | "application/javascript"
        | "application/x-javascript"
        | "application/toml"
        | "application/yaml"
        | "application/x-yaml" => AttachmentKind::Utf8Text,
        media if media.ends_with("+json") || media.ends_with("+xml") => AttachmentKind::Utf8Text,
        _ => return Err(unsupported_media_type(raw)),
    };
    if has_utf8_charset && kind != AttachmentKind::Utf8Text {
        return Err(unsupported_media_type(raw));
    }
    Ok((media_type, kind))
}

fn is_mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn unsupported_media_type(raw: &str) -> UploadError {
    UploadError::UnsupportedMediaType {
        media_type: raw.to_string(),
    }
}

fn sanitize_file_name(raw: &str) -> String {
    let basename = raw.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let mut sanitized = String::new();
    for character in basename.chars().take(120) {
        if character.is_alphanumeric()
            || matches!(character, '.' | '-' | '_' | ' ' | '(' | ')' | '[' | ']')
        {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }
    let sanitized = sanitized.trim().trim_matches('.');
    if sanitized.is_empty() {
        "attachment".to_string()
    } else {
        sanitized.to_string()
    }
}

fn escape_attribute(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for character in raw.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            '\r' => escaped.push_str("&#13;"),
            '\n' => escaped.push_str("&#10;"),
            '\t' => escaped.push_str("&#9;"),
            character if character.is_control() => escaped.push('�'),
            character if character.is_whitespace() && character != ' ' => {
                write!(&mut escaped, "&#x{:X};", character as u32)
                    .expect("writing to a String cannot fail");
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn is_forbidden_extension(file_name: &str) -> bool {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    matches!(
        extension.as_deref(),
        Some(
            "pdf"
                | "doc"
                | "docx"
                | "docm"
                | "xls"
                | "xlsx"
                | "xlsm"
                | "ppt"
                | "pptx"
                | "pptm"
                | "odt"
                | "ods"
                | "odp"
                | "rtf"
                | "zip"
                | "7z"
                | "rar"
                | "tar"
                | "gz"
                | "tgz"
                | "bz2"
                | "xz"
                | "exe"
                | "msi"
                | "dll"
                | "com"
                | "scr"
                | "bat"
                | "cmd"
                | "ps1"
                | "jar"
        )
    )
}

fn sniff_image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn sniff_forbidden_content(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF-") {
        Some("PDF")
    } else if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        Some("ZIP or Office archive")
    } else if bytes.starts_with(b"MZ")
        || bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"\xfe\xed\xfa\xce")
        || bytes.starts_with(b"\xce\xfa\xed\xfe")
        || bytes.starts_with(b"\xfe\xed\xfa\xcf")
        || bytes.starts_with(b"\xcf\xfa\xed\xfe")
        || bytes.starts_with(b"\xca\xfe\xba\xbe")
    {
        Some("executable")
    } else if bytes.starts_with(b"Rar!\x1a\x07")
        || bytes.starts_with(b"\x1f\x8b")
        || bytes.starts_with(b"7z\xbc\xaf\x27\x1c")
    {
        Some("compressed archive")
    } else if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") || bytes.starts_with(b"{\\rtf")
    {
        Some("Office document")
    } else {
        None
    }
}

fn opaque_id(namespace: &str) -> String {
    format!("zode_{namespace}_{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const CONNECTION: u64 = 7;
    const TASK: &str = "task-1";

    fn request(name: &str, media_type: &str, declared_size: usize) -> BeginUpload {
        BeginUpload {
            file_name: name.to_string(),
            media_type: media_type.to_string(),
            declared_size,
        }
    }

    fn begin(
        registry: &mut AttachmentRegistry,
        now: Instant,
        name: &str,
        media_type: &str,
        declared_size: usize,
    ) -> UploadTicket {
        registry
            .begin(
                CONNECTION,
                TASK,
                request(name, media_type, declared_size),
                now,
            )
            .unwrap()
    }

    fn upload_bytes(
        registry: &mut AttachmentRegistry,
        now: Instant,
        ticket: &UploadTicket,
        bytes: &[u8],
    ) {
        for (sequence, chunk) in bytes.chunks(MAX_RAW_CHUNK_BYTES).enumerate() {
            registry
                .push_chunk(CONNECTION, &ticket.upload_id, sequence as u64, chunk, now)
                .unwrap();
        }
    }

    fn consume_receipt(
        registry: &mut AttachmentRegistry,
        now: Instant,
        receipt: &FinishedReceipt,
    ) -> FinishedUpload {
        registry
            .consume_finished(
                CONNECTION,
                TASK,
                std::slice::from_ref(&receipt.attachment_id),
                now,
            )
            .unwrap()
            .pop()
            .unwrap()
    }

    fn valid_png(size: usize) -> Vec<u8> {
        let mut bytes = vec![0; size.max(8)];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.truncate(size.max(8));
        bytes
    }

    #[test]
    fn protocol_limits_are_exact() {
        assert_eq!(MAX_IMAGE_BYTES, 5 * 1024 * 1024);
        assert_eq!(MAX_TEXT_BYTES, 1024 * 1024);
        assert_eq!(MAX_TURN_BYTES, 20 * 1024 * 1024);
        assert_eq!(MAX_FILES_PER_TURN, 8);
        assert_eq!(MAX_RAW_CHUNK_BYTES, 256 * 1024);
        assert_eq!(MAX_IN_FLIGHT_PER_CONNECTION, 2);
        assert_eq!(MAX_PENDING_FILES_PER_CONNECTION, 16);
        assert_eq!(MAX_PENDING_BYTES_PER_CONNECTION, 40 * 1024 * 1024);
        assert_eq!(UPLOAD_TTL, Duration::from_secs(120));
    }

    #[test]
    fn opaque_ids_use_random_uuid_v4_payloads() {
        let upload = opaque_id("upload");
        let attachment = opaque_id("attachment");
        let upload_uuid = uuid::Uuid::parse_str(upload.strip_prefix("zode_upload_").unwrap())
            .expect("upload ID payload must be a UUID");
        let attachment_uuid =
            uuid::Uuid::parse_str(attachment.strip_prefix("zode_attachment_").unwrap())
                .expect("attachment ID payload must be a UUID");

        assert_eq!(upload_uuid.get_version_num(), 4);
        assert_eq!(attachment_uuid.get_version_num(), 4);
        assert_ne!(upload_uuid, attachment_uuid);
    }

    #[test]
    fn image_limit_matches_the_inline_encoder_exact_boundary() {
        const INLINE_IMAGE_LIMIT: usize = 5 * 1024 * 1024;

        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let exact = registry.begin(
            CONNECTION,
            TASK,
            request("exact.png", "image/png", INLINE_IMAGE_LIMIT),
            now,
        );
        assert!(exact.is_ok());

        registry.disconnect(CONNECTION);
        assert_eq!(
            registry.begin(
                CONNECTION,
                TASK,
                request("too-large.png", "image/png", INLINE_IMAGE_LIMIT + 1),
                now,
            ),
            Err(UploadError::FileTooLarge {
                declared: INLINE_IMAGE_LIMIT + 1,
                limit: INLINE_IMAGE_LIMIT,
            })
        );
    }

    #[test]
    fn begin_classifies_supported_mime_and_returns_server_ids() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let first = begin(
            &mut registry,
            now,
            "../notes/readme.md",
            "text/markdown;charset=UTF-8",
            4,
        );
        let second = begin(&mut registry, now, "photo.png", "image/png", 8);

        assert_ne!(first.upload_id, second.upload_id);
        assert!(!first.upload_id.contains("readme"));
        assert_eq!(first.file_name, "readme.md");
        assert_eq!(first.media_type, "text/markdown");
        assert_eq!(first.kind, AttachmentKind::Utf8Text);
        assert_eq!(second.kind, AttachmentKind::Image);
    }

    #[test]
    fn begin_rejects_unsupported_and_dangerous_file_types() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();

        for (name, mime) in [
            ("manual.pdf", "application/pdf"),
            (
                "report.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            ("bundle.zip", "application/zip"),
            ("RUN.EXE", "application/octet-stream"),
            ("fake.pdf", "text/plain"),
            ("renamed.txt", "text/rtf"),
            ("renamed.txt", "application/rtf"),
            ("utf16.txt", "text/plain;charset=utf-16"),
        ] {
            let error = registry
                .begin(CONNECTION, TASK, request(name, mime, 10), now)
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    UploadError::UnsupportedMediaType { .. }
                        | UploadError::UnsupportedFileType { .. }
                ),
                "unexpected error for {name}: {error:?}"
            );
        }
    }

    #[test]
    fn mime_parser_requires_exact_tokens_and_a_single_canonical_charset_parameter() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let invalid = [
            "",
            "text/",
            "/plain",
            "text/plain/foo",
            "text//plain",
            "garbage+json",
            "text/pl@in",
            "te(x)t/plain",
            " text/plain",
            "text/plain ",
            "text /plain",
            "text/ plain",
            "text/plain\t",
            "text/plain\r",
            "text/plain\n",
            "text/plain\0",
            "text/plain\u{00a0}",
            "text/plain; charset=utf-8",
            "text/plain;charset=utf8",
            "text/plain;charset=\"utf-8\"",
            "text/plain;charset=utf-8;foo=bar",
            "text/plain;foo=bar",
            "image/png;charset=utf-8",
        ];

        for media_type in invalid {
            assert!(
                matches!(
                    registry.begin(CONNECTION, TASK, request("safe.txt", media_type, 1), now,),
                    Err(UploadError::UnsupportedMediaType { .. })
                ),
                "malformed MIME was accepted: {media_type:?}"
            );
        }

        let problem = registry
            .begin(
                CONNECTION,
                TASK,
                request("problem.json", "application/problem+json", 0),
                now,
            )
            .unwrap();
        assert_eq!(problem.media_type, "application/problem+json");
        assert_eq!(problem.kind, AttachmentKind::Utf8Text);
        registry.disconnect(CONNECTION);
        let utf8 = registry
            .begin(
                CONNECTION,
                TASK,
                request("readme.txt", "TEXT/PLAIN;CHARSET=UTF-8", 0),
                now,
            )
            .unwrap();
        assert_eq!(utf8.media_type, "text/plain");
        assert_eq!(utf8.kind, AttachmentKind::Utf8Text);
    }

    #[test]
    fn attribute_escape_neutralizes_markup_controls_and_line_separators() {
        assert_eq!(
            escape_attribute("a&<>\"'\r\n\t\0\u{2028}z"),
            "a&amp;&lt;&gt;&quot;&#39;&#13;&#10;&#9;�&#x2028;z"
        );
    }

    #[test]
    fn begin_enforces_per_file_size_limits() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();

        assert!(
            begin(
                &mut registry,
                now,
                "exact.txt",
                "text/plain",
                MAX_TEXT_BYTES,
            )
            .upload_id
            .len()
                > 10
        );
        registry.disconnect(CONNECTION);
        assert!(matches!(
            registry.begin(
                CONNECTION,
                TASK,
                request("large.txt", "text/plain", MAX_TEXT_BYTES + 1),
                now
            ),
            Err(UploadError::FileTooLarge {
                limit: MAX_TEXT_BYTES,
                ..
            })
        ));
        assert!(
            begin(
                &mut registry,
                now,
                "exact.png",
                "image/png",
                MAX_IMAGE_BYTES,
            )
            .upload_id
            .len()
                > 10
        );
        registry.disconnect(CONNECTION);
        assert!(matches!(
            registry.begin(
                CONNECTION,
                TASK,
                request("large.png", "image/png", MAX_IMAGE_BYTES + 1),
                now
            ),
            Err(UploadError::FileTooLarge {
                limit: MAX_IMAGE_BYTES,
                ..
            })
        ));
    }

    #[test]
    fn begin_enforces_two_concurrent_uploads_per_connection() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        begin(&mut registry, now, "a.txt", "text/plain", 1);
        begin(&mut registry, now, "b.txt", "text/plain", 1);

        assert!(matches!(
            registry.begin(
                CONNECTION,
                "another-turn",
                request("c.txt", "text/plain", 1),
                now
            ),
            Err(UploadError::TooManyInFlight { limit: 2 })
        ));
    }

    #[test]
    fn connection_pending_file_limit_spans_tasks_and_consume_releases_it() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let mut task_a_ids = Vec::new();

        for task_index in 0..2 {
            let task_id = format!("task-{task_index}");
            for file_index in 0..8 {
                let ticket = registry
                    .begin(
                        CONNECTION,
                        &task_id,
                        request(&format!("{file_index}.txt"), "text/plain", 1),
                        now,
                    )
                    .unwrap();
                upload_bytes(&mut registry, now, &ticket, b"x");
                let finished = registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
                if task_index == 0 {
                    task_a_ids.push(finished.attachment_id);
                }
            }
        }

        let error = registry
            .begin(
                CONNECTION,
                "task-2",
                request("seventeenth.txt", "text/plain", 1),
                now,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            UploadError::TooManyPendingFiles { limit: 16 }
        ));

        registry
            .consume_finished(CONNECTION, "task-0", &[task_a_ids.remove(0)], now)
            .unwrap();
        assert!(registry
            .begin(
                CONNECTION,
                "task-2",
                request("replacement.txt", "text/plain", 1),
                now,
            )
            .is_ok());
    }

    #[test]
    fn connection_pending_byte_limit_spans_tasks_and_cancel_or_ttl_releases_it() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let bytes = valid_png(MAX_IMAGE_BYTES);
        let mut first_upload_id = None;

        for task_index in 0..4 {
            let task_id = format!("images-{task_index}");
            for file_index in 0..2 {
                let ticket = registry
                    .begin(
                        CONNECTION,
                        &task_id,
                        request(&format!("{file_index}.png"), "image/png", bytes.len()),
                        now,
                    )
                    .unwrap();
                upload_bytes(&mut registry, now, &ticket, &bytes);
                first_upload_id.get_or_insert_with(|| ticket.upload_id.clone());
                if task_index != 1 || file_index != 1 {
                    registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
                }
            }
        }

        let error = registry
            .begin(
                CONNECTION,
                "overflow",
                request("over.txt", "text/plain", 1),
                now,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            UploadError::PendingBytesExceeded {
                declared_total,
                limit: MAX_PENDING_BYTES_PER_CONNECTION,
            } if declared_total == MAX_PENDING_BYTES_PER_CONNECTION + 1
        ));

        registry
            .cancel_upload(CONNECTION, first_upload_id.as_deref().unwrap())
            .unwrap();
        assert!(registry
            .begin(
                CONNECTION,
                "overflow",
                request("after-cancel.txt", "text/plain", 1),
                now,
            )
            .is_ok());

        registry.disconnect(CONNECTION);
        for task_index in 0..2 {
            let task_id = format!("ttl-{task_index}");
            for file_index in 0..8 {
                let ticket = registry
                    .begin(
                        CONNECTION,
                        &task_id,
                        request(&format!("{file_index}.txt"), "text/plain", 1),
                        now,
                    )
                    .unwrap();
                upload_bytes(&mut registry, now, &ticket, b"x");
                registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
            }
        }
        assert!(registry
            .begin(
                CONNECTION,
                "after-ttl",
                request("fresh.txt", "text/plain", 1),
                now + UPLOAD_TTL,
            )
            .is_ok());
    }

    #[test]
    fn chunks_require_connection_owner_sequence_and_raw_size() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(
            &mut registry,
            now,
            "data.txt",
            "text/plain",
            MAX_RAW_CHUNK_BYTES + 1,
        );

        assert!(matches!(
            registry.push_chunk(CONNECTION + 1, &ticket.upload_id, 0, b"x", now),
            Err(UploadError::WrongConnection)
        ));
        assert!(matches!(
            registry.push_chunk(CONNECTION, &ticket.upload_id, 1, b"x", now),
            Err(UploadError::UnexpectedSequence {
                expected: 0,
                actual: 1
            })
        ));
        assert!(matches!(
            registry.push_chunk(CONNECTION, &ticket.upload_id, 0, b"", now),
            Err(UploadError::EmptyChunk)
        ));
        assert!(matches!(
            registry.push_chunk(
                CONNECTION,
                &ticket.upload_id,
                0,
                &vec![0; MAX_RAW_CHUNK_BYTES + 1],
                now
            ),
            Err(UploadError::ChunkTooLarge { .. })
        ));

        let full_chunk = vec![b'x'; MAX_RAW_CHUNK_BYTES];
        let ack = registry
            .push_chunk(CONNECTION, &ticket.upload_id, 0, &full_chunk, now)
            .unwrap();
        assert_eq!(ack.next_sequence, 1);
        assert!(matches!(
            registry.push_chunk(CONNECTION, &ticket.upload_id, 1, b"xx", now),
            Err(UploadError::DeclaredSizeExceeded { .. })
        ));
    }

    #[test]
    fn finish_requires_declared_size_and_valid_utf8() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let short = begin(&mut registry, now, "short.txt", "text/plain", 2);
        registry
            .push_chunk(CONNECTION, &short.upload_id, 0, b"x", now)
            .unwrap();
        assert!(matches!(
            registry.finish(CONNECTION, &short.upload_id, now),
            Err(UploadError::SizeMismatch {
                declared: 2,
                actual: 1
            })
        ));
        registry
            .cancel_upload(CONNECTION, &short.upload_id)
            .unwrap();

        let invalid = begin(&mut registry, now, "bad.txt", "text/plain", 2);
        registry
            .push_chunk(CONNECTION, &invalid.upload_id, 0, &[0xff, 0xfe], now)
            .unwrap();
        assert!(matches!(
            registry.finish(CONNECTION, &invalid.upload_id, now),
            Err(UploadError::InvalidUtf8)
        ));

        registry
            .cancel_upload(CONNECTION, &invalid.upload_id)
            .unwrap();
        let valid = begin(&mut registry, now, "good.txt", "text/plain", 6);
        upload_bytes(&mut registry, now, &valid, "你好".as_bytes());
        let receipt = registry.finish(CONNECTION, &valid.upload_id, now).unwrap();
        let finished = consume_receipt(&mut registry, now, &receipt);
        assert!(matches!(
            finished.payload,
            FinishedPayload::Utf8Text(ref text) if text == "你好"
        ));
    }

    #[test]
    fn finish_rejects_dangerous_content_even_when_declared_as_text() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let dangerous = [
            ("renamed.txt", b"%PDF-1.7\n".as_slice()),
            ("archive.txt", b"PK\x03\x04payload".as_slice()),
            ("program.txt", b"MZthis-is-an-executable".as_slice()),
            (
                "legacy-office.txt",
                b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1payload".as_slice(),
            ),
            ("document.txt", b"{\\rtf1 content}".as_slice()),
            ("linux-binary.txt", b"\x7fELFpayload".as_slice()),
            ("archive-rar.txt", b"Rar!\x1a\x07payload".as_slice()),
            ("archive-gzip.txt", b"\x1f\x8bpayload".as_slice()),
            ("archive-7z.txt", b"7z\xbc\xaf\x27\x1cpayload".as_slice()),
            ("mac-binary.txt", b"\xfe\xed\xfa\xcfpayload".as_slice()),
            ("java-class.txt", b"\xca\xfe\xba\xbepayload".as_slice()),
        ];

        for (name, bytes) in dangerous {
            let ticket = begin(
                &mut registry,
                now,
                name,
                "text/plain;charset=utf-8",
                bytes.len(),
            );
            upload_bytes(&mut registry, now, &ticket, bytes);
            assert!(
                matches!(
                    registry.finish(CONNECTION, &ticket.upload_id, now),
                    Err(UploadError::ForbiddenContent { .. })
                ),
                "dangerous signature in {name} was accepted"
            );
            registry
                .cancel_upload(CONNECTION, &ticket.upload_id)
                .unwrap();
        }
    }

    #[test]
    fn finish_sniffs_image_bytes_and_requires_declared_mime_match() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let mismatched = begin(&mut registry, now, "wrong.png", "image/png", 8);
        upload_bytes(&mut registry, now, &mismatched, b"GIF89a!!");
        assert!(matches!(
            registry.finish(CONNECTION, &mismatched.upload_id, now),
            Err(UploadError::ImageTypeMismatch { .. })
        ));
        registry
            .cancel_upload(CONNECTION, &mismatched.upload_id)
            .unwrap();

        let png = valid_png(8);
        let valid = begin(&mut registry, now, "right.png", "image/png", png.len());
        upload_bytes(&mut registry, now, &valid, &png);
        let receipt = registry.finish(CONNECTION, &valid.upload_id, now).unwrap();
        assert_ne!(receipt.attachment_id, valid.upload_id);
        let finished = consume_receipt(&mut registry, now, &receipt);
        assert!(matches!(
            finished.payload,
            FinishedPayload::Image(ref bytes) if bytes == &png
        ));
    }

    #[test]
    fn finish_returns_lightweight_receipt_and_registry_retains_the_payload() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(&mut registry, now, "owned.txt", "text/plain", 5);
        upload_bytes(&mut registry, now, &ticket, b"owned");

        let receipt: FinishedReceipt = registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
        assert_eq!(receipt.kind, AttachmentKind::Utf8Text);
        assert_eq!(receipt.size, 5);
        let consumed = registry
            .consume_finished(CONNECTION, TASK, &[receipt.attachment_id], now)
            .unwrap();
        assert!(matches!(
            consumed.as_slice(),
            [FinishedUpload {
                payload: FinishedPayload::Utf8Text(text),
                ..
            }] if text == "owned"
        ));
    }

    #[test]
    fn turn_limits_include_in_flight_and_finished_reservations() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();

        for index in 0..MAX_FILES_PER_TURN {
            let ticket = begin(&mut registry, now, &format!("{index}.txt"), "text/plain", 1);
            upload_bytes(&mut registry, now, &ticket, b"x");
            registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
        }
        assert!(matches!(
            registry.begin(CONNECTION, TASK, request("ninth.txt", "text/plain", 1), now),
            Err(UploadError::TooManyFiles { limit: 8 })
        ));

        registry.disconnect(CONNECTION);
        for index in 0..4 {
            let bytes = valid_png(MAX_IMAGE_BYTES);
            let ticket = begin(
                &mut registry,
                now,
                &format!("{index}.png"),
                "image/png",
                bytes.len(),
            );
            upload_bytes(&mut registry, now, &ticket, &bytes);
            registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
        }
        assert!(matches!(
            registry.begin(CONNECTION, TASK, request("over.txt", "text/plain", 1), now),
            Err(UploadError::TurnTooLarge {
                limit: MAX_TURN_BYTES,
                ..
            })
        ));
    }

    #[test]
    fn finished_attachments_are_consumed_once_and_batch_is_atomic() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let a = begin(&mut registry, now, "a.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &a, b"a");
        let a = registry.finish(CONNECTION, &a.upload_id, now).unwrap();
        let b = begin(&mut registry, now, "b.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &b, b"b");
        let b = registry.finish(CONNECTION, &b.upload_id, now).unwrap();

        assert!(matches!(
            registry.consume_finished(
                CONNECTION,
                "another-task",
                std::slice::from_ref(&a.attachment_id),
                now,
            ),
            Err(UploadError::WrongTask)
        ));
        assert!(matches!(
            registry.consume_finished(
                CONNECTION,
                TASK,
                &[a.attachment_id.clone(), "missing".to_string()],
                now
            ),
            Err(UploadError::AttachmentNotFound)
        ));
        let consumed = registry
            .consume_finished(
                CONNECTION,
                TASK,
                &[a.attachment_id.clone(), b.attachment_id.clone()],
                now,
            )
            .unwrap();
        assert_eq!(consumed.len(), 2);
        assert!(matches!(
            registry.consume_finished(CONNECTION, TASK, &[a.attachment_id], now),
            Err(UploadError::AttachmentNotFound)
        ));
    }

    #[test]
    fn failed_preparation_leaves_the_entire_batch_available_for_retry() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let a = begin(&mut registry, now, "a.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &a, b"a");
        let a = registry.finish(CONNECTION, &a.upload_id, now).unwrap();
        let b = begin(&mut registry, now, "b.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &b, b"b");
        let b = registry.finish(CONNECTION, &b.upload_id, now).unwrap();
        let ids = vec![a.attachment_id, b.attachment_id];

        let error = registry
            .consume_finished_with(CONNECTION, TASK, &ids, now, |upload| {
                if upload.file_name == "b.txt" {
                    Err("cannot encode image")
                } else {
                    Ok(upload.file_name.clone())
                }
            })
            .unwrap_err();
        assert_eq!(error, ConsumeFinishedError::Prepare("cannot encode image"));

        let retried = registry
            .consume_finished(CONNECTION, TASK, &ids, now)
            .unwrap();
        assert_eq!(retried.len(), 2);
    }

    #[test]
    fn remove_task_clears_open_and_finished_uploads_without_touching_other_tasks() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let open = registry
            .begin(
                CONNECTION,
                "gone",
                request("open.txt", "text/plain", 1),
                now,
            )
            .unwrap();
        upload_bytes(&mut registry, now, &open, b"x");
        let finished = registry
            .begin(
                CONNECTION,
                "gone",
                request("done.txt", "text/plain", 1),
                now,
            )
            .unwrap();
        upload_bytes(&mut registry, now, &finished, b"x");
        registry
            .finish(CONNECTION, &finished.upload_id, now)
            .unwrap();
        let kept = registry
            .begin(
                CONNECTION,
                "kept",
                request("kept.txt", "text/plain", 1),
                now,
            )
            .unwrap();
        upload_bytes(&mut registry, now, &kept, b"x");
        let kept = registry.finish(CONNECTION, &kept.upload_id, now).unwrap();

        assert_eq!(
            registry.remove_task("gone"),
            CleanupStats {
                uploads: 1,
                attachments: 1,
            }
        );
        assert_eq!(registry.reserved_for_task(CONNECTION, "gone"), (0, 0));
        assert_eq!(
            registry
                .consume_finished(CONNECTION, "kept", &[kept.attachment_id], now)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn consume_revalidates_turn_file_and_byte_limits_before_ownership() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let mut attachment_ids = Vec::new();
        for index in 0..9 {
            let task_id = if index < 8 { "files-a" } else { "files-b" };
            let ticket = registry
                .begin(
                    CONNECTION,
                    task_id,
                    request(&format!("{index}.txt"), "text/plain", 1),
                    now,
                )
                .unwrap();
            upload_bytes(&mut registry, now, &ticket, b"x");
            attachment_ids.push(
                registry
                    .finish(CONNECTION, &ticket.upload_id, now)
                    .unwrap()
                    .attachment_id,
            );
        }
        assert!(matches!(
            registry.consume_finished(CONNECTION, "files-a", &attachment_ids, now),
            Err(UploadError::TooManyFiles { limit: 8 })
        ));

        registry.disconnect(CONNECTION);
        let bytes = valid_png(MAX_IMAGE_BYTES);
        let mut attachment_ids = Vec::new();
        for index in 0..5 {
            let task_id = if index < 2 { "bytes-a" } else { "bytes-b" };
            let ticket = registry
                .begin(
                    CONNECTION,
                    task_id,
                    request(&format!("{index}.png"), "image/png", bytes.len()),
                    now,
                )
                .unwrap();
            upload_bytes(&mut registry, now, &ticket, &bytes);
            attachment_ids.push(
                registry
                    .finish(CONNECTION, &ticket.upload_id, now)
                    .unwrap()
                    .attachment_id,
            );
        }
        assert!(matches!(
            registry.consume_finished(CONNECTION, "bytes-a", &attachment_ids, now),
            Err(UploadError::TurnTooLarge {
                declared_total,
                limit: MAX_TURN_BYTES,
            }) if declared_total == 5 * MAX_IMAGE_BYTES
        ));
    }

    #[test]
    fn zero_byte_utf8_text_can_finish_and_be_consumed() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(&mut registry, now, "empty.txt", "text/plain", 0);
        let receipt = registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
        let consumed = registry
            .consume_finished(CONNECTION, TASK, &[receipt.attachment_id], now)
            .unwrap();
        assert!(matches!(
            consumed.as_slice(),
            [FinishedUpload {
                payload: FinishedPayload::Utf8Text(text),
                size: 0,
                ..
            }] if text.is_empty()
        ));
    }

    #[test]
    fn protocol_uses_task_only_at_begin_and_consume_and_keeps_upload_id_for_cancel() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(&mut registry, now, "chip.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &ticket, b"x");
        let finished = registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();
        assert_eq!(registry.reserved_for_task(CONNECTION, TASK), (1, 1));

        assert!(registry
            .cancel_upload(CONNECTION, &ticket.upload_id)
            .unwrap());
        assert_eq!(registry.reserved_for_task(CONNECTION, TASK), (0, 0));
        assert!(!registry
            .cancel_upload(CONNECTION, &ticket.upload_id)
            .unwrap());
        assert!(matches!(
            registry.consume_finished(CONNECTION, TASK, &[finished.attachment_id], now),
            Err(UploadError::AttachmentNotFound)
        ));
    }

    #[test]
    fn duplicate_attachment_ids_are_rejected_without_consuming() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(&mut registry, now, "a.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &ticket, b"a");
        let finished = registry.finish(CONNECTION, &ticket.upload_id, now).unwrap();

        assert!(matches!(
            registry.consume_finished(
                CONNECTION,
                TASK,
                &[
                    finished.attachment_id.clone(),
                    finished.attachment_id.clone()
                ],
                now
            ),
            Err(UploadError::DuplicateAttachment)
        ));
        assert_eq!(
            registry
                .consume_finished(CONNECTION, TASK, &[finished.attachment_id], now)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn cancel_disconnect_and_ttl_release_all_reservations() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let cancel = begin(&mut registry, now, "cancel.txt", "text/plain", 1);
        registry
            .cancel_upload(CONNECTION, &cancel.upload_id)
            .unwrap();
        assert_eq!(registry.reserved_for_task(CONNECTION, TASK), (0, 0));

        let unfinished = begin(&mut registry, now, "open.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &unfinished, b"x");
        let done = begin(&mut registry, now, "done.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &done, b"x");
        registry.finish(CONNECTION, &done.upload_id, now).unwrap();
        let cleanup = registry.disconnect(CONNECTION);
        assert_eq!(cleanup.uploads, 1);
        assert_eq!(cleanup.attachments, 1);
        assert_eq!(registry.reserved_for_task(CONNECTION, TASK), (0, 0));

        let expired_upload = begin(&mut registry, now, "old.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &expired_upload, b"x");
        let expired_done = begin(&mut registry, now, "old2.txt", "text/plain", 1);
        upload_bytes(&mut registry, now, &expired_done, b"x");
        registry
            .finish(CONNECTION, &expired_done.upload_id, now)
            .unwrap();
        let cleanup = registry.cleanup_expired(now + UPLOAD_TTL);
        assert_eq!(cleanup.uploads, 1);
        assert_eq!(cleanup.attachments, 1);
        assert_eq!(registry.reserved_for_task(CONNECTION, TASK), (0, 0));
    }

    #[test]
    fn valid_chunk_activity_refreshes_upload_ttl() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let ticket = begin(&mut registry, now, "alive.txt", "text/plain", 2);
        registry
            .push_chunk(
                CONNECTION,
                &ticket.upload_id,
                0,
                b"a",
                now + Duration::from_secs(119),
            )
            .unwrap();

        assert_eq!(
            registry.cleanup_expired(now + Duration::from_secs(120)),
            CleanupStats::default()
        );
        registry
            .push_chunk(
                CONNECTION,
                &ticket.upload_id,
                1,
                b"b",
                now + Duration::from_secs(120),
            )
            .unwrap();
        assert!(registry
            .finish(
                CONNECTION,
                &ticket.upload_id,
                now + Duration::from_secs(120)
            )
            .is_ok());
    }

    #[test]
    fn conversion_adds_a_sanitized_text_boundary_and_preserves_images() {
        let now = Instant::now();
        let mut registry = AttachmentRegistry::new();
        let text = begin(
            &mut registry,
            now,
            "C:\\secret\\..\\name\nwith-control.rs",
            "text/x-rust",
            11,
        );
        upload_bytes(&mut registry, now, &text, b"fn main(){}");
        let receipt = registry.finish(CONNECTION, &text.upload_id, now).unwrap();
        let text_id = receipt.attachment_id.clone();
        let text = consume_receipt(&mut registry, now, &receipt);
        let prepared = text.into_prepared();
        let PreparedTurnAttachment::TextBlock { text } = prepared else {
            panic!("expected text block");
        };
        let boundary = format!("ZODE-ATTACHMENT-{text_id}");
        assert_eq!(
            text,
            format!(
                "<attached_file name=\"name_with-control.rs\" media_type=\"text/x-rust\" boundary=\"{boundary}\">\n--- BEGIN {boundary} ---\nfn main(){{}}\n--- END {boundary} ---\n</attached_file>"
            )
        );
        assert!(!text.contains("secret"));

        let png = valid_png(8);
        let image = begin(&mut registry, now, "photo.png", "image/png", png.len());
        upload_bytes(&mut registry, now, &image, &png);
        let receipt = registry.finish(CONNECTION, &image.upload_id, now).unwrap();
        let image = consume_receipt(&mut registry, now, &receipt);
        assert_eq!(
            image.into_prepared(),
            PreparedTurnAttachment::Image {
                display_name: "photo.png".to_string(),
                media_type: "image/png".to_string(),
                bytes: png,
            }
        );
    }
}
