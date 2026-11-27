//! [`Timestamp`] text and `SystemTime`, without a date crate.
//!
//! The format is `core`'s: `"2026-11-25T17:19:07.841Z"`, UTC, always with milliseconds, so
//! text order is time order for years 0000 to 9999. Civil dates use Howard Hinnant's
//! days-from-civil algorithms, valid for every proleptic Gregorian date.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::Timestamp;

const NANOS_PER_MILLI: i128 = 1_000_000;
const SECS_PER_DAY: i64 = 86_400;

/// `t` in `core`'s format, truncated to the millisecond (towards the past).
pub fn to_ts(t: SystemTime) -> Timestamp {
    let nanos: i128 = match t.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_nanos() as i128,
        Err(before) => -(before.duration().as_nanos() as i128),
    };
    let millis = nanos.div_euclid(NANOS_PER_MILLI);
    let secs = millis.div_euclid(1000) as i64;
    let milli = millis.rem_euclid(1000);
    let (year, month, day) = civil_from_days(secs.div_euclid(SECS_PER_DAY));
    let of_day = secs.rem_euclid(SECS_PER_DAY);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// Reads `core`'s format back, and also GitHub's (`"2026-11-25T17:19:08Z"`, no fraction). A
/// fraction may have 1 to 9 digits. `None` for anything else, including impossible dates.
pub fn from_ts(ts: &str) -> Option<SystemTime> {
    let b = ts.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    if b[16] != b':' || b.last() != Some(&b'Z') {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let digits = ts.get(from..to)?;
        if digits.bytes().all(|c| c.is_ascii_digit()) {
            digits.parse().ok()
        } else {
            None
        }
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // `get`, not indexing: a multi-byte character could straddle byte 19.
    let nanos = match ts.get(19..ts.len() - 1)? {
        "" => 0,
        fraction => {
            let digits = fraction.strip_prefix('.')?;
            if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            format!("{digits:0<9}").parse::<u32>().ok()?
        }
    };
    let month = u32::try_from(month).ok().filter(|m| (1..=12).contains(m))?;
    let day = u32::try_from(day)
        .ok()
        .filter(|d| (1..=days_in_month(year, month)).contains(d))?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let secs =
        days_from_civil(year, month, day) * SECS_PER_DAY + hour * 3600 + minute * 60 + second;
    let whole = Duration::from_secs(secs.unsigned_abs());
    let at = if secs >= 0 {
        UNIX_EPOCH.checked_add(whole)?
    } else {
        UNIX_EPOCH.checked_sub(whole)?
    };
    at.checked_add(Duration::from_nanos(u64::from(nanos)))
}

/// The day a `YYYY-MM-DD` date names, as days since 1970-01-01. `None` for anything else,
/// including impossible dates such as 2026-02-30.
pub fn date_days(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u32> {
        let digits = date.get(from..to)?;
        digits
            .bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| digits.parse().ok())
            .flatten()
    };
    let (year, month, day) = (i64::from(num(0, 4)?), num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&month) || !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// `days` since 1970-01-01 as `YYYY-MM-DD`.
pub fn days_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The local day `t` falls on, as days since 1970-01-01, for a zone `utc_offset` seconds east
/// of UTC at that moment.
pub fn local_day(t: SystemTime, utc_offset: i32) -> i64 {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs_f64().ceil() as i64),
    };
    (secs + i64::from(utc_offset)).div_euclid(SECS_PER_DAY)
}

/// The moment local day `days` starts, for a zone `utc_offset` seconds east of UTC then.
pub fn local_day_start(days: i64, utc_offset: i32) -> SystemTime {
    let secs = days * SECS_PER_DAY - i64::from(utc_offset);
    let whole = Duration::from_secs(secs.unsigned_abs());
    if secs >= 0 {
        UNIX_EPOCH + whole
    } else {
        UNIX_EPOCH - whole
    }
}

/// The day of the week of `days` since 1970-01-01: 0 for Monday to 6 for Sunday.
pub fn weekday(days: i64) -> u32 {
    // 1970-01-01 was a Thursday.
    (days + 3).rem_euclid(7) as u32
}

/// The month and day of month of `days` since 1970-01-01, and its year.
pub fn days_civil(days: i64) -> (i64, u32, u32) {
    civil_from_days(days)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
