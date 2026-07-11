//! Private on-disk discovery credentials for the WebSocket server.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

const SERVER_FILE_NAME: &str = "server.json";

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerFile {
    pub port: u16,
    pub pid: u32,
    pub token: String,
}

/// Atomically create `server.json`, refusing to replace an existing owner.
pub fn write_atomic(dir: &Path, file: &ServerFile) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(SERVER_FILE_NAME);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("server discovery file already exists: {}", path.display()),
        ));
    }

    let mut temp = tempfile::Builder::new()
        .prefix(".server.json.")
        .tempfile_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    serde_json::to_writer(temp.as_file_mut(), file).map_err(io::Error::other)?;
    temp.as_file_mut().write_all(b"\n")?;
    temp.as_file_mut().sync_all()?;
    temp.persist_noclobber(&path).map_err(|error| {
        let source = error.error;
        if source.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("server discovery file already exists: {}", path.display()),
            )
        } else {
            source
        }
    })?;
    Ok(path)
}

/// Remove `server.json` only when it is readable and owned by `pid`.
pub fn cleanup_if_owner(dir: &Path, pid: u32) {
    let path = dir.join(SERVER_FILE_NAME);
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    let Ok(file) = serde_json::from_slice::<ServerFile>(&bytes) else {
        return;
    };
    if file.pid == pid {
        let _ = fs::remove_file(path);
    }
}

pub fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(token, "{byte:02x}").expect("writing to a string cannot fail");
    }
    token
}
