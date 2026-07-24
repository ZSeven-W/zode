use super::*;

fn schedule(id: &str) -> ScheduleJob {
    ScheduleJob {
        id: id.into(),
        spec: ScheduleSpec::Interval { secs: 30 },
        prompt: format!("run {id}"),
        enabled: true,
        last_fired_ms: Some(1_800_000_000_000),
        watchdog_failures: 0,
        watchdog_last_failure_ms: None,
        watchdog_retry_at_ms: None,
        watchdog_active_since_ms: None,
    }
}

#[test]
#[serial_test::serial]
fn roundtrip_and_corrupt_quarantine() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    assert!(load_schedules().is_empty(), "missing file loads empty");
    let jobs = vec![schedule("ab12")];
    save_schedules(&jobs).unwrap();
    assert_eq!(load_schedules(), jobs);
    std::fs::write(dir.path().join("schedules.json"), "{not json").unwrap();
    assert!(load_schedules().is_empty(), "corrupt file loads empty");
    assert!(dir.path().join("schedules.json.corrupt").exists());
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn legacy_missing_anchor_is_migrated_once_from_store_mtime() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let path = dir.path().join("schedules.json");
    let mut legacy = schedule("legacy");
    legacy.last_fired_ms = None;
    std::fs::write(&path, serde_json::to_vec(&[legacy]).unwrap()).unwrap();
    let anchor_secs = 1_700_000_000;
    filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(anchor_secs, 0)).unwrap();

    let first = load_schedules();
    assert_eq!(first[0].last_fired_ms, Some(anchor_secs as u64 * 1_000));
    assert_eq!(load_schedules()[0].last_fired_ms, first[0].last_fired_ms);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn cas_on_one_job_preserves_an_unrelated_invalid_row_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let path = dir.path().join("schedules.json");

    // A valid row plus a row this build considers invalid (a slash-prefixed
    // prompt). Write both directly so the invalid one lands on disk.
    let valid = schedule("valid");
    let mut invalid = schedule("legacyslash");
    invalid.prompt = "/some-old-command".into();
    std::fs::write(&path, serde_json::to_vec(&[valid, invalid]).unwrap()).unwrap();

    // A CAS against the valid job must not erase the invalid row.
    assert!(try_mark_fired("valid", 1_800_000_100_000));

    let on_disk: Vec<ScheduleJob> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(
        on_disk.iter().any(|job| job.id == "legacyslash"),
        "an unrelated invalid row must survive a CAS on a different job"
    );
    // But the running roster still filters it out.
    assert!(load_schedules().iter().all(|job| job.id != "legacyslash"));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn exact_fire_tokens_allow_both_thirty_second_slots() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let first_fire = 1_800_000_010_123;
    let mut job = schedule("fast");
    job.last_fired_ms = Some(first_fire - 30_000);
    save_schedules(&[job]).unwrap();

    assert!(try_mark_fired("fast", first_fire));
    assert!(!try_mark_fired("fast", first_fire));
    assert!(
        try_mark_fired("fast", first_fire + 30_000),
        "the second slot in the same wall-clock minute is distinct"
    );
    assert_eq!(load_schedules()[0].last_fired_ms, Some(first_fire + 30_000));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn fire_claim_fences_the_attempt_before_a_queued_turn_starts() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let fire_ms = 1_800_000_010_123;
    let mut job = schedule("queued");
    job.last_fired_ms = Some(fire_ms - 30_000);
    save_schedules(&[job]).unwrap();

    let lease = try_claim_watchdog_fire("queued", fire_ms)
        .unwrap()
        .expect("first fire owns the attempt");
    let persisted = load_schedules().pop().unwrap();
    assert_eq!(persisted.last_fired_ms, Some(fire_ms));
    assert_eq!(
        persisted.watchdog_active_since_ms,
        Some(lease.active_token_ms())
    );
    assert!(
        try_claim_watchdog_fire("queued", fire_ms + 30_000)
            .unwrap()
            .is_none(),
        "the queued attempt fences later recurrences"
    );
    assert!(
        !try_mark_fired("queued", fire_ms + 30_000),
        "non-watchdog claimers also observe the active token"
    );

    assert!(!clear_watchdog_attempt_if("queued", lease.active_token_ms() + 1).unwrap());
    assert!(clear_watchdog_attempt_if("queued", lease.active_token_ms()).unwrap());
    drop(lease);
    let next = try_claim_watchdog_fire("queued", fire_ms + 30_000)
        .unwrap()
        .expect("terminal persistence and lease drop reopen the schedule");
    drop(next);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn unstarted_fire_restore_is_an_exact_cas() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let previous_fire_ms = 1_800_000_000_123;
    let fire_ms = previous_fire_ms + 30_000;
    let mut job = schedule("restore-fire");
    job.last_fired_ms = Some(previous_fire_ms);
    save_schedules(&[job]).unwrap();

    let lease = try_claim_watchdog_fire("restore-fire", fire_ms)
        .unwrap()
        .expect("fire should be claimed");
    assert!(disable_schedule_atomic("restore-fire").unwrap().applied);
    assert!(restore_claimed_watchdog_fire_for_shutdown(&lease).unwrap());
    let restored = load_schedules().pop().unwrap();
    assert!(!restored.enabled, "a concurrent disable must be preserved");
    assert_eq!(restored.last_fired_ms, Some(previous_fire_ms));
    assert_eq!(restored.watchdog_active_since_ms, None);
    drop(lease);

    assert!(enable_schedule_atomic("restore-fire").unwrap().applied);
    let anchor = load_schedules()[0].last_fired_ms.unwrap();
    let next_fire_ms = anchor + 30_000;
    let lease = try_claim_watchdog_fire("restore-fire", next_fire_ms)
        .unwrap()
        .expect("fire should be reclaimed");
    let mut raced = load_schedules().pop().unwrap();
    raced.last_fired_ms = Some(next_fire_ms + 30_000);
    save_schedules(&[raced]).unwrap();

    assert!(
        !restore_claimed_watchdog_fire_for_shutdown(&lease).unwrap(),
        "a newer fire watermark must make the rollback stale"
    );
    let unchanged = load_schedules().pop().unwrap();
    assert_eq!(unchanged.last_fired_ms, Some(next_fire_ms + 30_000));
    assert_eq!(
        unchanged.watchdog_active_since_ms,
        Some(lease.active_token_ms())
    );
    assert!(persist_watchdog_state_for_attempt(
        "restore-fire",
        lease.active_token_ms(),
        unchanged.watchdog_failures,
        unchanged.watchdog_last_failure_ms,
        unchanged.watchdog_retry_at_ms,
        None,
        None,
    )
    .unwrap());
    drop(lease);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn fire_claim_is_first_writer_wins() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let fire_ms = 1_800_000_010_123;
    let mut job = schedule("fire-race");
    job.last_fired_ms = Some(fire_ms - 30_000);
    save_schedules(&[job]).unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            try_claim_watchdog_fire("fire-race", fire_ms)
                .unwrap()
                .is_some()
        }));
    }
    barrier.wait();
    let winners = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
    let persisted = load_schedules().pop().unwrap();
    assert_eq!(persisted.last_fired_ms, Some(fire_ms));
    assert!(persisted.watchdog_active_since_ms.is_some());
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn exact_attempt_clear_preserves_concurrent_disable_and_failure_state() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let fire_ms = 1_800_000_010_123;
    let mut job = schedule("clear-only-active");
    job.last_fired_ms = Some(fire_ms - 30_000);
    job.watchdog_failures = 2;
    job.watchdog_last_failure_ms = Some(fire_ms - 1_000);
    save_schedules(&[job]).unwrap();

    let lease = try_claim_watchdog_fire("clear-only-active", fire_ms)
        .unwrap()
        .unwrap();
    assert!(
        disable_schedule_atomic("clear-only-active")
            .unwrap()
            .applied
    );
    assert!(clear_watchdog_attempt_if("clear-only-active", lease.active_token_ms()).unwrap());
    drop(lease);

    let persisted = load_schedules().pop().unwrap();
    assert!(!persisted.enabled);
    assert_eq!(persisted.last_fired_ms, Some(fire_ms));
    assert_eq!(persisted.watchdog_failures, 2);
    assert_eq!(persisted.watchdog_last_failure_ms, Some(fire_ms - 1_000));
    assert_eq!(persisted.watchdog_active_since_ms, None);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn exact_retry_clear_preserves_failure_history() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let retry_at_ms = 1_800_000_020_000;
    let mut job = schedule("cancel-retry");
    job.watchdog_failures = 2;
    job.watchdog_last_failure_ms = Some(retry_at_ms - 5_000);
    job.watchdog_retry_at_ms = Some(retry_at_ms);
    save_schedules(&[job]).unwrap();

    assert!(!clear_watchdog_retry_if("cancel-retry", retry_at_ms - 1).unwrap());
    assert!(clear_watchdog_retry_if("cancel-retry", retry_at_ms).unwrap());
    let persisted = load_schedules().pop().unwrap();
    assert_eq!(persisted.watchdog_retry_at_ms, None);
    assert_eq!(persisted.watchdog_failures, 2);
    assert_eq!(
        persisted.watchdog_last_failure_ms,
        Some(retry_at_ms - 5_000)
    );
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn retry_claim_is_first_writer_wins_and_preserves_other_jobs() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let token = epoch_ms_now().saturating_sub(1);
    let mut retry = schedule("retry1");
    retry.prompt = "sync safely".into();
    retry.watchdog_failures = 2;
    retry.watchdog_last_failure_ms = Some(token - 1_000);
    retry.watchdog_retry_at_ms = Some(token);
    let mut other = schedule("other");
    other.prompt = "untouched".into();
    save_schedules(&[retry, other]).unwrap();
    assert!(try_claim_watchdog_retry("retry1", token - 1)
        .unwrap()
        .is_none());

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            try_claim_watchdog_retry("retry1", token).unwrap().is_some()
        }));
    }
    barrier.wait();
    let wins = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|claimed| *claimed)
        .count();
    assert_eq!(wins, 1);

    let jobs = load_schedules();
    let claimed = jobs.iter().find(|job| job.id == "retry1").unwrap();
    assert_eq!(claimed.watchdog_retry_at_ms, None);
    assert!(claimed.watchdog_active_since_ms.is_some());
    assert_eq!(claimed.watchdog_failures, 2);
    assert_eq!(claimed.prompt, "sync safely");
    assert_eq!(
        jobs.iter().find(|job| job.id == "other").unwrap().prompt,
        "untouched"
    );
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn future_retry_cannot_be_claimed_before_its_deadline() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let retry_at_ms = epoch_ms_now().saturating_add(60_000);
    let mut job = schedule("future-retry");
    job.watchdog_failures = 1;
    job.watchdog_retry_at_ms = Some(retry_at_ms);
    save_schedules(&[job]).unwrap();

    assert!(try_claim_watchdog_retry("future-retry", retry_at_ms)
        .unwrap()
        .is_none());
    let persisted = load_schedules().pop().unwrap();
    assert_eq!(persisted.watchdog_retry_at_ms, Some(retry_at_ms));
    assert_eq!(persisted.watchdog_active_since_ms, None);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn claimed_retry_restore_requires_exact_active_token() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let retry_at_ms = epoch_ms_now().saturating_sub(1);
    let mut job = schedule("restore-retry");
    job.watchdog_failures = 1;
    job.watchdog_retry_at_ms = Some(retry_at_ms);
    save_schedules(&[job]).unwrap();

    let lease = try_claim_watchdog_retry("restore-retry", retry_at_ms)
        .unwrap()
        .expect("due retry should be claimed");
    assert!(!restore_claimed_watchdog_retry_for_shutdown(
        "restore-retry",
        lease.active_token_ms().saturating_add(1),
        retry_at_ms,
    )
    .unwrap());
    assert!(restore_claimed_watchdog_retry_for_shutdown(
        "restore-retry",
        lease.active_token_ms(),
        retry_at_ms,
    )
    .unwrap());
    let restored = load_schedules().pop().unwrap();
    assert_eq!(restored.watchdog_retry_at_ms, Some(retry_at_ms));
    assert_eq!(restored.watchdog_active_since_ms, None);
    assert!(
        try_claim_watchdog_retry("restore-retry", retry_at_ms)
            .unwrap()
            .is_none(),
        "the original process still holds the attempt lock"
    );
    drop(lease);
    let reclaimed = try_claim_watchdog_retry("restore-retry", retry_at_ms)
        .unwrap()
        .expect("restored retry becomes claimable after lease release");
    drop(reclaimed);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn queue_restore_conflict_quarantines_only_the_owned_token() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    assert_eq!(
        quarantine_claimed_watchdog_queue_restore_conflict("missing", 7).unwrap(),
        WatchdogQueueRestoreConflict::Missing
    );
    let mut owned = schedule("owned-queue");
    owned.last_fired_ms = Some(123);
    owned.watchdog_failures = 2;
    owned.watchdog_last_failure_ms = Some(100);
    owned.watchdog_retry_at_ms = Some(200);
    owned.watchdog_active_since_ms = Some(7);
    save_schedules(&[owned]).unwrap();
    assert!(!restore_claimed_watchdog_retry_for_shutdown("owned-queue", 7, 150).unwrap());
    assert_eq!(
        quarantine_claimed_watchdog_queue_restore_conflict("owned-queue", 7).unwrap(),
        WatchdogQueueRestoreConflict::OwnedAttemptQuarantined {
            newly_disabled: true
        }
    );
    let owned = load_schedules().pop().unwrap();
    assert!(!owned.enabled);
    assert_eq!(owned.watchdog_active_since_ms, None);
    assert_eq!(owned.last_fired_ms, Some(123));
    assert_eq!(owned.watchdog_retry_at_ms, Some(200));
    let mut foreign = schedule("foreign-queue");
    foreign.watchdog_active_since_ms = Some(9);
    save_schedules(&[foreign]).unwrap();
    assert_eq!(
        quarantine_claimed_watchdog_queue_restore_conflict("foreign-queue", 8).unwrap(),
        WatchdogQueueRestoreConflict::OwnershipChanged {
            persisted_active_token_ms: Some(9),
            newly_disabled: true,
        }
    );
    let foreign = load_schedules().pop().unwrap();
    assert!(!foreign.enabled);
    assert_eq!(foreign.watchdog_active_since_ms, Some(9));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn live_attempt_lease_is_exclusive_until_terminal_persist_and_drop() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    save_schedules(&[schedule("lease1")]).unwrap();

    let lease = try_begin_watchdog_attempt("lease1")
        .unwrap()
        .expect("first attempt claims lease");
    let token = lease.active_token_ms();
    assert!(try_begin_watchdog_attempt("lease1").unwrap().is_none());
    assert_eq!(
        recover_orphaned_watchdog_attempt("lease1", token).unwrap(),
        OrphanAttemptRecovery::Live
    );
    assert!(
        persist_watchdog_state_for_attempt("lease1", token, 0, None, None, None, None).unwrap()
    );
    assert!(
        try_begin_watchdog_attempt("lease1").unwrap().is_none(),
        "the OS lease remains exclusive until its guard drops"
    );
    drop(lease);
    let next = try_begin_watchdog_attempt("lease1")
        .unwrap()
        .expect("released lease can be reclaimed");
    assert_eq!(next.schedule_id(), "lease1");
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn orphan_recovery_requires_free_lock_and_exact_active_token() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    save_schedules(&[schedule("orphan")]).unwrap();
    let lease = try_begin_watchdog_attempt("orphan").unwrap().unwrap();
    let token = lease.active_token_ms();

    assert_eq!(
        recover_orphaned_watchdog_attempt("orphan", token).unwrap(),
        OrphanAttemptRecovery::Live
    );
    drop(lease);
    assert_eq!(
        recover_orphaned_watchdog_attempt("orphan", token + 1).unwrap(),
        OrphanAttemptRecovery::Stale
    );
    let unchanged = load_schedules().pop().unwrap();
    assert!(unchanged.enabled);
    assert_eq!(unchanged.watchdog_active_since_ms, Some(token));

    let recovered = recover_orphaned_watchdog_attempt("orphan", token).unwrap();
    let OrphanAttemptRecovery::Recovered(roster) = recovered else {
        panic!("exact orphan token should recover");
    };
    assert!(!roster[0].enabled);
    assert_eq!(roster[0].watchdog_failures, 1);
    assert_eq!(roster[0].watchdog_active_since_ms, None);

    let mut quarantined = schedule("quarantined");
    quarantined.enabled = false;
    quarantined.watchdog_failures = 2;
    quarantined.watchdog_last_failure_ms = Some(token + 20);
    quarantined.watchdog_active_since_ms = Some(token + 30);
    save_schedules(&[quarantined]).unwrap();
    let recovered = recover_orphaned_watchdog_attempt("quarantined", token + 30).unwrap();
    let OrphanAttemptRecovery::Recovered(roster) = recovered else {
        panic!("disabled quarantine token should still be cleared");
    };
    assert!(!roster[0].enabled);
    assert_eq!(roster[0].watchdog_failures, 2);
    assert_eq!(roster[0].watchdog_active_since_ms, None);

    let mut newer = schedule("newer");
    newer.watchdog_active_since_ms = Some(token + 10);
    save_schedules(&[newer]).unwrap();
    let enabled = enable_schedule_atomic("newer").unwrap();
    assert!(!enabled.applied, "an active attempt cannot be re-enabled");
    assert_eq!(
        enabled.schedules[0].watchdog_active_since_ms,
        Some(token + 10),
        "enable cannot erase evidence owned by an active attempt"
    );
    assert_eq!(
        recover_orphaned_watchdog_attempt("newer", token).unwrap(),
        OrphanAttemptRecovery::Stale
    );
    let newer = load_schedules().pop().unwrap();
    assert!(newer.enabled);
    assert_eq!(newer.watchdog_active_since_ms, Some(token + 10));

    assert!(
        !persist_watchdog_state_for_attempt("newer", token, 0, None, None, None, None).unwrap()
    );
    assert_eq!(
        load_schedules()[0].watchdog_active_since_ms,
        Some(token + 10)
    );
    assert!(
        persist_watchdog_state_for_attempt("newer", token + 10, 0, None, None, None, None,)
            .unwrap()
    );
    assert_eq!(
        recover_orphaned_watchdog_attempt("newer", token + 10).unwrap(),
        OrphanAttemptRecovery::Stale
    );
    assert!(load_schedules()[0].enabled);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn atomic_roster_mutations_preserve_concurrent_additions() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    save_schedules(&[schedule("base")]).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for id in ["left", "right"] {
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let mut job = schedule(id);
            job.last_fired_ms = None;
            add_schedule_atomic(job).unwrap().applied
        }));
    }
    barrier.wait();
    assert!(threads.into_iter().all(|thread| thread.join().unwrap()));

    let disabled = disable_schedule_atomic("base").unwrap();
    assert!(disabled.applied);
    assert_eq!(disabled.schedules.len(), 3);
    assert!(disabled
        .schedules
        .iter()
        .all(|job| job.last_fired_ms.is_some()));
    assert!(
        !disabled
            .schedules
            .iter()
            .find(|j| j.id == "base")
            .unwrap()
            .enabled
    );
    let removed = remove_schedule_atomic("left").unwrap();
    assert!(removed.applied);
    assert!(removed.schedules.iter().any(|job| job.id == "right"));
    let enabled = enable_schedule_atomic("base").unwrap();
    assert!(enabled.applied);
    assert!(
        enabled
            .schedules
            .iter()
            .find(|j| j.id == "base")
            .unwrap()
            .enabled
    );
    assert!(!add_schedule_atomic(schedule("right")).unwrap().applied);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn persisted_watchdog_cas_updates_only_watchdog_fields() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let mut job = schedule("state1");
    job.prompt = "keep this prompt".into();
    job.last_fired_ms = Some(999_000);
    let expected = job.clone();
    save_schedules(&[job]).unwrap();

    assert!(persist_idle_watchdog_state_if_matches(
        &expected,
        3,
        Some(1_000_000),
        Some(1_005_000),
        Some(false),
    )
    .unwrap());
    let job = load_schedules().pop().unwrap();
    assert_eq!(job.prompt, "keep this prompt");
    assert_eq!(job.last_fired_ms, Some(999_000));
    assert!(!job.enabled);
    assert_eq!(job.watchdog_failures, 3);
    assert_eq!(job.watchdog_retry_at_ms, Some(1_005_000));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn stale_idle_snapshot_cannot_overwrite_an_active_to_idle_aba() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    save_schedules(&[schedule("idle-aba")]).unwrap();
    let stale_idle = load_schedules().pop().unwrap();

    let lease = try_begin_watchdog_attempt("idle-aba")
        .unwrap()
        .expect("attempt should be claimed");
    let retry_at_ms = epoch_ms_now().saturating_add(30_000);
    assert!(persist_watchdog_state_for_attempt(
        "idle-aba",
        lease.active_token_ms(),
        1,
        Some(retry_at_ms - 1_000),
        Some(retry_at_ms),
        None,
        None,
    )
    .unwrap());
    drop(lease);

    assert!(!persist_idle_watchdog_state_if_matches(
        &stale_idle,
        9,
        Some(retry_at_ms + 1_000),
        None,
        Some(false),
    )
    .unwrap());
    let persisted = load_schedules().pop().unwrap();
    assert!(persisted.enabled);
    assert_eq!(persisted.watchdog_failures, 1);
    assert_eq!(persisted.watchdog_retry_at_ms, Some(retry_at_ms));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn active_schedule_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    save_schedules(&[schedule("active-remove")]).unwrap();
    let lease = try_begin_watchdog_attempt("active-remove")
        .unwrap()
        .expect("attempt should be claimed");

    let update = remove_schedule_atomic("active-remove").unwrap();
    assert!(!update.applied);
    assert_eq!(update.schedules.len(), 1);
    assert_eq!(
        update.schedules[0].watchdog_active_since_ms,
        Some(lease.active_token_ms())
    );
    assert!(persist_watchdog_state_for_attempt(
        "active-remove",
        lease.active_token_ms(),
        0,
        None,
        None,
        None,
        None,
    )
    .unwrap());
    drop(lease);
    assert!(remove_schedule_atomic("active-remove").unwrap().applied);
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn terminal_conflict_fallback_classifies_and_quarantines() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    assert_eq!(
        quarantine_watchdog_terminal_conflict("missing", 7).unwrap(),
        WatchdogTerminalConflict::Missing
    );

    save_schedules(&[schedule("terminal-conflict")]).unwrap();
    let lease = try_begin_watchdog_attempt("terminal-conflict")
        .unwrap()
        .expect("attempt should be claimed");
    assert_eq!(
        quarantine_watchdog_terminal_conflict("terminal-conflict", lease.active_token_ms())
            .unwrap(),
        WatchdogTerminalConflict::StillOwned
    );

    assert!(clear_watchdog_attempt_if("terminal-conflict", lease.active_token_ms()).unwrap());
    assert_eq!(
        quarantine_watchdog_terminal_conflict("terminal-conflict", lease.active_token_ms())
            .unwrap(),
        WatchdogTerminalConflict::SafeToRelease {
            persisted_active_token_ms: None,
            newly_disabled: true,
        }
    );
    let quarantined = load_schedules().pop().unwrap();
    assert!(!quarantined.enabled);
    assert_eq!(quarantined.watchdog_active_since_ms, None);
    drop(lease);

    let mut mismatched = schedule("token-conflict");
    mismatched.watchdog_active_since_ms = Some(99);
    save_schedules(&[mismatched]).unwrap();
    assert_eq!(
        quarantine_watchdog_terminal_conflict("token-conflict", 98).unwrap(),
        WatchdogTerminalConflict::SafeToRelease {
            persisted_active_token_ms: Some(99),
            newly_disabled: true,
        }
    );
    let quarantined = load_schedules().pop().unwrap();
    assert!(!quarantined.enabled);
    assert_eq!(quarantined.watchdog_active_since_ms, Some(99));
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn load_schedules_sanitizes_invalid_specs_and_prompts() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let raw = serde_json::json!([
        {
            "id": "good",
            "spec": { "kind": "interval", "secs": 30 },
            "prompt": "valid prompt",
            "enabled": true,
            "lastFiredMs": 1000
        },
        {
            "id": "bad_time",
            "spec": { "kind": "daily", "hour": 99, "minute": 0 },
            "prompt": "broken",
            "enabled": true,
            "lastFiredMs": null
        },
        {
            "id": "bad_prompt",
            "spec": { "kind": "daily", "hour": 10, "minute": 0 },
            "prompt": " /compact",
            "enabled": true,
            "lastFiredMs": null
        },
        {
            "id": "bad_interval",
            "spec": { "kind": "interval", "secs": 1 },
            "prompt": "too fast",
            "enabled": true,
            "lastFiredMs": null
        }
    ]);
    std::fs::write(
        dir.path().join("schedules.json"),
        serde_json::to_vec(&raw).unwrap(),
    )
    .unwrap();
    let loaded = load_schedules();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "good");
    std::env::remove_var("ZODE_CONFIG_DIR");
}

#[test]
#[serial_test::serial]
fn load_schedules_drops_empty_and_duplicate_ids() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ZODE_CONFIG_DIR", dir.path());
    let mut empty = schedule("");
    empty.prompt = "empty id".into();
    let mut whitespace = schedule("   ");
    whitespace.prompt = "whitespace id".into();
    let mut first = schedule("same");
    first.prompt = "first wins".into();
    let mut duplicate = schedule("same");
    duplicate.prompt = "second loses".into();
    let unique = schedule("unique");
    std::fs::write(
        dir.path().join("schedules.json"),
        serde_json::to_vec(&[empty, whitespace, first, duplicate, unique]).unwrap(),
    )
    .unwrap();

    let loaded = load_schedules();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].id, "same");
    assert_eq!(loaded[0].prompt, "first wins");
    assert_eq!(loaded[1].id, "unique");
    std::env::remove_var("ZODE_CONFIG_DIR");
}
