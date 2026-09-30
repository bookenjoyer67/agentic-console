//! ISO-8601 time helpers with no external date crate.
//!
//! The journals and the config carry UTC timestamps such as
//! `2026-09-29T18:35:23.556231+00:00`. The console shows the age of every reading, so it has to turn
//! those strings into a `SystemTime` and format a duration back out. That is all this module does.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's civil algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The inverse of [`days_from_civil`].
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parse_i64(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

/// Split the seconds remainder (with an optional fraction) from a `hh:mm:ss` field.
fn split_seconds(text: &str) -> Option<(i64, u32)> {
    match text.split_once('.') {
        Some((whole, frac)) => {
            let mut nanos = String::from(frac);
            nanos.truncate(9);
            while nanos.len() < 9 {
                nanos.push('0');
            }
            Some((parse_i64(whole)?, nanos.parse::<u32>().ok()?))
        }
        None => Some((parse_i64(text)?, 0)),
    }
}

/// Split a trailing UTC offset off a time field, returning the time text and the offset in seconds.
fn split_offset(rest: &str) -> (&str, i64) {
    if let Some(stripped) = rest.strip_suffix('Z') {
        return (stripped, 0);
    }
    // The time field itself carries no '-' or '+', so the first one found starts the offset.
    if let Some(index) = rest.rfind(['+', '-']) {
        let (time, offset) = rest.split_at(index);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let body = &offset[1..];
        let mut parts = body.split(':');
        let hours = parts.next().and_then(parse_i64).unwrap_or(0);
        let minutes = parts.next().and_then(parse_i64).unwrap_or(0);
        return (time, sign * (hours * 3600 + minutes * 60));
    }
    (rest, 0)
}

/// Parse an ISO-8601 timestamp into a `SystemTime`, treating a missing offset as UTC.
pub fn parse_iso8601(text: &str) -> Option<SystemTime> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut date_parts = date.split('-');
    let year = parse_i64(date_parts.next()?)?;
    let month = parse_i64(date_parts.next()?)?;
    let day = parse_i64(date_parts.next()?)?;
    let (time, offset) = split_offset(rest);
    let mut time_parts = time.split(':');
    let hour = parse_i64(time_parts.next()?)?;
    let minute = parse_i64(time_parts.next()?)?;
    let (second, nanos) = split_seconds(time_parts.next().unwrap_or("0"))?;
    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    let nanos = Duration::new(0, nanos);
    if seconds >= 0 {
        UNIX_EPOCH
            .checked_add(Duration::from_secs(seconds as u64))?
            .checked_add(nanos)
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs((-seconds) as u64))?
            .checked_add(nanos)
    }
}

/// Whole seconds since the Unix epoch, for a `SystemTime` at or after it.
pub fn unix_secs(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_secs() as i64,
        Err(error) => -(error.duration().as_secs() as i64),
    }
}

/// Format a `SystemTime` as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_utc(time: SystemTime) -> String {
    let secs = unix_secs(time);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Format the age of a reading as `12s`, `1m04s` or `3h20m`; a future timestamp reads as `0s`.
pub fn format_age(then: SystemTime, now: SystemTime) -> String {
    let delta = match now.duration_since(then) {
        Ok(delta) => delta,
        Err(_) => return "0s".to_string(),
    };
    let secs = delta.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d{:02}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

/// The age of a reading, ready to paste into a UI line.
pub fn age_text(then: SystemTime, now: SystemTime) -> String {
    format!("age {}", format_age(then, now))
}
