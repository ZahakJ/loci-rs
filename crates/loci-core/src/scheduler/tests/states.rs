//! Port of `states.test.ts`.

use super::{local_ms, make_card, make_config};
use crate::scheduler::convert::{collection_day, day_start_ms};
use crate::scheduler::states::{apply_answer, next_card_states, preview_intervals, NextCardState, SchedulingInput};
use crate::types::*;

const ROLL: i64 = 4;
fn crt() -> i64 {
    local_ms(2022, 1, 1, 4, 0) / 1000
}

fn input<'a>(card: &'a Card, config: &'a DeckConfig, now_ms: i64, fuzz: bool) -> SchedulingInput<'a> {
    SchedulingInput {
        card,
        config,
        now_ms,
        today: collection_day(now_ms, crt(), ROLL),
        crt: crt(),
        rollover_hour: ROLL,
        fuzz,
        learn_ahead_secs: None,
        memory: None,
        last_review_ms: None,
        short_term_with_steps: false,
    }
}

fn sm2() -> DeckConfig {
    DeckConfig { scheduler: SchedulerKind::Sm2, ..make_config() }
}

/// Answers `ratings` in order, each at the card's due time (12:30 local for day-based dues).
fn simulate(config: &DeckConfig, ratings: &[Rating], start_ms: i64) -> Vec<i64> {
    let mut card = make_card();
    let mut now = start_ms;
    let mut ivls = Vec::new();
    for &r in ratings {
        let out = apply_answer(&input(&card, config, now, false), r, 0).unwrap();
        card = out.card;
        ivls.push(if out.revlog.ivl > 0 { out.revlog.ivl } else { 0 });
        now = if card.queue == CardQueue::Learn {
            card.due * 1000
        } else {
            day_start_ms(card.due, crt(), ROLL) + 8 * 3_600_000 + 1_800_000
        };
    }
    ivls
}

const G: Rating = Rating::Good;
const A: Rating = Rating::Again;

#[test]
fn s7_8_fsrs_full_scheduler_sequence() {
    let ivls = simulate(&make_config(), &[G, G, G, G, G, G, A, A, G, G, G, G, G], local_ms(2022, 12, 29, 12, 30));
    assert_eq!(ivls, vec![0, 2, 11, 46, 163, 498, 0, 0, 2, 4, 7, 12, 21]);
}

#[test]
fn fsrs5_defaults_as_ts_fsrs_pads_them_reproduce_its_ivl_history() {
    // ts-fsrs migrates a 19-vector with w19 clipped to 0.01 (enable_short_term floor); fsrs-rs
    // FSRS::new keeps w19 = 0. Passing the 21-vector ts-fsrs actually uses reproduces its history.
    let mut p5 = vec![
        0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925, 1.9395, 0.11, 0.29605,
        2.2698, 0.2315, 2.9898, 0.51655, 0.6621,
    ];
    p5.extend_from_slice(&[0.01, 0.5]);
    let cfg = DeckConfig { fsrs_params: p5, ..make_config() };
    let ivls = simulate(&cfg, &[G, G, G, G, G, G, A, A, G, G, G, G, G], local_ms(2022, 12, 29, 12, 30));
    assert_eq!(ivls, vec![0, 4, 14, 44, 125, 328, 0, 0, 6, 14, 31, 65, 130]);
}

#[test]
fn learning_steps_1m_10m_again_60_hard_330_good_600_easy_graduates() {
    let now = local_ms(2023, 6, 1, 12, 0);
    let card = make_card();
    let cfg = sm2();
    let p = preview_intervals(&input(&card, &cfg, now, false)).unwrap();
    assert_eq!(p.seconds_until, RatingMap { again: 60, hard: 330, good: 600, easy: 4 * 86400 });
    assert_eq!(p.labels, RatingMap { again: "1m".into(), hard: "6m".into(), good: "10m".into(), easy: "4d".into() });
}

#[test]
fn new_card_good_enters_learning_and_revlog_fields() {
    let now = local_ms(2023, 6, 1, 12, 0);
    let card = Card { due: 77, ..make_card() };
    let cfg = sm2();
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Good, 12_345).unwrap();
    assert_eq!(out.card.ctype, CardType::Learn);
    assert_eq!(out.card.queue, CardQueue::Learn);
    assert_eq!(out.card.left, 1);
    assert_eq!(out.card.due, now / 1000 + 600);
    assert_eq!(out.card.data.pos, Some(77));
    let r = &out.revlog;
    assert_eq!((r.ease, r.ivl, r.last_ivl, r.factor, r.time, r.rtype, r.id), (3, -600, 0, 0, 12_345, RevlogType::Learn, now));
    let grad = apply_answer(&input(&out.card, &cfg, now + 600_000, false), Rating::Good, 0).unwrap();
    assert_eq!(grad.card.ctype, CardType::Review);
    assert_eq!(grad.card.queue, CardQueue::Review);
    assert_eq!((grad.card.ivl, grad.card.factor, grad.card.left), (1, 2500, 0));
    assert_eq!(
        (grad.revlog.ivl, grad.revlog.last_ivl, grad.revlog.factor, grad.revlog.rtype),
        (1, -600, 2500, RevlogType::Learn)
    );
}

#[test]
fn a_step_that_crosses_the_rollover_becomes_interday_learning() {
    let now = local_ms(2023, 6, 1, 23, 0);
    let cfg = DeckConfig { learn_steps: vec![1.0, 360.0], ..sm2() };
    let card = make_card();
    let inp = input(&card, &cfg, now, false);
    let first = apply_answer(&inp, Rating::Good, 0).unwrap();
    assert_eq!(first.card.queue, CardQueue::DayLearn);
    assert_eq!(first.card.due, inp.today + 1);
    assert_eq!(first.revlog.ivl, 1);
}

#[test]
fn answer_time_is_capped_by_max_answer_seconds() {
    let card = make_card();
    let cfg = make_config();
    let now = chrono::Utc::now().timestamp_millis();
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Good, 500_000).unwrap();
    assert_eq!(out.revlog.time, 60_000);
}

fn review_card(today: i64) -> Card {
    Card {
        ctype: CardType::Review,
        queue: CardQueue::Review,
        ivl: 100,
        factor: 2500,
        due: today - 8,
        lapses: 1,
        reps: 5,
        ..make_card()
    }
}

#[test]
fn s7_9_hard_120_2350_good_260_2500_easy_351_2650() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = review_card(today);
    let cfg = sm2();
    for (r, ivl, factor) in [(Rating::Hard, 120, 2350), (Rating::Good, 260, 2500), (Rating::Easy, 351, 2650)] {
        let out = apply_answer(&input(&card, &cfg, now, false), r, 0).unwrap();
        assert_eq!(out.card.ivl, ivl);
        assert_eq!(out.card.factor, factor);
        assert_eq!(out.card.due, today + ivl);
        assert_eq!(
            (out.revlog.ivl, out.revlog.last_ivl, out.revlog.factor, out.revlog.rtype),
            (ivl, 100, factor, RevlogType::Review)
        );
    }
}

#[test]
fn s7_9_again_relearning_one_day_pending_interval_ease_minus_200() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = review_card(today);
    let cfg = sm2();
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Again, 0).unwrap();
    let c = &out.card;
    assert_eq!((c.ctype, c.queue, c.ivl, c.factor, c.lapses, c.left), (CardType::Relearn, CardQueue::Learn, 1, 2300, 2, 1));
    let back = apply_answer(&input(&out.card, &cfg, now + 600_000, false), Rating::Good, 0).unwrap();
    let b = &back.card;
    assert_eq!((b.ctype, b.queue, b.ivl, b.factor, b.due), (CardType::Review, CardQueue::Review, 1, 2300, today + 1));
    assert_eq!(back.revlog.rtype, RevlogType::Relearn);
    let easy = apply_answer(&input(&out.card, &cfg, now + 600_000, false), Rating::Easy, 0).unwrap();
    assert_eq!(easy.card.ivl, 2);
}

#[test]
fn s7_9_lapse_multiplier_and_minimum_interval() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = review_card(today);
    let cfg = DeckConfig { lapse_multiplier: 0.5, minimum_lapse_interval: 3, ..sm2() };
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Again, 0).unwrap();
    assert_eq!(out.card.ivl, 50);
    let short = Card { ivl: 4, ..card.clone() };
    let cfg2 = DeckConfig { relearn_steps: vec![], ..cfg };
    let out2 = apply_answer(&input(&short, &cfg2, now, false), Rating::Again, 0).unwrap();
    assert_eq!((out2.card.ctype, out2.card.queue, out2.card.ivl), (CardType::Review, CardQueue::Review, 3));
}

#[test]
fn s7_9_ease_never_drops_below_1300() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let cfg = sm2();
    let mut c = Card { factor: 1400, ..review_card(today) };
    for i in 0..6 {
        let r = if i % 2 == 1 { Rating::Hard } else { Rating::Again };
        c = apply_answer(&input(&c, &cfg, now, false), r, 0).unwrap().card;
        assert!(c.factor >= 1300);
    }
    assert_eq!(c.factor, 1300);
}

#[test]
fn s7_9_interval_modifier_and_maximum_interval_apply() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = review_card(today);
    let im = DeckConfig { interval_modifier: 0.5, ..sm2() };
    assert_eq!(apply_answer(&input(&card, &im, now, false), Rating::Good, 0).unwrap().card.ivl, 130);
    let capped = DeckConfig { maximum_interval: 200, ..sm2() };
    assert_eq!(apply_answer(&input(&card, &capped, now, false), Rating::Easy, 0).unwrap().card.ivl, 200);
}

#[test]
fn s7_9_early_review_uses_the_early_formulas() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let early = Card { due: today + 50, ..review_card(today) }; // elapsed 50 of 100
    let cfg = sm2();
    let s = next_card_states(&input(&early, &cfg, now, false)).unwrap();
    assert_eq!(s.good.scheduled_days(), Some(125));
    assert_eq!(s.hard.scheduled_days(), Some(60));
    assert_eq!(s.easy.scheduled_days(), Some(144));
}

#[test]
fn lapses_7_again_suspends_with_action_suspend_threshold_8() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card =
        Card { ctype: CardType::Review, queue: CardQueue::Review, ivl: 100, factor: 2500, due: today, lapses: 7, ..make_card() };
    for scheduler in [SchedulerKind::Sm2, SchedulerKind::Fsrs] {
        let suspend = DeckConfig { scheduler, leech_action: LeechAction::Suspend, ..make_config() };
        let out = apply_answer(&input(&card, &suspend, now, false), Rating::Again, 0).unwrap();
        assert!(out.became_leech);
        assert_eq!(out.card.queue, CardQueue::Suspended);
        assert_eq!(out.card.lapses, 8);
        let tag = DeckConfig { scheduler, leech_action: LeechAction::Tag, ..make_config() };
        let tag_only = apply_answer(&input(&card, &tag, now, false), Rating::Again, 0).unwrap();
        assert!(tag_only.became_leech);
        assert_eq!(tag_only.card.queue, CardQueue::Learn);
        let plain = DeckConfig { scheduler, ..make_config() };
        let eight = Card { lapses: 8, ..card.clone() };
        assert!(!apply_answer(&input(&eight, &plain, now, false), Rating::Again, 0).unwrap().became_leech);
        let eleven = Card { lapses: 11, ..card.clone() };
        assert!(apply_answer(&input(&eleven, &plain, now, false), Rating::Again, 0).unwrap().became_leech);
    }
}

#[test]
fn fsrs_review_card_without_memory_state_is_converted_from_sm2() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = Card { ctype: CardType::Review, queue: CardQueue::Review, ivl: 10, factor: 2500, due: today, ..make_card() };
    let cfg = make_config();
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Good, 0).unwrap();
    assert!(out.card.data.s.unwrap() > 10.0);
    assert_eq!(out.card.data.dr, Some(0.9));
    assert_eq!(out.card.data.decay, Some(0.154));
    let d = out.card.data.d.unwrap();
    assert_eq!(out.revlog.factor, (((d - 1.0) / 9.0 + 0.1) * 1000.0).round() as i64);
}

#[test]
fn fsrs_first_rating_easy_graduates_to_8_days_others_learn() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let card = make_card();
    let cfg = make_config();
    let p = preview_intervals(&input(&card, &cfg, now, false)).unwrap();
    assert_eq!(p.seconds_until.easy, 8 * 86400);
    assert_eq!(p.labels.again, "1m");
    assert_eq!(p.labels.hard, "6m"); // (1m + 10m) / 2 = 5.5 minutes, shown whole
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Easy, 0).unwrap();
    assert!((out.card.data.s.unwrap() - 8.2956).abs() < 5e-5);
    assert_eq!(out.card.data.d, Some(1.0));
}

#[test]
fn fsrs_passing_intervals_are_strictly_ordered_and_grow() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = Card {
        ctype: CardType::Review,
        queue: CardQueue::Review,
        ivl: 20,
        factor: 2500,
        due: today,
        data: CardData { s: Some(20.0), d: Some(5.0), ..CardData::default() },
        ..make_card()
    };
    let cfg = make_config();
    for fuzz in [false, true] {
        let s = next_card_states(&input(&card, &cfg, now, fuzz)).unwrap();
        let d = |st: &NextCardState| st.scheduled_days().unwrap();
        assert!(d(&s.hard) < d(&s.good));
        assert!(d(&s.good) < d(&s.easy));
        assert!(d(&s.good) >= 21);
    }
}

#[test]
fn fuzz_is_deterministic_for_the_same_card_and_reps() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = Card {
        ctype: CardType::Review,
        queue: CardQueue::Review,
        ivl: 30,
        factor: 2500,
        due: today,
        reps: 9,
        data: CardData { s: Some(30.0), d: Some(5.0), ..CardData::default() },
        ..make_card()
    };
    let cfg = make_config();
    let a = preview_intervals(&input(&card, &cfg, now, true)).unwrap();
    let b = preview_intervals(&input(&card, &cfg, now, true)).unwrap();
    assert_eq!(a, b);
    // Regenerated from the Anki PRNG (StdRng seeded with id + reps); the TS numbers came from mulberry32.
    let fuzzed = apply_answer(&input(&card, &cfg, now, true), Rating::Good, 0).unwrap().card.ivl;
    let plain = apply_answer(&input(&card, &cfg, now, false), Rating::Good, 0).unwrap().card.ivl;
    let (lo, hi) = crate::scheduler::fuzz::constrained_fuzz_bounds(plain as f64, 31, 36500);
    assert!(fuzzed >= lo && fuzzed <= hi, "{fuzzed} not in [{lo}, {hi}]");
}

#[test]
fn sm2_answers_clear_fsrs_memory() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let today = collection_day(now, crt(), ROLL);
    let card = Card {
        ctype: CardType::Review,
        queue: CardQueue::Review,
        ivl: 30,
        factor: 2500,
        due: today,
        data: CardData { s: Some(30.0), d: Some(5.0), pos: Some(3), ..CardData::default() },
        ..make_card()
    };
    let cfg = sm2();
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Good, 0).unwrap();
    assert_eq!(out.card.data.s, None);
    assert_eq!(out.card.data.pos, Some(3));
}

#[test]
fn no_learning_steps_plus_short_term_params_again_on_a_new_card_stays_intraday() {
    let now = local_ms(2023, 6, 10, 12, 0);
    let card = make_card();
    let cfg = DeckConfig { learn_steps: vec![], ..make_config() };
    let out = apply_answer(&input(&card, &cfg, now, false), Rating::Again, 0).unwrap();
    assert_eq!(out.card.ctype, CardType::Learn);
    assert_eq!(out.card.queue, CardQueue::Learn);
    assert!(out.revlog.ivl < 0);
}
