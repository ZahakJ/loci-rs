//! FSRS-6 (21 parameters), ported literally from fsrs-rs v6.6.2
//! (research/fsrs-spec.md §1–§2).
//!
//! Units: stability in days, difficulty on [1, 10], retrievability as a probability, elapsed
//! time in (integer, scheduler) days. All arithmetic is f64; fsrs-rs uses f32, so results agree
//! to ~1e-5 relative (spec §7.5).

use crate::error::{LociError, Result};

// ───────────────────────── constants (spec §1.1) ─────────────────────────
/// Minimum stability, days.
pub const S_MIN: f64 = 0.001;
/// Maximum stability, days.
pub const S_MAX: f64 = 36500.0;
pub const D_MIN: f64 = 1.0;
pub const D_MAX: f64 = 10.0;
/// Upper clamp for the initial stabilities w0–w3, days.
pub const INIT_S_MAX: f64 = 100.0;
/// Decay used when a 17/19-parameter vector is padded to 21.
pub const FSRS5_DEFAULT_DECAY: f64 = 0.5;
/// Default FSRS-6 decay (w20).
pub const FSRS6_DEFAULT_DECAY: f64 = 0.1542;
/// Number of FSRS-6 parameters.
pub const N_PARAMS: usize = 21;

/// fsrs-rs v6.6.2 DEFAULT_PARAMETERS (same vector as `collection::presets::FSRS6_DEFAULT_PARAMS`).
pub const DEFAULT_PARAMS_FSRS6: [f64; N_PARAMS] = crate::collection::presets::FSRS6_DEFAULT_PARAMS;

/// FSRS memory state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryState {
    /// Stability, days (interval at which R = 0.9).
    pub stability: f64,
    /// Difficulty, 1..10.
    pub difficulty: f64,
}

impl MemoryState {
    pub fn new(stability: f64, difficulty: f64) -> Self {
        MemoryState { stability, difficulty }
    }
}

/// FSRS grade: 1 Again, 2 Hard, 3 Good, 4 Easy (0 = no rating, state unchanged).
pub type Grade = u8;

/// `clamp` without the NaN panic of `f64::clamp` (NaN passes through).
#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

// ───────────────────────── forgetting curve (§1.2, §1.3) ─────────────────────────
/// FACTOR = 0.9^(1/DECAY) − 1 with DECAY = −decay. `decay` is the positive w20.
pub fn curve_factor(decay: f64) -> f64 {
    (0.9f64.ln() / -decay).exp() - 1.0
}

/// Retrievability after `elapsed_days` (may be fractional) for a card of `stability` days.
/// `decay` is the positive w20 (0.1542 default; 0.5 for FSRS-4.5/5). R(S, S) = 0.9.
pub fn forgetting_curve(elapsed_days: f64, stability: f64, decay: f64) -> f64 {
    let factor = curve_factor(decay);
    (1.0 + factor * elapsed_days / stability).powf(-decay)
}

/// Unrounded interval in days at which R falls to `desired_retention` (§1.3).
pub fn next_interval(stability: f64, desired_retention: f64, decay: f64) -> f64 {
    let factor = curve_factor(decay);
    stability / factor * (desired_retention.powf(1.0 / -decay) - 1.0)
}

// ───────────────────────── initial state (§1.4) ─────────────────────────
/// S0(G) = w[G−1], days.
pub fn init_stability(w: &[f64], rating: Grade) -> f64 {
    let g = rating.clamp(1, 4) as usize;
    w[g - 1]
}

/// D0(G) = w4 − exp(w5·(G−1)) + 1, unclamped (clamp when storing as a state).
pub fn init_difficulty(w: &[f64], rating: Grade) -> f64 {
    w[4] - (w[5] * (rating as f64 - 1.0)).exp() + 1.0
}

// ───────────────────────── difficulty (§1.5) ─────────────────────────
/// Linear-damped difficulty before mean reversion: D + (−w6·(G−3))·(10−D)/9.
pub fn linear_damping(w: &[f64], difficulty: f64, rating: Grade) -> f64 {
    let delta = -w[6] * (rating as f64 - 3.0);
    difficulty + delta * (10.0 - difficulty) / 9.0
}

/// Mean reversion toward the unclamped D0(Easy): w7·D0(4) + (1−w7)·D.
pub fn mean_reversion(w: &[f64], difficulty: f64) -> f64 {
    let init = init_difficulty(w, 4);
    w[7] * (init - difficulty) + difficulty
}

/// Full difficulty update: damping, mean reversion, clamp to [1, 10].
pub fn next_difficulty(w: &[f64], difficulty: f64, rating: Grade) -> f64 {
    clamp(mean_reversion(w, linear_damping(w, difficulty, rating)), D_MIN, D_MAX)
}

// ───────────────────────── stability (§1.6–§1.8) ─────────────────────────
/// Stability after a successful (G ≥ 2) review with elapsed > 0.
pub fn next_recall_stability(w: &[f64], s: f64, d: f64, r: f64, rating: Grade) -> f64 {
    let hard_penalty = if rating == 2 { w[15] } else { 1.0 };
    let easy_bonus = if rating == 4 { w[16] } else { 1.0 };
    s * (w[8].exp() * (11.0 - d) * s.powf(-w[9]) * ((w[10] * (1.0 - r)).exp() - 1.0) * hard_penalty * easy_bonus + 1.0)
}

/// Stability after a lapse (G = 1) with elapsed > 0, capped at S / exp(w17·w18).
pub fn next_forget_stability(w: &[f64], s: f64, d: f64, r: f64) -> f64 {
    let long_term = w[11] * d.powf(-w[12]) * ((s + 1.0).powf(w[13]) - 1.0) * (w[14] * (1.0 - r)).exp();
    let cap = s / (w[17] * w[18]).exp();
    long_term.min(cap)
}

/// Same-day (elapsed = 0) stability: S · SInc, SInc ≥ 1 for G ≥ 2.
pub fn same_day_stability(w: &[f64], s: f64, rating: Grade) -> f64 {
    let sinc = (w[17] * (rating as f64 - 3.0 + w[18])).exp() * s.powf(-w[19]);
    s * if rating >= 2 { sinc.max(1.0) } else { sinc }
}

// ───────────────────────── step (§1.9) ─────────────────────────
/// One review. `state` `None` means a new card (first rating). `elapsed_days` is the integer
/// number of scheduler days since the previous review (0 = same day).
pub fn step(w: &[f64], state: Option<MemoryState>, rating: Grade, elapsed_days: i64) -> MemoryState {
    let state = match state {
        Some(s) if !(s.stability == 0.0 && s.difficulty == 0.0) => s,
        _ => {
            if rating == 0 {
                return MemoryState::new(0.0, 0.0);
            }
            return MemoryState {
                stability: clamp(init_stability(w, rating), S_MIN, S_MAX),
                difficulty: clamp(init_difficulty(w, rating), D_MIN, D_MAX),
            };
        }
    };
    let last_s = clamp(state.stability, S_MIN, S_MAX);
    let last_d = clamp(state.difficulty, D_MIN, D_MAX);
    if rating == 0 {
        return MemoryState::new(last_s, last_d);
    }
    let decay = w[20];
    let new_s = if elapsed_days == 0 {
        same_day_stability(w, last_s, rating)
    } else {
        let r = forgetting_curve(elapsed_days as f64, last_s, decay);
        if rating == 1 {
            next_forget_stability(w, last_s, last_d, r)
        } else {
            next_recall_stability(w, last_s, last_d, r, rating)
        }
    };
    MemoryState { stability: clamp(new_s, S_MIN, S_MAX), difficulty: next_difficulty(w, last_d, rating) }
}

/// Candidate outcome for one button: new memory and unrounded interval (days).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemState {
    pub memory: MemoryState,
    pub interval: f64,
}

/// The four candidate next states of fsrs-rs `next_states`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextStates {
    pub again: ItemState,
    pub hard: ItemState,
    pub good: ItemState,
    pub easy: ItemState,
}

impl NextStates {
    /// The candidate for a rating 1..4.
    pub fn get(&self, rating: Grade) -> &ItemState {
        match rating {
            1 => &self.again,
            2 => &self.hard,
            3 => &self.good,
            _ => &self.easy,
        }
    }
}

/// fsrs-rs `next_states`: the four candidate memory states with their unrounded intervals.
pub fn next_states(w: &[f64], state: Option<MemoryState>, desired_retention: f64, elapsed_days: i64) -> NextStates {
    let one = |g: Grade| {
        let memory = step(w, state, g, elapsed_days);
        ItemState { memory, interval: next_interval(memory.stability, desired_retention, w[20]) }
    };
    NextStates { again: one(1), hard: one(2), good: one(3), easy: one(4) }
}

/// Replays a sequence of (rating, elapsed_days) from `start` (`None` = new card).
pub fn memory_state_after(w: &[f64], reviews: &[(Grade, i64)], start: Option<MemoryState>) -> Option<MemoryState> {
    let mut state = start;
    for &(rating, delta_t) in reviews {
        state = Some(step(w, state, rating, delta_t));
    }
    state
}

// ───────────────────────── SM-2 conversion (§5.4) ─────────────────────────
/// fsrs-rs `memory_state_from_sm2`: a memory state that reproduces SM-2's growth factor
/// `ease` (e.g. 2.5) at retention `r` for a card with `interval` days.
pub fn memory_state_from_sm2(w: &[f64], ease: f64, interval: f64, r: f64) -> MemoryState {
    let decay = w[20];
    let factor = curve_factor(decay);
    let stability = interval.max(S_MIN) * factor / (r.powf(1.0 / -decay) - 1.0);
    let difficulty = 11.0 - (ease - 1.0) / (w[8].exp() * stability.powf(-w[9]) * (((1.0 - r) * w[10]).exp() - 1.0));
    MemoryState { stability: clamp(stability, S_MIN, S_MAX), difficulty: clamp(difficulty, D_MIN, D_MAX) }
}

// ───────────────────────── parameters (§2.2, §2.3) ─────────────────────────
fn params_error(msg: String) -> LociError {
    LociError::invalid(msg)
}

/// fsrs-rs `check_and_fill_parameters`: 0 → defaults; 17 → FSRS-4.5 conversion + pad;
/// 19 → pad with w19 = 0, decay 0.5; 21 → as is. Errors on other lengths or non-finite values.
pub fn check_and_fill_parameters(params: &[f64]) -> Result<Vec<f64>> {
    if params.iter().any(|x| !x.is_finite()) {
        return Err(params_error("FSRS parameters must be finite numbers".into()));
    }
    match params.len() {
        0 => Ok(DEFAULT_PARAMS_FSRS6.to_vec()),
        17 => {
            let mut w = params.to_vec();
            w[4] += 2.0 * w[5];
            w[5] = (1.0 + 3.0 * w[5]).ln() / 3.0;
            w[6] += 0.5;
            w.extend_from_slice(&[0.0, 0.0, 0.0, FSRS5_DEFAULT_DECAY]);
            Ok(w)
        }
        19 => {
            let mut w = params.to_vec();
            w.extend_from_slice(&[0.0, FSRS5_DEFAULT_DECAY]);
            Ok(w)
        }
        21 => Ok(params.to_vec()),
        n => Err(params_error(format!("FSRS needs 17, 19 or 21 parameters, got {n}"))),
    }
}

/// Ceiling for w17 and w18 (§2.2) so relearning steps cannot raise post-lapse stability.
pub fn w17w18_ceiling(w: &[f64], num_relearning_steps: usize) -> f64 {
    if num_relearning_steps <= 1 {
        return 2.0;
    }
    let value = -(w[11].ln() + (2f64.powf(w[13]) - 1.0).ln() + 0.3 * w[14]) / num_relearning_steps as f64;
    value.max(0.01).sqrt().min(2.0)
}

/// Clamp bounds per index (§2.2).
pub fn parameter_bounds(w: &[f64], num_relearning_steps: usize, enable_short_term: bool) -> [(f64, f64); N_PARAMS] {
    let ceiling = w17w18_ceiling(w, num_relearning_steps);
    [
        (S_MIN, INIT_S_MAX),
        (S_MIN, INIT_S_MAX),
        (S_MIN, INIT_S_MAX),
        (S_MIN, INIT_S_MAX),
        (1.0, 10.0),
        (0.001, 4.0),
        (0.001, 4.0),
        (0.001, 0.75),
        (0.0, 4.5),
        (0.0, 0.8),
        (0.001, 3.5),
        (0.001, 5.0),
        (0.001, 0.25),
        (0.001, 0.9),
        (0.0, 4.0),
        (0.0, 1.0),
        (1.0, 6.0),
        (0.0, ceiling),
        (0.0, ceiling),
        (if enable_short_term { 0.01 } else { 0.0 }, 0.8),
        (0.1, 0.8),
    ]
}

/// fsrs-rs `clip_parameters` on a 21-vector (returns a new vector).
pub fn clamp_parameters(w: &[f64], num_relearning_steps: usize, enable_short_term: bool) -> Result<Vec<f64>> {
    if w.len() != N_PARAMS {
        return Err(params_error(format!("clamp_parameters needs 21 parameters, got {}", w.len())));
    }
    let bounds = parameter_bounds(w, num_relearning_steps, enable_short_term);
    Ok(w.iter().zip(bounds.iter()).map(|(&x, &(lo, hi))| clamp(x, lo, hi)).collect())
}

/// `clamp_parameters` with Anki's training arguments (`enable_short_term = true`).
pub fn clamp_parameters_default(w: &[f64]) -> Result<Vec<f64>> {
    clamp_parameters(w, 1, true)
}

/// Parameters as `FSRS::new` sees them: filled to 21 and clipped with one relearning step and
/// short-term disabled for the w19 floor (spec §2.2 last line).
pub fn normalize_parameters(params: &[f64]) -> Result<Vec<f64>> {
    clamp_parameters(&check_and_fill_parameters(params)?, 1, false)
}

/// Anki `get_decay_from_params`: empty → 0.1542, fewer than 21 → 0.5, else w20.
pub fn decay_from_params(params: &[f64]) -> f64 {
    if params.is_empty() {
        FSRS6_DEFAULT_DECAY
    } else if params.len() < N_PARAMS {
        FSRS5_DEFAULT_DECAY
    } else {
        params[20]
    }
}

/// Anki `fsrs_allow_short_term`: params ≥ 19 → w17 > 0 && w18 > 0; empty → true.
pub fn allow_short_term(params: &[f64]) -> bool {
    if params.len() >= 19 {
        params[17] > 0.0 && params[18] > 0.0
    } else {
        params.is_empty()
    }
}

// ───────────────────────── model (FSRS-7 ready) ─────────────────────────
/// An FSRS-6 memory model built from a normalised parameter vector: forgetting curve, interval
/// and step. FSRS-7 (two-component curve, 34 params) can be added behind the same methods later.
#[derive(Debug, Clone, PartialEq)]
pub struct Fsrs6 {
    /// Normalised 21-parameter vector.
    pub params: Vec<f64>,
    /// Positive decay used by the curve (w20).
    pub decay: f64,
}

impl Fsrs6 {
    /// Builds a model from any accepted parameter vector (0/17/19/21 values).
    pub fn new(params: &[f64]) -> Result<Fsrs6> {
        let w = normalize_parameters(params)?;
        let decay = w[20];
        Ok(Fsrs6 { params: w, decay })
    }

    /// The model with the default FSRS-6 parameters.
    pub fn default_model() -> Fsrs6 {
        Fsrs6 { params: DEFAULT_PARAMS_FSRS6.to_vec(), decay: FSRS6_DEFAULT_DECAY }
    }

    /// Retrievability of `state` after `elapsed_days` (fractional allowed).
    pub fn retrievability(&self, state: MemoryState, elapsed_days: f64) -> f64 {
        forgetting_curve(elapsed_days, state.stability, self.decay)
    }

    /// Unrounded interval in days for `desired_retention`.
    pub fn interval(&self, state: MemoryState, desired_retention: f64) -> f64 {
        next_interval(state.stability, desired_retention, self.decay)
    }

    /// One review (`None` = new card).
    pub fn step(&self, state: Option<MemoryState>, rating: Grade, elapsed_days: i64) -> MemoryState {
        step(&self.params, state, rating, elapsed_days)
    }

    /// The four candidate next states.
    pub fn next_states(&self, state: Option<MemoryState>, desired_retention: f64, elapsed_days: i64) -> NextStates {
        next_states(&self.params, state, desired_retention, elapsed_days)
    }

    /// SM-2 ease/interval → memory state at `retention`.
    pub fn from_sm2(&self, ease: f64, interval: f64, retention: f64) -> MemoryState {
        memory_state_from_sm2(&self.params, ease, interval, retention)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &[f64] = &DEFAULT_PARAMS_FSRS6;

    /// fsrs-rs assert_approx_eq-style closeness: |a−b| ≤ tol·max(1, |b|).
    pub(crate) fn near(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b} (tol {tol})");
    }
    fn replay(w: &[f64], rs: &[(Grade, i64)]) -> MemoryState {
        memory_state_after(w, rs, None).unwrap()
    }

    #[test]
    fn default_parameters_are_the_fsrs_rs_v662_vector() {
        assert_eq!(
            DEFAULT_PARAMS_FSRS6.to_vec(),
            vec![
                0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629,
                1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542
            ]
        );
        assert_eq!(FSRS6_DEFAULT_DECAY, 0.1542);
    }

    const SEQ: &[(Grade, i64)] = &[(1, 0), (3, 0), (3, 1), (3, 3), (3, 8), (3, 21)];

    #[test]
    fn s7_1_default_params_sequence() {
        let s = replay(W, SEQ);
        near(s.stability, 53.62691, 1e-4);
        near(s.difficulty, 6.3574867, 1e-4);
    }

    #[test]
    fn s7_1_intermediate_states_match_the_derivation_table() {
        let expected =
            [(0.212, 6.4133), (0.24669, 6.40212), (2.02148, 6.39094), (7.8637, 6.37978), (21.91771, 6.36863), (53.6269, 6.35749)];
        let mut st = None;
        for (i, &(g, t)) in SEQ.iter().enumerate() {
            let n = step(W, st, g, t);
            near(n.stability, expected[i].0, 1e-4);
            near(n.difficulty, expected[i].1, 1e-4);
            st = Some(n);
        }
    }

    #[test]
    fn s7_1_short_term_frozen() {
        let mut w = W.to_vec();
        w[17] = 0.0;
        w[18] = 0.0;
        w[19] = 0.0;
        let s = replay(&w, SEQ);
        near(s.stability, 53.335106, 1e-4);
        near(s.difficulty, 6.3574867, 1e-4);
    }

    #[test]
    fn s7_2_first_rating_next_states() {
        let n = next_states(W, None, 0.9, 0);
        let exp = [(0.212, 6.4133), (1.2931, 5.1121707), (2.3065, 2.118104), (8.2956, 1.0)];
        for (i, st) in [n.again, n.hard, n.good, n.easy].iter().enumerate() {
            near(st.memory.stability, exp[i].0, 1e-4);
            near(st.memory.difficulty, exp[i].1, 1e-4);
            near(st.interval, exp[i].0, 1e-4);
        }
    }

    #[test]
    fn s7_3_difficulty_damping_and_mean_reversion() {
        let before = [8.354889, 6.6774445, 5.0, 3.3225555];
        let after = [8.341763, 6.6659956, 4.990228, 3.3144615];
        for g in 1..=4u8 {
            near(linear_damping(W, 5.0, g), before[g as usize - 1], 1e-4);
            near(mean_reversion(W, linear_damping(W, 5.0, g)), after[g as usize - 1], 1e-4);
        }
    }

    #[test]
    fn s7_3_recall_forget_and_same_day_stability() {
        let r = [0.9, 0.8, 0.7, 0.6];
        let recall = [25.602541, 28.226582, 58.656002, 127.226685];
        let forget = [1.0525396, 1.1894329, 1.3680838, 1.584989];
        let same = [1.596818, 5.0, 5.0, 8.12961];
        for i in 0..4 {
            near(next_recall_stability(W, 5.0, (i + 1) as f64, r[i], (i + 1) as u8), recall[i], 1e-4);
            near(next_forget_stability(W, 5.0, (i + 1) as f64, r[i]), forget[i], 1e-4);
            near(same_day_stability(W, 5.0, (i + 1) as u8), same[i], 1e-4);
        }
    }

    #[test]
    fn s7_3_init_difficulty() {
        let exp = [6.4133, 5.1121707, 2.118104];
        for g in 1..=3u8 {
            near(init_difficulty(W, g), exp[g as usize - 1], 1e-4);
        }
        near(init_difficulty(W, 4), -4.77, 1e-2);
        assert_eq!(step(W, None, 4, 0).difficulty, 1.0);
    }

    #[test]
    fn s7_4_forgetting_curve_vectors() {
        let cases = [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.0, 4.0), (4.0, 4.0), (5.0, 2.0)];
        let exp = [1.0, 0.9403443, 0.9253786, 0.9185229, 0.9, 0.8261359];
        for (i, &(t, s)) in cases.iter().enumerate() {
            near(forgetting_curve(t, s, 0.1542), exp[i], 1e-6);
        }
        let f64_exp = [1.0, 0.9, 0.8458846447796301, 0.8093881028681906];
        for (t, exp) in f64_exp.iter().enumerate() {
            assert!((forgetting_curve(t as f64, 1.0, 0.1542) - exp).abs() < 5e-9);
        }
        let d2 = [1.0, 0.9, 0.84028935, 0.7985001];
        for (t, exp) in d2.iter().enumerate() {
            near(forgetting_curve(t as f64, 1.0, 0.2), *exp, 1e-6);
        }
        near(forgetting_curve(8.0, 8.2956, 0.1542), 0.9024733, 1e-6);
    }

    #[test]
    fn s7_4_curve_properties() {
        for decay in [0.1, 0.1542, 0.3, 0.5, 0.8] {
            for s in [0.01, 1.0, 10.0, 1000.0] {
                near(forgetting_curve(s, s, decay), 0.9, 1e-12);
            }
            let mut prev = 1.0000001;
            let mut t = 0.0;
            while t <= 5000.0 {
                let r = forgetting_curve(t, 5.0, decay);
                assert!(r < prev && r > 0.0 && r <= 1.0);
                prev = r;
                t += 37.0;
            }
        }
    }

    #[test]
    fn s7_5_interval_vs_desired_retention() {
        let got: Vec<i64> = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]
            .iter()
            .map(|&dr| (next_interval(1.0, dr, 0.1542).round() as i64).max(1))
            .collect();
        assert!(got[0] == 3116766 || got[0] == 3116769, "{}", got[0]);
        assert_eq!(&got[1..], &[34793, 2508, 387, 90, 27, 9, 3, 1, 1]);
        near(next_interval(121.01552, 0.9, 0.1542), 121.01551, 1e-6);
        let mut prev = 0.0;
        let mut s = 0.01;
        while s < 40000.0 {
            let i = next_interval(s, 0.9, 0.1542);
            assert!(i > prev);
            prev = i;
            s *= 1.3;
        }
    }

    const P19: &[f64] = &[
        0.6845422,
        1.6790825,
        4.7349424,
        10.042885,
        7.4410233,
        0.64219797,
        1.071918,
        0.0025195254,
        1.432437,
        0.1544,
        0.8692766,
        2.0696752,
        0.0953,
        0.2975,
        2.4691248,
        0.19542035,
        3.201072,
        0.18046261,
        0.121442534,
    ];

    #[test]
    fn s7_6_fsrs5_style_19_parameter_set() {
        let w = normalize_parameters(P19).unwrap();
        assert_eq!(w.len(), 21);
        assert_eq!(w[19], 0.0);
        assert_eq!(w[20], 0.5);
        let s = replay(&w, &[(1, 0), (3, 1), (3, 3), (3, 8), (3, 21)]);
        near(s.stability, 31.722992, 1e-4);
        near(s.difficulty, 7.382128, 1e-4);
        let n = next_states(&w, Some(MemoryState::new(20.925528, 7.005062)), 0.9, 21);
        near(n.good.memory.stability, 40.87456, 1e-4);
        near(n.good.memory.difficulty, 6.9913807, 1e-4);
        let st = replay(&w, &[(1, 0), (3, 1), (3, 3), (3, 8)]);
        let n = next_states(&w, Some(st), 0.9, 21);
        let exp = [(2.9691455, 8.000659), (17.091452, 7.6913934), (31.722992, 7.382128), (71.7502, 7.0728626)];
        for (i, x) in [n.again, n.hard, n.good, n.easy].iter().enumerate() {
            near(x.memory.stability, exp[i].0, 1e-4);
            near(x.memory.difficulty, exp[i].1, 1e-4);
            near(x.interval, x.memory.stability, 1e-9);
        }
    }

    #[test]
    fn s7_7_memory_state_from_sm2_vectors() {
        let cases = [
            (2.5, 10.0, 0.9, 10.0, 6.9140563),
            (2.5, 10.0, 0.8, 3.01572, 9.393428),
            (2.5, 10.0, 0.95, 24.841097, 1.2974405),
            (1.3, 20.0, 0.9, 20.0, 10.0),
        ];
        for (ease, ivl, r, s, d) in cases {
            let m = memory_state_from_sm2(W, ease, ivl, r);
            near(m.stability, s, 1e-4);
            near(m.difficulty, d, 1e-4);
        }
        let m = memory_state_from_sm2(W, 2.0, 15.0, 0.9);
        let after = next_recall_stability(W, m.stability, m.difficulty, 0.9, 3);
        assert!((after / 15.0 - 2.0).abs() < 0.01);
        let m = memory_state_from_sm2(W, 1.3, 100.0, 0.9);
        near(m.stability, 99.999954, 1e-6);
        assert!(m.difficulty > 9.0);
    }

    #[test]
    fn py_fsrs_repeated_easy_drives_difficulty_to_1() {
        let mut st = None;
        for _ in 0..10 {
            st = Some(step(W, st, 4, 0));
        }
        assert_eq!(st.unwrap().difficulty, 1.0);
    }

    #[test]
    fn parameters_17_to_21_upgrade() {
        let out = check_and_fill_parameters(&[
            0.4, 0.6, 2.4, 5.8, 4.93, 0.94, 0.86, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61,
        ])
        .unwrap();
        let exp = [
            0.4, 0.6, 2.4, 5.8, 6.81, 0.44675013, 1.36, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61, 0.0, 0.0,
            0.0, 0.5,
        ];
        assert_eq!(out.len(), 21);
        for (x, e) in out.iter().zip(exp.iter()) {
            near(*x, *e, 1e-6);
        }
    }

    #[test]
    fn parameters_19_to_21_defaults_and_errors() {
        assert_eq!(&check_and_fill_parameters(&[1.0; 19]).unwrap()[19..], &[0.0, 0.5]);
        assert_eq!(check_and_fill_parameters(&[]).unwrap(), W.to_vec());
        assert!(check_and_fill_parameters(&[1.0, 2.0, 3.0]).is_err());
        let mut bad = W[..20].to_vec();
        bad.push(f64::NAN);
        assert!(check_and_fill_parameters(&bad).is_err());
        assert_eq!(decay_from_params(&[]), 0.1542);
        assert_eq!(decay_from_params(&[1.0; 19]), 0.5);
        assert_eq!(decay_from_params(W), 0.1542);
    }

    #[test]
    fn parameters_clamping_bounds() {
        let lo = clamp_parameters_default(&[-100.0; 21]).unwrap();
        let hi = clamp_parameters_default(&[100.0; 21]).unwrap();
        assert_eq!(
            lo,
            vec![
                0.001, 0.001, 0.001, 0.001, 1.0, 0.001, 0.001, 0.001, 0.0, 0.0, 0.001, 0.001, 0.001, 0.001, 0.0, 0.0, 1.0, 0.0,
                0.0, 0.01, 0.1
            ]
        );
        assert_eq!(
            hi,
            vec![
                100.0, 100.0, 100.0, 100.0, 10.0, 4.0, 4.0, 0.75, 4.5, 0.8, 3.5, 5.0, 0.25, 0.9, 4.0, 1.0, 6.0, 2.0, 2.0, 0.8,
                0.8
            ]
        );
        assert_eq!(clamp_parameters(&[-100.0; 21], 1, false).unwrap()[19], 0.0);
        assert_eq!(clamp_parameters_default(W).unwrap(), W.to_vec());
    }

    #[test]
    fn w17_w18_ceiling_with_several_relearning_steps() {
        assert_eq!(w17w18_ceiling(W, 1), 2.0);
        let v = -(W[11].ln() + (2f64.powf(W[13]) - 1.0).ln() + 0.3 * W[14]) / 3.0;
        assert!((w17w18_ceiling(W, 3) - v.max(0.01).sqrt().min(2.0)).abs() < 1e-12);
        let mut p = W.to_vec();
        p[17] = 5.0;
        p[18] = 5.0;
        let c = clamp_parameters(&p, 3, true).unwrap();
        assert!((c[17] - w17w18_ceiling(W, 3)).abs() < 1e-12);
    }

    #[test]
    fn model_interface() {
        let m = Fsrs6::new(&[]).unwrap();
        assert_eq!(m.params, W.to_vec());
        assert!((m.retrievability(MemoryState::new(4.0, 5.0), 4.0) - 0.9).abs() < 1e-12);
        assert!((m.interval(MemoryState::new(4.0, 5.0), 0.9) - 4.0).abs() < 1e-9);
        assert!(Fsrs6::new(&[1.0, 2.0]).is_err());
    }
}
