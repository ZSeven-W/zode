use super::*;

fn config() -> BackgroundWatchdogConfig {
    BackgroundWatchdogConfig {
        enabled: Some(true),
        inactivity_timeout_secs: Some(5),
        max_runtime_secs: Some(30),
        abort_grace_secs: Some(2),
        max_retries: Some(2),
        initial_backoff_secs: Some(3),
        max_backoff_secs: Some(5),
    }
}

fn activity() -> TurnActivity {
    agent::abort::AbortController::new().activity()
}

#[test]
fn heartbeat_defers_inactivity_but_runtime_cap_still_wins() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Loop(1);
    let pulse = watchdog
        .start(job, 7, 9, "work".into(), start, activity())
        .expect("watchdog enabled");
    pulse.activity(start + Duration::from_secs(4));
    assert!(watchdog.poll(start + Duration::from_secs(8)).is_empty());
    assert!(matches!(
        watchdog.poll(start + Duration::from_secs(30)).as_slice(),
        [WatchdogAction::Abort {
            kind: TimeoutKind::Runtime,
            ..
        }]
    ));
}

#[test]
fn timeout_abort_grace_hard_stop_requires_manual_review() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("ab12".into());
    watchdog.start(job.clone(), 2, 4, "check".into(), start, activity());
    assert!(matches!(
        watchdog.poll(start + Duration::from_secs(6)).as_slice(),
        [WatchdogAction::Abort {
            kind: TimeoutKind::Inactive,
            ..
        }]
    ));
    assert!(watchdog.poll(start + Duration::from_secs(7)).is_empty());
    let actions = watchdog.poll(start + Duration::from_secs(8));
    let WatchdogAction::HardStop { failure, .. } = &actions[0] else {
        panic!("expected hard stop");
    };
    assert!(matches!(
        failure.recovery,
        Recovery::ManualReview { failures: 1 }
    ));
    assert!(watchdog
        .due_retries(start + Duration::from_secs(99))
        .is_empty());
}

#[test]
fn provider_failures_retry_then_exhaust_and_success_resets_counter() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Loop(3);
    for failure_number in 1..=3 {
        let turn = failure_number as u64;
        watchdog.start(job.clone(), 1, turn, "run".into(), start, activity());
        let failure = watchdog
            .finish(1, turn, &Err("boom".into()), start)
            .expect("watched failure");
        if failure_number <= 2 {
            assert!(matches!(
                failure.recovery,
                Recovery::RetryScheduled { attempt, .. } if attempt == failure_number
            ));
        } else {
            assert!(matches!(
                failure.recovery,
                Recovery::Exhausted { failures: 3 }
            ));
        }
    }

    watchdog.cancel_job(&job);
    watchdog.start(job.clone(), 1, 10, "run".into(), start, activity());
    assert!(watchdog.finish(1, 10, &Ok(()), start).is_none());
    watchdog.start(job, 1, 11, "run".into(), start, activity());
    let failure = watchdog
        .finish(1, 11, &Err("again".into()), start)
        .expect("failure after reset");
    assert!(matches!(
        failure.recovery,
        Recovery::RetryScheduled { attempt: 1, .. }
    ));
}

#[test]
fn manual_cancel_never_schedules_recovery() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Loop(8);
    watchdog.start(job.clone(), 4, 2, "run".into(), start, activity());
    watchdog.cancel_turn(4, 2, start);
    assert!(
        watchdog.job_is_occupied(&job),
        "the run stays supervised while cooperative cancellation drains"
    );
    assert!(watchdog
        .finish(4, 2, &Err("aborted".into()), start)
        .is_none());
    assert!(!watchdog.job_is_occupied(&job));
    assert!(watchdog
        .due_retries(start + Duration::from_secs(99))
        .is_empty());
}

#[test]
fn manual_cancel_gets_the_same_bounded_hard_stop_without_retry() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("cancel1".into());
    watchdog.start(job.clone(), 4, 3, "run".into(), start, activity());
    watchdog.cancel_turn(4, 3, start);
    assert!(watchdog.poll(start + Duration::from_secs(1)).is_empty());
    assert!(matches!(
        watchdog.poll(start + Duration::from_secs(2)).as_slice(),
        [WatchdogAction::ForceCancel {
            tab_id: 4,
            turn_id: 3,
            job: forced_job,
            failure: None,
        }] if forced_job == &job
    ));
    assert!(
        watchdog.job_is_occupied(&job),
        "the force-stop waiter retains canonical completion context"
    );
    watchdog.cancel_job(&job);
    assert!(!watchdog.job_is_occupied(&job));
    assert!(watchdog
        .due_retries(start + Duration::from_secs(99))
        .is_empty());
}

#[test]
fn force_cancel_rechecks_side_effect_risk_before_canonical_recovery() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("late-risk".into());
    let source = activity();
    watchdog.start(job.clone(), 4, 30, "run".into(), start, source.clone());
    watchdog.cancel_turn(4, 30, start);
    assert!(matches!(
        watchdog.poll(start + Duration::from_secs(2)).as_slice(),
        [WatchdogAction::ForceCancel { failure: None, .. }]
    ));

    source.mark_side_effect_risk();
    let failure = watchdog
        .finish(
            4,
            30,
            &Err("provider terminal raced the hard stop".into()),
            start + Duration::from_secs(3),
        )
        .expect("late risk must fail closed");
    assert!(matches!(
        failure.recovery,
        Recovery::ManualReview { failures: 1 }
    ));
    assert!(watchdog
        .due_retries(start + Duration::from_secs(99))
        .is_empty());
}

#[test]
fn manual_cancel_with_possible_side_effects_requires_review() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("cancel-risk".into());
    let source = activity();
    source.mark_side_effect_risk();
    watchdog.start(job.clone(), 4, 4, "run".into(), start, source);
    watchdog.cancel_turn(4, 4, start);

    let failure = watchdog
        .finish(4, 4, &Err("aborted".into()), start)
        .expect("unknown mutation state must fail closed");
    assert_eq!(failure.job, job);
    assert!(matches!(
        failure.recovery,
        Recovery::ManualReview { failures: 1 }
    ));
}

#[test]
fn successful_turn_with_detached_work_stops_recurrence() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("detached".into());
    let source = activity();
    source.mark_unresolved_external_work();
    watchdog.start(job.clone(), 5, 1, "run".into(), start, source);

    let failure = watchdog
        .finish(5, 1, &Ok(()), start)
        .expect("detached work must not be followed by automatic recurrence");
    assert_eq!(failure.job, job);
    assert!(matches!(
        failure.recovery,
        Recovery::ManualReview { failures: 1 }
    ));
}

#[test]
fn terminal_during_abort_grace_uses_canonical_timeout_outcome_once() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("term1".into());
    let pulse = watchdog
        .start(job, 2, 9, "check".into(), start, activity())
        .unwrap();
    assert!(matches!(
        watchdog.poll(start + Duration::from_secs(6)).as_slice(),
        [WatchdogAction::Abort { .. }]
    ));
    pulse.terminal();
    assert!(watchdog.poll(start + Duration::from_secs(99)).is_empty());
    let failure = watchdog
        .finish(2, 9, &Ok(()), start + Duration::from_secs(99))
        .expect("a timed-out turn cannot become success during drain");
    assert!(matches!(
        failure.cause,
        FailureCause::Timeout(TimeoutKind::Inactive)
    ));
    assert!(matches!(
        failure.recovery,
        Recovery::RetryScheduled { attempt: 1, .. }
    ));
    assert!(watchdog.finish(2, 9, &Ok(()), start).is_none());
}

#[test]
fn side_effect_risk_blocks_replay_even_before_retry_budget_is_exhausted() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Loop(12);
    let activity = activity();
    watchdog.start(job, 1, 4, "publish".into(), start, activity.clone());
    activity.mark_side_effect_risk();
    let failure = watchdog
        .finish(1, 4, &Err("connection lost".into()), start)
        .unwrap();
    assert!(matches!(
        failure.recovery,
        Recovery::ManualReview { failures: 1 }
    ));
    assert!(watchdog
        .due_retries(start + Duration::from_secs(99))
        .is_empty());
}

#[test]
fn zero_retry_budget_exhausts_on_first_safe_failure() {
    let start = Instant::now();
    let mut cfg = config();
    cfg.max_retries = Some(0);
    let mut watchdog = BackgroundWatchdog::new(cfg);
    watchdog.start(
        SchedJobRef::Loop(13),
        1,
        1,
        "read".into(),
        start,
        activity(),
    );
    let failure = watchdog.finish(1, 1, &Err("failed".into()), start).unwrap();
    assert!(matches!(
        failure.recovery,
        Recovery::Exhausted { failures: 1 }
    ));
}

#[test]
fn queued_start_timeout_uses_bounded_side_effect_free_recovery() {
    let start = Instant::now();
    let mut watchdog = BackgroundWatchdog::new(config());
    let job = SchedJobRef::Schedule("queued-timeout".into());

    let first = watchdog.fail_queued(job.clone(), 3, "sync".into(), start);
    assert!(matches!(first.cause, FailureCause::QueueStartTimeout));
    assert!(matches!(
        first.recovery,
        Recovery::RetryScheduled { attempt: 1, .. }
    ));
    assert!(watchdog.job_is_occupied(&job));

    let second = watchdog.fail_queued(
        job.clone(),
        3,
        "sync".into(),
        start + Duration::from_secs(1),
    );
    assert!(matches!(
        second.recovery,
        Recovery::RetryScheduled { attempt: 2, .. }
    ));
    assert_eq!(watchdog.queue_start_timeout(), Duration::from_secs(5));
}
