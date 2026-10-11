//! `SchedClock`: the collection clock in the units the scheduler uses (recall
//! `src/core/collection/sched-context.ts` `SchedClock`). Phase 2's `Collection` builds one from
//! its settings (`crt`, rollover hour, learn-ahead minutes) and the current time; everything in
//! this module takes it by reference. Units: ms for `now_ms`, seconds for `crt`/`next_day_at`,
//! collection days for `today`.

use super::convert::{collection_day, day_start_ms};
use crate::types::{CollectionDay, Seconds};

/// Anki's default learn-ahead limit, seconds (20 minutes).
pub const DEFAULT_LEARN_AHEAD_SECS: i64 = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedClock {
    /// Now, ms epoch.
    pub now_ms: i64,
    /// Collection day of now.
    pub today: CollectionDay,
    /// Collection creation, seconds (start of day 0 at the rollover hour).
    pub crt: Seconds,
    /// "Next day starts at" hour, 0..23 (Anki default 4).
    pub rollover_hour: i64,
    /// Start of the next collection day, seconds.
    pub next_day_at: Seconds,
    /// Learn-ahead limit, seconds.
    pub learn_ahead_secs: i64,
}

impl SchedClock {
    /// The clock at `now_ms` for a collection created at `crt` with the given rollover hour.
    pub fn at(now_ms: i64, crt: Seconds, rollover_hour: i64, learn_ahead_secs: i64) -> SchedClock {
        let today = collection_day(now_ms, crt, rollover_hour);
        let next_day_at = day_start_ms(today + 1, crt, rollover_hour).div_euclid(1000);
        SchedClock { now_ms, today, crt, rollover_hour, next_day_at, learn_ahead_secs: learn_ahead_secs.max(0) }
    }

    /// Now, whole seconds.
    pub fn now_secs(&self) -> Seconds {
        self.now_ms.div_euclid(1000)
    }

    /// Start of collection day `day`, seconds (rollover-aligned; DST shifts ignored).
    pub fn day_start_secs(&self, day: CollectionDay) -> Seconds {
        self.next_day_at - (self.today + 1 - day) * 86_400
    }

    /// Collection day of an instant (ms), relative to the clock's day boundaries.
    pub fn day_of_ms(&self, ms: i64) -> CollectionDay {
        self.today - (self.next_day_at * 1000 - 1 - ms).div_euclid(86_400_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn day_arithmetic_is_anchored_on_next_day_at() {
        let c = SchedClock { now_ms: 0, today: 10, crt: 0, rollover_hour: 4, next_day_at: 1_000_000, learn_ahead_secs: 1200 };
        assert_eq!(c.day_start_secs(11), 1_000_000);
        assert_eq!(c.day_start_secs(10), 1_000_000 - 86_400);
        assert_eq!(c.day_of_ms(1_000_000 * 1000 - 1), 10);
        assert_eq!(c.day_of_ms(1_000_000 * 1000), 11);
        assert_eq!(c.day_of_ms((1_000_000 - 86_400) * 1000 - 1), 9);
    }

    #[test]
    fn at_computes_today_and_next_day() {
        let crt = chrono::Local.with_ymd_and_hms(2022, 1, 1, 4, 0, 0).single().unwrap().timestamp();
        let now = chrono::Local.with_ymd_and_hms(2022, 6, 1, 23, 0, 0).single().unwrap().timestamp_millis();
        let c = SchedClock::at(now, crt, 4, 1200);
        assert_eq!(c.today, collection_day(now, crt, 4));
        assert_eq!(c.next_day_at, chrono::Local.with_ymd_and_hms(2022, 6, 2, 4, 0, 0).single().unwrap().timestamp());
        assert_eq!(c.day_of_ms(now), c.today);
    }
}
