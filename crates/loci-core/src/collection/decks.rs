//! Deck operations (recall `src/core/collection/decks.ts`): '::'-separated hierarchy, tree with
//! counts, rename cascading to descendants, delete with cards (undoable), today's-limit extension.

use crate::collection::counter::CounterFn;
use crate::collection::internals::{CollectionCore, IdTable, DEFAULT_DECK_ID, DEFAULT_PRESET_ID};
use crate::collection::rows::{self, deck_params, deck_today_to_json, parse_deck_today, DECK_COLUMNS};
use crate::db::unicase_compare;
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::types::*;
use crate::undo::{self, GraveRow, Table, UndoSnapshot};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub const DECK_SEPARATOR: &str = "::";

/// `deleteDeck` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteDeckResult {
    pub cards: i64,
}

/// A single deck column `setDeckField` may change.
#[derive(Debug, Clone, PartialEq)]
pub enum DeckField {
    Collapsed(bool),
    ConfigId(DeckConfigId),
    Description(String),
}

/// Normalises a human deck name: components trimmed, empty components dropped, control characters
/// removed. `invalid` for an empty result.
pub fn normalise_deck_name(name: &str) -> Result<String> {
    let cleaned: String = name.chars().filter(|c| !(*c <= '\u{1f}' || *c == '\u{7f}')).collect();
    let parts: Vec<&str> = cleaned.split(DECK_SEPARATOR).map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(LociError::invalid("Deck name is empty"));
    }
    Ok(parts.join(DECK_SEPARATOR))
}

/// 'A::B::C' → ['A', 'A::B']
pub fn ancestor_names(name: &str) -> Vec<String> {
    let parts: Vec<&str> = name.split(DECK_SEPARATOR).collect();
    (1..parts.len()).map(|i| parts[..i].join(DECK_SEPARATOR)).collect()
}

/// True when `name` is `parent` itself or below it (case-insensitive, like the NOCASE column).
pub fn is_self_or_descendant(name: &str, parent: &str) -> bool {
    let n = name.to_lowercase();
    let p = parent.to_lowercase();
    n == p || n.starts_with(&format!("{p}{DECK_SEPARATOR}"))
}

fn new_deck(id: DeckId, name: &str, config_id: DeckConfigId, mtime: Seconds) -> Deck {
    Deck {
        id,
        name: name.to_string(),
        config_id,
        description: String::new(),
        collapsed: false,
        today: DeckToday::default(),
        mtime,
        usn: -1,
    }
}

fn insert_deck(core: &CollectionCore, name: &str, config_id: DeckConfigId) -> Result<Deck> {
    let deck = new_deck(core.new_id(IdTable::Decks)?, name, config_id, core.now_seconds());
    core.db().run(&rows::insert_sql("decks", &DECK_COLUMNS), rusqlite::params_from_iter(deck_params(&deck, "{}")))?;
    Ok(deck)
}

/// Creates any missing ancestors of `name`; returns ids of decks created.
fn ensure_ancestors(core: &CollectionCore, name: &str, config_id: DeckConfigId) -> Result<Vec<DeckId>> {
    let mut created = Vec::new();
    for anc in ancestor_names(name) {
        if core.deck_id_by_name(&anc)?.is_none() {
            created.push(insert_deck(core, &anc, config_id)?.id);
        }
    }
    Ok(created)
}

fn preset_exists(core: &CollectionCore, id: DeckConfigId) -> Result<bool> {
    Ok(core.db().scalar::<i64, _>("SELECT 1 FROM deck_configs WHERE id = ?", [id])?.is_some())
}

/// Creates a deck (and missing parents). Preset: given, else the parent's, else 1.
pub fn create_deck(core: &CollectionCore, raw_name: &str, config_id: Option<DeckConfigId>) -> Result<Deck> {
    let name = normalise_deck_name(raw_name)?;
    core.db().transaction(|| {
        if core.deck_id_by_name(&name)?.is_some() {
            return Err(LociError::conflict(format!("A deck named \"{name}\" already exists")));
        }
        if let Some(cfg) = config_id {
            if !preset_exists(core, cfg)? {
                return Err(LociError::not_found(format!("Preset {cfg} not found")));
            }
        }
        let cfg = match config_id {
            Some(c) => c,
            None => {
                let parents = ancestor_names(&name);
                let parent_id = match parents.last() {
                    Some(p) => core.deck_id_by_name(p)?,
                    None => None,
                };
                match parent_id {
                    Some(pid) => core.deck(pid)?.config_id,
                    None => DEFAULT_PRESET_ID,
                }
            }
        };
        ensure_ancestors(core, &name, cfg)?;
        let deck = insert_deck(core, &name, cfg)?;
        core.mark_changed()?;
        Ok(deck)
    })
}

/// Finds or creates a deck by name (used by importers and the add screen).
pub fn ensure_deck(core: &CollectionCore, raw_name: &str) -> Result<DeckId> {
    let name = normalise_deck_name(raw_name)?;
    core.db().transaction(|| match core.deck_id_by_name(&name)? {
        Some(id) => Ok(id),
        None => Ok(create_deck(core, &name, None)?.id),
    })
}

/// Renames a deck; descendants follow ('A::x' → 'B::x'); missing new parents are created.
pub fn rename_deck(core: &CollectionCore, id: DeckId, raw_name: &str) -> Result<()> {
    let name = normalise_deck_name(raw_name)?;
    core.db().transaction(|| {
        let deck = core.deck(id)?;
        let old = deck.name.clone();
        if old == name {
            return Ok(());
        }
        if is_self_or_descendant(&name, &old) && name.to_lowercase() != old.to_lowercase() {
            return Err(LociError::invalid("A deck cannot be moved inside itself"));
        }
        if let Some(clash) = core.deck_id_by_name(&name)? {
            if clash != id {
                return Err(LociError::conflict(format!("A deck named \"{name}\" already exists")));
            }
        }
        let descendants: Vec<(DeckId, String)> = core
            .db()
            .all("SELECT id, name FROM decks WHERE id != ?", [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .into_iter()
            .filter(|(_, n)| is_self_or_descendant(n, &old))
            .collect();
        let mtime = core.now_seconds();
        // Two-phase rename avoids transient UNIQUE clashes between renamed siblings. Control chars
        // never survive normalise_deck_name (NUL would truncate in bindings).
        let tmp_prefix = format!("\u{1}rename-{id}-");
        core.db().run("UPDATE decks SET name = ?, mtime = ?, usn = -1 WHERE id = ?", rusqlite::params![tmp_prefix, mtime, id])?;
        for (did, _) in &descendants {
            core.db().run("UPDATE decks SET name = ? WHERE id = ?", rusqlite::params![format!("{tmp_prefix}{did}"), did])?;
        }
        ensure_ancestors(core, &name, deck.config_id)?;
        core.db().run("UPDATE decks SET name = ? WHERE id = ?", rusqlite::params![name, id])?;
        for (did, dname) in &descendants {
            let new_name = format!("{name}{}", &dname[old.len()..]);
            if core.deck_id_by_name(&new_name)?.is_some() {
                return Err(LociError::conflict(format!("A deck named \"{new_name}\" already exists")));
            }
            core.db()
                .run("UPDATE decks SET name = ?, mtime = ?, usn = -1 WHERE id = ?", rusqlite::params![new_name, mtime, did])?;
        }
        core.mark_changed()
    })
}

/// Ids of a deck and all its descendants.
pub fn deck_and_descendant_ids(core: &CollectionCore, id: DeckId) -> Result<Vec<DeckId>> {
    let deck = core.deck(id)?;
    let all: Vec<(DeckId, String)> = core.db().all("SELECT id, name FROM decks", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(all.into_iter().filter(|(_, n)| is_self_or_descendant(n, &deck.name)).map(|(i, _)| i).collect())
}

fn insert_graves(core: &CollectionCore, graves: &[GraveRow]) -> Result<()> {
    for g in graves {
        core.db().run("INSERT INTO graves (oid, type, usn) VALUES (?, ?, -1)", rusqlite::params![g.oid, g.gtype])?;
    }
    Ok(())
}

/// Deletes a deck, its descendants and their cards; notes left without cards are deleted too
/// (Anki semantics). The Default deck cannot be deleted. Undoable.
pub fn delete_deck(core: &CollectionCore, id: DeckId) -> Result<DeleteDeckResult> {
    if id == DEFAULT_DECK_ID {
        return Err(LociError::invalid("The Default deck cannot be deleted"));
    }
    core.db().transaction(|| {
        let deck_ids = deck_and_descendant_ids(core, id)?;
        if deck_ids.contains(&DEFAULT_DECK_ID) {
            return Err(LociError::invalid("The Default deck cannot be deleted"));
        }
        let deck_rows = undo::snapshot_rows(core.db(), Table::Decks, &deck_ids)?;
        let m = undo::marks(deck_ids.len());
        let mut params: Vec<i64> = deck_ids.clone();
        params.extend(deck_ids.iter().copied());
        let card_ids: Vec<CardId> = core.db().all(
            &format!("SELECT id FROM cards WHERE did IN ({m}) OR odid IN ({m})"),
            rusqlite::params_from_iter(params),
            |r| r.get(0),
        )?;
        let card_rows = undo::snapshot_rows(core.db(), Table::Cards, &card_ids)?;
        let mut note_ids: Vec<NoteId> = Vec::new();
        let mut seen = HashSet::new();
        for c in &card_rows {
            if let Some(nid) = c.get("nid").and_then(|v| v.as_i64()) {
                if seen.insert(nid) {
                    note_ids.push(nid);
                }
            }
        }
        undo::delete_rows_by_id(core.db(), "cards", &card_ids)?;
        // Notes that now have no cards are removed as well.
        let mut orphan_notes = Vec::new();
        for nid in note_ids {
            if core.db().scalar::<i64, _>("SELECT 1 FROM cards WHERE nid = ? LIMIT 1", [nid])?.is_none() {
                orphan_notes.push(nid);
            }
        }
        let note_rows = undo::snapshot_rows(core.db(), Table::Notes, &orphan_notes)?;
        undo::delete_rows_by_id(core.db(), "notes", &orphan_notes)?;
        undo::delete_rows_by_id(core.db(), "decks", &deck_ids)?;
        let mut graves: Vec<GraveRow> = card_ids.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Card as i64 }).collect();
        graves.extend(orphan_notes.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Note as i64 }));
        graves.extend(deck_ids.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Deck as i64 }));
        insert_graves(core, &graves)?;
        undo::record(
            core.db(),
            core.now(),
            "Delete deck",
            &UndoSnapshot { decks: deck_rows, cards: card_rows, notes: note_rows, graves, ..Default::default() },
        )?;
        core.mark_changed()?;
        Ok(DeleteDeckResult { cards: card_ids.len() as i64 })
    })
}

pub fn set_deck_field(core: &CollectionCore, id: DeckId, field: DeckField) -> Result<()> {
    core.db().transaction(|| {
        core.deck(id)?;
        let mtime = core.now_seconds();
        match &field {
            DeckField::Collapsed(c) => {
                core.db().run(
                    "UPDATE decks SET collapsed = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![*c as i64, mtime, id],
                )?;
            }
            DeckField::ConfigId(cfg) => {
                if !preset_exists(core, *cfg)? {
                    return Err(LociError::not_found(format!("Preset {cfg} not found")));
                }
                core.db()
                    .run("UPDATE decks SET config_id = ?, mtime = ?, usn = -1 WHERE id = ?", rusqlite::params![cfg, mtime, id])?;
            }
            DeckField::Description(d) => {
                core.db()
                    .run("UPDATE decks SET description = ?, mtime = ?, usn = -1 WHERE id = ?", rusqlite::params![d, mtime, id])?;
            }
        }
        if !matches!(field, DeckField::Collapsed(_)) {
            core.mark_changed()?;
        }
        Ok(())
    })
}

/// Orders deck names component-wise, case-insensitively ('A::B' sorts right after 'A').
pub fn compare_deck_names(a: &str, b: &str) -> Ordering {
    let pa: Vec<&str> = a.split(DECK_SEPARATOR).collect();
    let pb: Vec<&str> = b.split(DECK_SEPARATOR).collect();
    for (x, y) in pa.iter().zip(pb.iter()) {
        let c = unicase_compare(x, y);
        if c != Ordering::Equal {
            return c;
        }
    }
    pa.len().cmp(&pb.len())
}

/// Builds the deck tree. Root is a virtual "All decks" node (deck id 0, level 0). Counts come from
/// `counter`; `total_cards` counts cards in each subtree. The Default deck is hidden when it is
/// empty, has no children and other decks exist (as Anki does).
pub fn deck_tree(core: &CollectionCore, counter: &CounterFn) -> Result<DeckTreeNode> {
    let mut decks = core.all_decks()?;
    decks.sort_by(|a, b| compare_deck_names(&a.name, &b.name));
    let counts = counter(core, &decks)?;
    let own: HashMap<DeckId, i64> =
        core.db().all("SELECT did, count(*) FROM cards GROUP BY did", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect();

    let mut root = DeckTreeNode {
        deck: new_deck(0, "", DEFAULT_PRESET_ID, 0),
        children: vec![],
        level: 0,
        new_count: 0,
        learn_count: 0,
        review_count: 0,
        estimated_minutes: 0,
        total_cards: 0,
    };
    root.deck.usn = 0;
    // Nodes are built flat, then attached by parent name; `paths` maps a lowercased name to the
    // child-index path from the root.
    let mut paths: HashMap<String, Vec<usize>> = HashMap::new();
    for deck in decks {
        let c = counts.get(&deck.id).copied().unwrap_or_default();
        let parts: Vec<&str> = deck.name.split(DECK_SEPARATOR).collect();
        let parent_name = parts[..parts.len() - 1].join(DECK_SEPARATOR);
        let node = DeckTreeNode {
            level: parts.len() as i64,
            children: vec![],
            new_count: c.new,
            learn_count: c.learn,
            review_count: c.review,
            estimated_minutes: c.estimated_minutes,
            total_cards: own.get(&deck.id).copied().unwrap_or(0),
            deck: deck.clone(),
        };
        let key = deck.name.to_lowercase();
        let parent_path = if parent_name.is_empty() { None } else { paths.get(&parent_name.to_lowercase()).cloned() };
        let mut path = parent_path.clone().unwrap_or_default();
        let parent = match parent_path {
            Some(p) => node_at_mut(&mut root, &p),
            None => &mut root,
        };
        path.push(parent.children.len());
        parent.children.push(node);
        paths.insert(key, path);
    }
    // Subtree totals (post-order) and level fix-up for orphans attached to the root.
    for ch in &mut root.children {
        total(ch, 1);
    }
    let n_children = root.children.len();
    root.children.retain(|n| !(n.deck.id == DEFAULT_DECK_ID && n.total_cards == 0 && n.children.is_empty() && n_children > 1));
    for ch in &root.children {
        root.new_count += ch.new_count;
        root.learn_count += ch.learn_count;
        root.review_count += ch.review_count;
        root.estimated_minutes += ch.estimated_minutes;
        root.total_cards += ch.total_cards;
    }
    Ok(root)
}

fn node_at_mut<'a>(root: &'a mut DeckTreeNode, path: &[usize]) -> &'a mut DeckTreeNode {
    let mut n = root;
    for &i in path {
        n = &mut n.children[i];
    }
    n
}

fn total(n: &mut DeckTreeNode, level: i64) -> i64 {
    n.level = level;
    let mut sum = 0;
    for ch in &mut n.children {
        sum += total(ch, level + 1);
    }
    n.total_cards += sum;
    n.total_cards
}

/// Inserts the Default deck if missing (id 1). Returns true when it was created.
pub fn ensure_default_deck(core: &CollectionCore) -> Result<bool> {
    if core.db().scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [DEFAULT_DECK_ID])?.is_some() {
        return Ok(false);
    }
    let mut name = "Default".to_string();
    let mut i = 1;
    while core.deck_id_by_name(&name)?.is_some() {
        name = format!("Default ({i})");
        i += 1;
    }
    let deck = new_deck(DEFAULT_DECK_ID, &name, DEFAULT_PRESET_ID, core.now_seconds());
    core.db().run(&rows::insert_sql("decks", &DECK_COLUMNS), rusqlite::params_from_iter(deck_params(&deck, "{}")))?;
    Ok(true)
}

/// Anki's "increase today's limits" (custom study): lowers today's new/review counters of the
/// deck and its descendants by `extra_new` / `extra_review`, so that many more cards are served
/// today; the counters (decks.today) may go negative and reset at the next day as usual.
/// Undoable. Returns the deck's counts after the change.
pub fn extend_today_limits(
    core: &CollectionCore,
    counter: &CounterFn,
    deck_id: DeckId,
    extra_new: i64,
    extra_review: i64,
) -> Result<StudyCounts> {
    let deck = core.deck(deck_id)?;
    let check = |x: i64, what: &str| -> Result<i64> {
        if !(0..=9999).contains(&x) {
            return Err(LociError::invalid(format!("Extra {what} cards must be a whole number from 0 to 9999")));
        }
        Ok(x)
    };
    let n = check(extra_new, "new")?;
    let r = check(extra_review, "review")?;
    if n > 0 || r > 0 {
        core.db().transaction(|| {
            let today = core.today()?;
            let before = undo::snapshot_rows(core.db(), Table::Decks, &deck_and_descendant_ids(core, deck_id)?)?;
            for row in &before {
                let id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let t = parse_deck_today(row.get("today").and_then(|v| v.as_str()).unwrap_or("{}"));
                // Counters from an earlier day are stale: today starts from zero (and carries today's day).
                let base = if t.day == today { t } else { DeckToday { day: today, ..DeckToday::default() } };
                // A counter already past the preset's limit (e.g. the limit was lowered after studying)
                // counts as exactly used up, so the deck gets `n` more than it has left now, not fewer.
                let cfg = core.deck_config(id)?;
                let used_new = base.new.min(cfg.new_per_day.max(0));
                let used_rev = base.rev.min(cfg.reviews_per_day.max(0));
                let next = DeckToday { day: today, new: used_new - n, rev: used_rev - r, ..base };
                core.db().run(
                    "UPDATE decks SET today = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![deck_today_to_json(&next), core.now_seconds(), id],
                )?;
            }
            undo::record(
                core.db(),
                core.now(),
                &msg("study.undoExtendLimits", "Extend today's limits", &[]),
                &UndoSnapshot { decks: before, ..Default::default() },
            )?;
            core.mark_changed()
        })?;
    }
    let c = counter(core, &core.all_decks()?)?.get(&deck.id).copied().unwrap_or_default();
    Ok(StudyCounts { new: c.new, learn: c.learn, review: c.review })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::counter::DeckCounts;
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    type Level3 = (String, i64, Vec<String>);
    type Level2 = (String, i64, Vec<Level3>);

    fn names(n: &DeckTreeNode) -> (String, i64, Vec<Level2>) {
        (
            n.deck.name.clone(),
            n.level,
            n.children
                .iter()
                .map(|c| {
                    (
                        c.deck.name.clone(),
                        c.level,
                        c.children
                            .iter()
                            .map(|g| {
                                (
                                    g.deck.name.clone(),
                                    g.level,
                                    g.children.iter().map(|x| format!("{}@{}", x.deck.name, x.level)).collect(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn normalises_names() {
        assert_eq!(normalise_deck_name("  A ::  B::::C ").unwrap(), "A::B::C");
        assert!(normalise_deck_name(" :: ").is_err());
        assert_eq!(compare_deck_names("a::b", "B"), Ordering::Less);
        assert_eq!(compare_deck_names("A", "A::B"), Ordering::Less);
        assert_eq!(ancestor_names("A::B::C"), ["A", "A::B"]);
    }

    #[test]
    fn creates_missing_parents_and_builds_the_tree() {
        let h = make_collection();
        h.col.create_deck("Lang::Arabic::Verbs", None).unwrap();
        h.col.create_deck("Lang::Spanish", None).unwrap();
        h.col.create_deck("maths", None).unwrap();
        let tree = h.col.deck_tree().unwrap();
        assert_eq!(tree.deck.id, 0);
        assert_eq!(tree.level, 0);
        // Default is hidden: empty, childless, and other decks exist.
        assert_eq!(
            names(&tree),
            (
                String::new(),
                0,
                vec![
                    (
                        "Lang".into(),
                        1,
                        vec![
                            ("Lang::Arabic".into(), 2, vec!["Lang::Arabic::Verbs@3".into()]),
                            ("Lang::Spanish".into(), 2, vec![]),
                        ]
                    ),
                    ("maths".into(), 1, vec![]),
                ]
            )
        );
        assert_eq!(h.col.create_deck("lang::spanish", None).unwrap_err().code, ErrorCode::Conflict);
    }

    #[test]
    fn subtree_card_totals_and_injected_counts() {
        // A harness with an injected counter (recall passes `queueCounter` to the constructor).
        let h = {
            let dir = tempfile::tempdir().unwrap();
            let clock = TestClock(std::sync::Arc::new(std::sync::atomic::AtomicI64::new(T0)));
            let counter: crate::collection::counter::QueueCounter = Box::new(|_core, decks| {
                Ok(decks
                    .iter()
                    .map(|d| (d.id, DeckCounts { new: d.name.len() as i64, learn: 1, review: 2, estimated_minutes: 3 }))
                    .collect())
            });
            let mut col = crate::collection::Collection::open_dir(dir.path(), Some(clock.clock_fn()), Some(Box::new(|_| 0)))
                .unwrap()
                .with_queue_counter(counter);
            col.open().unwrap();
            Harness { col, clock, dir }
        };
        let verbs = h.col.create_deck("Lang::Verbs", None).unwrap();
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, verbs.id, &["a", "b"], &[]);
        h.add_note(&basic, 1, &["c", "d"], &[]);
        let tree = h.col.deck_tree().unwrap();
        let lang = tree.children.iter().find(|c| c.deck.name == "Lang").unwrap();
        assert_eq!(lang.total_cards, 1);
        assert_eq!(lang.children[0].total_cards, 1);
        assert_eq!(tree.children.iter().find(|c| c.deck.id == 1).unwrap().total_cards, 1);
        assert_eq!(tree.total_cards, 2);
        assert_eq!(lang.new_count, 4);
        assert_eq!(tree.review_count, 4); // two top-level decks × 2
    }

    #[test]
    fn rename_cascades_to_children_and_creates_new_parents() {
        let h = make_collection();
        let a = h.col.create_deck("A", None).unwrap();
        let ab = h.col.create_deck("A::B", None).unwrap();
        let abc = h.col.create_deck("A::B::C", None).unwrap();
        let other = h.col.create_deck("AB", None).unwrap();
        h.col.rename_deck(a.id, "X::Y").unwrap();
        assert_eq!(h.col.get_deck(a.id).unwrap().name, "X::Y");
        assert_eq!(h.col.get_deck(ab.id).unwrap().name, "X::Y::B");
        assert_eq!(h.col.get_deck(abc.id).unwrap().name, "X::Y::B::C");
        assert_eq!(h.col.get_deck(other.id).unwrap().name, "AB");
        assert!(h.col.core().deck_id_by_name("X").unwrap().is_some());
        assert_eq!(h.col.rename_deck(a.id, "X::Y::B::Z").unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(h.col.rename_deck(ab.id, "AB").unwrap_err().code, ErrorCode::Conflict);
        // Case-only rename of itself is allowed.
        h.col.rename_deck(other.id, "ab").unwrap();
        assert_eq!(h.col.get_deck(other.id).unwrap().name, "ab");
    }

    #[test]
    fn delete_removes_subtree_cards_and_orphaned_notes_and_undo_restores_everything() {
        let mut h = make_collection();
        let a = h.col.create_deck("A", None).unwrap();
        let b = h.col.create_deck("A::B", None).unwrap();
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let n1 = h.add_note(&rev, b.id, &["front", "back"], &[]);
        // Move one card of n1 to Default: n1 must survive the delete.
        h.col.core().db().run("UPDATE cards SET did = 1 WHERE id = ?", [n1.card_ids[1]]).unwrap();
        let n2 = h.add_note(&rev, a.id, &["f2", "b2"], &[]);
        assert_eq!(h.col.delete_deck(a.id).unwrap(), DeleteDeckResult { cards: 3 });
        assert_eq!(h.count_p("SELECT count(*) FROM decks WHERE id IN (?, ?)", [a.id, b.id]), 0);
        assert_eq!(h.count_p("SELECT count(*) FROM notes WHERE id = ?", [n1.note_id]), 1);
        assert_eq!(h.count_p("SELECT count(*) FROM notes WHERE id = ?", [n2.note_id]), 0);
        assert_eq!(h.count("SELECT count(*) FROM graves"), 3 + 1 + 2);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Delete deck"));
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Delete deck");
        assert_eq!(h.col.get_deck(b.id).unwrap().name, "A::B");
        assert_eq!(h.count("SELECT count(*) FROM cards"), 4);
        assert_eq!(h.count("SELECT count(*) FROM notes"), 2);
        assert_eq!(h.count("SELECT count(*) FROM graves"), 0);
        assert_eq!(h.col.delete_deck(1).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn deck_tree_shows_default_when_it_is_the_only_deck_and_when_it_has_cards() {
        let h = make_collection();
        let tree = h.col.deck_tree().unwrap();
        assert_eq!(tree.children.iter().map(|c| c.deck.id).collect::<Vec<_>>(), [1]);
        h.col.create_deck("Other", None).unwrap();
        assert_eq!(h.col.deck_tree().unwrap().children.iter().map(|c| c.deck.name.as_str()).collect::<Vec<_>>(), ["Other"]);
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["a", "b"], &[]);
        let tree = h.col.deck_tree().unwrap();
        assert_eq!(tree.children.iter().map(|c| c.deck.name.as_str()).collect::<Vec<_>>(), ["Default", "Other"]);
        assert_eq!((tree.new_count, tree.total_cards), (1, 1));
    }

    // ── extend-limits.test.ts (the deckTree/counter half; startStudy is the integration agent's) ──
    fn node(h: &Harness, id: DeckId) -> DeckTreeNode {
        fn find(n: &DeckTreeNode, id: DeckId) -> Option<DeckTreeNode> {
            if n.deck.id == id {
                return Some(n.clone());
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&h.col.deck_tree().unwrap(), id).expect("deck in tree")
    }

    #[test]
    fn extend_by_10_on_a_preset_with_no_new_cards_a_day_serves_new_cards_and_counters_carry_today() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let mut cfg = h.col.get_preset(1).unwrap().config;
        cfg.new_per_day = 0;
        let preset = h.col.save_preset(&DeckConfigPresetInput { id: None, name: "None new".into(), config: cfg }).unwrap();
        let deck = h.col.create_deck("Zero", Some(preset.id)).unwrap();
        for i in 0..4 {
            h.add_note(&basic, deck.id, &[&format!("q{i}"), &format!("a{i}")], &[]);
        }
        h.clock.advance(3 * 86_400_000); // a collection day other than 0
        let today = h.col.today().unwrap();
        assert_eq!(today, 3);
        assert_eq!(node(&h, deck.id).new_count, 0);
        let counts = h.col.extend_today_limits(deck.id, 10, 0).unwrap();
        assert_eq!(counts.new, 4); // min(10, the 4 new cards there are)
        let t = h.col.get_deck(deck.id).unwrap().today;
        assert_eq!((t.day, t.new), (today, -10));
        assert_eq!(node(&h, deck.id).new_count, 4);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Extend today's limits"));
        assert_eq!(h.col.extend_today_limits(deck.id, 10_000, 0).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(h.col.extend_today_limits(deck.id, 0, -1).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn a_counter_already_past_a_lowered_limit_still_gets_the_full_extension() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("Lowered", None).unwrap();
        for i in 0..30 {
            h.add_note(&basic, deck.id, &[&format!("q{i}"), &format!("a{i}")], &[]);
        }
        // Five new cards studied today (the counter the study session would have written), then the limit lowered to 0.
        let today = h.col.today().unwrap();
        let t = DeckToday { day: today, new: 5, ..DeckToday::default() };
        h.col
            .core()
            .db()
            .run("UPDATE decks SET today = ? WHERE id = ?", rusqlite::params![deck_today_to_json(&t), deck.id])
            .unwrap();
        let p = h.col.get_preset(1).unwrap();
        let mut cfg = p.config.clone();
        cfg.new_per_day = 0;
        h.col.save_preset(&DeckConfigPresetInput { id: Some(p.id), name: p.name.clone(), config: cfg }).unwrap();
        assert_eq!(node(&h, deck.id).new_count, 0);
        h.col.extend_today_limits(deck.id, 10, 0).unwrap();
        assert_eq!(node(&h, deck.id).new_count, 10);
        // Undo puts the counter back.
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Extend today's limits");
        assert_eq!(h.col.get_deck(deck.id).unwrap().today.new, 5);
        assert_eq!(node(&h, deck.id).new_count, 0);
    }

    #[test]
    fn counters_from_an_earlier_day_are_reset_not_extended() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("Days", None).unwrap();
        for i in 0..30 {
            h.add_note(&basic, deck.id, &[&format!("q{i}"), &format!("a{i}")], &[]);
        }
        h.col.extend_today_limits(deck.id, 5, 0).unwrap();
        assert_eq!(node(&h, deck.id).new_count, 25);
        h.clock.advance(86_400_000);
        assert_eq!(node(&h, deck.id).new_count, 20); // yesterday's extension is gone
        h.col.extend_today_limits(deck.id, 5, 0).unwrap();
        let t = h.col.get_deck(deck.id).unwrap().today;
        assert_eq!((t.day, t.new), (h.col.today().unwrap(), -5));
    }

    #[test]
    fn ensure_deck_and_default_deck_naming() {
        let h = make_collection();
        let id = ensure_deck(h.col.core(), "New::One").unwrap();
        assert_eq!(ensure_deck(h.col.core(), "new::one").unwrap(), id);
        assert!(!ensure_default_deck(h.col.core()).unwrap());
        h.col.core().db().run("DELETE FROM decks WHERE id = 1", []).unwrap();
        h.col.create_deck("Default", None).unwrap();
        assert!(ensure_default_deck(h.col.core()).unwrap());
        assert_eq!(h.col.get_deck(1).unwrap().name, "Default (1)");
    }
}
