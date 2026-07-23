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
use std::collections::HashSet;
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
        /// Canonical absolute token. Interval schedules use epoch arithmetic
        /// in the host path, so DST fall-back cannot pause or duplicate phase.
        fire_epoch_ms: u64,
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
        self.schedules = schedules;
    }

    /// Current `/schedule` roster.
    pub fn schedules(&self) -> &[ScheduleJob] {
        &self.schedules
    }

    /// Disable a `/schedule` job by id (sets `enabled = false` in the
    /// in-memory roster). Returns whether a matching job was found. Callers
    /// that need the change to survive a restart use
    /// `disable_schedule_atomic` and refresh this roster — this method has no
    /// I/O and must not be used to overwrite a concurrently edited store.
    pub fn disable_schedule(&mut self, id: &str) -> bool {
        match self.schedules.iter_mut().find(|j| j.id == id) {
            Some(job) => {
                job.enabled = false;
                true
            }
            None => false,
        }
    }

    /// Persistable watchdog bookkeeping for a schedule attempt.
    pub fn record_watchdog_failure(
        &mut self,
        id: &str,
        failures: u32,
        at_ms: u64,
        retry_at_ms: Option<u64>,
    ) -> bool {
        match self.schedules.iter_mut().find(|job| job.id == id) {
            Some(job) => {
                job.watchdog_failures = failures;
                job.watchdog_last_failure_ms = Some(at_ms);
                job.watchdog_retry_at_ms = retry_at_ms;
                job.watchdog_active_since_ms = None;
                true
            }
            None => false,
        }
    }

    /// A successful run or explicit re-enable opens a fresh recovery window.
    pub fn clear_watchdog_failures(&mut self, id: &str) -> bool {
        match self.schedules.iter_mut().find(|job| job.id == id) {
            Some(job) => {
                job.watchdog_failures = 0;
                job.watchdog_last_failure_ms = None;
                job.watchdog_retry_at_ms = None;
                job.watchdog_active_since_ms = None;
                true
            }
            None => false,
        }
    }

    /// Mirror a successful persisted retry claim into the in-memory roster.
    pub fn clear_watchdog_retry_if(&mut self, id: &str, expected_retry_at_ms: u64) -> bool {
        match self.schedules.iter_mut().find(|job| job.id == id) {
            Some(job) if job.watchdog_retry_at_ms == Some(expected_retry_at_ms) => {
                job.watchdog_retry_at_ms = None;
                true
            }
            _ => false,
        }
    }

    pub fn mark_watchdog_active(&mut self, id: &str, active_since_ms: u64) -> bool {
        match self.schedules.iter_mut().find(|job| job.id == id) {
            Some(job) => {
                job.watchdog_retry_at_ms = None;
                job.watchdog_active_since_ms = Some(active_since_ms);
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
    /// Schedule jobs: only `enabled` jobs are considered. Their persisted
    /// `last_fired_ms` is the shared recurrence anchor. Interval candidates are
    /// exact multiples of the interval from that anchor; daily and weekly
    /// candidates use their intrinsic calendar phase. This is intentionally
    /// independent of when a particular process first observes the roster.
    ///
    /// If several occurrences were missed, only the latest due occurrence is
    /// returned. That coalesces backlog without changing phase or replaying one
    /// prompt per missed tick. `last_fired_ms` is stamped to the exact trigger
    /// point (also reported as `fire_ms_hint`), not to the observing `wall`, so
    /// every process presents the same millisecond CAS token for a given slot.
    /// The persistent store supplies a creation/re-enable anchor for all normal
    /// jobs; a legacy in-memory job with no anchor uses a fixed Unix-epoch
    /// local phase rather than a process-local startup time.
    ///
    /// A trigger point with no valid local instant (a DST spring-forward gap)
    /// is skipped. Once the following valid occurrence is due, it becomes the
    /// latest canonical slot.
    ///
    /// `due()` reads no clock of its own: everything is derived from `now`,
    /// `wall`, and stored state.
    pub fn due(&mut self, now: Instant, wall: chrono::NaiveDateTime) -> Vec<DueJob> {
        self.due_with_blocked_loops(now, wall, &HashSet::new())
    }

    /// Variant used by the watchdog-aware host. A blocked loop remains
    /// overdue without advancing `runs` or consuming `max_runs`; once its
    /// active attempt/backoff clears, the next poll fires it exactly once.
    pub fn due_with_blocked_loops(
        &mut self,
        now: Instant,
        wall: chrono::NaiveDateTime,
        blocked_loops: &HashSet<u32>,
    ) -> Vec<DueJob> {
        self.due_with_blocked_jobs(now, wall, blocked_loops, &HashSet::new())
    }

    /// Host-aware variant that leaves both locally occupied loop and schedule
    /// watermarks untouched. This keeps the in-memory schedule phase aligned
    /// with the persisted store while an attempt waits, runs, or backs off.
    pub fn due_with_blocked_jobs(
        &mut self,
        now: Instant,
        wall: chrono::NaiveDateTime,
        blocked_loops: &HashSet<u32>,
        blocked_schedules: &HashSet<String>,
    ) -> Vec<DueJob> {
        self.due_impl(now, wall, None, blocked_loops, blocked_schedules, true)
    }

    /// Host claim mode: loop accounting is committed immediately, while a
    /// persisted schedule is only peeked. Its authoritative watermark moves
    /// in the cross-process store transaction, so a failed I/O, text
    /// collision, or lost claim cannot consume the local occurrence.
    pub fn due_candidates_with_blocked_jobs(
        &mut self,
        now: Instant,
        wall: chrono::NaiveDateTime,
        wall_epoch_ms: u64,
        blocked_loops: &HashSet<u32>,
        blocked_schedules: &HashSet<String>,
    ) -> Vec<DueJob> {
        self.due_impl(
            now,
            wall,
            Some(wall_epoch_ms),
            blocked_loops,
            blocked_schedules,
            false,
        )
    }

    fn due_impl(
        &mut self,
        now: Instant,
        wall: chrono::NaiveDateTime,
        wall_epoch_ms: Option<u64>,
        blocked_loops: &HashSet<u32>,
        blocked_schedules: &HashSet<String>,
        advance_schedule_watermarks: bool,
    ) -> Vec<DueJob> {
        let mut due = Vec::new();

        for job in &mut self.loops {
            if blocked_loops.contains(&job.id) {
                continue;
            }
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
            if !job.enabled || blocked_schedules.contains(&job.id) {
                continue;
            }
            let last_fired = job.last_fired_ms.and_then(epoch_ms_to_naive);
            let anchor = last_fired.unwrap_or_else(unix_epoch_local_phase);
            let Some((next_fire, fired_ms)) = resolve_trigger(
                &job.spec,
                anchor,
                job.last_fired_ms,
                wall,
                wall_epoch_ms,
                naive_to_epoch_ms,
            ) else {
                continue;
            };
            due.push(DueJob {
                prompt: job.prompt.clone(),
                kind: DueKind::Schedule {
                    id: job.id.clone(),
                    fire_ms_hint: next_fire,
                    fire_epoch_ms: fired_ms,
                },
            });
            if advance_schedule_watermarks {
                job.last_fired_ms = Some(fired_ms);
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

/// Resolve the latest canonical trigger point that is due for one schedule.
///
/// `anchor` is normally the persisted `last_fired_ms` converted to local wall
/// time. For an interval it is also the phase origin; for calendar schedules
/// the configured hour/weekday supplies the phase and the anchor prevents an
/// occurrence at or before creation/the previous fire from being returned.
/// Only the latest occurrence at or before `wall` is considered, so downtime
/// coalesces to one token instead of replaying the entire backlog.
///
/// `to_epoch_ms` is injected to keep DST-gap tests independent of the host
/// timezone. A nonexistent latest slot is skipped; on a later call the next
/// valid calendar occurrence naturally becomes the latest candidate.
fn resolve_trigger(
    spec: &jobs::ScheduleSpec,
    anchor: chrono::NaiveDateTime,
    anchor_epoch_ms: Option<u64>,
    wall: chrono::NaiveDateTime,
    wall_epoch_ms: Option<u64>,
    to_epoch_ms: impl Fn(chrono::NaiveDateTime) -> Option<u64>,
) -> Option<(chrono::NaiveDateTime, u64)> {
    if let (jobs::ScheduleSpec::Interval { secs }, Some(anchor_epoch_ms), Some(wall_epoch_ms)) =
        (spec, anchor_epoch_ms, wall_epoch_ms)
    {
        let fired_ms = latest_interval_epoch_slot(anchor_epoch_ms, wall_epoch_ms, *secs)?;
        return epoch_ms_to_naive(fired_ms).map(|candidate| (candidate, fired_ms));
    }
    let candidate = match spec {
        jobs::ScheduleSpec::Interval { secs } => latest_interval_slot(anchor, wall, *secs)?,
        jobs::ScheduleSpec::Daily { hour, minute } => latest_daily_slot(wall, *hour, *minute)?,
        jobs::ScheduleSpec::Weekly {
            weekday,
            hour,
            minute,
        } => latest_weekly_slot(wall, *weekday, *hour, *minute)?,
    };
    if candidate <= anchor {
        return None;
    }
    to_epoch_ms(candidate).map(|ms| (candidate, ms))
}

fn latest_interval_epoch_slot(anchor_ms: u64, wall_ms: u64, secs: u64) -> Option<u64> {
    let period_ms = secs.checked_mul(1_000)?;
    let elapsed_ms = wall_ms.checked_sub(anchor_ms)?;
    if elapsed_ms < period_ms {
        return None;
    }
    anchor_ms.checked_add((elapsed_ms / period_ms).checked_mul(period_ms)?)
}

fn latest_interval_slot(
    anchor: chrono::NaiveDateTime,
    wall: chrono::NaiveDateTime,
    secs: u64,
) -> Option<chrono::NaiveDateTime> {
    let period_ms = secs.checked_mul(1_000)?;
    if period_ms == 0 {
        return None;
    }
    let elapsed_ms = wall.signed_duration_since(anchor).num_milliseconds();
    if elapsed_ms < i64::try_from(period_ms).ok()? {
        return None;
    }
    let slots = u64::try_from(elapsed_ms).ok()? / period_ms;
    let offset_ms = slots.checked_mul(period_ms)?;
    anchor.checked_add_signed(chrono::Duration::milliseconds(
        i64::try_from(offset_ms).ok()?,
    ))
}

fn latest_daily_slot(
    wall: chrono::NaiveDateTime,
    hour: u32,
    minute: u32,
) -> Option<chrono::NaiveDateTime> {
    let today = wall.date().and_hms_opt(hour, minute, 0)?;
    if today <= wall {
        Some(today)
    } else {
        today.checked_sub_signed(chrono::Duration::days(1))
    }
}

fn latest_weekly_slot(
    wall: chrono::NaiveDateTime,
    weekday: chrono::Weekday,
    hour: u32,
    minute: u32,
) -> Option<chrono::NaiveDateTime> {
    use chrono::Datelike;

    for days_ago in 0..7 {
        let date = wall
            .date()
            .checked_sub_signed(chrono::Duration::days(days_ago))?;
        let candidate = date.and_hms_opt(hour, minute, 0)?;
        if date.weekday() == weekday && candidate <= wall {
            return Some(candidate);
        }
    }
    None
}

fn unix_epoch_local_phase() -> chrono::NaiveDateTime {
    chrono::DateTime::UNIX_EPOCH.naive_utc()
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
    fn blocked_loop_does_not_consume_max_runs() {
        let mut scheduler = Scheduler::default();
        let now = Instant::now();
        let id = scheduler.add_loop(7, "check ci".into(), Duration::from_secs(60), Some(2), now);
        scheduler.rewind_loop_for_test(id, Duration::from_secs(61));
        assert_eq!(scheduler.due(now, sample_wall()).len(), 1);
        assert_eq!(scheduler.loops()[0].runs, 1);

        scheduler.rewind_loop_for_test(id, Duration::from_secs(61));
        let blocked = HashSet::from([id]);
        for _ in 0..3 {
            assert!(scheduler
                .due_with_blocked_loops(now, sample_wall(), &blocked)
                .is_empty());
            assert_eq!(scheduler.loops()[0].runs, 1);
        }

        assert_eq!(
            scheduler
                .due_with_blocked_loops(now, sample_wall(), &HashSet::new())
                .len(),
            1
        );
        assert!(scheduler.loops().is_empty());
    }

    #[test]
    fn blocked_schedule_does_not_advance_its_canonical_watermark() {
        let now = Instant::now();
        let anchor = dt(2026, 7, 18, 8, 0);
        let mut scheduler = Scheduler::default();
        scheduler.set_schedules(vec![schedule(
            "blocked-schedule",
            ScheduleSpec::Interval { secs: 3600 },
            anchor,
        )]);
        let blocked = HashSet::from(["blocked-schedule".to_string()]);

        assert!(scheduler
            .due_with_blocked_jobs(now, dt(2026, 7, 18, 10, 30), &HashSet::new(), &blocked,)
            .is_empty());
        assert_eq!(
            scheduler.schedules()[0].last_fired_ms,
            naive_to_epoch_ms(anchor),
            "queued/running work must not consume persisted cadence locally"
        );
        let due = scheduler.due_with_blocked_jobs(
            now,
            dt(2026, 7, 18, 10, 30),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(only_fire_hint(&due), dt(2026, 7, 18, 10, 0));
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
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
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

    fn schedule(id: &str, spec: ScheduleSpec, anchor: chrono::NaiveDateTime) -> ScheduleJob {
        ScheduleJob {
            id: id.into(),
            spec,
            prompt: "standup notes".into(),
            enabled: true,
            last_fired_ms: Some(naive_to_epoch_ms(anchor).expect("valid local anchor")),
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
        }
    }

    fn only_fire_hint(due: &[DueJob]) -> chrono::NaiveDateTime {
        assert_eq!(due.len(), 1, "expected one canonical schedule trigger");
        match &due[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => *fire_ms_hint,
            other => panic!("expected a schedule job, got {other:?}"),
        }
    }

    fn only_schedule_fire_epoch(due: &[DueJob]) -> u64 {
        let mut epochs = due.iter().filter_map(|job| match &job.kind {
            DueKind::Schedule { fire_epoch_ms, .. } => Some(*fire_epoch_ms),
            DueKind::Loop { .. } => None,
        });
        let epoch = epochs
            .next()
            .expect("expected one canonical schedule trigger");
        assert!(
            epochs.next().is_none(),
            "expected only one schedule trigger"
        );
        epoch
    }

    #[test]
    fn host_candidates_peek_schedule_watermark_but_commit_loop_accounting() {
        let now = Instant::now();
        let anchor = dt(2026, 7, 18, 8, 0);
        let anchor_epoch_ms = naive_to_epoch_ms(anchor).expect("valid local anchor");
        let mut scheduler = Scheduler::default();
        scheduler.set_schedules(vec![schedule(
            "candidate-peek",
            ScheduleSpec::Interval { secs: 3600 },
            anchor,
        )]);
        let loop_id = scheduler.add_loop(
            7,
            "check loop".into(),
            Duration::from_secs(60),
            Some(2),
            now,
        );
        scheduler.rewind_loop_for_test(loop_id, Duration::from_secs(61));

        let wall_epoch_ms =
            anchor_epoch_ms + Duration::from_secs(2 * 3600 + 30 * 60).as_millis() as u64;
        let first = scheduler.due_candidates_with_blocked_jobs(
            now,
            dt(2026, 7, 18, 10, 30),
            wall_epoch_ms,
            &HashSet::new(),
            &HashSet::new(),
        );
        let expected_epoch_ms = anchor_epoch_ms + Duration::from_secs(2 * 3600).as_millis() as u64;
        assert_eq!(only_schedule_fire_epoch(&first), expected_epoch_ms);
        assert_eq!(scheduler.loops()[0].runs, 1, "loop accounting is committed");
        assert_eq!(
            scheduler.schedules()[0].last_fired_ms,
            Some(anchor_epoch_ms),
            "the host must leave schedule advancement to the store claim"
        );

        let repeated = scheduler.due_candidates_with_blocked_jobs(
            now,
            dt(2026, 7, 18, 10, 30),
            wall_epoch_ms,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            only_schedule_fire_epoch(&repeated),
            expected_epoch_ms,
            "an unclaimed candidate remains available with the same CAS token"
        );
        assert!(
            repeated
                .iter()
                .all(|job| !matches!(&job.kind, DueKind::Loop { .. })),
            "the loop was committed on the first candidate poll"
        );
        assert_eq!(
            scheduler.schedules()[0].last_fired_ms,
            Some(anchor_epoch_ms)
        );
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
            dt(2026, 7, 18, 8, 0),
        )]);

        // The persisted creation anchor is 08:00; nothing is due yet.
        assert!(s.due(now, dt(2026, 7, 18, 8, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 18, 8, 59)).is_empty());

        // A tick a little AFTER 09:00 fires, and reports 09:00 sharp.
        let fired = s.due(now, dt(2026, 7, 18, 9, 0));
        assert_eq!(fired.len(), 1, "daily job fires once the time passes");
        assert_eq!(fired[0].prompt, "standup notes");
        match &fired[0].kind {
            DueKind::Schedule {
                id, fire_ms_hint, ..
            } => {
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
            dt(2026, 7, 18, 8, 0),
        )]);

        // Creation anchor at 08:00; the first fire is one interval later.
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

    #[test]
    fn host_interval_candidates_keep_epoch_cadence_when_naive_wall_moves_back() {
        let now = Instant::now();
        let anchor_epoch_ms = 2_000_000_000_000;
        let mut job = schedule(
            "dst-fallback",
            ScheduleSpec::Interval { secs: 3600 },
            dt(2026, 10, 31, 0, 0),
        );
        job.last_fired_ms = Some(anchor_epoch_ms);
        let mut scheduler = Scheduler::default();
        scheduler.set_schedules(vec![job]);

        let before_fallback = scheduler.due_candidates_with_blocked_jobs(
            now,
            dt(2026, 11, 1, 1, 55),
            anchor_epoch_ms + Duration::from_secs(55 * 60).as_millis() as u64,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(
            before_fallback.is_empty(),
            "less than one epoch hour elapsed"
        );

        let after_fallback = scheduler.due_candidates_with_blocked_jobs(
            now,
            dt(2026, 11, 1, 1, 5),
            anchor_epoch_ms + Duration::from_secs(65 * 60).as_millis() as u64,
            &HashSet::new(),
            &HashSet::new(),
        );
        let first_epoch_ms = only_schedule_fire_epoch(&after_fallback);
        assert_eq!(
            first_epoch_ms,
            anchor_epoch_ms + Duration::from_secs(3600).as_millis() as u64,
            "absolute elapsed time fires even though local wall time moved backward"
        );

        let mut claimed = scheduler.schedules()[0].clone();
        claimed.last_fired_ms = Some(first_epoch_ms);
        scheduler.set_schedules(vec![claimed]);
        let next = scheduler.due_candidates_with_blocked_jobs(
            now,
            dt(2026, 11, 1, 1, 10),
            anchor_epoch_ms + Duration::from_secs(125 * 60).as_millis() as u64,
            &HashSet::new(),
            &HashSet::new(),
        );
        let second_epoch_ms = only_schedule_fire_epoch(&next);
        assert_eq!(second_epoch_ms - first_epoch_ms, 3_600_000);
        assert_eq!(
            scheduler.schedules()[0].last_fired_ms,
            Some(first_epoch_ms),
            "candidate polling still leaves the claimed store watermark untouched"
        );
    }

    #[test]
    fn independent_schedulers_share_interval_phase_and_exact_token() {
        let now = Instant::now();
        let anchor = dt(2026, 7, 18, 8, 0) + chrono::Duration::milliseconds(347);
        let job = schedule(
            "phase-interval",
            ScheduleSpec::Interval { secs: 3600 },
            anchor,
        );
        let mut process_a = Scheduler::default();
        let mut process_b = Scheduler::default();
        process_a.set_schedules(vec![job.clone()]);
        process_b.set_schedules(vec![job]);

        // These are the first observations in each process and happen at
        // different points inside the same overdue interval slot.
        let a = process_a.due(now, dt(2026, 7, 18, 9, 10));
        let b = process_b.due(now, dt(2026, 7, 18, 9, 55));
        let expected = dt(2026, 7, 18, 9, 0) + chrono::Duration::milliseconds(347);
        assert_eq!(only_fire_hint(&a), expected);
        assert_eq!(only_fire_hint(&b), expected);
        assert_eq!(
            process_a.schedules()[0].last_fired_ms,
            process_b.schedules()[0].last_fired_ms,
            "both processes present the identical millisecond CAS token"
        );
        assert_eq!(
            process_a.schedules()[0].last_fired_ms,
            naive_to_epoch_ms(expected)
        );
    }

    #[test]
    fn independent_schedulers_coalesce_interval_backlog_without_phase_drift() {
        let now = Instant::now();
        let job = schedule(
            "phase-backlog",
            ScheduleSpec::Interval { secs: 3600 },
            dt(2026, 7, 18, 8, 0),
        );
        let mut process_a = Scheduler::default();
        let mut process_b = Scheduler::default();
        process_a.set_schedules(vec![job.clone()]);
        process_b.set_schedules(vec![job]);

        // Four occurrences are overdue. Both observers coalesce directly to
        // 12:00 even though their ticks happen at different wall times.
        let a = process_a.due(now, dt(2026, 7, 18, 12, 5));
        let b = process_b.due(now, dt(2026, 7, 18, 12, 59));
        assert_eq!(only_fire_hint(&a), dt(2026, 7, 18, 12, 0));
        assert_eq!(only_fire_hint(&b), dt(2026, 7, 18, 12, 0));
    }

    #[test]
    fn independent_schedulers_share_daily_and_weekly_calendar_slots() {
        let now = Instant::now();

        let daily = schedule(
            "phase-daily",
            ScheduleSpec::Daily { hour: 9, minute: 0 },
            dt(2026, 7, 17, 9, 0),
        );
        let mut daily_a = Scheduler::default();
        let mut daily_b = Scheduler::default();
        daily_a.set_schedules(vec![daily.clone()]);
        daily_b.set_schedules(vec![daily]);
        assert!(daily_a.due(now, dt(2026, 7, 18, 8, 0)).is_empty());
        let daily_b_due = daily_b.due(now, dt(2026, 7, 18, 9, 15));
        let daily_a_due = daily_a.due(now, dt(2026, 7, 18, 9, 45));
        assert_eq!(only_fire_hint(&daily_a_due), dt(2026, 7, 18, 9, 0));
        assert_eq!(only_fire_hint(&daily_b_due), dt(2026, 7, 18, 9, 0));

        let weekly = schedule(
            "phase-weekly",
            ScheduleSpec::Weekly {
                weekday: chrono::Weekday::Mon,
                hour: 9,
                minute: 0,
            },
            dt(2026, 7, 13, 9, 0),
        );
        let mut weekly_a = Scheduler::default();
        let mut weekly_b = Scheduler::default();
        weekly_a.set_schedules(vec![weekly.clone()]);
        weekly_b.set_schedules(vec![weekly]);
        assert!(weekly_a.due(now, dt(2026, 7, 19, 12, 0)).is_empty());
        let weekly_b_due = weekly_b.due(now, dt(2026, 7, 20, 9, 15));
        let weekly_a_due = weekly_a.due(now, dt(2026, 7, 20, 9, 45));
        assert_eq!(only_fire_hint(&weekly_a_due), dt(2026, 7, 20, 9, 0));
        assert_eq!(only_fire_hint(&weekly_b_due), dt(2026, 7, 20, 9, 0));
    }

    /// Downtime is coalesced, not replayed: a job whose stored `last_fired` is
    /// days old emits only the latest canonical slot, never one prompt per
    /// missed day.
    #[test]
    fn missed_triggers_coalesce_to_the_latest_slot() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let stale = naive_to_epoch_ms(dt(2026, 7, 10, 9, 0)).expect("valid local time");
        let job = schedule(
            "ef56",
            ScheduleSpec::Daily { hour: 9, minute: 0 },
            epoch_ms_to_naive(stale).expect("valid local anchor"),
        );
        s.set_schedules(vec![job]);

        // At 12:00 on the 18th, eight daily occurrences were missed. Only the
        // latest (today at 09:00) is emitted.
        let coalesced = s.due(now, dt(2026, 7, 18, 12, 0));
        assert_eq!(coalesced.len(), 1, "backlog coalesces to one trigger");
        match &coalesced[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 9, 0))
            }
            other => panic!("expected a schedule job, got {other:?}"),
        }
        assert!(s.due(now, dt(2026, 7, 18, 12, 1)).is_empty());
        // Tomorrow's occurrence remains a distinct slot.
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
            dt(2026, 7, 18, 10, 0),
        )]);

        // Persisted creation anchor on Saturday; nothing until Monday 09:00.
        assert!(s.due(now, dt(2026, 7, 18, 10, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 19, 23, 0)).is_empty());
        assert!(s.due(now, dt(2026, 7, 20, 8, 59)).is_empty());

        let fired = s.due(now, dt(2026, 7, 20, 9, 0));
        assert_eq!(fired.len(), 1, "weekly job fires on its weekday");
        match &fired[0].kind {
            DueKind::Schedule {
                id, fire_ms_hint, ..
            } => {
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

    /// Re-enabling persists a fresh shared anchor, so occurrences accrued while
    /// the job was disabled are not replayed.
    #[test]
    fn re_enabling_a_schedule_re_anchors_instead_of_replaying() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        let job = schedule(
            "ij90",
            ScheduleSpec::Interval { secs: 7200 },
            dt(2026, 7, 18, 9, 0),
        );
        s.set_schedules(vec![job.clone()]);

        // 09:00 creation anchor, disabled at 09:05 before it ever fires.
        assert!(s.due(now, dt(2026, 7, 18, 9, 0)).is_empty());
        assert!(s.disable_schedule("ij90"));
        // Disabled jobs remain inert.
        assert!(s.due(now, dt(2026, 7, 18, 9, 5)).is_empty());
        // Nothing accrues over the next eight hours either.
        assert!(s.due(now, dt(2026, 7, 18, 13, 0)).is_empty());

        // Mirror `/schedule enable`, which atomically stores its wall time as
        // the new shared cadence anchor before refreshing the scheduler.
        let mut enabled = s.schedules()[0].clone();
        enabled.enabled = true;
        enabled.last_fired_ms =
            Some(naive_to_epoch_ms(dt(2026, 7, 18, 17, 0)).expect("valid re-enable anchor"));
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

    /// A re-enable anchor is persisted even inside the old interval, so every
    /// process starts the same fresh cadence instead of preserving a private
    /// in-memory phase.
    #[test]
    fn brief_disable_re_enable_starts_a_shared_fresh_cadence() {
        let mut s = Scheduler::default();
        let now = Instant::now();
        s.set_schedules(vec![schedule(
            "kl12",
            ScheduleSpec::Interval { secs: 7200 },
            dt(2026, 7, 18, 7, 0),
        )]);

        assert!(s.due(now, dt(2026, 7, 18, 7, 0)).is_empty()); // baseline
        assert_eq!(s.due(now, dt(2026, 7, 18, 9, 0)).len(), 1); // fires at 09:00

        assert!(s.disable_schedule("kl12"));
        assert!(s.due(now, dt(2026, 7, 18, 9, 5)).is_empty());
        let mut enabled = s.schedules()[0].clone();
        enabled.enabled = true;
        enabled.last_fired_ms =
            Some(naive_to_epoch_ms(dt(2026, 7, 18, 9, 10)).expect("valid re-enable anchor"));
        s.set_schedules(vec![enabled]);
        assert!(s.due(now, dt(2026, 7, 18, 9, 10)).is_empty());

        // The persisted 09:10 re-enable anchor intentionally moves the next
        // interval to 11:10 in every process.
        assert!(s.due(now, dt(2026, 7, 18, 10, 59)).is_empty());
        assert!(s.due(now, dt(2026, 7, 18, 11, 0)).is_empty());
        let fired = s.due(now, dt(2026, 7, 18, 11, 10));
        assert_eq!(fired.len(), 1);
        match &fired[0].kind {
            DueKind::Schedule { fire_ms_hint, .. } => {
                assert_eq!(*fire_ms_hint, dt(2026, 7, 18, 11, 10))
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
            resolve_trigger(&spec, anchor, None, dt(2026, 3, 8, 10, 0), None, to_ms,),
            None,
            "the gap occurrence is skipped, not fired"
        );
        // ...and crucially, once tomorrow arrives it DOES fire, rather than the
        // pre-fix behavior of recomputing the same nonexistent 03-08 02:30
        // forever and wedging the job for the life of the process.
        assert_eq!(
            resolve_trigger(&spec, anchor, None, dt(2026, 3, 9, 3, 0), None, to_ms,),
            Some((dt(2026, 3, 9, 2, 30), 1)),
            "the following occurrence fires"
        );
    }

    /// An invalid local trigger token must be skipped rather than fabricated.
    #[test]
    fn resolve_trigger_skips_an_unresolvable_local_slot() {
        let spec = ScheduleSpec::Interval { secs: 30 };
        let anchor = dt(2026, 3, 8, 0, 0);
        assert_eq!(
            resolve_trigger(&spec, anchor, None, dt(2030, 1, 1, 0, 0), None, |_| None,),
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
            watchdog_failures: 0,
            watchdog_last_failure_ms: None,
            watchdog_retry_at_ms: None,
            watchdog_active_since_ms: None,
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
