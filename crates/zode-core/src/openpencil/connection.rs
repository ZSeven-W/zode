//! Discover + connect to a live OpenPencil instance.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::client::{OpClient, ReqwestTransport, Transport};
use super::{install, launcher, Consent, OpError};
use crate::config::OpenPencilConfig;

/// Parsed `~/.openpencil/.op-mcp-port` (written by the Rust host on startup).
#[derive(Debug, Clone, Deserialize)]
pub struct PortFile {
    pub port: u16,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub transport: String,
}

impl PortFile {
    /// Returns the base HTTP URL for the OpenPencil MCP endpoint.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// Returns the platform-specific path to the OpenPencil port file.
/// Uses `dirs::home_dir()` so it works on Windows, macOS, and Linux.
pub fn port_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".openpencil").join(".op-mcp-port"))
}

/// Parse the contents of an `.op-mcp-port` JSON file.
pub fn parse_port_file(s: &str) -> Result<PortFile, OpError> {
    serde_json::from_str::<PortFile>(s).map_err(|e| OpError::Parse(e.to_string()))
}

/// Try to discover a running OpenPencil instance by reading the port file.
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn discover() -> Option<PortFile> {
    let raw = std::fs::read_to_string(port_file_path()?).ok()?;
    parse_port_file(&raw).ok()
}

/// Decision produced by [`plan_connect`]: what should `ensure` do next?
#[derive(Debug, PartialEq, Eq)]
pub enum ConnectAction {
    /// A live, token-verified instance is already up — reuse it.
    UseExisting,
    /// No live instance (or ping failed); launch one via `op start`.
    LaunchGui,
    /// No live instance and auto-launch is off, but headless mode is opted in.
    Headless,
    /// No live instance, auto-launch is off, headless not opted in.
    Error(String),
}

/// Pure connection decision. Takes booleans so the planner is testable without
/// I/O. No desktop-presence probe is done here — `op start` is the authority
/// (it errors clearly if no desktop app is found), so we attempt it whenever
/// auto-launch is on.
pub fn plan_connect(discovered: bool, ping_ok: bool, cfg: &OpenPencilConfig) -> ConnectAction {
    if discovered && ping_ok {
        return ConnectAction::UseExisting;
    }
    if cfg.auto_launch_gui() {
        return ConnectAction::LaunchGui;
    }
    if cfg.headless_fallback() {
        return ConnectAction::Headless;
    }
    ConnectAction::Error(
        "no live OpenPencil instance and auto-launch is off; open OpenPencil, \
         or set openpencil.autoLaunchGui / headlessFallback"
            .into(),
    )
}

/// Validate a `ping` result against the port-file token (pure → testable).
/// Requires server=="openpencil-mcp", mode=="live", and matching token.
pub fn ping_result_ok(result: &Value, pf: &PortFile) -> bool {
    result.get("server").and_then(|v| v.as_str()) == Some("openpencil-mcp")
        && result.get("mode").and_then(|v| v.as_str()) == Some("live")
        && result.get("token").and_then(|v| v.as_str()) == Some(pf.token.as_str())
}

/// Send a `ping` JSON-RPC call and validate the response.
async fn ping(client: &OpClient, pf: &PortFile) -> bool {
    matches!(client.ping().await, Ok(r) if ping_result_ok(&r, pf))
}

/// Session-scoped connection manager.
#[derive(Debug, Default)]
pub struct OpConnection;

impl OpConnection {
    /// Ensure a live, token-verified OpenPencil connection exists, launching
    /// the GUI on demand if auto-launch is configured. Returns a ready
    /// [`OpClient`] or an [`OpError`].
    pub async fn ensure(
        cfg: &OpenPencilConfig,
        consent: &dyn Consent,
        tag: &str,
    ) -> Result<OpClient, OpError> {
        let transport: Arc<dyn Transport> = Arc::new(ReqwestTransport::new());
        let discovered = discover();
        let ping_ok = match &discovered {
            Some(pf) => ping(&OpClient::new(pf.base_url(), transport.clone()), pf).await,
            None => false,
        };
        match plan_connect(discovered.is_some(), ping_ok, cfg) {
            ConnectAction::UseExisting => {
                let pf = discovered.expect("discovered is Some when UseExisting");
                Ok(OpClient::new(pf.base_url(), transport))
            }
            ConnectAction::LaunchGui => {
                // Ensure `op` is available (install on demand), then let
                // `op start` handle the actual desktop detection — it errors
                // clearly if no GUI app is installed.
                let op = install::ensure_op(cfg, consent, tag).await?;
                let prompt = format!(
                    "Launch OpenPencil? Runs: {} ({})",
                    op.display(),
                    cfg.launch_command()
                );
                if !consent.confirm(&prompt).await {
                    return Err(OpError::LaunchDeclined);
                }
                launcher::launch_gui(&op, cfg)?;
                poll_until_live(cfg, transport).await
            }
            ConnectAction::Headless => Err(OpError::NoInstance(
                "headless mode requires op-host-web-server (not in releases)".into(),
            )),
            ConnectAction::Error(m) => Err(OpError::NoInstance(m)),
        }
    }
}

/// Poll for a live, token-verified instance until the configured timeout.
async fn poll_until_live(
    cfg: &OpenPencilConfig,
    transport: Arc<dyn Transport>,
) -> Result<OpClient, OpError> {
    let deadline = Duration::from_millis(cfg.connect_timeout_ms());
    let step = Duration::from_millis(200);
    let mut waited = Duration::ZERO;
    while waited < deadline {
        if let Some(pf) = discover() {
            let client = OpClient::new(pf.base_url(), transport.clone());
            if ping(&client, &pf).await {
                return Ok(client);
            }
        }
        tokio::time::sleep(step).await;
        waited += step;
    }
    Err(OpError::NoInstance(
        "GUI did not come up in time; is the OpenPencil desktop app installed? \
         (`op start` handles detection)"
            .into(),
    ))
}

/// Report discovery + live-ping status (used by the `/op status` command).
/// This is a zode-side diagnostic, not an MCP tool.
pub async fn connection_status(_cfg: &OpenPencilConfig) -> String {
    match discover() {
        None => "OpenPencil: no live instance (no ~/.openpencil/.op-mcp-port)".into(),
        Some(pf) => {
            let client = OpClient::new(pf.base_url(), Arc::new(ReqwestTransport::new()));
            if ping(&client, &pf).await {
                format!("OpenPencil: live at {} (token-verified)", pf.base_url())
            } else {
                format!(
                    "OpenPencil: port file at {} but ping failed/not live",
                    pf.base_url()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── pure planner tests ────────────────────────────────────────────────────

    #[test]
    fn plan_uses_existing_when_pingable() {
        assert!(matches!(
            plan_connect(true, true, &crate::config::OpenPencilConfig::default()),
            ConnectAction::UseExisting
        ));
    }

    #[test]
    fn plan_launches_when_autolaunch_on() {
        let c = crate::config::OpenPencilConfig::default(); // auto_launch_gui defaults true
        assert!(matches!(
            plan_connect(false, false, &c),
            ConnectAction::LaunchGui
        ));
        // A dead ping (port file exists but ping failed) should also relaunch.
        assert!(matches!(
            plan_connect(true, false, &c),
            ConnectAction::LaunchGui
        ));
    }

    #[test]
    fn plan_errors_when_autolaunch_off_no_fallback() {
        let c = crate::config::OpenPencilConfig {
            auto_launch_gui: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            plan_connect(false, false, &c),
            ConnectAction::Error(_)
        ));
    }

    #[test]
    fn plan_headless_when_opted_in() {
        let c = crate::config::OpenPencilConfig {
            auto_launch_gui: Some(false),
            headless_fallback: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            plan_connect(false, false, &c),
            ConnectAction::Headless
        ));
    }

    // ── ping result validation ────────────────────────────────────────────────

    #[test]
    fn ping_result_validates_server_mode_token() {
        let pf = PortFile {
            port: 1,
            token: "t".into(),
            transport: "json-rpc".into(),
        };
        assert!(ping_result_ok(
            &serde_json::json!({"server":"openpencil-mcp","mode":"live","token":"t"}),
            &pf
        ));
        // wrong mode
        assert!(!ping_result_ok(
            &serde_json::json!({"server":"openpencil-mcp","mode":"web-canvas","token":"t"}),
            &pf
        ));
        // wrong token
        assert!(!ping_result_ok(
            &serde_json::json!({"server":"openpencil-mcp","mode":"live","token":"x"}),
            &pf
        ));
        // wrong server
        assert!(!ping_result_ok(
            &serde_json::json!({"server":"other","mode":"live","token":"t"}),
            &pf
        ));
    }

    // ── port-file parsing (kept from original) ────────────────────────────────

    #[test]
    fn parses_op_mcp_port_json() {
        let pf = parse_port_file(r#"{"port":3142,"token":"abc","transport":"json-rpc"}"#).unwrap();
        assert_eq!(pf.port, 3142);
        assert_eq!(pf.token, "abc");
        assert_eq!(pf.base_url(), "http://127.0.0.1:3142");
    }

    #[test]
    fn malformed_port_file_errs() {
        assert!(parse_port_file("nope").is_err());
        assert!(parse_port_file(r#"{"token":"x"}"#).is_err()); // missing port
    }
}
