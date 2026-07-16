//! Chrome Native Messaging framing for the extension autostart host.

use std::io::{self, Read, Write};

use serde_json::{json, Value};

const MAX_FRAME_BYTES: usize = 64 * 1024;

pub fn read_start_request() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let value = read_frame(&mut stdin)?;
    if value.get("type").and_then(Value::as_str) != Some("start") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native host expected a start request",
        ));
    }
    Ok(())
}

pub fn write_ready(port: u16) -> io::Result<()> {
    write_stdout(&json!({ "ok": true, "port": port }))
}

pub fn write_error(message: &str) -> io::Result<()> {
    write_stdout(&json!({ "ok": false, "error": message }))
}

/// After the initial start frame has been consumed, keep draining Chrome's
/// native pipe. EOF means the extension port closed and the daemon can exit.
pub fn spawn_disconnect_watcher() -> tokio::sync::oneshot::Receiver<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let mut stdin = io::stdin().lock();
        loop {
            if read_frame(&mut stdin).is_err() {
                break;
            }
        }
        let _ = tx.send(());
    });
    rx
}

fn write_stdout(value: &Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    write_frame(&mut stdout, value)?;
    stdout.flush()
}

fn read_frame(reader: &mut impl Read) -> io::Result<Value> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid native messaging frame length",
        ));
    }
    let mut bytes = vec![0u8; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn write_frame(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native messaging response is too large",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_message_frames_round_trip() {
        let value = json!({"type": "start", "unicode": "你好"});
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert_eq!(read_frame(&mut bytes.as_slice()).unwrap(), value);
    }

    #[test]
    fn native_message_rejects_oversized_frames_before_allocation() {
        let mut bytes = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let error = read_frame(&mut bytes.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
