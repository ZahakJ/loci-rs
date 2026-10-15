//! Card state operations: suspend, bury, flag, unbury, the
//! day-rollover unbury; plus `forget_cards` and `resolve_leech`, which
//! the browser's bulk "Forget" and the study screen share.

use crate::collection::decks::deck_and_descendant_ids;
use crate::collection::internals::{meta, CollectionCore, IdTable};
use crate::collection::notes::{get_card, get_note};
use crate::collection::rows::{self, card_params, revlog_params, REVLOG_COLUMNS};
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::text::join_tags;
use crate::types::*;
use crate::undo::{self, Table, UndoSnapshot};
use std::collections::BTreeMap;

/// SQL expression giving the queue a card returns to when unsuspended/unburied (Anki
/// `restore_queue_after_bury_or_suspend`): new → 0, review → 2, learn/relearn → 1 when `due` is a
/// timestamp (> 1e9), else 3 (day-learn).
pub const RESTORED_QUEUE_SQL: &str =
    "CASE type WHEN 0 THEN 0 WHEN 2 THEN 2 ELSE (CASE WHEN due > 1000000000 THEN 1 ELSE 3 END) END";

const CHUNK: usize = 500;

fn unique(ids: &[i64]) -> Vec<i64> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|i| seen.insert(*i)).collect()
}

fn for_chunks(ids: &[i64], mut f: impl FnMut(&[i64], &str) -> Result<()>) -> Result<()> {
    for part in unique(ids).chunks(CHUNK) {
        f(part, &undo::marks(part.len()))?;
    }
    Ok(())
}

fn row_queue(row: &undo::RowSnapshot) -> i64 {
    row.get("queue").and_then(|v| v.as_i64()).unwrap_or(0)
}
fn row_id(row: &undo::RowSnapshot) -> i64 {
    row.get("id").and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Suspends (queue −1) or unsuspends (queue restored from type) cards. Undoable.
pub fn suspend_cards(core: &CollectionCore, ids: &[CardId], suspend: bool) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    core.db().transaction(|| {
        let suspended = CardQueue::Suspended as i64;
        let before: Vec<undo::RowSnapshot> = undo::snapshot_rows(core.db(), Table::Cards, ids)?
            .into_iter()
            .filter(|c| if suspend { row_queue(c) != suspended } else { row_queue(c) == suspended })
            .collect();
        if before.is_empty() {
            return Ok(());
        }
        let mtime = core.now_seconds();
        let ids: Vec<i64> = before.iter().map(row_id).collect();
        for_chunks(&ids, |part, marks| {
            let mut params: Vec<i64> = Vec::new();
            let sql = if suspend {
                params.push(suspended);
                params.push(mtime);
                format!("UPDATE cards SET queue = ?, mtime = ?, usn = -1 WHERE id IN ({marks})")
            } else {
                params.push(mtime);
                format!("UPDATE cards SET queue = {RESTORED_QUEUE_SQL}, mtime = ?, usn = -1 WHERE id IN ({marks})")
            };
            params.extend_from_slice(part);
            core.db().run(&sql, rusqlite::params_from_iter(params))?;
            Ok(())
        })?;
        undo::record(
            core.db(),
            core.now(),
            if suspend { "Suspend" } else { "Unsuspend" },
            &UndoSnapshot { cards: before, ..Default::default() },
        )?;
        core.mark_changed()
    })
}

/// Buries cards until tomorrow (queue −3). Suspended cards stay suspended. Undoable.
pub fn bury_cards(core: &CollectionCore, ids: &[CardId]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    core.db().transaction(|| {
        let before: Vec<undo::RowSnapshot> =
            undo::snapshot_rows(core.db(), Table::Cards, ids)?.into_iter().filter(|c| row_queue(c) >= 0).collect();
        if before.is_empty() {
            return Ok(());
        }
        let mtime = core.now_seconds();
        let ids: Vec<i64> = before.iter().map(row_id).collect();
        for_chunks(&ids, |part, marks| {
            let mut params = vec![CardQueue::UserBuried as i64, mtime];
            params.extend_from_slice(part);
            core.db().run(
                &format!("UPDATE cards SET queue = ?, mtime = ?, usn = -1 WHERE id IN ({marks})"),
                rusqlite::params_from_iter(params),
            )?;
            Ok(())
        })?;
        undo::record(core.db(), core.now(), "Bury", &UndoSnapshot { cards: before, ..Default::default() })?;
        core.mark_changed()
    })
}

/// Sets the flag colour (low 3 bits of `flags`) on cards. Undoable.
pub fn set_flag(core: &CollectionCore, ids: &[CardId], flag: i64) -> Result<()> {
    if !(0..=7).contains(&flag) {
        return Err(LociError::invalid(format!("Invalid flag {flag}")));
    }
    if ids.is_empty() {
        return Ok(());
    }
    core.db().transaction(|| {
        let before: Vec<undo::RowSnapshot> = undo::snapshot_rows(core.db(), Table::Cards, ids)?
            .into_iter()
            .filter(|c| (c.get("flags").and_then(|v| v.as_i64()).unwrap_or(0) & 7) != flag)
            .collect();
        if before.is_empty() {
            return Ok(());
        }
        let mtime = core.now_seconds();
        let ids: Vec<i64> = before.iter().map(row_id).collect();
        for_chunks(&ids, |part, marks| {
            let mut params = vec![flag, mtime];
            params.extend_from_slice(part);
            core.db().run(
                &format!("UPDATE cards SET flags = (flags & ~7) | ?, mtime = ?, usn = -1 WHERE id IN ({marks})"),
                rusqlite::params_from_iter(params),
            )?;
            Ok(())
        })?;
        undo::record(
            core.db(),
            core.now(),
            if flag == 0 { "Remove flag" } else { "Flag" },
            &UndoSnapshot { cards: before, ..Default::default() },
        )?;
        core.mark_changed()
    })
}

/// Unburies all buried cards (user and scheduler) in a deck subtree, or everywhere for None. Returns the count.
pub fn unbury_deck(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<i64> {
    core.db().transaction(|| {
        let buried = [CardQueue::UserBuried as i64, CardQueue::SchedBuried as i64];
        let ids: Vec<CardId> = match deck_id {
            None => core.db().all("SELECT id FROM cards WHERE queue IN (?, ?)", buried, |r| r.get(0))?,
            Some(did) => {
                let decks = deck_and_descendant_ids(core, did)?;
                let mut params: Vec<i64> = buried.to_vec();
                params.extend(decks.iter().copied());
                core.db().all(
                    &format!("SELECT id FROM cards WHERE queue IN (?, ?) AND did IN ({})", undo::marks(decks.len())),
                    rusqlite::params_from_iter(params),
                    |r| r.get(0),
                )?
            }
        };
        if ids.is_empty() {
            return Ok(0);
        }
        let before = undo::snapshot_rows(core.db(), Table::Cards, &ids)?;
        let mtime = core.now_seconds();
        for_chunks(&ids, |part, marks| {
            let mut params = vec![mtime];
            params.extend_from_slice(part);
            core.db().run(
                &format!("UPDATE cards SET queue = {RESTORED_QUEUE_SQL}, mtime = ?, usn = -1 WHERE id IN ({marks})"),
                rusqlite::params_from_iter(params),
            )?;
            Ok(())
        })?;
        undo::record(core.db(), core.now(), "Unbury", &UndoSnapshot { cards: before, ..Default::default() })?;
        core.mark_changed()?;
        Ok(ids.len() as i64)
    })
}

/// Anki's day-rollover unbury: the first time a new collection day is seen, every buried card
/// (sibling-buried and manually buried) returns to its queue. The last day handled is kept in
/// col_meta 'last_unburied'; when it is missing (a collection from before this rule) it is set to
/// today without unburying. Not undoable and no change marker (it is the scheduler's own
/// bookkeeping). Returns the number of cards unburied.
pub fn unbury_if_day_rolled_over(core: &CollectionCore) -> Result<i64> {
    let today = core.today()?;
    let raw = core.get_meta(meta::LAST_UNBURIED)?;
    let last = raw.as_deref().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
    let Some(last) = last else {
        core.set_meta(meta::LAST_UNBURIED, Some(&today.to_string()))?;
        return Ok(0);
    };
    if last >= today as f64 {
        return Ok(0);
    }
    core.db().transaction(|| {
        let n = core.db().run(
            &format!("UPDATE cards SET queue = {RESTORED_QUEUE_SQL}, mtime = ?, usn = -1 WHERE queue IN (?, ?)"),
            rusqlite::params![core.now_seconds(), CardQueue::UserBuried as i64, CardQueue::SchedBuried as i64],
        )?;
        core.set_meta(meta::LAST_UNBURIED, Some(&today.to_string()))?;
        Ok(n as i64)
    })
}

/// Writes every column of an existing card row.
pub fn update_card(core: &CollectionCore, card: &Card) -> Result<()> {
    let values = card_params(card);
    let rest: Vec<String> = rows::CARD_COLUMNS[1..].iter().map(|c| format!("\"{c}\" = ?")).collect();
    let mut params: Vec<rusqlite::types::Value> = values[1..].to_vec();
    params.push(values[0].clone());
    core.db().run(&format!("UPDATE cards SET {} WHERE id = ?", rest.join(", ")), rusqlite::params_from_iter(params))?;
    Ok(())
}

/// Anki "Reset" / forget: cards become new (type 0, queue 0, ivl 0, factor 0), FSRS memory and
/// last-review time cleared, due = original position (data.pos) when `restore_position` and known,
/// else appended after the last new position; reps/lapses zeroed when `reset_counts`. Writes a
/// Manual revlog row with ease 0 and factor 0 (Anki's reset marker, which FSRS treats as a history
/// break). Must run inside the caller's transaction; returns inserted revlog ids.
pub fn forget_cards(core: &CollectionCore, cards: &[Card], reset_counts: bool, restore_position: bool) -> Result<Vec<RevlogId>> {
    let now = core.now_seconds();
    let mut revlog_ids = Vec::new();
    for c in cards {
        let pos = match (restore_position, c.data.pos) {
            (true, Some(p)) => p,
            _ if c.ctype == CardType::New => c.due,
            _ => core.allocate_new_position()?,
        };
        let data = CardData {
            s: None,
            d: None,
            dr: None,
            decay: None,
            lrt: None,
            pos: None,
            cd: c.data.cd.clone(),
            extra: c.data.extra.clone(),
        };
        let next = Card {
            ctype: CardType::New,
            queue: CardQueue::New,
            due: pos,
            ivl: 0,
            factor: 0,
            left: 0,
            odue: 0,
            odid: 0,
            mtime: now,
            usn: -1,
            reps: if reset_counts { 0 } else { c.reps },
            lapses: if reset_counts { 0 } else { c.lapses },
            data,
            ..c.clone()
        };
        update_card(core, &next)?;
        if c.ctype != CardType::New {
            let id = core.new_id(IdTable::Revlog)?;
            let entry = RevlogEntry {
                id,
                cid: c.id,
                usn: -1,
                ease: 0,
                ivl: 0,
                last_ivl: c.ivl,
                factor: 0,
                time: 0,
                rtype: RevlogType::Manual,
            };
            core.db().run(&rows::insert_sql("revlog", &REVLOG_COLUMNS), rusqlite::params_from_iter(revlog_params(&entry)))?;
            revlog_ids.push(id);
        }
    }
    let max_pos = cards.iter().map(|c| c.data.pos.unwrap_or(0)).fold(0, i64::max);
    let next = core.get_meta(meta::NEXT_POS)?.and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
    if (max_pos + 1) as f64 > next {
        core.set_meta(meta::NEXT_POS, Some(&(max_pos + 1).to_string()))?;
    }
    Ok(revlog_ids)
}

/// Resolves a leech: `suspend` suspends the card; `reset` and `keep` drop the `leech` tag, and
/// `reset` also forgets the card. Undoable.
pub fn resolve_leech(core: &CollectionCore, card_id: CardId, action: LeechResolution) -> Result<()> {
    core.db().transaction(|| {
        let card = get_card(core, card_id)?;
        let card_before = undo::snapshot_rows(core.db(), Table::Cards, &[card_id])?;
        let note_before = undo::snapshot_rows(core.db(), Table::Notes, &[card.nid])?;
        let now = core.now_seconds();
        let mut inserted_revlog = Vec::new();
        if action == LeechResolution::Suspend {
            if card.queue != CardQueue::Suspended {
                core.db().run(
                    "UPDATE cards SET queue = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![CardQueue::Suspended as i64, now, card_id],
                )?;
            }
        } else {
            let note = get_note(core, card.nid)?;
            let tags: Vec<String> = note.tags.iter().filter(|t| t.to_lowercase() != "leech").cloned().collect();
            if tags.len() != note.tags.len() {
                core.db().run(
                    "UPDATE notes SET tags = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![join_tags(&tags), now, card.nid],
                )?;
            }
            if action == LeechResolution::Reset {
                inserted_revlog = forget_cards(core, std::slice::from_ref(&card), true, true)?;
            }
        }
        undo::record(
            core.db(),
            core.now(),
            &msg("study.undoLeech", "Struggling card", &[]),
            &UndoSnapshot { cards: card_before, notes: note_before, inserted_revlog, ..Default::default() },
        )?;
        core.mark_changed()
    })
}

/// Cards by id, in id order (helper for callers that forget several cards at once).
pub fn cards_by_ids(core: &CollectionCore, ids: &[CardId]) -> Result<Vec<Card>> {
    let mut out: BTreeMap<CardId, Card> = BTreeMap::new();
    for part in unique(ids).chunks(CHUNK) {
        let sql = format!("{} WHERE id IN ({})", rows::select_sql("cards", &rows::CARD_COLUMNS), undo::marks(part.len()));
        for c in core.db().all(&sql, rusqlite::params_from_iter(part.iter()), rows::row_to_card)? {
            out.insert(c.id, c);
        }
    }
    Ok(out.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    fn queues(h: &Harness, ids: &[CardId]) -> Vec<CardQueue> {
        ids.iter().map(|i| h.col.get_card(*i).unwrap().queue).collect()
    }

    // ── undo-cards ──
    #[test]
    fn undoes_delete_notes_notes_cards_fts_and_graves_come_back() {
        let mut h = make_collection();
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let a = h.add_note(&rev, 1, &["alpha", "beta"], &["t1"]);
        let b = h.add_note(&rev, 1, &["gamma", "delta"], &[]);
        let before =
            undo::snapshot_rows(h.col.core().db(), Table::Cards, &[a.card_ids[0], a.card_ids[1], b.card_ids[0], b.card_ids[1]])
                .unwrap();
        h.col.delete_notes(&[a.note_id, b.note_id]).unwrap();
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Delete 2 notes"));
        assert_eq!(h.count("SELECT count(*) FROM notes"), 0);
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Delete 2 notes");
        let after =
            undo::snapshot_rows(h.col.core().db(), Table::Cards, &[a.card_ids[0], a.card_ids[1], b.card_ids[0], b.card_ids[1]])
                .unwrap();
        assert_eq!(after, before);
        assert_eq!(h.col.get_note(a.note_id).unwrap().tags, strs(&["t1"]));
        assert_eq!(h.count("SELECT count(*) FROM graves"), 0);
        assert_eq!(h.count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'alpha'"), 1);
        assert!(h.col.check_integrity().unwrap().ok);
        // Next undo reverts the second add.
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Add note");
        assert_eq!(h.count("SELECT count(*) FROM notes"), 1);
    }

    #[test]
    fn undoes_suspend_and_unsuspend_restoring_the_exact_queue() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let mut ids = Vec::new();
        for f in ["a", "b", "c"] {
            ids.extend(h.add_note(&basic, 1, &[f, ""], &[]).card_ids);
        }
        // Make one a review card and one an intraday learning card.
        h.col.core().db().run("UPDATE cards SET type = 2, queue = 2, due = 5, ivl = 3 WHERE id = ?", [ids[1]]).unwrap();
        h.col.core().db().run("UPDATE cards SET type = 1, queue = 1, due = 1773200000 WHERE id = ?", [ids[2]]).unwrap();
        h.col.suspend_cards(&ids, true).unwrap();
        assert!(queues(&h, &ids).iter().all(|q| *q == CardQueue::Suspended));
        h.col.suspend_cards(&ids, false).unwrap();
        assert_eq!(queues(&h, &ids), [CardQueue::New, CardQueue::Review, CardQueue::Learn]);
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Unsuspend");
        assert_eq!(h.col.get_card(ids[0]).unwrap().queue, CardQueue::Suspended);
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Suspend");
        assert_eq!(queues(&h, &ids), [CardQueue::New, CardQueue::Review, CardQueue::Learn]);
        // Suspending already-suspended cards records nothing.
        h.col.suspend_cards(&ids, false).unwrap();
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Add note"));
    }

    #[test]
    fn caps_the_stack_at_50_entries_and_returns_none_when_empty() {
        let mut h = make_collection();
        assert_eq!(h.col.undo().unwrap(), None);
        let basic = h.notetype_by_name("Basic");
        for i in 0..(undo::UNDO_CAP + 5) {
            h.add_note(&basic, 1, &[&format!("n{i}"), ""], &[]);
        }
        assert_eq!(h.count("SELECT count(*) FROM undo_log"), undo::UNDO_CAP);
        let mut undone = 0;
        while h.col.undo().unwrap().is_some() {
            undone += 1;
        }
        assert_eq!(undone, undo::UNDO_CAP);
        assert_eq!(h.count("SELECT count(*) FROM notes"), 5);
    }

    #[test]
    fn a_failed_operation_leaves_no_undo_entry_and_no_partial_writes() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        assert!(h.col.add_note(basic.id, 1, &strs(&["", "x"]), &[]).is_err());
        assert_eq!(h.col.undo_label().unwrap(), None);
        assert_eq!(h.count("SELECT count(*) FROM notes"), 0);
    }

    #[test]
    fn bury_unbury_flags() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("P::C", None).unwrap();
        let c1 = h.add_note(&basic, deck.id, &["a", ""], &[]).card_ids[0];
        let c2 = h.add_note(&basic, 1, &["b", ""], &[]).card_ids[0];
        h.col.bury_cards(&[c1, c2]).unwrap();
        assert_eq!(h.col.get_card(c1).unwrap().queue, CardQueue::UserBuried);
        let parent = h.col.core().deck_id_by_name("P").unwrap().unwrap();
        assert_eq!(h.col.unbury_deck(Some(parent)).unwrap(), 1);
        assert_eq!(h.col.get_card(c1).unwrap().queue, CardQueue::New);
        assert_eq!(h.col.get_card(c2).unwrap().queue, CardQueue::UserBuried);
        assert_eq!(h.col.unbury_deck(None).unwrap(), 1);
        assert_eq!(h.col.unbury_deck(None).unwrap(), 0);
        h.col.set_flag(&[c1], 3).unwrap();
        h.col.core().db().run("UPDATE cards SET flags = flags | 8 WHERE id = ?", [c1]).unwrap();
        h.col.set_flag(&[c1], 5).unwrap();
        assert_eq!(h.col.get_card(c1).unwrap().flags, 8 | 5);
        assert_eq!(h.col.set_flag(&[c1], 9).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(h.col.get_card(c1).unwrap().ctype, CardType::New);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Flag"));
        h.col.set_flag(&[c1], 0).unwrap();
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Remove flag"));
    }

    #[test]
    fn day_rollover_unburies_once_per_new_day() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let c1 = h.add_note(&basic, 1, &["a", ""], &[]).card_ids[0];
        h.col.bury_cards(&[c1]).unwrap();
        assert_eq!(h.meta("last_unburied").as_deref(), Some("0"));
        assert_eq!(unbury_if_day_rolled_over(h.col.core()).unwrap(), 0);
        h.clock.advance(86_400_000);
        assert_eq!(h.col.today().unwrap(), 1);
        assert_eq!(h.col.get_card(c1).unwrap().queue, CardQueue::New);
        assert_eq!(h.meta("last_unburied").as_deref(), Some("1"));
        // A collection without the marker only records today.
        h.col.core().set_meta(meta::LAST_UNBURIED, None).unwrap();
        h.col.bury_cards(&[c1]).unwrap();
        assert_eq!(unbury_if_day_rolled_over(h.col.core()).unwrap(), 0);
        assert_eq!(h.col.get_card(c1).unwrap().queue, CardQueue::UserBuried);
    }

    #[test]
    fn resolve_leech_and_forget_cards() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let r = h.add_note(&basic, 1, &["a", ""], &["leech", "keep"]);
        let cid = r.card_ids[0];
        h.col
            .core()
            .db()
            .run("UPDATE cards SET type = 2, queue = 2, due = 9, ivl = 4, reps = 12, lapses = 9, data = '{\"s\":3.5,\"d\":6,\"pos\":1,\"cd\":\"x\"}' WHERE id = ?", [cid])
            .unwrap();
        h.col.resolve_leech(cid, LeechResolution::Reset).unwrap();
        let c = h.col.get_card(cid).unwrap();
        assert_eq!((c.ctype, c.queue, c.due, c.ivl, c.reps, c.lapses), (CardType::New, CardQueue::New, 1, 0, 0, 0));
        assert_eq!(c.data.s, None);
        assert_eq!(c.data.cd.as_deref(), Some("x"));
        assert_eq!(h.col.get_note(r.note_id).unwrap().tags, strs(&["keep"]));
        assert_eq!(h.count("SELECT count(*) FROM revlog WHERE type = 4 AND ease = 0"), 1);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Struggling card"));
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Struggling card");
        let c = h.col.get_card(cid).unwrap();
        assert_eq!((c.ctype, c.reps), (CardType::Review, 12));
        assert_eq!(h.col.get_note(r.note_id).unwrap().tags, strs(&["keep", "leech"]));
        assert_eq!(h.count("SELECT count(*) FROM revlog"), 0);
        h.col.resolve_leech(cid, LeechResolution::Suspend).unwrap();
        assert_eq!(h.col.get_card(cid).unwrap().queue, CardQueue::Suspended);
        assert_eq!(h.col.get_note(r.note_id).unwrap().tags, strs(&["keep", "leech"]));
        h.col.resolve_leech(cid, LeechResolution::Keep).unwrap();
        assert_eq!(h.col.get_note(r.note_id).unwrap().tags, strs(&["keep"]));
        assert_eq!(h.col.get_card(cid).unwrap().queue, CardQueue::Suspended);
    }
}
