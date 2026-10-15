//! Shared helpers that turn the collection's clock, settings and decks into the inputs the pure
//! scheduler, search and stats code expect. Units:
//! ms for `now_ms`, seconds for `crt`/`next_day_at`, collection days for `today`.
//!
//! The SQLite loaders live in `scheduler::queue`; this module adds what they need from the core.

use crate::collection::internals::CollectionCore;
use crate::error::Result;
use crate::scheduler::{
    card_revlog, memory_state_from_revlog, CardSched, QueueDeck, RevlogConversionInput, SchedClock, SchedulingInput,
};
use crate::search::sql::SearchContext;
use crate::types::*;
use std::collections::HashMap;

/// SQLite bound-parameter chunk size.
pub const SQL_CHUNK: usize = 500;

/// The collection clock in the units the scheduler uses. Built from the core's own day numbering
/// (creation offset and injected time zone honoured), not from `SchedClock::at`.
pub fn sched_clock(core: &CollectionCore) -> Result<SchedClock> {
    let s = core.settings()?;
    Ok(SchedClock {
        now_ms: core.now(),
        today: core.today()?,
        crt: core.crt()?,
        rollover_hour: s.rollover_hour,
        next_day_at: core.next_day_at()?,
        learn_ahead_secs: ((s.learn_ahead_minutes as f64) * 60.0).round().max(0.0) as i64,
    })
}

/// Search context for `search::sql` (field index loaded lazily by `run_search`).
pub fn search_context(core: &CollectionCore, current_deck_id: Option<DeckId>) -> Result<SearchContext> {
    let c = sched_clock(core)?;
    Ok(SearchContext {
        today: c.today,
        now_ms: c.now_ms,
        crt: c.crt,
        rollover_hour: c.rollover_hour,
        current_deck_id,
        next_day_at: Some(c.next_day_at),
        fields: None,
    })
}

/// True when `name` is `parent` itself or below it (case-insensitive, like the NOCASE column).
pub fn is_self_or_child(name: &str, parent: &str) -> bool {
    let n = name.to_lowercase();
    let p = parent.to_lowercase();
    n == p || n.starts_with(&format!("{p}::"))
}

/// Ids of a deck and its descendants; None for "all decks". Not-found for an unknown deck.
pub fn subtree_deck_ids(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<Option<Vec<DeckId>>> {
    let Some(deck_id) = deck_id else { return Ok(None) };
    let deck = core.deck(deck_id)?;
    Ok(Some(core.all_decks()?.into_iter().filter(|d| is_self_or_child(&d.name, &deck.name)).map(|d| d.id).collect()))
}

/// Every deck with its preset config, for the queue builder. Configs are shared per preset.
pub fn queue_decks(core: &CollectionCore, decks: &[Deck]) -> Result<Vec<QueueDeck>> {
    let mut by_preset: HashMap<DeckConfigId, DeckConfig> = HashMap::new();
    let mut out = Vec::with_capacity(decks.len());
    for d in decks {
        let config = match by_preset.get(&d.config_id) {
            Some(c) => c.clone(),
            None => {
                let c = core.deck_config(d.id)?;
                by_preset.insert(d.config_id, c.clone());
                c
            }
        };
        out.push(QueueDeck { id: d.id, name: d.name.clone(), config, today: Some(d.today.clone()) });
    }
    Ok(out)
}

/// Scheduler input for answering or previewing `card` now. Under FSRS, a non-new card without a
/// stored memory state gets one replayed from its revlog (Anki's update_memory_state), and the
/// last review time comes from the revlog when card.data.lrt is missing.
pub fn scheduling_input<'a>(
    core: &CollectionCore,
    card: &'a Card,
    config: &'a DeckConfig,
    clock: &SchedClock,
) -> Result<SchedulingInput<'a>> {
    let mut input = SchedulingInput::new(card, config, clock, true);
    if card.ctype == CardType::New {
        return Ok(input);
    }
    let needs_memory = config.scheduler == SchedulerKind::Fsrs && (card.data.s.is_none() || card.data.d.is_none());
    let needs_last = !card.data.lrt.is_some_and(|l| l > 0);
    if !needs_memory && !needs_last {
        return Ok(input);
    }
    let revlog = card_revlog(core.db(), card.id)?;
    if needs_last {
        if let Some(last) = revlog.iter().rfind(|r| (1..=4).contains(&r.ease)) {
            input.last_review_ms = Some(last.id);
        }
    }
    if needs_memory {
        let memory = memory_state_from_revlog(&RevlogConversionInput {
            card: CardSched { ctype: card.ctype, ivl: card.ivl, factor: card.factor },
            revlog: &revlog,
            params: &config.fsrs_params,
            historical_retention: config.historical_retention,
            next_day_at_ms: clock.next_day_at * 1000,
            ignore_before_ms: 0,
        })?;
        if let Some(m) = memory {
            input.memory = Some(m);
        }
    }
    Ok(input)
}

/// Splits ids into SQL-sized chunks.
pub fn chunked<T: Clone>(xs: &[T], size: usize) -> Vec<Vec<T>> {
    xs.chunks(size.max(1)).map(|c| c.to_vec()).collect()
}

/// Placeholders for `n` bound values.
pub fn marks(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// A deck and its ancestors, nearest first: `[self, parent, grandparent, …]`.
pub fn self_and_ancestors(core: &CollectionCore, deck_id: DeckId) -> Result<Vec<Deck>> {
    let all = core.all_decks()?;
    let Some(me) = all.iter().find(|d| d.id == deck_id).cloned() else { return Ok(vec![]) };
    let by_name: HashMap<String, &Deck> = all.iter().map(|d| (d.name.to_lowercase(), d)).collect();
    let parts: Vec<&str> = me.name.split("::").collect();
    let mut out = vec![me.clone()];
    for i in (1..parts.len()).rev() {
        if let Some(p) = by_name.get(&parts[..i].join("::").to_lowercase()) {
            out.push((*p).clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;

    #[test]
    fn clock_subtrees_and_ancestors() {
        let h = make_collection();
        let c = sched_clock(h.col.core()).unwrap();
        assert_eq!(c.today, 0);
        assert_eq!(c.learn_ahead_secs, 1200);
        assert_eq!(c.next_day_at, c.crt + 86_400);
        let a = h.col.create_deck("A::B::C", None).unwrap();
        let b = h.col.core().deck_id_by_name("A::B").unwrap().unwrap();
        let top = h.col.core().deck_id_by_name("A").unwrap().unwrap();
        let mut ids = subtree_deck_ids(h.col.core(), Some(top)).unwrap().unwrap();
        ids.sort();
        let mut want = vec![top, b, a.id];
        want.sort();
        assert_eq!(ids, want);
        assert_eq!(subtree_deck_ids(h.col.core(), None).unwrap(), None);
        assert_eq!(subtree_deck_ids(h.col.core(), Some(999)).unwrap_err().code, crate::error::ErrorCode::NotFound);
        let anc: Vec<String> = self_and_ancestors(h.col.core(), a.id).unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(anc, ["A::B::C", "A::B", "A"]);
        assert_eq!(chunked(&[1, 2, 3, 4, 5], 2), vec![vec![1, 2], vec![3, 4], vec![5]]);
        assert_eq!(marks(3), "?,?,?");
        let qd = queue_decks(h.col.core(), &h.col.core().all_decks().unwrap()).unwrap();
        assert_eq!(qd.len(), 4);
        assert!(qd.iter().all(|d| d.config.new_per_day == 20));
        let sc = search_context(h.col.core(), Some(1)).unwrap();
        assert_eq!(sc.current_deck_id, Some(1));
    }

    #[test]
    fn scheduling_input_replays_memory_from_revlog() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let r = h.add_note(&basic, 1, &["q", "a"], &[]);
        let cid = r.card_ids[0];
        let db = h.col.core().db();
        db.run("UPDATE cards SET type = 2, queue = 2, due = 5, ivl = 3, reps = 1 WHERE id = ?", [cid]).unwrap();
        db.run(
            "INSERT INTO revlog (id, cid, usn, ease, ivl, last_ivl, factor, time, type) VALUES (?, ?, -1, 3, 3, 0, 2500, 4000, 0)",
            rusqlite::params![T0 - 86_400_000, cid],
        )
        .unwrap();
        let card = h.col.get_card(cid).unwrap();
        let cfg = h.col.core().deck_config(1).unwrap();
        let clock = sched_clock(h.col.core()).unwrap();
        let input = scheduling_input(h.col.core(), &card, &cfg, &clock).unwrap();
        assert_eq!(input.last_review_ms, Some(T0 - 86_400_000));
        assert!(input.memory.is_some());
        // A new card needs nothing.
        let r2 = h.add_note(&basic, 1, &["q2", "a"], &[]);
        let fresh = h.col.get_card(r2.card_ids[0]).unwrap();
        let input = scheduling_input(h.col.core(), &fresh, &cfg, &clock).unwrap();
        assert!(input.memory.is_none() && input.last_review_ms.is_none());
    }
}

#[cfg(test)]
mod name_tests {
    use super::is_self_or_child;

    #[test]
    fn is_self_or_child_is_case_insensitive_and_component_aware() {
        assert!(is_self_or_child("a::b", "A"));
        assert!(is_self_or_child("A", "a"));
        assert!(!is_self_or_child("AB", "A"));
        assert!(!is_self_or_child("A", "A::B"));
    }
}
