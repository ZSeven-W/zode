//! A thin named logger backed by `tracing` (no buffering by default).

use std::fmt::Display;
use std::sync::Arc;

/// Named logger facade. Messages go to the `cordis::<name>` tracing target,
/// so exporters/subscribers already configured for zode pick them up without
/// any extra in-process buffering.
#[derive(Debug, Clone)]
pub struct Logger {
    /// Logger name (usually the owning plugin or scope name).
    pub name: Arc<str>,
}

impl Logger {
    pub fn error(&self, message: impl Display) {
        self.event(tracing::Level::ERROR, message.to_string());
    }

    pub fn warn(&self, message: impl Display) {
        self.event(tracing::Level::WARN, message.to_string());
    }

    pub fn info(&self, message: impl Display) {
        self.event(tracing::Level::INFO, message.to_string());
    }

    pub fn debug(&self, message: impl Display) {
        self.event(tracing::Level::DEBUG, message.to_string());
    }

    fn event(&self, level: tracing::Level, message: String) {
        // Static callsite metadata: the logger name rides as a field on the
        // fixed `cordis` target so tracing's static callsite machinery works.
        match level {
            tracing::Level::ERROR => {
                tracing::error!(target: "cordis", name = %self.name, "{}", message)
            }
            tracing::Level::WARN => {
                tracing::warn!(target: "cordis", name = %self.name, "{}", message)
            }
            tracing::Level::INFO => {
                tracing::info!(target: "cordis", name = %self.name, "{}", message)
            }
            tracing::Level::DEBUG | tracing::Level::TRACE => {
                tracing::debug!(target: "cordis", name = %self.name, "{}", message)
            }
        }
    }
}
