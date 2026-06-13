//! Events that drive the TUI main loop. Terminal input comes straight from
//! crossterm's EventStream in the select!; the agent turn and the tick are
//! delivered as AppEvents over an mpsc channel.
//!
//! Each agent event carries the `turn_id` of the turn that produced it, so
//! the app can drop events from an aborted/superseded turn (the agent
//! `Event` itself has no turn identity).

use agent::stream::Event;

#[derive(Debug)]
pub enum AppEvent {
    /// One event from the running agent turn `turn_id`.
    Agent { turn_id: u64, event: Event },
    /// Turn `turn_id` finished (Ok) or errored.
    TurnDone {
        turn_id: u64,
        result: Result<(), String>,
    },
    /// A transient toast, posted from off-loop work (e.g. /undo running on a
    /// spawned task) so the event loop is never blocked.
    Toast { text: String, error: bool },
}
