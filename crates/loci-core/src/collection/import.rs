//! Anki package import (internals §13): preview, and merge
//! into the collection following Anki's Rust importer:
//!   - note types matched by id: same schema → updated per the conflict rule; a different note
//!     type with the same id → the incoming one is added under a new id with
//!     `config.originalId` = its Anki id (reused by later imports of the same note type);
//!     an unknown id whose name matches (case-insensitive) an identical note type here (same
//!     fields, templates and CSS) maps onto it; one matching an *unused* note type of the same
//!     kind (built-ins: same field and template count) replaces it under the Anki id and plain
//!     name; only a used, different note type of the same name makes the import add a suffix;
//!   - deck options imported as presets, deduplicated by name; decks merged by name (parents
//!     first), new decks get new ids, their "today" counters re-based onto our day;
//!   - notes matched by guid: added, updated (conflict 'newer' = incoming mtime is newer,
//!     'theirs' = whenever different, 'keep' = never) or skipped; a guid owned by a note of a
//!     different note type is skipped as conflicting;
//!   - cards of added/updated notes are added unless that (note, template) card exists; review
//!     and day-learning due days shift by (source day − our day) so they fall due on the same
//!     calendar day; intraday learning dues are unix timestamps and need no shift; FSRS state in
//!     card.data is kept verbatim; new cards keep their positions (next_pos moves past them);
//!   - revlog rows of imported cards are added (existing ids are left alone);
//!   - media: every file in the package is stored (name normalised by the reader); a name taken
//!     by different content gets Anki's `-<sha1>` suffix and imported notes are rewritten; static
//!     files (`_x`, `latex-x`) are only copied when absent.
//!
//! Without scheduling, cards are reset to new as Anki's "without scheduling" export does.
//!
//! Progress phases are the labels the import screen lists: `Reading package`, `Notes`, `Cards`,
//! `Review history`, `Media`, `Checking integrity`, then `Done`.

mod notetypes;

use crate::anki::{read_package, AnkiImage};
use crate::backup::backup::backup_now;
use crate::backup::BackupFileKind;
use crate::collection::decks::{create_deck, normalise_deck_name};
use crate::collection::full_backup::{guid_overlap, open_loci_backup, preview_loci_backup};
use crate::collection::internals::{meta, CollectionCore, IdTable, DEFAULT_DECK_ID};
use crate::collection::media::{add_media, MEDIA_PREFIX};
use crate::collection::rows::{
    card_params, deck_today_to_json, insert_sql, note_derived_columns, note_params, revlog_params, CARD_COLUMNS, NOTE_COLUMNS,
    REVLOG_COLUMNS,
};
use crate::collection::service::Collection;
use crate::collection::settings::{KEY_LEARN_AHEAD_MINUTES, KEY_ROLLOVER_HOUR};
use crate::collection::time::sched_timing;
use crate::db::unicase_compare;
use crate::error::Result;
use crate::text::{join_fields, join_tags, sha1_hex};
use crate::types::*;
use crate::undo::{self, PresetRow, RowSnapshot, Table, UndoSnapshot};
use notetypes::import_notetypes;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Above this many inserted rows the import is not recorded on the undo stack (a backup is written first instead).
pub const IMPORT_UNDO_LIMIT: usize = 20_000;

pub const PHASE_READ: &str = "Reading package";
pub const PHASE_NOTES: &str = "Notes";
pub const PHASE_CARDS: &str = "Cards";
pub const PHASE_REVLOG: &str = "Review history";
pub const PHASE_MEDIA: &str = "Media";
pub const PHASE_CHECK: &str = "Checking integrity";
pub const PHASE_DONE: &str = "Done";

fn report(progress: &mut dyn FnMut(Progress), phase: &str, done: i64, total: i64) {
    progress(Progress { phase: phase.to_string(), done, total });
}

/// Day number of the source collection at `now_ms` (Anki days_elapsed with its own crt/rollover/offsets).
pub fn source_today(image: &AnkiImage, now_ms: i64, tz: &dyn Fn(i64) -> i64) -> CollectionDay {
    let cfg = &image.config;
    let creation = cfg.creation_offset.unwrap_or_else(|| tz(image.crt * 1000));
    let current = cfg.local_offset.unwrap_or_else(|| tz(now_ms));
    sched_timing(image.crt, creation, now_ms, current, cfg.rollover).days_elapsed
}

fn has_memory(c: &Card) -> bool {
    c.data.s.is_some() && c.data.d.is_some()
}

pub fn preview_import(core: &CollectionCore, bytes: Vec<u8>) -> Result<ImportPreview> {
    let image = read_package(bytes)?;
    let mut decks: Vec<String> = image.decks.iter().filter(|d| !d.filtered).map(|d| d.name.clone()).collect();
    // `localeCompare`: case-insensitive, like the unicase collation
    decks.sort_by(|a, b| unicase_compare(a, b));
    let guids: Vec<String> = image.notes.iter().map(|n| n.guid.clone()).collect();
    let (notes_existing, notes_new) = guid_overlap(core, &guids)?;
    Ok(ImportPreview {
        layout: image.layout,
        // Packages carry no marker; a collection package is recognised by the UI from the file name.
        is_collection_package: false,
        notes: image.notes.len() as i64,
        cards: image.cards.len() as i64,
        decks,
        notetypes: image.notetypes.iter().map(|n| n.name.clone()).collect(),
        reviews: image.revlog.len() as i64,
        media: image.media.len() as i64,
        has_scheduling: !image.revlog.is_empty() || image.cards.iter().any(|c| c.ctype != CardType::New),
        fsrs_cards: image.cards.iter().filter(|c| has_memory(c)).count() as i64,
        source_crt: image.crt,
        warnings: image.warnings.clone(),
        notes_existing,
        notes_new,
    })
}

/// Anki "without scheduling": every card new (position from data.pos or appended), flags cleared, revlog dropped, leech/marked tags removed.
fn strip_scheduling(image: &mut AnkiImage) {
    let mut pos = image.config.next_pos.max(1);
    for c in &image.cards {
        if c.ctype == CardType::New && c.queue >= CardQueue::UserBuried {
            pos = pos.max(c.due + 1);
        }
    }
    for c in &mut image.cards {
        let due = if c.ctype == CardType::New {
            c.due
        } else if let Some(p) = c.data.pos {
            p
        } else {
            pos += 1;
            pos - 1
        };
        c.ctype = CardType::New;
        c.queue = CardQueue::New;
        c.due = due;
        c.ivl = 0;
        c.factor = 0;
        c.reps = 0;
        c.lapses = 0;
        c.left = 0;
        c.odue = 0;
        c.odid = 0;
        c.flags = 0;
        c.data = CardData::default();
    }
    image.revlog.clear();
    for n in &mut image.notes {
        n.tags.retain(|t| {
            let l = t.to_lowercase();
            l != "leech" && l != "marked"
        });
    }
    for d in &mut image.decks {
        d.today = DeckToday::default();
    }
}

/// JavaScript `encodeURI`: reserved and unreserved URI characters stay, everything else is percent-encoded.
fn encode_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b';' | b',' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*'
            | b'\'' | b'(' | b')' | b'#' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Rewrites references to media file `from` as `to` in field HTML (src/href/data/poster attributes and [sound:]).
pub fn replace_media_refs(html: &str, from: &str, to: &str) -> String {
    let encoded = encode_uri(from);
    if !html.contains(from) && !html.contains(&encoded) {
        return html.to_string();
    }
    let mut out = html.to_string();
    let mut variants = vec![from.to_string()];
    if encoded != from {
        variants.push(encoded);
    }
    for variant in variants {
        let v = regex::escape(&variant);
        for quote in ['"', '\''] {
            if let Ok(re) = regex::Regex::new(&format!(r#"(?i)(\b(?:src|href|data|poster)\s*=\s*){quote}{v}{quote}"#)) {
                out = re.replace_all(&out, |caps: &regex::Captures| format!("{}{quote}{to}{quote}", &caps[1])).into_owned();
            }
        }
        // unquoted value followed by whitespace or `>` (the following character is re-emitted)
        if let Ok(re) = regex::Regex::new(&format!(r"(?i)(\b(?:src|href|data|poster)\s*=\s*){v}([\s>])")) {
            out = re.replace_all(&out, |caps: &regex::Captures| format!("{}\"{to}\"{}", &caps[1], &caps[2])).into_owned();
        }
        out = out.replace(&format!("[sound:{variant}]"), &format!("[sound:{to}]"));
    }
    out
}

fn is_static_media(name: &str) -> bool {
    name.starts_with('_') || name.starts_with("latex-")
}

fn preset_row(core: &CollectionCore, id: DeckConfigId) -> Result<Option<PresetRow>> {
    core.db().get("SELECT id, name, config, mtime, usn FROM deck_configs WHERE id = ?", [id], |r| {
        Ok(PresetRow { id: r.get(0)?, name: r.get(1)?, config: r.get(2)?, mtime: r.get(3)?, usn: r.get(4)? })
    })
}

/// Imports deck-option presets, deduplicated by name; returns source id → target id.
/// Into an empty collection (`takeover`), Anki's "Default" preset replaces Loci's untouched preset
/// 1 ("Standard", which every deck falls back to) under its Anki name, so the preset the imported
/// decks use is the one Settings edits; `before` receives the replaced row for undo.
fn import_presets(
    core: &CollectionCore,
    image: &AnkiImage,
    takeover: bool,
    before: &mut Vec<PresetRow>,
) -> Result<HashMap<DeckConfigId, DeckConfigId>> {
    let mut map = HashMap::new();
    let one = preset_row(core, 1)?;
    let any_default = image
        .deck_configs
        .iter()
        .find(|p| p.id == 1 && p.name.to_lowercase() == "default")
        .or_else(|| image.deck_configs.iter().find(|p| p.name.to_lowercase() == "default"));
    if let (true, Some(one), Some(d)) = (takeover, one, any_default) {
        if one.name == "Standard" {
            before.push(one);
            let mtime = if d.mtime != 0 { d.mtime } else { core.now_seconds() };
            core.db().run(
                "UPDATE deck_configs SET name = ?, config = ?, mtime = ?, usn = -1 WHERE id = 1",
                rusqlite::params![d.name, serde_json::to_string(&d.config)?, mtime],
            )?;
            map.insert(d.id, 1);
        }
    }
    for p in &image.deck_configs {
        if map.contains_key(&p.id) {
            continue;
        }
        if let Some(same) = core.db().scalar::<i64, _>("SELECT id FROM deck_configs WHERE name = ?", [&p.name])? {
            map.insert(p.id, same);
            continue;
        }
        let id = if p.id > 0 && core.db().scalar::<i64, _>("SELECT 1 FROM deck_configs WHERE id = ?", [p.id])?.is_none() {
            p.id
        } else {
            core.new_id(IdTable::DeckConfigs)?
        };
        let mtime = if p.mtime != 0 { p.mtime } else { core.now_seconds() };
        core.db().run(
            "INSERT INTO deck_configs (id, name, config, mtime, usn) VALUES (?, ?, ?, ?, -1)",
            rusqlite::params![id, p.name, serde_json::to_string(&p.config)?, mtime],
        )?;
        map.insert(p.id, id);
    }
    Ok(map)
}

struct DeckImport {
    map: HashMap<DeckId, DeckId>,
    inserted: Vec<DeckId>,
    before: Vec<RowSnapshot>,
    added: i64,
}

fn import_decks(
    core: &CollectionCore,
    image: &AnkiImage,
    preset_map: &HashMap<DeckConfigId, DeckConfigId>,
    src_today: CollectionDay,
    today: CollectionDay,
) -> Result<DeckImport> {
    let mut out = DeckImport { map: HashMap::new(), inserted: vec![], before: vec![], added: 0 };
    let mut decks: Vec<&crate::anki::AnkiDeck> = image.decks.iter().filter(|d| !d.filtered).collect();
    decks.sort_by_key(|d| d.name.split("::").count());
    for d in decks {
        let Ok(name) = normalise_deck_name(&d.name) else {
            out.map.insert(d.id, DEFAULT_DECK_ID);
            continue;
        };
        let config_id = preset_map.get(&d.config_id).copied().unwrap_or(1);
        if let Some(existing) = core.deck_id_by_name(&name)? {
            out.map.insert(d.id, existing);
            let row = undo::snapshot_rows(core.db(), Table::Decks, &[existing])?.into_iter().next();
            let Some(row) = row else { continue };
            let row_desc = row.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let row_cfg = row.get("config_id").and_then(|v| v.as_i64()).unwrap_or(1);
            let desc = if !d.description.is_empty() && d.description != row_desc { Some(d.description.clone()) } else { None };
            let cfg = if d.config_id != 1 && config_id != row_cfg { Some(config_id) } else { None };
            if desc.is_some() || cfg.is_some() {
                if !out.inserted.contains(&existing) {
                    out.before.push(row);
                }
                core.db().run(
                    "UPDATE decks SET description = ?, config_id = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![desc.unwrap_or(row_desc), cfg.unwrap_or(row_cfg), core.now_seconds(), existing],
                )?;
            }
            continue;
        }
        let before: Vec<DeckId> = core.all_decks()?.iter().map(|x| x.id).collect();
        let created = create_deck(core, &name, Some(config_id))?;
        for x in core.all_decks()? {
            if !before.contains(&x.id) {
                out.inserted.push(x.id);
                out.added += 1;
            }
        }
        let t = if d.today.day == src_today { DeckToday { day: today, ..d.today.clone() } } else { DeckToday::default() };
        let mut extra = Map::new();
        extra.insert("extendNew".into(), Value::from(d.extend_new));
        extra.insert("extendRev".into(), Value::from(d.extend_rev));
        if let Some(o) = &d.original_json {
            extra.insert("originalAnkiJson".into(), Value::Object(o.clone()));
        }
        core.db().run(
            "UPDATE decks SET description = ?, collapsed = ?, today = ?, config = ? WHERE id = ?",
            rusqlite::params![
                d.description,
                d.collapsed as i64,
                deck_today_to_json(&t),
                Value::Object(extra).to_string(),
                created.id
            ],
        )?;
        out.map.insert(d.id, created.id);
    }
    Ok(out)
}

/// Fits a field list to a note type: pads; joins extra fields into the last one (Anki's behaviour).
fn fit_fields(fields: &[String], count: usize) -> Vec<String> {
    if fields.len() == count {
        return fields.to_vec();
    }
    if fields.len() < count {
        let mut out = fields.to_vec();
        out.resize(count, String::new());
        return out;
    }
    if count == 0 {
        return vec![];
    }
    let mut out = fields[..count - 1].to_vec();
    out.push(fields[count - 1..].join("; "));
    out
}

struct MediaImport {
    renames: Vec<(String, String)>,
    added: i64,
}

/// Stores the package's media; returns renames (package name → stored name) and the count of new files.
fn import_media(
    core: &CollectionCore,
    image: &AnkiImage,
    progress: &mut dyn FnMut(Progress),
    warnings: &mut Vec<String>,
) -> Result<MediaImport> {
    let mut renames = Vec::new();
    let mut added = 0;
    let mut done = 0;
    let names: Vec<&String> = image.media.keys().collect();
    let total = names.len() as i64;
    report(progress, PHASE_MEDIA, 0, total);
    for name in names {
        let bytes = match image.read_media(name) {
            Ok(b) => b,
            Err(e) => {
                warnings.push(format!("Media file \"{name}\" could not be read and was skipped ({}).", e.message));
                continue;
            }
        };
        if is_static_media(name) {
            let path = format!("{MEDIA_PREFIX}{name}");
            if !core.blobs.exists(&path) {
                core.blobs.put(&path, &bytes)?;
                core.db().run(
                    "INSERT INTO media (name, sha1, size, mtime) VALUES (?, ?, ?, ?) ON CONFLICT(name) DO UPDATE SET sha1 = excluded.sha1, size = excluded.size, mtime = excluded.mtime",
                    rusqlite::params![name, sha1_hex(&bytes), bytes.len() as i64, core.now_seconds()],
                )?;
                added += 1;
            }
        } else {
            let known = core.db().scalar::<String, _>("SELECT name FROM media WHERE sha1 = ? LIMIT 1", [sha1_hex(&bytes)])?;
            let stored = add_media(core, name, &bytes)?;
            if known.is_none() {
                added += 1;
            }
            if &stored != name {
                renames.push((name.clone(), stored));
            }
        }
        done += 1;
        if done % 25 == 0 || done == total {
            report(progress, PHASE_MEDIA, done, total);
        }
    }
    Ok(MediaImport { renames, added })
}

fn snapshot_str<'a>(row: &'a RowSnapshot, key: &str) -> &'a str {
    row.get(key).and_then(|v| v.as_str()).unwrap_or("")
}
fn snapshot_i64(row: &RowSnapshot, key: &str) -> i64 {
    row.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Imports a package into this collection (merge). `replace` mode is handled by the Collection
/// (backup, wipe, then this function). Throws before writing anything when the package cannot be read.
pub fn import_package(
    core: &CollectionCore,
    bytes: Vec<u8>,
    options: &ImportOptions,
    progress: &mut dyn FnMut(Progress),
    adopt_settings: bool,
) -> Result<ImportReport> {
    let started = core.now();
    report(progress, PHASE_READ, 0, 1);
    let mut image = read_package(bytes)?;
    report(progress, PHASE_READ, 1, 1);
    let mut rep = ImportReport { warnings: image.warnings.clone(), ..Default::default() };
    if !options.keep_scheduling {
        strip_scheduling(&mut image);
    }
    if adopt_settings {
        // 'replace' mode: the collection takes over the package's day rollover and learn-ahead limit.
        core.set_meta(KEY_ROLLOVER_HOUR, Some(&image.config.rollover.clamp(0, 23).to_string()))?;
        let minutes = ((image.config.collapse_time as f64) / 60.0).round().max(0.0) as i64;
        core.set_meta(KEY_LEARN_AHEAD_MINUTES, Some(&minutes.to_string()))?;
    }
    let conflict = options.conflict;

    let large = image.notes.len() + image.cards.len() + image.revlog.len() > IMPORT_UNDO_LIMIT;
    let has_data = core.db().scalar::<i64, _>("SELECT 1 FROM notes LIMIT 1", [])?.is_some();
    let source_crt_before = core.get_meta(meta::SOURCE_CRT)?;
    if large && has_data {
        backup_now(core, BackupFileKind::PreRestore)?;
    }

    let media = import_media(core, &image, progress, &mut rep.warnings)?;
    rep.media_added = media.added;

    let image = &image;
    let insert_note_sql = insert_sql("notes", &NOTE_COLUMNS);
    let insert_card_sql = insert_sql("cards", &CARD_COLUMNS);
    let insert_revlog_sql = insert_sql("revlog", &REVLOG_COLUMNS);
    core.db().transaction(|| {
        let today = core.today()?;
        let src_today = source_today(image, core.now(), &|ms| core.tz_offset(ms));
        let delta = src_today - today;
        // The first import into an empty collection remembers the source crt (export writes it back).
        if !has_data && !image.notes.is_empty() {
            let v = serde_json::json!({ "crt": image.crt, "creationOffset": image.config.creation_offset });
            core.set_meta(meta::SOURCE_CRT, Some(&v.to_string()))?;
        }
        let nt_map = import_notetypes(core, image, conflict, &mut rep)?;
        let mut presets_before: Vec<PresetRow> = Vec::new();
        let preset_map = import_presets(core, image, !has_data, &mut presets_before)?;
        let decks = import_decks(core, image, &preset_map, src_today, today)?;
        rep.decks_added = decks.added;

        // ── notes ──
        let n_notes = image.notes.len() as i64;
        report(progress, PHASE_NOTES, 0, n_notes);
        let mut note_map: HashMap<NoteId, NoteId> = HashMap::new();
        let mut inserted_notes: Vec<NoteId> = Vec::new();
        let mut notes_before: Vec<RowSnapshot> = Vec::new();
        let mut nt_cache: HashMap<NotetypeId, Notetype> = HashMap::new();
        let mut all_tags: Vec<String> = Vec::new();
        let mut conflicting = 0;
        for (i, src) in image.notes.iter().enumerate() {
            let Some(&ntid) = nt_map.get(&src.ntid) else {
                rep.notes_skipped += 1;
                continue;
            };
            if let std::collections::hash_map::Entry::Vacant(e) = nt_cache.entry(ntid) {
                e.insert(core.notetype(ntid)?);
            }
            let nt = &nt_cache[&ntid];
            let mut fields = fit_fields(&src.fields, nt.fields.len());
            if !media.renames.is_empty() {
                for f in fields.iter_mut() {
                    for (a, b) in &media.renames {
                        *f = replace_media_refs(f, a, b);
                    }
                }
            }
            let tags = core.canonical_tags(&src.tags)?;
            let existing_id: Option<i64> = core.db().scalar("SELECT id FROM notes WHERE guid = ?", [&src.guid])?;
            let ex = match existing_id {
                Some(id) => undo::snapshot_rows(core.db(), Table::Notes, &[id])?.into_iter().next(),
                None => None,
            };
            match ex {
                None => {
                    let mut id = src.id;
                    while core.db().scalar::<i64, _>("SELECT 1 FROM notes WHERE id = ?", [id])?.is_some() {
                        id += 1;
                    }
                    let note = Note { id, guid: src.guid.clone(), ntid, mtime: src.mtime, usn: -1, tags: tags.clone(), fields };
                    core.db().run(&insert_note_sql, rusqlite::params_from_iter(note_params(&note, nt.sort_field)))?;
                    inserted_notes.push(id);
                    note_map.insert(src.id, id);
                    all_tags.extend(tags);
                    rep.notes_added += 1;
                }
                Some(ex) if snapshot_i64(&ex, "ntid") != ntid => {
                    conflicting += 1;
                    rep.notes_skipped += 1;
                }
                Some(ex) => {
                    let same = snapshot_str(&ex, "flds") == join_fields(&fields) && snapshot_str(&ex, "tags") == join_tags(&tags);
                    let update = !same
                        && (conflict == ConflictPolicy::Theirs
                            || (conflict == ConflictPolicy::Newer && snapshot_i64(&ex, "mtime") < src.mtime));
                    if update {
                        let ex_id = snapshot_i64(&ex, "id");
                        let d = note_derived_columns(&fields, nt.sort_field);
                        core.db().run(
                            "UPDATE notes SET mtime = ?, usn = -1, tags = ?, flds = ?, sfld = ?, csum = ?, plain = ? WHERE id = ?",
                            rusqlite::params![src.mtime, join_tags(&tags), join_fields(&fields), d.sfld, d.csum, d.plain, ex_id],
                        )?;
                        notes_before.push(ex);
                        all_tags.extend(tags);
                        note_map.insert(src.id, ex_id);
                        rep.notes_updated += 1;
                    } else {
                        rep.notes_skipped += 1;
                    }
                }
            }
            if (i + 1) % 500 == 0 {
                report(progress, PHASE_NOTES, i as i64 + 1, n_notes);
            }
        }
        if conflicting > 0 {
            rep.warnings.push(format!(
                "{conflicting} notes were skipped because a note with the same id (guid) uses a different note type here."
            ));
        }
        core.register_tags(&all_tags)?;
        report(progress, PHASE_NOTES, n_notes, n_notes);

        // ── cards ──
        let n_cards = image.cards.len() as i64;
        report(progress, PHASE_CARDS, 0, n_cards);
        let mut card_map: HashMap<CardId, CardId> = HashMap::new();
        let mut inserted_cards: Vec<CardId> = Vec::new();
        let mut max_new_pos = 0;
        for (i, src) in image.cards.iter().enumerate() {
            let Some(&nid) = note_map.get(&src.nid) else { continue };
            if core.db().scalar::<i64, _>("SELECT 1 FROM cards WHERE nid = ? AND ord = ?", rusqlite::params![nid, src.ord])?.is_some() {
                continue;
            }
            let mut card = Card { nid, did: decks.map.get(&src.did).copied().unwrap_or(DEFAULT_DECK_ID), usn: -1, odid: 0, ..src.clone() };
            if matches!(card.queue, CardQueue::Review | CardQueue::DayLearn) || card.ctype == CardType::Review {
                card.due -= delta;
            }
            if card.ctype == CardType::Review && card.odue != 0 {
                card.odue -= delta;
            }
            if core.db().scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [card.did])?.is_none() {
                card.did = DEFAULT_DECK_ID;
            }
            let mut id = src.id;
            while core.db().scalar::<i64, _>("SELECT 1 FROM cards WHERE id = ?", [id])?.is_some() {
                id += 1;
            }
            card.id = id;
            if card.ctype == CardType::New {
                max_new_pos = max_new_pos.max(card.due);
            }
            core.db().run(&insert_card_sql, rusqlite::params_from_iter(card_params(&card)))?;
            card_map.insert(src.id, id);
            inserted_cards.push(id);
            rep.cards_added += 1;
            if (i + 1) % 1000 == 0 {
                report(progress, PHASE_CARDS, i as i64 + 1, n_cards);
            }
        }
        let next_pos = core.get_meta(meta::NEXT_POS)?.and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
        if (max_new_pos + 1) as f64 > next_pos {
            core.set_meta(meta::NEXT_POS, Some(&(max_new_pos + 1).to_string()))?;
        }
        report(progress, PHASE_CARDS, n_cards, n_cards);

        // ── revlog ──
        let n_revlog = image.revlog.len() as i64;
        report(progress, PHASE_REVLOG, 0, n_revlog);
        let mut inserted_revlog: Vec<RevlogId> = Vec::new();
        for (i, src) in image.revlog.iter().enumerate() {
            let Some(&cid) = card_map.get(&src.cid) else { continue };
            if core.db().scalar::<i64, _>("SELECT 1 FROM revlog WHERE id = ?", [src.id])?.is_some() {
                continue;
            }
            let entry = RevlogEntry { cid, usn: -1, ..src.clone() };
            core.db().run(&insert_revlog_sql, rusqlite::params_from_iter(revlog_params(&entry)))?;
            inserted_revlog.push(src.id);
            rep.reviews_added += 1;
            if (i + 1) % 5000 == 0 {
                report(progress, PHASE_REVLOG, i as i64 + 1, n_revlog);
            }
        }
        report(progress, PHASE_REVLOG, n_revlog, n_revlog);

        let rows = inserted_notes.len() + inserted_cards.len() + inserted_revlog.len() + notes_before.len();
        if rows <= IMPORT_UNDO_LIMIT {
            let mut meta_snap = Map::new();
            meta_snap.insert(meta::SOURCE_CRT.to_string(), source_crt_before.clone().map(Value::String).unwrap_or(Value::Null));
            undo::record(
                core.db(),
                core.now(),
                "Import",
                &UndoSnapshot {
                    inserted_notes,
                    inserted_cards,
                    inserted_revlog,
                    inserted_decks: decks.inserted.clone(),
                    notes: notes_before,
                    decks: decks.before.clone(),
                    meta: meta_snap,
                    presets: presets_before,
                    ..Default::default()
                },
            )?;
        } else {
            undo::clear(core.db())?;
        }
        core.mark_changed()
    })?;
    report(progress, PHASE_CHECK, 0, 1);
    core.invalidate_caches();
    report(progress, PHASE_CHECK, 1, 1);
    rep.duration_ms = core.now() - started;
    report(progress, PHASE_DONE, 1, 1);
    Ok(rep)
}

impl Collection {
    /// Previews an Anki package or a Loci full backup without writing anything.
    pub fn preview_import(&self, bytes: Vec<u8>) -> Result<ImportPreview> {
        if let Some(loci) = open_loci_backup(&bytes)? {
            return preview_loci_backup(&self.core, &loci);
        }
        preview_import(&self.core, bytes)
    }

    /// Imports a package. 'merge' adds/updates by guid. 'replace' first checks that the package is
    /// readable, writes a pre-restore backup, wipes the collection, then imports everything
    /// (incoming versions win) and adopts the package's rollover hour and learn-ahead limit.
    /// A Loci full backup is restored instead (replace mode only).
    pub fn import_package(
        &mut self,
        bytes: Vec<u8>,
        options: &ImportOptions,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<ImportReport> {
        self.session = None;
        if let Some(loci) = open_loci_backup(&bytes)? {
            return self.restore_full_backup(&loci, options, progress);
        }
        if options.mode == ImportMode::Replace {
            preview_import(&self.core, bytes.clone())?;
            self.reset_collection()?;
            let opts = ImportOptions { conflict: ConflictPolicy::Theirs, ..options.clone() };
            return import_package(&self.core, bytes, &opts, progress, true);
        }
        import_package(&self.core, bytes, options, progress, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_references_are_rewritten_in_every_attribute_form() {
        let html = r#"<img src="a b.png"><img src='a b.png'><img src=a%20b.png> [sound:a b.png] <a href="other.png">x</a>"#;
        let out = replace_media_refs(html, "a b.png", "c.png");
        assert_eq!(out, r#"<img src="c.png"><img src='c.png'><img src="c.png"> [sound:c.png] <a href="other.png">x</a>"#);
        assert_eq!(replace_media_refs("<p>no media</p>", "x.png", "y.png"), "<p>no media</p>");
    }

    #[test]
    fn fields_are_padded_or_joined_to_the_note_type() {
        let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(fit_fields(&f(&["a"]), 3), f(&["a", "", ""]));
        assert_eq!(fit_fields(&f(&["a", "b", "c", "d"]), 2), f(&["a", "b; c; d"]));
        assert_eq!(fit_fields(&f(&["a", "b"]), 2), f(&["a", "b"]));
    }

    #[test]
    fn encode_uri_keeps_reserved_characters() {
        assert_eq!(encode_uri("a b/c?d=é"), "a%20b/c?d=%C3%A9");
    }
}
