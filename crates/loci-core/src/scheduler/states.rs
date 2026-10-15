//! Anki v3 card-state machine over Card rows (rslib/src/scheduler/states/*, answering/*;
//! research/fsrs-spec.md §3 and §8), for both FSRS-6
//! and SM-2.
//!
//! Units follow `types.rs`: intraday learning `due` = unix seconds; review/day-learn `due` =
//! collection day; `ivl` days; `factor` permille; revlog ivl/last_ivl days (>0) or seconds (<0).

use super::clock::{SchedClock, DEFAULT_LEARN_AHEAD_SECS};
use super::convert::{collection_day, day_start_ms, format_interval};
use super::fsrs::{allow_short_term, Fsrs6, MemoryState, NextStates};
use super::fuzz::{
    fuzz_factor_for, learning_fuzz_factor_for, learning_ivl_with_fuzz, minimum_review_fuzz_interval, with_review_fuzz,
};
use super::sm2::{
    ease_from_factor, leech_threshold_met, min_max_intervals, next_ease, sm2_lapse_interval, sm2_passing_intervals, Sm2Options,
    EASE_DELTA_AGAIN, MINIMUM_EASE,
};
use super::steps::{LearningSteps, DAY_SECS};
use crate::error::Result;
use crate::types::{
    Card, CardQueue, CardType, CollectionDay, DeckConfig, LeechAction, Rating, RatingMap, RevlogEntry, RevlogType, SchedulerKind,
    SchedulingPreview, Seconds,
};

/// Everything the state machine needs to answer or preview one card.
#[derive(Debug, Clone)]
pub struct SchedulingInput<'a> {
    pub card: &'a Card,
    pub config: &'a DeckConfig,
    /// Answer time, ms epoch.
    pub now_ms: i64,
    /// Collection day of `now_ms`.
    pub today: CollectionDay,
    /// Collection creation time, seconds.
    pub crt: Seconds,
    pub rollover_hour: i64,
    /// Apply Anki's review fuzz (seeded by card id + reps) and learning-step fuzz.
    pub fuzz: bool,
    /// Learn-ahead limit, seconds (passed to the label formatter; default 1200).
    pub learn_ahead_secs: Option<i64>,
    /// FSRS memory state to use instead of card.data (e.g. derived from the revlog with
    /// `convert::memory_state_from_revlog`). `None` = read card.data, then SM-2 conversion.
    pub memory: Option<MemoryState>,
    /// Time of the last review (ms) when card.data.lrt is missing (e.g. from the revlog).
    pub last_review_ms: Option<i64>,
    /// Anki "FSRS short-term with steps" (global, default false).
    pub short_term_with_steps: bool,
}

impl<'a> SchedulingInput<'a> {
    /// The usual input: a card under its deck config at the clock's instant, fuzz on.
    pub fn new(card: &'a Card, config: &'a DeckConfig, clock: &SchedClock, fuzz: bool) -> Self {
        SchedulingInput {
            card,
            config,
            now_ms: clock.now_ms,
            today: clock.today,
            crt: clock.crt,
            rollover_hour: clock.rollover_hour,
            fuzz,
            learn_ahead_secs: Some(clock.learn_ahead_secs),
            memory: None,
            last_review_ms: None,
            short_term_with_steps: false,
        }
    }
}

// ───────────────────────── abstract states ─────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub struct LearnState {
    /// Steps left until graduation (card.left).
    pub remaining: i64,
    /// Delay of the step just scheduled, seconds (before learning fuzz).
    pub scheduled_secs: i64,
    pub memory: Option<MemoryState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewState {
    pub scheduled_days: i64,
    pub elapsed_days: i64,
    /// Ease as a factor (2.5).
    pub ease: f64,
    pub lapses: i64,
    /// This answer crossed the leech threshold.
    pub leeched: bool,
    pub memory: Option<MemoryState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelearnState {
    pub learning: LearnState,
    /// The review state restored on graduation (pending interval, ease, lapses).
    pub review: ReviewState,
}

#[derive(Debug, Clone, PartialEq)]
enum CurrentState {
    New,
    Learning(LearnState),
    Review(ReviewState),
    Relearning(RelearnState),
}

/// The abstract state a card moves to after an answer.
#[derive(Debug, Clone, PartialEq)]
pub enum NextCardState {
    Learning(LearnState),
    Review(ReviewState),
    Relearning(RelearnState),
}

impl NextCardState {
    /// Scheduled review days when the next state is a review (tests, card info).
    pub fn scheduled_days(&self) -> Option<i64> {
        match self {
            NextCardState::Review(r) => Some(r.scheduled_days),
            _ => None,
        }
    }
    /// Seconds until due (learning: the step delay before fuzz; review: days·86400).
    pub fn seconds_until(&self) -> i64 {
        match self {
            NextCardState::Review(r) => r.scheduled_days * DAY_SECS,
            NextCardState::Learning(l) => l.scheduled_secs,
            NextCardState::Relearning(r) => r.learning.scheduled_secs,
        }
    }
}

struct Ctx<'a> {
    input: &'a SchedulingInput<'a>,
    steps: LearningSteps,
    relearn_steps: LearningSteps,
    model: Option<Fsrs6>,
    fsrs: Option<NextStates>,
    fuzz_factor: Option<f64>,
    max_ivl: i64,
    initial_ease: f64,
    short_term_allowed: bool,
    short_term_with_steps: bool,
}

/// Days since the last review (scheduler days), from lrt / last_review_ms or the card's due.
pub fn days_since_last_review(input: &SchedulingInput<'_>) -> i64 {
    let c = input.card;
    let last_ms = match c.data.lrt {
        Some(lrt) if lrt > 0 => Some(lrt * 1000),
        _ => input.last_review_ms,
    };
    if let Some(last) = last_ms {
        return (input.today - collection_day(last, input.crt, input.rollover_hour)).max(0);
    }
    if c.ctype == CardType::New {
        return 0;
    }
    if c.queue == CardQueue::Review || (c.ctype == CardType::Review && (c.queue as i64) < 0) {
        return (c.ivl - (c.due - input.today)).max(0);
    }
    if c.queue == CardQueue::DayLearn {
        return 1;
    }
    0
}

fn current_memory(input: &SchedulingInput<'_>, model: &Fsrs6) -> Option<MemoryState> {
    if let Some(m) = input.memory {
        return Some(m);
    }
    let c = input.card;
    if c.ctype == CardType::New {
        return None;
    }
    if let (Some(s), Some(d)) = (c.data.s, c.data.d) {
        return Some(MemoryState::new(s, d));
    }
    if c.ctype == CardType::Review || c.ctype == CardType::Relearn {
        return Some(model.from_sm2(
            ease_from_factor(c.factor, input.config.initial_ease),
            c.ivl.max(1) as f64,
            input.config.historical_retention,
        ));
    }
    None
}

fn build_ctx<'a>(input: &'a SchedulingInput<'a>) -> Result<(Ctx<'a>, CurrentState)> {
    let card = input.card;
    let config = input.config;
    let steps = LearningSteps::new(&config.learn_steps);
    let relearn_steps = LearningSteps::new(&config.relearn_steps);
    let model = if config.scheduler == SchedulerKind::Fsrs { Some(Fsrs6::new(&config.fsrs_params)?) } else { None };
    let elapsed = days_since_last_review(input);
    let memory = model.as_ref().and_then(|m| current_memory(input, m));
    let ease = ease_from_factor(card.factor, config.initial_ease);
    let left = card.left % 1000;
    let review = |elapsed_days: i64| ReviewState {
        scheduled_days: card.ivl,
        elapsed_days,
        ease,
        lapses: card.lapses,
        leeched: false,
        memory,
    };
    let current = match card.ctype {
        CardType::New => CurrentState::New,
        CardType::Learn => {
            CurrentState::Learning(LearnState { remaining: left, scheduled_secs: steps.current_delay(left).unwrap_or(0), memory })
        }
        CardType::Review => CurrentState::Review(review(elapsed)),
        CardType::Relearn => CurrentState::Relearning(RelearnState {
            learning: LearnState { remaining: left, scheduled_secs: relearn_steps.current_delay(left).unwrap_or(0), memory },
            review: review(elapsed),
        }),
    };
    let fsrs = model
        .as_ref()
        .map(|m| m.next_states(memory, config.desired_retention, if card.ctype == CardType::New { 0 } else { elapsed }));
    let short_term_allowed = match &model {
        // An empty configured vector is judged as empty, not as the defaults it becomes.
        Some(m) => allow_short_term(if config.fsrs_params.is_empty() { &[] } else { &m.params }),
        None => false,
    };
    let ctx = Ctx {
        input,
        steps,
        relearn_steps,
        model,
        fsrs,
        fuzz_factor: if input.fuzz { Some(fuzz_factor_for(card.id, card.reps)) } else { None },
        max_ivl: config.maximum_interval.max(1),
        initial_ease: config.initial_ease as f64 / 1000.0,
        short_term_allowed,
        short_term_with_steps: input.short_term_with_steps,
    };
    Ok((ctx, current))
}

impl Ctx<'_> {
    fn mem_of(&self, rating: Rating) -> Option<MemoryState> {
        self.fsrs.as_ref().map(|n| n.get(rating as u8).memory)
    }
    fn ivl_of(&self, rating: Rating) -> f64 {
        self.fsrs.as_ref().map(|n| n.get(rating as u8).interval).unwrap_or(0.0)
    }
    fn fuzz_days(&self, ivl: f64, minimum: i64) -> i64 {
        let (min, max) = min_max_intervals(minimum, self.max_ivl);
        with_review_fuzz(self.fuzz_factor, ivl, min, max)
    }
    fn is_short_term(&self, steps: &LearningSteps, ivl: f64) -> bool {
        self.short_term_allowed && (self.short_term_with_steps || steps.is_empty()) && ivl < 0.5
    }
    fn review(&self, scheduled_days: i64, memory: Option<MemoryState>, base: Option<&ReviewState>) -> ReviewState {
        ReviewState {
            scheduled_days,
            elapsed_days: 0,
            ease: base.map(|b| b.ease).unwrap_or(self.initial_ease),
            lapses: base.map(|b| b.lapses).unwrap_or(self.input.card.lapses),
            leeched: false,
            memory,
        }
    }
    fn sm2(&self) -> Sm2Options {
        Sm2Options::from(self.input.config)
    }
}

fn rounded_min1(x: f64) -> i64 {
    (x.round() as i64).max(1)
}

// ── learning (and new) ──
fn answer_learning(ctx: &Ctx<'_>, cur: &LearnState, rating: Rating) -> NextCardState {
    let steps = &ctx.steps;
    let memory = ctx.mem_of(rating);
    let graduate = |r: Rating| -> NextCardState {
        if ctx.fsrs.is_some() {
            let ivl = ctx.ivl_of(r);
            if ctx.is_short_term(steps, ivl) {
                return NextCardState::Learning(LearnState {
                    remaining: 0,
                    scheduled_secs: (ivl * DAY_SECS as f64).floor() as i64,
                    memory,
                });
            }
            return NextCardState::Review(ctx.review(ctx.fuzz_days(rounded_min1(ivl) as f64, 1), memory, None));
        }
        NextCardState::Review(ctx.review(ctx.fuzz_days(ctx.input.config.graduating_interval as f64, 1), memory, None))
    };
    match rating {
        Rating::Again => match steps.again_delay() {
            Some(delay) => {
                NextCardState::Learning(LearnState { remaining: steps.remaining_for_failed(), scheduled_secs: delay, memory })
            }
            None => graduate(Rating::Again),
        },
        Rating::Hard => match steps.hard_delay(cur.remaining) {
            Some(delay) => NextCardState::Learning(LearnState { remaining: cur.remaining, scheduled_secs: delay, memory }),
            None => graduate(Rating::Hard),
        },
        Rating::Good => match steps.good_delay(cur.remaining) {
            Some(delay) => NextCardState::Learning(LearnState {
                remaining: steps.remaining_for_good(cur.remaining),
                scheduled_secs: delay,
                memory,
            }),
            None => graduate(Rating::Good),
        },
        Rating::Easy => {
            if let Some(f) = &ctx.fsrs {
                let good = ctx.fuzz_days(f.good.interval, 1);
                return NextCardState::Review(ctx.review(
                    ctx.fuzz_days(rounded_min1(f.easy.interval) as f64, good + 1),
                    memory,
                    None,
                ));
            }
            NextCardState::Review(ctx.review(ctx.fuzz_days(ctx.input.config.easy_interval as f64, 1), memory, None))
        }
    }
}

// ── review ──
fn fsrs_passing(ctx: &Ctx<'_>, f: &NextStates, prev: i64) -> (i64, i64, i64) {
    let max = ctx.max_ivl;
    let (h, g, e) = (f.hard.interval, f.good.interval, f.easy.interval);
    let hard = ctx.fuzz_days(h, minimum_review_fuzz_interval(h, prev, max).max(1));
    let good = ctx.fuzz_days(g, minimum_review_fuzz_interval(g, prev, max).max(hard + 1));
    let easy = ctx.fuzz_days(e, minimum_review_fuzz_interval(e, prev, max).max(good + 1));
    (hard, good, easy)
}

fn answer_review(ctx: &Ctx<'_>, cur: &ReviewState, rating: Rating) -> NextCardState {
    let cfg = ctx.input.config;
    let memory = ctx.mem_of(rating);
    if rating == Rating::Again {
        let lapses = cur.lapses + 1;
        let leeched = leech_threshold_met(lapses, cfg.leech_threshold);
        let scheduled_days = match &ctx.fsrs {
            Some(f) => rounded_min1(f.again.interval).min(ctx.max_ivl),
            None => sm2_lapse_interval(&ctx.sm2(), cur.scheduled_days, ctx.fuzz_factor),
        };
        let again = ReviewState {
            scheduled_days,
            elapsed_days: 0,
            ease: (cur.ease + EASE_DELTA_AGAIN).max(MINIMUM_EASE),
            lapses,
            leeched,
            memory,
        };
        if let Some(delay) = ctx.relearn_steps.again_delay() {
            return NextCardState::Relearning(RelearnState {
                learning: LearnState { remaining: ctx.relearn_steps.remaining_for_failed(), scheduled_secs: delay, memory },
                review: again,
            });
        }
        if let Some(f) = &ctx.fsrs {
            if ctx.is_short_term(&ctx.relearn_steps, f.again.interval) {
                return NextCardState::Relearning(RelearnState {
                    learning: LearnState {
                        remaining: 0,
                        scheduled_secs: (f.again.interval * DAY_SECS as f64).floor() as i64,
                        memory,
                    },
                    review: again,
                });
            }
        }
        return NextCardState::Review(again);
    }
    let (hard, good, easy) = match &ctx.fsrs {
        Some(f) => fsrs_passing(ctx, f, cur.scheduled_days),
        None => {
            let p = sm2_passing_intervals(&ctx.sm2(), cur.scheduled_days, cur.elapsed_days, cur.ease, ctx.fuzz_factor);
            (p.hard, p.good, p.easy)
        }
    };
    let days = match rating {
        Rating::Hard => hard,
        Rating::Good => good,
        _ => easy,
    };
    NextCardState::Review(ReviewState {
        scheduled_days: days,
        elapsed_days: 0,
        ease: next_ease(cur.ease, rating as u8),
        lapses: cur.lapses,
        leeched: false,
        memory,
    })
}

// ── relearning ──
fn answer_relearning(ctx: &Ctx<'_>, cur: &RelearnState, rating: Rating) -> NextCardState {
    let rs = &ctx.relearn_steps;
    let memory = ctx.mem_of(rating);
    let base = &cur.review;
    let relearn = |remaining: i64, secs: i64, scheduled_days: i64| {
        NextCardState::Relearning(RelearnState {
            learning: LearnState { remaining, scheduled_secs: secs, memory },
            review: ReviewState { scheduled_days, elapsed_days: 0, memory, leeched: false, ..base.clone() },
        })
    };
    let to_review = |scheduled_days: i64| {
        NextCardState::Review(ReviewState { scheduled_days, elapsed_days: 0, memory, leeched: false, ..base.clone() })
    };
    let graduate = |r: Rating| -> NextCardState {
        if ctx.fsrs.is_some() {
            let ivl = ctx.ivl_of(r);
            if ctx.is_short_term(rs, ivl) {
                return relearn(0, (ivl * DAY_SECS as f64).floor() as i64, base.scheduled_days);
            }
            return to_review(ctx.fuzz_days(rounded_min1(ivl) as f64, 1));
        }
        to_review(base.scheduled_days)
    };
    match rating {
        Rating::Again => match rs.again_delay() {
            Some(delay) => {
                let days = match &ctx.fsrs {
                    Some(f) => rounded_min1(f.again.interval).min(ctx.max_ivl),
                    None => base.scheduled_days,
                };
                relearn(rs.remaining_for_failed(), delay, days)
            }
            None => graduate(Rating::Again),
        },
        Rating::Hard => match rs.hard_delay(cur.learning.remaining) {
            Some(delay) => relearn(cur.learning.remaining, delay, base.scheduled_days),
            None => graduate(Rating::Hard),
        },
        Rating::Good => match rs.good_delay(cur.learning.remaining) {
            Some(delay) => relearn(rs.remaining_for_good(cur.learning.remaining), delay, base.scheduled_days),
            None => graduate(Rating::Good),
        },
        Rating::Easy => {
            let days = match &ctx.fsrs {
                Some(f) => {
                    let good = ctx.fuzz_days(f.good.interval, 1);
                    ctx.fuzz_days(rounded_min1(f.easy.interval) as f64, good + 1)
                }
                None => (base.scheduled_days + 1).min(ctx.max_ivl),
            };
            to_review(days)
        }
    }
}

fn answer_state(ctx: &Ctx<'_>, cur: &CurrentState, rating: Rating) -> NextCardState {
    match cur {
        CurrentState::New => {
            let fresh = LearnState { remaining: ctx.steps.len() as i64, scheduled_secs: 0, memory: None };
            answer_learning(ctx, &fresh, rating)
        }
        CurrentState::Learning(l) => answer_learning(ctx, l, rating),
        CurrentState::Review(r) => answer_review(ctx, r, rating),
        CurrentState::Relearning(r) => answer_relearning(ctx, r, rating),
    }
}

/// The four candidate next states (exposed for tests and card info).
pub fn next_card_states(input: &SchedulingInput<'_>) -> Result<RatingMap<NextCardState>> {
    let (ctx, current) = build_ctx(input)?;
    Ok(RatingMap::from_fn(|r| answer_state(&ctx, &current, r)))
}

/// Button preview: seconds until due and labels ('1m', '6m', '3d', '1.2mo').
pub fn preview_intervals(input: &SchedulingInput<'_>) -> Result<SchedulingPreview> {
    let states = next_card_states(input)?;
    let collapse = input.learn_ahead_secs.unwrap_or(DEFAULT_LEARN_AHEAD_SECS);
    let seconds_until = RatingMap::from_fn(|r| states.get(r).seconds_until());
    let labels = RatingMap::from_fn(|r| format_interval(*seconds_until.get(r), collapse));
    Ok(SchedulingPreview { labels, seconds_until })
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnswerOutput {
    pub card: Card,
    /// Revlog row: `id` = now_ms (caller may bump it for uniqueness), usn −1.
    pub revlog: RevlogEntry,
    /// The answer crossed the leech threshold (caller adds the `leech` tag; suspension already applied).
    pub became_leech: bool,
    /// The abstract next state, for callers that need it.
    pub next: NextCardState,
}

fn round_dp(x: f64, dp: i32) -> f64 {
    let f = 10f64.powi(dp);
    (x * f).round() / f
}

/// Anki revlog factor for FSRS answers: round(((D − 1)/9 + 0.1)·1000), 100..1100.
pub fn difficulty_shifted(d: f64) -> i64 {
    (((d - 1.0) / 9.0 + 0.1) * 1000.0).round() as i64
}

/// Revlog encoding of the interval the card had before answering.
fn last_interval(cur: &CurrentState) -> i64 {
    let secs_to_revlog = |secs: i64| if secs >= DAY_SECS { (secs as f64 / DAY_SECS as f64).round() as i64 } else { -secs };
    match cur {
        CurrentState::New => 0,
        CurrentState::Learning(l) => secs_to_revlog(l.scheduled_secs),
        CurrentState::Review(r) => r.scheduled_days,
        CurrentState::Relearning(r) => secs_to_revlog(r.learning.scheduled_secs),
    }
}

/// Answers `input.card` with `rating` (`answer_ms` = time taken); returns the updated card row
/// and its revlog entry.
pub fn apply_answer(input: &SchedulingInput<'_>, rating: Rating, answer_ms: i64) -> Result<AnswerOutput> {
    let (ctx, current) = build_ctx(input)?;
    let next = answer_state(&ctx, &current, rating);
    let old = input.card;
    let config = input.config;
    let now_ms = input.now_ms;
    let today = input.today;
    let now_secs = now_ms.div_euclid(1000);
    let next_day_at = day_start_ms(today + 1, input.crt, input.rollover_hour);
    let secs_until_rollover = (next_day_at - now_ms).div_euclid(1000).max(0);
    let mut card = old.clone();
    card.mtime = now_secs;
    card.usn = -1;
    card.reps = old.reps + 1;
    if current == CurrentState::New && card.data.pos.is_none() {
        card.data.pos = Some(old.due);
    }

    // Places a learning delay: intraday (queue Learn, due secs) or interday (DayLearn, due day).
    let place_learning = |card: &mut Card, secs: i64| -> i64 {
        if secs >= secs_until_rollover {
            let days = (secs - secs_until_rollover).div_euclid(DAY_SECS) + 1;
            card.queue = CardQueue::DayLearn;
            card.due = today + days;
            return days;
        }
        card.queue = CardQueue::Learn;
        let factor = if input.fuzz { Some(learning_fuzz_factor_for(old.id, old.reps)) } else { None };
        card.due = now_secs + learning_ivl_with_fuzz(factor, secs);
        -secs
    };

    let revlog_ivl;
    let memory;
    let mut became_leech = false;
    match &next {
        NextCardState::Learning(l) => {
            card.ctype = CardType::Learn;
            card.left = l.remaining;
            revlog_ivl = place_learning(&mut card, l.scheduled_secs);
            memory = l.memory;
        }
        NextCardState::Relearning(r) => {
            card.ctype = CardType::Relearn;
            card.ivl = r.review.scheduled_days;
            card.factor = (r.review.ease * 1000.0).round() as i64;
            card.lapses = r.review.lapses;
            card.left = r.learning.remaining;
            revlog_ivl = place_learning(&mut card, r.learning.scheduled_secs);
            memory = r.learning.memory;
            became_leech = r.review.leeched;
        }
        NextCardState::Review(r) => {
            card.ctype = CardType::Review;
            card.queue = CardQueue::Review;
            card.ivl = r.scheduled_days;
            card.due = today + r.scheduled_days;
            card.factor = (r.ease * 1000.0).round() as i64;
            card.lapses = r.lapses;
            card.left = 0;
            revlog_ivl = r.scheduled_days;
            memory = r.memory;
            became_leech = r.leeched;
        }
    }
    card.odue = 0;
    card.odid = 0;
    if became_leech && config.leech_action == LeechAction::Suspend {
        card.queue = CardQueue::Suspended;
    }

    let fsrs_memory = match (&ctx.model, memory) {
        (Some(m), Some(mem)) => Some((m, mem)),
        _ => None,
    };
    match fsrs_memory {
        Some((m, mem)) => {
            card.data.s = Some(round_dp(mem.stability, 4));
            card.data.d = Some(round_dp(mem.difficulty, 3));
            card.data.dr = Some(round_dp(config.desired_retention, 2));
            card.data.decay = Some(round_dp(m.decay, 3));
        }
        None => {
            card.data.s = None;
            card.data.d = None;
            card.data.dr = None;
            card.data.decay = None;
        }
    }
    card.data.lrt = Some(now_secs);

    let revlog_factor = match fsrs_memory {
        Some((_, mem)) => difficulty_shifted(mem.difficulty),
        None => match &next {
            NextCardState::Learning(_) => 0,
            _ => card.factor,
        },
    };
    let revlog = RevlogEntry {
        id: now_ms,
        cid: old.id,
        usn: -1,
        ease: rating as i64,
        ivl: revlog_ivl,
        last_ivl: last_interval(&current),
        factor: revlog_factor,
        time: answer_ms.clamp(0, config.max_answer_seconds * 1000),
        rtype: match current {
            CurrentState::Review(_) => RevlogType::Review,
            CurrentState::Relearning(_) => RevlogType::Relearn,
            _ => RevlogType::Learn,
        },
    };
    Ok(AnswerOutput { card, revlog, became_leech, next })
}
