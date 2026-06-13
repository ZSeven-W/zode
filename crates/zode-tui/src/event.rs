//! Events that drive the TUI main loop. Terminal input comes straight from
//! crossterm's EventStream in the select!; the agent turn and the tick are
//! delivered as AppEvents over an mpsc channel.

use agent::stream::Event;

#[derive(Debug)]
pub enum AppEvent {
    /// One event from the running agent turn.
    Agent(Event),
    /// The current turn finished (Ok) or errored.
    TurnDone(Result<(), String>),
}
