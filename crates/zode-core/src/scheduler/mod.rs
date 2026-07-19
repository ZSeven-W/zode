//! Scheduler core: in-memory `/loop` jobs plus persisted `/schedule` jobs,
//! and the `due()` query that turns elapsed time into prompts to re-run.
//!
//! Pure and I/O-free by design — no file access, no clock other than the
//! `Instant`/`NaiveDateTime` passed in by the caller. The JSON store for
//! `ScheduleJob` persistence lands in a later task; this module only holds
//! the in-memory `Scheduler` and its due-check logic.

pub mod jobs;

pub use jobs::{LoopJob, ScheduleJob, ScheduleSpec};

use chrono::TimeZone;
use std::time::{Duration, Instant};

/// Something that is due to fire: `prompt` is what gets re-run, `kind`
/// distinguishes a `/loop` job from a `/schedule` job for the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct DueJob {
    pub prompt: String,
    pub kind: DueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DueKind {
    Loop {
        id: u32,
        owner: u64,
    },
    Schedule {
        id: String,
        /// The wall-clock instant this schedule job was due at, for Task 7's
        /// cross-process CAS dedup against `last_fired_ms`.
        fire_ms_hint: chrono::NaiveDateTime,
    },
}

/// In-process registry of `/loop` jobs plus the `/schedule` roster mirrored
/// from the store. `due()` is the only query: given the current `Instant`
/// (for loops) and wall clock (for schedules), it returns everything ready
/// to fire, advancing/retiring loop jobs and stamping `last_fired_ms` on
/// schedule jobs as it goes.
#[derive(Debug, Clone, Default)]
pub struct Scheduler {
    loops: Vec<LoopJob>,
    schedules: Vec<ScheduleJob>,
    next_loop_id: u32,
}

impl Scheduler {
    /// Register a new `/loop` job, returning its id.
    pub fn add_loop(
        &mut self,
        owner: u64,
        prompt: String,
        interval: Duration,
        max_runs: Option<u32>,
        now: Instant,
    ) -> u32 {
        self.next_loop_id += 1;
        let id = self.next_loop_id;
        self.loops.push(LoopJob {
            id,
            owner,
            prompt,
            interval,
            next_fire: now + interval,
            max_runs,
            runs: 0,
        });
        id
    }

    /// Stop a loop job by id, or all loop jobs when `id` is `None`. Returns
    /// how many were removed.
    pub fn stop_loop(&mut self, id: Option<u32>) -> usize {
        match id {
            Some(id) => {
                let before = self.loops.len();
                self.loops.retain(|j| j.id != id);
                before - self.loops.len()
            }
            None => {
                let count = self.loops.len();
                self.loops.clear();
                count
            }
        }
    }

    /// Current `/loop` roster.
    pub fn loops(&self) -> &[LoopJob] {
        &self.loops
    }

    /// Replace the `/schedule` roster wholesale (mirrors the store's
    /// contents; Task 7 owns loading/saving `schedules.json`).
    pub fn set_schedules(&mut self, schedules: Vec<ScheduleJob>) {
        self.schedules = schedules;
    }

    /// Current `/schedule` roster.
    pub fn schedules(&self) -> &[ScheduleJob] {
        &self.schedules
    }

    /// What's due to fire right now. `now` drives loop jobs (monotonic
    /// clock); `wall` drives schedule jobs (wall clock, naive local time).
    ///
    /// Loop jobs: skips backlog rather than firing multiple times for a
    /// single missed tick — `next_fire` always advances to `now + interval`,
    /// never accumulates. A job retires (is removed) once `runs >= max_runs`.
    ///
    /// Schedule jobs: only `enabled` jobs are considered. `last_fired_ms` is
    /// converted to a naive wall-clock timestamp and fed to
    /// `ScheduleSpec::next_after`; if that next-fire point is `<= wall`, the
    /// job fires and `last_fired_ms` is stamped to `wall`.
    pub fn due(&mut self, now: Instant, wall: chrono::NaiveDateTime) -> Vec<DueJob> {
        let mut due = Vec::new();

        for job in &mut self.loops {
            if job.next_fire <= now {
                due.push(DueJob {
                    prompt: job.prompt.clone(),
                    kind: DueKind::Loop {
                        id: job.id,
                        owner: job.owner,
                    },
                });
                job.runs += 1;
                job.next_fire = now + job.interval;
            }
        }
        self.loops
            .retain(|j| j.max_runs.is_none_or(|max| j.runs < max));

        for job in &mut self.schedules {
            if !job.enabled {
                continue;
            }
            let last_fired = job.last_fired_ms.map(epoch_ms_to_naive);
            let next_fire = job.spec.next_after(wall, last_fired);
            if next_fire <= wall {
                due.push(DueJob {
                    prompt: job.prompt.clone(),
                    kind: DueKind::Schedule {
                        id: job.id.clone(),
                        fire_ms_hint: next_fire,
                    },
                });
                job.last_fired_ms = Some(naive_to_epoch_ms(wall));
            }
        }

        due
    }

    /// Test-only escape hatch: rewind a loop job's `next_fire` so `due()`
    /// treats it as overdue without waiting on a real clock. `#[doc(hidden)]`
    /// rather than `#[cfg(test)]` so Task 9's `zode-tui` integration tests
    /// can reach it across the crate boundary too.
    #[doc(hidden)]
    pub fn rewind_loop_for_test(&mut self, id: u32, by: Duration) {
        if let Some(job) = self.loops.iter_mut().find(|j| j.id == id) {
            job.next_fire -= by;
        }
    }
}

/// Convert an epoch-millisecond wall-clock timestamp to naive local time.
/// `last_fired_ms` is stored as epoch ms (serializable, restart-safe); this
/// is the one spot the store's wire format meets the naive-time math that
/// `ScheduleSpec::next_after` operates on.
fn epoch_ms_to_naive(ms: u64) -> chrono::NaiveDateTime {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
        // Corrupt/out-of-range stored value: fall back to "now" rather than
        // panicking — treated as "just fired" so the job simply reschedules.
        .unwrap_or_else(|| chrono::Local::now().naive_local())
}

/// Inverse of [`epoch_ms_to_naive`]: naive local time back to epoch ms.
fn naive_to_epoch_ms(naive: chrono::NaiveDateTime) -> u64 {
    chrono::Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    /// Fixed wall-clock instant for schedule-job tests that don't care about
    /// the exact value, only that it's stable across calls in a test.
    fn sample_wall() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 18)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    #[test]
    fn loop_job_fires_and_respects_max_runs() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let id = s.add_loop(7, "check ci".into(), Duration::from_secs(60), Some(2), now);
        // Not due yet.
        assert!(s.due(now, sample_wall()).is_empty());
        // Due once the interval elapsed (simulate by rewinding next_fire).
        s.rewind_loop_for_test(id, Duration::from_secs(61));
        let due = s.due(now, sample_wall());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].prompt, "check ci");
        // Second run hits max_runs and the job retires.
        s.rewind_loop_for_test(id, Duration::from_secs(61));
        assert_eq!(s.due(now, sample_wall()).len(), 1);
        assert!(s.loops().is_empty(), "job retired after max_runs");
    }

    #[test]
    fn stop_loop_none_clears_all() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        s.add_loop(1, "a".into(), Duration::from_secs(60), None, now);
        s.add_loop(1, "b".into(), Duration::from_secs(60), None, now);
        assert_eq!(s.stop_loop(None), 2);
        assert!(s.loops().is_empty());
    }

    #[test]
    fn disabled_schedule_never_fires() {
        let mut s = Scheduler::default();
        s.set_schedules(vec![ScheduleJob {
            id: "ab12".into(),
            spec: ScheduleSpec::Interval { secs: 60 },
            prompt: "sync".into(),
            enabled: false,
            last_fired_ms: None,
        }]);
        // Far-future wall clock: still nothing because disabled.
        assert!(s.due(Instant::now(), sample_wall()).is_empty());
    }
}
