//! Day/time helpers (Anki v2/v3 day numbering, fsrs-spec §9.1), Anki-style interval labels and
//! SM-2 → FSRS memory-state conversion (fsrs-spec §5.4); recall `src/core/scheduler/convert.ts`.
//!
//! Day numbering: collection day N starts at `rollover_hour` local time on the calendar date
//! that is N days after the local date of `crt`. Uses the process's local time zone
//! (`chrono::Local`), like Anki's "count calendar-day changes" rule, so DST shifts do not move
//! day boundaries.

use super::fsrs::{memory_state_from_sm2, normalize_parameters, step, Grade, MemoryState};
use super::items::reviews_for_fsrs;
use crate::error::Result;
use crate::types::{CardType, CollectionDay, RevlogEntry, Seconds};
use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone, Timelike};

fn local(ms: i64) -> DateTime<Local> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default().with_timezone(&Local)
}

/// Calendar-date index (days since 1970-01-01) of a local date.
fn local_date_index(ms: i64) -> i64 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default();
    (local(ms).date_naive() - epoch).num_days()
}

/// Collection day number for `now_ms` (≥ 0). `crt_seconds` = collection creation time, seconds.
pub fn collection_day(now_ms: i64, crt_seconds: Seconds, rollover_hour: i64) -> CollectionDay {
    let now = local(now_ms);
    let mut days = local_date_index(now_ms) - local_date_index(crt_seconds * 1000);
    if (now.hour() as i64) < rollover_hour {
        days -= 1;
    }
    days.max(0)
}

/// Local start (ms epoch) of collection day `day`: its calendar date at `rollover_hour`.
pub fn day_start_ms(day: CollectionDay, crt_seconds: Seconds, rollover_hour: i64) -> i64 {
    let c = local(crt_seconds * 1000).date_naive();
    let date = if day >= 0 { c.checked_add_days(Days::new(day as u64)) } else { c.checked_sub_days(Days::new((-day) as u64)) }
        .unwrap_or(c);
    let hour = rollover_hour.clamp(0, 23) as u32;
    let naive = date.and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap_or_default());
    // A rollover inside a DST gap does not exist as a local time; JS Date moves it forward.
    let resolved = Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest());
    resolved.map(|d| d.timestamp_millis()).unwrap_or(crt_seconds * 1000 + day * 86_400_000)
}

/// Start of the next collection day after `now_ms` (Anki `next_day_at`), ms epoch.
pub fn next_day_at_ms(now_ms: i64, crt_seconds: Seconds, rollover_hour: i64) -> i64 {
    day_start_ms(collection_day(now_ms, crt_seconds, rollover_hour) + 1, crt_seconds, rollover_hour)
}

/// Whole scheduler days between two instants (rollover-aligned), ≥ 0.
pub fn elapsed_days(from_ms: i64, to_ms: i64, crt_seconds: Seconds, rollover_hour: i64) -> i64 {
    (collection_day(to_ms, crt_seconds, rollover_hour) - collection_day(from_ms, crt_seconds, rollover_hour)).max(0)
}

/// Local calendar year/month/day of an instant (for callers that build dates from a clock).
pub fn local_ymd(ms: i64) -> (i32, u32, u32) {
    let d = local(ms).date_naive();
    (d.year(), d.month(), d.day())
}

// ───────────────────────── labels ─────────────────────────
const MINUTE: f64 = 60.0;
const HOUR: f64 = 3600.0;
const DAY: f64 = 86_400.0;
/// Anki timespan.rs: MONTH = 30.417 days (365/12), YEAR = 365 days.
const MONTH: f64 = 30.417 * DAY;
const YEAR: f64 = 365.0 * DAY;

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// JS number formatting for the label amounts: `1` not `1.0`, `1.5` as is.
fn amount_str(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
    }
}

/// Interval label for answer buttons, the study screen's announcement and card info: '<1m'
/// under a minute, whole minutes ('6m', '10m'), hours to one decimal ('1.5h'), whole days
/// ('10d'), months and years to one decimal ('1.2mo', '1.5y'). Unlike Anki's
/// `answer_button_time` there is no '<' prefix for learn-ahead and no fractional minutes.
/// `_collapse_seconds` is accepted for compatibility and ignored.
pub fn format_interval(seconds: i64, _collapse_seconds: i64) -> String {
    let secs = seconds.abs() as f64;
    if secs < MINUTE {
        return "<1m".to_string();
    }
    let minutes = (secs / MINUTE).round();
    if minutes < 60.0 {
        return format!("{minutes:.0}m");
    }
    let (unit, amount) = if secs < DAY {
        let a = round1(secs / HOUR);
        if a >= 24.0 {
            ("d", 1.0)
        } else {
            ("h", a)
        }
    } else if secs < MONTH {
        ("d", (secs / DAY).round())
    } else if secs < YEAR {
        ("mo", round1(secs / MONTH))
    } else {
        ("y", round1(secs / YEAR))
    };
    format!("{}{unit}", amount_str(amount))
}

// ───────────────────────── SM-2 → FSRS (§5.4) ─────────────────────────
/// The card fields `memory_state_from_revlog` needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardSched {
    pub ctype: CardType,
    pub ivl: i64,
    pub factor: i64,
}

#[derive(Debug, Clone)]
pub struct RevlogConversionInput<'a> {
    pub card: CardSched,
    /// The card's revlog, oldest first.
    pub revlog: &'a [RevlogEntry],
    /// FSRS parameters (any accepted length).
    pub params: &'a [f64],
    /// Retention assumed for SM-2 history (deck config `historical_retention`, default 0.9).
    pub historical_retention: f64,
    /// Anki `next_day_at`, ms epoch (start of tomorrow).
    pub next_day_at_ms: i64,
    /// Reviews before this instant (ms) are ignored (Anki "ignore reviews before"); 0 = none.
    pub ignore_before_ms: i64,
}

/// Anki `update_memory_state` for one card: replay the revlog from the first learning step when
/// history is complete; otherwise seed from the first kept review's SM-2 ease and interval and
/// replay the rest; with no usable revlog fall back to the card's ease/ivl. `None` for new cards
/// with no history. Errors only on an invalid parameter vector.
pub fn memory_state_from_revlog(input: &RevlogConversionInput<'_>) -> Result<Option<MemoryState>> {
    let w = normalize_parameters(input.params)?;
    let hr = input.historical_retention;
    if let Some(r) = reviews_for_fsrs(input.revlog, input.next_day_at_ms, false, input.ignore_before_ms) {
        if !r.reviews.is_empty() {
            if r.revlogs_complete {
                let mut state = None;
                for rv in &r.reviews {
                    state = Some(step(&w, state, rv.rating as Grade, rv.delta_t));
                }
                return Ok(state);
            }
            let first = &r.entries[0];
            let ef = (if first.factor == 0 { 2500 } else { first.factor }) as f64 / 1000.0;
            let ivl = first.ivl.max(1) as f64;
            let mut state = memory_state_from_sm2(&w, ef, ivl, hr);
            if ef <= 1.1 {
                // Written by FSRS (revlog factor = difficulty_shifted); invert it (spec §5.4).
                state = MemoryState { stability: state.stability, difficulty: (ef - 0.1) * 9.0 + 1.0 };
            }
            for rv in &r.reviews[1..] {
                state = step(&w, Some(state), rv.rating as Grade, rv.delta_t);
            }
            return Ok(Some(state));
        }
    }
    if input.card.ctype != CardType::New && input.card.ivl > 0 {
        let ease = (if input.card.factor == 0 { 2500 } else { input.card.factor }) as f64 / 1000.0;
        return Ok(Some(memory_state_from_sm2(&w, ease, input.card.ivl as f64, hr)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::fsrs::DEFAULT_PARAMS_FSRS6;
    use crate::scheduler::items::tests::{entry, NEXT_DAY_AT};
    use crate::types::RevlogType;

    fn lt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp_millis()
    }

    #[test]
    fn format_interval_units_and_rounding() {
        let cases: [(i64, &str); 21] = [
            (0, "<1m"),
            (45, "<1m"),
            (59, "<1m"),
            (60, "1m"),
            (330, "6m"),
            (600, "10m"),
            (3569, "59m"),
            (3590, "1h"),
            (3600, "1h"),
            (5400, "1.5h"),
            (86_399, "1d"),
            (86_400, "1d"),
            (3 * 86_400, "3d"),
            (10 * 86_400, "10d"),
            (30 * 86_400, "30d"),
            (31 * 86_400, "1mo"),
            (36 * 86_400 + 43_200, "1.2mo"),
            (200 * 86_400, "6.6mo"),
            (365 * 86_400, "1y"),
            (547 * 86_400 + 43_200, "1.5y"),
            (3650 * 86_400, "10y"),
        ];
        for (s, label) in cases {
            assert_eq!(format_interval(s, 0), label, "{s}");
        }
    }

    #[test]
    fn no_learn_ahead_prefix() {
        assert_eq!(format_interval(60, 1200), "1m");
        assert_eq!(format_interval(330, 1200), "6m");
        assert_eq!(format_interval(600, 1200), "10m");
        assert_eq!(format_interval(1200, 1200), "20m");
        assert_eq!(format_interval(86_400, 1200), "1d");
    }

    #[test]
    fn day_changes_at_the_rollover_hour() {
        let crt = lt(2022, 1, 1, 4, 0) / 1000;
        assert_eq!(collection_day(lt(2022, 1, 1, 12, 0), crt, 4), 0);
        assert_eq!(collection_day(lt(2022, 1, 2, 3, 59), crt, 4), 0);
        assert_eq!(collection_day(lt(2022, 1, 2, 4, 0), crt, 4), 1);
        assert_eq!(collection_day(lt(2021, 12, 25, 0, 0), crt, 4), 0);
    }

    #[test]
    fn dst_does_not_shift_day_numbers_or_boundaries() {
        // US DST started 2022-03-13 and ended 2022-11-06 (the machine runs a DST zone; in a
        // zone without DST the arithmetic is the same).
        let crt = lt(2022, 1, 1, 4, 0) / 1000;
        assert_eq!(collection_day(lt(2022, 3, 13, 12, 0), crt, 4), 71);
        assert_eq!(collection_day(lt(2022, 3, 14, 4, 0), crt, 4), 72);
        assert_eq!(local(day_start_ms(72, crt, 4)).hour(), 4);
        assert_eq!(local(day_start_ms(309, crt, 4)).hour(), 4);
        let mut day = 0;
        while day < 400 {
            assert_eq!(collection_day(day_start_ms(day, crt, 4), crt, 4), day);
            day += 7;
        }
    }

    #[test]
    fn next_day_and_elapsed_days() {
        let crt = lt(2022, 1, 1, 4, 0) / 1000;
        assert_eq!(next_day_at_ms(lt(2022, 6, 1, 23, 0), crt, 4), lt(2022, 6, 2, 4, 0));
        assert_eq!(next_day_at_ms(lt(2022, 6, 1, 2, 0), crt, 4), lt(2022, 6, 1, 4, 0));
        assert_eq!(elapsed_days(lt(2022, 6, 1, 23, 0), lt(2022, 6, 2, 3, 0), crt, 4), 0);
        assert_eq!(elapsed_days(lt(2022, 6, 1, 23, 0), lt(2022, 6, 2, 5, 0), crt, 4), 1);
    }

    fn conv<'a>(card: CardSched, revlog: &'a [RevlogEntry]) -> RevlogConversionInput<'a> {
        RevlogConversionInput {
            card,
            revlog,
            params: &[],
            historical_retention: 0.9,
            next_day_at_ms: NEXT_DAY_AT,
            ignore_before_ms: 0,
        }
    }

    #[test]
    fn complete_history_replays_from_scratch() {
        let revlog = [entry(15, RevlogType::Learn, 3), entry(13, RevlogType::Review, 3), entry(5, RevlogType::Review, 3)];
        let card = CardSched { ctype: CardType::Review, ivl: 10, factor: 2500 };
        let got = memory_state_from_revlog(&conv(card, &revlog)).unwrap().unwrap();
        let mut st = None;
        for (g, t) in [(3u8, 0i64), (3, 2), (3, 8)] {
            st = Some(step(&DEFAULT_PARAMS_FSRS6, st, g, t));
        }
        assert!((got.stability - st.unwrap().stability).abs() < 1e-10);
    }

    #[test]
    fn truncated_history_seeds_from_sm2_ease_and_ivl_of_the_first_kept_review() {
        let mut a = entry(99, RevlogType::Review, 3);
        a.ivl = 100;
        let mut b = entry(0, RevlogType::Review, 3);
        b.ivl = 250;
        let card = CardSched { ctype: CardType::Review, ivl: 250, factor: 2500 };
        let got = memory_state_from_revlog(&conv(card, &[a, b])).unwrap().unwrap();
        let w = &DEFAULT_PARAMS_FSRS6;
        let start = MemoryState {
            stability: 100.0,
            difficulty: 11.0 - 1.5 / (w[8].exp() * 100f64.powf(-w[9]) * ((0.1 * w[10]).exp() - 1.0)),
        };
        let exp = step(w, Some(start), 3, 99);
        assert!((got.stability - exp.stability).abs() < 1e-8);
        assert!((got.difficulty - exp.difficulty).abs() < 1e-8);
        assert!(got.stability > 100.0);
    }

    #[test]
    fn no_revlog_uses_card_ease_and_ivl_new_card_is_none() {
        let card = CardSched { ctype: CardType::Review, ivl: 10, factor: 2500 };
        let m = memory_state_from_revlog(&conv(card, &[])).unwrap().unwrap();
        assert!((m.stability - 10.0).abs() < 1e-9);
        assert!((m.difficulty - 6.9140563).abs() < 1e-5);
        let fresh = CardSched { ctype: CardType::New, ivl: 0, factor: 0 };
        assert!(memory_state_from_revlog(&conv(fresh, &[])).unwrap().is_none());
    }
}
