//! Discover + connect to a live OpenPencil instance.

use std::path::PathBuf;

use serde::Deserialize;

use super::OpError;

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

#[cfg(test)]
mod tests {
    use super::*;

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
