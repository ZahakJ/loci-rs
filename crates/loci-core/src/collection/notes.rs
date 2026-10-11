//! Notes, cards-of-notes and tags (recall `src/core/collection/notes.ts`): add/update/delete with
//! card generation, duplicate check, tag registry maintenance, tag rename. All writes are
//! transactions; user-level operations push undo entries. Also home to `install_sample_deck`
//! (recall `export.ts:170`), which creates the bundled deck's notes the same way.

use crate::anki::{sample_deck, SampleNoteKind};
use crate::collection::builtin_notetypes::builtin_notetype;
use crate::collection::decks::ensure_deck;
use crate::collection::internals::{CollectionCore, IdTable, DEFAULT_DECK_ID};
use crate::collection::notetypes::save_notetype;
use crate::collection::rows::{
    self, note_derived_columns, note_params, row_to_card, row_to_note, select_sql, CARD_COLUMNS, NOTE_COLUMNS,
};
use crate::db::unicase_compare;
use crate::error::{LociError, Result};
use crate::template::{card_ords_for_note, pad_fields, CardOrdsOptions};
use crate::text::{
    field_checksum, join_fields, join_tags, new_guid, plain_text, split_fields, split_tags, strip_html_preserving_media,
};
use crate::types::*;
use crate::undo::{self, GraveRow, Table, UndoSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteResult {
    pub note_id: NoteId,
    pub card_ids: Vec<CardId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNoteResult {
    pub cards_added: i64,
    pub cards_removed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

pub fn get_note(core: &CollectionCore, id: NoteId) -> Result<Note> {
    core.db()
        .get(&format!("{} WHERE id = ?", select_sql("notes", &NOTE_COLUMNS)), [id], row_to_note)?
        .ok_or_else(|| LociError::not_found(format!("Note {id} not found")))
}

pub fn get_card(core: &CollectionCore, id: CardId) -> Result<Card> {
    core.db()
        .get(&format!("{} WHERE id = ?", select_sql("cards", &CARD_COLUMNS)), [id], row_to_card)?
        .ok_or_else(|| LociError::not_found(format!("Card {id} not found")))
}

pub fn get_cards_of_note(core: &CollectionCore, nid: NoteId) -> Result<Vec<Card>> {
    get_note(core, nid)?;
    core.db().all(&format!("{} WHERE nid = ? ORDER BY ord", select_sql("cards", &CARD_COLUMNS)), [nid], row_to_card)
}

fn check_field_count(nt: &Notetype, fields: &[String]) -> Result<()> {
    if fields.len() > nt.fields.len() {
        return Err(LociError::invalid(format!(
            "Note type \"{}\" has {} fields, got {}",
            nt.name,
            nt.fields.len(),
            fields.len()
        )));
    }
    Ok(())
}

fn insert_grave(core: &CollectionCore, g: &GraveRow) -> Result<()> {
    core.db().run("INSERT INTO graves (oid, type, usn) VALUES (?, ?, -1)", rusqlite::params![g.oid, g.gtype])?;
    Ok(())
}

/// Adds a note and its cards. Refuses an empty first field (as Anki does). Undoable.
pub fn add_note(
    core: &CollectionCore,
    ntid: NotetypeId,
    deck_id: DeckId,
    raw_fields: &[String],
    tags: &[String],
) -> Result<AddNoteResult> {
    core.db().transaction(|| {
        let nt = core.notetype(ntid)?;
        core.deck(deck_id)?;
        check_field_count(&nt, raw_fields)?;
        let fields = pad_fields(&nt, raw_fields);
        if strip_html_preserving_media(fields.first().map(String::as_str).unwrap_or("")).trim().is_empty() {
            return Err(LociError::invalid("The first field is empty"));
        }
        let canonical = core.canonical_tags(tags)?;
        let note = Note {
            id: core.new_id(IdTable::Notes)?,
            guid: new_guid(),
            ntid,
            mtime: core.now_seconds(),
            usn: -1,
            tags: canonical.clone(),
            fields,
        };
        core.db()
            .run(&rows::insert_sql("notes", &NOTE_COLUMNS), rusqlite::params_from_iter(note_params(&note, nt.sort_field)))?;
        core.register_tags(&canonical)?;
        let ords = card_ords_for_note(&nt, &note.fields, &CardOrdsOptions::default());
        let ords = if ords.is_empty() { vec![0] } else { ords };
        let card_ids = core.insert_new_cards(&note, &nt, &ords, deck_id)?;
        undo::record(
            core.db(),
            core.now(),
            "Add note",
            &UndoSnapshot { inserted_notes: vec![note.id], inserted_cards: card_ids.clone(), ..Default::default() },
        )?;
        core.mark_changed()?;
        Ok(AddNoteResult { note_id: note.id, card_ids })
    })
}

/// Updates fields and tags. Cards the templates now generate are added (in the deck of the note's
/// first card). Cards no longer generated are removed only when they are still new and never
/// reviewed, and never the last card; studied cards are kept (Anki's "Empty Cards" semantics are
/// destructive, Loci prefers not to lose history). Undoable.
pub fn update_note(core: &CollectionCore, id: NoteId, raw_fields: &[String], tags: &[String]) -> Result<UpdateNoteResult> {
    core.db().transaction(|| {
        let before_rows = undo::snapshot_rows(core.db(), Table::Notes, &[id])?;
        let Some(before_row) = before_rows.first().cloned() else {
            return Err(LociError::not_found(format!("Note {id} not found")));
        };
        let before = get_note(core, id)?;
        let nt = core.notetype(before.ntid)?;
        check_field_count(&nt, raw_fields)?;
        let fields = pad_fields(&nt, raw_fields);
        let canonical = core.canonical_tags(tags)?;
        let stored_flds = before_row.get("flds").and_then(|v| v.as_str()).unwrap_or("");
        let stored_tags = before_row.get("tags").and_then(|v| v.as_str()).unwrap_or("");
        let fields_same = join_fields(&fields) == stored_flds;
        let tags_same = join_tags(&canonical) == stored_tags;
        if fields_same && tags_same {
            return Ok(UpdateNoteResult { cards_added: 0, cards_removed: 0 });
        }

        let note = Note { fields, tags: canonical.clone(), mtime: core.now_seconds(), usn: -1, ..before };
        let d = note_derived_columns(&note.fields, nt.sort_field);
        core.db().run(
            "UPDATE notes SET mtime = ?, usn = -1, tags = ?, flds = ?, sfld = ?, csum = ?, plain = ? WHERE id = ?",
            rusqlite::params![note.mtime, join_tags(&note.tags), join_fields(&note.fields), d.sfld, d.csum, d.plain, id],
        )?;
        core.register_tags(&canonical)?;

        let mut inserted_cards: Vec<CardId> = Vec::new();
        let mut removed_ids: Vec<CardId> = Vec::new();
        let mut removed_rows = Vec::new();
        if !fields_same {
            let cards = core.db().all(
                &format!("{} WHERE nid = ? ORDER BY ord", select_sql("cards", &CARD_COLUMNS)),
                [id],
                row_to_card,
            )?;
            let have: HashSet<i64> = cards.iter().map(|c| c.ord).collect();
            let want: HashSet<i64> = card_ords_for_note(&nt, &note.fields, &CardOrdsOptions::default()).into_iter().collect();
            let mut to_add: Vec<i64> = want.iter().copied().filter(|o| !have.contains(o)).collect();
            to_add.sort_unstable();
            let deck = cards.first().map(|c| c.did).unwrap_or(DEFAULT_DECK_ID);
            let target = if core.db().scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [deck])?.is_some() {
                deck
            } else {
                DEFAULT_DECK_ID
            };
            inserted_cards = core.insert_new_cards(&note, &nt, &to_add, target)?;
            let removable: Vec<&Card> =
                cards.iter().filter(|c| !want.contains(&c.ord) && c.ctype == CardType::New && c.reps == 0).collect();
            // Never remove the last remaining card.
            let remaining_after = cards.len() as i64 + inserted_cards.len() as i64 - removable.len() as i64;
            let removed: Vec<&Card> = if remaining_after > 0 { removable } else { removable.into_iter().skip(1).collect() };
            removed_ids = removed.iter().map(|c| c.id).collect();
            removed_rows = undo::snapshot_rows(core.db(), Table::Cards, &removed_ids)?;
            undo::delete_rows_by_id(core.db(), "cards", &removed_ids)?;
            for cid in &removed_ids {
                insert_grave(core, &GraveRow { oid: *cid, gtype: GraveType::Card as i64 })?;
            }
        }
        undo::record(
            core.db(),
            core.now(),
            "Edit note",
            &UndoSnapshot {
                notes: vec![before_row],
                cards: removed_rows,
                inserted_cards: inserted_cards.clone(),
                graves: removed_ids.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Card as i64 }).collect(),
                ..Default::default()
            },
        )?;
        core.mark_changed()?;
        Ok(UpdateNoteResult { cards_added: inserted_cards.len() as i64, cards_removed: removed_ids.len() as i64 })
    })
}

/// Deletes notes and their cards (review history is kept, as in Anki). Undoable.
pub fn delete_notes(core: &CollectionCore, ids: &[NoteId]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    core.db().transaction(|| {
        let notes = undo::snapshot_rows(core.db(), Table::Notes, ids)?;
        if notes.is_empty() {
            return Ok(());
        }
        let note_ids: Vec<NoteId> = notes.iter().filter_map(|n| n.get("id").and_then(|v| v.as_i64())).collect();
        let cards = undo::snapshot_cards_of_notes(core.db(), &note_ids)?;
        let card_ids: Vec<CardId> = cards.iter().filter_map(|c| c.get("id").and_then(|v| v.as_i64())).collect();
        undo::delete_rows_by_id(core.db(), "cards", &card_ids)?;
        undo::delete_rows_by_id(core.db(), "notes", &note_ids)?;
        let mut graves: Vec<GraveRow> = card_ids.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Card as i64 }).collect();
        graves.extend(note_ids.iter().map(|&oid| GraveRow { oid, gtype: GraveType::Note as i64 }));
        for g in &graves {
            insert_grave(core, g)?;
        }
        let label = if note_ids.len() == 1 { "Delete note".to_string() } else { format!("Delete {} notes", note_ids.len()) };
        undo::record(core.db(), core.now(), &label, &UndoSnapshot { notes, cards, graves, ..Default::default() })?;
        core.mark_changed()
    })
}

/// Notes of `ntid` whose first field equals `first_field` after Anki's stripping (csum match,
/// then exact stripped-text match; research/anki-format.md §10.2).
pub fn find_duplicates(core: &CollectionCore, ntid: NotetypeId, first_field: &str) -> Result<Vec<NoteId>> {
    let stripped = strip_html_preserving_media(first_field).trim().to_string();
    if stripped.is_empty() {
        return Ok(vec![]);
    }
    let csum = field_checksum(&strip_html_preserving_media(first_field)) as i64;
    let rows: Vec<(NoteId, String)> =
        core.db().all("SELECT id, flds FROM notes WHERE ntid = ? AND csum = ?", rusqlite::params![ntid, csum], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    Ok(rows
        .into_iter()
        .filter(|(_, flds)| {
            strip_html_preserving_media(split_fields(flds).first().map(String::as_str).unwrap_or("")).trim() == stripped
        })
        .map(|(id, _)| id)
        .collect())
}

/// Every registered tag with the number of notes carrying it (exact tag, case-insensitive).
pub fn all_tags(core: &CollectionCore) -> Result<Vec<TagCount>> {
    let mut counts: HashMap<String, i64> = HashMap::new();
    let note_tags: Vec<String> = core.db().all("SELECT tags FROM notes WHERE tags != ''", [], |r| r.get(0))?;
    for t in &note_tags {
        let uniq: HashSet<String> = split_tags(t).into_iter().map(|x| x.to_lowercase()).collect();
        for u in uniq {
            *counts.entry(u).or_insert(0) += 1;
        }
    }
    let mut registered: Vec<String> = core.db().all("SELECT tag FROM tags", [], |r| r.get(0))?;
    let mut seen: HashSet<String> = registered.iter().map(|t| t.to_lowercase()).collect();
    // Tags present on notes but missing from the registry still show up.
    for t in &note_tags {
        for tag in split_tags(t) {
            if seen.insert(tag.to_lowercase()) {
                registered.push(tag);
            }
        }
    }
    registered.sort_by(|a, b| unicase_compare(a, b));
    Ok(registered.into_iter().map(|tag| TagCount { count: counts.get(&tag.to_lowercase()).copied().unwrap_or(0), tag }).collect())
}

fn tag_matches(tag: &str, from: &str) -> bool {
    let t = tag.to_lowercase();
    let f = from.to_lowercase();
    t == f || t.starts_with(&format!("{f}::"))
}

/// Renames a tag and its children (`from::x` → `to::x`) on every note, case-insensitively.
/// Returns the number of notes changed. Undoable.
pub fn rename_tag(core: &CollectionCore, raw_from: &str, raw_to: &str) -> Result<i64> {
    let from_parts = core.canonical_tags(&[raw_from.to_string()])?;
    let to_parts = core.canonical_tags(&[raw_to.to_string()])?;
    let Some(from) = from_parts.first().cloned() else { return Err(LociError::invalid("Tag to rename is empty")) };
    if to_parts.len() != 1 {
        return Err(LociError::invalid("New tag name must be a single non-empty tag"));
    }
    let to = to_parts[0].clone();
    core.db().transaction(|| {
        let escaped: String =
            from.chars().flat_map(|c| if matches!(c, '\\' | '%' | '_') { vec!['\\', c] } else { vec![c] }).collect();
        let like = format!("% {escaped}%");
        let candidate_ids: Vec<NoteId> =
            core.db().all("SELECT id FROM notes WHERE tags LIKE ? ESCAPE '\\'", [like], |r| r.get(0))?;
        let candidates = undo::snapshot_rows(core.db(), Table::Notes, &candidate_ids)?;
        let mut changed = Vec::new();
        let mtime = core.now_seconds();
        let mut new_tags: BTreeSet<String> = BTreeSet::new();
        for row in candidates {
            let tags = split_tags(row.get("tags").and_then(|v| v.as_str()).unwrap_or(""));
            if !tags.iter().any(|t| tag_matches(t, &from)) {
                continue;
            }
            let renamed: Vec<String> = tags
                .iter()
                .map(|t| if tag_matches(t, &from) { format!("{to}{}", &t[from.len()..]) } else { t.clone() })
                .collect();
            let canonical = dedupe_sorted(&renamed);
            new_tags.extend(canonical.iter().cloned());
            let id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            core.db().run(
                "UPDATE notes SET tags = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![join_tags(&canonical), mtime, id],
            )?;
            changed.push(row);
        }
        // Registry: drop the old names, add the new ones.
        let registered: Vec<String> = core.db().all("SELECT tag FROM tags", [], |r| r.get(0))?;
        for tag in registered {
            if tag_matches(&tag, &from) {
                core.db().run("DELETE FROM tags WHERE tag = ?", [&tag])?;
                new_tags.insert(format!("{to}{}", &tag[from.len()..]));
            }
        }
        core.register_tags(&new_tags.into_iter().collect::<Vec<_>>())?;
        let n = changed.len() as i64;
        if n > 0 {
            undo::record(core.db(), core.now(), "Rename tag", &UndoSnapshot { notes: changed, ..Default::default() })?;
            core.mark_changed()?;
        }
        Ok(n)
    })
}

fn dedupe_sorted(tags: &[String]) -> Vec<String> {
    let mut m: BTreeMap<String, String> = BTreeMap::new();
    for t in tags {
        m.entry(t.to_lowercase()).or_insert_with(|| t.clone());
    }
    let mut v: Vec<String> = m.into_values().collect();
    v.sort_by(|a, b| unicase_compare(a, b));
    v
}

/// Maintenance: recomputes `notes.plain` (the FTS text) for every note whose stored value differs
/// from the current `plain_text` rules, `chunk` notes per transaction so a large collection never
/// holds one huge write. No undo entry and no mtime change: the column is derived data.
/// Returns the number of rows rewritten.
pub fn rebuild_plain(core: &CollectionCore, chunk: i64) -> Result<i64> {
    let mut last = i64::MIN;
    let mut changed = 0;
    loop {
        let rows: Vec<(NoteId, String, String)> = core.db().all(
            "SELECT id, flds, plain FROM notes WHERE id > ? ORDER BY id LIMIT ?",
            rusqlite::params![last, chunk.max(1)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let Some(last_row) = rows.last() else { break };
        last = last_row.0;
        core.db().transaction(|| {
            for (id, flds, stored) in &rows {
                let plain =
                    split_fields(flds).iter().map(|f| plain_text(f)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
                if plain != *stored {
                    core.db().run("UPDATE notes SET plain = ? WHERE id = ?", rusqlite::params![plain, id])?;
                    changed += 1;
                }
            }
            Ok(())
        })?;
    }
    Ok(changed)
}

// ───────────────────────── sample deck (export.ts:131-203) ─────────────────────────

fn builtin_kind_of(kind: SampleNoteKind) -> BuiltinKind {
    match kind {
        SampleNoteKind::Basic => BuiltinKind::Basic,
        SampleNoteKind::BasicReversed => BuiltinKind::BasicReversed,
        SampleNoteKind::BasicTyped => BuiltinKind::BasicTyped,
        SampleNoteKind::Cloze => BuiltinKind::Cloze,
    }
}

/// A note type shaped like the built-in `kind`: the stock one by name and shape, else a fresh copy
/// named `<name> 2`, `<name> 3`, … (export.ts `builtinLike`).
fn builtin_like(core: &CollectionCore, kind: BuiltinKind, by_kind: &mut HashMap<BuiltinKind, Notetype>) -> Result<Notetype> {
    let draft = builtin_notetype(kind);
    let all = core.all_notetypes()?;
    let same = all.iter().find(|nt| {
        nt.name.to_lowercase() == draft.name.to_lowercase()
            && nt.kind == draft.kind
            && nt.fields.len() == draft.fields.len()
            && nt.fields.iter().zip(draft.fields.iter()).all(|(a, b)| a.name == b.name)
            && nt.templates.len() == draft.templates.len()
    });
    let nt = match same {
        Some(nt) => nt.clone(),
        None => {
            let mut name = draft.name.clone();
            let mut i = 2;
            while all.iter().any(|x| x.name.to_lowercase() == name.to_lowercase()) {
                name = format!("{} {i}", draft.name);
                i += 1;
            }
            save_notetype(core, &NotetypeInput { name, ..draft })?
        }
    };
    by_kind.insert(kind, nt.clone());
    Ok(nt)
}

/// Installs the bundled sample deck (idempotent by sort field): creates the deck, adds the notes
/// whose first field is not there yet, one undo entry. Returns the deck id.
pub fn install_sample_deck(core: &CollectionCore) -> Result<DeckId> {
    core.db().transaction(|| {
        let sample = sample_deck();
        let before: HashSet<DeckId> = core.all_decks()?.into_iter().map(|d| d.id).collect();
        let deck_id = ensure_deck(core, &sample.name)?;
        let inserted_decks: Vec<DeckId> = core.all_decks()?.into_iter().map(|d| d.id).filter(|id| !before.contains(id)).collect();
        if core.deck(deck_id)?.description.is_empty() {
            core.db().run("UPDATE decks SET description = ? WHERE id = ?", rusqlite::params![sample.description, deck_id])?;
        }
        let mut by_kind: HashMap<BuiltinKind, Notetype> = HashMap::new();
        for nt in core.all_notetypes()? {
            if let Some(k) = nt.config.builtin {
                by_kind.entry(k).or_insert(nt);
            }
        }
        let existing: HashSet<String> = core
            .db()
            .all("SELECT n.sfld FROM notes n WHERE n.id IN (SELECT nid FROM cards WHERE did = ?)", [deck_id], |r| r.get(0))?
            .into_iter()
            .collect();
        let mut inserted_notes = Vec::new();
        let mut inserted_cards = Vec::new();
        for s in &sample.notes {
            let kind = builtin_kind_of(s.kind);
            let nt = match by_kind.get(&kind) {
                Some(nt) => nt.clone(),
                None => builtin_like(core, kind, &mut by_kind)?,
            };
            let mut fields: Vec<String> = s.fields.to_vec();
            fields.resize(nt.fields.len().max(fields.len()), String::new());
            fields.truncate(nt.fields.len());
            let tags = core.canonical_tags(&s.tags)?;
            let note = Note {
                id: core.new_id(IdTable::Notes)?,
                guid: new_guid(),
                ntid: nt.id,
                mtime: core.now_seconds(),
                usn: -1,
                tags: tags.clone(),
                fields,
            };
            let d = note_derived_columns(&note.fields, nt.sort_field);
            if existing.contains(&d.sfld) {
                continue;
            }
            core.db()
                .run(&rows::insert_sql("notes", &NOTE_COLUMNS), rusqlite::params_from_iter(note_params(&note, nt.sort_field)))?;
            core.register_tags(&tags)?;
            let ords = card_ords_for_note(&nt, &note.fields, &CardOrdsOptions::default());
            let ords = if ords.is_empty() { vec![0] } else { ords };
            inserted_notes.push(note.id);
            inserted_cards.extend(core.insert_new_cards(&note, &nt, &ords, deck_id)?);
        }
        if !inserted_notes.is_empty() || !inserted_decks.is_empty() {
            undo::record(
                core.db(),
                core.now(),
                "Install sample deck",
                &UndoSnapshot { inserted_notes, inserted_cards, inserted_decks, ..Default::default() },
            )?;
            core.mark_changed()?;
        }
        Ok(deck_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    #[test]
    fn standard_note_types_one_card_per_template_that_renders() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let a = h.add_note(&basic, 1, &["<b>Question</b>", "Answer"], &[]);
        assert_eq!(a.card_ids.len(), 1);
        let b = h.add_note(&rev, 1, &["Front", "Back"], &[]);
        assert_eq!(h.col.get_cards_of_note(b.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0, 1]);
        let c = h.add_note(&rev, 1, &["Front only", ""], &[]);
        assert_eq!(h.col.get_cards_of_note(c.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0]);
        let card = h.col.get_card(a.card_ids[0]).unwrap();
        assert_eq!(
            (card.ctype, card.queue, card.due, card.did, card.ivl, card.reps),
            (CardType::New, CardQueue::New, 1, 1, 0, 0)
        );
        assert_eq!(card.data, CardData::default());
        // Siblings share a position; the next note gets the next one.
        assert_eq!(h.col.get_cards_of_note(b.note_id).unwrap().iter().map(|c| c.due).collect::<Vec<_>>(), [2, 2]);
        assert_eq!(h.meta("next_pos").as_deref(), Some("4"));
    }

    #[test]
    fn cloze_one_card_per_distinct_cloze_number_nested_included() {
        let h = make_collection();
        let cloze = h.notetype_by_name("Cloze");
        let r = h.add_note(
            &cloze,
            1,
            &["{{c1::Cairo}} is the capital of {{c3::Egypt}}; {{c1::again}} {{c2::outer {{c4::inner}}}}", "extra"],
            &[],
        );
        assert_eq!(h.col.get_cards_of_note(r.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0, 1, 2, 3]);
        let none = h.add_note(&cloze, 1, &["no clozes here", ""], &[]);
        assert_eq!(h.col.get_cards_of_note(none.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn stores_sfld_csum_and_plain_text_and_indexes_fts() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let r = h.add_note(&basic, 1, &["<div>test</div>", "مرحبا <img src=\"cat.png\">"], &["Arabic"]);
        let row: (String, i64, String, String) = h
            .col
            .core()
            .db()
            .get("SELECT sfld, csum, plain, tags FROM notes WHERE id = ?", [r.note_id], |x| {
                Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?))
            })
            .unwrap()
            .unwrap();
        assert_eq!(row.0, "test");
        assert_eq!(row.1, 2840236005);
        assert_eq!(row.1, field_checksum("test") as i64);
        assert_eq!(row.2, "test مرحبا cat.png");
        assert_eq!(row.3, " Arabic ");
        let hits: Vec<i64> =
            h.col.core().db().all("SELECT rowid FROM notes_fts WHERE notes_fts MATCH 'رحب'", [], |x| x.get(0)).unwrap();
        assert_eq!(hits, vec![r.note_id]);
    }

    #[test]
    fn refuses_an_empty_first_field_unknown_note_types_decks_and_extra_fields() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let err = |r: Result<AddNoteResult>| r.unwrap_err().code;
        assert_eq!(err(h.col.add_note(basic.id, 1, &strs(&["<br>", "x"]), &[])), ErrorCode::Invalid);
        assert_eq!(err(h.col.add_note(basic.id, 999, &strs(&["a", "b"]), &[])), ErrorCode::NotFound);
        assert_eq!(err(h.col.add_note(42, 1, &strs(&["a", "b"]), &[])), ErrorCode::NotFound);
        assert_eq!(err(h.col.add_note(basic.id, 1, &strs(&["a", "b", "c"]), &[])), ErrorCode::Invalid);
        // Short field lists are padded.
        let r = h.add_note(&basic, 1, &["a"], &[]);
        assert_eq!(h.col.get_note(r.note_id).unwrap().fields, strs(&["a", ""]));
    }

    #[test]
    fn update_note_adds_newly_generated_cards_and_removes_unstudied_ones_no_longer_generated() {
        let h = make_collection();
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let deck = h.col.create_deck("Words", None).unwrap();
        let r = h.add_note(&rev, deck.id, &["front", ""], &[]);
        let upd = |f: &[&str]| h.col.update_note(r.note_id, &strs(f), &[]).unwrap();
        assert_eq!(upd(&["front", "back"]), UpdateNoteResult { cards_added: 1, cards_removed: 0 });
        let cards = h.col.get_cards_of_note(r.note_id).unwrap();
        assert_eq!(cards.iter().map(|c| (c.ord, c.did)).collect::<Vec<_>>(), [(0, deck.id), (1, deck.id)]);
        assert_eq!(cards[1].due, cards[0].due);
        assert_eq!(upd(&["front", ""]), UpdateNoteResult { cards_added: 0, cards_removed: 1 });
        // A studied card is kept.
        upd(&["front", "back"]);
        let c2 = h.col.get_cards_of_note(r.note_id).unwrap()[1].clone();
        h.col.core().db().run("UPDATE cards SET type = 2, queue = 2, reps = 3 WHERE id = ?", [c2.id]).unwrap();
        assert_eq!(upd(&["front", ""]), UpdateNoteResult { cards_added: 0, cards_removed: 0 });
        // No-op edit: nothing recorded.
        let label = h.col.undo_label().unwrap();
        h.col.core().db().run("DELETE FROM undo_log", []).unwrap();
        assert_eq!(upd(&["front", ""]), UpdateNoteResult { cards_added: 0, cards_removed: 0 });
        assert_eq!(h.col.undo_label().unwrap(), None);
        assert_eq!(label.as_deref(), Some("Edit note"));
        assert_eq!(h.col.update_note(999, &strs(&["x"]), &[]).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn find_duplicates_compares_stripped_first_fields_within_a_note_type() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let a = h.add_note(&basic, 1, &["<b>Apple</b>", "1"], &[]);
        h.add_note(&rev, 1, &["Apple", "2"], &[]);
        assert_eq!(h.col.find_duplicates(basic.id, "Apple").unwrap(), vec![a.note_id]);
        assert_eq!(h.col.find_duplicates(basic.id, "apple").unwrap(), Vec::<NoteId>::new());
        assert_eq!(h.col.find_duplicates(basic.id, "  ").unwrap(), Vec::<NoteId>::new());
    }

    #[test]
    fn delete_notes_removes_notes_and_cards_keeps_revlog_writes_graves() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let a = h.add_note(&basic, 1, &["a", "b"], &[]);
        h.col
            .core()
            .db()
            .run(
                "INSERT INTO revlog (id, cid, ease, ivl, last_ivl, factor, time, type) VALUES (?, ?, 3, 1, 0, 2500, 5000, 0)",
                rusqlite::params![1, a.card_ids[0]],
            )
            .unwrap();
        h.col.delete_notes(&[a.note_id]).unwrap();
        assert_eq!(h.count("SELECT count(*) FROM notes"), 0);
        assert_eq!(h.count("SELECT count(*) FROM cards"), 0);
        assert_eq!(h.count("SELECT count(*) FROM revlog"), 1);
        assert_eq!(h.count("SELECT count(*) FROM graves"), 2);
        assert_eq!(h.col.get_note(a.note_id).unwrap_err().code, ErrorCode::NotFound);
        h.col.delete_notes(&[]).unwrap();
        h.col.delete_notes(&[12345]).unwrap();
    }

    #[test]
    fn canonicalises_tags_and_maintains_the_tags_table() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["a", ""], &["Lang::Arabic", "verbs  noun", "VERBS"]);
        let r = h.add_note(&basic, 1, &["b", ""], &["lang::arabic", "z::"]);
        assert_eq!(h.col.get_note(r.note_id).unwrap().tags, strs(&["Lang::Arabic", "z"]));
        let tags = h.col.all_tags().unwrap();
        let want = [("Lang::Arabic", 2), ("noun", 1), ("verbs", 1), ("z", 1)];
        assert_eq!(tags.iter().map(|t| (t.tag.as_str(), t.count)).collect::<Vec<_>>(), want);
    }

    #[test]
    fn rename_tag_renames_children_updates_registry_and_is_undoable() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let a = h.add_note(&basic, 1, &["a", ""], &["lang::ar", "other"]);
        let b = h.add_note(&basic, 1, &["b", ""], &["Lang"]);
        h.add_note(&basic, 1, &["c", ""], &["language"]);
        assert_eq!(h.col.rename_tag("lang", "Languages").unwrap(), 2);
        assert_eq!(h.col.get_note(a.note_id).unwrap().tags, strs(&["Languages::ar", "other"]));
        assert_eq!(h.col.get_note(b.note_id).unwrap().tags, strs(&["Languages"]));
        let tags: Vec<String> = h.col.all_tags().unwrap().into_iter().map(|t| t.tag).collect();
        assert!(tags.contains(&"Languages::ar".to_string()));
        assert!(tags.contains(&"language".to_string()));
        assert!(!tags.contains(&"lang::ar".to_string()));
        let hits: Vec<i64> = h
            .col
            .core()
            .db()
            .all("SELECT rowid FROM notes_fts WHERE notes_fts MATCH 'tags:Languages'", [], |x| x.get(0))
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Rename tag");
        assert_eq!(h.col.get_note(a.note_id).unwrap().tags, strs(&["lang::ar", "other"]));
        assert!(h.col.all_tags().unwrap().iter().any(|t| t.tag == "lang::ar"));
        assert_eq!(h.col.rename_tag("", "x").unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(h.col.rename_tag("x", "a b").unwrap_err().code, ErrorCode::Invalid);
    }

    // ── plain.test.ts ──
    #[test]
    fn rebuild_plain_rewrites_stale_rows_once_on_open_when_plain_version_is_old() {
        let mut h = make_collection();
        assert_eq!(h.meta("plain_version").unwrap(), crate::text::PLAIN_VERSION.to_string());
        let cloze = h.notetype_by_name("Cloze");
        let r = h.add_note(&cloze, 1, &["The {{c1::method of loci}} is a {{c2::memory palace::technique}}", ""], &[]);
        let fresh: (String, i64) = h
            .col
            .core()
            .db()
            .get("SELECT plain, mtime FROM notes WHERE id = ?", [r.note_id], |x| Ok((x.get(0)?, x.get(1)?)))
            .unwrap()
            .unwrap();
        assert_eq!(fresh.0, "The method of loci is a memory palace");
        // simulate a collection written by the old rules
        h.col
            .core()
            .db()
            .run(
                "UPDATE notes SET plain = ? WHERE id = ?",
                rusqlite::params!["The {{c1::method of loci}} is a {{c2::memory palace::technique}}", r.note_id],
            )
            .unwrap();
        h.col.core().db().run("DELETE FROM col_meta WHERE key = 'plain_version'", []).unwrap();
        assert_eq!(h.count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH '\"loci is a memory\"'"), 0);
        h.col.close().unwrap();
        let col = h.reopen();
        let after: (String, i64) = col
            .core()
            .db()
            .get("SELECT plain, mtime FROM notes WHERE id = ?", [r.note_id], |x| Ok((x.get(0)?, x.get(1)?)))
            .unwrap()
            .unwrap();
        assert_eq!(after, fresh); // rebuilt, mtime untouched
        assert_eq!(col.core().get_meta("plain_version").unwrap().as_deref(), Some("2"));
        assert_eq!(
            col.core().db().count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH '\"loci is a memory\"'", []).unwrap(),
            1
        );
        h.col = col;
    }

    #[test]
    fn rebuild_plain_works_in_chunks() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        for i in 0..7 {
            h.add_note(&basic, 1, &[&format!("front {{{{c1::{i}}}}}"), "back"], &[]);
        }
        h.col.core().db().run("UPDATE notes SET plain = 'stale'", []).unwrap();
        assert_eq!(rebuild_plain(h.col.core(), 3).unwrap(), 7);
        let plains: Vec<String> = h.col.core().db().all("SELECT plain FROM notes ORDER BY id", [], |r| r.get(0)).unwrap();
        assert_eq!(plains, (0..7).map(|i| format!("front {i} back")).collect::<Vec<_>>());
        assert_eq!(rebuild_plain(h.col.core(), 3).unwrap(), 0);
    }

    // ── sample deck (export.ts installSampleDeck) ──
    #[test]
    fn install_sample_deck_is_idempotent_and_undoable() {
        let mut h = make_collection();
        let deck_id = h.col.install_sample_deck().unwrap();
        let deck = h.col.get_deck(deck_id).unwrap();
        assert_eq!(deck.name, sample_deck().name);
        assert_eq!(deck.description, sample_deck().description);
        assert_eq!(h.count("SELECT count(*) FROM cards"), 30);
        assert_eq!(h.count("SELECT count(*) FROM notes"), sample_deck().notes.len() as i64);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Install sample deck"));
        // Again: nothing new.
        assert_eq!(h.col.install_sample_deck().unwrap(), deck_id);
        assert_eq!(h.count("SELECT count(*) FROM cards"), 30);
        assert_eq!(h.col.undo().unwrap().unwrap().label, "Install sample deck");
        assert_eq!(h.count("SELECT count(*) FROM cards"), 0);
        assert!(h.col.core().deck_id_by_name(&sample_deck().name).unwrap().is_none());
    }

    #[test]
    fn install_sample_deck_recreates_a_missing_built_in_note_type() {
        let h = make_collection();
        let cloze = h.notetype_by_name("Cloze");
        h.col.delete_notetype(cloze.id).unwrap();
        h.col.install_sample_deck().unwrap();
        assert_eq!(h.count("SELECT count(*) FROM cards"), 30);
        assert!(h.col.list_notetypes().unwrap().iter().any(|n| n.name == "Cloze"));
    }
}
