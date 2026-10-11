//! The scheduler: FSRS-6, Anki's SM-2 variant, the v3 card-state machine, queue gathering, the
//! revlog → FSRS item builder, the parameter optimizer and the workload forecast (recall
//! `src/core/scheduler/*`, specified by `research/fsrs-spec.md`).
//!
//! Module map (each mirrors one TypeScript file):
//! - [`fsrs`]: the FSRS-6 formulas, parameter upgrade/clipping and the `Fsrs6` model;
//! - [`fuzz`]: Anki's interval fuzz with Anki's exact PRNG (`StdRng`/ChaCha12 seeded by id + reps);
//! - [`sm2`], [`steps`]: SM-2 review arithmetic and learning-step semantics;
//! - [`states`]: `apply_answer` / `preview_intervals` / `next_card_states` over `Card` rows;
//! - [`queue`]: v3 gathering with limits, burying and display orders, plus the SQLite loaders;
//! - [`items`], [`convert`]: revlog → FSRS items, day numbering, labels, SM-2 → FSRS memory;
//! - [`optimizer`], [`workload`]: fsrs-rs's optimizer pipeline and the expected-value forecast;
//! - [`clock`]: `SchedClock`, the context phase 2 supplies from the collection's settings.
//!
//! Phase 2 (`collection::study`, import, stats, backlog, catch-up) calls the re-exports below.

pub mod clock;
pub mod convert;
pub mod fsrs;
pub mod fuzz;
pub mod items;
pub mod optimizer;
pub mod queue;
pub mod sm2;
pub mod states;
pub mod steps;
pub mod workload;

#[cfg(test)]
mod tests;

pub use clock::{SchedClock, DEFAULT_LEARN_AHEAD_SECS};
pub use convert::{
    collection_day, day_start_ms, elapsed_days, format_interval, memory_state_from_revlog, next_day_at_ms, CardSched,
    RevlogConversionInput,
};
pub use fsrs::{
    allow_short_term, check_and_fill_parameters, clamp_parameters, decay_from_params, forgetting_curve, memory_state_from_sm2,
    next_interval, normalize_parameters, Fsrs6, Grade, MemoryState, NextStates, DEFAULT_PARAMS_FSRS6, FSRS6_DEFAULT_DECAY,
};
pub use fuzz::{fuzz_factor_for, with_review_fuzz};
pub use items::{reviews_for_fsrs, training_items_from_revlog, FsrsItem, FsrsReview};
pub use optimizer::{
    evaluate, optimise, run as run_optimizer, Metrics, OptimiseOutcome, OptimizeMode, OptimizeOptions, OptimizeResult,
};
pub use queue::{
    build_queue, card_revlog, deck_filter_sql, due_counts, expected_answers, load_queue_cards, minutes_for, seconds_per_card,
    siblings_to_bury, QueueDeck, QueueInput, QueueResult,
};
pub use sm2::leech_threshold_met;
pub use states::{
    apply_answer, days_since_last_review, difficulty_shifted, next_card_states, preview_intervals, AnswerOutput, NextCardState,
    SchedulingInput,
};
pub use steps::LearningSteps;
pub use workload::{simulate_workload, WorkloadInput, WorkloadResult};
