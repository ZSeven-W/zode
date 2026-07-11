//! Bounded outbound JSON-RPC channel and serialized writer.

use std::io;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use zode_app_server_protocol::{notify, JsonRpcMessage};

pub const OUTBOUND_CAPACITY: usize = 1024;

pub fn outbound() -> (mpsc::Sender<JsonRpcMessage>, mpsc::Receiver<JsonRpcMessage>) {
    mpsc::channel(OUTBOUND_CAPACITY)
}

/// Receive one batch, draining immediately available messages and coalescing
/// adjacent agent-message deltas for the same thread and turn.
pub(crate) async fn next_batch(
    rx: &mut mpsc::Receiver<JsonRpcMessage>,
) -> Option<Vec<JsonRpcMessage>> {
    let first = rx.recv().await?;
    let mut batch = vec![first];
    while let Ok(message) = rx.try_recv() {
        batch.push(message);
    }

    let mut coalesced = Vec::with_capacity(batch.len());
    let mut messages = batch.into_iter().peekable();
    while let Some(message) = messages.next() {
        let message = if let Some((thread_id, turn_id, mut delta)) = delta_parts(&message) {
            while let Some(next) = messages.peek() {
                let Some((next_thread_id, next_turn_id, next_delta)) = delta_parts(next) else {
                    break;
                };
                if next_thread_id != thread_id || next_turn_id != turn_id {
                    break;
                }
                delta.push_str(&next_delta);
                messages.next();
            }
            JsonRpcMessage::Notification(notify::agent_message_delta(&thread_id, &turn_id, &delta))
        } else {
            message
        };
        coalesced.push(message);
    }

    Some(coalesced)
}

/// Drains the receiver into `write` until all senders drop.
pub async fn writer_task<W: AsyncWrite + Unpin>(
    mut rx: mpsc::Receiver<JsonRpcMessage>,
    mut write: W,
) -> io::Result<()> {
    while let Some(batch) = next_batch(&mut rx).await {
        for message in batch {
            let encoded = zode_app_server_transport::stdio::encode_message(&message)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            write.write_all(encoded.as_bytes()).await?;
            write.flush().await?;
        }
    }

    Ok(())
}

fn delta_parts(message: &JsonRpcMessage) -> Option<(String, String, String)> {
    let JsonRpcMessage::Notification(notification) = message else {
        return None;
    };
    if notification.method != "item/agentMessage/delta" {
        return None;
    }
    let params = notification.params.as_ref()?;
    Some((
        params.get("threadId")?.as_str()?.to_owned(),
        params.get("turnId")?.as_str()?.to_owned(),
        params.get("delta")?.as_str()?.to_owned(),
    ))
}
