//! Integrity check and repair: SQLite `integrity_check`,
//! the FTS index, and collection consistency (Anki "Check Database" subset). Repairs never delete notes.

use crate::collection::decks::{ancestor_names, ensure_default_deck, DECK_SEPARATOR};
use crate::collection::internals::{meta, CollectionCore, IdTable, DEFAULT_DECK_ID, DEFAULT_PRESET_ID};
use crate::collection::presets::standard_deck_config;
use crate::collection::rows::{note_derived_columns, row_to_note, select_sql, NOTE_COLUMNS};
use crate::db::Db;
use crate::error::Result;
use crate::template::{card_ords_for_note, ord_is_valid, pad_fields, CardOrdsOptions};
use crate::text::{join_fields, split_fields, split_tags};
use crate::types::*;
use crate::undo::delete_rows_by_id;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Runs PRAGMA integrity_check; returns the problem lines ([] when 'ok').
pub fn sqlite_integrity(db: &Db) -> Vec<String> {
    match db.all("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)) {
        Ok(lines) => {
            if lines.len() == 1 && lines[0] == "ok" {
                vec![]
            } else {
                lines
            }
        }
        Err(e) => vec![format!("integrity_check failed: {}", e.message)],
    }
}

fn fts_problem(db: &Db) -> Option<String> {
    match db.run("INSERT INTO notes_fts(notes_fts, rank) VALUES ('integrity-check', 1)", []) {
        Ok(_) => None,
        Err(e) => Some(format!("Search index is inconsistent ({})", e.message)),
    }
}

fn count(n: i64, one: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {one}s")
    }
}

struct Report {
    repair: bool,
    problems: Vec<String>,
    repaired: Vec<String>,
}

impl Report {
    fn fix(&mut self, problem: String, action: impl FnOnce() -> Result<String>) -> Result<()> {
        if self.repair {
            self.repaired.push(action()?);
        } else {
            self.problems.push(problem);
        }
        Ok(())
    }
}

/// Checks the collection; with `repair` it fixes what it safely can, in one transaction: rebuilds
/// indexes, recreates the Default deck/preset and missing parent decks, deletes cards whose note is
/// gone or whose template no longer exists, moves cards in missing decks to Default, pads short
/// notes, gives notes without cards their cards, re-registers tags and repairs the new-card
/// position counter. `ok` is true when no problem remains.
pub fn check_integrity(core: &CollectionCore, repair: bool) -> Result<IntegrityReport> {
    let db = core.db();
    let mut rep = Report { repair, problems: vec![], repaired: vec![] };

    // 1. Page-level integrity.
    let sqlite_lines = sqlite_integrity(db);
    if !sqlite_lines.is_empty() {
        if repair {
            let _ = db
                .execute_batch("REINDEX")
                .and_then(|_| db.run("INSERT INTO notes_fts(notes_fts) VALUES ('rebuild')", []).map(|_| ()));
            let after = sqlite_integrity(db);
            if after.is_empty() {
                rep.repaired.push(format!("Rebuilt indexes ({} found)", count(sqlite_lines.len() as i64, "problem")));
            } else {
                rep.problems.extend(after.iter().take(20).map(|l| format!("Database: {l}")));
            }
        } else {
            rep.problems.extend(sqlite_lines.iter().take(20).map(|l| format!("Database: {l}")));
        }
        if !rep.problems.is_empty() && repair {
            return Ok(IntegrityReport { ok: false, problems: rep.problems, repaired: rep.repaired });
        }
    }

    if let Some(fts) = fts_problem(db) {
        if repair {
            db.run("INSERT INTO notes_fts(notes_fts) VALUES ('rebuild')", [])?;
            rep.repaired.push("Rebuilt the search index".to_string());
        } else {
            rep.problems.push(fts);
        }
    }

    let outcome = if repair { db.transaction(|| body(core, &mut rep)) } else { body(core, &mut rep) };
    match outcome {
        Ok(()) => {
            if repair && !rep.repaired.is_empty() {
                core.invalidate_caches();
                core.mark_changed()?;
            }
        }
        Err(e) => rep.problems.push(format!("Consistency check failed: {}", e.message)),
    }
    Ok(IntegrityReport { ok: rep.problems.is_empty(), problems: rep.problems, repaired: rep.repaired })
}

fn body(core: &CollectionCore, rep: &mut Report) -> Result<()> {
    let db = core.db();
    // 2. Default preset and deck.
    if db.scalar::<i64, _>("SELECT 1 FROM deck_configs WHERE id = ?", [DEFAULT_PRESET_ID])?.is_none() {
        rep.fix("The default preset is missing".into(), || {
            db.run(
                "INSERT INTO deck_configs (id, name, config, mtime, usn) VALUES (?, ?, ?, ?, -1)",
                rusqlite::params![
                    DEFAULT_PRESET_ID,
                    "Standard",
                    serde_json::to_string(&standard_deck_config())?,
                    core.now_seconds()
                ],
            )?;
            Ok("Recreated the default preset".into())
        })?;
    }
    if db.scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [DEFAULT_DECK_ID])?.is_none() {
        rep.fix("The Default deck is missing".into(), || {
            ensure_default_deck(core)?;
            Ok("Recreated the Default deck".into())
        })?;
    }
    let bad_preset = db.count("SELECT count(*) FROM decks WHERE config_id NOT IN (SELECT id FROM deck_configs)", [])?;
    if bad_preset > 0 {
        rep.fix(format!("{} use a missing preset", count(bad_preset, "deck")), || {
            db.run(
                "UPDATE decks SET config_id = ?, usn = -1 WHERE config_id NOT IN (SELECT id FROM deck_configs)",
                [DEFAULT_PRESET_ID],
            )?;
            Ok(format!("Reset {} to the default preset", count(bad_preset, "deck")))
        })?;
    }
    // Missing parent decks ('A::B' without 'A').
    let names: HashSet<String> =
        db.all("SELECT name FROM decks", [], |r| r.get::<_, String>(0))?.into_iter().map(|n| n.to_lowercase()).collect();
    let mut missing_parents: BTreeSet<String> = BTreeSet::new();
    for name in db.all(&format!("SELECT name FROM decks WHERE name LIKE '%{DECK_SEPARATOR}%'"), [], |r| r.get::<_, String>(0))? {
        for a in ancestor_names(&name) {
            if !names.contains(&a.to_lowercase()) {
                missing_parents.insert(a);
            }
        }
    }
    if !missing_parents.is_empty() {
        let n = missing_parents.len() as i64;
        rep.fix(format!("{} missing", count(n, "parent deck")), || {
            let mut sorted: Vec<String> = missing_parents.iter().cloned().collect();
            sorted.sort_by_key(|a| a.len());
            for a in sorted {
                if core.deck_id_by_name(&a)?.is_some() {
                    continue;
                }
                db.run(
                    "INSERT INTO decks (id, name, config_id, mtime, usn, today, config) VALUES (?, ?, ?, ?, -1, '{}', '{}')",
                    rusqlite::params![core.new_id(IdTable::Decks)?, a, DEFAULT_PRESET_ID, core.now_seconds()],
                )?;
            }
            Ok(format!("Recreated {}", count(n, "parent deck")))
        })?;
    }

    // 3. Cards whose note is missing.
    let orphan_cards: Vec<CardId> = db.all("SELECT id FROM cards WHERE nid NOT IN (SELECT id FROM notes)", [], |r| r.get(0))?;
    if !orphan_cards.is_empty() {
        let n = orphan_cards.len() as i64;
        rep.fix(format!("{} without a note", count(n, "card")), || {
            delete_rows_by_id(db, "cards", &orphan_cards)?;
            for id in &orphan_cards {
                db.run("INSERT INTO graves (oid, type, usn) VALUES (?, ?, -1)", rusqlite::params![id, GraveType::Card as i64])?;
            }
            Ok(format!("Deleted {} without a note", count(n, "card")))
        })?;
    }

    // 4. Cards in missing decks.
    let lost = db.count("SELECT count(*) FROM cards WHERE did NOT IN (SELECT id FROM decks)", [])?;
    if lost > 0 {
        rep.fix(format!("{} in a missing deck", count(lost, "card")), || {
            ensure_default_deck(core)?;
            db.run(
                "UPDATE cards SET did = ?, mtime = ?, usn = -1 WHERE did NOT IN (SELECT id FROM decks)",
                rusqlite::params![DEFAULT_DECK_ID, core.now_seconds()],
            )?;
            Ok(format!("Moved {} to the Default deck", count(lost, "card")))
        })?;
    }
    let lost_original = db.count("SELECT count(*) FROM cards WHERE odid != 0 AND odid NOT IN (SELECT id FROM decks)", [])?;
    if lost_original > 0 {
        rep.fix(format!("{} with a missing original deck", count(lost_original, "card")), || {
            db.run("UPDATE cards SET odid = 0, odue = 0, usn = -1 WHERE odid != 0 AND odid NOT IN (SELECT id FROM decks)", [])?;
            Ok(format!("Cleared the missing original deck of {}", count(lost_original, "card")))
        })?;
    }

    // 5. New-card position counter must exceed every new card's position (before generating cards).
    let max_new: i64 = db.scalar::<Option<i64>, _>("SELECT max(due) FROM cards WHERE type = 0", [])?.flatten().unwrap_or(0);
    let next_pos = core.get_meta(meta::NEXT_POS)?.unwrap_or_else(|| "1".into()).parse::<f64>().ok().filter(|v| v.is_finite());
    if !next_pos.is_some_and(|p| p > max_new as f64) {
        rep.fix("The new-card position counter is behind".into(), || {
            core.set_meta(meta::NEXT_POS, Some(&(max_new + 1).to_string()))?;
            Ok("Repaired the new-card position counter".into())
        })?;
    }

    // 6. Notes: missing note type, field count, invalid card ords, notes without cards.
    let notetypes: BTreeMap<NotetypeId, Notetype> = core.all_notetypes()?.into_iter().map(|nt| (nt.id, nt)).collect();
    let no_notetype = db.count("SELECT count(*) FROM notes WHERE ntid NOT IN (SELECT id FROM notetypes)", [])?;
    if no_notetype > 0 {
        rep.problems
            .push(format!("{} use a missing note type (not repaired; export them before deleting)", count(no_notetype, "note")));
    }

    let mut padded = 0;
    let mut merged = 0;
    let mut bad_ord_cards: Vec<CardId> = Vec::new();
    for nt in notetypes.values() {
        let rows: Vec<(NoteId, String)> =
            db.all("SELECT id, flds FROM notes WHERE ntid = ?", [nt.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        for (id, flds) in rows {
            let fields = split_fields(&flds);
            if fields.len() != nt.fields.len() {
                let fixed: Vec<String> = if fields.len() < nt.fields.len() {
                    padded += 1;
                    pad_fields(nt, &fields)
                } else {
                    merged += 1;
                    let keep = nt.fields.len() - 1;
                    let mut v: Vec<String> = fields[..keep].to_vec();
                    v.push(fields[keep..].join(" "));
                    v
                };
                if rep.repair {
                    let d = note_derived_columns(&fixed, nt.sort_field);
                    db.run(
                        "UPDATE notes SET flds = ?, sfld = ?, csum = ?, plain = ?, mtime = ?, usn = -1 WHERE id = ?",
                        rusqlite::params![join_fields(&fixed), d.sfld, d.csum, d.plain, core.now_seconds(), id],
                    )?;
                }
            }
        }
        let cards: Vec<(CardId, i64)> =
            db.all("SELECT c.id, c.ord FROM cards c JOIN notes n ON n.id = c.nid WHERE n.ntid = ?", [nt.id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        for (id, ord) in cards {
            if !ord_is_valid(nt, ord) {
                bad_ord_cards.push(id);
            }
        }
    }
    if padded > 0 {
        rep.fix(format!("{} with too few fields", count(padded, "note")), || {
            Ok(format!("Added empty fields to {}", count(padded, "note")))
        })?;
    }
    if merged > 0 {
        rep.fix(format!("{} with too many fields", count(merged, "note")), || {
            Ok(format!("Merged extra fields into the last field of {}", count(merged, "note")))
        })?;
    }
    if !bad_ord_cards.is_empty() {
        let n = bad_ord_cards.len() as i64;
        rep.fix(format!("{} whose card template no longer exists", count(n, "card")), || {
            delete_rows_by_id(db, "cards", &bad_ord_cards)?;
            for id in &bad_ord_cards {
                db.run("INSERT INTO graves (oid, type, usn) VALUES (?, ?, -1)", rusqlite::params![id, GraveType::Card as i64])?;
            }
            Ok(format!("Deleted {} whose card template no longer exists", count(n, "card")))
        })?;
    }

    let cardless = db.all(
        &format!(
            "{} WHERE id NOT IN (SELECT nid FROM cards) AND ntid IN (SELECT id FROM notetypes)",
            select_sql("notes", &NOTE_COLUMNS)
        ),
        [],
        row_to_note,
    )?;
    if !cardless.is_empty() {
        let n = cardless.len() as i64;
        rep.fix(format!("{} without cards", count(n, "note")), || {
            let mut added = 0;
            for note in &cardless {
                let Some(nt) = notetypes.get(&note.ntid) else { continue };
                let ords = card_ords_for_note(nt, &pad_fields(nt, &note.fields), &CardOrdsOptions::default());
                let ords = if ords.is_empty() { vec![0] } else { ords };
                added += core.insert_new_cards(note, nt, &ords, DEFAULT_DECK_ID)?.len() as i64;
            }
            Ok(format!("Generated {} for {} that had none", count(added, "card"), count(n, "note")))
        })?;
    }

    // 7. Tag registry.
    let registered: HashSet<String> =
        db.all("SELECT tag FROM tags", [], |r| r.get::<_, String>(0))?.into_iter().map(|t| t.to_lowercase()).collect();
    let mut unregistered: BTreeMap<String, String> = BTreeMap::new();
    for tags in db.all("SELECT tags FROM notes WHERE tags != ''", [], |r| r.get::<_, String>(0))? {
        for t in split_tags(&tags) {
            if !registered.contains(&t.to_lowercase()) {
                unregistered.insert(t.to_lowercase(), t);
            }
        }
    }
    if !unregistered.is_empty() {
        let n = unregistered.len() as i64;
        rep.fix(format!("{} missing from the tag list", count(n, "tag")), || {
            core.register_tags(&unregistered.values().cloned().collect::<Vec<_>>())?;
            Ok(format!("Registered {}", count(n, "tag")))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;

    #[test]
    fn repairs_orphans_lost_decks_invalid_ords_cardless_notes_tags_and_positions() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("Doomed", None).unwrap();
        let a = h.add_note(&basic, deck.id, &["a", "b"], &["t"]);
        let b = h.add_note(&basic, 1, &["c", "d"], &[]);
        let c = h.add_note(&basic, 1, &["e", "f"], &[]);
        let db = h.col.core().db();
        // Orphan card (note gone), card in a deleted deck, invalid ord, note without cards, unregistered tag, short note.
        db.run("INSERT INTO cards (id, nid, did, ord, mtime) VALUES (777, 123456, 1, 0, 0)", []).unwrap();
        db.run("DELETE FROM decks WHERE id = ?", [deck.id]).unwrap();
        db.run("UPDATE cards SET ord = 5 WHERE id = ?", [b.card_ids[0]]).unwrap();
        db.run("DELETE FROM cards WHERE nid = ?", [c.note_id]).unwrap();
        db.run("UPDATE notes SET tags = ' t ghost ' WHERE id = ?", [a.note_id]).unwrap();
        db.run("UPDATE notes SET flds = 'only' WHERE id = ?", [c.note_id]).unwrap();
        db.run("UPDATE col_meta SET value = '1' WHERE key = 'next_pos'", []).unwrap();
        h.col.core().invalidate_caches();

        let report = h.col.check_integrity().unwrap();
        assert!(report.ok);
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(
            report.repaired,
            [
                "Deleted 1 card without a note",
                "Moved 1 card to the Default deck",
                "Repaired the new-card position counter",
                "Added empty fields to 1 note",
                "Deleted 1 card whose card template no longer exists",
                "Generated 2 cards for 2 notes that had none",
                "Registered 1 tag",
            ]
        );
        assert_eq!(h.count("SELECT count(*) FROM cards WHERE id = 777"), 0);
        assert_eq!(h.col.get_card(a.card_ids[0]).unwrap().did, 1);
        assert_eq!(h.col.get_cards_of_note(b.note_id).unwrap().iter().map(|x| x.ord).collect::<Vec<_>>(), [0]);
        assert_eq!(h.col.get_cards_of_note(c.note_id).unwrap().iter().map(|x| x.ord).collect::<Vec<_>>(), [0]);
        assert_eq!(h.col.get_note(c.note_id).unwrap().fields, strs(&["only", ""]));
        assert_eq!(h.col.all_tags().unwrap().iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(), ["ghost", "t"]);
        assert!(h.meta("next_pos").unwrap().parse::<i64>().unwrap() > 3);
        // Second run is clean.
        assert_eq!(h.col.check_integrity().unwrap(), IntegrityReport { ok: true, problems: vec![], repaired: vec![] });
    }

    #[test]
    fn recreates_missing_default_deck_and_parents_reports_notes_with_a_missing_note_type() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.col.create_deck("P::Q", None).unwrap();
        let n = h.add_note(&basic, 1, &["x", ""], &[]);
        let db = h.col.core().db();
        db.run("DELETE FROM decks WHERE id = 1", []).unwrap();
        db.run("DELETE FROM decks WHERE name = 'P'", []).unwrap();
        db.run("UPDATE notes SET ntid = 42 WHERE id = ?", [n.note_id]).unwrap();
        let report = h.col.check_integrity().unwrap();
        assert!(report.repaired.contains(&"Recreated the Default deck".to_string()));
        assert!(report.repaired.contains(&"Recreated 1 parent deck".to_string()));
        assert_eq!(report.problems, ["1 note use a missing note type (not repaired; export them before deleting)"]);
        assert!(!report.ok);
        assert_eq!(h.col.get_deck(1).unwrap().name, "Default");
        assert!(h.col.core().deck_id_by_name("P").unwrap().is_some());
    }

    #[test]
    fn rebuilds_a_desynchronised_search_index() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["searchable", ""], &[]);
        h.col.core().db().run("INSERT INTO notes_fts(notes_fts) VALUES ('delete-all')", []).unwrap();
        let report = h.col.check_integrity().unwrap();
        assert!(report.repaired.contains(&"Rebuilt the search index".to_string()));
        assert_eq!(h.count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'searchable'"), 1);
    }

    #[test]
    fn check_only_mode_reports_without_repairing() {
        let h = make_collection();
        h.col.core().db().run("INSERT INTO cards (id, nid, did, ord, mtime) VALUES (777, 123456, 1, 0, 0)", []).unwrap();
        h.col.core().db().run("DELETE FROM deck_configs WHERE id = 1", []).unwrap();
        let report = check_integrity(h.col.core(), false).unwrap();
        assert!(!report.ok);
        assert!(report.repaired.is_empty());
        assert!(report.problems.contains(&"The default preset is missing".to_string()));
        assert!(report.problems.contains(&"1 card without a note".to_string()));
        assert!(report.problems.contains(&"1 deck use a missing preset".to_string()));
        assert_eq!(h.count("SELECT count(*) FROM cards WHERE id = 777"), 1);
    }
}
