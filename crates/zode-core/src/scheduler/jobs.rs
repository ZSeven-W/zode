//! Pure job types for the `/loop` and `/schedule` scheduler.
//!
//! No I/O here: `ScheduleSpec::next_after` is a pure function of its inputs,
//! and `LoopJob`/`ScheduleJob` are plain data. The store (persistence) lands
//! in a later task; this module only defines the shapes and next-fire math.

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Recurrence rule for a persisted `/schedule` job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScheduleSpec {
    Daily {
        hour: u32,
        minute: u32,
    },
    Weekly {
        weekday: chrono::Weekday,
        hour: u32,
        minute: u32,
    },
    Interval {
        secs: u64,
    },
}

impl ScheduleSpec {
    /// Next fire time strictly after `now` (naive local time). Pure.
    pub fn next_after(
        &self,
        now: chrono::NaiveDateTime,
        last_fired: Option<chrono::NaiveDateTime>,
    ) -> chrono::NaiveDateTime {
        match self {
            Self::Daily { hour, minute } => {
                let today = now
                    .date()
                    .and_hms_opt(*hour, *minute, 0)
                    .expect("validated");
                if today > now {
                    today
                } else {
                    today + chrono::Duration::days(1)
                }
            }
            Self::Weekly {
                weekday,
                hour,
                minute,
            } => {
                let mut day = now.date();
                for _ in 0..8 {
                    let candidate = day.and_hms_opt(*hour, *minute, 0).expect("validated");
                    if day.weekday() == *weekday && candidate > now {
                        return candidate;
                    }
                    day += chrono::Duration::days(1);
                }
                unreachable!("a weekday recurs within 8 days")
            }
            Self::Interval { secs } => {
                let base = last_fired.unwrap_or(now);
                let next = base + chrono::Duration::seconds(*secs as i64);
                if next > now {
                    next
                } else {
                    now + chrono::Duration::seconds(*secs as i64)
                }
            }
        }
    }
}

/// A `/loop` job: re-runs `prompt` every `interval` until `max_runs` (or
/// forever, if `None`) or an explicit `/loop stop`. Not persisted — process
/// lifetime only, so no serde derive and `next_fire`/`interval` use
/// `std::time::Instant`/`Duration` rather than wall-clock types.
#[derive(Debug, Clone)]
pub struct LoopJob {
    pub id: u32,
    pub owner: u64,
    pub prompt: String,
    pub interval: Duration,
    pub next_fire: Instant,
    pub max_runs: Option<u32>,
    pub runs: u32,
}

/// A `/schedule` job: fires `prompt` according to `spec`, persisted across
/// process restarts (Task 7 owns the JSON store). `last_fired_ms` is a wall
/// clock timestamp (epoch milliseconds) so it survives serialization; the
/// scheduler converts it to a `NaiveDateTime` at `due()` time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleJob {
    pub id: String,
    pub spec: ScheduleSpec,
    pub prompt: String,
    pub enabled: bool,
    pub last_fired_ms: Option<u64>,
    /// Consecutive watchdog failures. Persisted so restarting zode cannot
    /// evade `backgroundWatchdog.maxRetries`; reset after a successful run or
    /// an explicit re-enable.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub watchdog_failures: u32,
    /// Wall-clock timestamp of the most recent watchdog-managed failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_last_failure_ms: Option<u64>,
    /// Next recovery deadline in epoch milliseconds. Keeping it in the same
    /// atomic schedule store makes an in-backoff restart resume rather than
    /// silently losing or immediately duplicating the recovery attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_retry_at_ms: Option<u64>,
    /// Persisted while a schedule turn owns execution. A leftover value on
    /// startup means the previous process died before a canonical terminal;
    /// replay is unsafe and the host disables the job for manual review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog_active_since_ms: Option<u64>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Weekday};

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    #[test]
    fn daily_next_after_rolls_to_tomorrow() {
        let spec = ScheduleSpec::Daily { hour: 9, minute: 0 };
        // Before 09:00 -> today 09:00; at/after -> tomorrow.
        assert_eq!(
            spec.next_after(dt(2026, 7, 18, 8, 0), None),
            dt(2026, 7, 18, 9, 0)
        );
        assert_eq!(
            spec.next_after(dt(2026, 7, 18, 9, 0), None),
            dt(2026, 7, 19, 9, 0)
        );
    }

    #[test]
    fn weekly_next_after_crosses_week() {
        let spec = ScheduleSpec::Weekly {
            weekday: Weekday::Mon,
            hour: 9,
            minute: 0,
        };
        // 2026-07-18 is a Saturday -> next Monday 2026-07-20.
        assert_eq!(
            spec.next_after(dt(2026, 7, 18, 10, 0), None),
            dt(2026, 7, 20, 9, 0)
        );
        // On Monday 09:00 exactly -> the following Monday.
        assert_eq!(
            spec.next_after(dt(2026, 7, 20, 9, 0), None),
            dt(2026, 7, 27, 9, 0)
        );
    }

    #[test]
    fn interval_next_after_prefers_last_fired() {
        let spec = ScheduleSpec::Interval { secs: 7200 };
        // Never fired -> now + interval; fired -> last + interval.
        assert_eq!(
            spec.next_after(dt(2026, 7, 18, 8, 0), None),
            dt(2026, 7, 18, 10, 0)
        );
        assert_eq!(
            spec.next_after(dt(2026, 7, 18, 8, 0), Some(dt(2026, 7, 18, 7, 0))),
            dt(2026, 7, 18, 9, 0)
        );
    }
}
