//! Tooltip text formatting (UI copy, not geometry). Pure functions only -
//! no clock reads (AGENTS.md 4.4): timestamps arrive from the host, and the
//! timezone offset comes with the call so the same function serves both the
//! UTC default (native adapter) and the local-time default (web adapter).

/// Format `epoch_ms` as "YYYY-MM-DD HH:MM" shifted by `tz_offset_minutes`
/// (e.g. +480 for UTC+8, -300 for UTC-5). Deterministic pure math.
pub fn format_tooltip_datetime(epoch_ms: i64, tz_offset_minutes: i32) -> String {
    let shifted = epoch_ms + tz_offset_minutes as i64 * 60_000;
    let days = shifted.div_euclid(86_400_000);
    let ms_of_day = shifted.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    let hh = ms_of_day / 3_600_000;
    let mm = (ms_of_day / 60_000) % 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// The adapters' default tooltip lines, titled: "作者：{creator}" then
/// "时间：{time}". Shared so native (UTC) and web (local timezone) show
/// identical copy - the adapter only injects the timezone offset. Hosts
/// overriding `set_tooltip_formatter` choose their own copy.
pub fn default_tooltip_lines(ann: &rofd_dom::Annotation, tz_offset_minutes: i32) -> Vec<String> {
    vec![
        format!("作者：{}", ann.creator),
        format!(
            "时间：{}",
            format_tooltip_datetime(ann.created, tz_offset_minutes)
        ),
    ]
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_utc() {
        assert_eq!(format_tooltip_datetime(0, 0), "1970-01-01 00:00");
    }

    #[test]
    fn spec_anchor_date() {
        // io 解析测试锚定 1_783_641_600_000 = 2026-07-10 00:00 UTC。
        assert_eq!(
            format_tooltip_datetime(1_783_641_600_000, 0),
            "2026-07-10 00:00"
        );
    }

    #[test]
    fn positive_offset_shifts_forward() {
        // UTC+8：同一时刻本地读数 08:00。
        assert_eq!(
            format_tooltip_datetime(1_783_641_600_000, 480),
            "2026-07-10 08:00"
        );
    }

    #[test]
    fn negative_offset_crosses_day_backwards() {
        // UTC-5：epoch 0 本地读数 1969-12-31 19:00。
        assert_eq!(format_tooltip_datetime(0, -300), "1969-12-31 19:00");
    }

    #[test]
    fn leap_day() {
        assert_eq!(
            format_tooltip_datetime(1_709_208_000_000, 0),
            "2024-02-29 12:00"
        );
    }

    #[test]
    fn year_boundary() {
        assert_eq!(
            format_tooltip_datetime(1_767_225_540_000, 0),
            "2025-12-31 23:59"
        );
    }

    #[test]
    fn negative_epoch_millis() {
        assert_eq!(format_tooltip_datetime(-1, 0), "1969-12-31 23:59");
    }

    fn ann(creator: &str, created: i64) -> rofd_dom::Annotation {
        rofd_dom::Annotation {
            id: rofd_dom::AnnotationId::from_int(1),
            kind: rofd_dom::AnnotationKind::Note,
            page: rofd_dom::PageId::new("P0"),
            creator: creator.into(),
            created,
            modified: created,
            reply_to: None,
            payload: rofd_dom::AnnotationPayload::Note {
                rect: rofd_dom::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: rofd_dom::Color::Rgb(0, 0, 0),
                content: String::new(),
                icon: rofd_dom::NoteIcon::Note,
            },
        }
    }

    #[test]
    fn default_tooltip_lines_are_titled() {
        let lines = default_tooltip_lines(&ann("flw", 1_783_641_600_000), 480);
        assert_eq!(lines, vec!["作者：flw", "时间：2026-07-10 08:00"]);
    }

    #[test]
    fn default_tooltip_lines_utc_offset_zero() {
        let lines = default_tooltip_lines(&ann("t", 0), 0);
        assert_eq!(lines, vec!["作者：t", "时间：1970-01-01 00:00"]);
    }
}
