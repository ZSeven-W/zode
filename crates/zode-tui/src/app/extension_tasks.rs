use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use agent::abort::AbortController;
use agent::message::{ContentBlock, Message, MessageStore};
use agent::session::Session;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;
use zode_core::approval::{Approval, ApprovalRequest};
#[cfg(test)]
use zode_core::browser::bridge::TaskClientFrame;
use zode_core::browser::bridge::{
    TaskClientBody, TaskInbound, TaskInboundKind, TaskReceiver, TaskServerFrame,
};
use zode_core::images::ImageAttachment;
use zode_core::session_meta::{SessionIndex, SessionMeta};
use zode_core::{EngineTemplate, ToolAccessMode, ZodeEngine};

use crate::event::{
    ExtensionApprovalDecision, ExtensionIndexPurpose, ExtensionSnapshotPurpose, ExtensionTaskEvent,
    ExtensionTaskFailure, ExtensionTaskRequest,
};

use super::extension_attachments::{
    AttachmentKind, BeginUpload, ConsumeFinishedError, PreparedTurnAttachment, UploadError,
};
use super::{
    resolve_image_submit_route, AppEvent, ImageSubmitRoute, Mode, ReassembleEffect,
    ReassembledEngine, SessionTab, TuiApp,
};

const EXTENSION_PENDING_REQUEST_LIMIT: usize = 16;
const EXTENSION_WORKER_LIMIT: usize = 4;
const EXTENSION_RECENT_REQUEST_LIMIT: usize = 128;
const EXTENSION_RECENT_TURN_LIMIT: usize = 128;
const EXTENSION_PENDING_APPROVAL_LIMIT: usize = 64;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(super) struct ExtensionTaskError {
    code: &'static str,
    message: String,
}

impl ExtensionTaskError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_params",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "task_not_found",
            message: message.into(),
        }
    }

    fn model_not_found(message: impl Into<String>) -> Self {
        Self {
            code: "model_not_found",
            message: message.into(),
        }
    }

    fn busy(message: impl Into<String>) -> Self {
        Self {
            code: "task_busy",
            message: message.into(),
        }
    }

    fn attachment(error: UploadError) -> Self {
        let code = match error {
            UploadError::UnsupportedMediaType { .. }
            | UploadError::UnsupportedFileType { .. }
            | UploadError::ForbiddenContent { .. }
            | UploadError::ImageTypeMismatch { .. } => "attachment_unsupported",
            UploadError::FileTooLarge { .. } => "attachment_too_large",
            UploadError::TooManyFiles { .. }
            | UploadError::TurnTooLarge { .. }
            | UploadError::TooManyInFlight { .. }
            | UploadError::TooManyPendingFiles { .. }
            | UploadError::PendingBytesExceeded { .. } => "attachment_limit",
            UploadError::UploadNotFound | UploadError::AttachmentNotFound => "attachment_not_found",
            UploadError::WrongConnection | UploadError::WrongTask => "attachment_forbidden",
            UploadError::UnexpectedSequence { .. }
            | UploadError::EmptyChunk
            | UploadError::ChunkTooLarge { .. }
            | UploadError::DeclaredSizeExceeded { .. }
            | UploadError::SizeMismatch { .. }
            | UploadError::InvalidUtf8
            | UploadError::DuplicateAttachment => "attachment_invalid",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }

    fn turn_not_found(message: impl Into<String>) -> Self {
        Self {
            code: "turn_not_found",
            message: message.into(),
        }
    }

    fn stale_approval(message: impl Into<String>) -> Self {
        Self {
            code: "stale_approval",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal_error",
            message: message.into(),
        }
    }

    pub(super) fn code(&self) -> &'static str {
        self.code
    }
}

#[derive(Debug, Clone)]
struct ExtensionSessionRepository {
    /// Production uses `SessionIndex`'s configured location. Tests inject an
    /// explicit root so they never mutate the process-wide ZODE_CONFIG_DIR.
    root: Option<PathBuf>,
    explicit_root_lock: Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    io_started: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    fail_upsert: Arc<AtomicBool>,
}

impl Default for ExtensionSessionRepository {
    fn default() -> Self {
        Self {
            root: None,
            explicit_root_lock: Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(test)]
            io_started: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(test)]
            fail_upsert: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ExtensionSessionRepository {
    async fn with_explicit_root<T>(
        &self,
        operation: &'static str,
        f: impl FnOnce(&Path) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
    ) -> Result<T, ExtensionTaskError>
    where
        T: Send + 'static,
    {
        let root = self
            .root
            .clone()
            .ok_or_else(|| ExtensionTaskError::internal("explicit session root is missing"))?;
        let _guard = self.explicit_root_lock.lock().await;
        #[cfg(test)]
        self.io_started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::task::spawn_blocking(move || f(&root))
            .await
            .map_err(|error| {
                ExtensionTaskError::internal(format!("{operation} worker failed: {error}"))
            })?
            .map_err(|error| ExtensionTaskError::internal(error.to_string()))
    }

    #[cfg(test)]
    async fn load_index(&self) -> Result<SessionIndex, ExtensionTaskError> {
        if self.root.is_none() {
            return crate::tab::session_index_load_checked()
                .await
                .map_err(|error| ExtensionTaskError::internal(error.to_string()));
        }
        self.with_explicit_root("session index load", |root| {
            let path = root.join("index.json");
            match std::fs::read(&path) {
                Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(SessionIndex::default())
                }
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    async fn load_index_if(
        &self,
        should_run: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Option<SessionIndex>, ExtensionTaskError> {
        if self.root.is_none() {
            return crate::tab::session_index_load_checked_if(move || should_run())
                .await
                .map_err(|error| ExtensionTaskError::internal(error.to_string()));
        }
        let root = self
            .root
            .clone()
            .ok_or_else(|| ExtensionTaskError::internal("explicit session root is missing"))?;
        let _guard = self.explicit_root_lock.lock().await;
        if !should_run() {
            return Ok(None);
        }
        #[cfg(test)]
        self.io_started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::task::spawn_blocking(move || {
            let path = root.join("index.json");
            match std::fs::read(&path) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(Into::into),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(SessionIndex::default())
                }
                Err(error) => Err(error.into()),
            }
        })
        .await
        .map_err(|error| {
            ExtensionTaskError::internal(format!("session index load worker failed: {error}"))
        })?
        .map(Some)
        .map_err(|error: Box<dyn std::error::Error + Send + Sync>| {
            ExtensionTaskError::internal(error.to_string())
        })
    }

    fn session_path(&self, id: &str) -> Result<PathBuf, ExtensionTaskError> {
        if let Some(root) = &self.root {
            return Ok(root.join(format!("{id}.jsonl")));
        }
        SessionIndex::session_path(id)
            .map_err(|error| ExtensionTaskError::internal(error.to_string()))
    }

    async fn upsert(&self, meta: SessionMeta) -> Result<(), ExtensionTaskError> {
        #[cfg(test)]
        if self.fail_upsert.load(Ordering::Acquire) {
            return Err(ExtensionTaskError::internal(
                "injected index upsert failure",
            ));
        }
        if self.root.is_none() {
            return crate::tab::index_upsert_checked(meta)
                .await
                .map_err(|error| ExtensionTaskError::internal(error.to_string()));
        }
        self.with_explicit_root("session index upsert", move |root| {
            std::fs::create_dir_all(root)?;
            let path = root.join("index.json");
            let mut index = match std::fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<SessionIndex>(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    SessionIndex::default()
                }
                Err(error) => return Err(error.into()),
            };
            index.upsert(meta);
            std::fs::write(path, serde_json::to_vec_pretty(&index)?)?;
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await
    }

    async fn remove(&self, id: &str) -> Result<(), ExtensionTaskError> {
        let index_result = if self.root.is_none() {
            crate::tab::index_remove_checked(id)
                .await
                .map(|_| ())
                .map_err(|error| ExtensionTaskError::internal(error.to_string()))
        } else {
            let id = id.to_string();
            self.with_explicit_root("session index remove", move |root| {
                let path = root.join("index.json");
                let mut index = match std::fs::read(&path) {
                    Ok(bytes) => serde_json::from_slice::<SessionIndex>(&bytes)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(error.into()),
                };
                if index.remove(&id) {
                    std::fs::write(path, serde_json::to_vec_pretty(&index)?)?;
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            })
            .await
        };

        // Attempt both operations, but remove the index entry first. If the
        // transcript deletion then fails, the residue is an invisible orphan;
        // doing this in the opposite order could leave a visible ghost task.
        let transcript_result = match self.session_path(id) {
            Ok(path) => match tokio::fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(ExtensionTaskError::internal(error.to_string())),
            },
            Err(error) => Err(error),
        };

        match (transcript_result, index_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(transcript), Err(index)) => Err(ExtensionTaskError::internal(format!(
                "transcript removal failed: {transcript}; index removal failed: {index}"
            ))),
        }
    }

    #[cfg(test)]
    fn io_started_for_test(&self) -> usize {
        self.io_started.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    fn set_fail_upsert_for_test(&self, fail: bool) {
        self.fail_upsert.store(fail, Ordering::Release);
    }
}

#[derive(Debug)]
pub(super) struct ExtensionTaskState {
    current_task_by_connection: HashMap<u64, String>,
    live_connections: HashSet<u64>,
    pending_requests: HashSet<(u64, String)>,
    claimed_requests: HashSet<(u64, String)>,
    recent_requests: VecDeque<(u64, String)>,
    cancellation_by_connection: HashMap<u64, Arc<AtomicBool>>,
    pending_task_metadata: HashMap<String, PendingTaskMetadata>,
    pending_completions: Vec<ExtensionCompletion>,
    turn_routes: HashMap<(usize, u64), ExtensionTurnRoute>,
    pending_approvals: HashMap<String, PendingExtensionApproval>,
    approval_sequence: u64,
    recent_turns: VecDeque<ExtensionTurnTombstone>,
    sessions: ExtensionSessionRepository,
    /// Bounds both index and snapshot workers. Cancellation is checked again
    /// after acquiring a slot, so stale queued work exits without touching I/O.
    worker_slots: Arc<Semaphore>,
    #[cfg(test)]
    bridge_active_for_test: Arc<std::sync::atomic::AtomicU64>,
    #[cfg(test)]
    sent_frames_for_test: Arc<std::sync::Mutex<Vec<(u64, TaskServerFrame)>>>,
    #[cfg(test)]
    fail_send_after_for_test: Arc<std::sync::Mutex<Option<usize>>>,
}

impl Default for ExtensionTaskState {
    fn default() -> Self {
        Self {
            current_task_by_connection: HashMap::new(),
            live_connections: HashSet::new(),
            pending_requests: HashSet::new(),
            claimed_requests: HashSet::new(),
            recent_requests: VecDeque::new(),
            cancellation_by_connection: HashMap::new(),
            pending_task_metadata: HashMap::new(),
            pending_completions: Vec::new(),
            turn_routes: HashMap::new(),
            pending_approvals: HashMap::new(),
            approval_sequence: 0,
            recent_turns: VecDeque::new(),
            sessions: ExtensionSessionRepository::default(),
            worker_slots: Arc::new(Semaphore::new(EXTENSION_WORKER_LIMIT)),
            #[cfg(test)]
            bridge_active_for_test: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            #[cfg(test)]
            sent_frames_for_test: Arc::new(std::sync::Mutex::new(Vec::new())),
            #[cfg(test)]
            fail_send_after_for_test: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestClaim {
    Accepted,
    Duplicate,
    Full,
}

#[derive(Debug, Clone)]
struct PendingTaskMetadata {
    cwd: String,
    model: String,
    access: ToolAccessMode,
}

#[derive(Debug, Clone)]
struct ExtensionCompletion {
    failure: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionTurnRouteState {
    Running,
    InterruptRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtensionApprovalRoute {
    Tui,
    Extension { connection_id: u64 },
    Deny,
}

#[derive(Debug, Clone)]
struct ExtensionTurnRoute {
    task_id: String,
    connection_id: Option<u64>,
    state: ExtensionTurnRouteState,
}

#[derive(Debug)]
struct PendingExtensionApproval {
    tab_id: usize,
    turn_id: u64,
    task_id: String,
    connection_id: u64,
    raw_tool: String,
    public_summary: String,
    source_cwd: PathBuf,
    sequence: u64,
    request: ApprovalRequest,
}

#[derive(Debug, Clone)]
struct ExtensionTurnTombstone {
    tab_id: usize,
    turn_id: u64,
    task_id: String,
    terminal: ExtensionTurnTerminal,
}

#[derive(Debug, Clone)]
enum ExtensionTurnTerminal {
    Completed,
    Failed(String),
    Interrupted,
}

struct PreparedExtensionTurn {
    tab_id: usize,
    turn_id: u64,
    task_id: String,
    input: String,
    content: Vec<ContentBlock>,
    images: Vec<ImageAttachment>,
    vision: Option<PreparedExtensionVision>,
    engine: Arc<ZodeEngine>,
    abort: AbortController,
}

struct PreparedExtensionVision {
    template: EngineTemplate,
    provider_name: String,
    prompt: String,
}

enum ConvertedExtensionAttachment {
    Text {
        block: ContentBlock,
        display_name: String,
        media_type: String,
    },
    Image(ImageAttachment),
}

fn spawn_prepared_extension_turn(
    prepared: PreparedExtensionTurn,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    let PreparedExtensionTurn {
        tab_id,
        turn_id,
        input,
        mut content,
        images,
        vision,
        engine,
        abort,
        ..
    } = prepared;
    tokio::spawn(async move {
        let stream_result: Result<Box<dyn agent::stream::EventStream>, String> = async {
            if let Some(vision) = vision {
                let assembled = vision
                    .template
                    .assemble_tab(
                        Some(engine.cwd.clone()),
                        Some(format!("{tab_id}:extension-vision")),
                    )
                    .await
                    .map_err(|error| format!("vision provider failed: {error}"))?;
                if !assembled.supports_images() {
                    return Err(format!(
                        "vision provider '{}' does not declare image support",
                        vision.provider_name
                    ));
                }
                let description = super::run_vision_description(
                    Arc::new(assembled),
                    vision.prompt,
                    input,
                    images,
                    abort.clone(),
                )
                .await?;
                content.push(ContentBlock::Text {
                    text: format!("Image context:\n{}", description.trim()),
                });
            }
            engine
                .turn_blocks(content, abort)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        super::forward_agent_turn_stream(engine, stream_result, tab_id, turn_id, tx).await;
    });
}

enum PreparedExtensionDirectRequest {
    Start(Box<PreparedExtensionTurn>),
    Interrupt {
        tab_id: usize,
        turn_id: u64,
        task_id: String,
        abort: Option<AbortController>,
        emit_stopping: bool,
        terminal_replay: Option<ExtensionTurnTerminal>,
        route_bound: bool,
    },
}

impl ExtensionTaskState {
    fn disconnected(&mut self, connection_id: u64) {
        if let Some(cancel) = self.cancellation_by_connection.remove(&connection_id) {
            cancel.store(true, Ordering::Release);
        }
        self.live_connections.remove(&connection_id);
        self.pending_requests
            .retain(|(owner, _)| *owner != connection_id);
        self.claimed_requests
            .retain(|(owner, _)| *owner != connection_id);
        self.recent_requests
            .retain(|(owner, _)| *owner != connection_id);
        self.current_task_by_connection.remove(&connection_id);
        let approval_ids: Vec<String> = self
            .pending_approvals
            .iter()
            .filter(|(_, approval)| approval.connection_id == connection_id)
            .map(|(approval_id, _)| approval_id.clone())
            .collect();
        for approval_id in approval_ids {
            if let Some(approval) = self.pending_approvals.remove(&approval_id) {
                let _ = approval.request.respond(Approval::Deny);
            }
        }
        for route in self.turn_routes.values_mut() {
            if route.connection_id == Some(connection_id) {
                route.connection_id = None;
            }
        }
        #[cfg(test)]
        if self
            .bridge_active_for_test
            .load(std::sync::atomic::Ordering::Acquire)
            == connection_id
        {
            self.bridge_active_for_test
                .store(0, std::sync::atomic::Ordering::Release);
        }
    }

    fn connected(&mut self, connection_id: u64) {
        let replaced: Vec<u64> = self
            .live_connections
            .iter()
            .copied()
            .filter(|owner| *owner != connection_id)
            .collect();
        for owner in replaced {
            self.disconnected(owner);
        }
        self.live_connections.insert(connection_id);
        self.cancellation_by_connection
            .entry(connection_id)
            .or_insert_with(|| Arc::new(AtomicBool::new(false)));
        #[cfg(test)]
        self.bridge_active_for_test
            .store(connection_id, std::sync::atomic::Ordering::Release);
    }

    #[cfg(test)]
    fn begin_request(&mut self, connection_id: u64, request_id: &str) -> bool {
        self.claim_request(connection_id, request_id) == RequestClaim::Accepted
    }

    fn claim_request(&mut self, connection_id: u64, request_id: &str) -> RequestClaim {
        let key = (connection_id, request_id.to_string());
        if self.claimed_requests.contains(&key) {
            return RequestClaim::Duplicate;
        }
        let pending_for_connection = self
            .pending_requests
            .iter()
            .filter(|(owner, _)| *owner == connection_id)
            .count();
        if pending_for_connection >= EXTENSION_PENDING_REQUEST_LIMIT {
            self.claimed_requests.insert(key.clone());
            self.remember_recent_request(key);
            return RequestClaim::Full;
        }
        self.claimed_requests.insert(key.clone());
        self.pending_requests.insert(key);
        RequestClaim::Accepted
    }

    fn request_is_pending(&self, connection_id: u64, request_id: &str) -> bool {
        self.live_connections.contains(&connection_id)
            && self
                .pending_requests
                .contains(&(connection_id, request_id.to_string()))
    }

    fn finish_request(&mut self, connection_id: u64, request_id: &str) {
        let key = (connection_id, request_id.to_string());
        if self.pending_requests.remove(&key) {
            self.remember_recent_request(key);
        }
    }

    fn remember_recent_request(&mut self, key: (u64, String)) {
        self.recent_requests.push_back(key);
        while self.recent_requests.len() > EXTENSION_RECENT_REQUEST_LIMIT {
            if let Some(expired) = self.recent_requests.pop_front() {
                if !self.pending_requests.contains(&expired) {
                    self.claimed_requests.remove(&expired);
                }
            }
        }
    }

    fn connection_is_live(&self, connection_id: u64) -> bool {
        self.live_connections.contains(&connection_id)
    }

    fn cancellation(&self, connection_id: u64) -> Option<Arc<AtomicBool>> {
        self.cancellation_by_connection.get(&connection_id).cloned()
    }

    #[cfg(test)]
    fn set_bridge_active_for_test(&self, connection_id: Option<u64>) {
        self.bridge_active_for_test.store(
            connection_id.unwrap_or(0),
            std::sync::atomic::Ordering::Release,
        );
    }

    #[cfg(test)]
    fn sent_frames_for_test(&self) -> Vec<(u64, TaskServerFrame)> {
        self.sent_frames_for_test
            .lock()
            .expect("sent-frame test lock")
            .clone()
    }

    #[cfg(test)]
    fn pending_approval_count_for_test(&self) -> usize {
        self.pending_approvals.len()
    }

    #[cfg(test)]
    fn fail_send_after_for_test(&self, successful_sends_before_failure: usize) {
        *self
            .fail_send_after_for_test
            .lock()
            .expect("send-failure test lock") = Some(successful_sends_before_failure);
    }

    #[cfg(test)]
    fn take_send_failure_for_test(&self) -> bool {
        let mut countdown = self
            .fail_send_after_for_test
            .lock()
            .expect("send-failure test lock");
        match *countdown {
            Some(0) => {
                *countdown = None;
                true
            }
            Some(remaining) => {
                *countdown = Some(remaining - 1);
                false
            }
            None => false,
        }
    }

    #[cfg(test)]
    fn pending_request_count_for_test(&self) -> usize {
        self.pending_requests.len()
    }

    pub(super) fn finish_background_task(&mut self, task_id: &str) {
        self.pending_task_metadata.remove(task_id);
    }

    pub(super) fn queue_completion(&mut self, failure: Option<(&str, &str)>) {
        self.pending_completions.push(ExtensionCompletion {
            failure: failure.map(|(code, message)| (code.to_string(), message.to_string())),
        });
    }

    fn take_completions(&mut self) -> Vec<ExtensionCompletion> {
        std::mem::take(&mut self.pending_completions)
    }

    pub(super) fn replace_task_selection(&mut self, removed_task_id: &str, fallback: Option<&str>) {
        for selected in self.current_task_by_connection.values_mut() {
            if selected == removed_task_id {
                if let Some(fallback) = fallback {
                    *selected = fallback.to_string();
                }
            }
        }
        if fallback.is_none() {
            self.current_task_by_connection
                .retain(|_, selected| selected != removed_task_id);
        }
    }

    pub(super) fn completion_connections(&self) -> Vec<u64> {
        let mut connections: Vec<u64> = self.current_task_by_connection.keys().copied().collect();
        connections.sort_unstable();
        connections
    }

    pub(super) fn retain_pending_tasks(
        &mut self,
        pending_task_ids: &HashSet<String>,
    ) -> Vec<String> {
        let removed: Vec<String> = self
            .pending_task_metadata
            .keys()
            .filter(|task_id| !pending_task_ids.contains(*task_id))
            .cloned()
            .collect();
        self.pending_task_metadata
            .retain(|task_id, _| pending_task_ids.contains(task_id));
        removed
    }

    #[cfg(test)]
    fn set_session_root_for_test(&mut self, root: PathBuf) {
        self.sessions.root = Some(root);
    }

    fn remember_turn_terminal(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        task_id: String,
        terminal: ExtensionTurnTerminal,
    ) {
        if self
            .recent_turns
            .iter()
            .any(|turn| turn.tab_id == tab_id && turn.turn_id == turn_id)
        {
            return;
        }
        self.recent_turns.push_back(ExtensionTurnTombstone {
            tab_id,
            turn_id,
            task_id,
            terminal,
        });
        while self.recent_turns.len() > EXTENSION_RECENT_TURN_LIMIT {
            self.recent_turns.pop_front();
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SnapshotParams {
    #[serde(default)]
    task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateParams {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SelectParams {
    task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModelSetParams {
    task_id: String,
    model: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PermissionSetParams {
    task_id: String,
    mode: ToolAccessMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TurnStartParams {
    task_id: String,
    input: String,
    #[serde(default)]
    attachment_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AttachmentBeginParams {
    task_id: String,
    name: String,
    media_type: String,
    size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AttachmentChunkParams {
    upload_id: String,
    sequence: u64,
    data: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AttachmentUploadParams {
    upload_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TurnInterruptParams {
    task_id: String,
    turn_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ApprovalRespondParams {
    task_id: String,
    turn_id: String,
    approval_id: String,
    decision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSummary {
    id: String,
    title: String,
    cwd: String,
    model: String,
    access: ToolAccessMode,
    status: &'static str,
    active_turn_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkspaceSummary {
    name: String,
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMessage {
    id: String,
    task_id: String,
    role: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryTool {
    id: String,
    task_id: String,
    name: String,
    summary: String,
    status: &'static str,
}

#[derive(Default)]
struct SnapshotHistory {
    messages: Vec<HistoryMessage>,
    tools: Vec<HistoryTool>,
}

struct PendingSnapshot {
    workspace: WorkspaceSummary,
    tasks: Vec<TaskSummary>,
    current_task_id: String,
    models: Vec<String>,
    history_source: Option<(String, Arc<std::sync::Mutex<MessageStore>>)>,
}

impl PendingSnapshot {
    fn finish(self, history: SnapshotHistory) -> Value {
        json!({
            "workspace": self.workspace,
            "tasks": self.tasks,
            "currentTaskId": self.current_task_id,
            "models": self.models,
            "messages": history.messages,
            "tools": history.tools,
            "approvals": [],
        })
    }
}

pub(super) async fn recv_extension_task(
    receiver: &mut Option<TaskReceiver>,
) -> Option<TaskInbound> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

impl TuiApp {
    fn disconnect_extension_connection(&mut self, connection_id: u64) {
        self.extension_tasks.disconnected(connection_id);
        self.extension_attachments.disconnect(connection_id);
    }

    fn connect_extension_connection(&mut self, connection_id: u64) {
        let replaced: Vec<u64> = self
            .extension_tasks
            .live_connections
            .iter()
            .copied()
            .filter(|owner| *owner != connection_id)
            .collect();
        for owner in replaced {
            self.extension_attachments.disconnect(owner);
        }
        self.extension_tasks.connected(connection_id);
    }

    pub(super) fn cleanup_extension_attachments_at(&mut self, now: Instant) {
        self.extension_attachments.cleanup_expired(now);
    }

    pub(super) fn extension_turn_has_route(&self, tab_id: usize, turn_id: u64) -> bool {
        self.extension_tasks
            .turn_routes
            .contains_key(&(tab_id, turn_id))
    }

    pub(super) fn classify_extension_approval_route(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        task_id: &str,
    ) -> ExtensionApprovalRoute {
        let Some(route) = self
            .extension_tasks
            .turn_routes
            .get(&(tab_id, turn_id))
            .cloned()
        else {
            return ExtensionApprovalRoute::Tui;
        };
        if route.task_id != task_id || route.state != ExtensionTurnRouteState::Running {
            return ExtensionApprovalRoute::Deny;
        }
        let Some(connection_id) = route.connection_id else {
            return ExtensionApprovalRoute::Deny;
        };
        if self.extension_connection_is_current(connection_id) {
            ExtensionApprovalRoute::Extension { connection_id }
        } else {
            // A route that names a dead bridge owner is an orphan lifecycle,
            // not merely a classification result. Fence the whole connection
            // so every route it owned becomes rebindable and every waiter it
            // owned fails closed together.
            self.fence_inactive_extension_connection(connection_id);
            ExtensionApprovalRoute::Deny
        }
    }

    pub(super) fn enqueue_extension_approval(
        &mut self,
        request: ApprovalRequest,
        tab_id: usize,
        turn_id: u64,
        task_id: String,
        connection_id: u64,
    ) {
        if self.extension_tasks.pending_approvals.len() >= EXTENSION_PENDING_APPROVAL_LIMIT {
            let _ = request.respond(Approval::Deny);
            return;
        }
        let Some(sequence) = self.extension_tasks.approval_sequence.checked_add(1) else {
            let _ = request.respond(Approval::Deny);
            return;
        };
        self.extension_tasks.approval_sequence = sequence;
        let approval_id = format!("approval-{sequence}");
        let raw_tool = request.tool.clone();
        let public_summary = public_tool_identity(&raw_tool);
        let Some(source_cwd) = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.engine.cwd.clone())
        else {
            let _ = request.respond(Approval::Deny);
            return;
        };
        self.extension_tasks.pending_approvals.insert(
            approval_id.clone(),
            PendingExtensionApproval {
                tab_id,
                turn_id,
                task_id: task_id.clone(),
                connection_id,
                raw_tool,
                public_summary: public_summary.clone(),
                source_cwd,
                sequence,
                request,
            },
        );
        let requested = TaskServerFrame::event(
            "approval/requested",
            json!({
                "taskId": task_id,
                "turnId": turn_id.to_string(),
                "approvalId": approval_id,
                "tool": public_summary,
                "summary": public_summary,
            }),
        );
        if !self.send_extension_frame(connection_id, requested) {
            self.disconnect_extension_connection(connection_id);
        }
    }

    fn extension_bridge_liveness(&self, connection_id: u64) -> Arc<dyn Fn() -> bool + Send + Sync> {
        #[cfg(test)]
        {
            let active = self.extension_tasks.bridge_active_for_test.clone();
            Arc::new(move || {
                let current = active.load(std::sync::atomic::Ordering::Acquire);
                current == 0 || current == connection_id
            })
        }
        #[cfg(not(test))]
        {
            let browser = self.extension_browser.clone();
            Arc::new(move || browser.is_extension_task_connection_active(connection_id))
        }
    }

    fn extension_connection_is_current(&self, connection_id: u64) -> bool {
        self.extension_tasks.connection_is_live(connection_id)
            && (self.extension_bridge_liveness(connection_id))()
    }

    fn fence_inactive_extension_connection(&mut self, connection_id: u64) -> bool {
        if self.extension_connection_is_current(connection_id) {
            true
        } else {
            self.disconnect_extension_connection(connection_id);
            false
        }
    }

    /// Parse and enqueue extension work without awaiting disk I/O or history
    /// conversion. The TUI select branch calls this synchronously.
    pub(super) fn dispatch_extension_inbound(
        &mut self,
        inbound: TaskInbound,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match inbound.kind {
            TaskInboundKind::Disconnected => {
                self.disconnect_extension_connection(inbound.connection_id);
            }
            TaskInboundKind::Request(frame) => {
                if !(self.extension_bridge_liveness(inbound.connection_id))() {
                    self.disconnect_extension_connection(inbound.connection_id);
                    return;
                }
                self.connect_extension_connection(inbound.connection_id);
                let TaskClientBody::Request {
                    id: request_id,
                    method,
                    params,
                } = frame.body;
                match self
                    .extension_tasks
                    .claim_request(inbound.connection_id, &request_id)
                {
                    RequestClaim::Accepted => {}
                    RequestClaim::Duplicate => {
                        tracing::debug!(
                            connection_id = inbound.connection_id,
                            request_id,
                            "duplicate extension request id dropped"
                        );
                        return;
                    }
                    RequestClaim::Full => {
                        if !self.send_extension_frame(
                            inbound.connection_id,
                            TaskServerFrame::error(
                                request_id,
                                "server_busy",
                                "too many extension requests are pending",
                            ),
                        ) {
                            self.disconnect_extension_connection(inbound.connection_id);
                        }
                        return;
                    }
                }
                let request = match parse_extension_request(&method, params) {
                    Ok(request) => request,
                    Err(error) => {
                        self.extension_tasks
                            .finish_request(inbound.connection_id, &request_id);
                        if !self.send_extension_frame(
                            inbound.connection_id,
                            TaskServerFrame::error(request_id, error.code(), error.to_string()),
                        ) {
                            self.disconnect_extension_connection(inbound.connection_id);
                        }
                        return;
                    }
                };
                match request {
                    request @ (ExtensionTaskRequest::TurnStart { .. }
                    | ExtensionTaskRequest::TurnInterrupt { .. }) => {
                        self.dispatch_extension_turn_request(
                            inbound.connection_id,
                            request_id,
                            request,
                            agent_tx,
                        );
                    }
                    ExtensionTaskRequest::ApprovalRespond {
                        task_id,
                        turn_id,
                        approval_id,
                        decision,
                    } => {
                        self.dispatch_extension_approval_response(
                            inbound.connection_id,
                            request_id,
                            task_id,
                            turn_id,
                            approval_id,
                            decision,
                        );
                    }
                    request @ (ExtensionTaskRequest::AttachmentBegin { .. }
                    | ExtensionTaskRequest::AttachmentChunk { .. }
                    | ExtensionTaskRequest::AttachmentFinish { .. }
                    | ExtensionTaskRequest::AttachmentCancel { .. }) => {
                        self.dispatch_extension_attachment_request(
                            inbound.connection_id,
                            request_id,
                            request,
                        );
                    }
                    request => {
                        self.dispatch_extension_index_worker(
                            inbound.connection_id,
                            ExtensionIndexPurpose::Request {
                                request_id,
                                request,
                            },
                            agent_tx,
                        );
                    }
                }
            }
        }
    }

    fn dispatch_extension_attachment_request(
        &mut self,
        connection_id: u64,
        request_id: String,
        request: ExtensionTaskRequest,
    ) {
        let now = Instant::now();
        let result = match request {
            ExtensionTaskRequest::AttachmentBegin {
                task_id,
                name,
                media_type,
                size,
            } => {
                if !self.tabs.iter().any(|tab| tab.session_id == task_id) {
                    Err(ExtensionTaskError::not_found(format!(
                        "task not found: {task_id}"
                    )))
                } else {
                    self.extension_attachments
                        .begin(
                            connection_id,
                            task_id,
                            BeginUpload {
                                file_name: name,
                                media_type,
                                declared_size: size,
                            },
                            now,
                        )
                        .map(|ticket| {
                            json!({
                                "uploadId": ticket.upload_id,
                                "name": ticket.file_name,
                                "mediaType": ticket.media_type,
                                "size": ticket.declared_size,
                                "kind": match ticket.kind {
                                    AttachmentKind::Utf8Text => "text",
                                    AttachmentKind::Image => "image",
                                },
                            })
                        })
                        .map_err(ExtensionTaskError::attachment)
                }
            }
            ExtensionTaskRequest::AttachmentChunk {
                upload_id,
                sequence,
                data,
            } => self
                .extension_attachments
                .push_chunk(connection_id, &upload_id, sequence, &data, now)
                .map(|ack| json!({"uploadId": upload_id, "nextSequence": ack.next_sequence}))
                .map_err(ExtensionTaskError::attachment),
            ExtensionTaskRequest::AttachmentFinish { upload_id } => self
                .extension_attachments
                .finish(connection_id, &upload_id, now)
                .map(|receipt| {
                    json!({
                        "uploadId": receipt.upload_id,
                        "attachmentId": receipt.attachment_id,
                        "taskId": receipt.task_id,
                        "name": receipt.file_name,
                        "mediaType": receipt.media_type,
                        "size": receipt.size,
                        "kind": match receipt.kind {
                            AttachmentKind::Utf8Text => "text",
                            AttachmentKind::Image => "image",
                        },
                    })
                })
                .map_err(ExtensionTaskError::attachment),
            ExtensionTaskRequest::AttachmentCancel { upload_id } => self
                .extension_attachments
                .cancel_upload(connection_id, &upload_id)
                .map(|cancelled| json!({"uploadId": upload_id, "cancelled": cancelled}))
                .map_err(ExtensionTaskError::attachment),
            _ => unreachable!("only attachment requests reach this path"),
        };

        self.extension_tasks
            .finish_request(connection_id, &request_id);
        let frame = match result {
            Ok(result) => TaskServerFrame::response(request_id, result),
            Err(error) => TaskServerFrame::error(request_id, error.code(), error.to_string()),
        };
        if !self.send_extension_frame(connection_id, frame) {
            self.disconnect_extension_connection(connection_id);
        }
    }

    fn dispatch_extension_approval_response(
        &mut self,
        connection_id: u64,
        request_id: String,
        task_id: String,
        turn_id: u64,
        approval_id: String,
        decision: ExtensionApprovalDecision,
    ) {
        let validation = self
            .extension_tasks
            .pending_approvals
            .get(&approval_id)
            .ok_or_else(|| ExtensionTaskError::stale_approval("approval is stale"))
            .and_then(|pending| {
                if pending.connection_id != connection_id
                    || pending.task_id != task_id
                    || pending.turn_id != turn_id
                {
                    return Err(ExtensionTaskError::stale_approval("approval is stale"));
                }
                let route_matches = self
                    .extension_tasks
                    .turn_routes
                    .get(&(pending.tab_id, pending.turn_id))
                    .is_some_and(|route| {
                        route.task_id == pending.task_id
                            && route.connection_id == Some(pending.connection_id)
                            && route.state == ExtensionTurnRouteState::Running
                    });
                let tab_matches = self
                    .tabs
                    .iter()
                    .find(|tab| tab.id == pending.tab_id)
                    .is_some_and(|tab| {
                        tab.session_id == pending.task_id
                            && tab.active_turn_id == pending.turn_id
                            && tab.turn_abort.is_some()
                            && tab.draining_turn_id.is_none()
                            && !tab.reassemble_pending
                    });
                if route_matches && tab_matches {
                    Ok(())
                } else {
                    Err(ExtensionTaskError::stale_approval("approval is stale"))
                }
            });

        self.extension_tasks
            .finish_request(connection_id, &request_id);
        if let Err(error) = validation {
            if !self.send_extension_frame(
                connection_id,
                TaskServerFrame::error(request_id, error.code(), error.to_string()),
            ) {
                self.disconnect_extension_connection(connection_id);
            }
            return;
        }

        let pending = self
            .extension_tasks
            .pending_approvals
            .remove(&approval_id)
            .expect("validated pending approval remains on the main loop");
        let decision_wire = extension_approval_decision_wire(decision);
        let response_enqueued = self.send_extension_frame(
            connection_id,
            TaskServerFrame::response(
                request_id,
                json!({"approvalId": approval_id, "decision": decision_wire}),
            ),
        );
        if !response_enqueued {
            let _ = pending.request.respond(Approval::Deny);
            self.disconnect_extension_connection(connection_id);
            return;
        }
        let resolved_enqueued = self.send_extension_frame(
            connection_id,
            TaskServerFrame::event(
                "approval/resolved",
                json!({
                    "taskId": pending.task_id,
                    "turnId": pending.turn_id.to_string(),
                    "approvalId": approval_id,
                    "decision": decision_wire,
                    "tool": pending.public_summary,
                    "summary": pending.public_summary,
                }),
            ),
        );
        if !resolved_enqueued {
            let _ = pending.request.respond(Approval::Deny);
            self.disconnect_extension_connection(connection_id);
            return;
        }

        let approval = match decision {
            ExtensionApprovalDecision::Allow => Approval::AllowOnce,
            ExtensionApprovalDecision::AllowAlways => Approval::AllowAlways,
            ExtensionApprovalDecision::Deny => Approval::Deny,
        };
        let responded = pending.request.respond(approval).is_ok();
        if !responded {
            return;
        }
        if decision == ExtensionApprovalDecision::AllowAlways {
            self.persist_allow_always_at(&pending.source_cwd, &pending.raw_tool);
        }
    }

    fn dispatch_extension_turn_request(
        &mut self,
        connection_id: u64,
        request_id: String,
        request: ExtensionTaskRequest,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let prepared = match request {
            ExtensionTaskRequest::TurnStart {
                task_id,
                input,
                attachment_ids,
            } => self
                .arm_extension_turn(connection_id, task_id, input, attachment_ids)
                .map(Box::new)
                .map(PreparedExtensionDirectRequest::Start),
            ExtensionTaskRequest::TurnInterrupt { task_id, turn_id } => {
                self.prepare_extension_turn_interrupt(connection_id, task_id, turn_id)
            }
            _ => unreachable!("only direct turn requests reach this path"),
        };
        self.extension_tasks
            .finish_request(connection_id, &request_id);
        match prepared {
            Ok(PreparedExtensionDirectRequest::Start(prepared)) => {
                let turn_id = prepared.turn_id.to_string();
                let response_enqueued = self.send_extension_frame(
                    connection_id,
                    TaskServerFrame::response(request_id, json!({"turnId": turn_id})),
                );
                let started_enqueued = response_enqueued
                    && self.send_extension_frame(
                        connection_id,
                        TaskServerFrame::event(
                            "turn/started",
                            json!({"taskId": prepared.task_id, "turnId": turn_id}),
                        ),
                    );
                if !started_enqueued {
                    self.disconnect_extension_connection(connection_id);
                }
                spawn_prepared_extension_turn(*prepared, agent_tx.clone());
            }
            Ok(PreparedExtensionDirectRequest::Interrupt {
                tab_id,
                turn_id,
                task_id,
                abort,
                emit_stopping,
                terminal_replay,
                route_bound,
            }) => {
                let response_enqueued = self.send_extension_frame(
                    connection_id,
                    TaskServerFrame::response(
                        request_id,
                        json!({
                            "turnId": turn_id.to_string(),
                            "alreadyFinished": terminal_replay.is_some(),
                        }),
                    ),
                );
                if !response_enqueued && route_bound {
                    self.disconnect_extension_connection(connection_id);
                } else if response_enqueued {
                    let event_enqueued = if let Some(terminal) = terminal_replay.as_ref() {
                        self.send_extension_frame(
                            connection_id,
                            extension_terminal_frame(&task_id, turn_id, terminal),
                        )
                    } else if emit_stopping {
                        self.deny_extension_approvals_for_turn(tab_id, turn_id, true)
                            && self.send_extension_frame(
                                connection_id,
                                TaskServerFrame::event(
                                    "turn/stopping",
                                    json!({"taskId": task_id, "turnId": turn_id.to_string()}),
                                ),
                            )
                    } else {
                        true
                    };
                    if !event_enqueued && route_bound {
                        self.disconnect_extension_connection(connection_id);
                    }
                }
                if let Some(abort) = abort {
                    abort.abort_with_reason("extension interrupted");
                }
            }
            Err(error) => {
                self.send_extension_frame(
                    connection_id,
                    TaskServerFrame::error(request_id, error.code(), error.to_string()),
                );
            }
        }
    }

    fn prepare_extension_turn_interrupt(
        &mut self,
        connection_id: u64,
        task_id: String,
        turn_id: u64,
    ) -> Result<PreparedExtensionDirectRequest, ExtensionTaskError> {
        let Some(tab_idx) = self.tabs.iter().position(|tab| tab.session_id == task_id) else {
            return Err(ExtensionTaskError::not_found(format!(
                "task not found: {task_id}"
            )));
        };
        let tab_id = self.tabs[tab_idx].id;
        if let Some(tombstone) = self
            .extension_tasks
            .recent_turns
            .iter()
            .find(|turn| {
                turn.tab_id == tab_id && turn.turn_id == turn_id && turn.task_id == task_id
            })
            .cloned()
        {
            return Ok(PreparedExtensionDirectRequest::Interrupt {
                tab_id,
                turn_id,
                task_id,
                abort: None,
                emit_stopping: false,
                terminal_replay: Some(tombstone.terminal),
                route_bound: false,
            });
        }

        if self.tabs[tab_idx].draining_turn_id == Some(turn_id) {
            let emit_stopping = !self
                .extension_tasks
                .turn_routes
                .contains_key(&(tab_id, turn_id));
            self.extension_tasks.turn_routes.insert(
                (tab_id, turn_id),
                ExtensionTurnRoute {
                    task_id: task_id.clone(),
                    connection_id: Some(connection_id),
                    state: ExtensionTurnRouteState::InterruptRequested,
                },
            );
            return Ok(PreparedExtensionDirectRequest::Interrupt {
                tab_id,
                turn_id,
                task_id,
                abort: None,
                emit_stopping,
                terminal_replay: None,
                route_bound: true,
            });
        }

        if self.tabs[tab_idx].active_turn_id != turn_id || self.tabs[tab_idx].turn_abort.is_none() {
            return Err(ExtensionTaskError::turn_not_found(format!(
                "turn not found for task {task_id}: {turn_id}"
            )));
        }
        let interrupt = self
            .prepare_tab_interrupt(tab_idx, Some(turn_id))
            .expect("live turn was validated before interrupt preparation");
        // Core/TUI ownership is cleared by prepare_tab_interrupt before the
        // extension claims the route, so no terminal modal can survive the
        // handoff or be mistaken for an extension-origin approval.
        self.extension_tasks.turn_routes.insert(
            (tab_id, turn_id),
            ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: Some(connection_id),
                state: ExtensionTurnRouteState::InterruptRequested,
            },
        );
        Ok(PreparedExtensionDirectRequest::Interrupt {
            tab_id,
            turn_id,
            task_id,
            abort: Some(interrupt.abort),
            emit_stopping: true,
            terminal_replay: None,
            route_bound: true,
        })
    }

    #[cfg(test)]
    fn arm_extension_text_turn(
        &mut self,
        connection_id: u64,
        task_id: String,
        input: String,
    ) -> Result<PreparedExtensionTurn, ExtensionTaskError> {
        self.arm_extension_turn(connection_id, task_id, input, Vec::new())
    }

    fn arm_extension_turn(
        &mut self,
        connection_id: u64,
        task_id: String,
        input: String,
        attachment_ids: Vec<String>,
    ) -> Result<PreparedExtensionTurn, ExtensionTaskError> {
        let Some(tab_idx) = self.tabs.iter().position(|tab| tab.session_id == task_id) else {
            return Err(ExtensionTaskError::not_found(format!(
                "task not found: {task_id}"
            )));
        };
        if self.tabs[tab_idx].is_busy() {
            return Err(ExtensionTaskError::busy(format!("task is busy: {task_id}")));
        }
        let turn_id = self.tabs[tab_idx]
            .turn_seq
            .checked_add(1)
            .ok_or_else(|| ExtensionTaskError::internal("turn id exhausted"))?;

        let engine = self.tabs[tab_idx].engine.clone();
        let images_config = self.template.images().clone();
        let image_route = resolve_image_submit_route(
            true,
            images_config.effective_mode(),
            engine.supports_images(),
            images_config.vision_provider.is_some(),
        );
        let vision = if image_route == ImageSubmitRoute::VisionModel {
            images_config
                .vision_provider
                .as_deref()
                .and_then(|provider_name| {
                    self.template.with_provider(provider_name).map(|template| {
                        PreparedExtensionVision {
                            template,
                            provider_name: provider_name.to_string(),
                            prompt: images_config.effective_prompt().to_string(),
                        }
                    })
                })
        } else {
            None
        };

        let converted = self
            .extension_attachments
            .consume_finished_with(
                connection_id,
                &task_id,
                &attachment_ids,
                Instant::now(),
                |upload| match upload.to_prepared() {
                    PreparedTurnAttachment::TextBlock { text } => {
                        Ok(ConvertedExtensionAttachment::Text {
                            block: ContentBlock::Text { text },
                            display_name: upload.file_name.clone(),
                            media_type: upload.media_type.clone(),
                        })
                    }
                    PreparedTurnAttachment::Image {
                        display_name,
                        media_type: _,
                        bytes,
                    } => {
                        if image_route == ImageSubmitRoute::Unsupported {
                            return Err(ExtensionTaskError {
                                code: "attachment_unsupported",
                                message: "current provider does not support images and no vision provider is configured".into(),
                            });
                        }
                        if image_route == ImageSubmitRoute::VisionModel && vision.is_none() {
                            return Err(ExtensionTaskError {
                                code: "attachment_unsupported",
                                message: "configured vision provider is unavailable".into(),
                            });
                        }
                        zode_core::images::image_attachment_from_bytes(&bytes, &display_name)
                            .map(ConvertedExtensionAttachment::Image)
                            .map_err(|error| ExtensionTaskError {
                                code: "attachment_invalid",
                                message: error.to_string(),
                            })
                    }
                },
            )
            .map_err(|error| match error {
                ConsumeFinishedError::Upload(error) => ExtensionTaskError::attachment(error),
                ConsumeFinishedError::Prepare(error) => error,
            })?;

        let mut content = Vec::new();
        if !input.trim().is_empty() {
            content.push(ContentBlock::Text {
                text: input.clone(),
            });
        }
        let mut images = Vec::new();
        let mut summaries = Vec::new();
        for attachment in converted {
            match attachment {
                ConvertedExtensionAttachment::Text {
                    block,
                    display_name,
                    media_type,
                } => {
                    summaries.push(format!("[Attached file: {display_name} ({media_type})]"));
                    content.push(block);
                }
                ConvertedExtensionAttachment::Image(image) => {
                    summaries.push(format!(
                        "[Attached image: {} ({})]",
                        image.display_name, image.media_type
                    ));
                    if image_route == ImageSubmitRoute::Direct {
                        content.push(image.content_block.clone());
                    }
                    images.push(image);
                }
            }
        }
        let vision = (!images.is_empty()).then_some(vision).flatten();

        let mut display_text = input.clone();
        if !summaries.is_empty() {
            if !display_text.trim().is_empty() {
                display_text.push_str("\n\n");
            }
            display_text.push_str(&summaries.join("\n"));
        }
        if !self.tabs[tab_idx].titled {
            let title = if input.trim().is_empty() {
                summaries
                    .first()
                    .map(String::as_str)
                    .unwrap_or("attachment")
            } else {
                &input
            };
            self.tabs[tab_idx].stamp_title(title);
        }
        let tab = &mut self.tabs[tab_idx];
        tab.chat
            .push_user_with_images(&display_text, super::image_previews(&images));
        tab.mode = Mode::Thinking;
        tab.active_tool_names.clear();
        tab.turn_used_tools = false;
        tab.turn_seq = turn_id;
        tab.active_turn_id = turn_id;
        let abort = AbortController::new();
        tab.turn_abort = Some(abort.clone());
        let tab_id = tab.id;
        // Approval requests snapshot this binding when they enter the core
        // queue. Arm it only after the canonical active id is installed and
        // before the provider task can emit a tool request.
        self.template
            .bind_approval_turn(&tab_id.to_string(), turn_id);
        self.extension_tasks.turn_routes.insert(
            (tab_id, turn_id),
            ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: Some(connection_id),
                state: ExtensionTurnRouteState::Running,
            },
        );
        Ok(PreparedExtensionTurn {
            tab_id,
            turn_id,
            task_id,
            input,
            content,
            images,
            vision,
            engine,
            abort,
        })
    }

    /// Deny every pending extension approval owned by one immutable turn.
    /// Resolved events are emitted in intake order before the core waiters are
    /// woken, allowing callers to append stopping/terminal frames afterwards.
    pub(super) fn deny_extension_approvals_for_turn(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        emit_resolved: bool,
    ) -> bool {
        let route = self
            .extension_tasks
            .turn_routes
            .get(&(tab_id, turn_id))
            .cloned();
        let mut approval_ids: Vec<(u64, String)> = self
            .extension_tasks
            .pending_approvals
            .iter()
            .filter(|(_, approval)| approval.tab_id == tab_id && approval.turn_id == turn_id)
            .map(|(approval_id, approval)| (approval.sequence, approval_id.clone()))
            .collect();
        approval_ids.sort_by_key(|(sequence, _)| *sequence);

        // Validate the immutable route owner even when this turn currently has
        // no waiters. Otherwise a TUI interrupt could leave a dead connection
        // attached merely because there was no approval frame to send.
        let mut sends_ok = route
            .as_ref()
            .and_then(|route| route.connection_id)
            .is_none_or(|connection_id| self.extension_connection_is_current(connection_id));
        for (_, approval_id) in approval_ids {
            let Some(pending) = self.extension_tasks.pending_approvals.remove(&approval_id) else {
                continue;
            };
            let route_owner = route.as_ref().and_then(|route| route.connection_id);
            let route_identity_matches = route.as_ref().is_some_and(|route| {
                route.task_id == pending.task_id
                    && route.connection_id == Some(pending.connection_id)
            });
            let live_connection = (route_identity_matches
                && self.extension_connection_is_current(pending.connection_id))
            .then_some(pending.connection_id);
            if emit_resolved && sends_ok {
                match (route_owner, live_connection) {
                    (_, Some(connection_id)) => {
                        sends_ok = self.send_extension_frame(
                            connection_id,
                            TaskServerFrame::event(
                                "approval/resolved",
                                json!({
                                    "taskId": pending.task_id,
                                    "turnId": pending.turn_id.to_string(),
                                    "approvalId": approval_id,
                                    "decision": "deny",
                                    "tool": pending.public_summary,
                                    "summary": pending.public_summary,
                                }),
                            ),
                        );
                    }
                    (Some(_), None) => sends_ok = false,
                    (None, None) => {}
                }
            }
            let _ = pending.request.respond(Approval::Deny);
        }
        sends_ok
    }

    pub(super) fn resolve_extension_approvals_before_tui_interrupt(
        &mut self,
        tab_id: usize,
        turn_id: u64,
    ) {
        let connection_id = self
            .extension_tasks
            .turn_routes
            .get(&(tab_id, turn_id))
            .and_then(|route| route.connection_id);
        let approvals_resolved = self.deny_extension_approvals_for_turn(tab_id, turn_id, true);
        if !approvals_resolved {
            if let Some(connection_id) = connection_id {
                self.disconnect_extension_connection(connection_id);
            }
        }
    }

    pub(super) fn mark_extension_turn_interrupt_requested(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        connection_id: Option<u64>,
    ) {
        if let Some(route) = self.extension_tasks.turn_routes.get_mut(&(tab_id, turn_id)) {
            route.state = ExtensionTurnRouteState::InterruptRequested;
            if let Some(connection_id) = connection_id {
                route.connection_id = Some(connection_id);
            }
        }
    }

    pub(super) fn clear_extension_turn_state_for_closed_tab(&mut self, tab_id: usize) {
        let mut keys: Vec<(usize, u64)> = self
            .extension_tasks
            .turn_routes
            .keys()
            .filter(|(route_tab_id, _)| *route_tab_id == tab_id)
            .copied()
            .collect();
        keys.sort_by_key(|(_, turn_id)| *turn_id);
        for key @ (_, turn_id) in keys {
            let connection_id = self
                .extension_tasks
                .turn_routes
                .get(&key)
                .and_then(|route| route.connection_id);
            if !self.deny_extension_approvals_for_turn(tab_id, turn_id, true) {
                if let Some(connection_id) = connection_id {
                    self.disconnect_extension_connection(connection_id);
                }
            }
            if let Some(route) = self.extension_tasks.turn_routes.remove(&key) {
                if let Some(connection_id) = route.connection_id {
                    if !self.send_extension_frame(
                        connection_id,
                        extension_terminal_frame(
                            &route.task_id,
                            turn_id,
                            &ExtensionTurnTerminal::Interrupted,
                        ),
                    ) {
                        self.disconnect_extension_connection(connection_id);
                    }
                }
            }
        }
        // A pending approval may outlive an abnormally missing route entry.
        // Closing the source tab is the final authority boundary, so sweep by
        // immutable tab ownership and fail closed without inventing a wire
        // owner for those orphaned approvals.
        let mut orphaned_approval_ids: Vec<(u64, String)> = self
            .extension_tasks
            .pending_approvals
            .iter()
            .filter(|(_, approval)| approval.tab_id == tab_id)
            .map(|(approval_id, approval)| (approval.sequence, approval_id.clone()))
            .collect();
        orphaned_approval_ids.sort_by_key(|(sequence, _)| *sequence);
        for (_, approval_id) in orphaned_approval_ids {
            if let Some(pending) = self.extension_tasks.pending_approvals.remove(&approval_id) {
                let _ = pending.request.respond(Approval::Deny);
            }
        }
        self.extension_tasks
            .recent_turns
            .retain(|turn| turn.tab_id != tab_id);
    }

    pub(super) fn forward_extension_turn_event(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        event: &AppEvent,
    ) {
        if let AppEvent::TurnDone { result, .. } = event {
            if turn_id == 0 {
                return;
            }
            let task_id_from_tab = self
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .map(|tab| tab.session_id.clone());
            let route_connection = self
                .extension_tasks
                .turn_routes
                .get(&(tab_id, turn_id))
                .and_then(|route| route.connection_id);
            if !self.deny_extension_approvals_for_turn(tab_id, turn_id, true) {
                if let Some(connection_id) = route_connection {
                    self.disconnect_extension_connection(connection_id);
                }
            }
            let route = self.extension_tasks.turn_routes.remove(&(tab_id, turn_id));
            let terminal = match route.as_ref().map(|route| route.state) {
                Some(ExtensionTurnRouteState::InterruptRequested) => {
                    ExtensionTurnTerminal::Interrupted
                }
                _ => match result {
                    Ok(()) => ExtensionTurnTerminal::Completed,
                    Err(message) => ExtensionTurnTerminal::Failed(message.clone()),
                },
            };
            let task_id = route
                .as_ref()
                .map(|route| route.task_id.clone())
                .or(task_id_from_tab);
            if let (Some(route), Some(task_id)) = (route.as_ref(), task_id.as_ref()) {
                if let Some(connection_id) = route.connection_id {
                    if !self.send_extension_frame(
                        connection_id,
                        extension_terminal_frame(task_id, turn_id, &terminal),
                    ) {
                        self.disconnect_extension_connection(connection_id);
                    }
                }
            }
            if let Some(task_id) = task_id {
                self.extension_tasks
                    .remember_turn_terminal(tab_id, turn_id, task_id, terminal);
            }
            return;
        }

        let Some(route) = self
            .extension_tasks
            .turn_routes
            .get(&(tab_id, turn_id))
            .cloned()
        else {
            return;
        };
        if route.state != ExtensionTurnRouteState::Running {
            return;
        }
        let Some(connection_id) = route.connection_id else {
            return;
        };
        let turn_id_string = turn_id.to_string();
        let frame = match event {
            AppEvent::Agent {
                event: agent::stream::Event::TextDelta { delta },
                ..
            } => Some(TaskServerFrame::event(
                "message/delta",
                json!({
                    "taskId": route.task_id,
                    "turnId": turn_id_string,
                    "messageId": format!("{}:{turn_id}:assistant", route.task_id),
                    "delta": delta,
                }),
            )),
            AppEvent::Agent {
                event: agent::stream::Event::ToolUse { id, name, .. },
                ..
            } => {
                let public_identity = public_tool_identity(name);
                Some(TaskServerFrame::event(
                    "tool/started",
                    json!({
                        "taskId": route.task_id,
                        "turnId": turn_id_string,
                        "toolId": id,
                        "tool": public_identity,
                        "summary": public_identity,
                    }),
                ))
            }
            AppEvent::Agent {
                event: agent::stream::Event::ToolResult { id, ok, .. },
                ..
            } => Some(TaskServerFrame::event(
                "tool/completed",
                json!({
                    "taskId": route.task_id,
                    "turnId": turn_id_string,
                    "toolId": id,
                    "failed": !ok,
                }),
            )),
            _ => None,
        };
        if let Some(frame) = frame {
            if !self.send_extension_frame(connection_id, frame) {
                self.disconnect_extension_connection(connection_id);
            }
        }
    }

    pub(super) fn record_stale_extension_turn_done(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        result: &Result<(), String>,
    ) {
        let connection_id = self
            .extension_tasks
            .turn_routes
            .get(&(tab_id, turn_id))
            .and_then(|route| route.connection_id);
        let approvals_resolved = self.deny_extension_approvals_for_turn(tab_id, turn_id, true);
        if !approvals_resolved {
            if let Some(connection_id) = connection_id {
                self.disconnect_extension_connection(connection_id);
            }
        }
        let Some(route) = self.extension_tasks.turn_routes.remove(&(tab_id, turn_id)) else {
            return;
        };
        let terminal = match route.state {
            ExtensionTurnRouteState::InterruptRequested => ExtensionTurnTerminal::Interrupted,
            ExtensionTurnRouteState::Running => match result {
                Ok(()) => ExtensionTurnTerminal::Completed,
                Err(message) => ExtensionTurnTerminal::Failed(message.clone()),
            },
        };
        // The tab may already be on N+1, but a surviving exact route means the
        // remote still observed N start. Emit N's scoped terminal; the client
        // generation fence keeps it from mutating N+1.
        if approvals_resolved {
            if let Some(connection_id) = route.connection_id {
                if !self.send_extension_frame(
                    connection_id,
                    extension_terminal_frame(&route.task_id, turn_id, &terminal),
                ) {
                    self.disconnect_extension_connection(connection_id);
                }
            }
        }
        self.extension_tasks
            .remember_turn_terminal(tab_id, turn_id, route.task_id, terminal);
    }

    fn dispatch_extension_index_worker(
        &self,
        connection_id: u64,
        purpose: ExtensionIndexPurpose,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let Some(cancel) = self.extension_tasks.cancellation(connection_id) else {
            return;
        };
        let sessions = self.extension_tasks.sessions.clone();
        let worker_slots = self.extension_tasks.worker_slots.clone();
        let bridge_is_active = self.extension_bridge_liveness(connection_id);
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let Ok(_permit) = worker_slots.acquire_owned().await else {
                return;
            };
            let still_current: Arc<dyn Fn() -> bool + Send + Sync> =
                Arc::new(move || !cancel.load(Ordering::Acquire) && bridge_is_active());
            if !still_current() {
                return;
            }
            let result = match sessions.load_index_if(still_current.clone()).await {
                Ok(Some(index)) => Ok(index),
                Ok(None) => return,
                Err(error) => Err(extension_failure(error)),
            };
            if !still_current() {
                return;
            }
            let _ = tx.send(AppEvent::ExtensionTask(ExtensionTaskEvent::IndexReady {
                connection_id,
                purpose,
                result,
            }));
        });
    }

    fn send_extension_frame(&self, connection_id: u64, frame: TaskServerFrame) -> bool {
        #[cfg(test)]
        {
            // The production branch owns this field; keep it observed in the
            // test-only in-memory transport so `--all-targets -D warnings`
            // does not treat the real dependency as dead.
            let _ = &self.extension_browser;
            if self.extension_tasks.take_send_failure_for_test() {
                return false;
            }
            self.extension_tasks
                .sent_frames_for_test
                .lock()
                .expect("sent-frame test lock")
                .push((connection_id, frame));
            true
        }
        #[cfg(not(test))]
        {
            let sent = self
                .extension_browser
                .send_extension_task(connection_id, frame);
            if let Err(error) = &sent {
                tracing::debug!(
                    connection_id,
                    error = %error,
                    "extension task frame dropped"
                );
            }
            sent.is_ok()
        }
    }

    /// Direct adapter helper retained for focused unit tests. Production uses
    /// `dispatch_extension_inbound` and typed `AppEvent` completions so the TUI
    /// loop never awaits this work.
    #[cfg(test)]
    pub(super) async fn handle_extension_request(
        &mut self,
        connection_id: u64,
        frame: TaskClientFrame,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Value, ExtensionTaskError> {
        let TaskClientBody::Request { method, params, .. } = frame.body;
        let request = parse_extension_request(&method, params)?;
        let index = self.extension_tasks.sessions.load_index().await?;
        let pending =
            self.apply_extension_request_with_index(connection_id, request, &index, agent_tx)?;
        let mut snapshot = self.finish_snapshot_off_loop(pending).await?;
        self.refresh_extension_snapshot_live(&mut snapshot, connection_id);
        Ok(snapshot)
    }

    fn apply_extension_request_with_index(
        &mut self,
        connection_id: u64,
        request: ExtensionTaskRequest,
        index: &SessionIndex,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> Result<PendingSnapshot, ExtensionTaskError> {
        let mut preserve_selection = false;
        let requested_task_id = match request {
            ExtensionTaskRequest::SnapshotRead { task_id } => {
                let mut requested_task_id = task_id;
                if let Some(task_id) = requested_task_id.as_deref() {
                    match self.ensure_extension_task_open(task_id, connection_id, agent_tx, index) {
                        Ok(()) => {
                            self.extension_tasks
                                .current_task_by_connection
                                .insert(connection_id, task_id.to_string());
                        }
                        Err(error) if error.code() == "task_not_found" => {
                            // A persisted side-panel selection may outlive a
                            // deleted session. Fall back authoritatively.
                            self.extension_tasks
                                .current_task_by_connection
                                .remove(&connection_id);
                            requested_task_id = None;
                        }
                        Err(error) => return Err(error),
                    }
                }
                requested_task_id
            }
            ExtensionTaskRequest::Create => {
                let task_id = self.create_extension_task(connection_id, agent_tx);
                self.extension_tasks
                    .current_task_by_connection
                    .insert(connection_id, task_id.clone());
                Some(task_id)
            }
            ExtensionTaskRequest::Select { task_id } => {
                self.ensure_extension_task_open(&task_id, connection_id, agent_tx, index)?;
                self.extension_tasks
                    .current_task_by_connection
                    .insert(connection_id, task_id.clone());
                Some(task_id)
            }
            ExtensionTaskRequest::ModelSet { task_id, model } => {
                let Some(tab_idx) = self.tabs.iter().position(|tab| tab.session_id == task_id)
                else {
                    return Err(ExtensionTaskError::not_found(format!(
                        "task not found: {task_id}"
                    )));
                };
                preserve_selection = true;
                let current_model = self
                    .extension_tasks
                    .pending_task_metadata
                    .get(&task_id)
                    .map(|meta| meta.model.as_str())
                    .unwrap_or(&self.tabs[tab_idx].engine.model);
                if current_model == model {
                    Some(task_id)
                } else {
                    if self.tabs[tab_idx].is_busy() {
                        return Err(ExtensionTaskError::busy(format!("task is busy: {task_id}")));
                    }
                    if !self
                        .template
                        .model_ids()
                        .iter()
                        .any(|candidate| candidate == &model)
                    {
                        return Err(ExtensionTaskError::model_not_found(format!(
                            "model not found: {model}"
                        )));
                    }
                    let access = self.tabs[tab_idx].extension_access;
                    self.start_extension_task_reconfigure(
                        tab_idx,
                        connection_id,
                        model,
                        access,
                        true,
                        agent_tx,
                    )?;
                    Some(task_id)
                }
            }
            ExtensionTaskRequest::PermissionSet { task_id, mode } => {
                let Some(tab_idx) = self.tabs.iter().position(|tab| tab.session_id == task_id)
                else {
                    return Err(ExtensionTaskError::not_found(format!(
                        "task not found: {task_id}"
                    )));
                };
                preserve_selection = true;
                let current_access = self
                    .extension_tasks
                    .pending_task_metadata
                    .get(&task_id)
                    .map(|meta| meta.access)
                    .unwrap_or(self.tabs[tab_idx].extension_access);
                if current_access == mode {
                    Some(task_id)
                } else {
                    if self.tabs[tab_idx].is_busy() {
                        return Err(ExtensionTaskError::busy(format!("task is busy: {task_id}")));
                    }
                    let model = self.tabs[tab_idx].engine.model.clone();
                    self.start_extension_task_reconfigure(
                        tab_idx,
                        connection_id,
                        model,
                        mode,
                        false,
                        agent_tx,
                    )?;
                    Some(task_id)
                }
            }
            ExtensionTaskRequest::TurnStart { .. }
            | ExtensionTaskRequest::TurnInterrupt { .. }
            | ExtensionTaskRequest::ApprovalRespond { .. }
            | ExtensionTaskRequest::AttachmentBegin { .. }
            | ExtensionTaskRequest::AttachmentChunk { .. }
            | ExtensionTaskRequest::AttachmentFinish { .. }
            | ExtensionTaskRequest::AttachmentCancel { .. } => {
                return Err(ExtensionTaskError::internal(
                    "turn, approval, and attachment requests must be handled directly on the main loop",
                ));
            }
        };
        if preserve_selection {
            self.prepare_extension_snapshot_preserving_selection(
                connection_id,
                requested_task_id.as_deref(),
                index,
            )
        } else {
            self.prepare_extension_snapshot(connection_id, requested_task_id.as_deref(), index)
        }
    }

    fn prepare_extension_snapshot_preserving_selection(
        &mut self,
        connection_id: u64,
        _requested_task_id: Option<&str>,
        index: &SessionIndex,
    ) -> Result<PendingSnapshot, ExtensionTaskError> {
        let previous = self
            .extension_tasks
            .current_task_by_connection
            .get(&connection_id)
            .cloned();
        // A mutation response is still an authoritative snapshot of the
        // connection's selected task. The mutated background task appears in
        // `tasks` (including its switching state), but must not become current.
        let result = self.prepare_extension_snapshot(connection_id, None, index);
        match previous {
            Some(task_id) => {
                self.extension_tasks
                    .current_task_by_connection
                    .insert(connection_id, task_id);
            }
            None => {
                self.extension_tasks
                    .current_task_by_connection
                    .remove(&connection_id);
            }
        }
        result
    }

    fn start_extension_task_reconfigure(
        &mut self,
        tab_idx: usize,
        connection_id: u64,
        model: String,
        access: ToolAccessMode,
        persist_model: bool,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(), ExtensionTaskError> {
        let (store, carry, cwd, tab_id, task_id, title, plan_mode, seq) = {
            let tab = &self.tabs[tab_idx];
            let store = tab
                .engine
                .store
                .lock()
                .map_err(|_| ExtensionTaskError::internal("message store lock poisoned"))?
                .clone();
            (
                store,
                tab.engine.carry_state(),
                tab.engine.cwd.clone(),
                tab.id,
                tab.session_id.clone(),
                tab.title.clone(),
                tab.plan_mode,
                tab.reassemble_seq + 1,
            )
        };
        self.extension_tasks.pending_task_metadata.insert(
            task_id.clone(),
            PendingTaskMetadata {
                cwd: cwd.display().to_string(),
                model: model.clone(),
                access,
            },
        );
        {
            let tab = &mut self.tabs[tab_idx];
            tab.reassemble_seq = seq;
            tab.reassemble_pending = true;
            tab.mode = Mode::Switching;
            tab.active_tool_names.clear();
        }

        let clean_template = self.template.clone();
        let engine_template = clean_template
            .with_model(model.clone())
            .with_tool_access(access)
            .with_plan_mode(plan_mode);
        let sessions = self.extension_tasks.sessions.clone();
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let assembled = engine_template
                .assemble_tab_with_carry(Some(cwd.clone()), Some(tab_id.to_string()), carry)
                .await;
            let (result, failure_code) = match assembled {
                Err(error) => (Err(error.to_string()), Some("engine_assemble_failed")),
                Ok(engine) if persist_model => {
                    let persisted = sessions
                        .upsert(SessionMeta {
                            id: task_id,
                            title,
                            cwd: cwd.display().to_string(),
                            model: model.clone(),
                            updated_at: unix_timestamp_secs(),
                        })
                        .await;
                    match persisted {
                        Ok(()) => (
                            Ok(ReassembledEngine {
                                template: clean_template,
                                engine: engine.with_store(store),
                            }),
                            None,
                        ),
                        Err(error) => (Err(error.to_string()), Some("session_persist_failed")),
                    }
                }
                Ok(engine) => (
                    Ok(ReassembledEngine {
                        template: clean_template,
                        engine: engine.with_store(store),
                    }),
                    None,
                ),
            };
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id,
                seq,
                effect: ReassembleEffect::ExtensionReconfigure {
                    connection_id,
                    failure_code,
                    model,
                    access,
                },
                result,
            });
        });
        Ok(())
    }

    #[cfg(test)]
    async fn finish_snapshot_off_loop(
        &self,
        pending: PendingSnapshot,
    ) -> Result<Value, ExtensionTaskError> {
        let should_run: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
        tokio::task::spawn_blocking(move || complete_pending_snapshot(pending, should_run))
            .await
            .map_err(|error| {
                ExtensionTaskError::internal(format!("snapshot history worker failed: {error}"))
            })??
            .ok_or_else(|| ExtensionTaskError::internal("snapshot history worker cancelled"))
    }

    fn dispatch_extension_snapshot_worker(
        &self,
        connection_id: u64,
        purpose: ExtensionSnapshotPurpose,
        pending: PendingSnapshot,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        let Some(cancel) = self.extension_tasks.cancellation(connection_id) else {
            return;
        };
        let worker_slots = self.extension_tasks.worker_slots.clone();
        let bridge_is_active = self.extension_bridge_liveness(connection_id);
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let Ok(_permit) = worker_slots.acquire_owned().await else {
                return;
            };
            let still_current: Arc<dyn Fn() -> bool + Send + Sync> =
                Arc::new(move || !cancel.load(Ordering::Acquire) && bridge_is_active());
            if !still_current() {
                return;
            }
            let worker_current = still_current.clone();
            let result = tokio::task::spawn_blocking(move || {
                complete_pending_snapshot(pending, worker_current).map_err(extension_failure)
            })
            .await
            .map_err(|error| ExtensionTaskFailure {
                code: "internal_error".into(),
                message: format!("snapshot history worker failed: {error}"),
            })
            .and_then(|result| result);
            let result = match result {
                Ok(Some(snapshot)) if still_current() => Ok(snapshot),
                Ok(_) => return,
                Err(error) if still_current() => Err(error),
                Err(_) => return,
            };
            let _ = tx.send(AppEvent::ExtensionTask(ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose,
                result,
            }));
        });
    }

    pub(super) fn handle_extension_task_event(
        &mut self,
        event: ExtensionTaskEvent,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        match event {
            ExtensionTaskEvent::IndexReady {
                connection_id,
                purpose:
                    ExtensionIndexPurpose::Request {
                        request_id,
                        request,
                    },
                result,
            } => {
                if !self.fence_inactive_extension_connection(connection_id)
                    || !self
                        .extension_tasks
                        .request_is_pending(connection_id, &request_id)
                {
                    return;
                }
                let index = match result {
                    Ok(index) => index,
                    Err(error) => {
                        self.extension_tasks
                            .finish_request(connection_id, &request_id);
                        self.send_extension_frame(
                            connection_id,
                            TaskServerFrame::error(request_id, error.code, error.message),
                        );
                        return;
                    }
                };
                let rebind_orphan_routes =
                    matches!(&request, ExtensionTaskRequest::SnapshotRead { .. });
                match self.apply_extension_request_with_index(
                    connection_id,
                    request,
                    &index,
                    agent_tx,
                ) {
                    Ok(pending) => self.dispatch_extension_snapshot_worker(
                        connection_id,
                        ExtensionSnapshotPurpose::Response {
                            request_id,
                            rebind_orphan_routes,
                        },
                        pending,
                        agent_tx,
                    ),
                    Err(error) => {
                        self.extension_tasks
                            .finish_request(connection_id, &request_id);
                        self.send_extension_frame(
                            connection_id,
                            TaskServerFrame::error(request_id, error.code(), error.to_string()),
                        );
                    }
                }
            }
            ExtensionTaskEvent::IndexReady {
                connection_id,
                purpose: ExtensionIndexPurpose::Completion { failure },
                result,
            } => {
                if !self.fence_inactive_extension_connection(connection_id)
                    || !self
                        .extension_tasks
                        .current_task_by_connection
                        .contains_key(&connection_id)
                {
                    return;
                }
                match result {
                    Ok(index) => match self.prepare_extension_snapshot(connection_id, None, &index)
                    {
                        Ok(pending) => self.dispatch_extension_snapshot_worker(
                            connection_id,
                            ExtensionSnapshotPurpose::Completion { failure },
                            pending,
                            agent_tx,
                        ),
                        Err(error) => self.send_completion_snapshot_error(
                            connection_id,
                            extension_failure(error),
                            failure,
                        ),
                    },
                    Err(error) => {
                        self.send_completion_snapshot_error(connection_id, error, failure)
                    }
                }
            }
            ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose:
                    ExtensionSnapshotPurpose::Response {
                        request_id,
                        rebind_orphan_routes,
                    },
                mut result,
            } => {
                if !self.fence_inactive_extension_connection(connection_id)
                    || !self
                        .extension_tasks
                        .request_is_pending(connection_id, &request_id)
                {
                    return;
                }
                if let Ok(snapshot) = result.as_mut() {
                    self.refresh_extension_snapshot_live(snapshot, connection_id);
                }
                self.extension_tasks
                    .finish_request(connection_id, &request_id);
                let successful_snapshot = result.as_ref().ok().cloned();
                let frame = match result {
                    Ok(snapshot) => TaskServerFrame::response(request_id, snapshot),
                    Err(error) => TaskServerFrame::error(request_id, error.code, error.message),
                };
                let response_enqueued = self.send_extension_frame(connection_id, frame);
                if response_enqueued && rebind_orphan_routes {
                    if let Some(snapshot) = successful_snapshot.as_ref() {
                        self.rebind_orphan_extension_turns(connection_id, snapshot);
                    }
                } else if !response_enqueued {
                    self.disconnect_extension_connection(connection_id);
                }
            }
            ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose: ExtensionSnapshotPurpose::Completion { failure },
                result,
            } => {
                if !self.fence_inactive_extension_connection(connection_id)
                    || !self
                        .extension_tasks
                        .current_task_by_connection
                        .contains_key(&connection_id)
                {
                    return;
                }
                match result {
                    Ok(mut snapshot) => {
                        self.refresh_extension_snapshot_live(&mut snapshot, connection_id);
                        let snapshot = TaskServerFrame::event("snapshot", snapshot);
                        for frame in extension_completion_frames(
                            snapshot,
                            failure
                                .as_ref()
                                .map(|(code, message)| (code.as_str(), message.as_str())),
                        ) {
                            if !self.send_extension_frame(connection_id, frame) {
                                self.disconnect_extension_connection(connection_id);
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        self.send_completion_snapshot_error(connection_id, error, failure)
                    }
                }
            }
        }
    }

    fn send_completion_snapshot_error(
        &mut self,
        connection_id: u64,
        snapshot_error: ExtensionTaskFailure,
        completion_failure: Option<(String, String)>,
    ) {
        if !self.send_extension_frame(
            connection_id,
            extension_connection_error_frame(&snapshot_error.code, &snapshot_error.message),
        ) {
            self.disconnect_extension_connection(connection_id);
            return;
        }
        if let Some((code, message)) = completion_failure {
            if !self.send_extension_frame(
                connection_id,
                extension_connection_error_frame(&code, &message),
            ) {
                self.disconnect_extension_connection(connection_id);
            }
        }
    }

    pub(super) fn dispatch_extension_completions(
        &mut self,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        for completion in self.extension_tasks.take_completions() {
            for connection_id in self.extension_tasks.completion_connections() {
                if self.extension_connection_is_current(connection_id) {
                    self.dispatch_extension_index_worker(
                        connection_id,
                        ExtensionIndexPurpose::Completion {
                            failure: completion.failure.clone(),
                        },
                        agent_tx,
                    );
                }
            }
        }
    }

    fn create_extension_task(
        &mut self,
        connection_id: u64,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> String {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let session_id = Uuid::new_v4().simple().to_string();
        let placeholder = self.tabs[self.active].engine.clone();
        let intended_model = self
            .template
            .model()
            .unwrap_or(&placeholder.model)
            .to_string();
        let intended_access = self.template.tool_access();
        self.extension_tasks.pending_task_metadata.insert(
            session_id.clone(),
            PendingTaskMetadata {
                cwd: self.template.cwd().display().to_string(),
                model: intended_model.clone(),
                access: intended_access,
            },
        );
        let mut tab = SessionTab::new(tab_id, placeholder, session_id.clone());
        tab.extension_access = intended_access;
        tab.title = "New task".into();
        tab.reassemble_pending = true;
        tab.reassemble_seq = 1;
        tab.mode = Mode::Switching;
        self.tabs.push(tab);

        let template = self.template.clone();
        let sessions = self.extension_tasks.sessions.clone();
        let tx = agent_tx.clone();
        let session_for_task = session_id.clone();
        tokio::spawn(async move {
            let prepared = async {
                let path = sessions.session_path(&session_for_task)?;
                Session::save(path, &MessageStore::new())
                    .await
                    .map_err(|error| ExtensionTaskError::internal(error.to_string()))?;
                sessions
                    .upsert(SessionMeta {
                        id: session_for_task.clone(),
                        title: "New task".into(),
                        cwd: template.cwd().display().to_string(),
                        model: intended_model,
                        updated_at: unix_timestamp_secs(),
                    })
                    .await?;
                Ok::<(), ExtensionTaskError>(())
            }
            .await;
            let (result, failure_code) = match prepared {
                Err(error) => (Err(error.to_string()), Some("session_persist_failed")),
                Ok(()) => match template.assemble_tab(None, Some(tab_id.to_string())).await {
                    Ok(engine) => (Ok(ReassembledEngine { template, engine }), None),
                    Err(error) => (Err(error.to_string()), Some("engine_assemble_failed")),
                },
            };
            if result.is_err() {
                if let Err(error) = sessions.remove(&session_for_task).await {
                    tracing::warn!(
                        task_id = %session_for_task,
                        error = %error,
                        "failed to clean up extension-created session"
                    );
                }
            }
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id,
                seq: 1,
                effect: ReassembleEffect::ExtensionNewTab {
                    connection_id,
                    failure_code,
                },
                result,
            });
        });
        session_id
    }

    fn ensure_extension_task_open(
        &mut self,
        task_id: &str,
        connection_id: u64,
        agent_tx: &mpsc::UnboundedSender<AppEvent>,
        index: &SessionIndex,
    ) -> Result<(), ExtensionTaskError> {
        if self.tabs.iter().any(|tab| tab.session_id == task_id) {
            return Ok(());
        }
        let meta = index
            .sessions
            .iter()
            .find(|meta| meta.id == task_id)
            .cloned()
            .ok_or_else(|| ExtensionTaskError::not_found(format!("task not found: {task_id}")))?;
        self.extension_tasks.pending_task_metadata.insert(
            meta.id.clone(),
            PendingTaskMetadata {
                cwd: meta.cwd.clone(),
                model: meta.model.clone(),
                access: ToolAccessMode::Prompt,
            },
        );
        let path = self.extension_tasks.sessions.session_path(task_id)?;
        let saved_cwd = PathBuf::from(&meta.cwd);
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let placeholder = self.tabs[self.active].engine.clone();
        let mut tab = SessionTab::new(tab_id, placeholder, meta.id.clone());
        tab.extension_access = ToolAccessMode::Prompt;
        tab.title = meta.title;
        tab.titled = true;
        tab.reassemble_pending = true;
        tab.reassemble_seq = 1;
        tab.mode = Mode::Switching;
        self.tabs.push(tab);

        let clean_template = self.template.clone();
        let engine_template = clean_template
            .with_model(meta.model.clone())
            .with_tool_access(ToolAccessMode::Prompt)
            .with_plan_mode(false);
        let tx = agent_tx.clone();
        tokio::spawn(async move {
            let cwd_override = match tokio::fs::metadata(&saved_cwd).await {
                Ok(metadata) if metadata.is_dir() => Some(saved_cwd),
                _ => None,
            };
            let (store, engine) = tokio::join!(
                Session::load(path),
                engine_template.assemble_tab(cwd_override, Some(tab_id.to_string()))
            );
            let (result, failure_code) = match (store, engine) {
                (Ok(store), Ok(engine)) => (
                    Ok(ReassembledEngine {
                        template: clean_template,
                        engine: engine.with_store(store),
                    }),
                    None,
                ),
                (Err(error), _) => (
                    Err(format!("load failed: {error}")),
                    Some("session_load_failed"),
                ),
                (_, Err(error)) => (
                    Err(format!("assemble failed: {error}")),
                    Some("engine_assemble_failed"),
                ),
            };
            let _ = tx.send(AppEvent::ReassembleDone {
                tab_id,
                seq: 1,
                effect: ReassembleEffect::ExtensionResumeTab {
                    connection_id,
                    failure_code,
                },
                result,
            });
        });
        Ok(())
    }

    #[cfg(test)]
    async fn extension_snapshot(
        &mut self,
        connection_id: u64,
        requested_task_id: Option<&str>,
    ) -> Result<Value, ExtensionTaskError> {
        let index = self.extension_tasks.sessions.load_index().await?;
        let pending = self.prepare_extension_snapshot(connection_id, requested_task_id, &index)?;
        let mut snapshot = self.finish_snapshot_off_loop(pending).await?;
        self.refresh_extension_snapshot_live(&mut snapshot, connection_id);
        Ok(snapshot)
    }

    fn prepare_extension_snapshot(
        &mut self,
        connection_id: u64,
        requested_task_id: Option<&str>,
        index: &SessionIndex,
    ) -> Result<PendingSnapshot, ExtensionTaskError> {
        let tasks = self.extension_task_summaries_with_index(index);
        let known_ids: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        let selected = requested_task_id
            .map(str::to_string)
            .or_else(|| {
                self.extension_tasks
                    .current_task_by_connection
                    .get(&connection_id)
                    .filter(|id| known_ids.contains(id.as_str()))
                    .cloned()
            })
            .or_else(|| self.tabs.get(self.active).map(|tab| tab.session_id.clone()))
            .ok_or_else(|| ExtensionTaskError::internal("no session is available"))?;
        if !known_ids.contains(selected.as_str()) {
            return Err(ExtensionTaskError::not_found(format!(
                "task not found: {selected}"
            )));
        }
        self.extension_tasks
            .current_task_by_connection
            .insert(connection_id, selected.clone());

        // A background placeholder borrows another tab's engine. Never read
        // that store: it belongs to the terminal tab and would leak history.
        let history_source = self
            .tabs
            .iter()
            .find(|tab| tab.session_id == selected && !tab.reassemble_pending)
            .map(|tab| (selected.clone(), tab.engine.store.clone()));
        let selected_task = tasks
            .iter()
            .find(|task| task.id == selected)
            .expect("selected task was validated");
        let workspace_path = selected_task.cwd.clone();
        let workspace_name = Path::new(&workspace_path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&workspace_path)
            .to_string();

        Ok(PendingSnapshot {
            workspace: WorkspaceSummary {
                name: workspace_name,
                path: workspace_path,
            },
            tasks,
            current_task_id: selected,
            models: self.template.model_ids(),
            history_source,
        })
    }

    fn refresh_extension_snapshot_live(&self, snapshot: &mut Value, connection_id: u64) {
        let live: HashMap<String, Value> = self
            .extension_task_summaries_with_index(&SessionIndex::default())
            .into_iter()
            .filter_map(|summary| {
                let id = summary.id.clone();
                serde_json::to_value(summary).ok().map(|value| (id, value))
            })
            .collect();
        let Some(tasks) = snapshot.get_mut("tasks").and_then(Value::as_array_mut) else {
            return;
        };
        for task in tasks {
            let Some(task_id) = task.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(current) = live.get(task_id) {
                *task = current.clone();
            }
        }

        let mut approvals: Vec<(u64, Value)> = self
            .extension_tasks
            .pending_approvals
            .iter()
            .filter_map(|(approval_id, approval)| {
                if approval.connection_id != connection_id {
                    return None;
                }
                let route_live = self
                    .extension_tasks
                    .turn_routes
                    .get(&(approval.tab_id, approval.turn_id))
                    .is_some_and(|route| {
                        route.task_id == approval.task_id
                            && route.connection_id == Some(connection_id)
                            && route.state == ExtensionTurnRouteState::Running
                    });
                let tab_live = self
                    .tabs
                    .iter()
                    .find(|tab| tab.id == approval.tab_id)
                    .is_some_and(|tab| {
                        tab.session_id == approval.task_id
                            && tab.active_turn_id == approval.turn_id
                            && tab.turn_abort.is_some()
                            && tab.draining_turn_id.is_none()
                            && !tab.reassemble_pending
                    });
                (route_live && tab_live).then(|| {
                    (
                        approval.sequence,
                        json!({
                            "id": approval_id,
                            "taskId": approval.task_id,
                            "turnId": approval.turn_id.to_string(),
                            "approvalId": approval_id,
                            "status": "pending",
                            "tool": approval.public_summary,
                            "summary": approval.public_summary,
                        }),
                    )
                })
            })
            .collect();
        approvals.sort_by_key(|(sequence, _)| *sequence);
        if let Some(snapshot) = snapshot.as_object_mut() {
            snapshot.insert(
                "approvals".into(),
                Value::Array(
                    approvals
                        .into_iter()
                        .map(|(_, approval)| approval)
                        .collect(),
                ),
            );
        }
    }

    fn rebind_orphan_extension_turns(&mut self, connection_id: u64, snapshot: &Value) {
        let live_turns: HashMap<String, (String, String)> = snapshot
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| {
                Some((
                    task.get("id")?.as_str()?.to_string(),
                    (
                        task.get("status")?.as_str()?.to_string(),
                        task.get("activeTurnId")?.as_str()?.to_string(),
                    ),
                ))
            })
            .collect();
        for ((_, turn_id), route) in self.extension_tasks.turn_routes.iter_mut() {
            let expected_status = match route.state {
                ExtensionTurnRouteState::Running => "running",
                ExtensionTurnRouteState::InterruptRequested => "stopping",
            };
            let turn_id = turn_id.to_string();
            let exact_live =
                live_turns
                    .get(&route.task_id)
                    .is_some_and(|(status, active_turn_id)| {
                        status == expected_status && active_turn_id == &turn_id
                    });
            if route.connection_id.is_none() && exact_live {
                route.connection_id = Some(connection_id);
            }
        }
    }

    fn extension_task_summaries_with_index(&self, index: &SessionIndex) -> Vec<TaskSummary> {
        let mut summaries: Vec<TaskSummary> = self
            .tabs
            .iter()
            .map(|tab| {
                let status = if tab.turn_abort.is_some() {
                    "running"
                } else if tab.draining_turn_id.is_some() {
                    "stopping"
                } else if tab.reassemble_pending {
                    "switching"
                } else {
                    "idle"
                };
                TaskSummary {
                    // A restoring tab temporarily borrows the terminal tab's
                    // engine. Its intended cwd/model come from SessionMeta until
                    // the real engine is installed.
                    id: tab.session_id.clone(),
                    title: tab.title.clone(),
                    cwd: self
                        .extension_tasks
                        .pending_task_metadata
                        .get(&tab.session_id)
                        .map(|meta| meta.cwd.clone())
                        .unwrap_or_else(|| tab.engine.cwd.display().to_string()),
                    model: self
                        .extension_tasks
                        .pending_task_metadata
                        .get(&tab.session_id)
                        .map(|meta| meta.model.clone())
                        .unwrap_or_else(|| tab.engine.model.clone()),
                    access: self
                        .extension_tasks
                        .pending_task_metadata
                        .get(&tab.session_id)
                        .map(|meta| meta.access)
                        .unwrap_or(tab.extension_access),
                    status,
                    active_turn_id: match status {
                        "running" if tab.active_turn_id != 0 => {
                            Some(tab.active_turn_id.to_string())
                        }
                        "stopping" => tab.draining_turn_id.map(|turn_id| turn_id.to_string()),
                        _ => None,
                    },
                }
            })
            .collect();
        let mut seen: HashSet<String> = summaries.iter().map(|task| task.id.clone()).collect();
        for meta in index.newest_first() {
            if seen.insert(meta.id.clone()) {
                summaries.push(TaskSummary {
                    id: meta.id.clone(),
                    title: meta.title.clone(),
                    cwd: meta.cwd.clone(),
                    model: meta.model.clone(),
                    access: ToolAccessMode::Prompt,
                    status: "idle",
                    active_turn_id: None,
                });
            }
        }
        summaries
    }

    #[cfg(test)]
    async fn extension_snapshot_event(&mut self, connection_id: u64) -> TaskServerFrame {
        match self.extension_snapshot(connection_id, None).await {
            Ok(snapshot) => TaskServerFrame::event("snapshot", snapshot),
            Err(error) => TaskServerFrame::event(
                "connection/error",
                json!({"code": error.code(), "message": error.to_string()}),
            ),
        }
    }
}

fn complete_pending_snapshot(
    pending: PendingSnapshot,
    should_run: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Option<Value>, ExtensionTaskError> {
    if !should_run() {
        return Ok(None);
    }
    let history = if let Some((task_id, store)) = &pending.history_source {
        let snapshot = {
            let store = store
                .lock()
                .map_err(|_| ExtensionTaskError::internal("message store lock poisoned"))?;
            if !should_run() {
                return Ok(None);
            }
            store.clone()
        };
        // Release the live engine store before walking or serializing history.
        if !should_run() {
            return Ok(None);
        }
        history_from_store(task_id, &snapshot)
    } else {
        SnapshotHistory::default()
    };
    if !should_run() {
        return Ok(None);
    }
    Ok(Some(pending.finish(history)))
}

fn extension_failure(error: ExtensionTaskError) -> ExtensionTaskFailure {
    ExtensionTaskFailure {
        code: error.code().to_string(),
        message: error.to_string(),
    }
}

fn parse_extension_request(
    method: &str,
    params: Value,
) -> Result<ExtensionTaskRequest, ExtensionTaskError> {
    match method {
        "snapshot/read" => {
            let params: SnapshotParams = parse_params(params)?;
            if let Some(task_id) = params.task_id.as_deref() {
                validate_task_id(task_id)?;
            }
            Ok(ExtensionTaskRequest::SnapshotRead {
                task_id: params.task_id,
            })
        }
        "task/create" => {
            let _: CreateParams = parse_params(params)?;
            Ok(ExtensionTaskRequest::Create)
        }
        "task/select" => {
            let params: SelectParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            Ok(ExtensionTaskRequest::Select {
                task_id: params.task_id,
            })
        }
        "model/set" => {
            let params: ModelSetParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            Ok(ExtensionTaskRequest::ModelSet {
                task_id: params.task_id,
                model: params.model,
            })
        }
        "permission/set" => {
            let params: PermissionSetParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            Ok(ExtensionTaskRequest::PermissionSet {
                task_id: params.task_id,
                mode: params.mode,
            })
        }
        "turn/start" => {
            let params: TurnStartParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            if params.input.trim().is_empty() && params.attachment_ids.is_empty() {
                return Err(ExtensionTaskError::invalid_params(
                    "input or attachmentIds must be non-empty",
                ));
            }
            if params
                .attachment_ids
                .iter()
                .any(|attachment_id| attachment_id.trim().is_empty())
            {
                return Err(ExtensionTaskError::invalid_params(
                    "attachmentIds must contain non-empty strings",
                ));
            }
            Ok(ExtensionTaskRequest::TurnStart {
                task_id: params.task_id,
                input: params.input,
                attachment_ids: params.attachment_ids,
            })
        }
        "attachment/begin" => {
            let params: AttachmentBeginParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            if params.name.trim().is_empty() {
                return Err(ExtensionTaskError::invalid_params(
                    "name must be a non-empty string",
                ));
            }
            if params.media_type.trim().is_empty() {
                return Err(ExtensionTaskError::invalid_params(
                    "mediaType must be a non-empty string",
                ));
            }
            Ok(ExtensionTaskRequest::AttachmentBegin {
                task_id: params.task_id,
                name: params.name,
                media_type: params.media_type,
                size: params.size,
            })
        }
        "attachment/chunk" => {
            let params: AttachmentChunkParams = parse_params(params)?;
            validate_upload_id(&params.upload_id)?;
            let max_encoded_len = super::extension_attachments::MAX_RAW_CHUNK_BYTES.div_ceil(3) * 4;
            if params.data.len() > max_encoded_len {
                return Err(ExtensionTaskError::invalid_params(
                    "attachment chunk exceeds the encoded size limit",
                ));
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(params.data.as_bytes())
                .map_err(|_| ExtensionTaskError::invalid_params("data must be canonical base64"))?;
            if base64::engine::general_purpose::STANDARD.encode(&data) != params.data {
                return Err(ExtensionTaskError::invalid_params(
                    "data must be canonical base64",
                ));
            }
            Ok(ExtensionTaskRequest::AttachmentChunk {
                upload_id: params.upload_id,
                sequence: params.sequence,
                data,
            })
        }
        "attachment/finish" | "attachment/cancel" => {
            let params: AttachmentUploadParams = parse_params(params)?;
            validate_upload_id(&params.upload_id)?;
            if method == "attachment/finish" {
                Ok(ExtensionTaskRequest::AttachmentFinish {
                    upload_id: params.upload_id,
                })
            } else {
                Ok(ExtensionTaskRequest::AttachmentCancel {
                    upload_id: params.upload_id,
                })
            }
        }
        "turn/interrupt" => {
            let params: TurnInterruptParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            Ok(ExtensionTaskRequest::TurnInterrupt {
                task_id: params.task_id,
                turn_id: parse_turn_id(&params.turn_id)?,
            })
        }
        "approval/respond" => {
            let params: ApprovalRespondParams = parse_params(params)?;
            validate_task_id(&params.task_id)?;
            if params.approval_id.trim().is_empty() {
                return Err(ExtensionTaskError::invalid_params(
                    "approvalId must be a non-empty string",
                ));
            }
            Ok(ExtensionTaskRequest::ApprovalRespond {
                task_id: params.task_id,
                turn_id: parse_turn_id(&params.turn_id)?,
                approval_id: params.approval_id,
                decision: parse_approval_decision(&params.decision)?,
            })
        }
        _ => Err(ExtensionTaskError {
            code: "method_not_found",
            message: format!("unsupported task method: {method}"),
        }),
    }
}

fn extension_connection_error_frame(code: &str, message: &str) -> TaskServerFrame {
    TaskServerFrame::event(
        "connection/error",
        json!({"code": code, "message": message}),
    )
}

fn public_tool_identity(raw_name: &str) -> String {
    let display = if let Some(rest) = raw_name.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        if let (Some(server), Some(tool)) = (parts.next(), parts.next()) {
            format!("MCP {server}.{tool}")
        } else {
            format!("Tool {raw_name}")
        }
    } else {
        format!("Tool {raw_name}")
    };

    let mut sanitized = String::with_capacity(display.len());
    for character in display.chars() {
        let wire_control = matches!(character, '\u{2028}' | '\u{2029}')
            || ('\u{0000}'..='\u{001f}').contains(&character)
            || ('\u{007f}'..='\u{009f}').contains(&character);
        if wire_control {
            if !sanitized.ends_with(' ') {
                sanitized.push(' ');
            }
        } else {
            sanitized.push(character);
        }
    }
    let mut sanitized = sanitized.trim().to_string();
    if sanitized.is_empty() {
        sanitized.push_str("Tool");
    }
    const MAX_PUBLIC_TOOL_BYTES: usize = 200;
    if sanitized.len() > MAX_PUBLIC_TOOL_BYTES {
        const ELLIPSIS: &str = "…";
        let mut end = MAX_PUBLIC_TOOL_BYTES - ELLIPSIS.len();
        while !sanitized.is_char_boundary(end) {
            end -= 1;
        }
        sanitized.truncate(end);
        sanitized.push_str(ELLIPSIS);
    }
    sanitized
}

fn parse_approval_decision(
    decision: &str,
) -> Result<ExtensionApprovalDecision, ExtensionTaskError> {
    match decision {
        "allow" => Ok(ExtensionApprovalDecision::Allow),
        "allowAlways" => Ok(ExtensionApprovalDecision::AllowAlways),
        "deny" => Ok(ExtensionApprovalDecision::Deny),
        _ => Err(ExtensionTaskError::invalid_params(
            "decision must be one of: allow, allowAlways, deny",
        )),
    }
}

fn extension_approval_decision_wire(decision: ExtensionApprovalDecision) -> &'static str {
    match decision {
        ExtensionApprovalDecision::Allow => "allow",
        ExtensionApprovalDecision::AllowAlways => "allowAlways",
        ExtensionApprovalDecision::Deny => "deny",
    }
}

fn extension_terminal_frame(
    task_id: &str,
    turn_id: u64,
    terminal: &ExtensionTurnTerminal,
) -> TaskServerFrame {
    let turn_id = turn_id.to_string();
    match terminal {
        ExtensionTurnTerminal::Completed => TaskServerFrame::event(
            "turn/completed",
            json!({"taskId": task_id, "turnId": turn_id, "status": "completed"}),
        ),
        ExtensionTurnTerminal::Failed(message) => TaskServerFrame::event(
            "turn/failed",
            json!({
                "taskId": task_id,
                "turnId": turn_id,
                "status": "failed",
                "message": message,
            }),
        ),
        ExtensionTurnTerminal::Interrupted => TaskServerFrame::event(
            "turn/interrupted",
            json!({"taskId": task_id, "turnId": turn_id, "status": "interrupted"}),
        ),
    }
}

fn extension_completion_frames(
    snapshot: TaskServerFrame,
    failure: Option<(&str, &str)>,
) -> Vec<TaskServerFrame> {
    let mut frames = vec![snapshot];
    if let Some((code, message)) = failure {
        frames.push(extension_connection_error_frame(code, message));
    }
    frames
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionTaskError> {
    serde_json::from_value(params)
        .map_err(|error| ExtensionTaskError::invalid_params(error.to_string()))
}

fn validate_task_id(task_id: &str) -> Result<(), ExtensionTaskError> {
    if task_id.trim().is_empty() {
        Err(ExtensionTaskError::invalid_params(
            "taskId must be a non-empty string",
        ))
    } else {
        Ok(())
    }
}

fn validate_upload_id(upload_id: &str) -> Result<(), ExtensionTaskError> {
    if upload_id.trim().is_empty() {
        Err(ExtensionTaskError::invalid_params(
            "uploadId must be a non-empty string",
        ))
    } else {
        Ok(())
    }
}

fn parse_turn_id(turn_id: &str) -> Result<u64, ExtensionTaskError> {
    if turn_id.is_empty()
        || turn_id.starts_with('0')
        || !turn_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ExtensionTaskError::invalid_params(
            "turnId must be a canonical decimal u64 greater than zero",
        ));
    }
    turn_id.parse::<u64>().map_err(|_| {
        ExtensionTaskError::invalid_params(
            "turnId must be a canonical decimal u64 greater than zero",
        )
    })
}

fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn history_from_store(task_id: &str, store: &MessageStore) -> SnapshotHistory {
    let mut history = SnapshotHistory::default();
    let mut tool_indexes = HashMap::<String, usize>::new();
    for message in store.iter() {
        match message {
            Message::User { header, content } => {
                push_history_text(
                    &mut history,
                    task_id,
                    header.uuid.to_string(),
                    "user",
                    content,
                );
                for block in content {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        is_error,
                        ..
                    } = block
                    {
                        if let Some(index) = tool_indexes.get(tool_use_id).copied() {
                            let tool = &mut history.tools[index];
                            tool.status = if *is_error { "failed" } else { "completed" };
                            tool.summary = if *is_error {
                                format!("{} failed", tool.name)
                            } else {
                                format!("{} completed", tool.name)
                            };
                        }
                    }
                }
            }
            Message::Assistant { header, content } => {
                push_history_text(
                    &mut history,
                    task_id,
                    header.uuid.to_string(),
                    "assistant",
                    content,
                );
                for block in content {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        let public_identity = public_tool_identity(name);
                        tool_indexes.insert(id.clone(), history.tools.len());
                        history.tools.push(HistoryTool {
                            id: id.clone(),
                            task_id: task_id.to_string(),
                            name: public_identity.clone(),
                            summary: public_identity,
                            status: "running",
                        });
                    }
                }
            }
            Message::System { .. } | Message::Progress { .. } | Message::Tombstone { .. } => {}
        }
    }
    history
}

fn push_history_text(
    history: &mut SnapshotHistory,
    task_id: &str,
    id: String,
    role: &'static str,
    content: &[ContentBlock],
) {
    let text = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } if !text.trim().is_empty() => {
                Some(super::stored_user_text_for_display(text))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        history.messages.push(HistoryMessage {
            id,
            task_id: task_id.to_string(),
            role,
            text,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use agent::abort::AbortController;
    use agent::message::{ContentBlock, Header, Message, MessageStore, ToolResultContent};
    use agent::session::Session;
    use serde_json::Value;
    use tokio::sync::mpsc;
    use zode_core::approval::{Approval, ApprovalQueue};
    use zode_core::browser::bridge::{
        TaskClientFrame, TaskInbound, TaskInboundKind, TaskServerFrame,
    };
    use zode_core::config::{
        ModelOverride, NoemaSettings, ProviderConfig, ProviderKind, ZodeConfig,
    };
    use zode_core::session_meta::{SessionIndex, SessionMeta};
    use zode_core::EngineTemplate;

    use super::super::extension_attachments::{BeginUpload, UploadError, UPLOAD_TTL};
    use super::super::{AppEvent, Mode, ReassembleEffect, SessionTab, TuiApp, UiConfig};
    use super::{EXTENSION_PENDING_REQUEST_LIMIT, EXTENSION_WORKER_LIMIT};

    async fn make_test_app() -> (
        TuiApp,
        mpsc::UnboundedSender<AppEvent>,
        mpsc::UnboundedReceiver<AppEvent>,
        tempfile::TempDir,
    ) {
        let (app, tx, rx, cwd, _approval_queue) = make_test_app_with_approval_queue().await;
        (app, tx, rx, cwd)
    }

    async fn make_test_app_with_approval_queue() -> (
        TuiApp,
        mpsc::UnboundedSender<AppEvent>,
        mpsc::UnboundedReceiver<AppEvent>,
        tempfile::TempDir,
        ApprovalQueue,
    ) {
        let cwd = tempfile::tempdir().unwrap();
        let mut provider = ProviderConfig {
            r#type: Some(ProviderKind::Ollama),
            base_url: Some("http://localhost:11434".to_string()),
            model: Some("test-model".to_string()),
            ..Default::default()
        };
        provider
            .models
            .insert("other-model".into(), ModelOverride::default());
        let mut cfg = ZodeConfig {
            provider: provider.clone(),
            noema: NoemaSettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        cfg.providers.insert("local".into(), provider);
        let (approval_queue, approval_rx) = zode_core::approval::approval_queue();
        let approval_queue_for_test = approval_queue.clone();
        let (question_queue, question_rx) = zode_core::question::question_queue();
        let op_question_queue = question_queue.clone();
        let template = EngineTemplate::new(
            cfg,
            cwd.path().to_path_buf(),
            Some(approval_queue),
            false,
            None,
            "2026-07-13".to_string(),
        )
        .with_question_queue(Some(question_queue));
        let engine = template.assemble().await.unwrap();
        let initial_access = template.tool_access();
        let mut app = TuiApp::new(
            engine,
            template,
            UiConfig {
                theme_id: None,
                yolo: false,
                initial_access,
                sandbox: false,
                provider_names: Vec::new(),
                needs_setup: false,
            },
            approval_rx,
            question_rx,
            op_question_queue,
            None,
        );
        app.extension_tasks
            .set_session_root_for_test(cwd.path().join("sessions"));
        let (agent_tx, agent_rx) = mpsc::unbounded_channel();
        (app, agent_tx, agent_rx, cwd, approval_queue_for_test)
    }

    async fn request_from_app_queue(
        app: &mut TuiApp,
        queue: ApprovalQueue,
        source: String,
    ) -> (
        zode_core::approval::ApprovalRequest,
        tokio::task::JoinHandle<Approval>,
    ) {
        let pending = tokio::spawn(async move {
            queue
                .request(
                    "Bash",
                    &serde_json::json!({"command": "echo approval"}),
                    Some(source),
                )
                .await
        });
        let request = app
            .approval_rx
            .next()
            .await
            .expect("approval request reaches app queue");
        (request, pending)
    }

    async fn request_from_app_queue_with_input(
        app: &mut TuiApp,
        queue: ApprovalQueue,
        source: String,
        tool: String,
        input: Value,
    ) -> (
        zode_core::approval::ApprovalRequest,
        tokio::task::JoinHandle<Approval>,
    ) {
        let pending = tokio::spawn(async move { queue.request(&tool, &input, Some(source)).await });
        let request = app
            .approval_rx
            .next()
            .await
            .expect("approval request reaches app queue");
        (request, pending)
    }

    async fn approval_result_with_timeout(pending: tokio::task::JoinHandle<Approval>) -> Approval {
        tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .expect("approval requester should be resolved")
            .expect("approval requester task should complete")
    }

    async fn detached_approval_request(
        source: Option<String>,
        turn_id: Option<u64>,
    ) -> (
        zode_core::approval::ApprovalRequest,
        tokio::task::JoinHandle<Approval>,
    ) {
        let (queue, mut receiver) = zode_core::approval::approval_queue();
        if let (Some(source), Some(turn_id)) = (source.as_deref(), turn_id) {
            queue.bind_turn(source, turn_id);
        }
        let pending = tokio::spawn(async move {
            queue
                .request(
                    "Bash",
                    &serde_json::json!({"command": "echo routed"}),
                    source,
                )
                .await
        });
        let request = receiver.next().await.expect("detached approval request");
        (request, pending)
    }

    #[tokio::test]
    async fn extension_turn_binds_core_approval_and_close_removes_source() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(710);
        app.extension_tasks.set_bridge_active_for_test(Some(710));
        let prepared = app
            .arm_extension_text_turn(710, task_id, "approval binding".into())
            .expect("extension turn arms");

        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(request.turn_id, Some(prepared.turn_id));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        app.close_active_tab();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn tui_turn_binds_core_approval_before_provider_work() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].titled = true;

        app.submit("tui approval binding", &tx).await;
        let turn_id = app.tabs[0].active_turn_id;
        assert!(turn_id > 0);
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, Some(turn_id));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn exact_turn_terminals_compare_clear_core_approval_binding() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(711);
        app.extension_tasks.set_bridge_active_for_test(Some(711));

        let first = app
            .arm_extension_text_turn(711, task_id.clone(), "first".into())
            .unwrap();
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: first.turn_id,
            result: Ok(()),
        });
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(
            request.turn_id, None,
            "canonical terminal clears its binding"
        );
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        let second = app
            .arm_extension_text_turn(711, task_id, "second".into())
            .unwrap();
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: first.turn_id,
            result: Ok(()),
        });
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(
            request.turn_id,
            Some(second.turn_id),
            "old terminal cannot clear the newer binding"
        );
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        assert!(app.interrupt_active_turn());
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: second.turn_id,
            result: Err("interrupted".into()),
        });
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None, "exact drain terminal clears binding");
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn immutable_exact_tui_approval_opens_the_existing_modal() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].turn_abort = Some(AbortController::new());
        let (request, pending) = detached_approval_request(Some(tab_id.to_string()), Some(7)).await;

        app.route_approval_request(request);

        assert!(app.active_dialog.is_some());
        app.answer_permission(Approval::Deny);
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn old_immutable_approval_is_denied_after_tui_or_extension_n_plus_one() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].active_turn_id = 2;
        app.tabs[0].turn_abort = Some(AbortController::new());

        let (request, pending) = detached_approval_request(Some(tab_id.to_string()), Some(1)).await;
        app.route_approval_request(request);
        assert_eq!(pending.await.unwrap(), Approval::Deny);
        assert!(app.active_dialog.is_none());

        app.extension_tasks.connected(712);
        app.extension_tasks.set_bridge_active_for_test(Some(712));
        app.extension_tasks.turn_routes.insert(
            (tab_id, 2),
            super::ExtensionTurnRoute {
                task_id,
                connection_id: Some(712),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        let (request, pending) = detached_approval_request(Some(tab_id.to_string()), Some(1)).await;
        app.route_approval_request(request);
        assert_eq!(pending.await.unwrap(), Approval::Deny);
        assert!(app.active_dialog.is_none());
    }

    #[tokio::test]
    async fn exact_extension_approval_is_pending_before_sanitized_requested_event() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 720;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "approval turn".into())
            .expect("extension turn arms");
        let raw_tool = format!("mcp__unsafe\nserver__{}", "你".repeat(100));
        let (request, pending) = request_from_app_queue_with_input(
            &mut app,
            queue,
            tab_id.to_string(),
            raw_tool,
            serde_json::json!({
                "command":"command-secret-sentinel",
                "path":"/secret/path-sentinel",
                "url":"https://example.test/?token=query-secret-sentinel"
            }),
        )
        .await;

        app.route_approval_request(request);

        assert!(
            app.active_dialog.is_none(),
            "extension origin never opens TUI"
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        assert!(
            !pending.is_finished(),
            "request waits for extension decision"
        );
        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1);
        let requested = serde_json::to_value(&frames[0].1).unwrap();
        assert_eq!(requested["event"], "approval/requested");
        assert_eq!(requested["params"]["taskId"], task_id);
        assert_eq!(requested["params"]["turnId"], prepared.turn_id.to_string());
        assert_eq!(requested["params"]["approvalId"], "approval-1");
        assert_eq!(requested["params"]["tool"], requested["params"]["summary"]);
        assert!(requested["params"]["tool"].as_str().unwrap().len() <= 200);
        let wire = serde_json::to_string(&requested).unwrap();
        for secret in [
            "command-secret-sentinel",
            "/secret/path-sentinel",
            "query-secret-sentinel",
            "input",
        ] {
            assert!(!wire.contains(secret), "wire leaked {secret}: {wire}");
        }

        drop(app);
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
    }

    #[tokio::test]
    async fn extension_approval_requested_send_failure_denies_and_orphans_without_recording_frame()
    {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 721;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "send failure".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.extension_tasks.fail_send_after_for_test(0);

        app.route_approval_request(request);

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            None
        );
    }

    #[tokio::test]
    async fn extension_approval_pending_cap_is_64_with_checked_monotonic_ids() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 722;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        app.arm_extension_text_turn(connection_id, task_id, "cap".into())
            .unwrap();
        let mut waiting = Vec::new();
        for _ in 0..64 {
            let (request, pending) =
                request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
            app.route_approval_request(request);
            assert!(!pending.is_finished());
            waiting.push(pending);
        }
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 64);
        assert!(waiting.iter().all(|pending| !pending.is_finished()));
        let route = app
            .extension_tasks
            .turn_routes
            .get(&(tab_id, 1))
            .expect("cap does not orphan the live route");
        assert_eq!(route.connection_id, Some(connection_id));
        assert_eq!(route.state, super::ExtensionTurnRouteState::Running);
        let (overflow, overflow_pending) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(overflow);
        assert_eq!(
            approval_result_with_timeout(overflow_pending).await,
            Approval::Deny
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 64);
        let ids: Vec<String> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .filter_map(|(_, frame)| {
                let value = serde_json::to_value(frame).ok()?;
                (value["event"] == "approval/requested")
                    .then(|| value["params"]["approvalId"].as_str().unwrap().to_string())
            })
            .collect();
        assert_eq!(ids.len(), 64);
        assert_eq!(ids.first().map(String::as_str), Some("approval-1"));
        assert_eq!(ids.last().map(String::as_str), Some("approval-64"));

        drop(app);
        for pending in waiting {
            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        }
    }

    #[tokio::test]
    async fn exhausted_approval_sequence_denies_without_frame_or_route_mutation() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 728;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "exhausted".into())
            .unwrap();
        app.extension_tasks.approval_sequence = u64::MAX;
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;

        app.route_approval_request(request);

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert_eq!(app.extension_tasks.approval_sequence, u64::MAX);
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        let route = app
            .extension_tasks
            .turn_routes
            .get(&(tab_id, prepared.turn_id))
            .expect("sequence exhaustion does not orphan route");
        assert_eq!(route.connection_id, Some(connection_id));
        assert_eq!(route.state, super::ExtensionTurnRouteState::Running);
    }

    #[tokio::test]
    async fn extension_approval_decisions_resolve_in_response_event_then_core_order() {
        let (mut app, tx, _rx, cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 723;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "decisions".into())
            .unwrap();

        for (index, decision, expected) in [
            (1, "allow", Approval::AllowOnce),
            (2, "allowAlways", Approval::AllowAlways),
            (3, "deny", Approval::Deny),
        ] {
            let (request, pending) =
                request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
            app.route_approval_request(request);
            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(
                        format!("respond-{index}"),
                        "approval/respond",
                        serde_json::json!({
                            "taskId": task_id,
                            "turnId": prepared.turn_id.to_string(),
                            "approvalId": format!("approval-{index}"),
                            "decision": decision,
                        }),
                    )),
                },
                &tx,
            );
            assert_eq!(approval_result_with_timeout(pending).await, expected);
        }
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 9);
        for index in 0..3 {
            assert_eq!(frames[index * 3]["event"], "approval/requested");
            assert_eq!(frames[index * 3 + 1]["kind"], "response");
            assert_eq!(frames[index * 3 + 2]["event"], "approval/resolved");
        }
        assert_eq!(frames[2]["params"]["decision"], "allow");
        assert_eq!(frames[5]["params"]["decision"], "allowAlways");
        assert_eq!(frames[8]["params"]["decision"], "deny");
        let state = std::fs::read_to_string(zode_core::config::ConfigManager::project_state_path(
            cwd.path(),
        ))
        .expect("allowAlways persists raw tool for its source cwd");
        assert!(state.contains("Bash"));

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "duplicate-decision",
                    "approval/respond",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": prepared.turn_id.to_string(),
                        "approvalId": "approval-1",
                        "decision": "deny",
                    }),
                )),
            },
            &tx,
        );
        let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        let last = serde_json::to_value(last).unwrap();
        assert_eq!(last["kind"], "error");
        assert_eq!(last["code"], "stale_approval");
    }

    #[tokio::test]
    async fn extension_approval_identity_mismatch_does_not_consume_pending_request() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 724;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "identity".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);

        for (request_id, wrong_task, wrong_turn, wrong_approval) in [
            ("wrong-task", "other-task", prepared.turn_id, "approval-1"),
            (
                "wrong-turn",
                task_id.as_str(),
                prepared.turn_id + 1,
                "approval-1",
            ),
            (
                "wrong-id",
                task_id.as_str(),
                prepared.turn_id,
                "approval-999",
            ),
        ] {
            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(
                        request_id,
                        "approval/respond",
                        serde_json::json!({
                            "taskId": wrong_task,
                            "turnId": wrong_turn.to_string(),
                            "approvalId": wrong_approval,
                            "decision": "deny",
                        }),
                    )),
                },
                &tx,
            );
            assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
            assert!(!pending.is_finished());
            let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
            assert_eq!(
                serde_json::to_value(last).unwrap()["code"],
                "stale_approval"
            );
        }

        app.dispatch_extension_approval_response(
            connection_id + 100,
            "wrong-connection".into(),
            task_id.clone(),
            prepared.turn_id,
            "approval-1".into(),
            crate::event::ExtensionApprovalDecision::Deny,
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        assert_eq!(
            serde_json::to_value(last).unwrap()["code"],
            "stale_approval"
        );

        let route = app
            .extension_tasks
            .turn_routes
            .remove(&(tab_id, prepared.turn_id))
            .expect("live route");
        app.dispatch_extension_approval_response(
            connection_id,
            "missing-route".into(),
            task_id.clone(),
            prepared.turn_id,
            "approval-1".into(),
            crate::event::ExtensionApprovalDecision::Deny,
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        assert_eq!(
            serde_json::to_value(last).unwrap()["code"],
            "stale_approval"
        );
        app.extension_tasks
            .turn_routes
            .insert((tab_id, prepared.turn_id), route);

        app.extension_tasks
            .turn_routes
            .get_mut(&(tab_id, prepared.turn_id))
            .unwrap()
            .state = super::ExtensionTurnRouteState::InterruptRequested;
        app.dispatch_extension_approval_response(
            connection_id,
            "wrong-route-state".into(),
            task_id.clone(),
            prepared.turn_id,
            "approval-1".into(),
            crate::event::ExtensionApprovalDecision::Deny,
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        assert_eq!(
            serde_json::to_value(last).unwrap()["code"],
            "stale_approval"
        );
        app.extension_tasks
            .turn_routes
            .get_mut(&(tab_id, prepared.turn_id))
            .unwrap()
            .state = super::ExtensionTurnRouteState::Running;

        app.extension_tasks
            .turn_routes
            .get_mut(&(tab_id, prepared.turn_id))
            .unwrap()
            .connection_id = None;
        app.dispatch_extension_approval_response(
            connection_id,
            "orphan-route".into(),
            task_id.clone(),
            prepared.turn_id,
            "approval-1".into(),
            crate::event::ExtensionApprovalDecision::Deny,
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        let last = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        assert_eq!(
            serde_json::to_value(last).unwrap()["code"],
            "stale_approval"
        );
        app.extension_tasks
            .turn_routes
            .get_mut(&(tab_id, prepared.turn_id))
            .unwrap()
            .connection_id = Some(connection_id);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "exact",
                    "approval/respond",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": prepared.turn_id.to_string(),
                        "approvalId": "approval-1",
                        "decision": "deny",
                    }),
                )),
            },
            &tx,
        );
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
    }

    #[tokio::test]
    async fn stale_approval_error_send_failure_disconnects_only_the_failed_connection() {
        {
            let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let connection_id = 742;
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            app.extension_tasks.connected(connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(connection_id));
            let prepared = app
                .arm_extension_text_turn(connection_id, task_id, "owner error send".into())
                .unwrap();
            let (request, pending) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
            app.route_approval_request(request);
            app.extension_tasks.fail_send_after_for_test(0);

            app.dispatch_extension_approval_response(
                connection_id,
                "bad-owner-response".into(),
                "wrong-task".into(),
                prepared.turn_id,
                "approval-1".into(),
                crate::event::ExtensionApprovalDecision::Deny,
            );

            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
            assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
            assert!(!app.extension_tasks.connection_is_live(connection_id));
            assert_eq!(
                app.extension_tasks
                    .turn_routes
                    .get(&(tab_id, prepared.turn_id))
                    .and_then(|route| route.connection_id),
                None
            );
        }

        {
            let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let owner_connection_id = 743;
            let wrong_connection_id = 744;
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            app.extension_tasks.connected(owner_connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(owner_connection_id));
            let prepared = app
                .arm_extension_text_turn(
                    owner_connection_id,
                    task_id.clone(),
                    "other error send".into(),
                )
                .unwrap();
            let (request, pending) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
            app.route_approval_request(request);
            app.extension_tasks.fail_send_after_for_test(0);

            app.dispatch_extension_approval_response(
                wrong_connection_id,
                "bad-other-response".into(),
                task_id.clone(),
                prepared.turn_id,
                "approval-1".into(),
                crate::event::ExtensionApprovalDecision::Deny,
            );

            assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
            assert!(!pending.is_finished());
            assert!(app.extension_tasks.connection_is_live(owner_connection_id));
            assert_eq!(
                app.extension_tasks
                    .turn_routes
                    .get(&(tab_id, prepared.turn_id))
                    .and_then(|route| route.connection_id),
                Some(owner_connection_id)
            );

            app.dispatch_extension_approval_response(
                owner_connection_id,
                "good-owner-response".into(),
                task_id,
                prepared.turn_id,
                "approval-1".into(),
                crate::event::ExtensionApprovalDecision::Deny,
            );
            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        }
    }

    #[tokio::test]
    async fn extension_approval_response_or_resolved_send_failure_denies_and_orphans() {
        for successful_sends_before_failure in [0, 1] {
            let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let connection_id = 725 + successful_sends_before_failure as u64;
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            app.extension_tasks.connected(connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(connection_id));
            let prepared = app
                .arm_extension_text_turn(connection_id, task_id.clone(), "send fail".into())
                .unwrap();
            let (request, pending) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
            app.route_approval_request(request);
            app.extension_tasks
                .fail_send_after_for_test(successful_sends_before_failure);

            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(
                        "respond-fail",
                        "approval/respond",
                        serde_json::json!({
                            "taskId": task_id,
                            "turnId": prepared.turn_id.to_string(),
                            "approvalId": "approval-1",
                            "decision": "allowAlways",
                        }),
                    )),
                },
                &tx,
            );

            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
            assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
            assert_eq!(
                app.extension_tasks
                    .turn_routes
                    .get(&(tab_id, prepared.turn_id))
                    .and_then(|route| route.connection_id),
                None
            );
            let frames = app.extension_tasks.sent_frames_for_test();
            assert_eq!(frames.len(), 1 + successful_sends_before_failure);
            assert_eq!(
                serde_json::to_value(&frames[0].1).unwrap()["event"],
                "approval/requested"
            );
            if successful_sends_before_failure == 1 {
                assert_eq!(
                    serde_json::to_value(&frames[1].1).unwrap()["kind"],
                    "response"
                );
            }
        }
    }

    #[tokio::test]
    async fn extension_approval_gone_requester_keeps_route_and_sibling_after_successful_frames() {
        let (mut app, tx, _rx, cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 729;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "gone requester".into())
            .unwrap();
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(request);
        let (sibling_request, sibling_pending) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(sibling_request);
        pending.abort();
        assert!(pending.await.is_err());

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "gone-response",
                    "approval/respond",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": prepared.turn_id.to_string(),
                        "approvalId": "approval-1",
                        "decision": "allowAlways",
                    }),
                )),
            },
            &tx,
        );

        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            Some(connection_id)
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["event"], "approval/requested");
        assert_eq!(frames[2]["kind"], "response");
        assert_eq!(frames[3]["event"], "approval/resolved");
        assert!(
            !zode_core::config::ConfigManager::project_state_path(cwd.path()).exists(),
            "failed core response must not persist allow-always"
        );

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "sibling-response",
                    "approval/respond",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": prepared.turn_id.to_string(),
                        "approvalId": "approval-2",
                        "decision": "deny",
                    }),
                )),
            },
            &tx,
        );
        assert_eq!(
            approval_result_with_timeout(sibling_pending).await,
            Approval::Deny
        );
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
    }

    #[tokio::test]
    async fn extension_terminal_resolves_pending_deny_before_terminal_frame() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 730;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "terminal".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);

        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: prepared.turn_id,
            result: Ok(()),
        });

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["event"], "approval/resolved");
        assert_eq!(frames[1]["params"]["decision"], "deny");
        assert_eq!(frames[2]["event"], "turn/completed");
        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
    }

    #[tokio::test]
    async fn tui_interrupt_resolves_extension_pending_before_aborting() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 731;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "tui interrupt".into())
            .unwrap();
        let abort = prepared.abort.clone();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);

        assert!(app.interrupt_active_turn());

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert!(abort.is_aborted());
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["event"], "approval/resolved");
        assert_eq!(frames[1]["params"]["decision"], "deny");
    }

    #[tokio::test]
    async fn tui_interrupt_fences_stale_extension_owner_while_denying_pending() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 736;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "stale owner".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        app.extension_tasks.set_bridge_active_for_test(Some(999));

        assert!(app.interrupt_active_turn());

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            None
        );
        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1, "stale owner receives no resolved frame");
        assert_eq!(
            serde_json::to_value(&frames[0].1).unwrap()["event"],
            "approval/requested"
        );
    }

    #[tokio::test]
    async fn tui_interrupt_fences_stale_extension_owner_without_pending_approval() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 739;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "stale owner no approval".into())
            .unwrap();
        app.extension_tasks.set_bridge_active_for_test(Some(999));

        assert!(app.interrupt_active_turn());

        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            None,
            "an empty approval set must not hide a stale route owner"
        );
        assert!(!app.extension_tasks.connection_is_live(connection_id));
    }

    #[tokio::test]
    async fn extension_interrupt_orders_response_resolved_stopping_then_abort() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 732;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "extension interrupt".into())
            .unwrap();
        let abort = prepared.abort.clone();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "interrupt-with-approval",
                    "turn/interrupt",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": prepared.turn_id.to_string(),
                    }),
                )),
            },
            &tx,
        );

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert!(abort.is_aborted());
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["kind"], "response");
        assert_eq!(frames[2]["event"], "approval/resolved");
        assert_eq!(frames[3]["event"], "turn/stopping");
    }

    #[tokio::test]
    async fn extension_interrupt_claiming_tui_turn_denies_active_and_queued_modals_without_wire_approvals(
    ) {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 749;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        let turn_id = 8;
        app.tabs[0].turn_seq = turn_id;
        app.tabs[0].active_turn_id = turn_id;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template
            .bind_approval_turn(&tab_id.to_string(), turn_id);
        let (active_request, active_waiter) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(active_request);
        let (queued_request, queued_waiter) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(queued_request);
        assert!(app.active_dialog.is_some());
        assert_eq!(app.pending_requests.len(), 1);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "claim-tui-turn",
                    "turn/interrupt",
                    serde_json::json!({
                        "taskId": task_id,
                        "turnId": turn_id.to_string(),
                    }),
                )),
            },
            &tx,
        );

        assert_eq!(
            approval_result_with_timeout(active_waiter).await,
            Approval::Deny
        );
        assert_eq!(
            approval_result_with_timeout(queued_waiter).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());
        assert!(app.pending_requests.is_empty());
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["kind"], "response");
        assert_eq!(frames[1]["event"], "turn/stopping");
        assert!(frames.iter().all(|frame| {
            frame["event"] != "approval/requested" && frame["event"] != "approval/resolved"
        }));
    }

    #[tokio::test]
    async fn lifecycle_first_fences_stamped_unrouted_approval_after_terminal_or_disconnect() {
        {
            let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let tab_id = app.tabs[0].id;
            let turn_id = 9;
            app.tabs[0].turn_seq = turn_id;
            app.tabs[0].active_turn_id = turn_id;
            app.tabs[0].turn_abort = Some(AbortController::new());
            app.template
                .bind_approval_turn(&tab_id.to_string(), turn_id);
            let (request, waiter) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;

            app.handle_agent_event(AppEvent::TurnDone {
                tab_id,
                turn_id,
                result: Ok(()),
            });
            app.route_approval_request(request);

            assert_eq!(approval_result_with_timeout(waiter).await, Approval::Deny);
            assert!(app.active_dialog.is_none());
            assert!(app.pending_requests.is_empty());
            assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        }

        {
            let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let connection_id = 750;
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            app.extension_tasks.connected(connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(connection_id));
            let prepared = app
                .arm_extension_text_turn(connection_id, task_id, "disconnect first".into())
                .unwrap();
            let (request, waiter) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;

            app.extension_tasks.disconnected(connection_id);
            app.route_approval_request(request);

            assert_eq!(approval_result_with_timeout(waiter).await, Approval::Deny);
            assert!(app.active_dialog.is_none());
            assert!(app.pending_requests.is_empty());
            assert_eq!(
                app.extension_tasks
                    .turn_routes
                    .get(&(tab_id, prepared.turn_id))
                    .and_then(|route| route.connection_id),
                None
            );
            assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        }
    }

    #[tokio::test]
    async fn disconnect_and_replacement_deny_pending_and_orphan_all_owned_routes() {
        for replacement in [false, true] {
            let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let connection_id = if replacement { 734 } else { 733 };
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            app.extension_tasks.connected(connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(connection_id));
            let prepared = app
                .arm_extension_text_turn(connection_id, task_id, "disconnect".into())
                .unwrap();
            let (request, pending) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
            app.route_approval_request(request);

            if replacement {
                app.extension_tasks.connected(connection_id + 100);
            } else {
                app.extension_tasks.disconnected(connection_id);
            }

            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
            assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
            assert_eq!(
                app.extension_tasks
                    .turn_routes
                    .get(&(tab_id, prepared.turn_id))
                    .and_then(|route| route.connection_id),
                None
            );
        }
    }

    #[tokio::test]
    async fn turn_event_send_failure_disconnects_and_orphans_every_owned_route() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 751;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "event send".into())
            .unwrap();
        app.extension_tasks.turn_routes.insert(
            (tab_id, prepared.turn_id + 1),
            super::ExtensionTurnRoute {
                task_id,
                connection_id: Some(connection_id),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        app.extension_tasks.fail_send_after_for_test(0);

        app.forward_extension_turn_event(
            tab_id,
            prepared.turn_id,
            &AppEvent::Agent {
                tab_id,
                turn_id: prepared.turn_id,
                cost_label: None,
                event: agent::stream::Event::TextDelta {
                    delta: "cannot send".into(),
                },
            },
        );

        assert!(!app.extension_tasks.connection_is_live(connection_id));
        assert!(app
            .extension_tasks
            .turn_routes
            .values()
            .all(|route| route.connection_id != Some(connection_id)));
    }

    #[tokio::test]
    async fn closing_extension_turn_resolves_pending_before_interrupted_terminal() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 735;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        app.arm_extension_text_turn(connection_id, task_id, "close".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);

        app.close_active_tab();

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["event"], "approval/resolved");
        assert_eq!(frames[2]["event"], "turn/interrupted");
    }

    #[tokio::test]
    async fn closing_tab_silently_denies_pending_approval_when_route_is_missing() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 740;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "close missing route".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        app.extension_tasks
            .turn_routes
            .remove(&(tab_id, prepared.turn_id));

        app.close_active_tab();

        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 1, "missing routes cannot own resolved frames");
        assert_eq!(frames[0]["event"], "approval/requested");
    }

    #[tokio::test]
    async fn stale_turn_done_denies_and_terminals_only_that_immutable_turn() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 741;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let old = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "old turn".into())
            .unwrap();
        let (old_request, old_waiter) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(old_request);

        let new_turn_id = old.turn_id + 1;
        app.tabs[0].turn_seq = new_turn_id;
        app.tabs[0].active_turn_id = new_turn_id;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template
            .bind_approval_turn(&tab_id.to_string(), new_turn_id);
        app.extension_tasks.turn_routes.insert(
            (tab_id, new_turn_id),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: Some(connection_id),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        let (new_request, new_waiter) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(new_request);

        app.record_stale_extension_turn_done(tab_id, old.turn_id, &Ok(()));

        assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
        assert_eq!(
            approval_result_with_timeout(old_waiter).await,
            Approval::Deny
        );
        assert!(app
            .extension_tasks
            .turn_routes
            .contains_key(&(tab_id, new_turn_id)));
        let remaining = app
            .extension_tasks
            .pending_approvals
            .values()
            .next()
            .expect("new turn approval remains pending");
        assert_eq!(remaining.turn_id, new_turn_id);
        assert_eq!(remaining.connection_id, connection_id);
        let frames: Vec<Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0]["event"], "approval/requested");
        assert_eq!(frames[1]["event"], "approval/requested");
        assert_eq!(frames[2]["event"], "approval/resolved");
        assert_eq!(frames[2]["params"]["turnId"], old.turn_id.to_string());
        assert_eq!(frames[3]["event"], "turn/completed");
        assert_eq!(frames[3]["params"]["turnId"], old.turn_id.to_string());
        assert!(frames.iter().all(|frame| {
            frame["params"]["turnId"].as_str() != Some(new_turn_id.to_string().as_str())
                || frame["event"] == "approval/requested"
        }));

        drop(app);
        assert_eq!(
            approval_result_with_timeout(new_waiter).await,
            Approval::Deny
        );
    }

    #[tokio::test]
    async fn snapshot_lists_only_live_exact_pending_approvals_in_sequence_without_input() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 737;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id, "snapshot approvals".into())
            .unwrap();
        let mut waiters = Vec::new();
        for secret in ["first-input-secret", "second-input-secret"] {
            let (request, pending) = request_from_app_queue_with_input(
                &mut app,
                queue.clone(),
                tab_id.to_string(),
                "Bash".into(),
                serde_json::json!({"command": secret}),
            )
            .await;
            app.route_approval_request(request);
            waiters.push(pending);
        }

        let snapshot = app.extension_snapshot(connection_id, None).await.unwrap();
        let approvals = snapshot["approvals"].as_array().unwrap();
        assert_eq!(approvals.len(), 2);
        assert_eq!(approvals[0]["id"], "approval-1");
        assert_eq!(approvals[0]["approvalId"], "approval-1");
        assert_eq!(approvals[1]["approvalId"], "approval-2");
        assert_eq!(approvals[0]["turnId"], prepared.turn_id.to_string());
        assert_eq!(approvals[0]["status"], "pending");
        assert_eq!(approvals[0]["tool"], approvals[0]["summary"]);
        let wire = serde_json::to_string(&snapshot).unwrap();
        assert!(!wire.contains("first-input-secret"));
        assert!(!wire.contains("second-input-secret"));

        app.tabs[0].active_turn_id = prepared.turn_id + 1;
        let stale_filtered = app.extension_snapshot(connection_id, None).await.unwrap();
        assert!(stale_filtered["approvals"].as_array().unwrap().is_empty());

        drop(app);
        for pending in waiters {
            assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        }
    }

    #[tokio::test]
    async fn snapshot_ready_refreshes_approvals_and_rebinds_only_exact_live_turn() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 738;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        app.tabs[0].active_turn_id = 2;
        app.tabs[0].turn_seq = 2;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 2);
        app.extension_tasks.turn_routes.insert(
            (tab_id, 2),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: Some(connection_id),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        app.extension_tasks.turn_routes.insert(
            (tab_id, 1),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: None,
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        assert!(app
            .extension_tasks
            .begin_request(connection_id, "snapshot-live"));
        let stale = serde_json::json!({
            "workspace":{"name":"x","path":"/tmp"},
            "tasks":[{
                "id":task_id,"title":"x","cwd":"/tmp","model":"x",
                "access":"prompt","status":"idle","activeTurnId":null
            }],
            "currentTaskId":task_id,"models":[],"messages":[],"tools":[],
            "approvals":[{"approvalId":"fake-stale"}]
        });

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose: crate::event::ExtensionSnapshotPurpose::Response {
                    request_id: "snapshot-live".into(),
                    rebind_orphan_routes: true,
                },
                result: Ok(stale),
            },
            &tx,
        );

        let response = app.extension_tasks.sent_frames_for_test().pop().unwrap().1;
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["result"]["approvals"][0]["id"], "approval-1");
        assert_eq!(
            response["result"]["approvals"][0]["approvalId"],
            "approval-1"
        );
        assert_eq!(response["result"]["approvals"][0]["status"], "pending");
        assert_eq!(response["result"]["tasks"][0]["status"], "running");
        assert_eq!(response["result"]["tasks"][0]["activeTurnId"], "2");
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 1))
                .and_then(|route| route.connection_id),
            None,
            "orphan old N must not rebind to a snapshot whose live turn is N+1"
        );
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 2))
                .and_then(|route| route.connection_id),
            Some(connection_id)
        );

        drop(app);
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
    }

    #[tokio::test]
    async fn snapshot_rebind_and_approval_intake_are_linearized_by_main_loop_order() {
        for snapshot_first in [false, true] {
            let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
            let connection_id = if snapshot_first { 746 } else { 745 };
            let tab_id = app.tabs[0].id;
            let task_id = app.tabs[0].session_id.clone();
            let turn_id = 1;
            app.extension_tasks.connected(connection_id);
            app.extension_tasks
                .set_bridge_active_for_test(Some(connection_id));
            app.tabs[0].turn_seq = turn_id;
            app.tabs[0].active_turn_id = turn_id;
            app.tabs[0].turn_abort = Some(AbortController::new());
            app.template
                .bind_approval_turn(&tab_id.to_string(), turn_id);
            app.extension_tasks.turn_routes.insert(
                (tab_id, turn_id),
                super::ExtensionTurnRoute {
                    task_id: task_id.clone(),
                    connection_id: None,
                    state: super::ExtensionTurnRouteState::Running,
                },
            );
            let snapshot = serde_json::json!({
                "workspace":{"name":"x","path":"/tmp"},
                "tasks":[{
                    "id":task_id,"title":"x","cwd":"/tmp","model":"x",
                    "access":"prompt","status":"idle","activeTurnId":null
                }],
                "currentTaskId":task_id,"models":[],"messages":[],"tools":[],
                "approvals":[]
            });

            if snapshot_first {
                assert!(app
                    .extension_tasks
                    .begin_request(connection_id, "snapshot-first"));
                app.handle_extension_task_event(
                    crate::event::ExtensionTaskEvent::SnapshotReady {
                        connection_id,
                        purpose: crate::event::ExtensionSnapshotPurpose::Response {
                            request_id: "snapshot-first".into(),
                            rebind_orphan_routes: true,
                        },
                        result: Ok(snapshot.clone()),
                    },
                    &tx,
                );
            }

            let (request, pending) =
                request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
            app.route_approval_request(request);

            if snapshot_first {
                assert!(!pending.is_finished());
                assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 1);
                let frames: Vec<Value> = app
                    .extension_tasks
                    .sent_frames_for_test()
                    .into_iter()
                    .map(|(_, frame)| serde_json::to_value(frame).unwrap())
                    .collect();
                assert_eq!(frames.len(), 2);
                assert_eq!(frames[0]["kind"], "response");
                assert_eq!(frames[1]["event"], "approval/requested");
                drop(app);
                assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
            } else {
                assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
                assert_eq!(app.extension_tasks.pending_approval_count_for_test(), 0);
                assert!(app.extension_tasks.sent_frames_for_test().is_empty());

                assert!(app
                    .extension_tasks
                    .begin_request(connection_id, "approval-first"));
                app.handle_extension_task_event(
                    crate::event::ExtensionTaskEvent::SnapshotReady {
                        connection_id,
                        purpose: crate::event::ExtensionSnapshotPurpose::Response {
                            request_id: "approval-first".into(),
                            rebind_orphan_routes: true,
                        },
                        result: Ok(snapshot),
                    },
                    &tx,
                );
                assert_eq!(app.extension_tasks.sent_frames_for_test().len(), 1);
                assert_eq!(
                    app.extension_tasks
                        .turn_routes
                        .get(&(tab_id, turn_id))
                        .and_then(|route| route.connection_id),
                    Some(connection_id)
                );
                assert_eq!(
                    app.extension_tasks.pending_approval_count_for_test(),
                    0,
                    "a denied pre-rebind request cannot be resurrected"
                );
            }
        }
    }

    #[tokio::test]
    async fn snapshot_response_send_failure_disconnects_and_denies_owned_approval() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 747;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "snapshot send".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        assert!(app
            .extension_tasks
            .begin_request(connection_id, "snapshot-send-fail"));
        app.extension_tasks.fail_send_after_for_test(0);

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose: crate::event::ExtensionSnapshotPurpose::Response {
                    request_id: "snapshot-send-fail".into(),
                    rebind_orphan_routes: true,
                },
                result: Ok(serde_json::json!({
                    "workspace":{"name":"x","path":"/tmp"},
                    "tasks":[{
                        "id":task_id,"title":"x","cwd":"/tmp","model":"x",
                        "access":"prompt","status":"idle","activeTurnId":null
                    }],
                    "currentTaskId":task_id,"models":[],"messages":[],"tools":[],
                    "approvals":[]
                })),
            },
            &tx,
        );

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert!(!app.extension_tasks.connection_is_live(connection_id));
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            None
        );
    }

    #[tokio::test]
    async fn completion_snapshot_send_failure_disconnects_and_denies_owned_approval() {
        let (mut app, tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let connection_id = 748;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        let prepared = app
            .arm_extension_text_turn(connection_id, task_id.clone(), "completion send".into())
            .unwrap();
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        app.extension_tasks
            .current_task_by_connection
            .insert(connection_id, task_id.clone());
        app.extension_tasks.fail_send_after_for_test(0);

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id,
                purpose: crate::event::ExtensionSnapshotPurpose::Completion { failure: None },
                result: Ok(serde_json::json!({
                    "workspace":{"name":"x","path":"/tmp"},
                    "tasks":[{
                        "id":task_id,"title":"x","cwd":"/tmp","model":"x",
                        "access":"prompt","status":"idle","activeTurnId":null
                    }],
                    "currentTaskId":task_id,"models":[],"messages":[],"tools":[],
                    "approvals":[]
                })),
            },
            &tx,
        );

        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert!(!app.extension_tasks.connection_is_live(connection_id));
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, prepared.turn_id))
                .and_then(|route| route.connection_id),
            None
        );
    }

    #[tokio::test]
    async fn workflow_local_operation_approval_is_stamped_and_routes_to_tui() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;

        // `spawn_workflow_run` reserves its approval-gated engine through this
        // same checked primitive before spawning provider/tool work.
        let (op_id, _abort) = app
            .begin_local_operation(0)
            .expect("workflow-style local operation reserves the busy slot");
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(op_id));

        app.route_approval_request(request);
        assert!(app.active_dialog.is_some());
        app.answer_permission(Approval::Deny);
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn old_local_approval_after_interrupt_and_new_operation_is_denied() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let (first_op_id, _abort) = app.begin_local_operation(0).expect("first local op arms");
        let (old_request, old_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(old_request.turn_id, None);
        assert_eq!(old_request.local_op_id, Some(first_op_id));

        assert!(app.interrupt_active_turn());
        let (unbound_request, unbound_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(unbound_request.turn_id, None);
        assert_eq!(unbound_request.local_op_id, None);
        unbound_request.respond(Approval::Deny).unwrap();
        assert_eq!(unbound_pending.await.unwrap(), Approval::Deny);

        let (second_op_id, _abort) = app.begin_local_operation(0).expect("second local op arms");
        assert!(second_op_id > first_op_id);
        app.route_approval_request(old_request);
        assert_eq!(old_pending.await.unwrap(), Approval::Deny);
        assert!(app.active_dialog.is_none());

        let (current_request, current_pending) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(current_request.turn_id, None);
        assert_eq!(current_request.local_op_id, Some(second_op_id));
        app.route_approval_request(current_request);
        assert!(app.active_dialog.is_some());
        app.answer_permission(Approval::Deny);
        assert_eq!(current_pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn local_operation_terminals_compare_clear_core_approval_binding() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let (first_op_id, _abort) = app.begin_local_operation(0).expect("first local op arms");
        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: first_op_id,
            result: Ok("first complete".into()),
        });
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        let (second_op_id, _abort) = app.begin_local_operation(0).expect("second local op arms");
        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: first_op_id,
            result: Ok("stale duplicate".into()),
        });
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        assert_eq!(request.local_op_id, Some(second_op_id));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);

        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: second_op_id,
            result: Ok("second complete".into()),
        });
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn stale_local_terminal_denies_its_tui_ghosts_without_touching_new_owner() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let (first_op_id, _abort) = app.begin_local_operation(0).expect("first local op arms");

        let (active_request, active_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(active_request);
        let (queued_request, queued_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(queued_request);
        assert!(app.active_dialog.is_some());
        assert_eq!(app.pending_requests.len(), 1);

        // Simulate a newer owner already occupying the shared slot while old
        // modal state is still present. The stale terminal must clean only its
        // typed ghosts, not the newer slot or core registry binding.
        app.tabs[0].active_local_op_id = None;
        app.tabs[0].turn_abort = None;
        let (second_op_id, _abort) = app.begin_local_operation(0).expect("second local op arms");
        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: first_op_id,
            result: Ok("stale terminal".into()),
        });

        assert_eq!(
            approval_result_with_timeout(active_pending).await,
            Approval::Deny
        );
        assert_eq!(
            approval_result_with_timeout(queued_pending).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());
        assert!(app.pending_requests.is_empty());
        assert_eq!(app.tabs[0].active_local_op_id, Some(second_op_id));
        assert!(app.tabs[0].turn_abort.is_some());
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, Some(second_op_id));
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn stale_turn_terminal_denies_its_tui_ghosts_without_touching_new_turn() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 1;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 1);

        let (active_request, active_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(active_request);
        let (queued_request, queued_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(queued_request);
        assert!(app.active_dialog.is_some());
        assert_eq!(app.pending_requests.len(), 1);

        app.tabs[0].active_turn_id = 2;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 2);
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 1,
            result: Ok(()),
        });

        assert_eq!(
            approval_result_with_timeout(active_pending).await,
            Approval::Deny
        );
        assert_eq!(
            approval_result_with_timeout(queued_pending).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());
        assert!(app.pending_requests.is_empty());
        assert_eq!(app.tabs[0].active_turn_id, 2);
        assert!(app.tabs[0].turn_abort.is_some());
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, Some(2));
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
    }

    #[tokio::test]
    async fn tui_interrupt_denies_exact_ghost_and_clears_core_stamp_immediately() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 3;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 3);
        let (request, pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(request);
        assert!(app.active_dialog.is_some());

        assert!(app.interrupt_active_turn());
        assert_eq!(approval_result_with_timeout(pending).await, Approval::Deny);
        assert!(app.active_dialog.is_none());
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn close_tab_denies_mixed_typed_tui_approvals_for_its_source() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 4;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 4);
        let (turn_request, turn_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(turn_request);

        app.tabs[0].active_turn_id = 0;
        app.tabs[0].turn_abort = None;
        let (_op_id, _abort) = app.begin_local_operation(0).expect("local owner arms");
        let (local_request, local_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(local_request);
        assert!(app.active_dialog.is_some());
        assert_eq!(app.pending_requests.len(), 1);

        app.close_active_tab();
        assert_eq!(
            approval_result_with_timeout(turn_pending).await,
            Approval::Deny
        );
        assert_eq!(
            approval_result_with_timeout(local_pending).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());
        assert!(app.pending_requests.is_empty());
        let (request, pending) = request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        assert_eq!(request.turn_id, None);
        assert_eq!(request.local_op_id, None);
        request.respond(Approval::Deny).unwrap();
        assert_eq!(pending.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn compact_and_owned_shell_terminals_deny_their_typed_tui_ghosts() {
        let (mut app, _tx, _rx, _cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        let (compact_op_id, _abort) = app.begin_local_operation(0).expect("compact owner arms");
        let (request, compact_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(request);
        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id: compact_op_id,
            result: Err("compact denied".into()),
            auto: false,
        });
        assert_eq!(
            approval_result_with_timeout(compact_pending).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());

        let (shell_op_id, _abort) = app.begin_local_operation(0).expect("shell owner arms");
        let (request, shell_pending) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(request);
        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "echo hidden".into(),
            output: None,
            op_id: Some(shell_op_id),
        });
        assert_eq!(
            approval_result_with_timeout(shell_pending).await,
            Approval::Deny
        );
        assert!(app.active_dialog.is_none());
    }

    #[tokio::test]
    async fn allow_always_persists_to_background_source_dialog_cwd() {
        let (mut app, _tx, _rx, foreground_cwd, queue) = make_test_app_with_approval_queue().await;
        let background_cwd = tempfile::tempdir().unwrap();
        let background_engine = app
            .template
            .assemble_tab(Some(background_cwd.path().to_path_buf()), Some("1".into()))
            .await
            .unwrap();
        let mut background = crate::tab::SessionTab::new(
            1,
            std::sync::Arc::new(background_engine),
            "background-task".into(),
        );
        background.active_turn_id = 8;
        background.turn_abort = Some(AbortController::new());
        app.tabs.push(background);
        app.template.bind_approval_turn("1", 8);

        let (request, pending) = request_from_app_queue(&mut app, queue, "1".into()).await;
        app.route_approval_request(request);
        assert_eq!(app.active, 0, "background approval must not steal focus");
        app.answer_permission(Approval::AllowAlways);
        assert_eq!(pending.await.unwrap(), Approval::AllowAlways);

        let background_state = std::fs::read_to_string(
            zode_core::config::ConfigManager::project_state_path(background_cwd.path()),
        )
        .expect("background state is persisted");
        assert!(background_state.contains("Bash"));
        let foreground_state =
            zode_core::config::ConfigManager::project_state_path(foreground_cwd.path());
        assert!(
            !foreground_state.exists(),
            "active foreground cwd must not receive background permission"
        );
    }

    #[tokio::test]
    async fn dropped_requester_dismisses_and_promotes_without_persisting_allow_always() {
        let (mut app, _tx, _rx, cwd, queue) = make_test_app_with_approval_queue().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].active_turn_id = 6;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.template.bind_approval_turn(&tab_id.to_string(), 6);

        let (first_request, first_pending) =
            request_from_app_queue(&mut app, queue.clone(), tab_id.to_string()).await;
        app.route_approval_request(first_request);
        let (second_request, second_pending) =
            request_from_app_queue(&mut app, queue, tab_id.to_string()).await;
        app.route_approval_request(second_request);
        first_pending.abort();
        assert!(first_pending.await.is_err());

        app.answer_permission(Approval::AllowAlways);
        assert!(
            app.active_dialog.is_some(),
            "next queued request is promoted even when send fails"
        );
        assert!(
            !zode_core::config::ConfigManager::project_state_path(cwd.path()).exists(),
            "failed response must not persist allow-always"
        );
        app.answer_permission(Approval::Deny);
        assert_eq!(second_pending.await.unwrap(), Approval::Deny);
        assert!(app.active_dialog.is_none());
    }

    #[tokio::test]
    async fn late_compact_done_cannot_clear_or_silence_a_new_extension_turn() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].local_op_seq = 1;
        app.tabs[0].active_local_op_id = Some(1);
        app.tabs[0].turn_abort = Some(AbortController::new());
        assert!(app.interrupt_active_turn());

        app.extension_tasks.connected(701);
        app.extension_tasks.set_bridge_active_for_test(Some(701));
        let prepared = app
            .arm_extension_text_turn(701, task_id.clone(), "new extension turn".into())
            .expect("new extension turn arms after old compact is interrupted");

        app.handle_agent_event(AppEvent::CompactDone {
            tab_id,
            op_id: 1,
            result: Ok("late compact".into()),
            auto: false,
        });
        assert_eq!(app.tabs[0].active_turn_id, prepared.turn_id);
        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].mode, Mode::Thinking);

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: prepared.turn_id,
            cost_label: None,
            event: agent::stream::Event::TextDelta {
                delta: "kept".into(),
            },
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: prepared.turn_id,
            result: Ok(()),
        });
        let events: Vec<String> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .filter_map(|(_, frame)| serde_json::to_value(frame).ok())
            .filter_map(|frame| {
                frame
                    .get("event")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        assert!(events.iter().any(|event| event == "message/delta"));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.as_str() == "turn/completed")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn late_bg_done_cannot_release_or_relabel_a_new_local_operation() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].local_op_seq = 1;
        app.tabs[0].active_local_op_id = Some(1);
        app.tabs[0].turn_abort = Some(AbortController::new());
        assert!(app.interrupt_active_turn());

        app.tabs[0].local_op_seq = 2;
        app.tabs[0].active_local_op_id = Some(2);
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.tabs[0].mode = Mode::Thinking;
        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: 1,
            result: Err("late old failure".into()),
        });

        assert_eq!(app.tabs[0].active_local_op_id, Some(2));
        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].mode, Mode::Thinking);
    }

    #[tokio::test]
    async fn late_bg_progress_and_done_cannot_land_in_a_new_agent_turn() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].local_op_seq = 1;
        app.tabs[0].active_local_op_id = Some(1);
        app.tabs[0].turn_abort = Some(AbortController::new());
        assert!(app.interrupt_active_turn());

        app.extension_tasks.connected(702);
        app.extension_tasks.set_bridge_active_for_test(Some(702));
        let prepared = app
            .arm_extension_text_turn(702, task_id, "new agent".into())
            .expect("new agent turn arms");
        app.handle_agent_event(AppEvent::BgProgress {
            tab_id,
            op_id: 1,
            line: "STALE_BG_PROGRESS".into(),
        });
        app.handle_agent_event(AppEvent::BgDone {
            tab_id,
            op_id: 1,
            result: Err("STALE_BG_DONE".into()),
        });

        assert_eq!(app.tabs[0].active_turn_id, prepared.turn_id);
        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].mode, Mode::Thinking);
        assert!(!app.tabs[0]
            .chat
            .messages()
            .iter()
            .any(|message| message.text.contains("STALE_BG_PROGRESS")
                || message.text.contains("STALE_BG_DONE")));
    }

    #[tokio::test]
    async fn late_owned_shell_a_cannot_clear_or_append_into_local_operation_b() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        app.tabs[0].local_op_seq = 1;
        app.tabs[0].active_local_op_id = Some(1);
        app.tabs[0].turn_abort = Some(AbortController::new());
        assert!(app.interrupt_active_turn());
        let (new_op_id, _abort) = app
            .begin_local_operation(0)
            .expect("second local operation owns the slot");
        assert_eq!(new_op_id, 2);

        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "echo stale-shell-a".into(),
            output: Some("STALE_SHELL_A".into()),
            op_id: Some(1),
        });

        assert_eq!(app.tabs[0].active_local_op_id, Some(2));
        assert!(app.tabs[0].turn_abort.is_some());
        assert!(app.tabs[0].pending_shell_context.is_empty());
    }

    #[tokio::test]
    async fn exact_local_operation_terminal_applies_once() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let (op_id, _abort) = app
            .begin_local_operation(0)
            .expect("local operation owns slot");
        for _ in 0..2 {
            app.handle_agent_event(AppEvent::BgDone {
                tab_id,
                op_id,
                result: Ok("EXACT_BG_DONE_ONCE".into()),
            });
        }

        assert_eq!(app.tabs[0].active_local_op_id, None);
        assert!(app.tabs[0].turn_abort.is_none());
        assert_eq!(
            app.tabs[0]
                .chat
                .messages()
                .iter()
                .filter(|message| message.text.contains("EXACT_BG_DONE_ONCE"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn concurrent_nonowned_shell_outputs_without_touching_local_owner() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let (op_id, _abort) = app
            .begin_local_operation(0)
            .expect("background operation owns slot");

        app.handle_agent_event(AppEvent::LocalShellDone {
            tab_id,
            cmd: "echo parallel".into(),
            output: Some("PARALLEL_SHELL_OUTPUT".into()),
            op_id: None,
        });

        assert_eq!(app.tabs[0].active_local_op_id, Some(op_id));
        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].pending_shell_context.len(), 1);
        assert!(app.tabs[0].pending_shell_context[0].contains("PARALLEL_SHELL_OUTPUT"));
    }

    #[tokio::test]
    async fn extension_tasks_new_task_does_not_change_terminal_focus() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.template = app
            .template
            .with_tool_access(zode_core::ToolAccessMode::Auto);
        let active = app.active;
        let active_session = app.tabs[active].session_id.clone();

        let response = app
            .handle_extension_request(
                4,
                TaskClientFrame::request("ext-1", "task/create", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();

        assert_eq!(app.active, active);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[active].session_id, active_session);
        assert_ne!(response["currentTaskId"], active_session);
        assert!(app.tabs[1].reassemble_pending);
        assert_eq!(
            app.tabs[1].extension_access,
            zode_core::ToolAccessMode::Auto
        );
        let created = response["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == response["currentTaskId"])
            .unwrap();
        assert_eq!(created["access"], "auto");

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("created task assembly finishes")
            .expect("event channel stays open");
        app.handle_agent_event(event);
        let created_id = response["currentTaskId"].as_str().unwrap();
        let created_tab = app
            .tabs
            .iter()
            .find(|tab| tab.session_id == created_id)
            .unwrap();
        assert_eq!(
            created_tab.extension_access,
            zode_core::ToolAccessMode::Auto
        );
    }

    #[tokio::test]
    async fn extension_tasks_snapshot_lists_open_and_persisted_sessions_once() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        app.tabs[0].session_id = "saved-1".into();
        app.tabs[0].title = "Open wins".into();
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: "saved-1".into(),
            title: "Stale persisted title".into(),
            cwd: "/persisted".into(),
            model: "persisted-model".into(),
            updated_at: 2,
        });
        index.upsert(SessionMeta {
            id: "saved-2".into(),
            title: "Closed task".into(),
            cwd: "/persisted".into(),
            model: "persisted-model".into(),
            updated_at: 1,
        });
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();

        let value = app
            .handle_extension_request(
                4,
                TaskClientFrame::request("ext-2", "snapshot/read", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();
        let tasks = value["tasks"].as_array().unwrap();
        assert_eq!(
            tasks.iter().filter(|task| task["id"] == "saved-1").count(),
            1
        );
        assert_eq!(
            tasks.iter().find(|task| task["id"] == "saved-1").unwrap()["title"],
            "Open wins"
        );
        assert!(tasks.iter().any(|task| task["id"] == "saved-2"));
    }

    #[tokio::test]
    async fn extension_tasks_running_snapshot_includes_string_turn_id() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.tabs[0].active_turn_id = 42;
        let task_id = app.tabs[0].session_id.clone();

        let value = app
            .handle_extension_request(
                5,
                TaskClientFrame::request("snapshot", "snapshot/read", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();
        let task = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == task_id)
            .unwrap();
        assert_eq!(task["status"], "running");
        assert_eq!(task["activeTurnId"], "42");

        app.tabs[0].turn_abort = None;
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].draining_turn_id = Some(42);
        let stopping = app
            .handle_extension_request(
                6,
                TaskClientFrame::request("stopping", "snapshot/read", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();
        let task = stopping["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == task_id)
            .unwrap();
        assert_eq!(task["status"], "stopping");
        assert_eq!(task["activeTurnId"], "42");
    }

    #[tokio::test]
    async fn extension_tasks_select_restores_in_background_and_redacts_tool_output() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        let restored_cwd = tempfile::tempdir().unwrap();
        let sessions = config.path().join("sessions");
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let active = app.active;
        let active_session = app.tabs[active].session_id.clone();
        app.tabs[active]
            .engine
            .store
            .lock()
            .unwrap()
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text {
                    text: "ACTIVE-TAB-PRIVATE-HISTORY".into(),
                }],
            })
            .unwrap();

        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text {
                    text: "inspect auth".into(),
                }],
            })
            .unwrap();
        store
            .push(Message::Assistant {
                header: Header::new(),
                content: vec![
                    ContentBlock::Text {
                        text: "I'll inspect it.".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".into(),
                        name: "shell".into(),
                        input: serde_json::json!({"command":"sensitive command"}),
                    },
                ],
            })
            .unwrap();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".into(),
                    content: ToolResultContent::Text("TOP-SECRET FULL TOOL OUTPUT".into()),
                    is_error: false,
                }],
            })
            .unwrap();
        let path = sessions.join("saved-task.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Session::save(&path, &store).await.unwrap();
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: "saved-task".into(),
            title: "Saved task".into(),
            cwd: restored_cwd.path().display().to_string(),
            model: "saved-model".into(),
            updated_at: 3,
        });
        std::fs::write(
            sessions.join("index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();

        let value = app
            .handle_extension_request(
                9,
                TaskClientFrame::request(
                    "ext-3",
                    "task/select",
                    serde_json::json!({"taskId":"saved-task"}),
                ),
                &tx,
            )
            .await
            .unwrap();

        assert_eq!(app.active, active);
        assert_eq!(app.tabs[active].session_id, active_session);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(value["currentTaskId"], "saved-task");
        assert_eq!(value["messages"], serde_json::json!([]));
        assert_eq!(value["tools"], serde_json::json!([]));
        let placeholder = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "saved-task")
            .unwrap();
        assert_eq!(
            placeholder["cwd"],
            restored_cwd.path().display().to_string()
        );
        assert_eq!(placeholder["model"], "saved-model");
        assert!(!serde_json::to_string(&value)
            .unwrap()
            .contains("ACTIVE-TAB-PRIVATE-HISTORY"));

        let assembled = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("background restore finishes")
            .expect("event channel stays open");
        app.handle_agent_event(assembled);
        assert_eq!(app.active, active, "completion also keeps terminal focus");
        let restored = app
            .tabs
            .iter()
            .find(|tab| tab.session_id == "saved-task")
            .unwrap();
        assert_eq!(restored.engine.model, "saved-model");
        assert_eq!(restored.extension_access, zode_core::ToolAccessMode::Prompt);
        assert_eq!(app.template.model(), Some("test-model"));
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt
        );
        let authoritative = app
            .handle_extension_request(
                9,
                TaskClientFrame::request(
                    "ext-4",
                    "snapshot/read",
                    serde_json::json!({"taskId":"saved-task"}),
                ),
                &tx,
            )
            .await
            .unwrap();
        let wire = serde_json::to_string(&authoritative).unwrap();
        assert!(wire.contains("inspect auth"));
        assert!(wire.contains("I'll inspect it."));
        assert!(wire.contains("shell"));
        assert!(wire.contains("completed"));
        assert!(!wire.contains("TOP-SECRET FULL TOOL OUTPUT"));
        assert!(!wire.contains("sensitive command"));

        let pushed = serde_json::to_value(app.extension_snapshot_event(9).await).unwrap();
        assert_eq!(pushed["kind"], "event");
        assert_eq!(pushed["event"], "snapshot");
        assert_eq!(pushed["params"]["currentTaskId"], "saved-task");
        assert!(pushed["params"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "inspect auth"));
    }

    #[tokio::test]
    async fn extension_tasks_request_params_are_strict() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));

        for (method, params) in [
            ("task/create", serde_json::json!({"surprise": true})),
            ("task/select", serde_json::json!({})),
            (
                "task/select",
                serde_json::json!({"taskId":"x","surprise":true}),
            ),
            ("snapshot/read", serde_json::json!({"surprise": true})),
        ] {
            let error = app
                .handle_extension_request(
                    1,
                    TaskClientFrame::request("strict", method, params),
                    &tx,
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), "invalid_params", "method={method}");
        }

        let error = app
            .handle_extension_request(
                1,
                TaskClientFrame::request(
                    "missing",
                    "task/select",
                    serde_json::json!({"taskId":"does-not-exist"}),
                ),
                &tx,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "task_not_found");

        let active_task = app.tabs[app.active].session_id.clone();
        let recovered = app
            .handle_extension_request(
                2,
                TaskClientFrame::request(
                    "stale-snapshot",
                    "snapshot/read",
                    serde_json::json!({"taskId":"stale-local-id"}),
                ),
                &tx,
            )
            .await
            .expect("a stale persisted selection falls back authoritatively");
        assert_eq!(recovered["currentTaskId"], active_task);
    }

    #[test]
    fn extension_turn_rpc_parser_is_strict_and_preserves_literal_input() {
        for input in ["  /clear  ", "!rm -rf literal", "\nkeep whitespace\n"] {
            let parsed = super::parse_extension_request(
                "turn/start",
                serde_json::json!({"taskId":"task-1","input":input}),
            )
            .expect("literal text is a valid turn input");
            match parsed {
                crate::event::ExtensionTaskRequest::TurnStart {
                    task_id,
                    input: parsed_input,
                    attachment_ids,
                } => {
                    assert_eq!(task_id, "task-1");
                    assert_eq!(parsed_input, input, "submit preserves original text");
                    assert!(attachment_ids.is_empty());
                }
                other => panic!("unexpected request: {other:?}"),
            }
        }

        for params in [
            serde_json::json!({"taskId":"task-1","input":"   "}),
            serde_json::json!({"taskId":"task-1","input":"ok","extra":true}),
            serde_json::json!({"taskId":"task-1"}),
        ] {
            let error = super::parse_extension_request("turn/start", params).unwrap_err();
            assert_eq!(error.code(), "invalid_params");
        }
    }

    #[test]
    fn extension_attachment_rpc_parser_is_strict_and_decodes_chunks() {
        let begin = super::parse_extension_request(
            "attachment/begin",
            serde_json::json!({
                "taskId": "task-1",
                "name": "main.rs",
                "mediaType": "text/plain",
                "size": 3,
            }),
        )
        .expect("canonical attachment begin parses");
        assert!(matches!(
            begin,
            crate::event::ExtensionTaskRequest::AttachmentBegin {
                task_id,
                name,
                media_type,
                size: 3,
            } if task_id == "task-1" && name == "main.rs" && media_type == "text/plain"
        ));

        let chunk = super::parse_extension_request(
            "attachment/chunk",
            serde_json::json!({
                "uploadId": "zode_upload_1",
                "sequence": 0,
                "data": "YWJj",
            }),
        )
        .expect("canonical base64 chunk parses");
        assert!(matches!(
            chunk,
            crate::event::ExtensionTaskRequest::AttachmentChunk {
                upload_id,
                sequence: 0,
                data,
            } if upload_id == "zode_upload_1" && data == b"abc"
        ));

        for (method, params) in [
            (
                "attachment/begin",
                serde_json::json!({
                    "taskId":"task-1","name":"main.rs","mediaType":"text/plain",
                    "size":3,"extra":true
                }),
            ),
            (
                "attachment/chunk",
                serde_json::json!({"uploadId":"zode_upload_1","sequence":0,"data":"***"}),
            ),
            (
                "attachment/chunk",
                serde_json::json!({"uploadId":"","sequence":0,"data":"YWJj"}),
            ),
            (
                "attachment/finish",
                serde_json::json!({"uploadId":"zode_upload_1","extra":true}),
            ),
            ("attachment/cancel", serde_json::json!({})),
        ] {
            let error = super::parse_extension_request(method, params.clone()).unwrap_err();
            assert_eq!(
                error.code(),
                "invalid_params",
                "method={method} params={params}"
            );
        }
    }

    #[test]
    fn extension_turn_parser_accepts_attachment_only_and_rejects_truly_empty_turns() {
        let parsed = super::parse_extension_request(
            "turn/start",
            serde_json::json!({
                "taskId":"task-1",
                "input":"   ",
                "attachmentIds":["zode_attachment_1"]
            }),
        )
        .expect("an attachment-only turn is valid");
        assert!(matches!(
            parsed,
            crate::event::ExtensionTaskRequest::TurnStart {
                task_id,
                input,
                attachment_ids,
            } if task_id == "task-1"
                && input == "   "
                && attachment_ids == ["zode_attachment_1"]
        ));

        for params in [
            serde_json::json!({"taskId":"task-1","input":"   ","attachmentIds":[]}),
            serde_json::json!({"taskId":"task-1","input":"ok","attachmentIds":[""]}),
        ] {
            let error = super::parse_extension_request("turn/start", params).unwrap_err();
            assert_eq!(error.code(), "invalid_params");
        }
    }

    #[tokio::test]
    async fn attachment_upload_requests_dispatch_directly_without_index_io() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 801;
        let task_id = app.tabs[0].session_id.clone();

        let dispatch = |app: &mut TuiApp, id: &str, method: &str, params| {
            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(id, method, params)),
                },
                &tx,
            );
        };

        dispatch(
            &mut app,
            "begin",
            "attachment/begin",
            serde_json::json!({
                "taskId": task_id,
                "name": "main.rs",
                "mediaType": "text/plain",
                "size": 3,
            }),
        );
        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1, "begin responds synchronously");
        let begin = serde_json::to_value(&frames[0].1).unwrap();
        assert_eq!(begin["kind"], "response");
        assert_eq!(begin["id"], "begin");
        let upload_id = begin["result"]["uploadId"].as_str().unwrap().to_string();

        dispatch(
            &mut app,
            "chunk",
            "attachment/chunk",
            serde_json::json!({"uploadId":upload_id,"sequence":0,"data":"YWJj"}),
        );
        dispatch(
            &mut app,
            "finish",
            "attachment/finish",
            serde_json::json!({"uploadId":upload_id}),
        );

        let frames: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1]["result"]["nextSequence"], 1);
        assert_eq!(frames[2]["kind"], "response");
        assert!(frames[2]["result"]["attachmentId"]
            .as_str()
            .is_some_and(|id| id.starts_with("zode_attachment_")));
        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);
        assert_eq!(app.extension_tasks.sessions.io_started_for_test(), 0);
    }

    #[tokio::test]
    async fn attachment_registry_cleans_disconnect_and_connection_replacement() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let task_id = app.tabs[0].session_id.clone();
        let begin = |app: &mut TuiApp, connection_id, request_id: &str| {
            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(
                        request_id,
                        "attachment/begin",
                        serde_json::json!({
                            "taskId": task_id,
                            "name": "pending.txt",
                            "mediaType": "text/plain",
                            "size": 3,
                        }),
                    )),
                },
                &tx,
            );
        };

        begin(&mut app, 811, "first");
        assert_eq!(
            app.extension_attachments.pending_for_connection(811),
            (1, 3)
        );
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 811,
                kind: TaskInboundKind::Disconnected,
            },
            &tx,
        );
        assert_eq!(
            app.extension_attachments.pending_for_connection(811),
            (0, 0)
        );

        begin(&mut app, 812, "old");
        assert_eq!(
            app.extension_attachments.pending_for_connection(812),
            (1, 3)
        );
        app.extension_tasks.set_bridge_active_for_test(Some(813));
        begin(&mut app, 813, "replacement");
        assert_eq!(
            app.extension_attachments.pending_for_connection(812),
            (0, 0)
        );
        assert_eq!(
            app.extension_attachments.pending_for_connection(813),
            (1, 3)
        );
    }

    #[tokio::test]
    async fn attachment_registry_periodic_sweep_expires_idle_uploads() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let now = Instant::now();
        let task_id = app.tabs[0].session_id.clone();
        app.extension_attachments
            .begin(
                821,
                task_id,
                BeginUpload {
                    file_name: "stale.txt".into(),
                    media_type: "text/plain".into(),
                    declared_size: 3,
                },
                now,
            )
            .unwrap();
        assert_eq!(
            app.extension_attachments.pending_for_connection(821),
            (1, 3)
        );

        app.cleanup_extension_attachments_at(now + UPLOAD_TTL);

        assert_eq!(
            app.extension_attachments.pending_for_connection(821),
            (0, 0)
        );
    }

    #[tokio::test]
    async fn closing_task_removes_all_owned_attachment_uploads() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let now = Instant::now();
        let task_id = app.tabs[0].session_id.clone();
        app.extension_attachments
            .begin(
                831,
                task_id.clone(),
                BeginUpload {
                    file_name: "closing.txt".into(),
                    media_type: "text/plain".into(),
                    declared_size: 1,
                },
                now,
            )
            .unwrap();
        assert_eq!(
            app.extension_attachments.reserved_for_task(831, &task_id),
            (1, 1)
        );

        app.close_active_tab();

        assert_eq!(
            app.extension_attachments.reserved_for_task(831, &task_id),
            (0, 0)
        );
    }

    #[tokio::test]
    async fn extension_turn_transactionally_consumes_utf8_attachment_into_content_blocks() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let now = Instant::now();
        let connection_id = 841;
        let task_id = app.tabs[0].session_id.clone();
        let secret_body = "fn private_api_key() {}";
        let ticket = app
            .extension_attachments
            .begin(
                connection_id,
                task_id.clone(),
                BeginUpload {
                    file_name: "main.rs".into(),
                    media_type: "text/plain".into(),
                    declared_size: secret_body.len(),
                },
                now,
            )
            .unwrap();
        app.extension_attachments
            .push_chunk(
                connection_id,
                &ticket.upload_id,
                0,
                secret_body.as_bytes(),
                now,
            )
            .unwrap();
        let receipt = app
            .extension_attachments
            .finish(connection_id, &ticket.upload_id, now)
            .unwrap();

        let prepared = app
            .arm_extension_turn(
                connection_id,
                task_id.clone(),
                "review this".into(),
                vec![receipt.attachment_id.clone()],
            )
            .expect("finished UTF-8 attachment starts a turn");

        assert!(matches!(
            &prepared.content[0],
            ContentBlock::Text { text } if text == "review this"
        ));
        assert!(matches!(
            &prepared.content[1],
            ContentBlock::Text { text }
                if text.contains("<attached_file name=\"main.rs\"")
                    && text.contains(secret_body)
        ));
        let shown = app.tabs[0].chat.messages().last().unwrap().text.clone();
        assert!(shown.contains("main.rs"));
        assert!(!shown.contains(secret_body));
        assert!(matches!(
            app.extension_attachments.consume_finished(
                connection_id,
                &task_id,
                &[receipt.attachment_id],
                now,
            ),
            Err(UploadError::AttachmentNotFound)
        ));
    }

    #[tokio::test]
    async fn unsupported_image_turn_does_not_consume_attachment_or_mutate_task() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let now = Instant::now();
        let connection_id = 842;
        let task_id = app.tabs[0].session_id.clone();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\0";
        let ticket = app
            .extension_attachments
            .begin(
                connection_id,
                task_id.clone(),
                BeginUpload {
                    file_name: "screen.png".into(),
                    media_type: "image/png".into(),
                    declared_size: png.len(),
                },
                now,
            )
            .unwrap();
        app.extension_attachments
            .push_chunk(connection_id, &ticket.upload_id, 0, png, now)
            .unwrap();
        let receipt = app
            .extension_attachments
            .finish(connection_id, &ticket.upload_id, now)
            .unwrap();
        let before_messages = app.tabs[0].chat.messages().len();

        let error = app
            .arm_extension_turn(
                connection_id,
                task_id.clone(),
                String::new(),
                vec![receipt.attachment_id.clone()],
            )
            .err()
            .expect("unsupported image route must fail");

        assert_eq!(error.code(), "attachment_unsupported");
        assert_eq!(app.tabs[0].turn_seq, 0);
        assert!(app.tabs[0].turn_abort.is_none());
        assert_eq!(app.tabs[0].chat.messages().len(), before_messages);
        assert_eq!(
            app.extension_attachments
                .reserved_for_task(connection_id, &task_id),
            (1, png.len())
        );
    }

    #[tokio::test]
    async fn turn_start_dispatch_accepts_finished_text_attachment_and_emits_started() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let now = Instant::now();
        let connection_id = 843;
        let task_id = app.tabs[0].session_id.clone();
        let body = b"const hidden = true;";
        let ticket = app
            .extension_attachments
            .begin(
                connection_id,
                task_id.clone(),
                BeginUpload {
                    file_name: "config.js".into(),
                    media_type: "text/javascript".into(),
                    declared_size: body.len(),
                },
                now,
            )
            .unwrap();
        app.extension_attachments
            .push_chunk(connection_id, &ticket.upload_id, 0, body, now)
            .unwrap();
        let receipt = app
            .extension_attachments
            .finish(connection_id, &ticket.upload_id, now)
            .unwrap();

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "start-with-file",
                    "turn/start",
                    serde_json::json!({
                        "taskId": task_id,
                        "input": "inspect",
                        "attachmentIds": [receipt.attachment_id],
                    }),
                )),
            },
            &tx,
        );

        let frames: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(frames[0]["kind"], "response");
        assert_eq!(frames[0]["id"], "start-with-file");
        assert_eq!(frames[1]["event"], "turn/started");
        assert_eq!(app.tabs[0].active_turn_id, 1);
        let shown = &app.tabs[0].chat.messages().last().unwrap().text;
        assert!(shown.contains("config.js"));
        assert!(!shown.contains("const hidden"));
        assert_eq!(
            app.extension_attachments
                .reserved_for_task(connection_id, &task_id),
            (0, 0)
        );
    }

    #[test]
    fn snapshot_history_and_resumed_chat_redact_attached_file_bodies() {
        let secret = "PRIVATE_ATTACHMENT_BODY_SHOULD_NOT_RENDER";
        let boundary = "ZODE-ATTACHMENT-zode_attachment_1";
        let attached = format!(
            "<attached_file name=\"secrets.txt\" media_type=\"text/plain\" boundary=\"{boundary}\">\n--- BEGIN {boundary} ---\n{secret}\n--- END {boundary} ---\n</attached_file>"
        );
        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![
                    ContentBlock::Text {
                        text: "inspect this file".into(),
                    },
                    ContentBlock::Text { text: attached },
                ],
            })
            .unwrap();

        let history = super::history_from_store("task-1", &store);
        let snapshot_wire = serde_json::to_string(&history.messages).unwrap();
        assert!(snapshot_wire.contains("inspect this file"));
        assert!(snapshot_wire.contains("secrets.txt"));
        assert!(!snapshot_wire.contains(secret));
        assert!(!snapshot_wire.contains("BEGIN ZODE-ATTACHMENT"));

        let chat = super::super::rebuild_chat_from_store(&store);
        let rendered = chat
            .messages()
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("inspect this file"));
        assert!(rendered.contains("secrets.txt"));
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("BEGIN ZODE-ATTACHMENT"));
    }

    #[test]
    fn noema_prefixed_attachment_only_history_and_chat_redact_the_envelope_body() {
        let secret = "NOEMA_PREFIXED_ATTACHMENT_SECRET";
        let boundary = "ZODE-ATTACHMENT-zode_attachment_noema";
        let attached = format!(
            "<attached_file name=\"only.txt\" media_type=\"text/plain\" boundary=\"{boundary}\">\n--- BEGIN {boundary} ---\n{secret}\n--- END {boundary} ---\n</attached_file>"
        );
        let stored = format!(
            "## Relevant Memories\n- remembered context\n## Subconscious Hints\n- private hint\n\n{attached}"
        );
        let mut store = MessageStore::new();
        store
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text { text: stored }],
            })
            .unwrap();

        let history = super::history_from_store("task-1", &store);
        let snapshot_wire = serde_json::to_string(&history.messages).unwrap();
        assert!(snapshot_wire.contains("only.txt"));
        assert!(!snapshot_wire.contains(secret));
        assert!(!snapshot_wire.contains("remembered context"));
        assert!(!snapshot_wire.contains("BEGIN ZODE-ATTACHMENT"));

        let chat = super::super::rebuild_chat_from_store(&store);
        let rendered = chat
            .messages()
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("only.txt"));
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("remembered context"));
        assert!(!rendered.contains("BEGIN ZODE-ATTACHMENT"));
    }

    #[tokio::test]
    async fn parse_error_send_failure_disconnects_and_clears_attachment_registry() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 844;
        let task_id = app.tabs[0].session_id.clone();
        let now = Instant::now();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        app.extension_attachments
            .begin(
                connection_id,
                task_id,
                BeginUpload {
                    file_name: "pending.txt".into(),
                    media_type: "text/plain".into(),
                    declared_size: 1,
                },
                now,
            )
            .unwrap();
        app.extension_tasks.fail_send_after_for_test(0);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "bad-base64",
                    "attachment/chunk",
                    serde_json::json!({
                        "uploadId":"zode_upload_missing",
                        "sequence":0,
                        "data":"***"
                    }),
                )),
            },
            &tx,
        );

        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        assert!(!app.extension_tasks.connection_is_live(connection_id));
        assert_eq!(
            app.extension_attachments
                .pending_for_connection(connection_id),
            (0, 0)
        );
    }

    #[tokio::test]
    async fn server_busy_send_failure_disconnects_and_clears_attachment_registry() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 845;
        let task_id = app.tabs[0].session_id.clone();
        let now = Instant::now();
        app.extension_tasks.connected(connection_id);
        app.extension_tasks
            .set_bridge_active_for_test(Some(connection_id));
        app.extension_attachments
            .begin(
                connection_id,
                task_id,
                BeginUpload {
                    file_name: "pending.txt".into(),
                    media_type: "text/plain".into(),
                    declared_size: 1,
                },
                now,
            )
            .unwrap();
        for index in 0..EXTENSION_PENDING_REQUEST_LIMIT {
            assert!(app
                .extension_tasks
                .begin_request(connection_id, &format!("pending-{index}")));
        }
        app.extension_tasks.fail_send_after_for_test(0);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "overflow",
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );

        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        assert!(!app.extension_tasks.connection_is_live(connection_id));
        assert_eq!(
            app.extension_attachments
                .pending_for_connection(connection_id),
            (0, 0)
        );
    }

    #[test]
    fn extension_turn_ids_are_canonical_positive_u64() {
        let parsed = super::parse_extension_request(
            "turn/interrupt",
            serde_json::json!({"taskId":"task-1","turnId":"18446744073709551615"}),
        )
        .expect("u64::MAX is canonical");
        assert!(matches!(
            parsed,
            crate::event::ExtensionTaskRequest::TurnInterrupt {
                task_id,
                turn_id: u64::MAX,
            } if task_id == "task-1"
        ));

        for turn_id in [
            serde_json::json!(0),
            serde_json::json!(1),
            serde_json::json!("0"),
            serde_json::json!("01"),
            serde_json::json!("+1"),
            serde_json::json!(" 1"),
            serde_json::json!("18446744073709551616"),
        ] {
            let error = super::parse_extension_request(
                "turn/interrupt",
                serde_json::json!({"taskId":"task-1","turnId":turn_id}),
            )
            .unwrap_err();
            assert_eq!(error.code(), "invalid_params", "turnId={turn_id}");
        }
    }

    #[test]
    fn extension_approval_response_dto_is_strict_and_decisions_are_exact() {
        for (wire, expected) in [
            ("allow", crate::event::ExtensionApprovalDecision::Allow),
            (
                "allowAlways",
                crate::event::ExtensionApprovalDecision::AllowAlways,
            ),
            ("deny", crate::event::ExtensionApprovalDecision::Deny),
        ] {
            let parsed = super::parse_extension_request(
                "approval/respond",
                serde_json::json!({
                    "taskId": "task-1",
                    "turnId": "7",
                    "approvalId": "approval-3",
                    "decision": wire,
                }),
            )
            .expect("canonical approval response parses");
            assert!(matches!(
                parsed,
                crate::event::ExtensionTaskRequest::ApprovalRespond {
                    task_id,
                    turn_id: 7,
                    approval_id,
                    decision,
                } if task_id == "task-1" && approval_id == "approval-3" && decision == expected
            ));
        }

        for params in [
            serde_json::json!({
                "taskId":"task-1","turnId":"7","approvalId":"approval-3",
                "decision":"allowOnce"
            }),
            serde_json::json!({
                "taskId":"task-1","turnId":"7","approvalId":"approval-3",
                "decision":"ALLOW"
            }),
            serde_json::json!({
                "taskId":"task-1","turnId":7,"approvalId":"approval-3",
                "decision":"deny"
            }),
            serde_json::json!({
                "taskId":"task-1","turnId":"07","approvalId":"approval-3",
                "decision":"deny"
            }),
            serde_json::json!({
                "taskId":"task-1","turnId":"7","approvalId":"",
                "decision":"deny"
            }),
            serde_json::json!({
                "taskId":"task-1","turnId":"7","approvalId":"approval-3",
                "decision":"deny","extra":true
            }),
        ] {
            let error =
                super::parse_extension_request("approval/respond", params.clone()).unwrap_err();
            assert_eq!(error.code(), "invalid_params", "params={params}");
        }
    }

    #[test]
    fn public_tool_identity_strips_wire_controls_and_caps_utf8_bytes() {
        assert_eq!(
            super::public_tool_identity("mcp__server__search"),
            "MCP server.search"
        );
        let raw = format!(
            "mcp__bad\0\n\u{0085}\u{2028}server__secret-sentinel-{}",
            "你".repeat(200)
        );
        let identity = super::public_tool_identity(&raw);
        assert!(identity.len() <= 200, "{} bytes", identity.len());
        assert!(identity.is_char_boundary(identity.len()));
        assert!(!identity.chars().any(|character| {
            matches!(character, '\u{2028}' | '\u{2029}')
                || ('\u{0000}'..='\u{001f}').contains(&character)
                || ('\u{007f}'..='\u{009f}').contains(&character)
        }));
    }

    #[tokio::test]
    async fn tool_started_uses_only_the_shared_public_tool_identity() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 515;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.tabs[0].active_turn_id = 5;
        app.tabs[0].turn_abort = Some(AbortController::new());
        app.extension_tasks.turn_routes.insert(
            (tab_id, 5),
            super::ExtensionTurnRoute {
                task_id,
                connection_id: Some(connection_id),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        let raw = format!("bad\n\u{2029}secret-sentinel-{}", "你".repeat(200));
        let expected = super::public_tool_identity(&raw);

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 5,
            cost_label: None,
            event: agent::stream::Event::ToolUse {
                id: "tool-public".into(),
                name: raw.clone(),
                input: serde_json::json!({"secret":"input-secret-sentinel"}),
            },
        });

        let frame = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .find(|frame| frame["event"] == "tool/started")
            .expect("tool/started frame");
        assert_eq!(frame["params"]["tool"], expected);
        assert_eq!(frame["params"]["summary"], expected);
        let wire = serde_json::to_string(&frame).unwrap();
        assert!(!wire.contains(&raw));
        assert!(!wire.contains("input-secret-sentinel"));
        assert!(frame["params"]["tool"].as_str().unwrap().len() <= 200);
    }

    #[tokio::test]
    async fn extension_turn_start_rejects_unknown_attachment_without_mutating_task() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let task_id = app.tabs[0].session_id.clone();
        let active = app.active;
        let message_count = app.tabs[0].chat.messages().len();

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 501,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "attachment-start",
                    "turn/start",
                    serde_json::json!({
                        "taskId": task_id,
                        "input": "keep this draft",
                        "attachmentIds": ["ready-attachment"]
                    }),
                )),
            },
            &tx,
        );

        assert_eq!(app.active, active);
        assert_eq!(app.tabs[0].turn_seq, 0);
        assert_eq!(app.tabs[0].active_turn_id, 0);
        assert!(app.tabs[0].turn_abort.is_none());
        assert_eq!(app.tabs[0].chat.messages().len(), message_count);
        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);
        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1);
        let frame = serde_json::to_value(&frames[0].1).unwrap();
        assert_eq!(frame["kind"], "error");
        assert_eq!(frame["id"], "attachment-start");
        assert_eq!(frame["code"], "attachment_not_found");
    }

    #[tokio::test]
    async fn extension_turn_start_arms_background_and_responds_before_provider_events() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let terminal_tab_id = app.tabs[0].id;
        let terminal_task_id = app.tabs[0].session_id.clone();
        let background_tab_id = app.next_tab_id;
        app.next_tab_id += 1;
        app.tabs.push(SessionTab::new(
            background_tab_id,
            app.tabs[0].engine.clone(),
            "background-task".into(),
        ));
        let worker_slots = app.extension_tasks.worker_slots.clone();
        let _index_workers_blocked = worker_slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();
        let literal_input = "  /clear and !rm stay literal  ";

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 502,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "start-background",
                    "turn/start",
                    serde_json::json!({
                        "taskId": "background-task",
                        "input": literal_input,
                        "attachmentIds": []
                    }),
                )),
            },
            &tx,
        );

        assert_eq!(app.active, 0);
        assert_eq!(app.tabs[0].id, terminal_tab_id);
        assert_eq!(app.tabs[0].session_id, terminal_task_id);
        assert!(app.tabs[0].turn_abort.is_none());
        let background = app
            .tabs
            .iter()
            .find(|tab| tab.id == background_tab_id)
            .unwrap();
        assert_eq!(background.active_turn_id, 1);
        assert_eq!(background.turn_seq, 1);
        assert!(background.turn_abort.is_some());
        assert_eq!(background.mode, Mode::Thinking);
        assert_eq!(
            background
                .chat
                .messages()
                .last()
                .map(|message| message.text.as_str()),
            Some(literal_input)
        );
        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);

        let frames = app.extension_tasks.sent_frames_for_test();
        assert!(frames.len() >= 2);
        let response = serde_json::to_value(&frames[0].1).unwrap();
        let started = serde_json::to_value(&frames[1].1).unwrap();
        assert_eq!(response["kind"], "response");
        assert_eq!(response["id"], "start-background");
        assert_eq!(response["result"]["turnId"], "1");
        assert_eq!(started["kind"], "event");
        assert_eq!(started["event"], "turn/started");
        assert_eq!(started["params"]["taskId"], "background-task");
        assert_eq!(started["params"]["turnId"], "1");
    }

    #[tokio::test]
    async fn extension_stream_fanout_is_sanitized_and_terminal_is_single_shot() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 503;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(connection_id);
        app.tabs[0].turn_seq = 9;
        app.tabs[0].active_turn_id = 9;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.extension_tasks.turn_routes.insert(
            (tab_id, 9),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: Some(connection_id),
                state: super::ExtensionTurnRouteState::Running,
            },
        );

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 9,
            cost_label: None,
            event: agent::stream::Event::TextDelta {
                delta: "hello".into(),
            },
        });
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 9,
            cost_label: None,
            event: agent::stream::Event::ToolUse {
                id: "tool-secret".into(),
                name: "shell".into(),
                input: serde_json::json!({"command":"DO-NOT-LEAK-INPUT"}),
            },
        });
        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 9,
            cost_label: None,
            event: agent::stream::Event::ToolResult {
                id: "tool-secret".into(),
                ok: false,
                output: serde_json::json!({"stdout":"DO-NOT-LEAK-OUTPUT"}),
            },
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 9,
            result: Ok(()),
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 9,
            result: Ok(()),
        });

        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "message/delta")
                .count(),
            1
        );
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "tool/started")
                .count(),
            1
        );
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "tool/completed")
                .count(),
            1
        );
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "turn/completed")
                .count(),
            1
        );
        let wire = serde_json::to_string(&values).unwrap();
        assert!(!wire.contains("DO-NOT-LEAK-INPUT"));
        assert!(!wire.contains("DO-NOT-LEAK-OUTPUT"));
        let tool_result = values
            .iter()
            .find(|frame| frame["event"] == "tool/completed")
            .unwrap();
        assert_eq!(tool_result["params"]["failed"], true);
        assert_eq!(tool_result["params"]["taskId"], task_id);
        assert_eq!(tool_result["params"]["turnId"], "9");
        assert!(!app.extension_tasks.turn_routes.contains_key(&(tab_id, 9)));
    }

    #[tokio::test]
    async fn extension_interrupt_claims_tui_turn_and_is_idempotent() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 504;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].turn_seq = 7;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());

        let interrupt = |id: &str, turn_id: &str| TaskInbound {
            connection_id,
            kind: TaskInboundKind::Request(TaskClientFrame::request(
                id,
                "turn/interrupt",
                serde_json::json!({"taskId": task_id, "turnId": turn_id}),
            )),
        };
        app.dispatch_extension_inbound(interrupt("stop-first", "7"), &tx);
        app.dispatch_extension_inbound(interrupt("stop-duplicate", "7"), &tx);
        app.dispatch_extension_inbound(interrupt("stop-future", "8"), &tx);

        assert!(app.tabs[0].turn_abort.is_none());
        assert_eq!(app.tabs[0].active_turn_id, 0);
        assert_eq!(app.tabs[0].draining_turn_id, Some(7));
        let route = app
            .extension_tasks
            .turn_routes
            .get(&(tab_id, 7))
            .expect("TUI-origin turn gets an extension terminal route");
        assert_eq!(route.task_id, task_id);
        assert_eq!(route.connection_id, Some(connection_id));
        assert_eq!(
            route.state,
            super::ExtensionTurnRouteState::InterruptRequested
        );
        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);

        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(values[0]["kind"], "response");
        assert_eq!(values[0]["id"], "stop-first");
        assert_eq!(values[1]["event"], "turn/stopping");
        assert_eq!(values[2]["kind"], "response");
        assert_eq!(values[2]["id"], "stop-duplicate");
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "turn/stopping")
                .count(),
            1
        );
        let future = values
            .iter()
            .find(|frame| frame["id"] == "stop-future")
            .unwrap();
        assert_eq!(future["kind"], "error");
        assert_eq!(future["code"], "turn_not_found");
    }

    #[tokio::test]
    async fn extension_interrupted_stale_turn_done_releases_exact_drain_and_terminal_once() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let connection_id = 505;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].turn_seq = 7;
        app.tabs[0].active_turn_id = 7;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "stop",
                    "turn/interrupt",
                    serde_json::json!({"taskId": task_id, "turnId": "7"}),
                )),
            },
            &tx,
        );

        app.handle_agent_event(AppEvent::Agent {
            tab_id,
            turn_id: 7,
            cost_label: None,
            event: agent::stream::Event::TextDelta {
                delta: "late delta".into(),
            },
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 6,
            result: Ok(()),
        });
        assert_eq!(app.tabs[0].draining_turn_id, Some(7));
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 7,
            result: Err("provider observed abort".into()),
        });
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 7,
            result: Err("duplicate terminal".into()),
        });

        assert_eq!(app.tabs[0].draining_turn_id, None);
        assert!(!app.extension_tasks.turn_routes.contains_key(&(tab_id, 7)));
        let tombstone = app
            .extension_tasks
            .recent_turns
            .iter()
            .find(|turn| turn.tab_id == tab_id && turn.turn_id == 7)
            .expect("interrupted turn is remembered");
        assert!(matches!(
            tombstone.terminal,
            super::ExtensionTurnTerminal::Interrupted
        ));
        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "turn/interrupted")
                .count(),
            1
        );
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "message/delta")
                .count(),
            0
        );
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "turn/failed")
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn closing_tab_clears_extension_turn_routes_and_tombstones() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let closed_tab_id = app.next_tab_id;
        app.next_tab_id += 1;
        app.tabs.push(SessionTab::new(
            closed_tab_id,
            app.tabs[0].engine.clone(),
            "closing-task".into(),
        ));
        app.active = 1;
        app.tabs[1].turn_seq = 3;
        app.tabs[1].active_turn_id = 3;
        app.tabs[1].turn_abort = Some(agent::abort::AbortController::new());
        app.extension_tasks.connected(506);
        app.extension_tasks.turn_routes.insert(
            (closed_tab_id, 3),
            super::ExtensionTurnRoute {
                task_id: "closing-task".into(),
                connection_id: Some(506),
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        app.extension_tasks.remember_turn_terminal(
            closed_tab_id,
            2,
            "closing-task".into(),
            super::ExtensionTurnTerminal::Completed,
        );

        app.close_active_tab();

        assert!(app.tabs.iter().all(|tab| tab.id != closed_tab_id));
        assert!(app
            .extension_tasks
            .turn_routes
            .keys()
            .all(|(tab_id, _)| *tab_id != closed_tab_id));
        assert!(app
            .extension_tasks
            .recent_turns
            .iter()
            .all(|turn| turn.tab_id != closed_tab_id));
        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(
            values
                .iter()
                .filter(|frame| frame["event"] == "turn/interrupted")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn completed_turn_interrupt_replays_terminal_without_touching_new_turn() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].turn_seq = 1;
        app.tabs[0].active_turn_id = 1;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.handle_agent_event(AppEvent::TurnDone {
            tab_id,
            turn_id: 1,
            result: Ok(()),
        });
        app.tabs[0].turn_seq = 2;
        app.tabs[0].active_turn_id = 2;
        let new_abort = agent::abort::AbortController::new();
        app.tabs[0].turn_abort = Some(new_abort);

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 507,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "old-stop",
                    "turn/interrupt",
                    serde_json::json!({"taskId": task_id, "turnId": "1"}),
                )),
            },
            &tx,
        );

        assert_eq!(app.tabs[0].active_turn_id, 2);
        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].draining_turn_id, None);
        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["kind"], "response");
        assert_eq!(values[0]["id"], "old-stop");
        assert_eq!(values[0]["result"]["alreadyFinished"], true);
        assert_eq!(values[1]["event"], "turn/completed");
        assert_eq!(values[1]["params"]["turnId"], "1");
    }

    #[tokio::test]
    async fn snapshot_read_refreshes_live_stopping_state_then_rebinds_orphan_route() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].active_turn_id = 0;
        app.tabs[0].turn_abort = None;
        app.tabs[0].draining_turn_id = Some(4);
        app.extension_tasks.turn_routes.insert(
            (tab_id, 4),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: None,
                state: super::ExtensionTurnRouteState::InterruptRequested,
            },
        );
        app.extension_tasks.connected(601);
        assert!(app.extension_tasks.begin_request(601, "snapshot-reconnect"));
        let stale_snapshot = serde_json::json!({
            "workspace": {"name":"workspace","path":"/tmp"},
            "tasks": [{
                "id": task_id,
                "title": "stale",
                "cwd": "/tmp",
                "model": "stale-model",
                "access": "prompt",
                "status": "idle",
                "activeTurnId": null
            }],
            "currentTaskId": task_id,
            "models": [],
            "messages": [],
            "tools": [],
            "approvals": []
        });

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id: 601,
                purpose: crate::event::ExtensionSnapshotPurpose::Response {
                    request_id: "snapshot-reconnect".into(),
                    rebind_orphan_routes: true,
                },
                result: Ok(stale_snapshot),
            },
            &tx,
        );

        let values = app.extension_tasks.sent_frames_for_test();
        assert_eq!(values.len(), 1);
        let response = serde_json::to_value(&values[0].1).unwrap();
        assert_eq!(response["kind"], "response");
        assert_eq!(response["result"]["tasks"][0]["status"], "stopping");
        assert_eq!(response["result"]["tasks"][0]["activeTurnId"], "4");
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 4))
                .unwrap()
                .connection_id,
            Some(601)
        );
        app.extension_tasks.disconnected(600);
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 4))
                .unwrap()
                .connection_id,
            Some(601),
            "a late old disconnect cannot clear a newer route binding"
        );
    }

    #[tokio::test]
    async fn ordinary_snapshot_response_does_not_rebind_orphan_route() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.turn_routes.insert(
            (tab_id, 1),
            super::ExtensionTurnRoute {
                task_id: task_id.clone(),
                connection_id: None,
                state: super::ExtensionTurnRouteState::Running,
            },
        );
        app.extension_tasks.connected(602);
        assert!(app.extension_tasks.begin_request(602, "select-response"));
        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id: 602,
                purpose: crate::event::ExtensionSnapshotPurpose::Response {
                    request_id: "select-response".into(),
                    rebind_orphan_routes: false,
                },
                result: Ok(serde_json::json!({
                    "tasks": [{"id": task_id}],
                    "currentTaskId": task_id,
                })),
            },
            &tx,
        );
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 1))
                .unwrap()
                .connection_id,
            None
        );
    }

    #[tokio::test]
    async fn turn_start_rejects_unknown_switching_and_busy_without_focus_mutation() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let active = app.active;
        let task_id = app.tabs[0].session_id.clone();
        let before_messages = app.tabs[0].chat.messages().len();
        let request = |id: &str, target: &str| TaskInbound {
            connection_id: 603,
            kind: TaskInboundKind::Request(TaskClientFrame::request(
                id,
                "turn/start",
                serde_json::json!({"taskId": target, "input": "literal"}),
            )),
        };

        app.dispatch_extension_inbound(request("unknown", "missing-task"), &tx);
        app.tabs[0].reassemble_pending = true;
        app.dispatch_extension_inbound(request("switching", &task_id), &tx);
        app.tabs[0].reassemble_pending = false;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.dispatch_extension_inbound(request("busy", &task_id), &tx);

        assert_eq!(app.active, active);
        assert_eq!(app.tabs[0].turn_seq, 0);
        assert_eq!(app.tabs[0].active_turn_id, 0);
        assert_eq!(app.tabs[0].chat.messages().len(), before_messages);
        assert!(app.extension_tasks.turn_routes.is_empty());
        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);
        let values: Vec<serde_json::Value> = app
            .extension_tasks
            .sent_frames_for_test()
            .into_iter()
            .map(|(_, frame)| serde_json::to_value(frame).unwrap())
            .collect();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0]["code"], "task_not_found");
        assert_eq!(values[1]["code"], "task_busy");
        assert_eq!(values[2]["code"], "task_busy");
    }

    #[tokio::test]
    async fn disconnect_orphans_running_route_without_aborting_turn() {
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let tab_id = app.tabs[0].id;
        let task_id = app.tabs[0].session_id.clone();
        app.tabs[0].active_turn_id = 5;
        app.tabs[0].turn_abort = Some(agent::abort::AbortController::new());
        app.extension_tasks.connected(604);
        app.extension_tasks.turn_routes.insert(
            (tab_id, 5),
            super::ExtensionTurnRoute {
                task_id,
                connection_id: Some(604),
                state: super::ExtensionTurnRouteState::Running,
            },
        );

        app.extension_tasks.disconnected(604);

        assert!(app.tabs[0].turn_abort.is_some());
        assert_eq!(app.tabs[0].active_turn_id, 5);
        assert_eq!(
            app.extension_tasks
                .turn_routes
                .get(&(tab_id, 5))
                .unwrap()
                .connection_id,
            None
        );
    }

    #[tokio::test]
    async fn extension_task_model_and_permission_params_are_strict() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();

        for (method, params) in [
            ("model/set", serde_json::json!({"taskId": task_id})),
            (
                "model/set",
                serde_json::json!({"taskId": task_id, "model": "test-model", "surprise": true}),
            ),
            ("permission/set", serde_json::json!({"taskId": task_id})),
            (
                "permission/set",
                serde_json::json!({"taskId": task_id, "mode": "prompt", "surprise": true}),
            ),
            (
                "permission/set",
                serde_json::json!({"taskId": task_id, "mode": "unknown"}),
            ),
        ] {
            let error = app
                .handle_extension_request(
                    1,
                    TaskClientFrame::request("strict-set", method, params),
                    &tx,
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), "invalid_params", "method={method}");
        }
    }

    #[tokio::test]
    async fn extension_task_model_and_permission_errors_do_not_mutate_task() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();
        let before_model = app.tabs[app.active].engine.model.clone();
        let before_access = app.tabs[app.active].extension_access;
        let before_seq = app.tabs[app.active].reassemble_seq;

        let unknown_model = app
            .handle_extension_request(
                1,
                TaskClientFrame::request(
                    "unknown-model",
                    "model/set",
                    serde_json::json!({"taskId": task_id, "model": "not-configured"}),
                ),
                &tx,
            )
            .await
            .unwrap_err();
        assert_eq!(unknown_model.code(), "model_not_found");

        let unknown_task = app
            .handle_extension_request(
                1,
                TaskClientFrame::request(
                    "unknown-task",
                    "permission/set",
                    serde_json::json!({"taskId": "missing", "mode": "auto"}),
                ),
                &tx,
            )
            .await
            .unwrap_err();
        assert_eq!(unknown_task.code(), "task_not_found");

        assert_eq!(app.tabs[app.active].engine.model, before_model);
        assert_eq!(app.tabs[app.active].extension_access, before_access);
        assert_eq!(app.tabs[app.active].reassemble_seq, before_seq);
        assert!(!app.tabs[app.active].reassemble_pending);

        let missing_unknown = app
            .handle_extension_request(
                1,
                TaskClientFrame::request(
                    "missing-unknown",
                    "model/set",
                    serde_json::json!({"taskId": "missing", "model": "not-configured"}),
                ),
                &tx,
            )
            .await
            .unwrap_err();
        assert_eq!(
            missing_unknown.code(),
            "task_not_found",
            "task lookup has precedence over model membership"
        );
    }

    #[tokio::test]
    async fn extension_task_changes_reject_all_three_busy_states_without_mutation() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();

        for state in ["turn", "draining", "reassemble"] {
            app.tabs[app.active].turn_abort = None;
            app.tabs[app.active].draining_turn_id = None;
            app.tabs[app.active].reassemble_pending = false;
            match state {
                "turn" => {
                    app.tabs[app.active].turn_abort = Some(agent::abort::AbortController::new())
                }
                "draining" => app.tabs[app.active].draining_turn_id = Some(1),
                "reassemble" => app.tabs[app.active].reassemble_pending = true,
                _ => unreachable!(),
            }
            let before_seq = app.tabs[app.active].reassemble_seq;
            let error = app
                .handle_extension_request(
                    1,
                    TaskClientFrame::request(
                        format!("busy-{state}"),
                        "permission/set",
                        serde_json::json!({"taskId": task_id, "mode": "auto"}),
                    ),
                    &tx,
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), "task_busy", "state={state}");
            assert_eq!(app.tabs[app.active].reassemble_seq, before_seq);
            assert_eq!(
                app.tabs[app.active].extension_access,
                zode_core::ToolAccessMode::Prompt
            );
        }
    }

    #[tokio::test]
    async fn extension_snapshot_access_is_independent_from_plan_mode() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();
        app.tabs[app.active].plan_mode = true;
        app.tabs[app.active].extension_access = zode_core::ToolAccessMode::Auto;

        let snapshot = app
            .handle_extension_request(
                1,
                TaskClientFrame::request(
                    "plan-auto",
                    "snapshot/read",
                    serde_json::json!({"taskId": task_id}),
                ),
                &tx,
            )
            .await
            .unwrap();
        let task = snapshot["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == task_id)
            .unwrap();
        assert_eq!(task["access"], "auto");
    }

    #[tokio::test]
    async fn extension_task_same_model_and_access_are_idempotent_even_while_busy() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();
        app.tabs[app.active].draining_turn_id = Some(1);
        let before_seq = app.tabs[app.active].reassemble_seq;

        for (method, params) in [
            (
                "model/set",
                serde_json::json!({"taskId": task_id, "model": "test-model"}),
            ),
            (
                "permission/set",
                serde_json::json!({"taskId": task_id, "mode": "prompt"}),
            ),
        ] {
            let snapshot = app
                .handle_extension_request(
                    1,
                    TaskClientFrame::request(format!("same-{method}"), method, params),
                    &tx,
                )
                .await
                .expect("same value is an idempotent snapshot");
            assert_eq!(snapshot["currentTaskId"], task_id);
        }
        assert_eq!(app.tabs[app.active].reassemble_seq, before_seq);
        assert!(!app.tabs[app.active].reassemble_pending);
    }

    #[tokio::test]
    async fn extension_permission_reassembles_background_target_without_focus_or_global_leakage() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let terminal_tab_id = app.tabs[app.active].id;
        let target_engine = app
            .template
            .assemble_tab(None, Some("77".into()))
            .await
            .unwrap();
        let target_cwd = target_engine.cwd.clone();
        let target_cost = target_engine.cost.clone();
        let mut target = SessionTab::new(77, Arc::new(target_engine), "background-task".into());
        target.plan_mode = true;
        target
            .engine
            .store
            .lock()
            .unwrap()
            .push(Message::User {
                header: Header::new(),
                content: vec![ContentBlock::Text {
                    text: "preserve me".into(),
                }],
            })
            .unwrap();
        app.tabs.push(target);
        app.next_tab_id = 78;
        app.extension_tasks
            .current_task_by_connection
            .insert(9, app.tabs[app.active].session_id.clone());

        let immediate = app
            .handle_extension_request(
                9,
                TaskClientFrame::request(
                    "permission-auto",
                    "permission/set",
                    serde_json::json!({"taskId":"background-task","mode":"auto"}),
                ),
                &tx,
            )
            .await
            .unwrap();

        assert_eq!(app.tabs[app.active].id, terminal_tab_id);
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt
        );
        assert_eq!(
            immediate["currentTaskId"], app.tabs[app.active].session_id,
            "mutation response keeps the connection's selected task"
        );
        assert_eq!(
            app.extension_tasks.current_task_by_connection.get(&9),
            Some(&app.tabs[app.active].session_id),
            "mutating a background task must not change the connection selection"
        );
        let switching = immediate["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "background-task")
            .unwrap();
        assert_eq!(switching["status"], "switching");
        assert_eq!(switching["access"], "auto");

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("background access reassembly finishes")
            .expect("event channel stays open");
        app.handle_agent_event(event);
        let target = app
            .tabs
            .iter()
            .find(|tab| tab.session_id == "background-task")
            .unwrap();
        assert_eq!(target.extension_access, zode_core::ToolAccessMode::Auto);
        assert_eq!(target.engine.cwd, target_cwd);
        assert!(target.plan_mode);
        assert!(Arc::ptr_eq(&target.engine.cost, &target_cost));
        assert_eq!(
            target.engine.store.lock().unwrap().len(),
            1,
            "conversation store survives access reassembly"
        );
        assert_eq!(app.tabs[app.active].id, terminal_tab_id);
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt
        );
    }

    #[tokio::test]
    async fn extension_model_reassembles_background_target_and_persists_index() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let terminal_tab_id = app.tabs[app.active].id;
        let target_engine = app
            .template
            .with_tool_access(zode_core::ToolAccessMode::Auto)
            .assemble_tab(None, Some("88".into()))
            .await
            .unwrap();
        let target_cwd = target_engine.cwd.clone();
        let target_cost = target_engine.cost.clone();
        let mut target = SessionTab::new(88, Arc::new(target_engine), "model-task".into());
        target.extension_access = zode_core::ToolAccessMode::Auto;
        target.plan_mode = true;
        target.title = "Model task".into();
        app.tabs.push(target);
        app.next_tab_id = 89;

        let immediate = app
            .handle_extension_request(
                12,
                TaskClientFrame::request(
                    "model-other",
                    "model/set",
                    serde_json::json!({"taskId":"model-task","model":"other-model"}),
                ),
                &tx,
            )
            .await
            .unwrap();
        let switching = immediate["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "model-task")
            .unwrap();
        assert_eq!(switching["status"], "switching");
        assert_eq!(switching["model"], "other-model");
        assert_eq!(switching["access"], "auto");

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("background model reassembly finishes")
            .expect("event channel stays open");
        app.handle_agent_event(event);
        let target = app
            .tabs
            .iter()
            .find(|tab| tab.session_id == "model-task")
            .unwrap();
        assert_eq!(target.engine.model, "other-model");
        assert_eq!(target.extension_access, zode_core::ToolAccessMode::Auto);
        assert_eq!(target.engine.cwd, target_cwd);
        assert!(target.plan_mode);
        assert!(Arc::ptr_eq(&target.engine.cost, &target_cost));
        assert_eq!(app.tabs[app.active].id, terminal_tab_id);
        assert_eq!(app.template.model(), Some("test-model"));
        assert_eq!(
            app.template.tool_access(),
            zode_core::ToolAccessMode::Prompt
        );

        let index = app.extension_tasks.sessions.load_index().await.unwrap();
        assert_eq!(
            index.find_prefix("model-task").unwrap().model,
            "other-model"
        );
    }

    #[tokio::test]
    async fn extension_model_index_failure_rolls_back_engine_model_access_and_index() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[app.active].session_id.clone();
        app.extension_tasks
            .sessions
            .upsert(SessionMeta {
                id: task_id.clone(),
                title: app.tabs[app.active].title.clone(),
                cwd: app.tabs[app.active].engine.cwd.display().to_string(),
                model: "test-model".into(),
                updated_at: 1,
            })
            .await
            .unwrap();
        app.extension_tasks.sessions.set_fail_upsert_for_test(true);
        let old_engine = app.tabs[app.active].engine.clone();
        let old_access = app.tabs[app.active].extension_access;

        let immediate = app
            .handle_extension_request(
                22,
                TaskClientFrame::request(
                    "model-persist-fails",
                    "model/set",
                    serde_json::json!({"taskId":task_id,"model":"other-model"}),
                ),
                &tx,
            )
            .await
            .unwrap();
        assert_eq!(
            immediate["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|task| task["id"] == task_id)
                .unwrap()["model"],
            "other-model"
        );

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("failed model change reports completion")
            .expect("event channel stays open");
        app.handle_agent_event(event);
        let tab = &app.tabs[app.active];
        assert!(Arc::ptr_eq(&tab.engine, &old_engine));
        assert_eq!(tab.engine.model, "test-model");
        assert_eq!(tab.extension_access, old_access);
        assert!(!tab.reassemble_pending);
        assert!(matches!(tab.mode, Mode::Ready));
        let index = app.extension_tasks.sessions.load_index().await.unwrap();
        assert_eq!(index.find_prefix(&task_id).unwrap().model, "test-model");
        assert_eq!(
            app.extension_tasks.pending_completions.len(),
            1,
            "failure still schedules authoritative snapshot then error"
        );
        assert_eq!(
            app.extension_tasks.pending_completions[0]
                .failure
                .as_ref()
                .unwrap()
                .0,
            "session_persist_failed"
        );
    }

    #[tokio::test]
    async fn extension_tasks_receiver_is_taken_once_and_absence_stays_pending() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));

        assert!(app.extension_task_rx.is_some());
        assert!(app
            .template
            .browser
            .take_extension_task_receiver()
            .is_none());
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_tx);
        app.extension_task_rx = Some(closed_rx);
        assert!(super::recv_extension_task(&mut app.extension_task_rx)
            .await
            .is_none());
        app.extension_task_rx = None;
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            super::recv_extension_task(&mut app.extension_task_rx)
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn extension_tasks_late_completion_does_not_resurrect_disconnected_client_state() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.handle_extension_request(
            77,
            TaskClientFrame::request("create", "task/create", serde_json::json!({})),
            &tx,
        )
        .await
        .unwrap();
        assert!(app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&77));

        app.extension_tasks.disconnected(77);
        app.extension_tasks.queue_completion(None);
        app.dispatch_extension_completions(&tx);

        assert!(!app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&77));
    }

    #[tokio::test]
    async fn extension_tasks_replacement_connection_receives_background_completion() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let created = app
            .handle_extension_request(
                77,
                TaskClientFrame::request("create", "task/create", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();
        let task_id = created["currentTaskId"].as_str().unwrap().to_string();
        app.extension_tasks.disconnected(77);
        app.handle_extension_request(
            78,
            TaskClientFrame::request(
                "reconnect",
                "snapshot/read",
                serde_json::json!({"taskId":task_id}),
            ),
            &tx,
        )
        .await
        .unwrap();

        assert_eq!(app.extension_tasks.completion_connections(), vec![78]);
    }

    #[tokio::test]
    async fn extension_tasks_failed_background_create_falls_back_to_terminal_task() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let terminal_active = app.active;
        let terminal_task = app.tabs[terminal_active].session_id.clone();
        app.handle_extension_request(
            88,
            TaskClientFrame::request("create", "task/create", serde_json::json!({})),
            &tx,
        )
        .await
        .unwrap();
        let placeholder_id = app.tabs[1].id;
        let placeholder_task = app.tabs[1].session_id.clone();

        app.handle_reassemble_done(
            placeholder_id,
            1,
            ReassembleEffect::ExtensionNewTab {
                connection_id: 88,
                failure_code: Some("engine_assemble_failed"),
            },
            Err("assembly failed".into()),
        );

        assert_eq!(app.active, terminal_active);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(
            app.extension_tasks.current_task_by_connection.get(&88),
            Some(&terminal_task)
        );
        assert!(!app
            .extension_tasks
            .pending_task_metadata
            .contains_key(&placeholder_task));

        let error = serde_json::to_value(super::extension_connection_error_frame(
            "engine_assemble_failed",
            "assembly failed",
        ))
        .unwrap();
        assert_eq!(error["kind"], "event");
        assert_eq!(error["event"], "connection/error");
        assert_eq!(error["params"]["code"], "engine_assemble_failed");
        assert_eq!(error["params"]["message"], "assembly failed");
    }

    #[test]
    fn extension_tasks_completion_sends_snapshot_before_failure() {
        let frames = super::extension_completion_frames(
            TaskServerFrame::event("snapshot", serde_json::json!({"currentTaskId":"task"})),
            Some(("engine_assemble_failed", "assembly failed")),
        );
        let events: Vec<String> = frames
            .into_iter()
            .map(|frame| {
                serde_json::to_value(frame).unwrap()["event"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();

        assert_eq!(events, ["snapshot", "connection/error"]);
    }

    #[tokio::test]
    async fn extension_tasks_failed_background_tab_keeps_same_focused_tab_id_after_vec_shift() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        for connection_id in [91, 92, 93] {
            app.handle_extension_request(
                connection_id,
                TaskClientFrame::request(
                    format!("create-{connection_id}"),
                    "task/create",
                    serde_json::json!({}),
                ),
                &tx,
            )
            .await
            .unwrap();
        }
        let failed_tab_id = app.tabs[1].id;
        app.active = 2;
        let focused_tab_id = app.tabs[app.active].id;

        app.handle_reassemble_done(
            failed_tab_id,
            1,
            ReassembleEffect::ExtensionNewTab {
                connection_id: 91,
                failure_code: Some("engine_assemble_failed"),
            },
            Err("assembly failed".into()),
        );

        assert_eq!(app.tabs[app.active].id, focused_tab_id);
    }

    #[tokio::test]
    async fn extension_tasks_failed_create_cleanup_removes_empty_session_and_index_entry() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let task_id = "failed-created-task";
        let path = sessions.join(format!("{task_id}.jsonl"));
        Session::save(&path, &MessageStore::new()).await.unwrap();
        app.extension_tasks
            .sessions
            .upsert(SessionMeta {
                id: task_id.into(),
                title: "New task".into(),
                cwd: "/workspace".into(),
                model: "model".into(),
                updated_at: 1,
            })
            .await
            .unwrap();

        app.extension_tasks.sessions.remove(task_id).await.unwrap();

        assert!(!path.exists());
        assert!(app
            .extension_tasks
            .sessions
            .load_index()
            .await
            .unwrap()
            .sessions
            .is_empty());
    }

    #[tokio::test]
    async fn extension_tasks_create_preflights_corrupt_index_without_ghost_task() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("index.json"), b"{broken json").unwrap();
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let original_tabs = app.tabs.len();
        let original_next_tab_id = app.next_tab_id;

        let error = app
            .handle_extension_request(
                109,
                TaskClientFrame::request("create", "task/create", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap_err();

        assert_eq!(error.code(), "internal_error");
        assert_eq!(app.tabs.len(), original_tabs);
        assert_eq!(app.next_tab_id, original_next_tab_id);
        assert!(app.extension_tasks.pending_task_metadata.is_empty());
        assert!(!app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&109));
    }

    #[tokio::test]
    async fn extension_tasks_create_rejects_directory_index_without_ghost_task() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        std::fs::create_dir_all(sessions.join("index.json")).unwrap();
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let original_tabs = app.tabs.len();
        let original_next_tab_id = app.next_tab_id;

        let error = app
            .handle_extension_request(
                110,
                TaskClientFrame::request("create", "task/create", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap_err();

        assert_eq!(error.code(), "internal_error");
        assert_eq!(app.tabs.len(), original_tabs);
        assert_eq!(app.next_tab_id, original_next_tab_id);
        assert!(app.extension_tasks.pending_task_metadata.is_empty());
        assert!(!app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&110));
    }

    #[tokio::test]
    async fn extension_tasks_remove_attempts_index_after_transcript_delete_failure() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, _tx, _rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let task_id = "delete-failure-task";
        app.extension_tasks
            .sessions
            .upsert(SessionMeta {
                id: task_id.into(),
                title: "Task".into(),
                cwd: "/workspace".into(),
                model: "model".into(),
                updated_at: 1,
            })
            .await
            .unwrap();
        let transcript_path = sessions.join(format!("{task_id}.jsonl"));
        std::fs::create_dir_all(transcript_path.join("child")).unwrap();

        let error = app
            .extension_tasks
            .sessions
            .remove(task_id)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("directory") || !error.to_string().is_empty());
        assert!(app
            .extension_tasks
            .sessions
            .load_index()
            .await
            .unwrap()
            .sessions
            .is_empty());
    }

    #[tokio::test]
    async fn extension_tasks_completion_after_placeholder_closed_prunes_state_and_resnapshots() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let terminal_task = app.tabs[0].session_id.clone();
        let created = app
            .handle_extension_request(
                101,
                TaskClientFrame::request("create", "task/create", serde_json::json!({})),
                &tx,
            )
            .await
            .unwrap();
        let task_id = created["currentTaskId"].as_str().unwrap().to_string();
        let tab_id = app.tabs[1].id;
        app.tabs.remove(1);

        app.handle_reassemble_done(
            tab_id,
            1,
            ReassembleEffect::ExtensionNewTab {
                connection_id: 101,
                failure_code: Some("engine_assemble_failed"),
            },
            Err("late failure".into()),
        );

        assert!(!app
            .extension_tasks
            .pending_task_metadata
            .contains_key(&task_id));
        assert_eq!(
            app.extension_tasks.current_task_by_connection.get(&101),
            Some(&terminal_task)
        );
    }

    #[tokio::test]
    async fn extension_tasks_last_placeholder_failure_keeps_a_safe_renderable_tab() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.handle_extension_request(
            111,
            TaskClientFrame::request("create", "task/create", serde_json::json!({})),
            &tx,
        )
        .await
        .unwrap();
        let placeholder_id = app.tabs[1].id;
        app.tabs.remove(0);
        app.active = 0;

        app.handle_reassemble_done(
            placeholder_id,
            1,
            ReassembleEffect::ExtensionNewTab {
                connection_id: 111,
                failure_code: Some("engine_assemble_failed"),
            },
            Err("assembly failed".into()),
        );

        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active, 0);
        assert_eq!(app.tabs[0].id, placeholder_id);
        assert!(!app.tabs[0].reassemble_pending);
        assert!(app.should_quit);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        app.print_resume_hint();
    }

    #[tokio::test]
    async fn extension_tasks_snapshot_dispatch_returns_while_index_worker_is_blocked() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let worker_lock = app.extension_tasks.sessions.explicit_root_lock.clone();
        let worker_guard = worker_lock.lock().await;

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 201,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "snapshot-blocked",
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );

        assert!(
            rx.try_recv().is_err(),
            "the blocked index worker must not finish before its barrier opens"
        );
        drop(worker_guard);
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("index worker completes after barrier opens")
            .expect("agent event channel stays open");
        assert!(matches!(
            event,
            AppEvent::ExtensionTask(crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 201,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn extension_tasks_history_worker_does_not_hold_the_event_dispatch_path() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.extension_tasks.connected(202);
        assert!(app.extension_tasks.begin_request(202, "history-blocked"));
        let worker_slots = app.extension_tasks.worker_slots.clone();
        let worker_guard = worker_slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 202,
                purpose: crate::event::ExtensionIndexPurpose::Request {
                    request_id: "history-blocked".into(),
                    request: crate::event::ExtensionTaskRequest::SnapshotRead { task_id: None },
                },
                result: Ok(SessionIndex::default()),
            },
            &tx,
        );

        assert!(
            rx.try_recv().is_err(),
            "history completion must wait behind the worker barrier"
        );
        drop(worker_guard);
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("history worker completes after barrier opens")
            .expect("agent event channel stays open");
        assert!(matches!(
            event,
            AppEvent::ExtensionTask(crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id: 202,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn extension_tasks_completion_dispatch_returns_while_index_worker_is_blocked() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let task_id = app.tabs[0].session_id.clone();
        app.extension_tasks.connected(203);
        app.extension_tasks
            .current_task_by_connection
            .insert(203, task_id);
        app.extension_tasks
            .queue_completion(Some(("engine_assemble_failed", "assembly failed")));
        let worker_lock = app.extension_tasks.sessions.explicit_root_lock.clone();
        let worker_guard = worker_lock.lock().await;

        app.dispatch_extension_completions(&tx);

        assert!(
            rx.try_recv().is_err(),
            "completion dispatch must not await the blocked index worker"
        );
        drop(worker_guard);
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("completion index worker finishes after barrier opens")
            .expect("agent event channel stays open");
        assert!(matches!(
            event,
            AppEvent::ExtensionTask(crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 203,
                purpose: crate::event::ExtensionIndexPurpose::Completion { .. },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn extension_tasks_disconnected_create_preflight_never_creates_a_ghost_placeholder() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let original_tabs = app.tabs.len();
        let original_next_tab_id = app.next_tab_id;
        let worker_lock = app.extension_tasks.sessions.explicit_root_lock.clone();
        let worker_slots = app.extension_tasks.worker_slots.clone();
        let worker_guard = worker_lock.lock().await;

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 204,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "create-disconnected",
                    "task/create",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 204,
                kind: TaskInboundKind::Disconnected,
            },
            &tx,
        );
        drop(worker_guard);
        let _workers_idle = tokio::time::timeout(
            Duration::from_secs(2),
            worker_slots.acquire_many(EXTENSION_WORKER_LIMIT as u32),
        )
        .await
        .expect("cancelled preflight releases its worker slot")
        .expect("worker semaphore stays open");

        assert!(
            rx.try_recv().is_err(),
            "cancelled preflight emitted an event"
        );
        assert_eq!(app.tabs.len(), original_tabs);
        assert_eq!(app.next_tab_id, original_next_tab_id);
        assert!(app.extension_tasks.pending_task_metadata.is_empty());
        assert!(!app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&204));
    }

    #[tokio::test]
    async fn extension_tasks_disconnected_select_preflight_never_restores_a_stale_placeholder() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        let sessions = config.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let mut index = SessionIndex::default();
        index.upsert(SessionMeta {
            id: "old-connection-task".into(),
            title: "Old connection task".into(),
            cwd: "/missing".into(),
            model: "test-model".into(),
            updated_at: 1,
        });
        std::fs::write(
            sessions.join("index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();
        app.extension_tasks
            .set_session_root_for_test(sessions.clone());
        let original_tabs = app.tabs.len();
        let original_next_tab_id = app.next_tab_id;
        let worker_lock = app.extension_tasks.sessions.explicit_root_lock.clone();
        let worker_slots = app.extension_tasks.worker_slots.clone();
        let worker_guard = worker_lock.lock().await;

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 205,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "select-disconnected",
                    "task/select",
                    serde_json::json!({"taskId":"old-connection-task"}),
                )),
            },
            &tx,
        );
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 205,
                kind: TaskInboundKind::Disconnected,
            },
            &tx,
        );
        drop(worker_guard);
        let _workers_idle = tokio::time::timeout(
            Duration::from_secs(2),
            worker_slots.acquire_many(EXTENSION_WORKER_LIMIT as u32),
        )
        .await
        .expect("cancelled select releases its worker slot")
        .expect("worker semaphore stays open");

        assert!(rx.try_recv().is_err(), "cancelled select emitted an event");
        assert_eq!(app.tabs.len(), original_tabs);
        assert_eq!(app.next_tab_id, original_next_tab_id);
        assert!(app.extension_tasks.pending_task_metadata.is_empty());
        assert!(!app
            .extension_tasks
            .current_task_by_connection
            .contains_key(&205));
    }

    #[tokio::test]
    async fn extension_tasks_replaced_index_result_is_fenced_before_disconnect_is_consumed() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let original_tabs = app.tabs.len();
        let original_next_tab_id = app.next_tab_id;
        app.extension_tasks.connected(301);
        assert!(app.extension_tasks.begin_request(301, "stale-create"));
        app.extension_tasks.set_bridge_active_for_test(Some(302));

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 301,
                purpose: crate::event::ExtensionIndexPurpose::Request {
                    request_id: "stale-create".into(),
                    request: crate::event::ExtensionTaskRequest::Create,
                },
                result: Ok(SessionIndex::default()),
            },
            &tx,
        );

        assert_eq!(app.tabs.len(), original_tabs);
        assert_eq!(app.next_tab_id, original_next_tab_id);
        assert!(app.extension_tasks.pending_task_metadata.is_empty());
    }

    #[tokio::test]
    async fn extension_tasks_replaced_snapshot_result_is_fenced_before_disconnect_is_consumed() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        app.extension_tasks.connected(303);
        assert!(app.extension_tasks.begin_request(303, "stale-snapshot"));
        app.extension_tasks.set_bridge_active_for_test(Some(304));

        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id: 303,
                purpose: crate::event::ExtensionSnapshotPurpose::Response {
                    request_id: "stale-snapshot".into(),
                    rebind_orphan_routes: true,
                },
                result: Ok(serde_json::json!({"currentTaskId":"stale"})),
            },
            &tx,
        );

        assert!(!app
            .extension_tasks
            .request_is_pending(303, "stale-snapshot"));
        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
    }

    #[tokio::test]
    async fn extension_tasks_stale_inbound_is_dropped_before_claim_or_io() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        app.extension_tasks.set_bridge_active_for_test(Some(312));

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 311,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "stale-inbound",
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );

        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 0);
        assert_eq!(app.extension_tasks.sessions.io_started_for_test(), 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn extension_tasks_pending_request_flood_is_capped() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let slots = app.extension_tasks.worker_slots.clone();
        let gate = slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();

        for index in 0..=EXTENSION_PENDING_REQUEST_LIMIT {
            app.dispatch_extension_inbound(
                TaskInbound {
                    connection_id: 401,
                    kind: TaskInboundKind::Request(TaskClientFrame::request(
                        format!("flood-{index}"),
                        "snapshot/read",
                        serde_json::json!({}),
                    )),
                },
                &tx,
            );
        }
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 401,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    format!("flood-{EXTENSION_PENDING_REQUEST_LIMIT}"),
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );

        assert_eq!(
            app.extension_tasks.pending_request_count_for_test(),
            EXTENSION_PENDING_REQUEST_LIMIT
        );
        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1);
        let value = serde_json::to_value(&frames[0].1).unwrap();
        assert_eq!(
            value["id"],
            format!("flood-{EXTENSION_PENDING_REQUEST_LIMIT}")
        );
        assert_eq!(value["code"], "server_busy");
        drop(gate);
    }

    #[tokio::test]
    async fn extension_tasks_duplicate_pending_id_never_emits_a_second_frame() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let slots = app.extension_tasks.worker_slots.clone();
        let gate = slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();
        let duplicate = || TaskInbound {
            connection_id: 402,
            kind: TaskInboundKind::Request(TaskClientFrame::request(
                "same-id",
                "snapshot/read",
                serde_json::json!({}),
            )),
        };

        app.dispatch_extension_inbound(duplicate(), &tx);
        app.dispatch_extension_inbound(duplicate(), &tx);

        assert_eq!(app.extension_tasks.pending_request_count_for_test(), 1);
        assert!(app.extension_tasks.sent_frames_for_test().is_empty());
        drop(gate);
    }

    #[tokio::test]
    async fn extension_tasks_malformed_duplicate_id_emits_exactly_one_error() {
        let (mut app, tx, _rx, _cwd) = make_test_app().await;
        let malformed = || TaskInbound {
            connection_id: 403,
            kind: TaskInboundKind::Request(TaskClientFrame::request(
                "malformed-id",
                "task/select",
                serde_json::json!({}),
            )),
        };

        app.dispatch_extension_inbound(malformed(), &tx);
        app.dispatch_extension_inbound(malformed(), &tx);

        let frames = app.extension_tasks.sent_frames_for_test();
        assert_eq!(frames.len(), 1);
        let value = serde_json::to_value(&frames[0].1).unwrap();
        assert_eq!(value["id"], "malformed-id");
        assert_eq!(value["code"], "invalid_params");
    }

    #[tokio::test]
    async fn extension_tasks_replacement_cancels_queued_index_io_before_it_starts() {
        let config = tempfile::tempdir().unwrap();
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        app.extension_tasks
            .set_session_root_for_test(config.path().join("sessions"));
        let slots = app.extension_tasks.worker_slots.clone();
        let gate = slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();

        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 404,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "old-queued",
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );
        app.extension_tasks.set_bridge_active_for_test(Some(405));
        app.dispatch_extension_inbound(
            TaskInbound {
                connection_id: 405,
                kind: TaskInboundKind::Request(TaskClientFrame::request(
                    "new-current",
                    "snapshot/read",
                    serde_json::json!({}),
                )),
            },
            &tx,
        );
        drop(gate);

        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("current connection worker completes")
            .expect("agent event channel stays open");
        assert!(matches!(
            event,
            AppEvent::ExtensionTask(crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 405,
                ..
            })
        ));
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err(), "cancelled worker emitted an event");
        assert_eq!(app.extension_tasks.sessions.io_started_for_test(), 1);
    }

    #[tokio::test]
    async fn extension_tasks_cancelled_snapshot_worker_does_not_block_replacement() {
        let (mut app, tx, mut rx, _cwd) = make_test_app().await;
        let slots = app.extension_tasks.worker_slots.clone();
        let gate = slots
            .acquire_many(EXTENSION_WORKER_LIMIT as u32)
            .await
            .unwrap();
        app.extension_tasks.connected(406);
        assert!(app.extension_tasks.begin_request(406, "old-history"));
        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 406,
                purpose: crate::event::ExtensionIndexPurpose::Request {
                    request_id: "old-history".into(),
                    request: crate::event::ExtensionTaskRequest::SnapshotRead { task_id: None },
                },
                result: Ok(SessionIndex::default()),
            },
            &tx,
        );
        app.extension_tasks.connected(407);
        assert!(app.extension_tasks.begin_request(407, "new-history"));
        app.handle_extension_task_event(
            crate::event::ExtensionTaskEvent::IndexReady {
                connection_id: 407,
                purpose: crate::event::ExtensionIndexPurpose::Request {
                    request_id: "new-history".into(),
                    request: crate::event::ExtensionTaskRequest::SnapshotRead { task_id: None },
                },
                result: Ok(SessionIndex::default()),
            },
            &tx,
        );
        drop(gate);

        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("replacement snapshot completes")
            .expect("agent event channel stays open");
        assert!(matches!(
            event,
            AppEvent::ExtensionTask(crate::event::ExtensionTaskEvent::SnapshotReady {
                connection_id: 407,
                ..
            })
        ));
        tokio::task::yield_now().await;
        assert!(
            rx.try_recv().is_err(),
            "cancelled snapshot emitted an event"
        );
    }
}
