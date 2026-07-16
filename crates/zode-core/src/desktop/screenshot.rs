//! Shared screenshot artifact helper: writes bytes to an owner-only dir with a
//! random filename and returns the `__agent_content_blocks__` sentinel (image
//! block + text path fallback). Used by both browser_read and desktop_screenshot
//! so file perms / media type / sentinel shape stay identical (spec §截图).

use std::path::Path;

use agent::error::AgentError;
use serde_json::{json, Value};

/// Create `dir` (owner-only on Unix), save `bytes` as a random-named jpg, and
/// return the content-blocks sentinel. Callers must drop any backend lease
/// before calling — this performs disk I/O.
pub fn save_screenshot_artifact(dir: &Path, bytes: &[u8]) -> Result<Value, AgentError> {
    std::fs::create_dir_all(dir).map_err(|e| AgentError::other(format!("shots dir: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let name = format!(
        "shot-{}-{}.jpg",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
    );
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| AgentError::other(format!("save screenshot: {e}")))?;
    let block = agent::attachments::image_from_bytes(bytes)
        .map_err(|e| AgentError::other(format!("screenshot encode: {e}")))?;
    Ok(json!({
        "__agent_content_blocks__": [serde_json::to_value(&block)
            .map_err(|e| AgentError::other(e.to_string()))?],
        "text": format!("screenshot saved: {}", path.display()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_file_and_emits_sentinel_only() {
        let dir = tempfile::tempdir().unwrap();
        let out = save_screenshot_artifact(dir.path(), &[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        let obj = out.as_object().unwrap();
        assert!(obj.contains_key("__agent_content_blocks__"));
        assert!(obj
            .keys()
            .all(|k| k == "__agent_content_blocks__" || k == "text"));
        assert!(obj["text"].as_str().unwrap().contains("screenshot saved: "));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn dir_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("shots");
        save_screenshot_artifact(&dir, &[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
