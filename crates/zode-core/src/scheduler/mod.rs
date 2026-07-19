//! Scheduler core: in-memory `/loop` jobs plus persisted `/schedule` jobs,
//! and the `due()` query that turns elapsed time into prompts to re-run.
//!
//! `Scheduler` itself is pure and I/O-free — no file access, no clock other
//! than the `Instant`/`NaiveDateTime` passed in by the caller. The JSON
//! store for `ScheduleJob` persistence (load/save/fire-dedup against
//! `<config-dir>/schedules.json`) lives in `store`, kept separate so the
//! in-memory due-check logic stays testable without touching the
//! filesystem.

pub mod jobs;
mod store;

pub use jobs::{LoopJob, ScheduleJob, ScheduleSpec};
pub use store::*;

use chrono::TimeZone;
use std::collections::HashMap;
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
    /// Per-schedule-job in-memory baseline: the wall clock the job was first
    /// *observed* by `due()` after entering the roster. Used as the anchor for
    /// `ScheduleSpec::next_after` while `last_fired_ms` is still unknown, so a
    /// job that has never fired schedules its first fire relative to when it
    /// joined the roster rather than relative to "now" (which, because
    /// `next_after` is strictly-after, could never be reached — see `due`).
    ///
    /// Captured lazily inside `due()` from that call's `wall` argument, never
    /// from a hidden clock read, so `due()` stays a pure function of
    /// `(now, wall)` plus the scheduler's own state.
    schedule_baselines: HashMap<String, chrono::NaiveDateTime>,
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
        // Drop baselines for jobs that are no longer in the roster; a job that
        // is re-added later must re-anchor from the moment it re-enters rather
        // than resurrecting a stale baseline. Jobs that survive the swap keep
        // theirs, so `/schedule enable` (which round-trips the whole roster
        // through this method) doesn't silently re-anchor everything.
        self.schedule_baselines
            .retain(|id, _| schedules.iter().any(|j| &j.id == id));
        self.schedules = schedules;
    }

    /// Current `/schedule` roster.
    pub fn schedules(&self) -> &[ScheduleJob] {
        &self.schedules
    }

    /// Disable a `/schedule` job by id (sets `enabled = false` in the
    /// in-memory roster). Returns whether a matching job was found. Callers
    /// that need the change to survive a restart persist it separately via
    /// `save_schedules(scheduler.schedules())` — this method has no I/O.
    pub fn disable_schedule(&mut self, id: &str) -> bool {
        match self.schedules.iter_mut().find(|j| j.id == id) {
            Some(job) => {
                job.enabled = false;
                true
            }
            None => false,
        }
    }

    /// What's due to fire right now. `now` drives loop jobs (monotonic
    /// clock); `wall` drives schedule jobs (wall clock, naive local time).
    ///
    /// Loop jobs: skips backlog rather than firing multiple times for a
    /// single missed tick — `next_fire` always advances to `now + interval`,
    /// never accumulates. A job retires (is removed) once `runs >= max_runs`.
    ///
    /// Schedule jobs: only `enabled` jobs are considered. The next-fire point
    /// is computed by anchoring `ScheduleSpec::next_after` on the job's own
    /// history — NOT on `wall`. `next_after` is documented to return a time
    /// strictly *after* its `now` argument, so anchoring it on `wall` and then
    /// testing `next_fire <= wall` could never be true and no schedule could
    /// ever fire. The anchor is therefore the later of:
    ///
    /// - the job's `last_fired_ms` (when known), and
    /// - the job's in-memory baseline: the `wall` of the first `due()` call
    ///   that observed it (see [`Scheduler::schedule_baselines`]).
    ///
    /// Taking the later of the two is what makes missed triggers *skipped*
    /// rather than replayed: after a restart a daily job whose `last_fired_ms`
    /// is three days old anchors on the startup baseline, so its next fire is
    /// the upcoming occurrence, not three backlogged ones.
    ///
    /// When that anchored next-fire point is `<= wall` the job fires, and
    /// `last_fired_ms` is stamped to the *trigger point* (the same value
    /// reported as `fire_ms_hint`), not to `wall` — so cross-process CAS dedup
    /// and the in-process "don't fire the same trigger twice" check agree on
    /// one canonical instant regardless of how late the tick observed it.
    ///
    /// `due()` reads no clock of its own: everything is derived from `now`,
    /// `wall`, and stored state.
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

        // Disjoint field borrows: the loop mutates `self.schedules` while
        // reading/inserting into `self.schedule_baselines`.
        let baselines = &mut self.schedule_baselines;
        for job in &mut self.schedules {
            if !job.enabled {
                continue;
            }
            // Baseline is captured the first time a job is observed, even when
            // it is not going to fire — so it is anchored to roster-entry time
            // rather than to whenever the first fire-eligible tick happens.
            let baseline = *baselines.entry(job.id.clone()).or_insert(wall);
            let last_fired = job.last_fired_ms.and_then(epoch_ms_to_naive);
            let anchor = match last_fired {
                Some(lf) if lf > baseline => lf,
                _ => baseline,
            };
            let next_fire = job.spec.next_after(anchor, last_fired);
            if next_fire > wall {
                continue;
            }
            // Stamp the trigger point, not `wall`. A `None` here means the
            // trigger point has no valid local instant (spring-forward gap):
            // skip the fire entirely rather than substituting epoch 0, which
            // would read back as a real, very old fire and permanently corrupt
            // the anchor.
            let Some(fired_ms) = naive_to_epoch_ms(next_fire) else {
                continue;
            };
            due.push(DueJob {
                prompt: job.prompt.clone(),
                kind: DueKind::Schedule {
                    id: job.id.clone(),
                    fire_ms_hint: next_fire,
                },
            });
            job.last_fired_ms = Some(fired_ms);
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
///
/// A corrupt/out-of-range stored value yields `None`, which callers treat as
/// "never fired" — deliberately NOT a clock read, so `due()` remains a pure
/// function of the `(now, wall)` it was handed.
fn epoch_ms_to_naive(ms: u64) -> Option<chrono::NaiveDateTime> {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
}

/// Inverse of [`epoch_ms_to_naive`]: naive local time back to epoch ms.
///
/// DST semantics, kept deliberately identical to the same-named helper in
/// `zode-tui/src/app.rs` (which converts `fire_ms_hint` for the cross-process
/// fire-dedup store — the two values must agree or dedup breaks):
///
/// - **Fall-back (ambiguous)**: the local time maps to two instants; resolve to
///   the earliest so the job fires once, on the first pass through that hour.
/// - **Spring-forward (nonexistent)**: the local time never happens; return
///   `None` so the caller skips rather than substituting epoch 0. Epoch 0 would
///   read back as a genuine 1970 fire and corrupt every later calculation.
fn naive_to_epoch_ms(naive: chrono::NaiveDateTime) -> Option<u64> {
    let local = chrono::Local.from_local_datetime(&naive);
    local
        .single()
        .or_else(|| local.earliest())
        .map(|dt| dt.timestamp_millis().max(0) as u64)
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
    fn disable_schedule_flips_enabled_and_reports_found() {
        let mut s = Scheduler::default();
        s.set_schedules(vec![ScheduleJob {
            id: "ab12".into(),
            spec: ScheduleSpec::Interval { secs: 60 },
            prompt: "sync".into(),
            enabled: true,
            last_fired_ms: None,
        }]);
        assert!(s.disable_schedule("ab12"), "existing id found");
        assert!(!s.schedules()[0].enabled, "flipped to disabled");
        assert!(!s.disable_schedule("missing"), "unknown id reports false");
    }

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    fn schedule(id: &str, spec: ScheduleSpec) -> ScheduleJob {
        ScheduleJob {
            id: id.into(),
            spec,
            prompt: "standup notes".into(),
            enabled: true,
            last_fired_ms: None,
        }
    }

    /// End-to-end for a `Daily` job: it must actually come out of `due()` once
    /// the wall clock passes its time, report the *trigger point* (not the
    /// observing tick) as `fire_ms_hint`, and never re-fire that trigger.
    #[test]
    fn daily_schedule_becomes_due_through_due() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        s.set_schedules(vec![schedule(
            "ab12",
            ScheduleSpec::Daily { hour: 9, minute: 0 },
        )]);

        // 08:00 registers the baseline; nothing is due yet.
        assert!(s.due(now, dt(2026, 7, 18, 8, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 18, 8, 59)).is_empty());

        // A tick a little AFTER 09:00 fires, and reports 09:00 sharp.
        let fired = s.due(now, dt(2026, 7, 18, 9, 0));
        assert_eq!(fired.len(), 1, "daily job fires once the time passes");
        assert_eq!(fired[0].prompt, "standup notes");
        match &fired[0].kind {
            DueKind::Schedule { id, fire_ms_hint } => {
                assert_eq!(id, "ab12");
                assert_eq!(
                    *fire_ms_hint,
                    dt(2026, 7, 18, 9, 0),
                    "hint is the trigger point, not the observing tick"
                );
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }

        // Same trigger point: no re-fire, however many ticks land in the day.
        assert!(s.due(now, dt(2026, 7, 18, 9, 1)).is_empty());
        assert!(s.due(now, dt(2026, 7, 18, 23, 59)).is_empty());
        // The NEXT day's occurrence is a new trigger point and does fire.
        let next_day = s.due(now, dt(2026, 7, 19, 9, 30));
        assert_eq!(
            next_day.len(),
            1,
            "tomorrow's occurrence is its own trigger"
        );
        match &next_day[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 19, 9, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    /// Same end-to-end for `Interval`, whose next-fire math takes the
    /// `last_fired` path rather than the wall-clock-date path.
    #[test]
    fn interval_schedule_becomes_due_through_due() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        s.set_schedules(vec![schedule(
            "cd34",
            ScheduleSpec::Interval { secs: 3600 },
        )]);

        // Baseline at 08:00; the first fire is one interval later.
        assert!(s.due(now, dt(2026, 7, 18, 8, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 18, 8, 59)).is_empty());

        let fired = s.due(now, dt(2026, 7, 18, 9, 0));
        assert_eq!(fired.len(), 1, "interval job fires one interval in");
        match &fired[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 9, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
        // No re-fire for the same trigger point; the next one is +1h.
        assert!(s.due(now, dt(2026, 7, 18, 9, 30)).is_empty());
        let again = s.due(now, dt(2026, 7, 18, 10, 0));
        assert_eq!(again.len(), 1);
        match &again[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 10, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    /// Downtime must be skipped, not replayed: a job whose stored `last_fired`
    /// is days old anchors on the startup baseline, so exactly one fire happens
    /// at the next real occurrence rather than one per missed day.
    #[test]
    fn missed_triggers_are_skipped_not_replayed() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let stale = naive_to_epoch_ms(dt(2026, 7, 10, 9, 0)).expect("valid local time");
        let mut job = schedule("ef56", ScheduleSpec::Daily { hour: 9, minute: 0 });
        job.last_fired_ms = Some(stale);
        s.set_schedules(vec![job]);

        // Startup at 12:00 on the 18th: eight days of occurrences were missed.
        assert!(
            s.due(now, dt(2026, 7, 18, 12, 0)).is_empty(),
            "no backlog replay on startup"
        );
        // Only the next real occurrence fires, exactly once.
        assert_eq!(s.due(now, dt(2026, 7, 19, 9, 0)).len(), 1);
        assert!(s.due(now, dt(2026, 7, 19, 9, 5)).is_empty());
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
        // Advance the wall clock well past several intervals: an ENABLED job
        // would fire here (see `interval_schedule_becomes_due_through_due`),
        // so this assertion is now about `enabled`, not about `due()` being
        // inert — it used to pass vacuously.
        assert!(s.due(Instant::now(), sample_wall()).is_empty());
        assert!(s
            .due(Instant::now(), sample_wall() + chrono::Duration::hours(5))
            .is_empty());
    }
}
