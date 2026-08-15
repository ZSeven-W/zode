//! UI control tools: the agent replaces the frontend or installs a skin
//! with a single tool call — one sentence from the user becomes a live
//! hot-swap, no compiler and no restart.
//!
//! These tools reach the process-wide UI host (installed by the app
//! entrypoint via `zode_core::ui::install_host`). Outside a harness launch
//! (unit tests, the app-server process) they report that no host is active.

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ui::{global_host, UiHost};

fn host_or_error() -> Result<std::sync::Arc<UiHost>, AgentError> {
    global_host().ok_or_else(|| AgentError::other("no active UI host (not a harness launch)"))
}

/// Register an agent-written JavaScript frontend at runtime.
#[derive(Debug, Default)]
pub struct UiRegisterTool;

#[derive(Debug, Deserialize)]
struct UiRegisterInput {
    id: String,
    source: String,
}

#[async_trait]
impl Tool for UiRegisterTool {
    fn name(&self) -> &str {
        "UiRegister"
    }

    fn description(&self) -> &str {
        "Register a new frontend written in JavaScript and make it available for          UiSwap. The source is a factory returning { serve(host) }; host provides          println(text), readLine(prompt, callback), emit/on (events), setSkin(json),          swapTo(id), and exit(). Ids must be unique."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "unique frontend id, e.g. js-table-view"},
                "source": {"type": "string", "description": "JavaScript source of the frontend"}
            },
            "required": ["id", "source"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let parsed: UiRegisterInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("UiRegister invalid input: {e}")))?;
        let host = host_or_error()?;
        // The registry keys live for the process lifetime; leaking the id
        // string matches that lifetime.
        let id: &'static str = Box::leak(parsed.id.into_boxed_str());
        host.register_js(id, parsed.source);
        Ok(json!({ "ok": true, "registered": host.registered() }))
    }
}

/// Hot-swap the active frontend to a registered one.
#[derive(Debug, Default)]
pub struct UiSwapTool;

#[derive(Debug, Deserialize)]
struct UiSwapInput {
    id: String,
}

#[async_trait]
impl Tool for UiSwapTool {
    fn name(&self) -> &str {
        "UiSwap"
    }

    fn description(&self) -> &str {
        "Hot-swap the active frontend at runtime. The target must be a registered          frontend id (built-ins: tui/headless/readline; agent-written ones via UiRegister).          The current frontend exits and the harness mounts the new one without a restart."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "frontend id to mount"}
            },
            "required": ["id"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let parsed: UiSwapInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("UiSwap invalid input: {e}")))?;
        let host = host_or_error()?;
        if !host.registered().iter().any(|id| *id == parsed.id) {
            return Err(AgentError::other(format!(
                "unknown frontend '{}'; registered: {:?}",
                parsed.id,
                host.registered()
            )));
        }
        // The host loop follows the swap; the active UI observes the event
        // and exits (the TUI polls its swap latch on the tick).
        host.ctx()
            .parallel_dyn("ui/swap", &json!({ "to": parsed.id }))
            .await
            .map_err(|e| AgentError::other(format!("UiSwap dispatch failed: {e}")))?;
        Ok(json!({ "ok": true, "swappingTo": parsed.id }))
    }
}

/// Install a runtime skin from its JSON.
#[derive(Debug, Default)]
pub struct SkinInstallTool;

#[derive(Debug, Deserialize)]
struct SkinInstallInput {
    json: String,
}

#[async_trait]
impl Tool for SkinInstallTool {
    fn name(&self) -> &str {
        "SkinInstall"
    }

    fn description(&self) -> &str {
        "Install a runtime skin (theme) from JSON — same schema as          ~/.zode/themes/*.json: name, description, colors (256-color palette          indices), optional icons and spinner. The UI re-renders on the next frame."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "json": {"type": "string", "description": "skin JSON (colors use 256-color palette indices as strings)"}
            },
            "required": ["json"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let parsed: SkinInstallInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("SkinInstall invalid input: {e}")))?;
        let host = host_or_error()?;
        let state = host
            .ctx()
            .use_service::<std::sync::Arc<crate::skin::SkinState>>("ui/skin")
            .map_err(|e| AgentError::other(format!("skin slot unavailable: {e}")))?;
        state
            .install(&parsed.json)
            .map_err(|e| AgentError::other(format!("SkinInstall failed: {e}")))?;
        Ok(json!({ "ok": true, "version": state.version() }))
    }
}
