//! Persistent store for `/schedule` jobs: `<config-dir>/schedules.json`.
//!
//! Atomic writes and a shared roster lock protect cross-process mutations.
//! Invalid/corrupt input is sanitized or quarantined. A separate stable OS
//! lock per job distinguishes live attempts from crash-orphaned tokens.

use super::{ScheduleJob, ScheduleSpec};
use crate::config::{write_atomic, ConfigManager};
use crate::error::CoreError;
use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::File;
use std::path::{Path, PathBuf};

fn store_path() -> Result<PathBuf, CoreError> {
    Ok(ConfigManager::config_dir()?.join("schedules.json"))
}

/// Load the roster, returning empty on missing, corrupt, or unreadable input.
pub fn load_schedules() -> Vec<ScheduleJob> {
    match try_load_schedules() {
        Ok(jobs) => jobs,
        Err(error) => {
            tracing::warn!(%error, "schedules store: failed to load roster");
            Vec::new()
        }
    }
}

/// Fallible load for callers that must distinguish I/O failure from no jobs.
pub fn try_load_schedules() -> Result<Vec<ScheduleJob>, CoreError> {
    with_store_lock(load_and_migrate_schedules_unlocked)
}

fn load_and_migrate_schedules_unlocked() -> Result<Vec<ScheduleJob>, CoreError> {
    let mut jobs = load_schedules_unlocked()?;
    if jobs.is_empty() || !jobs.iter().any(|job| job.last_fired_ms.is_none()) {
        return Ok(jobs);
    }

    // Migrate old jobs to one stable cross-process recurrence anchor.
    let anchor_ms = store_path()
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_to_epoch_ms)
        .unwrap_or_else(epoch_ms_now);
    for job in &mut jobs {
        if job.last_fired_ms.is_none() {
            job.last_fired_ms = Some(anchor_ms);
        }
    }
    save_schedules_unlocked(&jobs)?;
    Ok(jobs)
}

fn load_schedules_unlocked() -> Result<Vec<ScheduleJob>, CoreError> {
    let path = store_path()?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let jobs: Vec<ScheduleJob> = match serde_json::from_str(&raw) {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::warn!(
                "schedules store: corrupt {}, quarantining: {e}",
                path.display()
            );
            quarantine(&path);
            return Ok(Vec::new());
        }
    };
    Ok(sanitize(jobs))
}

/// Drop invalid specs/prompts/ids and later duplicate ids with warnings.
const MIN_INTERVAL_SECS: u64 = 30;

fn sanitize(jobs: Vec<ScheduleJob>) -> Vec<ScheduleJob> {
    let mut seen_ids = HashSet::new();
    jobs.into_iter()
        .filter(|job| {
            let mut valid = true;
            let mut reason = String::new();

            if job.id.trim().is_empty() {
                valid = false;
                reason = "empty id".to_string();
            }

            // Check spec validity.
            match &job.spec {
                ScheduleSpec::Daily { hour, minute }
                | ScheduleSpec::Weekly { hour, minute, .. } => {
                    if valid && (*hour > 23 || *minute > 59) {
                        valid = false;
                        reason = "out-of-range hour or minute".to_string();
                    }
                }
                ScheduleSpec::Interval { secs } => {
                    if valid && *secs < MIN_INTERVAL_SECS {
                        valid = false;
                        reason = format!(
                            "interval below minimum ({} < {} secs)",
                            secs, MIN_INTERVAL_SECS
                        );
                    }
                }
            }

            // Check prompt validity (must not start with / or ! after trimming).
            if valid && job.prompt.trim_start().starts_with(['/', '!']) {
                valid = false;
                reason = "prompt starts with '/' or '!'".to_string();
            }

            if valid && !seen_ids.insert(job.id.clone()) {
                valid = false;
                reason = "duplicate id".to_string();
            }

            if !valid {
                tracing::warn!("schedules store: dropping job {:?}: {}", job.id, reason);
            }
            valid
        })
        .collect()
}

/// Best-effort rename of corrupt input for manual recovery.
fn quarantine(path: &Path) {
    let corrupt = path.with_extension("json.corrupt");
    if let Err(e) = std::fs::rename(path, &corrupt) {
        tracing::warn!(
            "schedules store: failed to quarantine {}: {e}",
            path.display()
        );
    }
}

/// Persist the roster with a writer-unique temporary file and atomic rename.
pub fn save_schedules(jobs: &[ScheduleJob]) -> Result<(), CoreError> {
    with_store_lock(|| save_schedules_unlocked(jobs))
}

fn save_schedules_unlocked(jobs: &[ScheduleJob]) -> Result<(), CoreError> {
    let path = store_path()?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(jobs)?;
    write_atomic(&path, json.as_bytes())?;
    Ok(())
}

fn with_store_lock<T>(f: impl FnOnce() -> Result<T, CoreError>) -> Result<T, CoreError> {
    let path = store_path()?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let lock_path = dir.join("schedules.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let result = f();
    let _ = FileExt::unlock(&lock);
    result
}

/// Claim an exact canonical fire token under the cross-process roster lock.
pub fn try_mark_fired(id: &str, fire_ms: u64) -> bool {
    match with_store_lock(|| try_mark_fired_unlocked(id, fire_ms)) {
        Ok(claimed) => claimed,
        Err(e) => {
            tracing::warn!("schedules store: failed to claim fire mark for {id}: {e}");
            false
        }
    }
}

fn try_mark_fired_unlocked(id: &str, fire_ms: u64) -> Result<bool, CoreError> {
    let mut jobs = load_schedules_unlocked()?;
    let Some(job) = jobs.iter_mut().find(|j| j.id == id) else {
        return Ok(false);
    };
    if !job.enabled || job.watchdog_retry_at_ms.is_some() || job.watchdog_active_since_ms.is_some()
    {
        return Ok(false);
    }
    if job.last_fired_ms.is_some_and(|last| last >= fire_ms) {
        return Ok(false);
    }
    job.last_fired_ms = Some(fire_ms);
    save_schedules_unlocked(&jobs)?;
    Ok(true)
}

/// Exclusive OS-lock ownership of one persisted schedule attempt.
pub struct ScheduleAttemptLease {
    schedule_id: String,
    active_token_ms: u64,
    fire_claim: Option<ScheduleFireClaim>,
    lock: File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScheduleFireClaim {
    fire_ms: u64,
    previous_last_fired_ms: Option<u64>,
}

impl ScheduleAttemptLease {
    pub fn schedule_id(&self) -> &str {
        &self.schedule_id
    }

    pub fn active_token_ms(&self) -> u64 {
        self.active_token_ms
    }
}

impl std::fmt::Debug for ScheduleAttemptLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduleAttemptLease")
            .field("schedule_id", &self.schedule_id)
            .field("active_token_ms", &self.active_token_ms)
            .field("fire_claim", &self.fire_claim)
            .finish_non_exhaustive()
    }
}

impl Drop for ScheduleAttemptLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
    }
}

fn attempt_lock_path(id: &str) -> Result<PathBuf, CoreError> {
    let dir = store_path()?
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("schedule-attempts");
    std::fs::create_dir_all(&dir)?;
    let digest = Sha256::digest(id.as_bytes());
    let mut name = String::with_capacity(digest.len() * 2 + 5);
    for byte in digest {
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".lock");
    Ok(dir.join(name))
}

fn try_acquire_attempt_lock(id: &str) -> Result<Option<File>, CoreError> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(attempt_lock_path(id)?)?;
    match FileExt::try_lock_exclusive(&lock) {
        Ok(()) => Ok(Some(lock)),
        Err(error) if error.kind() == fs4::lock_contended_error().kind() => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Atomically claim a canonical fire and its OS-lock-backed watchdog lease.
pub fn try_claim_watchdog_fire(
    id: &str,
    fire_ms: u64,
) -> Result<Option<ScheduleAttemptLease>, CoreError> {
    let Some(lock) = try_acquire_attempt_lock(id)? else {
        return Ok(None);
    };
    let claim = with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(None);
        };
        if !job.enabled
            || job.watchdog_retry_at_ms.is_some()
            || job.watchdog_active_since_ms.is_some()
            || job.last_fired_ms.is_some_and(|last| last >= fire_ms)
        {
            return Ok(None);
        }
        let previous_last_fired_ms = job.last_fired_ms;
        let active_token_ms = epoch_ms_now();
        job.last_fired_ms = Some(fire_ms);
        job.watchdog_active_since_ms = Some(active_token_ms);
        save_schedules_unlocked(&jobs)?;
        Ok(Some((active_token_ms, previous_last_fired_ms)))
    })?;
    Ok(claim.map(
        |(active_token_ms, previous_last_fired_ms)| ScheduleAttemptLease {
            schedule_id: id.to_string(),
            active_token_ms,
            fire_claim: Some(ScheduleFireClaim {
                fire_ms,
                previous_last_fired_ms,
            }),
            lock,
        },
    ))
}

/// Claim a due retry by its exact token and return its live attempt lease.
pub fn try_claim_watchdog_retry(
    id: &str,
    expected_retry_at_ms: u64,
) -> Result<Option<ScheduleAttemptLease>, CoreError> {
    let Some(lock) = try_acquire_attempt_lock(id)? else {
        return Ok(None);
    };
    let active_token_ms = with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(None);
        };
        if !job.enabled
            || job.watchdog_retry_at_ms != Some(expected_retry_at_ms)
            || job.watchdog_active_since_ms.is_some()
            || epoch_ms_now() < expected_retry_at_ms
        {
            return Ok(None);
        }
        job.watchdog_retry_at_ms = None;
        let active_token_ms = epoch_ms_now();
        job.watchdog_active_since_ms = Some(active_token_ms);
        save_schedules_unlocked(&jobs)?;
        Ok(Some(active_token_ms))
    })?;
    Ok(active_token_ms.map(|active_token_ms| ScheduleAttemptLease {
        schedule_id: id.to_string(),
        active_token_ms,
        fire_claim: None,
        lock,
    }))
}

/// Claim a non-retry attempt immediately before starting its turn.
pub fn try_begin_watchdog_attempt(id: &str) -> Result<Option<ScheduleAttemptLease>, CoreError> {
    let Some(lock) = try_acquire_attempt_lock(id)? else {
        return Ok(None);
    };
    let active_token_ms = with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(None);
        };
        if !job.enabled
            || job.watchdog_retry_at_ms.is_some()
            || job.watchdog_active_since_ms.is_some()
        {
            return Ok(None);
        }
        let active_since_ms = epoch_ms_now();
        job.watchdog_active_since_ms = Some(active_since_ms);
        save_schedules_unlocked(&jobs)?;
        Ok(Some(active_since_ms))
    })?;
    Ok(active_token_ms.map(|active_token_ms| ScheduleAttemptLease {
        schedule_id: id.to_string(),
        active_token_ms,
        fire_claim: None,
        lock,
    }))
}

/// Restore an unstarted fire after exact id, active, and fire-token checks.
pub fn restore_claimed_watchdog_fire_for_shutdown(
    lease: &ScheduleAttemptLease,
) -> Result<bool, CoreError> {
    let Some(claim) = lease.fire_claim else {
        return Ok(false);
    };
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == lease.schedule_id) else {
            return Ok(false);
        };
        if job.watchdog_active_since_ms != Some(lease.active_token_ms)
            || job.last_fired_ms != Some(claim.fire_ms)
        {
            return Ok(false);
        }
        job.watchdog_active_since_ms = None;
        job.last_fired_ms = claim.previous_last_fired_ms;
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

/// Clear only the exact active token owned by a queued lease.
pub fn clear_watchdog_attempt_if(
    id: &str,
    expected_active_token_ms: u64,
) -> Result<bool, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(false);
        };
        if job.watchdog_active_since_ms != Some(expected_active_token_ms) {
            return Ok(false);
        }
        job.watchdog_active_since_ms = None;
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

/// Cancel one not-yet-claimed retry by its exact persisted deadline while
/// preserving failure history and every unrelated roster field.
pub fn clear_watchdog_retry_if(id: &str, expected_retry_at_ms: u64) -> Result<bool, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(false);
        };
        if job.watchdog_retry_at_ms != Some(expected_retry_at_ms)
            || job.watchdog_active_since_ms.is_some()
        {
            return Ok(false);
        }
        job.watchdog_retry_at_ms = None;
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

/// Restore an unstarted retry while the caller still holds its lease.
pub fn restore_claimed_watchdog_retry_for_shutdown(
    id: &str,
    expected_active_token_ms: u64,
    retry_at_ms: u64,
) -> Result<bool, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(false);
        };
        if job.watchdog_active_since_ms != Some(expected_active_token_ms)
            || job.watchdog_retry_at_ms.is_some()
        {
            return Ok(false);
        }
        job.watchdog_active_since_ms = None;
        job.watchdog_retry_at_ms = job.enabled.then_some(retry_at_ms);
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

/// Fail-closed outcome after an unstarted queue restore loses its CAS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogQueueRestoreConflict {
    /// The row no longer exists, so the lease is safe to release.
    Missing,
    /// The expected token was cleared and its row was durably disabled.
    OwnedAttemptQuarantined { newly_disabled: bool },
    /// Ownership changed; the current token was preserved and its row disabled.
    OwnershipChanged {
        persisted_active_token_ms: Option<u64>,
        newly_disabled: bool,
    },
}

/// Quarantine an unstarted attempt whose precise fire/retry restore conflicted.
///
/// The exact owned token is cleared only while disabling the row atomically.
/// A missing row is safe, while a mismatched/no token is never cleared and its
/// row is disabled. Current fire and watchdog recovery fields are preserved.
pub fn quarantine_claimed_watchdog_queue_restore_conflict(
    id: &str,
    expected_active_token_ms: u64,
) -> Result<WatchdogQueueRestoreConflict, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(WatchdogQueueRestoreConflict::Missing);
        };
        let persisted_active_token_ms = job.watchdog_active_since_ms;
        let newly_disabled = job.enabled;
        job.enabled = false;
        let outcome = if persisted_active_token_ms == Some(expected_active_token_ms) {
            job.watchdog_active_since_ms = None;
            WatchdogQueueRestoreConflict::OwnedAttemptQuarantined { newly_disabled }
        } else {
            WatchdogQueueRestoreConflict::OwnershipChanged {
                persisted_active_token_ms,
                newly_disabled,
            }
        };
        if newly_disabled || persisted_active_token_ms == Some(expected_active_token_ms) {
            save_schedules_unlocked(&jobs)?;
        }
        Ok(outcome)
    })
}

fn epoch_ms_now() -> u64 {
    system_time_to_epoch_ms(std::time::SystemTime::now()).unwrap_or(0)
}

fn system_time_to_epoch_ms(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

/// Result of an atomic roster mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleRosterUpdate {
    /// Whether the requested command was applied.
    pub applied: bool,
    /// Authoritative roster from the same locked mutation.
    pub schedules: Vec<ScheduleJob>,
}

/// Add a schedule with a shared persisted creation anchor.
pub fn add_schedule_atomic(mut job: ScheduleJob) -> Result<ScheduleRosterUpdate, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_and_migrate_schedules_unlocked()?;
        if jobs.iter().any(|existing| existing.id == job.id) {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        }
        if job.last_fired_ms.is_none() {
            job.last_fired_ms = Some(epoch_ms_now());
        }
        jobs.push(job);
        save_schedules_unlocked(&jobs)?;
        Ok(ScheduleRosterUpdate {
            applied: true,
            schedules: jobs,
        })
    })
}

/// Remove an idle schedule without deleting active recovery evidence.
pub fn remove_schedule_atomic(id: &str) -> Result<ScheduleRosterUpdate, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_and_migrate_schedules_unlocked()?;
        let Some(index) = jobs.iter().position(|job| job.id == id) else {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        };
        if jobs[index].watchdog_active_since_ms.is_some() {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        }
        jobs.remove(index);
        save_schedules_unlocked(&jobs)?;
        Ok(ScheduleRosterUpdate {
            applied: true,
            schedules: jobs,
        })
    })
}

/// Enable an idle schedule with a fresh canonical cadence.
pub fn enable_schedule_atomic(id: &str) -> Result<ScheduleRosterUpdate, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_and_migrate_schedules_unlocked()?;
        let Some(index) = jobs.iter().position(|job| job.id == id) else {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        };
        if jobs[index].watchdog_active_since_ms.is_some() {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        }
        let job = &mut jobs[index];
        job.enabled = true;
        job.last_fired_ms = Some(epoch_ms_now());
        job.watchdog_failures = 0;
        job.watchdog_last_failure_ms = None;
        job.watchdog_retry_at_ms = None;
        save_schedules_unlocked(&jobs)?;
        Ok(ScheduleRosterUpdate {
            applied: true,
            schedules: jobs,
        })
    })
}

/// Disable one schedule under the roster lock without replacing unrelated
/// jobs or newer fire/watchdog state.
pub fn disable_schedule_atomic(id: &str) -> Result<ScheduleRosterUpdate, CoreError> {
    mutate_schedule_roster(id, |jobs, index| {
        jobs[index].enabled = false;
    })
}

fn mutate_schedule_roster(
    id: &str,
    mutation: impl FnOnce(&mut Vec<ScheduleJob>, usize),
) -> Result<ScheduleRosterUpdate, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_and_migrate_schedules_unlocked()?;
        let Some(index) = jobs.iter().position(|job| job.id == id) else {
            return Ok(ScheduleRosterUpdate {
                applied: false,
                schedules: jobs,
            });
        };
        mutation(&mut jobs, index);
        save_schedules_unlocked(&jobs)?;
        Ok(ScheduleRosterUpdate {
            applied: true,
            schedules: jobs,
        })
    })
}

/// Outcome of checking and recovering one persisted active-attempt token.
#[derive(Debug, Clone, PartialEq)]
pub enum OrphanAttemptRecovery {
    /// The per-job OS lock still has a live owner.
    Live,
    /// The row disappeared, cleared, or carries another token.
    Stale,
    /// The exact orphan was recovered into a disabled state.
    Recovered(Vec<ScheduleJob>),
}

/// Recover only an exact orphan token after acquiring its per-job lock.
pub fn recover_orphaned_watchdog_attempt(
    id: &str,
    expected_active_token_ms: u64,
) -> Result<OrphanAttemptRecovery, CoreError> {
    let Some(_attempt_lock) = try_acquire_attempt_lock(id)? else {
        return Ok(OrphanAttemptRecovery::Live);
    };
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(OrphanAttemptRecovery::Stale);
        };
        if job.watchdog_active_since_ms != Some(expected_active_token_ms) {
            return Ok(OrphanAttemptRecovery::Stale);
        }
        let already_quarantined = !job.enabled && job.watchdog_last_failure_ms.is_some();
        job.enabled = false;
        if !already_quarantined {
            job.watchdog_failures = job.watchdog_failures.saturating_add(1);
            job.watchdog_last_failure_ms = Some(epoch_ms_now());
        }
        job.watchdog_retry_at_ms = None;
        job.watchdog_active_since_ms = None;
        save_schedules_unlocked(&jobs)?;
        Ok(OrphanAttemptRecovery::Recovered(jobs))
    })
}

/// Complete or quarantine one live attempt by its exact persisted token.
/// The caller must still hold that attempt's OS lease. A stale terminal can
/// never clear a newer process's claim, while unrelated prompt/spec edits and
/// a concurrent disable remain preserved.
pub fn persist_watchdog_state_for_attempt(
    id: &str,
    expected_active_since_ms: u64,
    failures: u32,
    last_failure_ms: Option<u64>,
    retry_at_ms: Option<u64>,
    active_since_ms: Option<u64>,
    enabled: Option<bool>,
) -> Result<bool, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(false);
        };
        if job.watchdog_active_since_ms != Some(expected_active_since_ms) {
            return Ok(false);
        }
        job.watchdog_failures = failures;
        job.watchdog_last_failure_ms = last_failure_ms;
        job.watchdog_retry_at_ms = retry_at_ms;
        job.watchdog_active_since_ms = active_since_ms;
        if let Some(enabled) = enabled {
            job.enabled = enabled;
        }
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

/// Fail-closed classification after terminal persistence loses its CAS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogTerminalConflict {
    /// The row still has the caller's token, so terminal persistence can retry.
    StillOwned,
    /// The schedule no longer exists, so releasing the lease cannot re-enable it.
    Missing,
    /// A different/no token is present and the row is durably disabled.
    SafeToRelease {
        persisted_active_token_ms: Option<u64>,
        newly_disabled: bool,
    },
}

/// Resolve a failed terminal CAS without dropping a lease onto an enabled row.
///
/// An exact token remains owned and should retry normal terminal persistence.
/// A missing row is already safe. Any mismatched or cleared token is preserved
/// but its row is atomically disabled, allowing shutdown to release the stale
/// lease without erasing another terminal/owner's recovery evidence.
pub fn quarantine_watchdog_terminal_conflict(
    id: &str,
    expected_active_token_ms: u64,
) -> Result<WatchdogTerminalConflict, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == id) else {
            return Ok(WatchdogTerminalConflict::Missing);
        };
        if job.watchdog_active_since_ms == Some(expected_active_token_ms) {
            return Ok(WatchdogTerminalConflict::StillOwned);
        }
        let persisted_active_token_ms = job.watchdog_active_since_ms;
        let newly_disabled = job.enabled;
        if newly_disabled {
            job.enabled = false;
            save_schedules_unlocked(&jobs)?;
        }
        Ok(WatchdogTerminalConflict::SafeToRelease {
            persisted_active_token_ms,
            newly_disabled,
        })
    })
}

/// Apply one startup-only idle watchdog transition if the entire scheduling
/// and recovery snapshot is still authoritative. Matching only `active=None`
/// is insufficient: an enable or newly-written retry can return to idle and
/// would otherwise be overwritten by a stale startup pass (an ABA race).
pub fn persist_idle_watchdog_state_if_matches(
    expected: &ScheduleJob,
    failures: u32,
    last_failure_ms: Option<u64>,
    retry_at_ms: Option<u64>,
    enabled: Option<bool>,
) -> Result<bool, CoreError> {
    with_store_lock(|| {
        let mut jobs = load_schedules_unlocked()?;
        let Some(job) = jobs.iter_mut().find(|job| job.id == expected.id) else {
            return Ok(false);
        };
        let snapshot_matches = job.enabled == expected.enabled
            && job.spec == expected.spec
            && job.prompt == expected.prompt
            && job.last_fired_ms == expected.last_fired_ms
            && job.watchdog_failures == expected.watchdog_failures
            && job.watchdog_last_failure_ms == expected.watchdog_last_failure_ms
            && job.watchdog_retry_at_ms == expected.watchdog_retry_at_ms
            && job.watchdog_active_since_ms.is_none()
            && expected.watchdog_active_since_ms.is_none();
        if !snapshot_matches {
            return Ok(false);
        }
        job.watchdog_failures = failures;
        job.watchdog_last_failure_ms = last_failure_ms;
        job.watchdog_retry_at_ms = retry_at_ms;
        if let Some(enabled) = enabled {
            job.enabled = enabled;
        }
        save_schedules_unlocked(&jobs)?;
        Ok(true)
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
