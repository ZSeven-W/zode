//! Calendar-math helpers for the compact timestamp label shown on a
//! transcript message's hover-revealed action row.
//!
//! There is no date/time crate anywhere in this workspace (see the other
//! desktop crates, which only ever format *relative* "N 天/N 小时" age
//! labels from raw epoch milliseconds - `project_sidebar/paint.rs`'s
//! `relative_age_label`). Adding one just for this label would be the only
//! place in the app pulling in calendar/timezone logic, so this computes
//! the Gregorian calendar date and weekday directly from epoch milliseconds
//! with the same well-known integer algorithm libc's `gmtime` uses (Howard
//! Hinnant's `civil_from_days`). Like the rest of the app, this is UTC-based
//! rather than adjusted to the user's local offset - a known simplification,
//! not an oversight, matching the codebase's existing epoch-ms handling.

const MS_PER_DAY: i64 = 86_400_000;

/// Wall-clock "now" for the same UTC-based, timezone-agnostic comparison
/// `format_timestamp` uses. Called directly from paint, same as
/// `project_sidebar/paint.rs`'s `current_time_ms` - a fresh read every frame
/// is fine since this only feeds a coarse day/hour label, not layout.
pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// Formats a message timestamp relative to `now_ms`, both epoch milliseconds:
/// same UTC day -> "HH:MM"; within the previous 6 days -> "星期X HH:MM";
/// older -> "M月D日 HH:MM".
pub(super) fn format_timestamp(timestamp_ms: i64, now_ms: i64) -> String {
    let days_ago = now_ms.div_euclid(MS_PER_DAY) - timestamp_ms.div_euclid(MS_PER_DAY);
    let time = format_time_of_day(timestamp_ms);
    match days_ago {
        0 => time,
        1..=6 => format!("{} {time}", weekday_label(timestamp_ms)),
        _ => {
            let (_, month, day) = civil_from_days(timestamp_ms.div_euclid(MS_PER_DAY));
            format!("{month}月{day}日 {time}")
        }
    }
}

fn format_time_of_day(timestamp_ms: i64) -> String {
    let ms_of_day = timestamp_ms.rem_euclid(MS_PER_DAY);
    let hour = ms_of_day / 3_600_000;
    let minute = (ms_of_day % 3_600_000) / 60_000;
    format!("{hour:02}:{minute:02}")
}

fn weekday_label(timestamp_ms: i64) -> &'static str {
    const LABELS: [&str; 7] = [
        "星期日",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
    ];
    let days = timestamp_ms.div_euclid(MS_PER_DAY);
    // Epoch day 0 (1970-01-01) was a Thursday, index 4 below.
    let index = (days.rem_euclid(7) + 4).rem_euclid(7) as usize;
    LABELS[index]
}

/// Howard Hinnant's `civil_from_days`: converts a day count since the Unix
/// epoch into a proleptic-Gregorian (year, month, day), valid for any `i64`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(days: i64, hour: i64, minute: i64) -> i64 {
        days * MS_PER_DAY + hour * 3_600_000 + minute * 60_000
    }

    #[test]
    fn civil_from_days_matches_known_epoch_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2024 is a leap year; day 19_783 is 2024-03-01.
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn weekday_label_matches_known_epoch_day() {
        assert_eq!(weekday_label(0), "星期四");
        assert_eq!(weekday_label(ms(1, 0, 0)), "星期五");
        assert_eq!(weekday_label(ms(-1, 0, 0)), "星期三");
    }

    #[test]
    fn same_day_uses_bare_time() {
        let now = ms(10, 18, 30);
        assert_eq!(format_timestamp(ms(10, 9, 5), now), "09:05");
    }

    #[test]
    fn earlier_this_week_uses_weekday_and_time() {
        let now = ms(10, 18, 30);
        assert_eq!(format_timestamp(ms(6, 9, 5), now), "星期三 09:05");
    }

    #[test]
    fn older_uses_month_and_day() {
        let now = ms(10, 18, 30);
        assert_eq!(format_timestamp(ms(0, 9, 5), now), "1月1日 09:05");
    }
}
