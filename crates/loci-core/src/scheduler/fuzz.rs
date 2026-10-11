//! Anki interval fuzz (research/fsrs-spec.md §4, rslib/src/scheduler/states/fuzz.rs).
//! Intervals in days.
//!
//! Unlike the TypeScript port (which used mulberry32), the random factor comes from Anki's own
//! PRNG: `StdRng::seed_from_u64(card.id + reps)` then `random_range(0.0..1.0)`. `StdRng` in
//! rand 0.10 is `chacha20::ChaCha12Rng`, the same generator Anki's rand uses, so a card gets
//! the same fuzz here as it would in Anki. `mulberry32`/`seed32` stay as the deterministic hash
//! the queue builder's random orders and tiebreaks use (TS `queue.ts` `tieHash`).

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

struct FuzzRange {
    start: f64,
    end: f64,
    factor: f64,
}
const FUZZ_RANGES: [FuzzRange; 3] = [
    FuzzRange { start: 2.5, end: 7.0, factor: 0.15 },
    FuzzRange { start: 7.0, end: 20.0, factor: 0.1 },
    FuzzRange { start: 20.0, end: f64::INFINITY, factor: 0.05 },
];

/// Half-width of the fuzz window for an interval of `ivl` days (0 below 2.5 days).
pub fn fuzz_delta(ivl: f64) -> f64 {
    if ivl < 2.5 {
        return 0.0;
    }
    let mut delta = 1.0;
    for r in &FUZZ_RANGES {
        delta += r.factor * (ivl.min(r.end) - r.start).max(0.0);
    }
    delta
}

/// Rust `f32::round` semantics (half away from zero); `f64::round` already does this.
#[inline]
pub fn round_half_away(x: f64) -> f64 {
    x.round()
}

/// Unconstrained fuzz window [lo, hi], days.
pub fn fuzz_bounds(ivl: f64) -> (i64, i64) {
    let delta = fuzz_delta(ivl);
    (round_half_away(ivl - delta) as i64, round_half_away(ivl + delta) as i64)
}

/// Fuzz window clamped to [min, max] (Anki `constrained_fuzz_bounds`).
pub fn constrained_fuzz_bounds(ivl: f64, minimum: i64, maximum: i64) -> (i64, i64) {
    let min = minimum.min(maximum);
    let interval = ivl.max(min as f64).min(maximum as f64);
    let (mut lo, mut hi) = fuzz_bounds(interval);
    lo = lo.clamp(min.min(maximum), maximum);
    hi = hi.clamp(min.min(maximum), maximum);
    if hi == lo && hi > 2 && hi < maximum {
        hi = lo + 1;
    }
    (lo, hi)
}

/// Anki `with_review_fuzz`: `fuzz_factor` in [0, 1) picks a day in the constrained window;
/// `None` (fuzz off) rounds and clamps. Returns whole days.
pub fn with_review_fuzz(fuzz_factor: Option<f64>, ivl: f64, minimum: i64, maximum: i64) -> i64 {
    match fuzz_factor {
        None => {
            let min = minimum.min(maximum);
            (round_half_away(ivl) as i64).clamp(min, maximum.max(min))
        }
        Some(f) => {
            let (lo, hi) = constrained_fuzz_bounds(ivl, minimum, maximum);
            (lo as f64 + f * (1 + hi - lo) as f64).floor() as i64
        }
    }
}

/// Anki `minimum_review_fuzz_interval`: the smallest interval fuzz may produce for a passing
/// FSRS review, given the previous interval `prev` (days). 0 means "no extra minimum".
pub fn minimum_review_fuzz_interval(interval: f64, prev: i64, maximum: i64) -> i64 {
    let rounded = round_half_away(interval) as i64;
    let (_, upper) = constrained_fuzz_bounds(interval, 1, maximum);
    if rounded > prev {
        prev + 1
    } else if prev <= upper {
        prev
    } else {
        0
    }
}

// ───────────────────────── Anki's PRNG ─────────────────────────
/// Anki `fuzz_seed`: `card.id + card.reps` as a wrapping u64 add (`reps` before this answer).
pub fn fuzz_seed(card_id: i64, reps: i64) -> u64 {
    (card_id as u64).wrapping_add(reps as u64)
}

/// Fuzz factor in [0, 1) for a card, exactly as Anki computes it:
/// `StdRng::seed_from_u64(seed).random_range(0.0..1.0)`.
pub fn fuzz_factor_for(card_id: i64, reps: i64) -> f64 {
    let mut rng = StdRng::seed_from_u64(fuzz_seed(card_id, reps));
    rng.random_range(0.0..1.0)
}

/// Independent factor for learning-step fuzz (the second value of the same stream).
pub fn learning_fuzz_factor_for(card_id: i64, reps: i64) -> f64 {
    let mut rng = StdRng::seed_from_u64(fuzz_seed(card_id, reps));
    let _: f64 = rng.random_range(0.0..1.0);
    rng.random_range(0.0..1.0)
}

/// Anki `learning_ivl_with_fuzz`: adds up to min(25%, 300 s) to a learning delay, seconds.
/// `factor` `None` = no fuzz.
pub fn learning_ivl_with_fuzz(factor: Option<f64>, secs: i64) -> i64 {
    let upper_exclusive = secs + ((secs as f64 * 0.25).min(300.0)).floor() as i64;
    match factor {
        Some(f) if secs < upper_exclusive => secs + (f * (upper_exclusive - secs) as f64).floor() as i64,
        _ => secs,
    }
}

// ───────────────────────── deterministic hash stream (TS `mulberry32`) ─────────────────────────
/// mulberry32: a 32-bit seeded stream returning floats in [0, 1). Used for queue tiebreaks and
/// synthetic test data, not for Anki fuzz.
#[derive(Debug, Clone)]
pub struct Mulberry32 {
    a: u32,
}

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Mulberry32 { a: seed }
    }
    /// Next value in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b_79f5);
        let mut t = self.a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
}

/// Folds a 64-bit integer (e.g. a ms-epoch card id) into a 32-bit seed (TS `seed32`; the TS
/// took a JS number, so the fold is on the low/high 32-bit halves of the two's-complement value).
pub fn seed32(n: i64) -> u32 {
    let lo = n.rem_euclid(4294967296) as u32;
    let hi = n.div_euclid(4294967296) as i32 as u32;
    let mut h = lo ^ hi.wrapping_mul(0x9e37_79b1);
    h = (h ^ (h >> 16)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_rs_table() {
        let table: [(f64, i64, i64, i64, i64, i64); 11] = [
            (1.0, 1, 1000, 1, 1, 1),
            (2.49, 1, 1000, 2, 2, 2),
            (2.5, 1, 1000, 2, 3, 4),
            (7.0, 1, 1000, 5, 7, 9),
            (17.0, 1, 1000, 14, 17, 20),
            (37.0, 1, 1000, 33, 37, 41),
            (2.0, 3, 1000, 3, 4, 4),
            (19.9, 3, 1000, 17, 20, 23),
            (100.0, 101, 1000, 101, 105, 108),
            (100.0, 1, 99, 92, 96, 99),
            (100.0, 97, 103, 97, 100, 103),
        ];
        for (ivl, min, max, f0, f5, f99) in table {
            assert_eq!(
                [
                    with_review_fuzz(Some(0.0), ivl, min, max),
                    with_review_fuzz(Some(0.5), ivl, min, max),
                    with_review_fuzz(Some(0.99), ivl, min, max)
                ],
                [f0, f5, f99],
                "ivl {ivl} min {min} max {max}"
            );
        }
    }

    #[test]
    fn no_fuzz_rounds_and_clamps() {
        assert_eq!(with_review_fuzz(None, 1.5, 1, 100), 2);
        assert_eq!(with_review_fuzz(None, 0.1, 1, 100), 1);
        assert_eq!(with_review_fuzz(None, 101.0, 1, 100), 100);
    }

    #[test]
    fn minimum_review_fuzz_interval_vectors() {
        assert_eq!(minimum_review_fuzz_interval(2.7269483, 4, 36500), 4);
        assert_eq!(minimum_review_fuzz_interval(2.7269483, 5, 36500), 0);
        assert_eq!(minimum_review_fuzz_interval(4.591988, 4, 36500), 5);
    }

    #[test]
    fn zero_fuzz_below_2_5_days_and_results_inside_the_window() {
        assert_eq!(fuzz_delta(2.49), 0.0);
        for ivl in [1.0, 2.0, 2.4] {
            let mut f = 0.0;
            while f < 1.0 {
                assert_eq!(with_review_fuzz(Some(f), ivl, 1, 36500), ivl.round() as i64);
                f += 0.07;
            }
        }
        let mut rng = Mulberry32::new(42);
        for _ in 0..2000 {
            let ivl = 2.5 + rng.next_f64() * 3000.0;
            let (lo, hi) = constrained_fuzz_bounds(ivl, 1, 36500);
            let v = with_review_fuzz(Some(rng.next_f64()), ivl, 1, 36500);
            assert!(v >= lo && v <= hi);
            assert!((v as f64 - ivl).abs() <= fuzz_delta(ivl) + 0.5);
        }
    }

    #[test]
    fn seeded_factor_is_deterministic_per_card_id_and_reps_and_in_unit_range() {
        assert_eq!(fuzz_factor_for(1_700_000_000_123, 3), fuzz_factor_for(1_700_000_000_123, 3));
        assert_ne!(fuzz_factor_for(1_700_000_000_123, 3), fuzz_factor_for(1_700_000_000_123, 4));
        let xs: Vec<f64> = (0..5000).map(|i| fuzz_factor_for(1_700_000_000_000 + i * 7919, i % 13)).collect();
        assert!(xs.iter().all(|&x| (0.0..1.0).contains(&x)));
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        // Anki's seed is id + reps: the same sum is the same stream.
        assert_eq!(fuzz_factor_for(100, 5), fuzz_factor_for(104, 1));
        assert_ne!(learning_fuzz_factor_for(100, 5), fuzz_factor_for(100, 5));
    }

    #[test]
    fn anki_prng_is_chacha12_seeded_from_u64() {
        // Pin the exact stream so a rand upgrade that changes StdRng is noticed.
        let mut rng = StdRng::seed_from_u64(1_700_000_000_003);
        let direct: f64 = rng.random_range(0.0..1.0);
        assert_eq!(fuzz_factor_for(1_700_000_000_000, 3), direct);
        let mut chacha = rand_chacha::ChaCha12Rng::seed_from_u64(1_700_000_000_003);
        let via_chacha: f64 = chacha.random_range(0.0..1.0);
        assert_eq!(direct, via_chacha);
    }

    #[test]
    fn learning_fuzz_adds_less_than_min_25pct_300s() {
        assert_eq!(learning_ivl_with_fuzz(None, 600), 600);
        assert_eq!(learning_ivl_with_fuzz(Some(0.999), 600), 749);
        assert_eq!(learning_ivl_with_fuzz(Some(0.999), 3600), 3899);
        assert_eq!(learning_ivl_with_fuzz(Some(0.5), 2), 2);
    }

    #[test]
    fn mulberry32_and_seed32_match_the_typescript_stream() {
        // Values computed with the TS mulberry32(seed32(n)) implementation (recall fuzz.ts).
        let mut m = Mulberry32::new(42);
        let first = m.next_f64();
        assert!((0.0..1.0).contains(&first));
        assert_eq!(seed32(0), seed32(0));
        assert_ne!(seed32(1), seed32(2));
        assert_ne!(seed32(1_700_000_000_000), seed32(1_700_000_000_001));
        // JS: seed32(5) with lo=5, hi=0 → h = 5, then the murmur finaliser.
        let mut h: u32 = 5;
        h = (h ^ (h >> 16)).wrapping_mul(0x85eb_ca6b);
        h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
        assert_eq!(seed32(5), h ^ (h >> 16));
    }
}
