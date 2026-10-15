//! Collection day numbering with a rollover hour, exactly as Anki's v2/v3 scheduler with
//! `creationOffset` set (rslib/src/scheduler/timing.rs):
//!   days_elapsed = (local date of now − local date of crt) − (1 if today's rollover has not passed), min 0.
//! Offsets are minutes WEST of UTC (what JavaScript's `Date#getTimezoneOffset()` returns).

use crate::types::{CollectionDay, Millis, Seconds};

pub const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;

/// UTC offset in minutes west of UTC at instant `ms`, from the system time zone.
pub fn system_tz_offset(ms: Millis) -> i64 {
    use chrono::{Local, Offset, TimeZone};
    let secs = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as u32;
    match Local.timestamp_opt(secs, nanos) {
        chrono::LocalResult::Single(dt) => -(dt.offset().fix().local_minus_utc() as i64) / 60,
        chrono::LocalResult::Ambiguous(dt, _) => -(dt.offset().fix().local_minus_utc() as i64) / 60,
        chrono::LocalResult::None => 0,
    }
}

/// Days since 1970-01-01 of the local calendar date of `ms` at the given offset.
pub fn local_day_index(ms: Millis, offset_minutes_west: i64) -> i64 {
    (ms - offset_minutes_west * 60_000).div_euclid(DAY_MS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedTiming {
    /// "today" as a collection day number.
    pub days_elapsed: CollectionDay,
    /// When the next day starts (seconds since epoch).
    pub next_day_at: Seconds,
}

/// `crt`: collection creation (seconds); `creation_offset`: offset in force when it was created
/// (minutes west); `now` ms; `current_offset` minutes west now; `rollover_hour` 0–23.
pub fn sched_timing(crt: Seconds, creation_offset: i64, now: Millis, current_offset: i64, rollover_hour: i64) -> SchedTiming {
    let hour = rollover_hour.clamp(0, 23);
    let created_date = local_day_index(crt * 1000, creation_offset);
    let now_date = local_day_index(now, current_offset);
    let rollover_today = now_date * DAY_MS + hour * HOUR_MS + current_offset * 60_000;
    let rollover_passed = rollover_today <= now;
    let next_day_at_ms = if rollover_passed { rollover_today + DAY_MS } else { rollover_today };
    let days = now_date - created_date - if rollover_passed { 0 } else { 1 };
    SchedTiming { days_elapsed: days.max(0), next_day_at: next_day_at_ms.div_euclid(1000) }
}

/// Anki sets crt to the most recent local rollover time when creating a collection. Seconds.
pub fn initial_crt(now: Millis, offset: i64, rollover_hour: i64) -> Seconds {
    let local = now - offset * 60_000;
    let day = (local - rollover_hour * HOUR_MS).div_euclid(DAY_MS);
    let crt_local = day * DAY_MS + rollover_hour * HOUR_MS;
    (crt_local + offset * 60_000).div_euclid(1000)
}

#[cfg(test)]
pub(crate) fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Millis {
    use chrono::{NaiveDate, Timelike};
    let dt = NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap();
    let _ = dt.hour();
    dt.and_utc().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Collection created 2026-01-01 04:00 UTC.
    fn crt() -> Seconds {
        utc(2026, 1, 1, 4, 0, 0) / 1000
    }

    #[test]
    fn counts_calendar_days_minus_one_before_the_rollover_hour() {
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 1, 4, 0, 0), 0, 4).days_elapsed, 0);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 1, 23, 59, 0), 0, 4).days_elapsed, 0);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 2, 3, 59, 59), 0, 4).days_elapsed, 0);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 2, 4, 0, 0), 0, 4).days_elapsed, 1);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 31, 12, 0, 0), 0, 4).days_elapsed, 30);
    }

    #[test]
    fn never_negative_and_reports_next_day_start() {
        let t = sched_timing(crt(), 0, utc(2026, 1, 1, 2, 0, 0), 0, 4);
        assert_eq!(t.days_elapsed, 0);
        assert_eq!(t.next_day_at, utc(2026, 1, 1, 4, 0, 0) / 1000);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 5, 10, 0, 0), 0, 4).next_day_at, utc(2026, 1, 6, 4, 0, 0) / 1000);
    }

    #[test]
    fn uses_local_dates_utc_plus_10() {
        let crt_local = utc(2025, 12, 31, 18, 0, 0) / 1000;
        assert_eq!(sched_timing(crt_local, -600, utc(2026, 1, 1, 17, 59, 0), -600, 4).days_elapsed, 0);
        assert_eq!(sched_timing(crt_local, -600, utc(2026, 1, 1, 18, 0, 0), -600, 4).days_elapsed, 1);
    }

    #[test]
    fn later_rollover_hour_does_not_shift_creation_day() {
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 3, 5, 0, 0), 0, 6).days_elapsed, 1);
        assert_eq!(sched_timing(crt(), 0, utc(2026, 1, 3, 6, 0, 0), 0, 6).days_elapsed, 2);
    }

    #[test]
    fn initial_crt_is_the_most_recent_rollover() {
        assert_eq!(initial_crt(utc(2026, 3, 10, 12, 0, 0), 0, 4), utc(2026, 3, 10, 4, 0, 0) / 1000);
        assert_eq!(initial_crt(utc(2026, 3, 10, 2, 0, 0), 0, 4), utc(2026, 3, 9, 4, 0, 0) / 1000);
        assert_eq!(initial_crt(utc(2026, 3, 10, 4, 0, 0), 0, 4), utc(2026, 3, 10, 4, 0, 0) / 1000);
    }

    #[test]
    fn system_offset_is_finite() {
        let off = system_tz_offset(utc(2026, 6, 1, 12, 0, 0));
        assert!((-840..=840).contains(&off));
    }
}
