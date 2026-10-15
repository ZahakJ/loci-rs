//! Merging a bundle into the collection (docs/SYNC.md "Merge").
//!
//! Every object is last-writer-wins by stamp; reviews are a union in which a deletion beats a
//! review. After the winners are written, deterministic repairs run (they depend only on the
//! merged state, so every device makes the same ones) and are stamped as this device's changes.

use super::bundle::Bundle;
use super::clock::{covers, dominates, hlc_ms, join, Stamp};
use super::state::{self, RowState};
use super::tables::{self, payload_hash, Kind, Payload};
use crate::anki::add_hash_suffix;
use crate::collection::internals::{meta, CollectionCore, DEFAULT_DECK_ID, DEFAULT_PRESET_ID};
use crate::collection::media::MEDIA_PREFIX;
use crate::collection::rows::{row_to_note, select_sql, NOTE_COLUMNS};
use crate::error::{LociError, Result};
use crate::template::{card_ords_for_note, pad_fields, CardOrdsOptions};
use crate::text::split_tags;
use crate::types::NotetypeKind;
use crate::undo;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Placeholder separator for a deck whose name is taken while the merge runs.
const NAME_SEP: char = '\u{1f}';

/// One object the merge will write.
#[derive(Debug, Clone)]
pub struct Step {
    pub kind: Kind,
    pub key: String,
    pub stamp: Stamp,
    pub deleted: bool,
    pub payload: Option<Payload>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub rows_in: i64,
    pub conflicts: i64,
    pub reviews_added: i64,
    pub complete: bool,
}

/// Does the incoming version beat the local one? Reviews: a deletion beats a review, then the
/// later stamp. Everything else: the later stamp.
fn beats(kind: Kind, stamp: &Stamp, deleted: bool, local: &RowState) -> bool {
    if kind == Kind::Revlog {
        (deleted, stamp) > (local.deleted, &local.stamp)
    } else {
        *stamp > local.stamp
    }
}

/// Decides which incoming objects win, in apply order. Also counts conflicts (objects both
/// sides changed without seeing the other's change) and whether the bundle is complete for us.
pub fn plan(core: &CollectionCore, b: &Bundle) -> Result<(Vec<Step>, i64, bool)> {
    let db = core.db();
    let mine = state::vector(db)?;
    let complete = dominates(&mine, &b.header.base);
    let mut steps = Vec::new();
    let mut conflicts = 0;
    let mut by_kind: BTreeMap<Kind, Vec<usize>> = BTreeMap::new();
    for (i, r) in b.rows.iter().enumerate() {
        if let Some(kind) = Kind::from_name(&r.0) {
            by_kind.entry(kind).or_default().push(i);
        }
    }
    for (kind, idx) in by_kind {
        let local_rows = if idx.len() > 2000 { Some(state::load_rows(db, kind)?) } else { None };
        let mut kind_steps = Vec::new();
        for i in idx {
            let r = &b.rows[i];
            let stamp = r.stamp();
            let deleted = r.deleted();
            let local = match &local_rows {
                Some(m) => m.get(&r.1).cloned(),
                None => state::load_row(db, kind, &r.1)?,
            };
            let win = match &local {
                None => true,
                Some(l) => beats(kind, &stamp, deleted, l),
            };
            if let Some(l) = &local {
                // Rows a new device started with (stamped at time zero) are not conflicts.
                let started_with = hlc_ms(l.stamp.hlc) == 0;
                if !started_with && l.stamp != stamp && !covers(&b.header.vector, &l.stamp) && !covers(&mine, &stamp) {
                    conflicts += 1;
                }
            }
            if !win {
                continue;
            }
            if !deleted && r.5.is_none() {
                return Err(LociError::corrupt(format!("The sync file has a {} row without values", kind.name())));
            }
            kind_steps.push(Step { kind, key: r.1.clone(), stamp, deleted, payload: if deleted { None } else { r.5.clone() } });
        }
        kind_steps.sort_by(|a, b| a.key.cmp(&b.key));
        steps.extend(kind_steps);
    }
    Ok((steps, conflicts, complete))
}

fn media_path(name: &str) -> String {
    format!("{MEDIA_PREFIX}{name}")
}

/// Writes the winners, repairs, stamps the repairs, and records what the sender had seen, in
/// one transaction.
pub fn apply(core: &CollectionCore, b: &Bundle, steps: &[Step], complete: bool) -> Result<Outcome> {
    let db = core.db();
    let own = state::device(db)?.unwrap_or_default();
    db.transaction(|| {
        let ctx = state::ctx(core)?;
        let mut out = Outcome { complete, ..Default::default() };
        let mut locals: Vec<(Kind, String)> = Vec::new();
        let mut touched_notes: BTreeSet<i64> = BTreeSet::new();
        let mut touched_ntids: BTreeSet<i64> = BTreeSet::new();
        for s in steps {
            if s.deleted {
                match s.kind {
                    Kind::Media => core.blobs.delete(&media_path(&s.key))?,
                    Kind::Cards => {
                        if let Some(nid) =
                            db.scalar::<i64, _>("SELECT nid FROM cards WHERE id = ?", [s.key.parse::<i64>().unwrap_or(0)])?
                        {
                            touched_notes.insert(nid);
                        }
                    }
                    _ => {}
                }
                tables::delete(db, s.kind, &s.key)?;
                state::store_row(db, s.kind, &s.key, &RowState { stamp: s.stamp.clone(), hash: 0, deleted: true })?;
            } else {
                let original = s.payload.as_ref().ok_or_else(|| LociError::internal("live step without payload"))?;
                let mut payload = original.clone();
                match s.kind {
                    Kind::Decks => {
                        let name = payload.get(1).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let id: i64 = s.key.parse().unwrap_or(0);
                        if db
                            .scalar::<i64, _>("SELECT id FROM decks WHERE name = ? AND id != ?", rusqlite::params![name, id])?
                            .is_some()
                        {
                            payload[1] = serde_json::Value::String(format!("{name}{NAME_SEP}{id}"));
                        }
                    }
                    Kind::Media => {
                        let sha1 = payload.get(1).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                        let old: Option<String> = db.scalar("SELECT sha1 FROM media WHERE name = ?", [&s.key])?;
                        if let Some(old) = old.filter(|o| *o != sha1) {
                            // This device's file loses the name: it stays, under Anki's hashed name.
                            if let Some(bytes) = core.blobs.get(&media_path(&s.key))? {
                                let kept = add_hash_suffix(&s.key, &old);
                                if kept != s.key {
                                    core.blobs.put(&media_path(&kept), &bytes)?;
                                    db.run(
                                        "INSERT OR IGNORE INTO media (name, sha1, size, mtime) VALUES (?, ?, ?, ?)",
                                        rusqlite::params![kept, old, bytes.len() as i64, ctx.now_s],
                                    )?;
                                    locals.push((Kind::Media, kept));
                                }
                            }
                            core.blobs.delete(&media_path(&s.key))?;
                        }
                    }
                    Kind::Notes => {
                        touched_notes.insert(s.key.parse().unwrap_or(0));
                    }
                    Kind::Cards => {
                        if let Some(nid) = payload.get(1).and_then(|v| v.as_i64()) {
                            touched_notes.insert(nid);
                        }
                    }
                    Kind::Notetypes => {
                        touched_ntids.insert(s.key.parse().unwrap_or(0));
                    }
                    _ => {}
                }
                let w = tables::write(db, s.kind, &s.key, &payload, &ctx)?;
                if s.kind == Kind::Revlog && w.inserted {
                    out.reviews_added += 1;
                }
                state::store_row(
                    db,
                    s.kind,
                    &s.key,
                    &RowState { stamp: s.stamp.clone(), hash: payload_hash(original), deleted: false },
                )?;
            }
            out.rows_in += 1;
        }
        // Everything written so far carries the sender's stamps; only repairs are ours.
        db.run("DELETE FROM sync_dirty", [])?;
        for (k, key) in &locals {
            db.run("INSERT OR IGNORE INTO sync_dirty (tbl, key) VALUES (?, ?)", [k.name(), key.as_str()])?;
        }
        core.invalidate_caches();
        if complete && out.rows_in > 0 {
            repair(core, &touched_notes, &touched_ntids)?;
        }
        fix_deck_names(core)?;
        // Our next stamps must be later than anything we have seen.
        let seen = steps.iter().map(|s| s.stamp.hlc).chain(b.header.vector.values().copied()).max().unwrap_or(0);
        state::bump_hlc(db, seen)?;
        state::stamp_pending(core)?;
        if complete {
            let mut v = state::vector(db)?;
            join(&mut v, &b.header.vector);
            state::store_vector(db, &v)?;
        }
        // A device that has not synced before joins the other's collection id; after that, ids
        // converge on the smaller.
        let first_sync = db.count("SELECT count(*) FROM sync_peers", [])? == 0;
        if let Some(cur) = core.get_meta(meta::COLLECTION_ID)? {
            let theirs = &b.header.collection;
            if b.header.device != own && !theirs.is_empty() && *theirs != cur && (first_sync || *theirs < cur) {
                core.set_meta(meta::COLLECTION_ID, Some(theirs))?;
            }
        }
        if b.header.device != own {
            db.run(
                "INSERT INTO sync_peers (dev, vector, wants, seen) VALUES (?, ?, ?, ?)
                 ON CONFLICT(dev) DO UPDATE SET vector = excluded.vector, wants = excluded.wants, seen = excluded.seen",
                rusqlite::params![
                    b.header.device,
                    serde_json::to_string(&b.header.vector)?,
                    serde_json::to_string(&b.header.wants)?,
                    ctx.now_s
                ],
            )?;
        }
        state::set(db, state::key::LAST_SYNC, Some(&ctx.now_s.to_string()))?;
        if out.rows_in > 0 {
            undo::clear(db)?;
        }
        Ok(out)
    })
}

/// Repairs that keep the merged collection consistent. Each depends only on the merged state.
fn repair(core: &CollectionCore, touched_notes: &BTreeSet<i64>, touched_ntids: &BTreeSet<i64>) -> Result<()> {
    let db = core.db();
    merge_duplicate_notetypes(core)?;
    db.run("DELETE FROM notes WHERE ntid NOT IN (SELECT id FROM notetypes)", [])?;
    db.run("DELETE FROM cards WHERE nid NOT IN (SELECT id FROM notes)", [])?;
    if db.scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [DEFAULT_DECK_ID])?.is_some() {
        db.run("UPDATE cards SET did = ? WHERE did NOT IN (SELECT id FROM decks)", [DEFAULT_DECK_ID])?;
    }
    if db.scalar::<i64, _>("SELECT 1 FROM deck_configs WHERE id = ?", [DEFAULT_PRESET_ID])?.is_some() {
        db.run("UPDATE decks SET config_id = ? WHERE config_id NOT IN (SELECT id FROM deck_configs)", [DEFAULT_PRESET_ID])?;
    }
    core.invalidate_caches();
    let mut notes: BTreeSet<i64> = touched_notes.clone();
    for ntid in touched_ntids {
        notes.extend(db.all("SELECT id FROM notes WHERE ntid = ?", [ntid], |r| r.get::<_, i64>(0))?);
    }
    notes.extend(db.all("SELECT id FROM notes WHERE id NOT IN (SELECT nid FROM cards)", [], |r| r.get::<_, i64>(0))?);
    for nid in &notes {
        fill_cards(core, *nid)?;
    }
    remove_duplicate_cards(core)?;
    for nid in touched_notes {
        if let Some(tags) = db.scalar::<String, _>("SELECT tags FROM notes WHERE id = ?", [nid])? {
            core.register_tags(&split_tags(&tags))?;
        }
    }
    Ok(())
}

/// Note types with the same name, kind, fields and templates (two devices that each started
/// with the built-in ones) become one: notes move to the one with the most notes (then the
/// smallest id) and the other is removed.
fn merge_duplicate_notetypes(core: &CollectionCore) -> Result<()> {
    let db = core.db();
    let all = core.all_notetypes()?;
    let mut groups: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for nt in &all {
        let sig = serde_json::to_string(&(
            nt.name.to_lowercase(),
            nt.kind as i64,
            nt.fields.iter().map(|f| f.name.to_lowercase()).collect::<Vec<_>>(),
            nt.templates.iter().map(|t| (t.name.clone(), t.qfmt.clone(), t.afmt.clone())).collect::<Vec<_>>(),
        ))?;
        groups.entry(sig).or_default().push(nt.id);
    }
    let mut changed = false;
    for ids in groups.values().filter(|g| g.len() > 1) {
        let mut ranked: Vec<(i64, i64)> = Vec::new();
        for id in ids {
            ranked.push((db.count("SELECT count(*) FROM notes WHERE ntid = ?", [id])?, *id));
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let keep = ranked[0].1;
        for (_, other) in &ranked[1..] {
            db.run("UPDATE notes SET ntid = ? WHERE ntid = ?", [keep, *other])?;
            tables::delete(db, Kind::Notetypes, &other.to_string())?;
            changed = true;
        }
    }
    if changed {
        core.invalidate_caches();
    }
    Ok(())
}

/// Gives a note the cards its note type calls for, with ids, decks and positions chosen from the
/// merged state alone (so every device creates the same cards); removes cards of templates that
/// no longer exist.
fn fill_cards(core: &CollectionCore, nid: i64) -> Result<()> {
    let db = core.db();
    let Some(note) = db.get(&format!("{} WHERE id = ?", select_sql("notes", &NOTE_COLUMNS)), [nid], row_to_note)? else {
        return Ok(());
    };
    let Ok(nt) = core.notetype(note.ntid) else { return Ok(()) };
    if nt.kind != NotetypeKind::Cloze && !nt.templates.is_empty() {
        let ords: Vec<String> = nt.templates.iter().map(|t| t.ord.to_string()).collect();
        db.run(&format!("DELETE FROM cards WHERE nid = ? AND ord NOT IN ({})", ords.join(", ")), [nid])?;
    }
    let existing: Vec<(i64, i64, i64, i64)> =
        db.all("SELECT ord, did, type, due FROM cards WHERE nid = ? ORDER BY ord, id", [nid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
    let have: BTreeSet<i64> = existing.iter().map(|e| e.0).collect();
    let want = card_ords_for_note(&nt, &pad_fields(&nt, &note.fields), &CardOrdsOptions::default());
    let missing: Vec<i64> = want.into_iter().filter(|o| !have.contains(o)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let due = match existing.iter().find(|e| e.2 == 0) {
        Some(e) => e.3,
        None => db.scalar::<i64, _>("SELECT coalesce(max(due), 0) FROM cards WHERE type = 0", [])?.unwrap_or(0) + 1,
    };
    let fallback_deck = existing.first().map(|e| e.1).unwrap_or(DEFAULT_DECK_ID);
    for ord in missing {
        let tmpl = if nt.kind == NotetypeKind::Cloze { nt.templates.first() } else { nt.templates.iter().find(|t| t.ord == ord) };
        let did = match tmpl.and_then(|t| t.config.deck_override) {
            Some(d) if db.scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [d])?.is_some() => d,
            _ => fallback_deck,
        };
        let mut id = nid + ord;
        while db.scalar::<i64, _>("SELECT 1 FROM cards WHERE id = ?", [id])?.is_some() {
            id += 1;
        }
        db.run(
            "INSERT INTO cards (id, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
             VALUES (?, ?, ?, ?, ?, -1, 0, 0, ?, 0, 0, 0, 0, 0, 0, 0, 0, '{}')",
            rusqlite::params![id, nid, did, ord, note.mtime, due],
        )?;
    }
    let next = core.get_meta(meta::NEXT_POS)?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(1);
    if due + 1 > next {
        core.set_meta(meta::NEXT_POS, Some(&(due + 1).to_string()))?;
    }
    Ok(())
}

/// Two cards for one template of a note (both devices generated it): the one with more reviews
/// stays (then the smaller id).
fn remove_duplicate_cards(core: &CollectionCore) -> Result<()> {
    let db = core.db();
    let dups: Vec<(i64, i64)> =
        db.all("SELECT nid, ord FROM cards GROUP BY nid, ord HAVING count(*) > 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    for (nid, ord) in dups {
        let ids: Vec<i64> =
            db.all("SELECT id FROM cards WHERE nid = ? AND ord = ? ORDER BY reps DESC, id ASC", [nid, ord], |r| r.get(0))?;
        undo::delete_rows_by_id(db, "cards", &ids[1..])?;
    }
    Ok(())
}

/// Decks that arrived under a name another deck already had: within each name, the smallest id
/// keeps it and the others become "Name (2)", "Name (3)"…
fn fix_deck_names(core: &CollectionCore) -> Result<()> {
    let db = core.db();
    let parked: Vec<String> = db.all("SELECT name FROM decks WHERE instr(name, char(31)) > 0", [], |r| r.get(0))?;
    if parked.is_empty() {
        return Ok(());
    }
    let intended = |name: &str| name.split(NAME_SEP).next().unwrap_or_default().to_string();
    let names: BTreeSet<String> = parked.iter().map(|n| intended(n).to_lowercase()).collect();
    let all: Vec<(i64, String)> = db.all("SELECT id, name FROM decks ORDER BY id", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut groups: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    for (id, name) in all {
        let want = intended(&name);
        if names.contains(&want.to_lowercase()) {
            groups.entry(want.to_lowercase()).or_default().push((id, want));
        }
    }
    let mut keys: Vec<&String> = groups.keys().collect();
    keys.sort();
    for k in keys {
        let group = &groups[k];
        for (id, want) in group {
            db.run("UPDATE decks SET name = ? WHERE id = ?", rusqlite::params![format!("{want}{NAME_SEP}{id}"), id])?;
        }
        for (i, (id, want)) in group.iter().enumerate() {
            let mut name = want.clone();
            let mut n = if i == 0 { 1 } else { 2 };
            loop {
                if n > 1 {
                    name = format!("{want} ({n})");
                }
                if db.scalar::<i64, _>("SELECT 1 FROM decks WHERE name = ?", [&name])?.is_none() {
                    break;
                }
                n += 1;
            }
            db.run("UPDATE decks SET name = ? WHERE id = ?", rusqlite::params![name, id])?;
        }
    }
    Ok(())
}
