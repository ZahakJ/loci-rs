//! FSRS-6 parameter optimizer and evaluation metrics (fsrs-rs v6.6.2 `compute_parameters` as
//! Anki calls it; research/fsrs-spec.md §6).
//!
//! Gradients are analytic forward-mode (a 21-wide tangent carried through every step of §1),
//! verified against finite differences in the tests. Everything here is plain synchronous code;
//! callers run it on a worker thread and get progress through the callback in
//! [`OptimizeOptions`].

use super::fsrs::{
    clamp, curve_factor, forgetting_curve, next_forget_stability, normalize_parameters, parameter_bounds, same_day_stability,
    DEFAULT_PARAMS_FSRS6, FSRS6_DEFAULT_DECAY, INIT_S_MAX, N_PARAMS, S_MAX, S_MIN,
};
use super::items::{long_term_review_count, FsrsItem};
use crate::error::Result;
use crate::types::Progress;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::collections::{BTreeMap, HashSet};

/// fsrs-rs PARAMS_STDDEV for the L2 prior.
pub const PARAMS_STDDEV: [f64; N_PARAMS] = [
    6.43, 9.66, 17.58, 27.85, 0.57, 0.28, 0.6, 0.12, 0.39, 0.18, 0.33, 0.3, 0.09, 0.16, 0.57, 0.25, 1.03, 0.31, 0.32, 0.14, 0.27,
];
pub const BATCH_SIZE: usize = 512;
pub const LEARNING_RATE: f64 = 4e-2;
/// Anki passes 8 epochs (fsrs-rs default is 5).
pub const DEFAULT_EPOCHS: usize = 8;
pub const MAX_SEQ_LEN: usize = 256;
/// fsrs-rs shuffles the batch order with `StdRng::seed_from_u64(2023)`.
pub const DEFAULT_SEED: u64 = 2023;
const GAMMA: f64 = 1.0;
const P_MIN: f64 = 1e-4;
const P_MAX: f64 = 1.0 - 1e-4;

/// A 21-wide parameter or gradient vector.
pub type Vec21 = [f64; N_PARAMS];

fn clip_in_place(w: &mut Vec21, num_relearning_steps: usize) {
    let bounds = parameter_bounds(w, num_relearning_steps, true);
    for (x, &(lo, hi)) in w.iter_mut().zip(bounds.iter()) {
        *x = clamp(*x, lo, hi);
    }
}

// ───────────────────────── prediction with gradient ─────────────────────────
/// Reusable tangent buffers for `predict_item`.
#[derive(Debug, Clone, Default)]
pub struct Scratch {
    gs: Vec21,
    gd: Vec21,
    ngs: Vec21,
    ngd: Vec21,
    gr: Vec21,
}

/// R(t, S) and its partials ∂R/∂S, ∂R/∂w20.
fn curve_with_grad(t: f64, s: f64, decay: f64) -> (f64, f64, f64) {
    let factor = curve_factor(decay);
    let u = 1.0 + factor * t / s;
    let r = u.powf(-decay);
    let d_r_d_s = r * decay * factor * t / (u * s * s);
    let d_f_d_w = (factor + 1.0) * 0.9f64.ln() / (decay * decay);
    let d_r_d_w = r * (-u.ln() - (decay / u) * (t / s) * d_f_d_w);
    (r, d_r_d_s, d_r_d_w)
}

/// Predicted recall probability for an item's last review (history = all but the last) and,
/// when `grad` is given, dR/dw written into it. Returns NaN for items with < 2 reviews.
pub fn predict_item(w: &[f64], item: &FsrsItem, grad: Option<&mut Vec21>, sc: &mut Scratch) -> f64 {
    let rv = &item.reviews;
    if rv.len() < 2 {
        return f64::NAN;
    }
    let with_grad = grad.is_some();
    let g0 = rv[0].rating.clamp(1, 4) as usize;
    let mut s = w[g0 - 1];
    if with_grad {
        sc.gs = [0.0; N_PARAMS];
        sc.gd = [0.0; N_PARAMS];
    }
    if !(S_MIN..=S_MAX).contains(&s) {
        s = clamp(s, S_MIN, S_MAX);
    } else if with_grad {
        sc.gs[g0 - 1] = 1.0;
    }
    let e0 = (w[5] * (g0 as f64 - 1.0)).exp();
    let mut d = w[4] - e0 + 1.0;
    if !(1.0..=10.0).contains(&d) {
        d = clamp(d, 1.0, 10.0);
    } else if with_grad {
        sc.gd[4] = 1.0;
        sc.gd[5] = -(g0 as f64 - 1.0) * e0;
    }
    let decay = w[20];
    for review in &rv[1..rv.len() - 1] {
        let g = review.rating as f64;
        let gi = review.rating;
        let t = review.delta_t as f64;
        if !(1..=4).contains(&gi) {
            continue;
        }
        let ns;
        if with_grad {
            sc.ngs = [0.0; N_PARAMS];
        }
        if review.delta_t == 0 {
            let sinc = (w[17] * (g - 3.0 + w[18])).exp() * s.powf(-w[19]);
            if gi >= 2 && sinc < 1.0 {
                ns = s;
                if with_grad {
                    sc.ngs = sc.gs;
                }
            } else {
                ns = s * sinc;
                if with_grad {
                    let k = (1.0 - w[19]) * ns / s;
                    for j in 0..N_PARAMS {
                        sc.ngs[j] = k * sc.gs[j];
                    }
                    sc.ngs[17] += (g - 3.0 + w[18]) * ns;
                    sc.ngs[18] += w[17] * ns;
                    sc.ngs[19] += -s.ln() * ns;
                }
            }
        } else {
            let (r, d_r_d_s, d_r_d_w) = curve_with_grad(t, s, decay);
            if with_grad {
                for j in 0..N_PARAMS {
                    sc.gr[j] = d_r_d_s * sc.gs[j];
                }
                sc.gr[20] += d_r_d_w;
            }
            if gi == 1 {
                let d_pow = d.powf(-w[12]);
                let s_pow = (s + 1.0).powf(w[13]);
                let ex = (w[14] * (1.0 - r)).exp();
                let sf = w[11] * d_pow * (s_pow - 1.0) * ex;
                let cap_div = (w[17] * w[18]).exp();
                let cap = s / cap_div;
                if sf <= cap {
                    ns = sf;
                    if with_grad {
                        let d_s = w[11] * d_pow * ex * w[13] * (s + 1.0).powf(w[13] - 1.0);
                        let d_d = -w[12] * sf / d;
                        let d_r = -w[14] * sf;
                        for j in 0..N_PARAMS {
                            sc.ngs[j] = d_s * sc.gs[j] + d_d * sc.gd[j] + d_r * sc.gr[j];
                        }
                        sc.ngs[11] += sf / w[11];
                        sc.ngs[12] += -d.ln() * sf;
                        sc.ngs[13] += w[11] * d_pow * ex * s_pow * (s + 1.0).ln();
                        sc.ngs[14] += (1.0 - r) * sf;
                    }
                } else {
                    ns = cap;
                    if with_grad {
                        for j in 0..N_PARAMS {
                            sc.ngs[j] = sc.gs[j] / cap_div;
                        }
                        sc.ngs[17] += -w[18] * cap;
                        sc.ngs[18] += -w[17] * cap;
                    }
                }
            } else {
                let ew8 = w[8].exp();
                let s_pow = s.powf(-w[9]);
                let e10 = (w[10] * (1.0 - r)).exp();
                let mult = (if gi == 2 { w[15] } else { 1.0 }) * (if gi == 4 { w[16] } else { 1.0 });
                let base = ew8 * (11.0 - d) * s_pow * (e10 - 1.0);
                let a = base * mult;
                ns = s * (a + 1.0);
                if with_grad {
                    let d_s = 1.0 + a * (1.0 - w[9]);
                    let d_d = -s * a / (11.0 - d);
                    let d_r = s * ew8 * (11.0 - d) * s_pow * mult * e10 * -w[10];
                    for j in 0..N_PARAMS {
                        sc.ngs[j] = d_s * sc.gs[j] + d_d * sc.gd[j] + d_r * sc.gr[j];
                    }
                    sc.ngs[8] += s * a;
                    sc.ngs[9] += -s.ln() * s * a;
                    sc.ngs[10] += s * ew8 * (11.0 - d) * s_pow * mult * e10 * (1.0 - r);
                    if gi == 2 {
                        sc.ngs[15] += s * base;
                    }
                    if gi == 4 {
                        sc.ngs[16] += s * base;
                    }
                }
            }
        }
        let mut ns = ns;
        if !(S_MIN..=S_MAX).contains(&ns) {
            ns = clamp(ns, S_MIN, S_MAX);
            if with_grad {
                sc.ngs = [0.0; N_PARAMS];
            }
        }
        // difficulty
        let delta = -w[6] * (g - 3.0);
        let dp = d + delta * (10.0 - d) / 9.0;
        let e3 = (3.0 * w[5]).exp();
        let d04 = w[4] - e3 + 1.0;
        let mut nd = w[7] * d04 + (1.0 - w[7]) * dp;
        if with_grad {
            let k = (1.0 - w[7]) * (1.0 - delta / 9.0);
            for j in 0..N_PARAMS {
                sc.ngd[j] = k * sc.gd[j];
            }
            sc.ngd[6] += (1.0 - w[7]) * -(g - 3.0) * (10.0 - d) / 9.0;
            sc.ngd[7] += d04 - dp;
            sc.ngd[4] += w[7];
            sc.ngd[5] += w[7] * -3.0 * e3;
        }
        if !(1.0..=10.0).contains(&nd) {
            nd = clamp(nd, 1.0, 10.0);
            if with_grad {
                sc.ngd = [0.0; N_PARAMS];
            }
        }
        s = ns;
        d = nd;
        if with_grad {
            std::mem::swap(&mut sc.gs, &mut sc.ngs);
            std::mem::swap(&mut sc.gd, &mut sc.ngd);
        }
    }
    let t_last = rv[rv.len() - 1].delta_t as f64;
    let (r, d_r_d_s, d_r_d_w) = curve_with_grad(t_last, s, decay);
    if let Some(grad) = grad {
        for (j, g) in grad.iter_mut().enumerate() {
            *g = d_r_d_s * sc.gs[j];
        }
        grad[20] += d_r_d_w;
    }
    r
}

fn label(item: &FsrsItem) -> f64 {
    if item.reviews.last().map(|r| r.rating > 1).unwrap_or(false) {
        1.0
    } else {
        0.0
    }
}

fn bce(p: f64, y: f64) -> f64 {
    let q = clamp(p, P_MIN, P_MAX);
    -(y * q.ln() + (1.0 - y) * (1.0 - q).ln())
}

/// fsrs-rs recency weights for N chronologically ordered items: 0.25 + 0.75·(i/(N−1))³.
pub fn recency_weights(n: usize) -> Vec<f64> {
    (0..n).map(|i| if n == 1 { 1.0 } else { 0.25 + 0.75 * (i as f64 / (n - 1) as f64).powi(3) }).collect()
}

/// Weighted mean BCE over `idx` plus the L2 prior, and its gradient (written into `grad`).
/// Exposed for gradient checks.
pub fn batch_loss_and_grad(
    w: &[f64],
    items: &[FsrsItem],
    weights: &[f64],
    idx: &[usize],
    init: &[f64],
    total_size: usize,
    mut grad: Option<&mut Vec21>,
) -> f64 {
    let mut sc = Scratch::default();
    let mut g = [0.0; N_PARAMS];
    if let Some(gr) = grad.as_deref_mut() {
        *gr = [0.0; N_PARAMS];
    }
    let mut loss = 0.0;
    let mut wsum = 0.0;
    for &i in idx {
        let item = &items[i];
        let wt = weights[i];
        let p = predict_item(w, item, if grad.is_some() { Some(&mut g) } else { None }, &mut sc);
        let y = label(item);
        loss += wt * bce(p, y);
        wsum += wt;
        if let Some(gr) = grad.as_deref_mut() {
            if p > P_MIN && p < P_MAX {
                let d_l_d_p = -(y / p) + (1.0 - y) / (1.0 - p);
                for j in 0..N_PARAMS {
                    gr[j] += wt * d_l_d_p * g[j];
                }
            }
        }
    }
    if wsum == 0.0 {
        return 0.0;
    }
    loss /= wsum;
    if let Some(gr) = grad.as_deref_mut() {
        for x in gr.iter_mut() {
            *x /= wsum;
        }
    }
    let scale = GAMMA * idx.len() as f64 / total_size as f64;
    for j in 0..N_PARAMS {
        let sd2 = PARAMS_STDDEV[j] * PARAMS_STDDEV[j];
        let diff = w[j] - init[j];
        loss += scale * diff * diff / sd2;
        if let Some(gr) = grad.as_deref_mut() {
            gr[j] += scale * 2.0 * diff / sd2;
        }
    }
    loss
}

// ───────────────────────── data preparation (§6.1 steps 2–4) ─────────────────────────
fn first_long_term_delta_t(item: &FsrsItem) -> i64 {
    item.reviews.iter().enumerate().find(|(i, r)| *i > 0 && r.delta_t > 0).map(|(_, r)| r.delta_t).unwrap_or(0)
}

/// fsrs-rs outlier filter. Returns `(pretrain, train)`: the filtered pretraining set (items with
/// exactly one long-term review) and the training set without items whose (first rating, first
/// long-term Δt) group was removed.
pub fn filter_outliers(items: &[FsrsItem]) -> (Vec<FsrsItem>, Vec<FsrsItem>) {
    let pre: Vec<usize> = (0..items.len()).filter(|&i| long_term_review_count(&items[i]) == 1).collect();
    let mut groups: BTreeMap<u8, BTreeMap<i64, Vec<usize>>> = BTreeMap::new();
    for &i in &pre {
        let it = &items[i];
        let r = it.reviews[0].rating;
        groups.entry(r).or_default().entry(first_long_term_delta_t(it)).or_default().push(i);
    }
    let mut removed: BTreeMap<u8, HashSet<i64>> = BTreeMap::new();
    let mut kept: HashSet<usize> = HashSet::new();
    for (rating, by_dt) in &groups {
        let mut subs: Vec<(i64, &Vec<usize>)> = by_dt.iter().map(|(dt, l)| (*dt, l)).collect();
        subs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(b.0.cmp(&a.0)));
        let total: usize = subs.iter().map(|(_, l)| l.len()).sum();
        let threshold = (total / 20).max(20);
        let mut rm = HashSet::new();
        let mut has_been_removed = 0;
        for (dt, list) in subs.iter().rev() {
            if has_been_removed + list.len() >= threshold {
                if list.len() >= 6 && *dt <= (if *rating != 4 { 100 } else { 365 }) {
                    kept.extend(list.iter().copied());
                } else {
                    rm.insert(*dt);
                }
            } else {
                has_been_removed += list.len();
                rm.insert(*dt);
            }
        }
        removed.insert(*rating, rm);
    }
    let pretrain = pre.iter().filter(|i| kept.contains(i)).map(|&i| items[i].clone()).collect();
    let train = items
        .iter()
        .filter(|it| !removed.get(&it.reviews[0].rating).map(|rm| rm.contains(&first_long_term_delta_t(it))).unwrap_or(false))
        .cloned()
        .collect();
    (pretrain, train)
}

/// fsrs-rs `smooth_and_fill`: monotone S0 across ratings and geometric fill of missing ones.
pub fn smooth_and_fill(stability: &BTreeMap<u8, f64>, counts: &BTreeMap<u8, i64>) -> Option<[f64; 4]> {
    let mut st = stability.clone();
    for (a, b) in [(1u8, 2u8), (2, 3), (3, 4), (1, 3), (2, 4), (1, 4)] {
        if let (Some(&sa), Some(&sb)) = (st.get(&a), st.get(&b)) {
            if sa > sb {
                if counts.get(&a).copied().unwrap_or(0) > counts.get(&b).copied().unwrap_or(0) {
                    st.insert(b, sa);
                } else {
                    st.insert(a, sb);
                }
            }
        }
    }
    let (w1, w2) = (0.41, 0.54);
    let def = &DEFAULT_PARAMS_FSRS6;
    if st.is_empty() {
        return None;
    }
    let out: Vec<f64> = if st.len() == 1 {
        let (&rating, &value) = st.iter().next()?;
        let f = value / def[rating as usize - 1];
        (0..4).map(|i| def[i] * f).collect()
    } else {
        // Log-space relations: ln r2 = w1·ln r1 + (1−w1)·ln r3 ;  ln r3 = (1−w2)·ln r2 + w2·ln r4.
        let mut l: BTreeMap<u8, f64> = st.iter().map(|(k, v)| (*k, v.ln())).collect();
        let has = |l: &BTreeMap<u8, f64>, k: u8| l.contains_key(&k);
        let g = |l: &BTreeMap<u8, f64>, k: u8| l.get(&k).copied().unwrap_or(0.0);
        if !has(&l, 1) && !has(&l, 2) {
            let v2 = (g(&l, 3) - w2 * g(&l, 4)) / (1.0 - w2);
            l.insert(2, v2);
            let v1 = (g(&l, 2) - (1.0 - w1) * g(&l, 3)) / w1;
            l.insert(1, v1);
        } else if !has(&l, 1) && !has(&l, 3) {
            let v3 = (1.0 - w2) * g(&l, 2) + w2 * g(&l, 4);
            l.insert(3, v3);
            let v1 = (g(&l, 2) - (1.0 - w1) * g(&l, 3)) / w1;
            l.insert(1, v1);
        } else if !has(&l, 1) && !has(&l, 4) {
            let v4 = (g(&l, 3) - (1.0 - w2) * g(&l, 2)) / w2;
            l.insert(4, v4);
            let v1 = (g(&l, 2) - (1.0 - w1) * g(&l, 3)) / w1;
            l.insert(1, v1);
        } else if !has(&l, 2) && !has(&l, 3) {
            let den = w1 + w2 - w1 * w2;
            let v2 = (w1 / den) * g(&l, 1) + ((1.0 - w1) * w2 / den) * g(&l, 4);
            l.insert(2, v2);
            let v3 = (1.0 - w2) * g(&l, 2) + w2 * g(&l, 4);
            l.insert(3, v3);
        } else if !has(&l, 2) && !has(&l, 4) {
            let v2 = w1 * g(&l, 1) + (1.0 - w1) * g(&l, 3);
            l.insert(2, v2);
            let v4 = (g(&l, 3) - (1.0 - w2) * g(&l, 2)) / w2;
            l.insert(4, v4);
        } else if !has(&l, 3) && !has(&l, 4) {
            let v3 = (g(&l, 2) - w1 * g(&l, 1)) / (1.0 - w1);
            l.insert(3, v3);
            let v4 = (g(&l, 3) - (1.0 - w2) * g(&l, 2)) / w2;
            l.insert(4, v4);
        } else if !has(&l, 1) {
            let v = (g(&l, 2) - (1.0 - w1) * g(&l, 3)) / w1;
            l.insert(1, v);
        } else if !has(&l, 2) {
            let v = w1 * g(&l, 1) + (1.0 - w1) * g(&l, 3);
            l.insert(2, v);
        } else if !has(&l, 3) {
            let v = (1.0 - w2) * g(&l, 2) + w2 * g(&l, 4);
            l.insert(3, v);
        } else if !has(&l, 4) {
            let v = (g(&l, 3) - (1.0 - w2) * g(&l, 2)) / w2;
            l.insert(4, v);
        }
        (1..=4u8).map(|k| g(&l, k).exp()).collect()
    };
    let mut res = [0.0; 4];
    for (i, x) in out.iter().enumerate() {
        res[i] = clamp(*x, S_MIN, INIT_S_MAX);
    }
    Some(res)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PretrainResult {
    pub s0: [f64; 4],
    pub rating_count: BTreeMap<u8, i64>,
}

/// fsrs-rs `pretrain`: per first rating, ternary-search S0 minimising count-weighted BCE of
/// Laplace-smoothed recall vs R(t, S) (default decay) + |S − default|/16. `None` when no data.
pub fn pretrain(items: &[FsrsItem], average_recall: f64) -> Option<PretrainResult> {
    struct Agg {
        pass: f64,
        n: f64,
    }
    let mut groups: BTreeMap<u8, BTreeMap<i64, Agg>> = BTreeMap::new();
    for it in items {
        let r = it.reviews[0].rating;
        let last = it.reviews[it.reviews.len() - 1];
        let agg = groups.entry(r).or_default().entry(last.delta_t).or_insert(Agg { pass: 0.0, n: 0.0 });
        agg.pass += if last.rating > 1 { 1.0 } else { 0.0 };
        agg.n += 1.0;
    }
    let mut stability: BTreeMap<u8, f64> = BTreeMap::new();
    let mut counts: BTreeMap<u8, i64> = BTreeMap::new();
    for (rating, by_dt) in &groups {
        let data: Vec<(f64, f64, f64)> =
            by_dt.iter().map(|(dt, a)| (*dt as f64, a.n, ((a.pass / a.n) * a.n + average_recall) / (a.n + 1.0))).collect();
        counts.insert(*rating, data.iter().map(|d| d.1).sum::<f64>() as i64);
        let def = DEFAULT_PARAMS_FSRS6[*rating as usize - 1];
        let loss = |s: f64| -> f64 {
            let mut l = 0.0;
            for &(dt, n, recall) in &data {
                let p = clamp(forgetting_curve(dt, s, FSRS6_DEFAULT_DECAY), 1e-7, 1.0 - 1e-7);
                l += -(recall * p.ln() + (1.0 - recall) * (1.0 - p).ln()) * n;
            }
            l + (s - def).abs() / 16.0
        };
        let (mut lo, mut hi, mut best) = (S_MIN, INIT_S_MAX, def);
        let mut iter = 0;
        while hi - lo > f64::EPSILON && iter < 1000 {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            if loss(m1) < loss(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
            best = (hi + lo) / 2.0;
            iter += 1;
        }
        stability.insert(*rating, best);
    }
    let s0 = smooth_and_fill(&stability, &counts)?;
    Some(PretrainResult { s0, rating_count: counts })
}

// ───────────────────────── training ─────────────────────────
/// Knobs and callbacks for a run. Callbacks are plain `FnMut`s; the caller owns the thread.
#[derive(Default)]
pub struct OptimizeOptions<'a> {
    /// Called after pretraining and after every epoch (`phase: "optimise"`, `total = epochs + 1`).
    pub progress: Option<&'a mut dyn FnMut(Progress)>,
    /// Polled between epochs; returning true stops and returns `cancelled: true`.
    pub should_cancel: Option<&'a mut dyn FnMut() -> bool>,
    /// Default 8.
    pub epochs: Option<usize>,
    /// Relearning steps whose cumulative length stays under one day (clipping of w17/w18). Default 1.
    pub num_relearning_steps: Option<usize>,
    /// Shuffle seed (fsrs-rs uses 2023).
    pub seed: Option<u64>,
}

/// What `run` did: defaults (< 8 items), pretrain only (< 64 items or no second long-term review), or full Adam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizeMode {
    Defaults,
    Pretrain,
    Full,
}

impl OptimizeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            OptimizeMode::Defaults => "defaults",
            OptimizeMode::Pretrain => "pretrain",
            OptimizeMode::Full => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptimizeResult {
    /// 21 parameters (defaults when there is too little data).
    pub params: Vec<f64>,
    pub mode: OptimizeMode,
    /// Training items after the outlier filter.
    pub train_items: usize,
    pub cancelled: bool,
    /// Mean training loss per epoch.
    pub loss_history: Vec<f64>,
}

fn chronological(items: &[FsrsItem]) -> Vec<FsrsItem> {
    let mut out = items.to_vec();
    if out.iter().all(|it| it.last_review_ms.is_some()) {
        out.sort_by_key(|it| it.last_review_ms.unwrap_or(0));
    }
    out
}

/// Optimises FSRS-6 parameters on `items` (fsrs-rs pipeline). `current` is not used as the
/// starting point (fsrs-rs starts from the defaults with pretrained w0–w3); it is only what a
/// cancelled run returns. See [`optimise`] for Anki's acceptance test against the current set.
pub fn run(items: &[FsrsItem], current: &[f64], opts: &mut OptimizeOptions<'_>) -> OptimizeResult {
    let epochs = opts.epochs.unwrap_or(DEFAULT_EPOCHS);
    let num_relearning = opts.num_relearning_steps.unwrap_or(1);
    let all: Vec<FsrsItem> = chronological(items)
        .into_iter()
        .filter(|it| {
            it.reviews.len() >= 2 && it.reviews.len() <= MAX_SEQ_LEN && it.reviews.iter().all(|r| (1..=4).contains(&r.rating))
        })
        .collect();
    let (init_set, train) = filter_outliers(&all);
    let defaults = DEFAULT_PARAMS_FSRS6.to_vec();
    let result = |params: Vec<f64>, mode: OptimizeMode, n: usize, cancelled: bool, loss_history: Vec<f64>| OptimizeResult {
        params,
        mode,
        train_items: n,
        cancelled,
        loss_history,
    };
    if train.len() < 8 {
        return result(defaults, OptimizeMode::Defaults, train.len(), false, vec![]);
    }
    let average_recall = train.iter().map(label).sum::<f64>() / train.len() as f64;
    let Some(pre) = pretrain(&init_set, average_recall) else {
        return result(defaults, OptimizeMode::Defaults, train.len(), false, vec![]);
    };
    let mut init = DEFAULT_PARAMS_FSRS6;
    init[..4].copy_from_slice(&pre.s0);
    clip_in_place(&mut init, num_relearning);
    let mut progress = |done: i64| {
        if let Some(p) = opts.progress.as_deref_mut() {
            p(Progress { phase: "optimise".to_string(), done, total: epochs as i64 + 1 });
        }
    };
    progress(1);
    if train.len() == init_set.len() || train.len() < 64 {
        return result(init.to_vec(), OptimizeMode::Pretrain, train.len(), false, vec![]);
    }
    let n = train.len();
    let weights = recency_weights(n);
    let batches_per_epoch = n.div_ceil(BATCH_SIZE);
    let t_max = ((n / BATCH_SIZE + 1) * epochs) as f64;
    let mut rng = StdRng::seed_from_u64(opts.seed.unwrap_or(DEFAULT_SEED));
    let mut w = init;
    let mut m = [0.0; N_PARAMS];
    let mut v = [0.0; N_PARAMS];
    let mut grad = [0.0; N_PARAMS];
    let (beta1, beta2, eps): (f64, f64, f64) = (0.9, 0.999, 1e-8);
    let mut step_count = 0u32;
    let mut order: Vec<usize> = (0..n).collect();
    let mut loss_history = Vec::new();
    for epoch in 0..epochs {
        if let Some(c) = opts.should_cancel.as_deref_mut() {
            if c() {
                return result(current.to_vec(), OptimizeMode::Full, n, true, loss_history);
            }
        }
        for i in (1..n).rev() {
            let j = rng.random_range(0..=i);
            order.swap(i, j);
        }
        let mut epoch_loss = 0.0;
        for b in 0..batches_per_epoch {
            let idx = &order[b * BATCH_SIZE..((b + 1) * BATCH_SIZE).min(n)];
            epoch_loss += batch_loss_and_grad(&w, &train, &weights, idx, &init, n, Some(&mut grad)) * idx.len() as f64;
            let lr = 0.5 * LEARNING_RATE * (1.0 + (std::f64::consts::PI * step_count as f64 / t_max).cos());
            step_count += 1;
            let bc1 = 1.0 - beta1.powi(step_count as i32);
            let bc2 = 1.0 - beta2.powi(step_count as i32);
            for j in 0..N_PARAMS {
                let gj = if grad[j].is_finite() { grad[j] } else { 0.0 };
                m[j] = beta1 * m[j] + (1.0 - beta1) * gj;
                v[j] = beta2 * v[j] + (1.0 - beta2) * gj * gj;
                w[j] -= lr * (m[j] / bc1) / ((v[j] / bc2).sqrt() + eps);
            }
            clip_in_place(&mut w, num_relearning);
        }
        loss_history.push(epoch_loss / n as f64);
        progress(epoch as i64 + 2);
    }
    let optimised: BTreeMap<u8, f64> = (1..=4u8).map(|r| (r, w[r as usize - 1])).collect();
    let counts: BTreeMap<u8, i64> = (1..=4u8).map(|r| (r, pre.rating_count.get(&r).copied().unwrap_or(0))).collect();
    let s0 = smooth_and_fill(&optimised, &counts).unwrap_or([w[0], w[1], w[2], w[3]]);
    let mut params = w;
    params[..4].copy_from_slice(&s0);
    clip_in_place(&mut params, num_relearning);
    result(params.to_vec(), OptimizeMode::Full, n, false, loss_history)
}

// ───────────────────────── evaluation (§6.2) ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Metrics {
    pub log_loss: f64,
    pub rmse: f64,
    pub items: usize,
}

/// fsrs-rs `r_matrix_index` bin for an item (RMSE(bins)): (delta_t bin, length bin, lapse bin).
pub fn rmse_bin(item: &FsrsItem) -> (i64, i64, i64) {
    let rv = &item.reviews;
    let dt = rv[rv.len() - 1].delta_t;
    let history = &rv[..rv.len() - 1];
    let long_term = history.iter().filter(|r| r.delta_t > 0).count() as f64;
    let lapses = history.iter().filter(|r| r.delta_t > 0 && r.rating == 1).count() as f64;
    let log_base = |x: f64, b: f64| x.ln() / b.ln();
    let dt_bin = if dt > 0 { (2.48 * 3.62f64.powf(log_base(dt as f64, 3.62).floor()) * 100.0).round() as i64 } else { 0 };
    let len_bin = (1.99 * 1.89f64.powf(log_base(long_term + 1.0, 1.89).floor())).round() as i64;
    let lapse_bin = if lapses == 0.0 { 0 } else { (1.65 * 1.73f64.powf(log_base(lapses, 1.73).floor())).round() as i64 };
    (dt_bin, len_bin, lapse_bin)
}

/// Recency-weighted log loss and RMSE(bins) of `params` on `items` (chronological order assumed).
pub fn evaluate(items: &[FsrsItem], params: &[f64]) -> Result<Metrics> {
    let w = normalize_parameters(params)?;
    let list: Vec<FsrsItem> = chronological(items).into_iter().filter(|it| it.reviews.len() >= 2).collect();
    if list.is_empty() {
        return Ok(Metrics::default());
    }
    let weights = recency_weights(list.len());
    let mut sc = Scratch::default();
    let mut loss = 0.0;
    let mut wsum = 0.0;
    struct Bin {
        p: f64,
        y: f64,
        n: f64,
        w: f64,
    }
    let mut bins: BTreeMap<(i64, i64, i64), Bin> = BTreeMap::new();
    for (i, it) in list.iter().enumerate() {
        let p = predict_item(&w, it, None, &mut sc);
        let y = label(it);
        let wt = weights[i];
        loss += wt * bce(p, y);
        wsum += wt;
        let b = bins.entry(rmse_bin(it)).or_insert(Bin { p: 0.0, y: 0.0, n: 0.0, w: 0.0 });
        b.p += p;
        b.y += y;
        b.n += 1.0;
        b.w += wt;
    }
    let mut num = 0.0;
    let mut den = 0.0;
    for b in bins.values() {
        num += b.w * (b.p / b.n - b.y / b.n).powi(2);
        den += b.w;
    }
    Ok(Metrics { log_loss: loss / wsum, rmse: (num / den).sqrt(), items: list.len() })
}

/// Anki's guard for keeping current params when several relearning steps exist: true when
/// repeated same-day Good presses after a lapse (S = 1, D = 1, R = 0.7) lift S above 1.
pub fn relearning_lifts_stability(params: &[f64], num_relearning_steps: usize) -> Result<bool> {
    let w = normalize_parameters(params)?;
    let mut s = next_forget_stability(&w, 1.0, 1.0, 0.7);
    for _ in 0..num_relearning_steps {
        s = same_day_stability(&w, s, 3);
    }
    Ok(s > 1.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptimiseOutcome {
    pub params: Vec<f64>,
    pub adopted: bool,
    pub reason: String,
    pub mode: OptimizeMode,
    pub cancelled: bool,
    pub before: Metrics,
    pub after: Metrics,
}

/// `run` + Anki's acceptance test: keep the current set unless the new one has lower log loss.
pub fn optimise(items: &[FsrsItem], current: &[f64], opts: &mut OptimizeOptions<'_>) -> Result<OptimiseOutcome> {
    let res = run(items, current, opts);
    let current_full = normalize_parameters(current)?;
    let before = evaluate(items, &current_full)?;
    if res.cancelled {
        return Ok(OptimiseOutcome {
            params: current_full,
            adopted: false,
            reason: "cancelled".into(),
            mode: res.mode,
            cancelled: true,
            before,
            after: before,
        });
    }
    let after = evaluate(items, &res.params)?;
    let n_relearn = opts.num_relearning_steps.unwrap_or(1);
    if before.log_loss <= after.log_loss && (n_relearn <= 1 || !relearning_lifts_stability(&current_full, n_relearn)?) {
        return Ok(OptimiseOutcome {
            params: current_full,
            adopted: false,
            reason: "current parameters already fit as well or better".into(),
            mode: res.mode,
            cancelled: false,
            before,
            after,
        });
    }
    let same = res.params.iter().zip(current_full.iter()).all(|(x, c)| (x - c).abs() < 5e-5);
    Ok(OptimiseOutcome {
        params: res.params,
        adopted: !same,
        reason: if same { "parameters appear optimal".into() } else { format!("optimised ({})", res.mode.as_str()) },
        mode: res.mode,
        cancelled: false,
        before,
        after,
    })
}
