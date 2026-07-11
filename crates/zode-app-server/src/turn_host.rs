use agent::abort::AbortController;
use async_trait::async_trait;
use tokio::sync::mpsc;
use zode_app_server_protocol::types::Thread;

use crate::session::SessionMsg;

#[async_trait]
pub trait TurnHost: Send + 'static {
    /// Starts a turn and arranges for exactly one `TurnFinished` message eventually.
    async fn start_turn(
        &mut self,
        thread: &Thread,
        input: String,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    );
}
