//! Workload forecast (internals §7): a deterministic
//! expected-value simulation of future answers per day, used by stats and the retention hint.

use super::fsrs::{Fsrs6, Grade, MemoryState};
use crate::error::Result;
use crate::types::{Card, CardQueue, CardType, CollectionDay, DeckConfig, SchedulerKind};
use std::collections::BTreeMap;

/// fsrs-rs simulator default first-rating probabilities (Again..Easy).
pub const DEFAULT_FIRST_RATING_PROB: [f64; 4] = [0.24, 0.094, 0.495, 0.171];

#[derive(Debug, Clone)]
pub struct WorkloadInput<'a> {
    pub cards: &'a [Card],
    pub config: &'a DeckConfig,
    /// Horizon, days (day 0 = today).
    pub days: usize,
    pub today: CollectionDay,
    /// Probability of each first rating for new cards (Again..Easy); fsrs-rs simulator default.
    pub first_rating_prob: Option<[f64; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadResult {
    /// Expected answers per day (reviews + learning/relearning steps).
    pub reviews_per_day: Vec<f64>,
    /// New cards introduced per day.
    pub new_per_day: Vec<f64>,
}

#[derive(Debug, Clone)]
struct Branch {
    mass: f64,
    s: f64,
    d: f64,
    ivl: i64,
    ease: f64,
    due: i64,
    last: i64,
}

/// Merge key: (last, S to 2 dp, D to 2 dp, ivl, ease to 2 dp)
type BranchKey = (i64, i64, i64, i64, i64);
/// Pending branches per due day.
type Pending = Vec<BTreeMap<BranchKey, Branch>>;

fn key(b: &Branch) -> BranchKey {
    (b.last, (b.s * 100.0).round() as i64, (b.d * 100.0).round() as i64, b.ivl, (b.ease * 100.0).round() as i64)
}

/// Deterministic expected-value forecast: each card's future is a probability tree (pass with R,
/// lapse with 1 − R, R from the FSRS curve) instead of Monte-Carlo draws. Branches below 2% mass
/// stop splitting (they follow the pass path, lapse steps still counted). Passing reviews are
/// modelled as Good; daily review limits are not applied. SM-2 presets schedule with SM-2
/// formulas and use default FSRS to estimate recall.
pub fn simulate_workload(input: &WorkloadInput<'_>) -> Result<WorkloadResult> {
    let cfg = input.config;
    let days = input.days;
    let today = input.today;
    let fsrs_on = cfg.scheduler == SchedulerKind::Fsrs;
    let model = Fsrs6::new(if fsrs_on { &cfg.fsrs_params } else { &[] })?;
    let dr = cfg.desired_retention;
    let max_ivl = cfg.maximum_interval.max(1);
    let mut reviews = vec![0.0; days];
    let mut new_per_day = vec![0.0; days];
    let n_learn = cfg.learn_steps.iter().filter(|m| **m > 0.0).count() as i64;
    let n_relearn = cfg.relearn_steps.iter().filter(|m| **m > 0.0).count() as i64;
    let fsrs_ivl = |s: f64| ((model.interval(MemoryState::new(s, 5.0), dr).round() as i64).max(1)).min(max_ivl);
    // Pending branches per due day, merged when (last, S, D, ivl, ease) coincide.
    let mut pending: Pending = (0..days).map(|_| BTreeMap::new()).collect();
    let push = |pending: &mut Pending, b: Branch| {
        if b.due >= days as i64 || b.due < 0 || b.mass <= 0.0 {
            return;
        }
        let bucket = &mut pending[b.due as usize];
        let k = key(&b);
        match bucket.get_mut(&k) {
            Some(ex) => ex.mass += b.mass,
            None => {
                bucket.insert(k, b);
            }
        }
    };
    let graduate = |pending: &mut Pending, mass: f64, mem: MemoryState, day: i64, sm2_ivl: i64, ease: f64| {
        let ivl = if fsrs_on { fsrs_ivl(mem.stability) } else { sm2_ivl.max(1).min(max_ivl) };
        push(pending, Branch { mass, s: mem.stability, d: mem.difficulty, ivl, ease, due: day + ivl, last: day });
    };
    // existing cards
    for c in input.cards {
        if c.queue == CardQueue::Suspended || c.ctype == CardType::New {
            continue;
        }
        let buried = c.queue == CardQueue::SchedBuried || c.queue == CardQueue::UserBuried;
        let ease = (if c.factor > 0 { c.factor } else { cfg.initial_ease }) as f64 / 1000.0;
        let mem = match (c.data.s, c.data.d) {
            (Some(s), Some(d)) => MemoryState::new(s, d),
            _ if c.ctype == CardType::Review || c.ctype == CardType::Relearn => {
                model.from_sm2(ease, c.ivl.max(1) as f64, cfg.historical_retention)
            }
            _ => model.step(None, 3, 0),
        };
        if c.ctype == CardType::Learn || c.ctype == CardType::Relearn {
            let reps = (c.left % 1000).max(1);
            if days > 0 {
                reviews[0] += reps as f64;
            }
            let mut m = mem;
            for _ in 0..reps {
                m = model.step(Some(m), 3, 0);
            }
            let sm2_ivl = if c.ctype == CardType::Learn { cfg.graduating_interval } else { c.ivl };
            graduate(&mut pending, 1.0, m, 0, sm2_ivl, ease);
            continue;
        }
        let due_day = (c.due - today).max(if buried { 1 } else { 0 });
        push(
            &mut pending,
            Branch {
                mass: 1.0,
                s: mem.stability,
                d: mem.difficulty,
                ivl: c.ivl.max(1),
                ease,
                due: due_day,
                last: c.due - c.ivl - today,
            },
        );
    }
    // new cards
    let mut available = input.cards.iter().filter(|c| c.ctype == CardType::New && c.queue == CardQueue::New).count() as i64;
    let probs = input.first_rating_prob.unwrap_or(DEFAULT_FIRST_RATING_PROB);
    let mut day = 0usize;
    while day < days && available > 0 {
        let n = cfg.new_per_day.min(available);
        available -= n;
        new_per_day[day] = n as f64;
        for g in 1..=4u8 {
            let mass = n as f64 * probs[g as usize - 1];
            if mass <= 0.0 {
                continue;
            }
            let reps = if g == 4 || n_learn == 0 {
                1
            } else if g == 3 {
                n_learn
            } else {
                1 + n_learn
            };
            reviews[day] += mass * reps as f64;
            let mut m = model.step(None, g as Grade, 0);
            for _ in 1..reps {
                m = model.step(Some(m), 3, 0);
            }
            let sm2_ivl = if g == 4 { cfg.easy_interval } else { cfg.graduating_interval };
            graduate(&mut pending, mass, m, day as i64, sm2_ivl, cfg.initial_ease as f64 / 1000.0);
        }
        day += 1;
    }
    // process branches in due order
    for day in 0..days {
        let branches: Vec<Branch> = pending[day].values().cloned().collect();
        for b in branches {
            reviews[day] += b.mass;
            let t = (day as i64 - b.last).max(0);
            let mem = MemoryState::new(b.s, b.d);
            let r = model.retrievability(mem, t as f64);
            let pass = model.step(Some(mem), 3, t);
            let late = (t - b.ivl).max(0) as f64;
            let pass_ivl =
                (((b.ivl as f64 + late / 2.0) * b.ease * cfg.interval_modifier).round() as i64).max(b.ivl + 1).min(max_ivl);
            let mut fail = model.step(Some(mem), 1, t);
            for _ in 0..n_relearn {
                fail = model.step(Some(fail), 3, 0);
            }
            let fail_ivl = ((b.ivl as f64 * cfg.lapse_multiplier).round() as i64).max(cfg.minimum_lapse_interval);
            let fail_ease = (b.ease - 0.2).max(1.3);
            reviews[day] += b.mass * (1.0 - r) * n_relearn as f64;
            if b.mass < 0.02 {
                graduate(&mut pending, b.mass, pass, day as i64, pass_ivl, b.ease);
            } else {
                graduate(&mut pending, b.mass * r, pass, day as i64, pass_ivl, b.ease);
                graduate(&mut pending, b.mass * (1.0 - r), fail, day as i64, fail_ivl, fail_ease);
            }
        }
    }
    Ok(WorkloadResult { reviews_per_day: reviews, new_per_day })
}
