//! Hybrid logical clock stamps and version vectors (docs/SYNC.md "Stamps").
//!
//! A stamp is `(hlc, device)`. `hlc` packs wall-clock milliseconds in the high bits and a counter
//! in the low 16 bits, so it always moves forward even when the wall clock does not. Stamps are
//! totally ordered by `(hlc, device)`; that order decides every last-writer-wins merge.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bits of the counter below the milliseconds.
pub const COUNTER_BITS: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub hlc: i64,
    pub dev: String,
}

impl Stamp {
    pub fn new(hlc: i64, dev: impl Into<String>) -> Stamp {
        Stamp { hlc, dev: dev.into() }
    }
}

/// The next clock value after `prev` at wall time `now_ms`: the wall time when it is ahead,
/// otherwise `prev + 1`.
pub fn hlc_next(prev: i64, now_ms: i64) -> i64 {
    let wall = now_ms.max(0) << COUNTER_BITS;
    if wall > prev {
        wall
    } else {
        prev + 1
    }
}

/// Wall-clock milliseconds of an hlc value.
pub fn hlc_ms(hlc: i64) -> i64 {
    hlc >> COUNTER_BITS
}

/// Highest hlc seen from each device: "every change by `dev` up to `hlc` has been seen".
pub type VersionVector = BTreeMap<String, i64>;

/// True when `v` has seen the change stamped `s`.
pub fn covers(v: &VersionVector, s: &Stamp) -> bool {
    v.get(&s.dev).is_some_and(|h| *h >= s.hlc)
}

/// True when `a` has seen everything `b` has (componentwise `a >= b`).
pub fn dominates(a: &VersionVector, b: &VersionVector) -> bool {
    b.iter().all(|(d, h)| a.get(d).is_some_and(|x| x >= h))
}

/// Componentwise maximum, in place.
pub fn join(into: &mut VersionVector, other: &VersionVector) {
    for (d, h) in other {
        let e = into.entry(d.clone()).or_insert(*h);
        if *e < *h {
            *e = *h;
        }
    }
}

/// Componentwise minimum of several vectors (a device missing from any of them is dropped).
/// The meet of nothing is the empty vector.
pub fn meet<'a>(vectors: impl IntoIterator<Item = &'a VersionVector>) -> VersionVector {
    let mut it = vectors.into_iter();
    let Some(first) = it.next() else { return VersionVector::new() };
    let mut out = first.clone();
    for v in it {
        out.retain(|d, h| match v.get(d) {
            Some(x) => {
                *h = (*h).min(*x);
                true
            }
            None => false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vv(pairs: &[(&str, i64)]) -> VersionVector {
        pairs.iter().map(|(d, h)| (d.to_string(), *h)).collect()
    }

    #[test]
    fn clock_moves_forward_even_when_the_wall_clock_stalls_or_goes_back() {
        let a = hlc_next(0, 1000);
        assert_eq!(hlc_ms(a), 1000);
        let b = hlc_next(a, 1000);
        assert_eq!(b, a + 1);
        let c = hlc_next(b, 900);
        assert_eq!(c, b + 1);
        assert_eq!(hlc_ms(hlc_next(c, 2000)), 2000);
    }

    #[test]
    fn stamps_order_by_clock_then_device() {
        assert!(Stamp::new(5, "b") > Stamp::new(5, "a"));
        assert!(Stamp::new(6, "a") > Stamp::new(5, "z"));
    }

    #[test]
    fn vector_operations() {
        let a = vv(&[("x", 5), ("y", 2)]);
        let b = vv(&[("x", 3), ("z", 9)]);
        assert!(covers(&a, &Stamp::new(5, "x")) && !covers(&a, &Stamp::new(6, "x")) && !covers(&a, &Stamp::new(1, "z")));
        assert!(dominates(&a, &vv(&[("x", 4)])) && !dominates(&a, &b));
        let mut j = a.clone();
        join(&mut j, &b);
        assert_eq!(j, vv(&[("x", 5), ("y", 2), ("z", 9)]));
        assert_eq!(meet([&a, &b]), vv(&[("x", 3)]));
        assert_eq!(meet(std::iter::empty()), VersionVector::new());
    }
}
