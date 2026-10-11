//! Port of `optimizer.test.ts` (optimizer, evaluation and workload parts; the revlog → item and
//! memory-state parts live next to `items.rs` / `convert.rs`).

use super::{make_card, make_config};
use crate::scheduler::fsrs::{forgetting_curve, next_interval, step, Grade, MemoryState, DEFAULT_PARAMS_FSRS6, N_PARAMS};
use crate::scheduler::fuzz::Mulberry32;
use crate::scheduler::items::{training_items_from_revlog, FsrsItem, FsrsReview};
use crate::scheduler::optimizer::*;
use crate::scheduler::workload::{simulate_workload, WorkloadInput};
use crate::types::*;
use std::collections::BTreeMap;

const DAY: i64 = 86_400_000;
const NEXT_DAY_AT: i64 = 86_400 * 1000 * 1000;
const W: [f64; 21] = DEFAULT_PARAMS_FSRS6;

fn item(pairs: &[(u8, i64)]) -> FsrsItem {
    FsrsItem::from_pairs(pairs)
}

/// Synthetic reviews from known parameters: intervals from DR 0.9 with ±50% noise, outcomes
/// ~ Bernoulli(R). Same mulberry32 call order as the TS `synth`, so the data sets are identical.
fn synth(w: &[f64], cards: usize, seed: u32, max_reviews: usize) -> Vec<FsrsItem> {
    let mut rng = Mulberry32::new(seed);
    let mut items = Vec::new();
    let mut clock = 0i64;
    for _ in 0..cards {
        let u = rng.next_f64();
        let g0: Grade = if u < 0.2 {
            1
        } else if u < 0.3 {
            2
        } else if u < 0.85 {
            3
        } else {
            4
        };
        let mut reviews = vec![FsrsReview::new(g0, 0)];
        let mut st = step(w, None, g0, 0);
        if g0 < 4 && rng.next_f64() < 0.5 {
            reviews.push(FsrsReview::new(3, 0));
            st = step(w, Some(st), 3, 0);
        }
        let n = 2 + (rng.next_f64() * (max_reviews - 1) as f64).floor() as usize;
        for _ in 0..n {
            let base = next_interval(st.stability, 0.9, w[20]).round().max(1.0);
            let t = (base * (0.5 + rng.next_f64())).round().max(1.0) as i64;
            let r = forgetting_curve(t as f64, st.stability, w[20]);
            let pass = rng.next_f64() < r;
            let v = rng.next_f64();
            let g: Grade = if pass {
                if v < 0.1 {
                    2
                } else if v < 0.9 {
                    3
                } else {
                    4
                }
            } else {
                1
            };
            reviews.push(FsrsReview::new(g, t));
            items.push(FsrsItem { reviews: reviews.clone(), last_review_ms: Some(clock), card_id: None });
            clock += 1;
            st = step(w, Some(st), g, t);
        }
    }
    items
}

fn truth() -> [f64; 21] {
    let mut t = W;
    t[0] = 0.5;
    t[1] = 1.0;
    t[2] = 4.0;
    t[3] = 15.0;
    t[8] = 1.6;
    t[20] = 0.3;
    t
}

#[test]
fn analytic_gradient_matches_finite_differences() {
    let items = synth(&truth(), 40, 3, 7);
    let weights = recency_weights(items.len());
    let idx: Vec<usize> = (0..items.len()).collect();
    let mut w = W;
    w[7] = 0.05;
    w[17] = 0.4;
    w[19] = 0.1; // move off the clamp edges
    let init: Vec<f64> = w.iter().map(|x| x * 1.01).collect();
    let mut grad = [0.0; N_PARAMS];
    batch_loss_and_grad(&w, &items, &weights, &idx, &init, items.len(), Some(&mut grad));
    for j in 0..N_PARAMS {
        let h = 1e-6 * w[j].abs().max(1.0);
        let mut wp = w;
        wp[j] += h;
        let mut wm = w;
        wm[j] -= h;
        let fd = (batch_loss_and_grad(&wp, &items, &weights, &idx, &init, items.len(), None)
            - batch_loss_and_grad(&wm, &items, &weights, &idx, &init, items.len(), None))
            / (2.0 * h);
        assert!((grad[j] - fd).abs() < 1e-5 + 1e-4 * fd.abs(), "w{j}: {} vs {fd}", grad[j]);
    }
}

#[test]
fn predict_item_equals_forgetting_curve_after_replay() {
    let it = item(&[(3, 0), (3, 0), (3, 2), (1, 9)]);
    let mut st: Option<MemoryState> = None;
    for r in &it.reviews[..it.reviews.len() - 1] {
        st = Some(step(&W, st, r.rating, r.delta_t));
    }
    let p = predict_item(&W, &it, None, &mut Scratch::default());
    assert!((p - forgetting_curve(9.0, st.unwrap().stability, 0.1542)).abs() < 1e-12);
}

#[test]
fn fewer_than_8_items_defaults_fewer_than_64_pretrain_only() {
    let few: Vec<FsrsItem> = synth(&truth(), 2, 5, 3).into_iter().take(5).collect();
    let r = run(&few, &W, &mut OptimizeOptions::default());
    assert_eq!(r.mode, OptimizeMode::Defaults);
    assert_eq!(r.params, W.to_vec());
    // 40 cards × 1 long-term review each, all in one big first-interval group per rating
    let mut items = Vec::new();
    for i in 0..60 {
        items.push(FsrsItem {
            reviews: vec![FsrsReview::new(3, 0), FsrsReview::new(if i % 5 != 0 { 3 } else { 1 }, 3)],
            last_review_ms: Some(i),
            card_id: None,
        });
    }
    for i in 0..30 {
        items.push(FsrsItem {
            reviews: vec![FsrsReview::new(1, 0), FsrsReview::new(if i % 3 != 0 { 3 } else { 1 }, 1)],
            last_review_ms: Some(100 + i),
            card_id: None,
        });
    }
    let res = run(&items, &W, &mut OptimizeOptions::default());
    assert_eq!(res.mode, OptimizeMode::Pretrain);
    assert_eq!(&res.params[4..], &W[4..]);
    assert_ne!(&res.params[..4], &W[..4]);
    for i in 0..3 {
        assert!(res.params[i] <= res.params[i + 1]);
    }
}

#[test]
fn pretraining_recovers_s0_from_first_interval_data() {
    let mut items = Vec::new();
    let mut rng = Mulberry32::new(9);
    for (g, s0) in [(1u8, 0.5), (3u8, 4.0)] {
        for t in [1i64, 2, 3, 4, 5, 7] {
            for _ in 0..400 {
                let pass = rng.next_f64() < forgetting_curve(t as f64, s0, 0.1542);
                items.push(item(&[(g, 0), (if pass { 3 } else { 1 }, t)]));
            }
        }
    }
    let avg = items.iter().filter(|i| i.reviews[1].rating > 1).count() as f64 / items.len() as f64;
    let res = pretrain(&items, avg).unwrap();
    assert!((res.s0[0] - 0.5).abs() / 0.5 < 0.2, "{}", res.s0[0]);
    assert!((res.s0[2] - 4.0).abs() / 4.0 < 0.15, "{}", res.s0[2]);
    assert!(res.s0[1] >= res.s0[0]);
    assert!(res.s0[3] >= res.s0[2]);
}

#[test]
fn full_training_loss_decreases_beats_defaults_recovers_parameters_loosely() {
    let t = truth();
    let items = synth(&t, 2500, 11, 7);
    let mut progress: Vec<i64> = Vec::new();
    let mut on_progress = |p: Progress| progress.push(p.done);
    let res = run(&items, &W, &mut OptimizeOptions { progress: Some(&mut on_progress), ..Default::default() });
    assert_eq!(res.mode, OptimizeMode::Full);
    assert!(res.loss_history.last().unwrap() < &res.loss_history[0]);
    assert_eq!(progress.last(), Some(&9));
    let fit = evaluate(&items, &res.params).unwrap();
    let def = evaluate(&items, &W).unwrap();
    let truth_m = evaluate(&items, &t).unwrap();
    assert!(fit.log_loss < def.log_loss, "{} vs {}", fit.log_loss, def.log_loss);
    assert!(fit.log_loss < truth_m.log_loss + 0.005);
    assert!(fit.rmse < def.rmse);
    assert!((res.params[2] - t[2]).abs() / t[2] < 0.35, "w2 {}", res.params[2]);
    assert!((res.params[3] - t[3]).abs() / t[3] < 0.5, "w3 {}", res.params[3]);
    assert!((res.params[0] - t[0]).abs() / t[0] < 0.35, "w0 {}", res.params[0]);
    assert!((res.params[1] - t[1]).abs() / t[1] < 0.35, "w1 {}", res.params[1]);
    // w8 and the decay w20 trade off against each other on this data (equal log loss), so they
    // are judged through the loss above rather than individually.
}

#[test]
fn optimise_adopts_better_params_and_keeps_current_ones_when_already_best() {
    let items = synth(&truth(), 800, 13, 7);
    let a = optimise(&items, &W, &mut OptimizeOptions::default()).unwrap();
    assert!(a.adopted);
    assert!(a.after.log_loss < a.before.log_loss);
    let b = optimise(&items, &a.params, &mut OptimizeOptions::default()).unwrap();
    assert_eq!(b.params, a.params);
}

#[test]
fn cancellation_between_epochs() {
    let items = synth(&truth(), 300, 17, 7);
    let mut cancel = || true;
    let res = run(&items, &W, &mut OptimizeOptions { should_cancel: Some(&mut cancel), ..Default::default() });
    assert!(res.cancelled);
    assert_eq!(res.params, W.to_vec());
}

#[test]
fn evaluate_rmse_bins_near_0_when_predictions_match_outcomes() {
    let t = truth();
    let items = synth(&t, 4000, 21, 4);
    let m = evaluate(&items, &t).unwrap();
    assert!(m.rmse < 0.02, "{}", m.rmse);
    assert!(m.log_loss > 0.0);
    assert_eq!(evaluate(&[], &t).unwrap(), Metrics::default());
}

#[test]
fn rmse_bins_follow_r_matrix_index() {
    assert_eq!(rmse_bin(&item(&[(3, 0), (3, 1)])), (248, 2, 0));
    assert_eq!(rmse_bin(&item(&[(3, 0), (1, 5), (3, 20)])), (3250, 4, 2));
}

#[test]
fn outlier_filter_removes_the_smallest_first_interval_groups() {
    let mut items = Vec::new();
    for _ in 0..100 {
        items.push(item(&[(3, 0), (3, 2)]));
    }
    for _ in 0..10 {
        items.push(item(&[(3, 0), (3, 9)]));
    }
    for _ in 0..5 {
        items.push(item(&[(3, 0), (3, 9), (3, 20)]));
    }
    let (pre, train) = filter_outliers(&items);
    assert_eq!(pre.len(), 100);
    assert_eq!(train.len(), 100);
}

#[test]
fn smooth_and_fill_vectors() {
    let one = smooth_and_fill(&BTreeMap::from([(3u8, 4.613)]), &BTreeMap::from([(3u8, 10)])).unwrap();
    for (i, x) in one.iter().enumerate() {
        assert!((x - W[i] * 2.0).abs() < 1e-9);
    }
    let filled = smooth_and_fill(&BTreeMap::from([(2u8, 1.0), (3u8, 3.0)]), &BTreeMap::from([(2u8, 5), (3u8, 5)])).unwrap();
    assert!((filled[0] - 1f64.powf(1.0 / 0.41) * 3f64.powf(1.0 - 1.0 / 0.41)).abs() < 1e-9);
    assert!((filled[3] - 1f64.powf(1.0 - 1.0 / 0.54) * 3f64.powf(1.0 / 0.54)).abs() < 1e-9);
    let mono = smooth_and_fill(
        &BTreeMap::from([(1u8, 5.0), (2, 1.0), (3, 3.0), (4, 9.0)]),
        &BTreeMap::from([(1u8, 1), (2, 10), (3, 10), (4, 10)]),
    )
    .unwrap();
    assert_eq!(mono[0], 1.0);
    assert!(smooth_and_fill(&BTreeMap::new(), &BTreeMap::new()).is_none());
}

/// Synthetic *revlog* (not items) from known parameters: the scheduler's own forgetting curve is
/// the ground truth. First ratings follow a realistic distribution (Again 15%, Hard 6%, Good 69%,
/// Easy 10%); each card graduates from its first answer, is reviewed at the FSRS interval (DR 0.9)
/// with ±40% scheduling noise (the first one ×0.5–2.5), and every outcome is a Bernoulli draw on R(t, S).
fn synth_revlog(w: &[f64], cards: usize, seed: u32, max_reviews: usize) -> Vec<RevlogEntry> {
    let mut rng = Mulberry32::new(seed);
    let mut revlog = Vec::new();
    let horizon = 400i64; // days of history
    let day_ms = |day: i64| NEXT_DAY_AT - (horizon - day) * DAY - DAY / 2;
    for c in 0..cards as i64 {
        let cid = c + 1;
        let u = rng.next_f64();
        let g0: Grade = if u < 0.15 {
            1
        } else if u < 0.21 {
            2
        } else if u < 0.9 {
            3
        } else {
            4
        };
        let mut day = (rng.next_f64() * 200.0).floor() as i64;
        revlog.push(RevlogEntry {
            id: day_ms(day) + c,
            cid,
            usn: 0,
            ease: g0 as i64,
            ivl: -600,
            last_ivl: 0,
            factor: 0,
            time: 3000,
            rtype: RevlogType::Learn,
        });
        let mut st = step(w, None, g0, 0);
        for k in 0..max_reviews {
            let base = next_interval(st.stability, 0.9, w[20]).round().max(1.0);
            // the first interval varies more (users study late, early, or change presets), which is
            // what makes per-rating S0 identifiable from first-interval recall curves
            let spread = if k == 0 { 0.5 + 2.0 * rng.next_f64() } else { 0.6 + 0.8 * rng.next_f64() };
            let t = (base * spread).round().max(1.0) as i64;
            if day + t >= horizon {
                break;
            }
            day += t;
            let pass = rng.next_f64() < forgetting_curve(t as f64, st.stability, w[20]);
            let v = rng.next_f64();
            let g: Grade = if pass {
                if v < 0.08 {
                    2
                } else if v < 0.92 {
                    3
                } else {
                    4
                }
            } else {
                1
            };
            revlog.push(RevlogEntry {
                id: day_ms(day) + c,
                cid,
                usn: 0,
                ease: g as i64,
                ivl: t,
                last_ivl: 0,
                factor: 2500,
                time: 3000,
                rtype: RevlogType::Review,
            });
            st = step(w, Some(st), g, t);
        }
    }
    revlog
}

fn revlog_truth() -> [f64; 21] {
    // distinct S0 per first rating; default decay so pretraining's fixed-decay curve is the truth
    let mut t = W;
    t[0] = 0.4;
    t[1] = 1.5;
    t[2] = 4.5;
    t[3] = 14.0;
    t
}

#[test]
fn pretraining_recovers_s0_for_each_first_rating_separately() {
    // first intervals only, many cards: S0 near 1 is weakly identified (R(1 day) moves little
    // with S), so the per-rating curves need a few thousand first answers to pin it down
    let t = revlog_truth();
    let big = synth_revlog(&t, 30000, 31, 1);
    let (init_set, train) = filter_outliers(&training_items_from_revlog(&big, NEXT_DAY_AT, 0));
    let avg = train.iter().filter(|it| it.reviews.last().unwrap().rating > 1).count() as f64 / train.len() as f64;
    let res = pretrain(&init_set, avg).unwrap();
    assert!(res.rating_count[&1] > 3000);
    assert!(res.rating_count[&2] > 1000);
    // each S0 within 25% of the truth, and strictly ordered (no collapsing by the monotone rule)
    for (i, (got, want)) in res.s0.iter().zip(t.iter()).enumerate() {
        assert!((got - want).abs() / want < 0.25, "w{i}: {got}");
    }
    for i in 0..3 {
        assert!(res.s0[i] < res.s0[i + 1]);
    }
}

#[test]
fn the_full_fit_beats_the_default_parameters_and_lands_near_the_truth() {
    let t = revlog_truth();
    let revlog = synth_revlog(&t, 6000, 29, 8);
    let items = training_items_from_revlog(&revlog, NEXT_DAY_AT, 0);
    let res = run(&items, &W, &mut OptimizeOptions::default());
    assert_eq!(res.mode, OptimizeMode::Full);
    let fit = evaluate(&items, &res.params).unwrap();
    let def = evaluate(&items, &W).unwrap();
    let truth_m = evaluate(&items, &t).unwrap();
    assert!(fit.log_loss < def.log_loss);
    assert!(fit.log_loss < truth_m.log_loss + 0.003, "{} vs {}", fit.log_loss, truth_m.log_loss);
    // S0 for Again, Good and Easy are well determined by this data set; Hard (6% of first answers,
    // all at 1–2 days) is not, so Hard is only checked to sit between its neighbours.
    for (i, tol) in [(0usize, 0.35), (2, 0.35), (3, 0.5)] {
        assert!((res.params[i] - t[i]).abs() / t[i] < tol, "w{i}: {}", res.params[i]);
    }
    for i in 0..3 {
        assert!(res.params[i] <= res.params[i + 1]);
    }
    let a = optimise(&items, &W, &mut OptimizeOptions::default()).unwrap();
    assert!(a.adopted);
    assert!(a.after.log_loss < a.before.log_loss);
}

#[test]
fn simulate_workload_deterministic_new_cards_at_new_per_day_higher_retention_more_reviews() {
    let today = 500;
    let mut cards: Vec<Card> = (0..200)
        .map(|i| Card {
            id: i + 1,
            ctype: CardType::Review,
            queue: CardQueue::Review,
            ivl: 10 + (i % 30),
            factor: 2500,
            due: today + (i % 20),
            data: CardData { s: Some((10 + (i % 30)) as f64), d: Some(5.0), ..CardData::default() },
            ..make_card()
        })
        .collect();
    cards.extend((0..100).map(|i| Card { id: 1000 + i, due: i, ..make_card() }));
    let cfg = DeckConfig { new_per_day: 20, ..make_config() };
    fn inp<'a>(cards: &'a [Card], config: &'a DeckConfig, today: i64) -> WorkloadInput<'a> {
        WorkloadInput { cards, config, days: 90, today, first_rating_prob: None }
    }
    let a = simulate_workload(&inp(&cards, &cfg, today)).unwrap();
    let b = simulate_workload(&inp(&cards, &cfg, today)).unwrap();
    assert_eq!(a, b);
    assert_eq!(&a.new_per_day[..6], &[20.0, 20.0, 20.0, 20.0, 20.0, 0.0]);
    let total = |r: &[f64]| r.iter().sum::<f64>();
    let hi = simulate_workload(&inp(&cards, &DeckConfig { desired_retention: 0.95, ..make_config() }, today)).unwrap();
    let lo = simulate_workload(&inp(&cards, &DeckConfig { desired_retention: 0.8, ..make_config() }, today)).unwrap();
    assert!(total(&hi.reviews_per_day) > total(&lo.reviews_per_day));
    assert!(a.reviews_per_day[0] > 10.0);
    let sm2 = simulate_workload(&inp(&cards, &DeckConfig { scheduler: SchedulerKind::Sm2, ..make_config() }, today)).unwrap();
    assert!(total(&sm2.reviews_per_day) > 0.0);
}
