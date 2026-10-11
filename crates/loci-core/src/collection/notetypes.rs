//! Note type persistence (recall `src/core/collection/notetypes.ts`). Saving an existing note
//! type may rename, reorder, add or remove fields and templates: notes and cards are remapped,
//! then missing cards are generated.
//!
//! Identity rule for edits: in the submitted note type, the ARRAY POSITION is the new ord, and
//! each field's/template's `ord` property names its ORIGINAL ord in the stored note type (so a
//! rename keeps `ord`, a move keeps `ord` but changes position). An `ord` that is out of range,
//! negative or already claimed marks a new field/template.

use crate::collection::internals::{CollectionCore, IdTable, DEFAULT_DECK_ID};
use crate::collection::rows::{note_derived_columns, row_to_note, select_sql, to_json, NOTE_COLUMNS};
use crate::db::unicase_compare;
use crate::error::{LociError, Result};
use crate::template::{card_ords_for_note, compute_req, pad_fields, CardOrdsOptions};
use crate::text::join_fields;
use crate::types::*;
use crate::undo;
use std::collections::{HashMap, HashSet};

/// Anki's field-name rules: strip `:{}"`, leading `#/^`, surrounding whitespace.
pub fn clean_field_name(name: &str) -> String {
    let stripped: String = name.chars().filter(|c| !matches!(c, ':' | '{' | '}' | '"')).collect();
    stripped.trim_start_matches(|c: char| c == '#' || c == '/' || c == '^' || c.is_whitespace()).trim().to_string()
}

fn validate_names(kind: &str, names: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for n in names {
        if n.is_empty() {
            return Err(LociError::invalid(format!("{kind} names cannot be empty")));
        }
        if !seen.insert(n.to_lowercase()) {
            return Err(LociError::invalid(format!("Duplicate {} name: {n}", kind.to_lowercase())));
        }
    }
    Ok(())
}

/// Maps new position → original ord (or None for new entries).
fn identity_map(ords: &[i64], old_count: usize) -> Vec<Option<i64>> {
    let mut claimed = HashSet::new();
    ords.iter()
        .map(|&o| {
            if o >= 0 && (o as usize) < old_count && !claimed.contains(&o) {
                claimed.insert(o);
                Some(o)
            } else {
                None
            }
        })
        .collect()
}

/// A unique name: appends '+' until no other note type uses it (Anki behaviour).
fn unique_notetype_name(core: &CollectionCore, name: &str, self_id: NotetypeId) -> Result<String> {
    let mut n = name.to_string();
    while core
        .db()
        .scalar::<i64, _>("SELECT 1 FROM notetypes WHERE name = ? AND id != ?", rusqlite::params![n, self_id])?
        .is_some()
    {
        n.push('+');
    }
    Ok(n)
}

fn write_notetype_rows(core: &CollectionCore, nt: &Notetype) -> Result<()> {
    let db = core.db();
    db.run("DELETE FROM notetypes WHERE id = ?", [nt.id])?;
    db.run("DELETE FROM fields WHERE ntid = ?", [nt.id])?;
    db.run("DELETE FROM templates WHERE ntid = ?", [nt.id])?;
    db.run(
        "INSERT INTO notetypes (id, name, kind, css, latex_pre, latex_post, sort_field, config, mtime, usn) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            nt.id,
            nt.name,
            nt.kind as i64,
            nt.css,
            nt.latex_pre,
            nt.latex_post,
            nt.sort_field,
            to_json(&nt.config),
            nt.mtime,
            nt.usn
        ],
    )?;
    for f in &nt.fields {
        db.run(
            "INSERT INTO fields (ntid, ord, name, config) VALUES (?, ?, ?, ?)",
            rusqlite::params![nt.id, f.ord, f.name, to_json(&f.config)],
        )?;
    }
    for t in &nt.templates {
        db.run(
            "INSERT INTO templates (ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![nt.id, t.ord, t.name, t.qfmt, t.afmt, t.bqfmt, t.bafmt, to_json(&t.config)],
        )?;
    }
    Ok(())
}

/// Creates or updates a note type; remaps notes/cards on structural edits, recomputes
/// `config.req`, regenerates missing cards. Not undoable (clears the undo stack when notes changed).
pub fn save_notetype(core: &CollectionCore, input: &NotetypeInput) -> Result<Notetype> {
    core.db().transaction(|| {
        let existing = match input.id {
            Some(id) if core.db().scalar::<i64, _>("SELECT 1 FROM notetypes WHERE id = ?", [id])?.is_some() => {
                Some(core.notetype(id)?)
            }
            _ => None,
        };
        let kind = input.kind;
        if input.fields.is_empty() {
            return Err(LociError::invalid("A note type needs at least one field"));
        }
        if input.templates.is_empty() {
            return Err(LociError::invalid("A note type needs at least one card template"));
        }
        if kind == NotetypeKind::Cloze && input.templates.len() != 1 {
            return Err(LociError::invalid("A cloze note type has exactly one template"));
        }
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(LociError::invalid("Note type name is empty"));
        }

        let fields: Vec<Field> = input
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| Field { ord: i as i64, name: clean_field_name(&f.name), config: f.config.clone() })
            .collect();
        let templates: Vec<Template> = input
            .templates
            .iter()
            .enumerate()
            .map(|(i, t)| Template {
                ord: i as i64,
                name: t.name.trim().to_string(),
                qfmt: t.qfmt.clone(),
                afmt: t.afmt.clone(),
                bqfmt: t.bqfmt.clone(),
                bafmt: t.bafmt.clone(),
                config: t.config.clone(),
            })
            .collect();
        validate_names("Field", &fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>())?;
        validate_names("Template", &templates.iter().map(|t| t.name.clone()).collect::<Vec<_>>())?;
        let sort_field = if input.sort_field >= 0 && (input.sort_field as usize) < fields.len() { input.sort_field } else { 0 };

        let id = match (&existing, input.id) {
            (Some(e), _) => e.id,
            (None, Some(given)) if given > 0 => given,
            _ => core.new_id(IdTable::Notetypes)?,
        };
        let mut nt = Notetype {
            id,
            name: unique_notetype_name(core, &name, id)?,
            kind,
            css: input.css.clone(),
            latex_pre: input.latex_pre.clone(),
            latex_post: input.latex_post.clone(),
            sort_field,
            fields,
            templates,
            config: NotetypeConfig { req: vec![], ..input.config.clone() },
            mtime: core.now_seconds(),
            usn: -1,
        };
        nt.config.req = compute_req(&nt);

        let mut notes_changed = false;
        if let Some(existing) = &existing {
            let note_count = core.db().count("SELECT count(*) FROM notes WHERE ntid = ?", [id])?;
            if existing.kind != kind && note_count > 0 {
                return Err(LociError::invalid("Cannot switch between standard and cloze while notes use this note type"));
            }
            let field_map = identity_map(&input.fields.iter().map(|f| f.ord).collect::<Vec<_>>(), existing.fields.len());
            let template_map = identity_map(&input.templates.iter().map(|t| t.ord).collect::<Vec<_>>(), existing.templates.len());
            let fields_changed =
                field_map.len() != existing.fields.len() || field_map.iter().enumerate().any(|(i, o)| *o != Some(i as i64));
            let templates_changed = kind == NotetypeKind::Standard
                && (template_map.len() != existing.templates.len()
                    || template_map.iter().enumerate().any(|(i, o)| *o != Some(i as i64)));
            write_notetype_rows(core, &nt)?;
            core.invalidate_caches();
            if note_count > 0 {
                if templates_changed {
                    remap_cards(core, id, &template_map, existing.templates.len())?;
                }
                if fields_changed || sort_field != existing.sort_field {
                    remap_notes(core, &nt, if fields_changed { Some(&field_map) } else { None })?;
                }
                notes_changed = fields_changed || templates_changed;
            }
        } else {
            write_notetype_rows(core, &nt)?;
            core.invalidate_caches();
        }
        generate_missing_cards(core, &nt)?;
        if notes_changed {
            undo::clear(core.db())?;
        }
        core.mark_changed()?;
        core.notetype(id)
    })
}

/// Moves card ords to the new template positions; deletes cards of removed templates.
fn remap_cards(core: &CollectionCore, ntid: NotetypeId, template_map: &[Option<i64>], old_count: usize) -> Result<()> {
    let mut old_to_new: HashMap<i64, i64> = HashMap::new();
    for (pos, old) in template_map.iter().enumerate() {
        if let Some(o) = old {
            old_to_new.insert(*o, pos as i64);
        }
    }
    let cards: Vec<(CardId, NoteId, i64)> =
        core.db().all("SELECT c.id, c.nid, c.ord FROM cards c JOIN notes n ON n.id = c.nid WHERE n.ntid = ?", [ntid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    let removed: Vec<&(CardId, NoteId, i64)> =
        cards.iter().filter(|(_, _, ord)| !old_to_new.contains_key(ord) && (*ord as usize) < old_count).collect();
    // Refuse if a note would lose all of its cards.
    let mut remaining: HashMap<NoteId, i64> = HashMap::new();
    for (_, nid, ord) in &cards {
        if old_to_new.contains_key(ord) {
            *remaining.entry(*nid).or_insert(0) += 1;
        }
    }
    for (_, nid, _) in &removed {
        if remaining.get(nid).copied().unwrap_or(0) == 0 {
            return Err(LociError::invalid(
                "Removing this template would leave some notes without cards; delete those notes first",
            ));
        }
    }
    let removed_ids: Vec<CardId> = removed.iter().map(|(id, _, _)| *id).collect();
    undo::delete_rows_by_id(core.db(), "cards", &removed_ids)?;
    for id in &removed_ids {
        core.db().run("INSERT INTO graves (oid, type, usn) VALUES (?, ?, -1)", rusqlite::params![id, GraveType::Card as i64])?;
    }
    // Shift ords via a negative staging value to avoid transient duplicates.
    let mtime = core.now_seconds();
    for (id, _, ord) in &cards {
        if let Some(to) = old_to_new.get(ord) {
            if to != ord {
                core.db()
                    .run("UPDATE cards SET ord = ?, mtime = ?, usn = -1 WHERE id = ?", rusqlite::params![-1 - to, mtime, id])?;
            }
        }
    }
    core.db().run("UPDATE cards SET ord = -1 - ord WHERE ord < 0 AND nid IN (SELECT id FROM notes WHERE ntid = ?)", [ntid])?;
    Ok(())
}

/// Rewrites note fields for a new field layout (None = layout unchanged) and derived columns.
fn remap_notes(core: &CollectionCore, nt: &Notetype, field_map: Option<&[Option<i64>]>) -> Result<()> {
    let notes = core.db().all(&format!("{} WHERE ntid = ?", select_sql("notes", &NOTE_COLUMNS)), [nt.id], row_to_note)?;
    let mtime = core.now_seconds();
    for note in notes {
        let old = &note.fields;
        let fields: Vec<String> = match field_map {
            Some(map) => map.iter().map(|o| o.and_then(|i| old.get(i as usize).cloned()).unwrap_or_default()).collect(),
            None => old.clone(),
        };
        let d = note_derived_columns(&fields, nt.sort_field);
        core.db().run(
            "UPDATE notes SET flds = ?, sfld = ?, csum = ?, plain = ?, mtime = ?, usn = -1 WHERE id = ?",
            rusqlite::params![
                join_fields(&fields),
                d.sfld,
                d.csum,
                d.plain,
                if field_map.is_some() { mtime } else { note.mtime },
                note.id
            ],
        )?;
    }
    Ok(())
}

fn deck_exists(core: &CollectionCore, id: DeckId) -> Result<bool> {
    Ok(core.db().scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [id])?.is_some())
}

/// Adds cards that the note type's templates now generate but do not exist yet.
pub fn generate_missing_cards(core: &CollectionCore, nt: &Notetype) -> Result<i64> {
    let notes = core.db().all(&format!("{} WHERE ntid = ?", select_sql("notes", &NOTE_COLUMNS)), [nt.id], row_to_note)?;
    let mut added = 0;
    for note in notes {
        let have: Vec<(i64, DeckId)> =
            core.db().all("SELECT ord, did FROM cards WHERE nid = ? ORDER BY ord", [note.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let have_ords: HashSet<i64> = have.iter().map(|(o, _)| *o).collect();
        let want: Vec<i64> = card_ords_for_note(nt, &pad_fields(nt, &note.fields), &CardOrdsOptions::default())
            .into_iter()
            .filter(|o| !have_ords.contains(o))
            .collect();
        // A note with no cards at all gets ord 0 (ensure_not_empty); otherwise only real generations.
        let ords = if !have.is_empty() || !want.is_empty() { want } else { vec![0] };
        if ords.is_empty() {
            continue;
        }
        let deck = have.first().map(|(_, d)| *d).unwrap_or(DEFAULT_DECK_ID);
        let target = if deck_exists(core, deck)? { deck } else { DEFAULT_DECK_ID };
        added += core.insert_new_cards(&note, nt, &ords, target)?.len() as i64;
    }
    Ok(added)
}

/// Deletes a note type. Refuses while notes use it, and refuses to delete the last note type.
pub fn delete_notetype(core: &CollectionCore, id: NotetypeId) -> Result<()> {
    core.db().transaction(|| {
        core.notetype(id)?;
        if core.db().scalar::<i64, _>("SELECT 1 FROM notes WHERE ntid = ? LIMIT 1", [id])?.is_some() {
            return Err(LociError::conflict("Notes still use this note type"));
        }
        if core.db().count("SELECT count(*) FROM notetypes", [])? <= 1 {
            return Err(LociError::invalid("The last note type cannot be deleted"));
        }
        core.db().run("DELETE FROM notetypes WHERE id = ?", [id])?;
        core.db().run("DELETE FROM fields WHERE ntid = ?", [id])?;
        core.db().run("DELETE FROM templates WHERE ntid = ?", [id])?;
        core.invalidate_caches();
        core.mark_changed()
    })
}

/// Note types sorted by name.
pub fn list_notetypes(core: &CollectionCore) -> Result<Vec<Notetype>> {
    let mut v = core.all_notetypes()?;
    v.sort_by(|a, b| unicase_compare(&a.name, &b.name));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::builtin_notetypes::notetype_to_input;
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    #[test]
    fn clean_field_names_follow_anki() {
        assert_eq!(clean_field_name("  #Front:{x}\"  "), "Frontx");
        assert_eq!(clean_field_name("/^ok"), "ok");
    }

    #[test]
    fn saving_a_note_type_with_a_new_template_generates_cards_for_existing_notes() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let deck = h.col.create_deck("D", None).unwrap();
        let n1 = h.add_note(&basic, deck.id, &["q", "a"], &[]);
        let n2 = h.add_note(&basic, deck.id, &["q2", ""], &[]);
        let mut input = notetype_to_input(&basic);
        input.templates.push(Template {
            ord: -1,
            name: "Reverse".into(),
            qfmt: "{{Back}}".into(),
            afmt: "{{FrontSide}}<hr id=answer>{{Front}}".into(),
            bqfmt: String::new(),
            bafmt: String::new(),
            config: TemplateConfig::default(),
        });
        let saved = h.col.save_notetype(&input).unwrap();
        assert_eq!(saved.templates.iter().map(|t| (t.ord, t.name.as_str())).collect::<Vec<_>>(), [(0, "Card 1"), (1, "Reverse")]);
        assert_eq!(saved.config.req, vec![Requirement(0, ReqKind::Any, vec![0]), Requirement(1, ReqKind::Any, vec![1])]);
        let cards = h.col.get_cards_of_note(n1.note_id).unwrap();
        assert_eq!(cards.iter().map(|c| (c.ord, c.did)).collect::<Vec<_>>(), [(0, deck.id), (1, deck.id)]);
        assert_eq!(h.col.get_cards_of_note(n2.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn reordering_and_renaming_fields_remaps_notes_and_removing_a_template_deletes_its_cards() {
        let h = make_collection();
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let n = h.add_note(&rev, 1, &["F", "B"], &[]);
        let (front, back) = (rev.fields[0].clone(), rev.fields[1].clone());
        let mut input = notetype_to_input(&rev);
        input.fields = vec![
            Field { name: "Answer".into(), ..back },
            Field { name: "Question".into(), ..front.clone() },
            Field { ord: 99, name: "Notes".into(), config: front.config.clone() },
        ];
        input.templates = vec![Template { qfmt: "{{Answer}}".into(), afmt: "{{Question}}".into(), ..rev.templates[1].clone() }];
        input.sort_field = 1;
        let saved = h.col.save_notetype(&input).unwrap();
        assert_eq!(saved.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Answer", "Question", "Notes"]);
        assert_eq!(h.col.get_note(n.note_id).unwrap().fields, strs(&["B", "F", ""]));
        assert_eq!(h.col.get_cards_of_note(n.note_id).unwrap().iter().map(|c| c.ord).collect::<Vec<_>>(), [0]);
        let sfld: String = h.col.core().db().scalar("SELECT sfld FROM notes WHERE id = ?", [n.note_id]).unwrap().unwrap();
        assert_eq!(sfld, "F");
        // The structural edit cleared the undo stack.
        assert_eq!(h.col.undo_label().unwrap(), None);
    }

    #[test]
    fn validates_names_and_kinds_and_refuses_deleting_used_note_types() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let mut dup = notetype_to_input(&basic);
        dup.fields[1].name = "front".into();
        assert_eq!(h.col.save_notetype(&dup).unwrap_err().code, ErrorCode::Invalid);
        let mut copy_in = notetype_to_input(&basic);
        copy_in.id = Some(0);
        let copy = h.col.save_notetype(&copy_in).unwrap();
        assert_eq!(copy.name, "Basic+");
        assert_ne!(copy.id, basic.id);
        h.add_note(&basic, 1, &["x", "y"], &[]);
        assert_eq!(h.col.delete_notetype(basic.id).unwrap_err().code, ErrorCode::Conflict);
        let mut cloze = notetype_to_input(&basic);
        cloze.kind = NotetypeKind::Cloze;
        assert_eq!(h.col.save_notetype(&cloze).unwrap_err().code, ErrorCode::Invalid);
        h.col.delete_notetype(copy.id).unwrap();
        assert_eq!(h.col.get_notetype(copy.id).unwrap_err().code, ErrorCode::NotFound);
        let mut empty = notetype_to_input(&basic);
        empty.templates.clear();
        assert_eq!(h.col.save_notetype(&empty).unwrap_err().code, ErrorCode::Invalid);
        let mut blank = notetype_to_input(&basic);
        blank.name = " ".into();
        assert_eq!(h.col.save_notetype(&blank).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn removing_a_template_that_would_leave_a_note_cardless_is_refused() {
        let h = make_collection();
        let rev = h.notetype_by_name("Basic (and reversed card)");
        let n = h.add_note(&rev, 1, &["f", "b"], &[]);
        h.col.core().db().run("DELETE FROM cards WHERE nid = ? AND ord = 0", [n.note_id]).unwrap(); // ord 1 only
        let mut input = notetype_to_input(&rev);
        input.templates = vec![rev.templates[0].clone()];
        assert_eq!(h.col.save_notetype(&input).unwrap_err().code, ErrorCode::Invalid);
    }
}
