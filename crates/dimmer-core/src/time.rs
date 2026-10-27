//! Clock helpers.
//!
//! The original application writes a local timestamp into `LastRun`.  The core
//! crate has no platform timezone dependency, so it produces a UTC timestamp in
//! the same human-readable shape.  The GTK front-end replaces it with local time
//! whenever it saves.

use std::time::{SystemTime, UNIX_EPOCH};

/// Formats the current UTC time as `YYYY-MM-DD HH:MM:SS`.
#[must_use]
pub fn now_string() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |delta| delta.as_secs());
    format_unix(seconds)
}

/// Formats a Unix timestamp as `YYYY-MM-DD HH:MM:SS` (UTC).
#[must_use]
pub fn format_unix(seconds: u64) -> String {
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = secs_of_day % 3_600 / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Converts days since 1970-01-01 to a civil date.
///
/// Implementation of Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, u32::try_from(m).unwrap_or(1), u32::try_from(d).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::field_reassign_with_default
    )]
    use super::*;

    #[test]
    fn formats_epoch() {
        assert_eq!(format_unix(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn formats_known_date() {
        assert_eq!(format_unix(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn handles_leap_day() {
        assert_eq!(format_unix(1_709_164_800), "2024-02-29 00:00:00");
    }
}
