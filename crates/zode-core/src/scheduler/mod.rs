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

    /// Stop every loop job owned by `owner`, returning the ids that were
    /// removed.
    ///
    /// Called when a tab closes. Without it the jobs stay in the roster with
    /// nowhere to run: `due()` keeps returning them (incrementing `runs`, so a
    /// `--max N` budget is silently burned with zero executions) while the
    /// caller drops each one for want of an owning tab, and they linger in
    /// `/loop list` forever.
    pub fn stop_loops_for_owner(&mut self, owner: u64) -> Vec<u32> {
        let stopped: Vec<u32> = self
            .loops
            .iter()
            .filter(|j| j.owner == owner)
            .map(|j| j.id)
            .collect();
        self.loops.retain(|j| j.owner != owner);
        stopped
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
    /// A disabled job drops its baseline, so `/schedule enable` re-anchors at
    /// the wall clock of the first tick that observes it enabled again rather
    /// than replaying every occurrence accrued while it was off.
    ///
    /// A trigger point with no valid local instant (DST spring-forward gap) is
    /// skipped *forward* to the following occurrence — see [`resolve_trigger`].
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
                // Drop the baseline while a job is disabled. `set_schedules`
                // deliberately keeps baselines for ids that survive a roster
                // swap, and a disabled job never leaves the roster — so without
                // this, `/schedule disable` at 09:05 followed by
                // `/schedule enable` at 17:00 would still be anchored at 09:00
                // and replay every occurrence accrued in between. Re-enabling
                // must re-anchor at the current wall clock, per the documented
                // "missed triggers are skipped, never replayed".
                baselines.remove(&job.id);
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
            // Resolves the trigger point, skipping *past* any occurrence that
            // has no valid local instant (spring-forward gap) rather than
            // retrying the same nonexistent one every tick.
            let Some((next_fire, fired_ms)) =
                resolve_trigger(&job.spec, anchor, last_fired, wall, naive_to_epoch_ms)
            else {
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

/// Runaway guard for [`resolve_trigger`]'s gap-skipping walk. A DST
/// spring-forward gap is at most a few hours, so with the 30s minimum
/// interval the real worst case is a few hundred steps.
const MAX_GAP_SKIPS: usize = 1024;

/// Resolve the trigger point a schedule job should fire at, given its anchor.
///
/// Returns `None` when the job is simply not due yet (`next_fire > wall`),
/// or when gap-skipping exhausts `MAX_GAP_SKIPS` (runaway guard against
/// pathological DST/spec combinations). Returns `Some((trigger_point, epoch_ms))`
/// when a valid fire point is found.
///
/// The loop exists for DST spring-forward: `to_epoch_ms` returns `None` for a
/// local time that never happens, and the fix for that must ADVANCE to the
/// following occurrence. Simply skipping the fire would recompute the identical
/// nonexistent trigger point on every subsequent tick — a `Daily`/`Weekly` job
/// scheduled inside the gap would be wedged for the life of the process, since
/// nothing ever stamps `last_fired_ms` to move the anchor forward. Advancing
/// matches the documented behavior ("fires on the following occurrence instead
/// of this one").
///
/// `to_epoch_ms` is a parameter rather than a direct call to
/// [`naive_to_epoch_ms`] so the gap-skipping walk is testable without depending
/// on the host machine's timezone (`chrono::Local`): a test can inject a
/// synthetic gap.
fn resolve_trigger(
    spec: &jobs::ScheduleSpec,
    anchor: chrono::NaiveDateTime,
    last_fired: Option<chrono::NaiveDateTime>,
    wall: chrono::NaiveDateTime,
    to_epoch_ms: impl Fn(chrono::NaiveDateTime) -> Option<u64>,
) -> Option<(chrono::NaiveDateTime, u64)> {
    let mut next_fire = spec.next_after(anchor, last_fired);
    for _ in 0..MAX_GAP_SKIPS {
        if next_fire > wall {
            return None; // not due yet
        }
        if let Some(ms) = to_epoch_ms(next_fire) {
            return Some((next_fire, ms));
        }
        // Nonexistent local time: step to the occurrence after it. `next_after`
        // is strictly-after its `now` argument, so this always makes progress.
        next_fire = spec.next_after(next_fire, last_fired);
    }
    None
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
///   `None` rather than substituting epoch 0, and let the caller advance to the
///   following occurrence (see [`resolve_trigger`]). Epoch 0 would
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

    /// Same end-to-end for `Weekly`, whose anchoring was previously covered
    /// only by the pure `next_after` unit test.
    #[test]
    fn weekly_schedule_becomes_due_through_due() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        // 2026-07-18 is a Saturday; 2026-07-20 is the following Monday.
        s.set_schedules(vec![schedule(
            "gh78",
            ScheduleSpec::Weekly {
                weekday: chrono::Weekday::Mon,
                hour: 9,
                minute: 0,
            },
        )]);

        // Baseline on Saturday; nothing until Monday 09:00.
        assert!(s.due(now, dt(2026, 7, 18, 10, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 19, 23, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 20, 8, 59)).is_empty());

        let fired = s.due(now, dt(2026, 7, 20, 9, 0));
        assert_eq!(fired.len(), 1, "weekly job fires on its weekday");
        match &fired[0].kind {
            DueKind::Schedule { id, fire_ms_hint } => {
                assert_eq!(id, "gh78");
                assert_eq!(
                    *fire_ms_hint,
                    dt(2026, 7, 20, 9, 0),
                    "hint is the trigger point, not the observing tick"
                );
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }

        // Same trigger point: no re-fire for the rest of the week.
        assert!(s.due(now, dt(2026, 7, 20, 9, 30)).is_empty());
        assert!(s.due(now, dt(2026, 7, 24, 12, 0)).is_empty());
        // The following Monday is a distinct trigger and does fire.
        let next_week = s.due(now, dt(2026, 7, 27, 9, 0));
        assert_eq!(
            next_week.len(),
            1,
            "next week's occurrence is its own trigger"
        );
        match &next_week[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 27, 9, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    /// Regression: re-enabling a job that was disabled for hours must NOT
    /// replay the occurrences accrued while it was off. The baseline is dropped
    /// on the disabled tick, so `enable` re-anchors at the current wall clock.
    #[test]
    fn re_enabling_a_schedule_re_anchors_instead_of_replaying() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let job = schedule("ij90", ScheduleSpec::Interval { secs: 7200 });
        s.set_schedules(vec![job.clone()]);

        // 09:00 baseline, disabled at 09:05 before it ever fires.
        assert!(s.due(now, dt(2026, 7, 18, 9, 0)).is_empty());
        assert!(s.disable_schedule("ij90"));
        // A tick while disabled drops the stale baseline.
        assert!(s.due(now, dt(2026, 7, 18, 9, 5)).is_empty());
        // Nothing accrues over the next eight hours either.
        assert!(s.due(now, dt(2026, 7, 18, 13, 0)).is_empty());

        // Re-enable at 17:00. `/schedule enable` round-trips the whole roster
        // through `set_schedules`, which preserves surviving ids' baselines —
        // the point of the fix is that there is no longer one to preserve.
        let mut enabled = s.schedules()[0].clone();
        enabled.enabled = true;
        s.set_schedules(vec![enabled]);

        // The re-anchoring tick fires nothing: no 11:00/13:00/15:00 backlog.
        assert!(
            s.due(now, dt(2026, 7, 18, 17, 0)).is_empty(),
            "re-enable must not replay occurrences accrued while disabled"
        );
        assert!(s.due(now, dt(2026, 7, 18, 18, 0)).is_empty());
        // One interval past the re-enable point, it resumes normally, once.
        let fired = s.due(now, dt(2026, 7, 18, 19, 0));
        assert_eq!(fired.len(), 1, "resumes one interval after re-enable");
        match &fired[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 19, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    /// Disabling and re-enabling inside a single interval must still behave
    /// sanely: the job re-anchors at the re-enable point but its own
    /// `last_fired_ms` still governs cadence when it is the later anchor.
    #[test]
    fn brief_disable_re_enable_keeps_the_original_cadence() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        s.set_schedules(vec![schedule(
            "kl12",
            ScheduleSpec::Interval { secs: 7200 },
        )]);

        assert!(s.due(now, dt(2026, 7, 18, 7, 0)).is_empty()); // baseline
        assert_eq!(s.due(now, dt(2026, 7, 18, 9, 0)).len(), 1); // fires at 09:00

        assert!(s.disable_schedule("kl12"));
        assert!(s.due(now, dt(2026, 7, 18, 9, 5)).is_empty());
        let mut enabled = s.schedules()[0].clone();
        enabled.enabled = true;
        s.set_schedules(vec![enabled]);
        assert!(s.due(now, dt(2026, 7, 18, 9, 10)).is_empty());

        // `last_fired` (09:00) + 2h == 11:00 is still later than the 09:10
        // re-anchor, so the original cadence is preserved rather than pushed
        // out to 11:10.
        assert!(s.due(now, dt(2026, 7, 18, 10, 59)).is_empty());
        let fired = s.due(now, dt(2026, 7, 18, 11, 0));
        assert_eq!(fired.len(), 1);
        match &fired[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 11, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    /// Regression for the DST spring-forward wedge. Driven through
    /// `resolve_trigger` with an injected gap rather than through `due()`,
    /// because `due()` converts via `chrono::Local` and no fixed local time is
    /// a gap on every machine's timezone — this keeps the test deterministic.
    #[test]
    fn spring_forward_gap_advances_to_the_following_occurrence() {
        let spec = ScheduleSpec::Daily {
            hour: 2,
            minute: 30,
        };
        // Synthetic gap: 2026-03-08 02:30 does not exist.
        let gap = dt(2026, 3, 8, 2, 30);
        let to_ms = |n: chrono::NaiveDateTime| if n == gap { None } else { Some(1) };

        // Anchored on yesterday's fire, observed at 10:00 on the gap day. The
        // trigger point lands in the gap, so it must advance to TOMORROW's
        // occurrence — which is not due yet, so nothing fires...
        let anchor = dt(2026, 3, 7, 2, 30);
        assert_eq!(
            resolve_trigger(&spec, anchor, Some(anchor), dt(2026, 3, 8, 10, 0), to_ms),
            None,
            "the gap occurrence is skipped, not fired"
        );
        // ...and crucially, once tomorrow arrives it DOES fire, rather than the
        // pre-fix behavior of recomputing the same nonexistent 03-08 02:30
        // forever and wedging the job for the life of the process.
        assert_eq!(
            resolve_trigger(&spec, anchor, Some(anchor), dt(2026, 3, 9, 3, 0), to_ms),
            Some((dt(2026, 3, 9, 2, 30), 1)),
            "the following occurrence fires"
        );
    }

    /// The gap walk must terminate even if every candidate were nonexistent,
    /// and must not fire anything in that case.
    #[test]
    fn resolve_trigger_gives_up_rather_than_looping_forever() {
        let spec = ScheduleSpec::Interval { secs: 30 };
        let anchor = dt(2026, 3, 8, 0, 0);
        assert_eq!(
            resolve_trigger(&spec, anchor, None, dt(2030, 1, 1, 0, 0), |_| None),
            None
        );
    }

    /// Closing a tab must take its `/loop` jobs with it — otherwise `due()`
    /// keeps burning a `--max N` budget for a job that can never run.
    #[test]
    fn stop_loops_for_owner_removes_only_that_owners_jobs() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let a = s.add_loop(1, "a".into(), Duration::from_secs(60), None, now);
        let b = s.add_loop(1, "b".into(), Duration::from_secs(60), None, now);
        let keep = s.add_loop(2, "c".into(), Duration::from_secs(60), None, now);

        let mut stopped = s.stop_loops_for_owner(1);
        stopped.sort_unstable();
        assert_eq!(stopped, vec![a, b], "both of tab 1's loops reported");
        assert_eq!(s.loops().len(), 1, "tab 2's loop survives");
        assert_eq!(s.loops()[0].id, keep);
        // Idempotent: a second call finds nothing.
        assert!(s.stop_loops_for_owner(1).is_empty());
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
