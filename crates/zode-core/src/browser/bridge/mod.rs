pub mod backend;
pub mod native_host;
pub mod server;
mod task_channel;
pub mod task_protocol;
pub use backend::BridgeBackend;
pub use server::{BridgeServer, PairingHandle};
pub use task_channel::{TaskInbound, TaskInboundKind, TaskReceiver};
pub use task_protocol::{
    TaskChannel, TaskClientBody, TaskClientFrame, TaskServerBody, TaskServerFrame,
};

use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const PAIRING_TTL: Duration = Duration::from_secs(120);
const MAX_PAIRING_ATTEMPTS: u8 = 5;
const TOKEN_FILE: &str = "browser-bridge.json";

/// One-shot pairing challenge. 6-digit code, 2-min TTL, single-use, invalidated
/// after 5 wrong attempts.
#[derive(Debug)]
pub(crate) struct Pairing {
    code: String,
    expiry: Instant,
    attempts: u8,
    consumed: bool,
}

impl Pairing {
    pub(crate) fn new(now: Instant) -> Self {
        let code = format!("{:06}", OsRng.gen_range(0..1_000_000));
        Self::with_code_inner(code, now)
    }

    #[cfg(test)]
    fn with_code(code: &str, now: Instant) -> Self {
        Self::with_code_inner(code.to_string(), now)
    }

    fn with_code_inner(code: String, now: Instant) -> Self {
        Self {
            code,
            expiry: now + PAIRING_TTL,
            attempts: 0,
            consumed: false,
        }
    }

    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    pub(crate) fn redeem(&mut self, code: &str, now: Instant) -> Result<String, PairError> {
        if self.consumed || self.attempts >= MAX_PAIRING_ATTEMPTS {
            return Err(PairError::LockedOut);
        }
        if now >= self.expiry {
            return Err(PairError::Expired);
        }
        if code != self.code {
            self.attempts = self.attempts.saturating_add(1);
            let remaining = MAX_PAIRING_ATTEMPTS.saturating_sub(self.attempts);
            return Err(if remaining == 0 {
                PairError::LockedOut
            } else {
                PairError::Wrong { remaining }
            });
        }

        self.consumed = true;
        Ok(BridgeToken::generate().token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PairError {
    Expired,
    LockedOut,
    Wrong { remaining: u8 },
}

/// Persisted long-term credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BridgeToken {
    pub token: String,
    pub extension_id: Option<String>,
}

impl BridgeToken {
    pub(crate) fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self {
            token: bytes_to_hex(&bytes),
            extension_id: None,
        }
    }

    pub(crate) fn load() -> Option<Self> {
        let path = token_path().ok()?;
        let mut file = OpenOptions::new().read(true).open(path).ok()?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).ok()?;
        serde_json::from_str(&buf).ok()
    }

    pub(crate) fn save(&self) -> std::io::Result<()> {
        let path = token_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub(crate) fn verify(&self, presented: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), presented.as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RpcRequest {
    pub id: u64,
    pub kind: RpcKind,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RpcKind {
    Cdp,
    Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RpcResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClientHello {
    Pair { code: String },
    Auth { token: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerHello {
    Paired { token: String },
    Ok,
    Rejected { reason: String },
}

fn token_path() -> std::io::Result<std::path::PathBuf> {
    crate::config::ConfigManager::config_dir()
        .map(|dir| dir.join(TOKEN_FILE))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for i in 0..max {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        diff |= (a ^ b) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::time::{Duration, Instant};

    #[test]
    fn pairing_code_is_six_digits() {
        let p = Pairing::new(Instant::now());
        assert_eq!(p.code().len(), 6);
        assert!(p.code().chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn redeem_succeeds_with_correct_code_then_is_single_use() {
        let now = Instant::now();
        let mut p = Pairing::new(now);
        let code = p.code().to_string();
        let tok = p.redeem(&code, now).unwrap();
        assert!(!tok.is_empty());
        assert!(p.redeem(&code, now).is_err());
    }

    #[test]
    fn redeem_expires_after_ttl() {
        let now = Instant::now();
        let mut p = Pairing::new(now);
        let code = p.code().to_string();
        let late = now + Duration::from_secs(121);
        assert!(matches!(p.redeem(&code, late), Err(PairError::Expired)));
    }

    #[test]
    fn five_wrong_attempts_lock_out() {
        let now = Instant::now();
        let mut p = Pairing::with_code("123456", now);
        for _ in 0..5 {
            let _ = p.redeem("000000", now);
        }
        assert!(matches!(p.redeem("123456", now), Err(PairError::LockedOut)));
    }

    #[test]
    fn token_verify_is_constant_time_equal() {
        let t = BridgeToken::generate();
        assert!(t.verify(&t.token));
        assert!(!t.verify("deadbeef"));
        assert_eq!(t.token.len(), 64);
    }

    #[test]
    #[serial]
    fn token_save_load_roundtrip_uses_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let token = BridgeToken {
            token: "a".repeat(64),
            extension_id: Some("abcdefghijklmnopabcdefghijklmnop".into()),
        };

        token.save().unwrap();
        let loaded = BridgeToken::load().unwrap();

        assert_eq!(loaded.token, token.token);
        assert_eq!(loaded.extension_id, token.extension_id);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("browser-bridge.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        std::env::remove_var("ZODE_CONFIG_DIR");
    }
}
