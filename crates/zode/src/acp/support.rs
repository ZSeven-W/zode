use std::path::{Path, PathBuf};

use agent::stream::Event;
use agent_client_protocol::schema::v1::{
    ContentBlock as AcpContentBlock, ContentChunk, McpServer, SessionNotification, SessionUpdate,
    StopReason, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use zode_core::config::ZodeConfig;
use zode_core::sandbox::SandboxConfig;

pub(super) fn send_event(
    connection: &ConnectionTo<Client>,
    session_id: &str,
    event: &Event,
) -> Result<(), String> {
    let update = match event {
        Event::TextDelta { delta } => SessionUpdate::AgentMessageChunk(ContentChunk::new(
            AcpContentBlock::Text(TextContent::new(delta)),
        )),
        Event::Thinking { delta } => SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            AcpContentBlock::Text(TextContent::new(delta)),
        )),
        Event::ToolUse { id, name, input } => SessionUpdate::ToolCall(
            ToolCall::new(id.clone(), name.clone())
                .kind(tool_kind(name))
                .status(ToolCallStatus::InProgress)
                .raw_input(input.clone()),
        ),
        Event::ToolResult { id, ok, output } => SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            id.clone(),
            ToolCallUpdateFields::new()
                .status(if *ok {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                })
                .raw_output(output.clone()),
        )),
        _ => return Ok(()),
    };
    connection
        .send_notification(SessionNotification::new(session_id.to_string(), update))
        .map_err(|e| e.to_string())
}

pub(super) fn tool_kind(name: &str) -> ToolKind {
    match name {
        "FileRead" | "Read" => ToolKind::Read,
        "FileWrite" | "FileEdit" | "NotebookEdit" | "Mkdir" => ToolKind::Edit,
        "Remove" => ToolKind::Delete,
        "Move" => ToolKind::Move,
        "Bash" | "BashRun" | "KillShell" | "Git" => ToolKind::Execute,
        name if name.contains("search") || name.contains("Search") => ToolKind::Search,
        name if name.starts_with("Browser") || name.contains("fetch") => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

pub(super) fn prompt_text(blocks: &[AcpContentBlock]) -> Result<String, String> {
    let mut result = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            AcpContentBlock::Text(text) => result.push(text.text.clone()),
            AcpContentBlock::ResourceLink(link) => result.push(format!(
                "[Referenced resource: {}]",
                serde_json::to_string(link).map_err(|e| e.to_string())?
            )),
            AcpContentBlock::Resource(resource) => result.push(format!(
                "[Embedded resource: {}]",
                serde_json::to_string(resource).map_err(|e| e.to_string())?
            )),
            _ => return Err("prompt contains a content type not advertised by Zode".into()),
        }
    }
    Ok(result.join("\n\n"))
}

/// Normalize ACP's transport schema into the runtime MCP schema. Values remain
/// memory-only: env vars and headers can contain session-scoped credentials.
pub(super) fn acp_mcp_config(
    servers: &[McpServer],
) -> Result<Option<agent::mcp::McpConfig>, String> {
    let mut normalized = serde_json::Map::new();
    for server in servers {
        let (name, spec) = match server {
            McpServer::Stdio(server) => {
                if !server.command.is_absolute() {
                    return Err(format!(
                        "ACP MCP stdio command must be absolute: {}",
                        server.command.display()
                    ));
                }
                let env = server
                    .env
                    .iter()
                    .map(|entry| {
                        (
                            entry.name.clone(),
                            serde_json::Value::String(entry.value.clone()),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                (
                    server.name.clone(),
                    serde_json::json!({
                        "transport": "stdio",
                        "command": server.command,
                        "args": server.args,
                        "env": env,
                    }),
                )
            }
            McpServer::Http(server) => (
                server.name.clone(),
                remote_mcp_spec(&server.url, &server.headers)?,
            ),
            McpServer::Sse(server) => (
                server.name.clone(),
                remote_mcp_spec(&server.url, &server.headers)?,
            ),
            _ => return Err("ACP MCP transport is not supported by this Zode build".into()),
        };
        if name.is_empty()
            || name.len() > 128
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            return Err(format!("invalid ACP MCP server name: {name:?}"));
        }
        if normalized.insert(name.clone(), spec).is_some() {
            return Err(format!("duplicate ACP MCP server name: {name}"));
        }
    }
    if normalized.is_empty() {
        Ok(None)
    } else {
        zode_core::mcp::config_from_servers(normalized).map(Some)
    }
}

fn remote_mcp_spec(
    url: &str,
    headers: &[agent_client_protocol::schema::v1::HttpHeader],
) -> Result<serde_json::Value, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("ACP MCP URL must use http or https: {url}"));
    }
    let headers = headers
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                serde_json::Value::String(entry.value.clone()),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(serde_json::json!({
        "transport": "sse",
        "url": url,
        "headers": headers,
    }))
}

pub(super) async fn resolve_sandbox(
    cfg: &ZodeConfig,
    cwd: &Path,
    additional_directories: &[PathBuf],
) -> Result<Option<SandboxConfig>, String> {
    // Shared resolver: the same config (windowsTier, restrictReads, …) must
    // yield the same sandbox here as in the CLI/TUI entrypoint.
    let sandbox = zode_core::sandbox::resolve_with_overrides(
        &cfg.sandbox,
        cwd,
        &zode_core::sandbox::SandboxOverrides::default(),
        additional_directories,
    )
    .map_err(|e| e.to_string())?;
    if let Some(sandbox) = &sandbox {
        sandbox.verify().await.map_err(|e| e.to_string())?;
    }
    Ok(sandbox)
}

pub(super) fn ensure_absolute(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(format!(
            "ACP workspace path must be absolute: {}",
            path.display()
        ))
    }
}

pub(super) fn map_stop_reason(reason: Option<&str>) -> StopReason {
    match reason {
        Some("max_tokens") => StopReason::MaxTokens,
        Some("max_iterations" | "max_turn_requests") => StopReason::MaxTurnRequests,
        Some("refusal") => StopReason::Refusal,
        Some("cancelled" | "aborted") => StopReason::Cancelled,
        _ => StopReason::EndTurn,
    }
}

pub(super) fn stop_reason_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        _ => "unknown",
    }
}

pub(super) fn today_date() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
