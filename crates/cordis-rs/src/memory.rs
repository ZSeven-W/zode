//! Memory accounting and budget caps.

use serde::Serialize;

/// Hard caps on harness growth.
///
/// Registration that would exceed a cap fails with
/// `CordisError::BudgetExceeded` instead of growing unbounded, so a
/// misbehaving plugin cannot exhaust memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemoryBudget {
    /// Maximum number of fibers (root fiber included).
    pub max_fibers: usize,
    /// Maximum number of fibers waiting for dependencies.
    pub max_pending: usize,
    /// Maximum number of provided services.
    pub max_services: usize,
    /// Maximum number of registered event listeners.
    pub max_listeners: usize,
    /// Maximum number of live contexts (scopes).
    pub max_contexts: usize,
    /// Event history ring-buffer size (older records are dropped).
    pub max_event_history: usize,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        MemoryBudget {
            max_fibers: 1024,
            max_pending: 1024,
            max_services: 512,
            max_listeners: 4096,
            max_contexts: 4096,
            max_event_history: 512,
        }
    }
}

impl MemoryBudget {
    /// A budget with no caps (history still bounded by `usize::MAX`).
    pub fn unlimited() -> Self {
        MemoryBudget {
            max_fibers: usize::MAX,
            max_pending: usize::MAX,
            max_services: usize::MAX,
            max_listeners: usize::MAX,
            max_contexts: usize::MAX,
            max_event_history: usize::MAX,
        }
    }
}

/// Live memory snapshot of a harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemoryStats {
    /// Live fibers (including the root fiber).
    pub fibers: usize,
    /// Fibers waiting for required services.
    pub pending_fibers: usize,
    /// Provided services.
    pub services: usize,
    /// Registered event listeners (normal + waterfall).
    pub listeners: usize,
    /// Distinct event names with listeners.
    pub events: usize,
    /// Live contexts (scopes).
    pub contexts: usize,
    /// Lazy services provided but never resolved (never allocated).
    pub lazy_uninitialized: usize,
    /// Retained event-history records.
    pub history_records: usize,
    /// Bytes retained by the event history (names + payloads).
    pub history_bytes: usize,
    /// Rough total estimate (object counts × average sizes + history bytes).
    pub estimated_bytes: usize,
}
