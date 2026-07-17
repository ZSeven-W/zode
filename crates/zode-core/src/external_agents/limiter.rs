//! Process-wide concurrency cap for external CLI processes. The cap is read
//! at every acquire (from the caller's current merged config), so config
//! reloads apply immediately and every tab shares one budget — a per-engine
//! semaphore would let N tabs run N × maxConcurrent processes.

use std::sync::atomic::{AtomicU32, Ordering};

static RUNNING: AtomicU32 = AtomicU32::new(0);

pub struct ExternalLimiter;

/// RAII permit: dropping it releases the slot.
#[derive(Debug)]
pub struct LimiterPermit(());

impl ExternalLimiter {
    /// Try to take a slot under the CURRENT cap. `None` = at capacity.
    pub fn acquire(max: u32) -> Option<LimiterPermit> {
        let mut cur = RUNNING.load(Ordering::Relaxed);
        loop {
            if cur >= max {
                return None;
            }
            match RUNNING.compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return Some(LimiterPermit(())),
                Err(actual) => cur = actual,
            }
        }
    }
}

impl Drop for LimiterPermit {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the process-wide counter so a leaked/aborted permit from another
    /// test can't skew these assertions (the static is shared within the test
    /// binary).
    fn reset() {
        RUNNING.store(0, Ordering::SeqCst);
    }

    #[test]
    #[serial_test::serial]
    fn limiter_is_process_wide_and_releases_on_drop() {
        reset();
        let p1 = ExternalLimiter::acquire(2).unwrap();
        let _p2 = ExternalLimiter::acquire(2).unwrap();
        assert!(
            ExternalLimiter::acquire(2).is_none(),
            "third must be rejected"
        );
        drop(p1);
        assert!(ExternalLimiter::acquire(2).is_some());
    }

    #[test]
    #[serial_test::serial]
    fn cap_is_read_per_acquire() {
        reset();
        let _p1 = ExternalLimiter::acquire(1).unwrap();
        assert!(ExternalLimiter::acquire(1).is_none());
        // a raised cap admits immediately — no rebuild needed
        assert!(ExternalLimiter::acquire(2).is_some());
    }
}
