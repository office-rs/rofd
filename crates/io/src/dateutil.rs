//! OFD LastModDate <-> i64 ms. chrono only formats/parses a caller-supplied
//! timestamp; it never reads a system clock (AGENTS.md §4.4).

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

const FMT_DATETIME: &str = "%Y-%m-%d %H:%M:%S";
const FMT_DATE: &str = "%Y-%m-%d";

/// Format i64 ms (UTC) as `yyyy-MM-dd HH:mm:ss` (real-world OFD producer convention).
pub fn format_last_mod_date(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|dt| dt.naive_utc().format(FMT_DATETIME).to_string())
        .unwrap_or_default()
}

/// Parse `yyyy-MM-dd HH:mm:ss` or `yyyy-MM-dd` (date-only -> midnight) to i64 ms.
/// Returns None on parse failure.
pub fn parse_last_mod_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, FMT_DATETIME) {
        return Some(dt.and_utc().timestamp_millis());
    }
    NaiveDate::parse_from_str(s, FMT_DATE)
        .ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ms_as_datetime() {
        // 2026-07-13 22:43:57 UTC = 1783982637000 ms
        assert_eq!(
            format_last_mod_date(1_783_982_637_000),
            "2026-07-13 22:43:57"
        );
    }

    #[test]
    fn parse_datetime_to_ms() {
        assert_eq!(
            parse_last_mod_date("2026-07-13 22:43:57"),
            Some(1_783_982_637_000)
        );
    }

    #[test]
    fn parse_date_only_to_midnight_ms() {
        assert_eq!(parse_last_mod_date("2026-07-13"), Some(1_783_900_800_000));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_last_mod_date("not a date"), None);
    }
}
