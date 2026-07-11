use agent::abort::AbortController;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use zode_app_server_protocol::types::Thread;
use zode_app_server_protocol::JsonRpcMessage;

use crate::accumulator::{TurnEndState, TurnOutcome};
use crate::outbound::{outbound, writer_task};
use crate::session::{SessionActor, SessionMsg};
use crate::turn_host::{HostFactory, TurnHost};

struct UnavailableHost {
    turn_ids: mpsc::UnboundedReceiver<String>,
}

struct UnavailableHostFactory;

impl HostFactory for UnavailableHostFactory {
    fn build_host(
        &mut self,
        _policy: zode_app_server_protocol::types::ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
    ) -> Box<dyn TurnHost> {
        // Task 8 replaces this temporary stdio wiring with ServerRuntimeOptions.
        Box::new(UnavailableHost { turn_ids })
    }
}

#[async_trait]
impl TurnHost for UnavailableHost {
    async fn start_turn(
        &mut self,
        thread: &Thread,
        _input: String,
        _abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    ) {
        let Some(turn_id) = self.turn_ids.recv().await else {
            return;
        };
        let _ = msgs
            .send(SessionMsg::TurnFinished {
                thread_id: thread.id.clone(),
                turn_id,
                outcome: TurnOutcome {
                    state: TurnEndState::Failed {
                        error: "EngineHost lands in Task 7".into(),
                    },
                    final_text: String::new(),
                    usage: Default::default(),
                },
            })
            .await;
    }
}

pub async fn run_stdio(zode_home: String) -> std::io::Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let (out_tx, out_rx) = outbound();
    let writer = tokio::spawn(writer_task(out_rx, tokio::io::stdout()));
    let (session, actor) =
        SessionActor::spawn(Box::new(UnavailableHostFactory), out_tx, zode_home, None);
    while let Some(line) = lines.next_line().await? {
        let Ok(JsonRpcMessage::Request(request)) =
            zode_app_server_transport::stdio::decode_line(&line)
        else {
            continue;
        };
        if session.send(SessionMsg::Rpc(request)).await.is_err() {
            break;
        }
    }
    let _ = session.send(SessionMsg::Shutdown).await;
    drop(session);
    let _ = actor.await;
    writer.await.map_err(std::io::Error::other)??;
    Ok(())
}
