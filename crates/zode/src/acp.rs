//! Agent Client Protocol adapter.
//!
//! ACP is deliberately a thin transport around the same engine, permission
//! gate, V1-compatible session journal, and checkpoint machinery used by the CLI/TUI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent::abort::AbortController;
use agent::message::MessageStore;
use agent::stream::Event;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ForkSessionRequest, ForkSessionResponse, Implementation,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse,
    McpCapabilities, McpServer, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptCapabilities, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionAdditionalDirectoriesCapabilities,
    SessionCapabilities, SessionForkCapabilities, StopReason, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Responder, Stdio};
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;
use zode_core::approval::{Approval, ApprovalGate};
use zode_core::config::{ConfigManager, ZodeConfig};
use zode_core::run_event::{RunEvent, RunEventContext, RunStatus, TurnOutcome, TurnRecorder};
use zode_core::session_meta::{title_from_prompt, SessionMeta};
use zode_core::sessions::{DurableSessionMeta, ForkRequest, SessionStore};
use zode_core::ZodeEngine;

mod support;

use support::*;

#[derive(Debug)]
struct AcpApprovalGate {
    connection: ConnectionTo<Client>,
    session_id: String,
}

#[async_trait]
impl ApprovalGate for AcpApprovalGate {
    fn interactive(&self) -> bool {
        true
    }

    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        let call_id = format!("permission-{}", Uuid::new_v4().simple());
        let update = ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .title(tool.to_string())
                .kind(tool_kind(tool))
                .status(ToolCallStatus::Pending)
                .raw_input(input.clone()),
        );
        let request = RequestPermissionRequest::new(
            self.session_id.clone(),
            update,
            vec![
                PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "allow_always",
                    "Always allow",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new("reject_once", "Reject", PermissionOptionKind::RejectOnce),
            ],
        );
        match self.connection.send_request(request).block_task().await {
            Ok(response) => match response.outcome {
                RequestPermissionOutcome::Selected(selected)
                    if selected.option_id.0.as_ref() == "allow_once" =>
                {
                    Approval::AllowOnce
                }
                RequestPermissionOutcome::Selected(selected)
                    if selected.option_id.0.as_ref() == "allow_always" =>
                {
                    Approval::AllowAlways
                }
                _ => Approval::Deny,
            },
            Err(_) => Approval::Deny,
        }
    }
}

struct AcpSession {
    engine: Arc<ZodeEngine>,
    meta: DurableSessionMeta,
    turn_lock: Arc<AsyncMutex<()>>,
}

impl Clone for AcpSession {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            meta: self.meta.clone(),
            turn_lock: self.turn_lock.clone(),
        }
    }
}

struct AcpState {
    cfg: ZodeConfig,
    store: SessionStore,
    sessions: AsyncMutex<HashMap<String, AcpSession>>,
    aborts: Mutex<HashMap<String, AbortController>>,
}

impl AcpState {
    async fn engine(
        &self,
        session_id: &str,
        cwd: &Path,
        additional_directories: &[PathBuf],
        mcp_servers: &[McpServer],
        connection: ConnectionTo<Client>,
        messages: Option<MessageStore>,
    ) -> Result<Arc<ZodeEngine>, String> {
        ensure_absolute(cwd)?;
        for root in additional_directories {
            ensure_absolute(root)?;
        }
        let mcp_config = acp_mcp_config(mcp_servers)?;
        let sandbox = resolve_sandbox(&self.cfg, cwd, additional_directories).await?;
        let gate: Arc<dyn ApprovalGate> = Arc::new(AcpApprovalGate {
            connection,
            session_id: session_id.to_string(),
        });
        let engine = ZodeEngine::assemble_with_tool_filter_and_mcp(
            &self.cfg,
            cwd.to_path_buf(),
            gate,
            sandbox,
            &today_date(),
            None,
            None,
            false,
            None,
            None,
            mcp_config,
        )
        .await
        .map_err(|error| error.to_string())?;
        if let Some(messages) = messages {
            *engine
                .store
                .lock()
                .map_err(|_| "message store lock poisoned".to_string())? = messages;
        }
        Ok(Arc::new(engine))
    }

    async fn new_session(
        &self,
        request: NewSessionRequest,
        connection: ConnectionTo<Client>,
    ) -> Result<NewSessionResponse, String> {
        let id = Uuid::new_v4().simple().to_string();
        let engine = self
            .engine(
                &id,
                &request.cwd,
                &request.additional_directories,
                &request.mcp_servers,
                connection,
                None,
            )
            .await?;
        let meta = DurableSessionMeta::new(SessionMeta {
            id: id.clone(),
            title: "(untitled)".into(),
            cwd: request.cwd.display().to_string(),
            model: engine.model.clone(),
            updated_at: now_secs(),
        });
        self.store.create(meta.clone()).map_err(|e| e.to_string())?;
        let snapshot = engine
            .store
            .lock()
            .map_err(|_| "message store lock poisoned".to_string())?
            .clone();
        self.store
            .save(&meta, &snapshot)
            .await
            .map_err(|e| e.to_string())?;
        self.sessions.lock().await.insert(
            id.clone(),
            AcpSession {
                engine,
                meta,
                turn_lock: Arc::new(AsyncMutex::new(())),
            },
        );
        Ok(NewSessionResponse::new(id))
    }

    async fn load_session(
        &self,
        request: LoadSessionRequest,
        connection: ConnectionTo<Client>,
    ) -> Result<LoadSessionResponse, String> {
        let id = request.session_id.0.to_string();
        let loaded = self.store.load(&id).await.map_err(|e| e.to_string())?;
        if Path::new(&loaded.meta.cwd) != request.cwd {
            return Err(format!(
                "session cwd mismatch: stored {}, requested {}",
                loaded.meta.cwd,
                request.cwd.display()
            ));
        }
        let engine = self
            .engine(
                &id,
                &request.cwd,
                &request.additional_directories,
                &request.mcp_servers,
                connection,
                Some(loaded.messages),
            )
            .await?;
        self.sessions.lock().await.insert(
            id,
            AcpSession {
                engine,
                meta: loaded.meta,
                turn_lock: Arc::new(AsyncMutex::new(())),
            },
        );
        Ok(LoadSessionResponse::new())
    }

    async fn fork_session(
        &self,
        request: ForkSessionRequest,
        connection: ConnectionTo<Client>,
    ) -> Result<ForkSessionResponse, String> {
        let source_id = request.session_id.0.to_string();
        let target_id = Uuid::new_v4().simple().to_string();
        let meta = self
            .store
            .fork(ForkRequest {
                source_id,
                target_id: target_id.clone(),
                parent_checkpoint_id: None,
                worktree: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        let loaded = self
            .store
            .load(&target_id)
            .await
            .map_err(|e| e.to_string())?;
        let engine = self
            .engine(
                &target_id,
                &request.cwd,
                &request.additional_directories,
                &request.mcp_servers,
                connection,
                Some(loaded.messages),
            )
            .await?;
        self.sessions.lock().await.insert(
            target_id.clone(),
            AcpSession {
                engine,
                meta,
                turn_lock: Arc::new(AsyncMutex::new(())),
            },
        );
        Ok(ForkSessionResponse::new(target_id))
    }

    async fn prompt(
        &self,
        request: PromptRequest,
        connection: ConnectionTo<Client>,
    ) -> Result<PromptResponse, String> {
        let id = request.session_id.0.to_string();
        let mut session = self
            .sessions
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("session not loaded: {id}"))?;
        let turn_lock = session.turn_lock.clone();
        let _one_turn = turn_lock.lock().await;
        let prompt = prompt_text(&request.prompt)?;
        if session.meta.title == "(untitled)" {
            let title = title_from_prompt(&prompt);
            match self.store.update_meta(&id, |meta| meta.title = title) {
                // Refresh the in-memory copies too — the end-of-turn save
                // republishes `session.meta`, and a stale clone would revert
                // the title to "(untitled)" in meta.json and the index.
                Ok(updated) => {
                    session.meta = updated.clone();
                    if let Some(entry) = self.sessions.lock().await.get_mut(&id) {
                        entry.meta = updated;
                    }
                }
                Err(error) => tracing::warn!("ACP session title update failed: {error}"),
            }
        }
        let turn_id = Uuid::new_v4().simple().to_string();
        let request_id = Uuid::new_v4().simple().to_string();
        let mut recorder = TurnRecorder::new(
            Some(self.store.clone()),
            RunEventContext::new(id.clone(), Some(turn_id), Some(request_id)),
        );
        recorder.start();
        let message_count = session
            .engine
            .store
            .lock()
            .map_err(|_| "message store lock poisoned".to_string())?
            .len();
        recorder
            .begin_checkpoint(
                &session.engine.checkpoints,
                session.engine.cwd.clone(),
                message_count,
            )
            .map_err(|e| e.to_string())?;

        let abort = AbortController::new();
        self.aborts
            .lock()
            .map_err(|_| "abort map lock poisoned".to_string())?
            .insert(id.clone(), abort.clone());
        let mut stream = match session.engine.turn(&prompt, abort.clone()).await {
            Ok(stream) => stream,
            Err(error) => {
                self.aborts.lock().ok().map(|mut map| map.remove(&id));
                recorder.record(RunEvent::Error {
                    code: "turn_start_error".into(),
                    message: error.to_string(),
                });
                recorder.complete(
                    Some(&session.engine.checkpoints),
                    false,
                    &TurnOutcome {
                        status: RunStatus::Failed,
                        stop_reason: None,
                        partial: false,
                    },
                );
                return Err(error.to_string());
            }
        };
        let mut stop_reason = StopReason::EndTurn;
        let mut raw_stop_reason: Option<String> = None;
        let mut failed = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => {
                    session.engine.cost.observe(&event).await;
                    recorder.record_agent(&event);
                    // A send failure (client disconnect) must not skip
                    // completion — journal the turn and persist the transcript,
                    // then stop streaming.
                    if send_event(&connection, &id, &event).is_err() {
                        failed = true;
                        recorder.record(RunEvent::Error {
                            code: "client_send_failed".into(),
                            message: "failed to forward event to ACP client".into(),
                        });
                        break;
                    }
                    if let Event::Result { data } = &event {
                        raw_stop_reason = data.stop_reason.clone();
                        stop_reason = map_stop_reason(data.stop_reason.as_deref());
                    }
                    if matches!(event, Event::Error { .. }) {
                        failed = true;
                    }
                }
                Err(error) => {
                    failed = true;
                    recorder.record(RunEvent::Error {
                        code: "stream_error".into(),
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
        let interrupted = abort.is_aborted();
        if interrupted {
            stop_reason = StopReason::Cancelled;
        }
        self.aborts.lock().ok().map(|mut map| map.remove(&id));
        session.engine.extract_post_turn_inline().await;
        let snapshot = session
            .engine
            .store
            .lock()
            .map_err(|_| "message store lock poisoned".to_string())?
            .clone();
        self.store
            .save(&session.meta, &snapshot)
            .await
            .map_err(|e| e.to_string())?;
        // Shared status derivation: a limit-terminated turn journals
        // LimitReached (not Completed) exactly like `-p`, and a mid-stream
        // failure is marked partial.
        let status = RunStatus::derive(interrupted, failed, raw_stop_reason.as_deref());
        recorder.complete(
            Some(&session.engine.checkpoints),
            true,
            &TurnOutcome {
                status,
                stop_reason: Some(stop_reason_name(stop_reason).into()),
                partial: failed,
            },
        );
        Ok(PromptResponse::new(stop_reason))
    }

    fn cancel(&self, id: &str) {
        if let Ok(aborts) = self.aborts.lock() {
            if let Some(abort) = aborts.get(id) {
                abort.abort_with_reason("ACP session/cancel");
            }
        }
    }
}

pub async fn run(cwd: &Path) -> i32 {
    match serve(cwd).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("zode acp: {error}");
            1
        }
    }
}

async fn serve(cwd: &Path) -> Result<(), String> {
    let mut cfg = ConfigManager::load(cwd).map_err(|e| e.to_string())?;
    cfg.resolve_provider_from_map();
    cfg.apply_env_fallbacks();
    let state = Arc::new(AcpState {
        cfg,
        store: SessionStore::open_default().map_err(|e| e.to_string())?,
        sessions: AsyncMutex::new(HashMap::new()),
        aborts: Mutex::new(HashMap::new()),
    });
    Agent
        .builder()
        .name("zode-acp")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                let capabilities = AgentCapabilities::new()
                    .load_session(true)
                    .prompt_capabilities(PromptCapabilities::new().embedded_context(true))
                    .mcp_capabilities(McpCapabilities::new().http(true).sse(true))
                    .session_capabilities(
                        SessionCapabilities::new()
                            .additional_directories(SessionAdditionalDirectoriesCapabilities::new())
                            .fork(SessionForkCapabilities::new()),
                    );
                responder.respond(
                    InitializeResponse::new(request.protocol_version)
                        .agent_capabilities(capabilities)
                        .agent_info(
                            Implementation::new("zode", env!("CARGO_PKG_VERSION")).title("Zode"),
                        ),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                move |request: NewSessionRequest,
                      responder: Responder<NewSessionResponse>,
                      cx: ConnectionTo<Client>| {
                    let state = state.clone();
                    async move {
                        let connection = cx.clone();
                        cx.spawn(async move {
                            match state.new_session(request, connection).await {
                                Ok(response) => responder.respond(response),
                                Err(error) => responder.respond_with_internal_error(error),
                            }
                        })
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                move |request: LoadSessionRequest,
                      responder: Responder<LoadSessionResponse>,
                      cx: ConnectionTo<Client>| {
                    let state = state.clone();
                    async move {
                        let connection = cx.clone();
                        cx.spawn(async move {
                            match state.load_session(request, connection).await {
                                Ok(response) => responder.respond(response),
                                Err(error) => responder.respond_with_internal_error(error),
                            }
                        })
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                move |request: ForkSessionRequest,
                      responder: Responder<ForkSessionResponse>,
                      cx: ConnectionTo<Client>| {
                    let state = state.clone();
                    async move {
                        let connection = cx.clone();
                        cx.spawn(async move {
                            match state.fork_session(request, connection).await {
                                Ok(response) => responder.respond(response),
                                Err(error) => responder.respond_with_internal_error(error),
                            }
                        })
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                move |request: PromptRequest,
                      responder: Responder<PromptResponse>,
                      cx: ConnectionTo<Client>| {
                    let state = state.clone();
                    async move {
                        let connection = cx.clone();
                        cx.spawn(async move {
                            match state.prompt(request, connection).await {
                                Ok(response) => responder.respond(response),
                                Err(error) => responder.respond_with_internal_error(error),
                            }
                        })
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                move |notification: CancelNotification, _cx| {
                    let state = state.clone();
                    async move {
                        state.cancel(notification.session_id.0.as_ref());
                        Ok(())
                    }
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        ContentBlock as AcpContentBlock, TextContent, ToolKind,
    };

    #[test]
    fn maps_tool_kinds() {
        assert_eq!(tool_kind("FileRead"), ToolKind::Read);
        assert_eq!(tool_kind("FileEdit"), ToolKind::Edit);
        assert_eq!(tool_kind("Bash"), ToolKind::Execute);
    }

    #[test]
    fn accepts_text_and_resource_prompts() {
        assert_eq!(
            prompt_text(&[AcpContentBlock::Text(TextContent::new("hello"))]).unwrap(),
            "hello"
        );
    }

    #[test]
    fn normalizes_client_supplied_stdio_and_http_mcp_servers() {
        use agent_client_protocol::schema::v1::{
            EnvVariable, HttpHeader, McpServerHttp, McpServerStdio,
        };
        let command = std::env::current_exe().unwrap();
        let config = acp_mcp_config(&[
            McpServer::Stdio(
                McpServerStdio::new("local", command)
                    .args(vec!["serve".into()])
                    .env(vec![EnvVariable::new("TOKEN", "secret")]),
            ),
            McpServer::Http(
                McpServerHttp::new("remote", "https://example.test/mcp")
                    .headers(vec![HttpHeader::new("Authorization", "Bearer secret")]),
            ),
        ])
        .unwrap()
        .unwrap();
        assert!(config.servers.contains_key("local"));
        assert!(config.servers.contains_key("remote"));
    }

    #[test]
    fn rejects_relative_or_duplicate_client_mcp_servers() {
        use agent_client_protocol::schema::v1::McpServerStdio;
        let relative = McpServer::Stdio(McpServerStdio::new("bad", "server"));
        assert!(acp_mcp_config(&[relative]).is_err());

        let command = std::env::current_exe().unwrap();
        let duplicate = McpServer::Stdio(McpServerStdio::new("same", command));
        assert!(acp_mcp_config(&[duplicate.clone(), duplicate]).is_err());
    }
}
