//! Anki's v3 SM-2 review arithmetic (rslib/src/scheduler/states/review.rs, fsrs-spec §8).
//! Intervals in days; ease as a factor (2.5 = 2500‰).

use super::fuzz::with_review_fuzz;
use crate::types::DeckConfig;

/// Minimum ease factor (1300‰).
pub const MINIMUM_EASE: f64 = 1.3;
pub const EASE_DELTA_AGAIN: f64 = -0.2;
pub const EASE_DELTA_HARD: f64 = -0.15;
pub const EASE_DELTA_EASY: f64 = 0.15;

/// SM-2 options used by the review formulas (from DeckConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sm2Options {
    /// 1.2
    pub hard_multiplier: f64,
    /// 1.3
    pub easy_bonus: f64,
    /// 1.0
    pub interval_modifier: f64,
    /// 0.0
    pub lapse_multiplier: f64,
    /// days
    pub minimum_lapse_interval: i64,
    /// days
    pub maximum_interval: i64,
}

impl From<&DeckConfig> for Sm2Options {
    fn from(c: &DeckConfig) -> Self {
        Sm2Options {
            hard_multiplier: c.hard_multiplier,
            easy_bonus: c.easy_bonus,
            interval_modifier: c.interval_modifier,
            lapse_multiplier: c.lapse_multiplier,
            minimum_lapse_interval: c.minimum_lapse_interval,
            maximum_interval: c.maximum_interval,
        }
    }
}

/// Anki `min_and_max_review_intervals`: maximum ≥ 1, minimum clamped into [1, maximum].
pub fn min_max_intervals(minimum: i64, maximum_interval: i64) -> (i64, i64) {
    let max = maximum_interval.max(1);
    (minimum.max(1).min(max), max)
}

/// Anki `constrain_passing_interval` (applies the interval modifier).
fn constrain_passing(o: &Sm2Options, interval: f64, minimum: i64, fuzz: Option<f64>) -> i64 {
    let ivl = interval * o.interval_modifier;
    let (min, max) = min_max_intervals(minimum, o.maximum_interval);
    with_review_fuzz(fuzz, ivl, min, max)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassingIntervals {
    pub hard: i64,
    pub good: i64,
    pub easy: i64,
}

/// Passing intervals for a review card. `scheduled_days` = current ivl, `elapsed_days` = days
/// since the last review, `ease` as a factor. Early reviews (elapsed < scheduled) use Anki's
/// early-review formulas without fuzz.
pub fn sm2_passing_intervals(
    o: &Sm2Options,
    scheduled_days: i64,
    elapsed_days: i64,
    ease: f64,
    fuzz: Option<f64>,
) -> PassingIntervals {
    let days_late = elapsed_days - scheduled_days;
    if days_late < 0 {
        let scheduled = scheduled_days as f64;
        let elapsed = elapsed_days as f64;
        let hard = constrain_passing(o, (elapsed * o.hard_multiplier).max(scheduled * (o.hard_multiplier / 2.0)), 0, None);
        let good = constrain_passing(o, (elapsed * ease).max(scheduled), 0, None);
        let reduced_bonus = o.easy_bonus - (o.easy_bonus - 1.0) / 2.0;
        let easy = constrain_passing(o, (elapsed * ease).max(scheduled) * reduced_bonus, 0, None);
        return PassingIntervals { hard, good, easy };
    }
    let current = scheduled_days.max(1) as f64;
    let late = days_late.max(0) as f64;
    let hard_min = if o.hard_multiplier <= 1.0 { 0 } else { scheduled_days + 1 };
    let hard = constrain_passing(o, current * o.hard_multiplier, hard_min, fuzz);
    let good_min = if o.hard_multiplier <= 1.0 { scheduled_days + 1 } else { hard + 1 };
    let good = constrain_passing(o, (current + late / 2.0) * ease, good_min, fuzz);
    let easy = constrain_passing(o, (current + late) * ease * o.easy_bonus, good + 1, fuzz);
    PassingIntervals { hard, good, easy }
}

/// Interval after a lapse: fuzz(max(1, ivl)·lapse_multiplier) within [minimum_lapse_interval, max].
pub fn sm2_lapse_interval(o: &Sm2Options, scheduled_days: i64, fuzz: Option<f64>) -> i64 {
    let (min, max) = min_max_intervals(o.minimum_lapse_interval, o.maximum_interval);
    with_review_fuzz(fuzz, scheduled_days.max(1) as f64 * o.lapse_multiplier, min, max)
}

/// Ease after an answer: Again −0.20, Hard −0.15, Good ±0, Easy +0.15; floor 1.3.
pub fn next_ease(ease: f64, rating: u8) -> f64 {
    let delta = match rating {
        1 => EASE_DELTA_AGAIN,
        2 => EASE_DELTA_HARD,
        4 => EASE_DELTA_EASY,
        _ => 0.0,
    };
    (ease + delta).max(MINIMUM_EASE)
}

/// Anki `leech_threshold_met`: true at the threshold and every ceil(threshold/2) lapses after.
/// threshold 0 disables.
pub fn leech_threshold_met(lapses: i64, threshold: i64) -> bool {
    if threshold <= 0 || lapses < threshold {
        return false;
    }
    let half = ((threshold + 1) / 2).max(1);
    (lapses - threshold) % half == 0
}

/// Permille ease (card.factor) → factor; 0 (new/unknown) → `fallback_permille`.
pub fn ease_from_factor(factor: i64, fallback_permille: i64) -> f64 {
    (if factor > 0 { factor } else { fallback_permille }) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Sm2Options {
        Sm2Options {
            hard_multiplier: 1.2,
            easy_bonus: 1.3,
            interval_modifier: 1.0,
            lapse_multiplier: 0.0,
            minimum_lapse_interval: 1,
            maximum_interval: 36500,
        }
    }

    #[test]
    fn s7_9_review_vectors_without_fuzz() {
        // ivl 100, due 8 days ago, ease 2.5: Hard 120, Good 260, Easy 351.
        let p = sm2_passing_intervals(&opts(), 100, 108, 2.5, None);
        assert_eq!(p, PassingIntervals { hard: 120, good: 260, easy: 351 });
        assert_eq!(next_ease(2.5, 2), 2.35);
        assert_eq!(next_ease(2.5, 3), 2.5);
        assert_eq!(next_ease(2.5, 4), 2.65);
        assert_eq!(next_ease(1.4, 1), 1.3);
    }

    #[test]
    fn early_review_formulas() {
        // elapsed 50 of 100: hard max(60, 60) = 60, good max(125, 100) = 125, easy 125 * 1.15 = 143.75 → 144.
        let p = sm2_passing_intervals(&opts(), 100, 50, 2.5, None);
        assert_eq!(p, PassingIntervals { hard: 60, good: 125, easy: 144 });
    }

    #[test]
    fn lapse_interval_and_minimums() {
        assert_eq!(sm2_lapse_interval(&opts(), 100, None), 1);
        let o = Sm2Options { lapse_multiplier: 0.5, minimum_lapse_interval: 3, ..opts() };
        assert_eq!(sm2_lapse_interval(&o, 100, None), 50);
        assert_eq!(sm2_lapse_interval(&o, 4, None), 3);
        assert_eq!(min_max_intervals(0, 0), (1, 1));
        assert_eq!(min_max_intervals(5, 3), (3, 3));
    }

    #[test]
    fn leech_threshold_met_vectors() {
        assert!([3, 5, 7].iter().all(|&l| leech_threshold_met(l, 3)));
        assert!([4, 6].iter().all(|&l| !leech_threshold_met(l, 3)));
        assert!([8, 12, 16].iter().all(|&l| leech_threshold_met(l, 8)));
        assert!([7, 9, 10, 11, 13].iter().all(|&l| !leech_threshold_met(l, 8)));
        assert!([1, 2, 3, 10].iter().all(|&l| leech_threshold_met(l, 1)));
        assert!(!leech_threshold_met(5, 0));
    }

    #[test]
    fn ease_from_factor_falls_back() {
        assert_eq!(ease_from_factor(2300, 2500), 2.3);
        assert_eq!(ease_from_factor(0, 2500), 2.5);
    }
}
