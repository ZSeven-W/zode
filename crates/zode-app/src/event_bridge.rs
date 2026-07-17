use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{reduce_agent_event, ConnectionState, ReduceOutcome, ZodeAppState};
use zode_node_protocol::{AgentEndpoint, AgentEvent};

use crate::window_state::AppWake;

enum BridgeItem {
    Event(AgentEvent),
    Unavailable,
}

/// Main-thread drain for the endpoint stream. The producer always wakes winit,
/// so streamed text progresses while the user is completely idle.
pub struct AgentEventBridge {
    receiver: mpsc::UnboundedReceiver<BridgeItem>,
}

impl AgentEventBridge {
    /// Must be called while a Tokio runtime is entered.
    pub fn spawn(endpoint: Arc<dyn AgentEndpoint>, proxy: EventLoopProxy<AppWake>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let Ok(mut events) = endpoint.subscribe().await else {
                enqueue(&sender, &proxy, BridgeItem::Unavailable);
                return;
            };
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => enqueue(&sender, &proxy, BridgeItem::Event(event)),
                    Err(_) => {
                        enqueue(&sender, &proxy, BridgeItem::Unavailable);
                        return;
                    }
                }
            }
            enqueue(&sender, &proxy, BridgeItem::Unavailable);
        });
        Self { receiver }
    }

    pub fn drain_into(&mut self, state: &mut ZodeAppState) -> usize {
        let mut applied = 0;
        while let Ok(item) = self.receiver.try_recv() {
            match item {
                BridgeItem::Event(event) => {
                    if reduce_agent_event(state, event) == ReduceOutcome::Applied {
                        applied += 1;
                    }
                }
                BridgeItem::Unavailable => state.host.connection = ConnectionState::Unavailable,
            }
        }
        applied
    }
}

fn enqueue(
    sender: &mpsc::UnboundedSender<BridgeItem>,
    proxy: &EventLoopProxy<AppWake>,
    item: BridgeItem,
) {
    let queue_was_empty = sender.is_empty();
    if sender.send(item).is_ok() && queue_was_empty {
        let _ = proxy.send_event(AppWake::Redraw);
    }
}
