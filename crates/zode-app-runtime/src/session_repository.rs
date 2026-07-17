//! Local-node adapter around Zode's shared session persistence.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent::message::MessageStore;
use agent::session::SessionError;
use zode_core::session_meta::SessionMeta;
use zode_core::session_store::{
    SessionRepository as CoreSessionRepository, SessionSave, SessionSaveOutcome, SessionWriteMode,
};
use zode_core::CoreError;
use zode_node_protocol::{
    EndpointError, EndpointErrorKind, NodeId, SessionLocator, ThreadStatus, ThreadSummary,
    WorkspaceUri,
};

use crate::AppStateStore;

/// A transcript and its persisted metadata loaded as one runtime value.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub meta: SessionMeta,
    pub store: MessageStore,
}

/// Restricts shared core persistence to one local node identity.
#[derive(Debug, Clone)]
pub struct LocalSessionRepository {
    node_id: NodeId,
    inner: CoreSessionRepository,
    app_state: AppStateStore,
}

impl LocalSessionRepository {
    pub fn new(config_dir: impl AsRef<Path>, node_id: NodeId) -> Self {
        let config_dir = config_dir.as_ref().to_path_buf();
        Self {
            node_id,
            inner: CoreSessionRepository::new(config_dir.clone()),
            app_state: AppStateStore::new(config_dir),
        }
    }

    /// List local sessions newest first as transport-neutral summaries.
    pub fn list(&self) -> Result<Vec<ThreadSummary>, EndpointError> {
        let sessions = self.inner.list().map_err(map_core_error)?;
        self.reconcile_app_state(sessions.iter().map(|meta| meta.id.as_str()))?;
        sessions
            .into_iter()
            .map(|meta| self.thread_summary(meta))
            .collect()
    }

    /// Load one local transcript lazily from disk.
    pub async fn load(&self, session: &SessionLocator) -> Result<LoadedSession, EndpointError> {
        let id = self.local_session_id(session)?;
        let meta = self.find_meta(id)?;
        let store = self.inner.load(id).await.map_err(map_core_error)?;
        Ok(LoadedSession { meta, store })
    }

    /// Create an empty session using the caller-allocated session identity.
    pub async fn create(
        &self,
        session: &SessionLocator,
        workspace_uri: &WorkspaceUri,
        model: String,
    ) -> Result<LoadedSession, EndpointError> {
        let id = self.local_session_id(session)?;
        let workspace = workspace_uri_to_path(workspace_uri)?;
        let cwd = workspace
            .to_str()
            .ok_or_else(invalid_workspace_path)?
            .to_string();
        let meta = SessionMeta {
            id: id.to_string(),
            title: "(untitled)".to_string(),
            cwd,
            model,
            updated_at: now_secs(),
        };
        let store = self
            .inner
            .create(meta.clone())
            .await
            .map_err(map_core_error)?;
        Ok(LoadedSession { meta, store })
    }

    pub async fn rename(
        &self,
        session: &SessionLocator,
        title: String,
    ) -> Result<(), EndpointError> {
        let id = self.local_session_id(session)?;
        self.inner.rename(id, title).await.map_err(map_core_error)
    }

    pub async fn update_model(
        &self,
        session: &SessionLocator,
        model: String,
    ) -> Result<(), EndpointError> {
        let id = self.local_session_id(session)?;
        self.inner
            .update_model(id, model)
            .await
            .map_err(map_core_error)
    }

    pub async fn delete(&self, session: &SessionLocator) -> Result<(), EndpointError> {
        let id = self.local_session_id(session)?;
        self.inner.delete(id).await.map_err(map_core_error)?;
        let remaining = self.inner.list().map_err(map_core_error)?;
        self.reconcile_app_state(remaining.iter().map(|meta| meta.id.as_str()))
    }

    /// Reserve a generation and persist a complete in-memory snapshot.
    pub async fn save(
        &self,
        session: &SessionLocator,
        mut meta: SessionMeta,
        store: MessageStore,
        requested: SessionWriteMode,
    ) -> Result<SessionSaveOutcome, EndpointError> {
        let id = self.local_session_id(session)?;
        if meta.id != id {
            return Err(endpoint_error(
                EndpointErrorKind::InvalidRequest,
                "session metadata does not match the requested session",
            ));
        }
        if !self.session_exists(id)? {
            return Err(session_not_found());
        }
        meta.updated_at = meta.updated_at.max(now_secs());
        let reservation = self
            .inner
            .reserve_save(id, requested)
            .map_err(map_core_error)?;
        self.inner
            .save(SessionSave {
                meta,
                store,
                write_mode: reservation.write_mode,
                generation: reservation.generation,
            })
            .await
            .map_err(map_core_error)
    }

    fn local_session_id<'a>(&self, session: &'a SessionLocator) -> Result<&'a str, EndpointError> {
        if session.node_id != self.node_id {
            return Err(endpoint_error(
                EndpointErrorKind::CapabilityDenied,
                "session is not owned by the local node",
            ));
        }
        if !is_valid_session_id(&session.session_id) {
            return Err(endpoint_error(
                EndpointErrorKind::InvalidRequest,
                "session identity is invalid",
            ));
        }
        Ok(&session.session_id)
    }

    fn find_meta(&self, id: &str) -> Result<SessionMeta, EndpointError> {
        self.inner
            .list()
            .map_err(map_core_error)?
            .into_iter()
            .find(|meta| meta.id == id)
            .ok_or_else(session_not_found)
    }

    fn session_exists(&self, id: &str) -> Result<bool, EndpointError> {
        Ok(self
            .inner
            .list()
            .map_err(map_core_error)?
            .iter()
            .any(|meta| meta.id == id))
    }

    fn reconcile_app_state<'a>(
        &self,
        existing: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), EndpointError> {
        let mut state = self.app_state.load().map_err(map_core_error)?;
        let original = state.clone();
        state.reconcile(existing);
        if state != original {
            self.app_state.save(&state).map_err(map_core_error)?;
        }
        Ok(())
    }

    fn thread_summary(&self, meta: SessionMeta) -> Result<ThreadSummary, EndpointError> {
        let workspace_uri = path_to_workspace_uri(Path::new(&meta.cwd))?;
        let updated_at_ms =
            i64::try_from(meta.updated_at.saturating_mul(1_000)).unwrap_or(i64::MAX);
        Ok(ThreadSummary {
            session: SessionLocator::new(self.node_id, meta.id),
            workspace_uri,
            title: meta.title,
            updated_at_ms,
            status: ThreadStatus::Idle,
        })
    }
}

/// Encode an absolute UTF-8 local path as a whitespace-free `file://` URI.
pub fn path_to_workspace_uri(path: &Path) -> Result<WorkspaceUri, EndpointError> {
    if !path.is_absolute() {
        return Err(invalid_workspace_path());
    }
    let path = path.to_str().ok_or_else(invalid_workspace_path)?;
    let uri_path = platform_uri_path(path, cfg!(windows));
    let encoded = percent_encode_uri_path(&uri_path);
    WorkspaceUri::new(format!("file://{encoded}")).map_err(|_| {
        endpoint_error(
            EndpointErrorKind::Internal,
            "failed to encode the local workspace URI",
        )
    })
}

fn percent_encode_uri_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if is_uri_path_byte(byte) {
            encoded.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn platform_uri_path(path: &str, windows: bool) -> String {
    if !windows {
        return path.to_string();
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    }
}

/// Decode a local `file://` workspace URI without filesystem normalization.
pub fn workspace_uri_to_path(uri: &WorkspaceUri) -> Result<PathBuf, EndpointError> {
    let Some(encoded) = uri.as_str().strip_prefix("file://") else {
        return Err(endpoint_error(
            EndpointErrorKind::CapabilityDenied,
            "workspace is not owned by the local node",
        ));
    };
    let bytes = percent_decode(encoded)?;
    let decoded = String::from_utf8(bytes).map_err(|_| {
        endpoint_error(
            EndpointErrorKind::InvalidRequest,
            "workspace URI path is not valid UTF-8",
        )
    })?;
    if decoded.contains('\0') {
        return Err(endpoint_error(
            EndpointErrorKind::InvalidRequest,
            "workspace URI path contains an invalid byte",
        ));
    }
    let decoded = platform_path_string(decoded, cfg!(windows));
    let path = PathBuf::from(decoded);
    if !path.is_absolute() {
        return Err(invalid_workspace_path());
    }
    Ok(path)
}

fn is_uri_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~')
}

fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn platform_path_string(mut path: String, windows: bool) -> String {
    let bytes = path.as_bytes();
    let drive_path = bytes.len() >= 4
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && bytes[3] == b'/';
    if windows && drive_path {
        path.remove(0);
    }
    path
}

fn percent_decode(encoded: &str) -> Result<Vec<u8>, EndpointError> {
    let input = encoded.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            output.push(input[index]);
            index += 1;
            continue;
        }
        if index + 2 >= input.len() {
            return Err(invalid_percent_encoding());
        }
        let high = hex_value(input[index + 1]).ok_or_else(invalid_percent_encoding)?;
        let low = hex_value(input[index + 2]).ok_or_else(invalid_percent_encoding)?;
        output.push((high << 4) | low);
        index += 3;
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn map_core_error(error: CoreError) -> EndpointError {
    match error {
        CoreError::Busy(_) => endpoint_error(EndpointErrorKind::Busy, "session repository is busy"),
        CoreError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => session_not_found(),
        CoreError::Session(SessionError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            session_not_found()
        }
        CoreError::Other(message) if message.starts_with("invalid session id:") => endpoint_error(
            EndpointErrorKind::InvalidRequest,
            "session identity is invalid",
        ),
        CoreError::Other(message) if message.starts_with("session already exists:") => {
            endpoint_error(
                EndpointErrorKind::InvalidRequest,
                "session identity already exists",
            )
        }
        CoreError::Other(message) if message.starts_with("session not found:") => {
            session_not_found()
        }
        CoreError::Json(_) => {
            endpoint_error(EndpointErrorKind::Internal, "session metadata is invalid")
        }
        CoreError::Session(
            SessionError::Json(_)
            | SessionError::MissingHeader
            | SessionError::UnsupportedVersion(_),
        ) => endpoint_error(EndpointErrorKind::Internal, "session transcript is invalid"),
        CoreError::Io(_)
        | CoreError::Session(_)
        | CoreError::MissingApiKey(_)
        | CoreError::UnknownProvider(_)
        | CoreError::UnknownDialect(_)
        | CoreError::Agent(_)
        | CoreError::Other(_) => endpoint_error(
            EndpointErrorKind::Internal,
            "session repository operation failed",
        ),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn endpoint_error(kind: EndpointErrorKind, message: &'static str) -> EndpointError {
    EndpointError {
        kind,
        message: message.to_string(),
    }
}

fn session_not_found() -> EndpointError {
    endpoint_error(EndpointErrorKind::NotFound, "session was not found")
}

fn invalid_workspace_path() -> EndpointError {
    endpoint_error(
        EndpointErrorKind::InvalidRequest,
        "workspace path must be an absolute UTF-8 path",
    )
}

fn invalid_percent_encoding() -> EndpointError {
    endpoint_error(
        EndpointErrorKind::InvalidRequest,
        "workspace URI has invalid percent encoding",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_drive_path_uses_standard_file_uri_shape() {
        let uri_path = platform_uri_path(r"C:\Users\Fini Zhang\设计", true);
        let encoded = percent_encode_uri_path(&uri_path);

        assert_eq!(
            format!("file://{encoded}"),
            "file:///C:/Users/Fini%20Zhang/%E8%AE%BE%E8%AE%A1"
        );
        assert_eq!(
            platform_path_string("/C:/Users/Fini Zhang/设计".to_string(), true),
            "C:/Users/Fini Zhang/设计"
        );
    }
}
