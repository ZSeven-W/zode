//! Human-readable duration formatting shared by every timing surface
//! (chat tool rows, turn footer, /tasks panel).

/// `<60s` → one-decimal seconds; `60s..1h` → `Nm SSs`; `≥1h` → `Nh MMm`.
pub fn format_duration_ms(ms: u64) -> String {
    if ms < 60_000 {
        // Round to one decimal; 999ms displays as 1.0s by design.
        return format!("{:.1}s", ms as f64 / 1000.0);
    }
    let total_secs = ms / 1000;
    if total_secs < 3600 {
        return format!("{}m {:02}s", total_secs / 60, total_secs % 60);
    }
    format!("{}h {:02}m", total_secs / 3600, (total_secs % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_minute_uses_one_decimal_second() {
        assert_eq!(format_duration_ms(0), "0.0s");
        assert_eq!(format_duration_ms(400), "0.4s");
        assert_eq!(format_duration_ms(999), "1.0s");
        assert_eq!(format_duration_ms(1234), "1.2s");
        assert_eq!(format_duration_ms(59_940), "59.9s");
    }

    #[test]
    fn minutes_zero_pad_seconds() {
        assert_eq!(format_duration_ms(60_000), "1m 00s");
        assert_eq!(format_duration_ms(83_000), "1m 23s");
        assert_eq!(format_duration_ms(125_000), "2m 05s");
    }

    #[test]
    fn hours_zero_pad_minutes() {
        assert_eq!(format_duration_ms(3_600_000), "1h 00m");
        assert_eq!(format_duration_ms(3_720_000), "1h 02m");
    }
}
