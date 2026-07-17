//! Relative-duration formatter for the M2 sub-agent panel's completed-row
//! timestamps (`subagents_panel.rs`) - "N 分钟" under an hour, "N 小时"
//! under a day, else "N 天". Split out from `timestamp.rs` (which formats
//! an absolute bubble timestamp relative to "now" with weekday/date
//! fallbacks) into its own module, rather than added there, purely so two
//! concurrently active changes don't collide on the same file - see
//! `docs/proposals/subagent-panel-m2.md`. Like `timestamp.rs`, this is
//! UTC-based rather than adjusted to the user's local offset - a known
//! simplification, not an oversight, matching the codebase's existing
//! epoch-ms handling.

const MS_PER_MINUTE: i64 = 60_000;
const MS_PER_HOUR: i64 = 60 * MS_PER_MINUTE;
const MS_PER_DAY: i64 = 24 * MS_PER_HOUR;

/// Wall-clock "now" in epoch milliseconds - a fresh read every frame is
/// fine since this only feeds a coarse label, not layout (same rationale
/// as `timestamp.rs`'s `now_ms`).
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// Formats how long ago `timestamp_ms` was relative to `now_ms`, both epoch
/// milliseconds: under an hour -> "N 分钟" (at least "1 分钟", never "0");
/// under a day -> "N 小时"; otherwise -> "N 天". A `timestamp_ms` at or
/// after `now_ms` (clock skew, or a completion stamped this same tick)
/// clamps to zero elapsed rather than going negative.
pub(crate) fn relative_duration_label(timestamp_ms: i64, now_ms: i64) -> String {
    let elapsed_ms = (now_ms - timestamp_ms).max(0);
    if elapsed_ms < MS_PER_HOUR {
        format!("{} 分钟", (elapsed_ms / MS_PER_MINUTE).max(1))
    } else if elapsed_ms < MS_PER_DAY {
        format!("{} 小时", elapsed_ms / MS_PER_HOUR)
    } else {
        format!("{} 天", elapsed_ms / MS_PER_DAY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_a_minute_still_reads_as_one_minute() {
        assert_eq!(relative_duration_label(1_000, 1_500), "1 分钟");
    }

    #[test]
    fn minutes_tier_covers_up_to_just_under_an_hour() {
        let now = 10 * MS_PER_HOUR;
        assert_eq!(
            relative_duration_label(now - 5 * MS_PER_MINUTE, now),
            "5 分钟"
        );
        assert_eq!(
            relative_duration_label(now - (MS_PER_HOUR - 1_000), now),
            "59 分钟"
        );
    }

    #[test]
    fn hours_tier_covers_up_to_just_under_a_day() {
        let now = 5 * MS_PER_DAY;
        assert_eq!(relative_duration_label(now - MS_PER_HOUR, now), "1 小时");
        assert_eq!(
            relative_duration_label(now - (MS_PER_DAY - 1_000), now),
            "23 小时"
        );
    }

    #[test]
    fn days_tier_covers_a_day_or_more() {
        let now = 10 * MS_PER_DAY;
        assert_eq!(relative_duration_label(now - MS_PER_DAY, now), "1 天");
        assert_eq!(relative_duration_label(now - 3 * MS_PER_DAY, now), "3 天");
    }

    #[test]
    fn future_or_equal_timestamps_clamp_to_the_shortest_label() {
        assert_eq!(relative_duration_label(1_000, 1_000), "1 分钟");
        assert_eq!(relative_duration_label(2_000, 1_000), "1 分钟");
    }
}
