//! Pure liveness state machine for unattended `/loop` and `/schedule` turns.
//! `TuiApp` owns abort handles and queues; this module decides when to abort,
//! hard-stop, and retry using caller-supplied `Instant`s.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent::abort::TurnActivity;
use zode_core::config::BackgroundWatchdogConfig;

use super::SchedJobRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimeoutKind {
    Inactive,
    Runtime,
}

impl TimeoutKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Inactive => "no activity",
            Self::Runtime => "maximum runtime",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FailureCause {
    Timeout(TimeoutKind),
    AbortGraceExpired(TimeoutKind),
    ManualCancellationUnknown,
    UnresolvedExternalWork,
    QueueStartTimeout,
    TurnFailed(String),
}

impl FailureCause {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Timeout(kind) => format!("{} timeout", kind.label()),
            Self::AbortGraceExpired(kind) => {
                format!("{} timeout; cancellation did not drain", kind.label())
            }
            Self::ManualCancellationUnknown => {
                "manual cancellation left possible side effects".to_string()
            }
            Self::UnresolvedExternalWork => {
                "turn left detached or externally unresolved work".to_string()
            }
            Self::QueueStartTimeout => {
                "queued attempt did not start before its deadline".to_string()
            }
            Self::TurnFailed(message) => {
                let message = message.trim();
                if message.is_empty() {
                    "turn failed".to_string()
                } else {
                    format!("turn failed: {message}")
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Recovery {
    RetryScheduled { attempt: u32, delay: Duration },
    Exhausted { failures: u32 },
    ManualReview { failures: u32 },
    Cancelled,
}

#[derive(Debug, Clone)]
pub(super) struct Failure {
    pub(super) job: SchedJobRef,
    pub(super) tab_id: usize,
    pub(super) cause: FailureCause,
    pub(super) recovery: Recovery,
}

#[derive(Debug, Clone)]
pub(super) enum WatchdogAction {
    Abort {
        tab_id: usize,
        turn_id: u64,
        kind: TimeoutKind,
    },
    HardStop {
        tab_id: usize,
        turn_id: u64,
        failure: Failure,
    },
    ForceCancel {
        tab_id: usize,
        turn_id: u64,
        job: SchedJobRef,
        failure: Option<Failure>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct DueRetry {
    pub(super) job: SchedJobRef,
    pub(super) tab_id: usize,
    pub(super) prompt: String,
    pub(super) attempt: u32,
}

#[derive(Debug, Clone, Copy)]
enum RunPhase {
    Active,
    Aborting {
        kind: TimeoutKind,
        deadline: Instant,
    },
    Cancelling {
        deadline: Instant,
    },
    /// The cancellation grace expired and the UI is joining the worker tree.
    /// Keep the immutable run context until that join finishes so a raced
    /// canonical terminal can still be classified exactly once.
    ForceStopping,
}

#[derive(Debug, Clone)]
struct WatchedTurn {
    job: SchedJobRef,
    tab_id: usize,
    turn_id: u64,
    prompt: String,
    started_at: Instant,
    pulse: WatchdogPulse,
    phase: RunPhase,
}

#[derive(Debug, Clone, Copy)]
struct PulseState {
    last_activity_at: Instant,
    terminal: bool,
    timeout: Option<TimeoutKind>,
}

/// Source-side heartbeat shared with the provider stream worker. Updating it
/// before an event enters the UI channel prevents a busy renderer or a queued
/// `TurnDone` from being mistaken for a dead turn.
#[derive(Debug, Clone)]
pub(super) struct WatchdogPulse {
    state: Arc<Mutex<PulseState>>,
    source: TurnActivity,
}

impl WatchdogPulse {
    fn new(now: Instant, source: TurnActivity) -> Self {
        source.pulse();
        Self {
            state: Arc::new(Mutex::new(PulseState {
                last_activity_at: now,
                terminal: false,
                timeout: None,
            })),
            source,
        }
    }

    pub(super) fn activity(&self, now: Instant) {
        self.source.pulse();
        if let Ok(mut state) = self.state.lock() {
            if !state.terminal {
                state.last_activity_at = now;
            }
        }
    }

    pub(super) fn terminal(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.terminal = true;
        }
    }

    pub(super) fn timeout_kind(&self) -> Option<TimeoutKind> {
        self.state.lock().ok().and_then(|state| state.timeout)
    }

    fn side_effect_risk(&self) -> bool {
        self.source.side_effect_risk()
    }

    fn unresolved_external_work(&self) -> bool {
        self.source.unresolved_external_work()
    }

    fn try_timeout(
        &self,
        now: Instant,
        started_at: Instant,
        inactivity: Duration,
        runtime: Duration,
    ) -> Option<TimeoutKind> {
        let source_activity = self.source.last_activity_at();
        // Recover a poisoned lock rather than returning None: the critical
        // sections are trivial field writes, so the state is still consistent,
        // and bailing out would make a panicked turn permanently immune to the
        // watchdog (fail-open) — it would hold its job and lease forever.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.last_activity_at = state.last_activity_at.max(source_activity);
        if state.terminal || state.timeout.is_some() {
            return None;
        }
        let kind = if now.saturating_duration_since(started_at) >= runtime {
            Some(TimeoutKind::Runtime)
        } else if now.saturating_duration_since(state.last_activity_at) >= inactivity {
            Some(TimeoutKind::Inactive)
        } else {
            None
        };
        if let Some(kind) = kind {
            state.timeout = Some(kind);
        }
        kind
    }

    fn snapshot(&self) -> PulseState {
        let source_activity = self.source.last_activity_at();
        // Recover a poisoned lock instead of fabricating `terminal: true`:
        // faking a completed turn would make the Aborting/Cancelling force
        // paths in `poll` skip a run that is actually stuck (fail-open).
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        PulseState {
            last_activity_at: state.last_activity_at.max(source_activity),
            ..*state
        }
    }
}

#[derive(Debug, Clone)]
struct PendingRetry {
    job: SchedJobRef,
    tab_id: usize,
    prompt: String,
    attempt: u32,
    due_at: Instant,
    retry_token_ms: Option<u64>,
}

/// Runtime watchdog state. Retry counters are consecutive failures and reset
/// after the first successful run. In-memory loops keep recovery process-local;
/// persisted schedules mirror the count and exact retry token through
/// `TuiApp`, allowing another process or a restart to resume safely.
#[derive(Debug, Clone)]
pub(super) struct BackgroundWatchdog {
    config: BackgroundWatchdogConfig,
    running: HashMap<(usize, u64), WatchedTurn>,
    failures: HashMap<SchedJobRef, u32>,
    pending_retries: HashMap<SchedJobRef, PendingRetry>,
    suppressed_recovery: HashSet<SchedJobRef>,
}

impl BackgroundWatchdog {
    pub(super) fn new(config: BackgroundWatchdogConfig) -> Self {
        Self {
            config,
            running: HashMap::new(),
            failures: HashMap::new(),
            pending_retries: HashMap::new(),
            suppressed_recovery: HashSet::new(),
        }
    }

    pub(super) fn enabled(&self) -> bool {
        self.config.enabled()
    }

    pub(super) fn queue_start_timeout(&self) -> Duration {
        self.config.inactivity_timeout()
    }

    pub(super) fn start(
        &mut self,
        job: SchedJobRef,
        tab_id: usize,
        turn_id: u64,
        prompt: String,
        now: Instant,
        activity: TurnActivity,
    ) -> Option<WatchdogPulse> {
        if !self.enabled() {
            return None;
        }
        // Dispatching a retry consumes its pending timer. A duplicate run for
        // the same job is never expected, but replacing it is safer than
        // allowing two watchdog records to race one recovery counter.
        self.pending_retries.remove(&job);
        self.suppressed_recovery.remove(&job);
        self.running.retain(|_, run| run.job != job);
        let pulse = WatchdogPulse::new(now, activity);
        self.running.insert(
            (tab_id, turn_id),
            WatchedTurn {
                job,
                tab_id,
                turn_id,
                prompt,
                started_at: now,
                pulse: pulse.clone(),
                phase: RunPhase::Active,
            },
        );
        Some(pulse)
    }

    /// Advance timeout and abort-grace state. Timeout actions are emitted once;
    /// after the grace deadline the run is removed and recovery is scheduled.
    pub(super) fn poll(&mut self, now: Instant) -> Vec<WatchdogAction> {
        if !self.enabled() {
            return Vec::new();
        }
        let inactivity = self.config.inactivity_timeout();
        let runtime = self.config.max_runtime();
        let grace = self.config.abort_grace();
        let mut actions = Vec::new();
        let mut force = Vec::new();
        let mut force_cancel = Vec::new();

        for (key, run) in &mut self.running {
            match run.phase {
                RunPhase::Active => {
                    let kind = run
                        .pulse
                        .try_timeout(now, run.started_at, inactivity, runtime);
                    if let Some(kind) = kind {
                        run.phase = RunPhase::Aborting {
                            kind,
                            deadline: now + grace,
                        };
                        actions.push(WatchdogAction::Abort {
                            tab_id: run.tab_id,
                            turn_id: run.turn_id,
                            kind,
                        });
                    }
                }
                RunPhase::Aborting { kind, deadline } => {
                    // The worker marks terminal after recorder/quiescence work
                    // and immediately before enqueueing TurnDone. Once marked,
                    // the UI must consume that canonical outcome rather than
                    // hard-aborting already-finished provider work.
                    if !run.pulse.snapshot().terminal && now >= deadline {
                        force.push((*key, kind));
                    }
                }
                RunPhase::Cancelling { deadline } => {
                    if !run.pulse.snapshot().terminal && now >= deadline {
                        force_cancel.push(*key);
                    }
                }
                RunPhase::ForceStopping => {}
            }
        }

        for (key, kind) in force {
            if let Some(run) = self.running.remove(&key) {
                let tab_id = run.tab_id;
                let turn_id = run.turn_id;
                let failure = self.fail_run(run, FailureCause::AbortGraceExpired(kind), now);
                actions.push(WatchdogAction::HardStop {
                    tab_id,
                    turn_id,
                    failure,
                });
            }
        }
        for key in force_cancel {
            let unsafe_to_release = self.running.get(&key).is_some_and(|run| {
                run.pulse.side_effect_risk() || run.pulse.unresolved_external_work()
            });
            if unsafe_to_release {
                let Some(run) = self.running.remove(&key) else {
                    continue;
                };
                let job = run.job.clone();
                let failure =
                    Some(self.fail_run(run, FailureCause::ManualCancellationUnknown, now));
                actions.push(WatchdogAction::ForceCancel {
                    tab_id: key.0,
                    turn_id: key.1,
                    job,
                    failure,
                });
            } else if let Some(run) = self.running.get_mut(&key) {
                run.phase = RunPhase::ForceStopping;
                actions.push(WatchdogAction::ForceCancel {
                    tab_id: key.0,
                    turn_id: key.1,
                    job: run.job.clone(),
                    failure: None,
                });
            }
        }
        actions
    }

    /// Complete a canonical or draining turn. A timeout keeps its timeout
    /// cause even when the provider reports a generic abort error afterward.
    pub(super) fn finish(
        &mut self,
        tab_id: usize,
        turn_id: u64,
        result: &Result<(), String>,
        now: Instant,
    ) -> Option<Failure> {
        let run = self.running.remove(&(tab_id, turn_id))?;
        if matches!(run.phase, RunPhase::Cancelling { .. }) {
            if run.pulse.side_effect_risk() {
                return Some(self.fail_run(run, FailureCause::ManualCancellationUnknown, now));
            }
            self.clear_job(&run.job);
            return None;
        }
        if matches!(run.phase, RunPhase::ForceStopping) {
            if run.pulse.side_effect_risk() || run.pulse.unresolved_external_work() {
                return Some(self.fail_run(run, FailureCause::ManualCancellationUnknown, now));
            }
            if result.is_ok() {
                self.clear_job(&run.job);
                return None;
            }
            let cause = FailureCause::TurnFailed(
                result
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "unknown error".to_string()),
            );
            return Some(self.fail_run(run, cause, now));
        }
        if result.is_ok() && matches!(run.phase, RunPhase::Active) {
            if run.pulse.unresolved_external_work() {
                return Some(self.fail_run(run, FailureCause::UnresolvedExternalWork, now));
            }
            self.clear_job(&run.job);
            return None;
        }
        let cause = match run.phase {
            RunPhase::Aborting { kind, .. } => FailureCause::Timeout(kind),
            RunPhase::Cancelling { .. } => unreachable!("handled above"),
            RunPhase::ForceStopping => unreachable!("handled above"),
            RunPhase::Active => FailureCause::TurnFailed(
                result
                    .as_ref()
                    .err()
                    .cloned()
                    .unwrap_or_else(|| "unknown error".to_string()),
            ),
        };
        Some(self.fail_run(run, cause, now))
    }

    /// Manual interruption is not a failure and must never trigger a retry,
    /// but it remains supervised until the worker drains or the same bounded
    /// hard-stop grace expires. Removing it here would leave an uncooperative
    /// provider permanently stuck in the tab's draining slot.
    pub(super) fn cancel_turn(&mut self, tab_id: usize, turn_id: u64, now: Instant) {
        let grace = self.config.abort_grace();
        if let Some(run) = self.running.get_mut(&(tab_id, turn_id)) {
            if matches!(run.phase, RunPhase::Active) {
                self.pending_retries.remove(&run.job);
                run.phase = RunPhase::Cancelling {
                    deadline: now + grace,
                };
            }
        }
    }

    /// Drop a record whose immutable `(tab, turn)` owner no longer exists.
    /// Unlike a user cancellation there is nothing left to supervise locally.
    pub(super) fn forget_turn(&mut self, tab_id: usize, turn_id: u64) {
        if let Some(run) = self.running.remove(&(tab_id, turn_id)) {
            self.clear_job(&run.job);
        }
    }

    pub(super) fn cancel_tab(&mut self, tab_id: usize) -> Vec<SchedJobRef> {
        let jobs: HashSet<SchedJobRef> = self
            .running
            .values()
            .filter(|run| run.tab_id == tab_id)
            .map(|run| run.job.clone())
            .chain(
                self.pending_retries
                    .values()
                    .filter(|retry| retry.tab_id == tab_id)
                    .map(|retry| retry.job.clone()),
            )
            .collect();
        self.running.retain(|_, run| run.tab_id != tab_id);
        self.pending_retries
            .retain(|_, retry| retry.tab_id != tab_id);
        for job in &jobs {
            self.failures.remove(job);
            self.suppressed_recovery.remove(job);
        }
        jobs.into_iter().collect()
    }

    pub(super) fn cancel_job(&mut self, job: &SchedJobRef) {
        self.running.retain(|_, run| &run.job != job);
        self.pending_retries.remove(job);
        self.failures.remove(job);
        self.suppressed_recovery.remove(job);
    }

    /// Forget queued recovery for jobs removed/disabled by the user while
    /// leaving an already-running turn watched until it exits. Stopping future
    /// recurrence must not make the current process immune to liveness checks.
    pub(super) fn cancel_recoveries_matching(&mut self, is_gone: impl Fn(&SchedJobRef) -> bool) {
        let suppressed: Vec<SchedJobRef> = self
            .running
            .values()
            .map(|run| &run.job)
            .chain(self.pending_retries.keys())
            .chain(self.failures.keys())
            .filter(|job| is_gone(job))
            .cloned()
            .collect();
        for job in suppressed {
            self.suppressed_recovery.insert(job);
        }
        self.pending_retries.retain(|job, _| !is_gone(job));
        self.failures.retain(|job, _| !is_gone(job));
    }

    pub(super) fn job_is_occupied(&self, job: &SchedJobRef) -> bool {
        self.running.values().any(|run| &run.job == job) || self.pending_retries.contains_key(job)
    }

    /// Restore a persisted consecutive-failure count before a schedule starts
    /// its next attempt. Existing in-memory progress wins.
    pub(super) fn seed_failures(&mut self, job: &SchedJobRef, failures: u32) {
        if failures > 0 {
            self.failures.entry(job.clone()).or_insert(failures);
        }
    }

    pub(super) fn restore_retry(
        &mut self,
        job: SchedJobRef,
        tab_id: usize,
        prompt: String,
        failures: u32,
        due_at: Instant,
        retry_token_ms: u64,
    ) {
        if !self.enabled() || failures == 0 {
            return;
        }
        self.failures.insert(job.clone(), failures);
        self.pending_retries.insert(
            job.clone(),
            PendingRetry {
                job,
                tab_id,
                prompt,
                attempt: failures,
                due_at,
                retry_token_ms: Some(retry_token_ms),
            },
        );
    }

    /// Attach the persisted epoch token after the caller commits recovery
    /// state. Schedules use it for a cross-process first-writer-wins claim;
    /// in-memory loops intentionally have no token.
    pub(super) fn set_retry_token(&mut self, job: &SchedJobRef, retry_token_ms: u64) {
        if let Some(retry) = self.pending_retries.get_mut(job) {
            retry.retry_token_ms = Some(retry_token_ms);
        }
    }

    pub(super) fn retry_token(&self, job: &SchedJobRef) -> Option<u64> {
        self.pending_retries
            .get(job)
            .and_then(|retry| retry.retry_token_ms)
    }

    /// Convert a claim that remained queued beyond its start deadline into the
    /// same bounded recovery path as a side-effect-free turn failure.
    pub(super) fn fail_queued(
        &mut self,
        job: SchedJobRef,
        tab_id: usize,
        prompt: String,
        now: Instant,
    ) -> Failure {
        self.running.retain(|_, run| run.job != job);
        self.fail_job(
            job,
            tab_id,
            prompt,
            FailureCause::QueueStartTimeout,
            false,
            now,
        )
    }

    pub(super) fn occupied_loop_ids(&self) -> HashSet<u32> {
        self.running
            .values()
            .map(|run| &run.job)
            .chain(self.pending_retries.keys())
            .filter_map(|job| match job {
                SchedJobRef::Loop(id) => Some(*id),
                SchedJobRef::Schedule(_) => None,
            })
            .collect()
    }

    pub(super) fn occupied_schedule_ids(&self) -> HashSet<String> {
        self.running
            .values()
            .map(|run| &run.job)
            .chain(self.pending_retries.keys())
            .filter_map(|job| match job {
                SchedJobRef::Loop(_) => None,
                SchedJobRef::Schedule(id) => Some(id.clone()),
            })
            .collect()
    }

    pub(super) fn due_retries(&self, now: Instant) -> Vec<DueRetry> {
        let mut due: Vec<DueRetry> = self
            .pending_retries
            .values()
            .filter(|retry| retry.due_at <= now)
            .map(|retry| DueRetry {
                job: retry.job.clone(),
                tab_id: retry.tab_id,
                prompt: retry.prompt.clone(),
                attempt: retry.attempt,
            })
            .collect();
        due.sort_by_key(|retry| retry.attempt);
        due
    }

    pub(super) fn status_lines(&self, now: Instant) -> Vec<String> {
        let mut lines = vec![format!(
            "watchdog: {} · queue/inactivity {}s · runtime {}s · grace {}s · retries {} · backoff {}s..{}s",
            if self.enabled() { "enabled" } else { "disabled" },
            self.config.inactivity_timeout().as_secs(),
            self.config.max_runtime().as_secs(),
            self.config.abort_grace().as_secs(),
            self.config.max_retries(),
            self.config.initial_backoff().as_secs(),
            self.config.max_backoff().as_secs(),
        )];
        let mut detail = Vec::new();
        for run in self.running.values() {
            let pulse = run.pulse.snapshot();
            let state = match run.phase {
                RunPhase::Active => format!(
                    "alive; last activity {}s ago",
                    now.saturating_duration_since(pulse.last_activity_at)
                        .as_secs()
                ),
                RunPhase::Aborting { kind, deadline } => format!(
                    "cancelling after {}; hard stop in {}s",
                    kind.label(),
                    deadline.saturating_duration_since(now).as_secs()
                ),
                RunPhase::Cancelling { deadline } => format!(
                    "user cancellation draining; hard stop in {}s",
                    deadline.saturating_duration_since(now).as_secs()
                ),
                RunPhase::ForceStopping => "joining force-stopped worker tree".to_string(),
            };
            let replay = if run.pulse.unresolved_external_work() {
                " · detached/external work requires review"
            } else if run.pulse.side_effect_risk() {
                " · auto-retry blocked by side-effect risk"
            } else {
                ""
            };
            detail.push(format!(
                "{} · tab {} turn {} · {state}{replay}",
                job_label(&run.job),
                run.tab_id,
                run.turn_id,
            ));
        }
        for retry in self.pending_retries.values() {
            detail.push(format!(
                "{} · retry {} in {}s · tab {}",
                job_label(&retry.job),
                retry.attempt,
                retry.due_at.saturating_duration_since(now).as_secs(),
                retry.tab_id,
            ));
        }
        detail.sort();
        lines.extend(detail);
        lines
    }

    fn fail_run(&mut self, run: WatchedTurn, cause: FailureCause, now: Instant) -> Failure {
        let unsafe_to_retry = run.pulse.side_effect_risk()
            || run.pulse.unresolved_external_work()
            || matches!(&cause, FailureCause::AbortGraceExpired(_));
        self.fail_job(run.job, run.tab_id, run.prompt, cause, unsafe_to_retry, now)
    }

    fn fail_job(
        &mut self,
        job: SchedJobRef,
        tab_id: usize,
        prompt: String,
        cause: FailureCause,
        unsafe_to_retry: bool,
        now: Instant,
    ) -> Failure {
        if self.suppressed_recovery.remove(&job) {
            self.failures.remove(&job);
            self.pending_retries.remove(&job);
            return Failure {
                job,
                tab_id,
                cause,
                recovery: Recovery::Cancelled,
            };
        }
        let failures = self.failures.entry(job.clone()).or_insert(0);
        *failures = failures.saturating_add(1);
        let failure_count = *failures;
        // A hard stop has unknown execution state even when no mutating
        // start was observed: the missing terminal may also mean the final
        // lifecycle event was lost. Never replay that prompt automatically.
        let recovery = if unsafe_to_retry {
            self.pending_retries.remove(&job);
            Recovery::ManualReview {
                failures: failure_count,
            }
        } else if failure_count <= self.config.max_retries() {
            let delay = retry_delay(&self.config, failure_count);
            self.pending_retries.insert(
                job.clone(),
                PendingRetry {
                    job: job.clone(),
                    tab_id,
                    prompt,
                    attempt: failure_count,
                    due_at: now + delay,
                    retry_token_ms: None,
                },
            );
            Recovery::RetryScheduled {
                attempt: failure_count,
                delay,
            }
        } else {
            self.pending_retries.remove(&job);
            Recovery::Exhausted {
                failures: failure_count,
            }
        };
        Failure {
            job,
            tab_id,
            cause,
            recovery,
        }
    }

    fn clear_job(&mut self, job: &SchedJobRef) {
        self.failures.remove(job);
        self.pending_retries.remove(job);
        self.suppressed_recovery.remove(job);
    }
}

fn retry_delay(config: &BackgroundWatchdogConfig, attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(31);
    let multiplier = 1_u32 << exponent;
    config
        .initial_backoff()
        .saturating_mul(multiplier)
        .min(config.max_backoff())
}

pub(super) fn job_label(job: &SchedJobRef) -> String {
    match job {
        SchedJobRef::Loop(id) => format!("loop #{id}"),
        SchedJobRef::Schedule(id) => format!("schedule {id}"),
    }
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
