//! Deck-tree counts (recall `src/core/collection/queue-counter.ts` + `scheduler-counter.ts`):
//! Anki's v3 gathering (`scheduler::due_counts`) per deck, plus an estimate of minutes to finish
//! from the recent seconds-per-answer. The counter is injectable (tests pass a fake one).

use crate::collection::internals::CollectionCore;
use crate::collection::sched_context::{queue_decks, sched_clock};
use crate::error::Result;
use crate::scheduler::{due_counts, expected_answers, load_queue_cards, minutes_for, seconds_per_card, QueueInput};
use crate::types::{Deck, DeckId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeckCounts {
    /// Cards studyable today after daily limits, including all descendant decks.
    pub new: i64,
    pub learn: i64,
    pub review: i64,
    /// Estimated minutes to finish today's cards, from recent seconds per card.
    pub estimated_minutes: i64,
}

/// Counts for each deck in `decks` (the whole deck list is passed so the counter can apply parent
/// limits). Decks missing from the returned map count as zero.
pub type QueueCounter = Box<dyn Fn(&CollectionCore, &[Deck]) -> Result<BTreeMap<DeckId, DeckCounts>> + Send + Sync>;
/// The unboxed counter signature (`deck_tree` and `extend_today_limits` borrow one).
pub type CounterFn = dyn Fn(&CollectionCore, &[Deck]) -> Result<BTreeMap<DeckId, DeckCounts>>;

/// The default counter of the TypeScript skeleton: everything is zero.
pub fn zero_deck_counts(_core: &CollectionCore, _decks: &[Deck]) -> Result<BTreeMap<DeckId, DeckCounts>> {
    Ok(BTreeMap::new())
}

/// The real deck-tree counter (`scheduler-counter.ts`).
pub fn scheduler_deck_counts(core: &CollectionCore, decks: &[Deck]) -> Result<BTreeMap<DeckId, DeckCounts>> {
    let mut out = BTreeMap::new();
    if decks.is_empty() {
        return Ok(out);
    }
    let clock = sched_clock(core)?;
    let qdecks = queue_decks(core, decks)?;
    let cards = load_queue_cards(core.db(), None, &clock)?;
    let counts = due_counts(&QueueInput::new(&cards, &qdecks, None, &clock));
    let pace = seconds_per_card(core.db(), clock.now_ms, None)?;
    for d in &qdecks {
        let c = counts.get(&d.id).copied().unwrap_or_default();
        let steps = d.config.learn_steps.len();
        out.insert(
            d.id,
            DeckCounts {
                new: c.new,
                learn: c.learn,
                review: c.review,
                estimated_minutes: minutes_for(expected_answers(&c, steps), pace),
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;

    #[test]
    fn counts_new_cards_with_the_default_pace() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("D", None).unwrap();
        for i in 0..3 {
            h.add_note(&basic, deck.id, &[&format!("q{i}"), "a"], &[]);
        }
        let counts = scheduler_deck_counts(h.col.core(), &h.col.core().all_decks().unwrap()).unwrap();
        let c = counts[&deck.id];
        assert_eq!((c.new, c.learn, c.review), (3, 0, 0));
        // 3 new cards × 2 learning steps × 8 s = 48 s → 1 minute.
        assert_eq!(c.estimated_minutes, 1);
        assert_eq!(counts[&1], DeckCounts::default());
        assert!(zero_deck_counts(h.col.core(), &[]).unwrap().is_empty());
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::collection::service::test_support::*;

    #[test]
    fn counts_due_reviews_and_skips_buried_and_future_cards() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let ids: Vec<i64> = (0..4).map(|i| h.add_note(&basic, 1, &[&format!("q{i}"), "a"], &[]).card_ids[0]).collect();
        let db = h.col.core().db();
        db.run("UPDATE cards SET type = 2, queue = 2, due = 0, ivl = 3 WHERE id = ?", [ids[0]]).unwrap(); // due today
        db.run("UPDATE cards SET type = 2, queue = 2, due = 5, ivl = 3 WHERE id = ?", [ids[1]]).unwrap(); // future
        db.run("UPDATE cards SET type = 2, queue = -3, due = 0, ivl = 3 WHERE id = ?", [ids[2]]).unwrap(); // buried
        let counts = scheduler_deck_counts(h.col.core(), &h.col.core().all_decks().unwrap()).unwrap();
        let c = counts[&1];
        assert_eq!((c.new, c.learn, c.review), (1, 0, 1));
    }
}
