//! Persistent store for `/schedule` jobs: `<config-dir>/schedules.json`.
//!
//! Reuses `config::write_atomic` — the same writer-unique-temp-file +
//! `fs::rename` helper `ConfigManager::save_global` uses — plus two
//! defensive behaviors a persisted roster needs that config.json doesn't:
//!
//! - **Corrupt-file quarantine**: an unparseable `schedules.json` is renamed
//!   to `schedules.json.corrupt` (never silently overwritten or deleted) and
//!   load falls back to an empty roster, so one bad write never blocks
//!   startup.
//! - **Load-time sanitization**: `ScheduleSpec::next_after` uses
//!   `.expect("validated")` on `and_hms_opt` for `Daily`/`Weekly` specs — it
//!   assumes the hour/minute are in range. The in-process `/schedule` command
//!   (Task 8) validates before construction, but a hand-edited or
//!   foreign-written `schedules.json` bypasses that. `load_schedules` drops
//!   any entry with an out-of-range `Daily`/`Weekly` hour/minute rather than
//!   loading a job that would later panic the scheduler tick.
//! - **Cross-process fire dedup**: `try_mark_fired` does a full
//!   load-mutate-save under the file system as the only shared state (no
//!   lock file), so two processes racing to fire the same minute-floored
//!   trigger have exactly one winner — whichever's rename lands last simply
//!   overwrites with the same `last_fired_ms`, but only the process that
//!   observed the *unmarked* state returns `true`.

use super::{ScheduleJob, ScheduleSpec};
use crate::config::{write_atomic, ConfigManager};
use crate::error::CoreError;
use std::path::{Path, PathBuf};

fn store_path() -> Result<PathBuf, CoreError> {
    Ok(ConfigManager::config_dir()?.join("schedules.json"))
}

/// Load the persisted `/schedule` roster. A missing file loads as empty (no
/// error — first run). A corrupt file is quarantined (renamed to
/// `schedules.json.corrupt`, warned via `tracing`) and also loads as empty.
/// Entries with an out-of-range `Daily`/`Weekly` hour/minute are dropped
/// (warned) rather than returned, since `ScheduleSpec::next_after` assumes
/// validated specs.
pub fn load_schedules() -> Vec<ScheduleJob> {
    let path = match store_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("schedules store: cannot resolve config dir: {e}");
            return Vec::new();
        }
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("schedules store: failed to read {}: {e}", path.display());
            return Vec::new();
        }
    };
    let jobs: Vec<ScheduleJob> = match serde_json::from_str(&raw) {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::warn!(
                "schedules store: corrupt {}, quarantining: {e}",
                path.display()
            );
            quarantine(&path);
            return Vec::new();
        }
    };
    sanitize(jobs)
}

/// Drop entries with invalid specs or prompts, warning once per dropped entry:
///
/// - `Daily`/`Weekly` with out-of-range hour/minute (hour > 23 or minute > 59)
/// - `Interval` with `secs < 30` (below the parser's minimum)
/// - Prompts whose trimmed form starts with `/` or `!` (rejected at parse time,
///   but can bypass via hand-edited `schedules.json`)
const MIN_INTERVAL_SECS: u64 = 30;

fn sanitize(jobs: Vec<ScheduleJob>) -> Vec<ScheduleJob> {
    jobs.into_iter()
        .filter(|job| {
            let mut valid = true;
            let mut reason = String::new();

            // Check spec validity.
            match &job.spec {
                ScheduleSpec::Daily { hour, minute }
                | ScheduleSpec::Weekly { hour, minute, .. } => {
                    if *hour > 23 || *minute > 59 {
                        valid = false;
                        reason = "out-of-range hour or minute".to_string();
                    }
                }
                ScheduleSpec::Interval { secs } => {
                    if *secs < MIN_INTERVAL_SECS {
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

            if !valid {
                tracing::warn!("schedules store: dropping job {:?}: {}", job.id, reason);
            }
            valid
        })
        .collect()
}

/// Rename a corrupt store file to `<name>.corrupt`, best-effort (a failed
/// rename is warned but never propagated — the caller already falls back to
/// an empty roster either way).
fn quarantine(path: &Path) {
    let corrupt = path.with_extension("json.corrupt");
    if let Err(e) = std::fs::rename(path, &corrupt) {
        tracing::warn!(
            "schedules store: failed to quarantine {}: {e}",
            path.display()
        );
    }
}

/// Persist the `/schedule` roster atomically via `config::write_atomic`:
/// stage in a writer-unique (pid + atomic counter suffixed) temp file in the
/// same directory, then `fs::rename` it over `schedules.json`. Readers
/// therefore only ever see the previous complete file or the new complete
/// file, never a partial write — and, critically for `try_mark_fired`'s
/// multi-process racing, two processes saving concurrently never share a
/// single fixed temp path (which would otherwise let interleaved O_TRUNC
/// writes rename corrupt JSON into place and quarantine the whole roster).
pub fn save_schedules(jobs: &[ScheduleJob]) -> Result<(), CoreError> {
    let path = store_path()?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(jobs)?;
    write_atomic(&path, json.as_bytes())?;
    Ok(())
}

/// Attempt to claim the fire slot for `id` at `fire_ms` (minute-floored).
/// Re-reads the store, checks whether `last_fired_ms` for that job is
/// already `>=` the floored `fire_ms`, and if not, writes the floored value
/// back and returns `true`. Returns `false` when another process already
/// claimed this minute (or a later one), or when the job id isn't found.
///
/// This is a full load -> mutate -> save on every call rather than a
/// separate lock file: the dedup granularity is a minute, so the tiny
/// window between two processes' read and write racing on the exact same
/// tick is an accepted, harmless double-fire (rare in practice, and no
/// worse than the pre-Task-7 in-memory-only behavior).
pub fn try_mark_fired(id: &str, fire_ms: u64) -> bool {
    let floored = fire_ms / 60_000 * 60_000;
    let mut jobs = load_schedules();
    let Some(job) = jobs.iter_mut().find(|j| j.id == id) else {
        return false;
    };
    if job.last_fired_ms.is_some_and(|last| last >= floored) {
        return false;
    }
    job.last_fired_ms = Some(floored);
    if let Err(e) = save_schedules(&jobs) {
        tracing::warn!("schedules store: failed to persist fire mark for {id}: {e}");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn roundtrip_and_corrupt_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        assert!(load_schedules().is_empty(), "missing file loads empty");
        let jobs = vec![ScheduleJob {
            id: "ab12".into(),
            spec: ScheduleSpec::Daily { hour: 9, minute: 0 },
            prompt: "standup notes".into(),
            enabled: true,
            last_fired_ms: None,
        }];
        save_schedules(&jobs).unwrap();
        assert_eq!(load_schedules(), jobs);
        std::fs::write(dir.path().join("schedules.json"), "{not json").unwrap();
        assert!(load_schedules().is_empty(), "corrupt file loads empty");
        assert!(dir.path().join("schedules.json.corrupt").exists());
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn try_mark_fired_is_first_writer_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        save_schedules(&[ScheduleJob {
            id: "ab12".into(),
            spec: ScheduleSpec::Interval { secs: 60 },
            prompt: "sync".into(),
            enabled: true,
            last_fired_ms: None,
        }])
        .unwrap();
        let fire = 1_800_000_120_000u64; // arbitrary epoch ms
        assert!(try_mark_fired("ab12", fire), "first claim wins");
        assert!(
            !try_mark_fired("ab12", fire),
            "same trigger point is deduped"
        );
        assert!(
            !try_mark_fired("ab12", fire + 30_000),
            "same minute is deduped"
        );
        assert!(
            try_mark_fired("ab12", fire + 60_000),
            "next minute is a new trigger"
        );
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn load_schedules_drops_out_of_range_spec() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let raw = serde_json::json!([
            {
                "id": "good1",
                "spec": { "kind": "daily", "hour": 9, "minute": 0 },
                "prompt": "standup notes",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "bad1",
                "spec": { "kind": "daily", "hour": 99, "minute": 0 },
                "prompt": "broken",
                "enabled": true,
                "lastFiredMs": null
            }
        ]);
        std::fs::write(
            dir.path().join("schedules.json"),
            serde_json::to_string_pretty(&raw).unwrap(),
        )
        .unwrap();
        let loaded = load_schedules();
        assert_eq!(loaded.len(), 1, "only the in-range entry survives");
        assert_eq!(loaded[0].id, "good1");
        std::env::remove_var("ZODE_CONFIG_DIR");
    }

    #[test]
    #[serial_test::serial]
    fn load_schedules_drops_invalid_prompts_and_intervals() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        let raw = serde_json::json!([
            {
                "id": "good",
                "spec": { "kind": "daily", "hour": 9, "minute": 0 },
                "prompt": "valid prompt",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "slash_prompt",
                "spec": { "kind": "daily", "hour": 10, "minute": 0 },
                "prompt": "/compact",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "slash_with_space",
                "spec": { "kind": "daily", "hour": 11, "minute": 0 },
                "prompt": "  /cost",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "bang_prompt",
                "spec": { "kind": "daily", "hour": 12, "minute": 0 },
                "prompt": "!git status",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "bad_interval",
                "spec": { "kind": "interval", "secs": 1 },
                "prompt": "too fast",
                "enabled": true,
                "lastFiredMs": null
            },
            {
                "id": "min_interval",
                "spec": { "kind": "interval", "secs": 30 },
                "prompt": "at the limit",
                "enabled": true,
                "lastFiredMs": null
            }
        ]);
        std::fs::write(
            dir.path().join("schedules.json"),
            serde_json::to_string_pretty(&raw).unwrap(),
        )
        .unwrap();
        let loaded = load_schedules();
        assert_eq!(loaded.len(), 2, "only valid entries survive");
        assert_eq!(loaded[0].id, "good");
        assert_eq!(loaded[1].id, "min_interval");
        std::env::remove_var("ZODE_CONFIG_DIR");
    }
}
