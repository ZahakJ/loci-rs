//! `read_package`: .apkg / .colpkg bytes → `AnkiImage` (research/anki-format.md §1, §2, §7).
//! Port of recall `src/core/anki/read.ts`.
//!
//! Steps: unzip → `meta` (layout) → collection file → zstd (LATEST) → WAL-header fix → open (our
//! `Db` registers Anki's `unicase` collation, so the TypeScript `stripUnicaseCollation` step is not
//! needed) → schema 11 JSON or schema 14–18 tables + protobuf blobs → Loci types → v1-scheduler
//! upgrade → filtered decks emptied → media index.

use super::deck_config::{dconf_to_deck_config, deck_config_proto_to_json, int, obj};
use super::image::*;
use super::media_names::{filename_is_safe, normalize_media_filename};
use super::proto::*;
use super::zip::ZipArchive;
use crate::collection::rows::{card_queue_from_i64, card_type_from_i64, revlog_type_from_i64};
use crate::db::{is_sqlite_image, Db};
use crate::error::{ErrorCode, LociError, Result};
use crate::text::{sha1_hex, split_fields};
use crate::types::*;
use mapping::{deck_from_json, notetype_from_json};
use repair::{repair_references, return_filtered_cards, upgrade_v1_scheduler};
use rusqlite::types::ValueRef;
use rusqlite::Row;
use schema18::{decks_from_tables, notetypes_from_tables};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::sync::Arc;

mod mapping;
mod repair;
mod schema18;

fn collection_file(layout: PackageLayout) -> &'static str {
    match layout {
        PackageLayout::Legacy1 => "collection.anki2",
        PackageLayout::Legacy2 => "collection.anki21",
        PackageLayout::Latest => "collection.anki21b",
    }
}

// ───────────────────────── small coercions ─────────────────────────

/// read.ts:49 `s`: string, or a number printed like JavaScript, or bytes as UTF-8; else the default.
pub(super) fn js(v: Option<&Value>, d: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            n.as_f64().map(super::deck_config::json_num).map(|x| x.to_string()).unwrap_or_else(|| d.to_string())
        }
        _ => d.to_string(),
    }
}
/// read.ts:55 `b`: boolean; number ≠ 0; string other than '', '0', 'false'.
pub(super) fn jb(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !(s.is_empty() || s == "0" || s == "false"),
        _ => false,
    }
}
fn parse_json(text: &str, what: &str) -> Result<Value> {
    serde_json::from_str::<Value>(text).map_err(|_| LociError::corrupt(format!("The package's {what} is not valid JSON.")))
}
fn obj_values(v: Value) -> Vec<Json> {
    match v {
        Value::Object(m) => m.into_iter().map(|(_, v)| obj(Some(&v))).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn col_i64(r: &Row<'_>, i: usize) -> i64 {
    match r.get_ref(i) {
        Ok(ValueRef::Integer(v)) => v,
        Ok(ValueRef::Real(f)) if f.is_finite() => f as i64,
        Ok(ValueRef::Text(t)) => {
            std::str::from_utf8(t).ok().and_then(|s| s.trim().parse::<f64>().ok()).map(|f| f as i64).unwrap_or(0)
        }
        _ => 0,
    }
}
pub(super) fn col_str(r: &Row<'_>, i: usize) -> String {
    match r.get_ref(i) {
        Ok(ValueRef::Text(t)) | Ok(ValueRef::Blob(t)) => String::from_utf8_lossy(t).into_owned(),
        Ok(ValueRef::Integer(v)) => v.to_string(),
        Ok(ValueRef::Real(f)) => super::deck_config::json_num(f).to_string(),
        _ => String::new(),
    }
}
pub(super) fn col_bytes(r: &Row<'_>, i: usize) -> Vec<u8> {
    match r.get_ref(i) {
        Ok(ValueRef::Blob(b)) | Ok(ValueRef::Text(b)) => b.to_vec(),
        _ => Vec::new(),
    }
}

/// Any database failure while reading means the collection file is damaged (read.ts:121).
pub(super) fn dbe(e: LociError) -> LociError {
    match e.code {
        ErrorCode::Corrupt | ErrorCode::UnsupportedPackage => e,
        _ => LociError::corrupt(format!("The collection database is damaged: {}", e.message)),
    }
}

// ───────────────────────── entry point ─────────────────────────

/// Reads an Anki package. Fails with `unsupported-package` (not an Anki package, or made by a
/// newer Anki) or `corrupt` (damaged zip, missing/unreadable collection, inconsistent data).
pub fn read_package(bytes: Vec<u8>) -> Result<AnkiImage> {
    let zip = match ZipArchive::open(bytes) {
        Ok(z) => Arc::new(z),
        Err(e) if e.truncated => {
            return Err(
                LociError::corrupt("The package is damaged (the zip archive is truncated or unreadable).").with_detail(e.message)
            )
        }
        Err(e) => {
            return Err(
                LociError::unsupported_package("This file is not an Anki package (.apkg or .colpkg).").with_detail(e.message)
            )
        }
    };
    let read_entry = |name: &str| -> Result<Vec<u8>> {
        zip.read(name)
            .map_err(|e| LociError::corrupt(format!("The package is damaged: cannot read \"{name}\".")).with_detail(e.message))
    };

    let layout = detect_layout(&zip, &read_entry)?;
    let col_name = collection_file(layout);
    if !zip.has(col_name) {
        return Err(LociError::corrupt(format!("The package has no collection file ({col_name}).")));
    }
    let mut col_bytes = read_entry(col_name)?;
    if layout == PackageLayout::Latest {
        col_bytes = zstd(&col_bytes, col_name)?;
    }
    if !is_sqlite_image(&col_bytes) {
        return Err(LociError::corrupt(format!("The collection file {col_name} is not an SQLite database.")));
    }
    // Db::open_from_bytes clears the WAL header flag and registers `unicase`.
    let db = Db::open_from_bytes(&col_bytes)
        .map_err(|e| LociError::corrupt("The collection database cannot be opened.").with_detail(e.message))?;
    drop(col_bytes);
    let mut warnings = Vec::new();
    let mut image = read_collection(&db, layout, &mut warnings)?;
    db.close().ok();

    let media = read_media_map(&zip, layout, &read_entry, &mut warnings)?;
    image.media = media.clone();
    image.warnings = warnings;
    image.media_reader = Box::new(move |name: &str| {
        let Some(entry) = media.get(name) else {
            return Err(LociError::not_found(format!("No media file named \"{name}\" in the package.")));
        };
        let mut body = zip.read(&entry.zip_entry).map_err(|e| {
            LociError::corrupt(format!("The package is damaged: cannot read \"{}\".", entry.zip_entry)).with_detail(e.message)
        })?;
        if layout == PackageLayout::Latest {
            body = zstd(&body, name)?;
        }
        if let Some(sha1) = &entry.sha1 {
            if sha1_hex(&body) != *sha1 {
                return Err(LociError::corrupt(format!("Media file \"{name}\" is damaged (checksum mismatch).")));
            }
        }
        Ok(body)
    });
    Ok(image)
}

/// Decompresses a zstd stream (all frames) into memory.
pub(crate) fn zstd(bytes: &[u8], what: &str) -> Result<Vec<u8>> {
    let fail = |e: String| LociError::corrupt(format!("The package is damaged: cannot decompress \"{what}\".")).with_detail(e);
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| fail(e.to_string()))?;
    Ok(out)
}

/// §1.1: `meta` decides; without it, collection.anki21 ⇒ legacy2, else legacy1.
fn detect_layout(zip: &ZipArchive, read_entry: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<PackageLayout> {
    if zip.has("meta") {
        let meta = read_entry("meta")?;
        let version = decode_package_metadata(&meta)
            .map_err(|e| LociError::corrupt("The package metadata is damaged.").with_detail(e.0))?
            .version;
        return match version {
            1 => Ok(PackageLayout::Legacy1),
            2 => Ok(PackageLayout::Legacy2),
            3 => Ok(PackageLayout::Latest),
            v => Err(LociError::unsupported_package("This package was made by a newer version of Anki than Loci understands.")
                .with_detail(format!("{{\"version\":{v}}}"))),
        };
    }
    if zip.has("collection.anki21") {
        return Ok(PackageLayout::Legacy2);
    }
    if zip.has("collection.anki2") {
        return Ok(PackageLayout::Legacy1);
    }
    if zip.has("collection.anki21b") {
        return Err(LociError::corrupt("The package has a collection.anki21b file but no metadata."));
    }
    Err(LociError::corrupt("The package has no collection file."))
}

// ───────────────────────── collection ─────────────────────────

fn has_table(db: &Db, name: &str) -> Result<bool> {
    Ok(db.scalar::<i64, _>("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", [name]).map_err(dbe)?.is_some())
}

struct ColRow {
    crt: i64,
    mod_ms: i64,
    ver: i64,
    conf: String,
    models: String,
    decks: String,
    dconf: String,
}

fn read_collection(db: &Db, layout: PackageLayout, warnings: &mut Vec<String>) -> Result<AnkiImage> {
    let col = db
        .get("SELECT crt, mod, ver, conf, models, decks, dconf FROM col LIMIT 1", [], |r| {
            Ok(ColRow {
                crt: col_i64(r, 0),
                mod_ms: col_i64(r, 1),
                ver: col_i64(r, 2),
                conf: col_str(r, 3),
                models: col_str(r, 4),
                decks: col_str(r, 5),
                dconf: col_str(r, 6),
            })
        })
        .map_err(dbe)?
        .ok_or_else(|| LociError::corrupt("The collection has no `col` row."))?;
    let ver = col.ver;
    if !(11..=18).contains(&ver) || ver == 12 || ver == 13 {
        return Err(LociError::unsupported_package(format!("Unsupported Anki collection schema version {ver}.")));
    }
    let crt = col.crt;

    // config
    let raw_conf: Json = if has_table(db, "config")? {
        let mut raw = Json::new();
        let rows = db.all("SELECT KEY AS k, val AS v FROM config", [], |r| Ok((col_str(r, 0), col_bytes(r, 1)))).map_err(dbe)?;
        for (k, v) in rows {
            match serde_json::from_slice::<Value>(&v) {
                Ok(val) => {
                    raw.insert(k, val);
                }
                Err(_) => warnings.push(format!("Ignored unreadable collection setting \"{k}\".")),
            }
        }
        raw
    } else {
        let text = if col.conf.is_empty() { "{}" } else { col.conf.as_str() };
        obj(Some(&parse_json(text, "collection settings")?))
    };
    let config = collection_config(raw_conf);

    // note types
    let nt_json: Vec<Json> = if has_table(db, "notetypes")? {
        notetypes_from_tables(db)?
    } else {
        let text = if col.models.is_empty() { "{}" } else { col.models.as_str() };
        obj_values(parse_json(text, "note types")?)
    };
    let mut notetypes = Vec::new();
    let mut notetype_originals = HashMap::new();
    for j in nt_json {
        let nt = notetype_from_json(&j, warnings)?;
        notetype_originals.insert(nt.id, j);
        notetypes.push(nt);
    }

    // deck options
    let dconf_json: Vec<Json> = if has_table(db, "deck_config")? {
        let rows = db
            .all("SELECT id, name, mtime_secs, usn, config FROM deck_config", [], |r| {
                Ok((col_i64(r, 0), col_str(r, 1), col_i64(r, 2), col_i64(r, 3), col_bytes(r, 4)))
            })
            .map_err(dbe)?;
        let mut out = Vec::new();
        for (id, name, mtime, usn, config) in rows {
            out.push(deck_config_proto_to_json(id, &name, mtime, usn, &decode_deck_config_config(&config)?));
        }
        out
    } else {
        let text = if col.dconf.is_empty() { "{}" } else { col.dconf.as_str() };
        obj_values(parse_json(text, "deck options")?)
    };
    let deck_configs: Vec<AnkiDeckConfig> = dconf_json
        .into_iter()
        .map(|j| AnkiDeckConfig {
            id: int(j.get("id"), 0),
            name: js(j.get("name"), "Default"),
            config: dconf_to_deck_config(&j, config.fsrs, config.est_times, warnings),
            mtime: int(j.get("mod"), 0),
            usn: int(j.get("usn"), 0),
            original_json: Some(j),
        })
        .collect();

    // decks
    let deck_json: Vec<Json> = if has_table(db, "decks")? {
        decks_from_tables(db)?
    } else {
        let text = if col.decks.is_empty() { "{}" } else { col.decks.as_str() };
        obj_values(parse_json(text, "decks")?)
    };
    let decks: Vec<AnkiDeck> = deck_json.into_iter().map(deck_from_json).collect();

    // notes, cards, revlog
    let notes: Vec<Note> = db
        .all("SELECT id, guid, mid, mod, usn, tags, flds FROM notes ORDER BY id", [], |r| {
            Ok(Note {
                id: col_i64(r, 0),
                guid: col_str(r, 1),
                ntid: col_i64(r, 2),
                mtime: col_i64(r, 3),
                usn: col_i64(r, 4),
                // read.ts:226 splits on ASCII and ideographic (U+3000) spaces
                tags: col_str(r, 5).split([' ', '\u{3000}']).filter(|t| !t.is_empty()).map(str::to_string).collect(),
                fields: split_fields(&col_str(r, 6)),
            })
        })
        .map_err(dbe)?;
    let cards: Vec<Card> = db
        .all(
            "SELECT id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data FROM cards ORDER BY id",
            [],
            |r| Ok(card_from_row(r)),
        )
        .map_err(dbe)?;
    let revlog: Vec<RevlogEntry> = db
        .all("SELECT id, cid, usn, ease, ivl, lastIvl, factor, time, type FROM revlog ORDER BY id", [], |r| {
            Ok(RevlogEntry {
                id: col_i64(r, 0),
                cid: col_i64(r, 1),
                usn: col_i64(r, 2),
                ease: col_i64(r, 3),
                ivl: col_i64(r, 4),
                last_ivl: col_i64(r, 5),
                factor: col_i64(r, 6),
                time: col_i64(r, 7),
                rtype: revlog_type_from_i64(col_i64(r, 8)),
            })
        })
        .map_err(dbe)?;

    let mut image = AnkiImage {
        layout,
        crt,
        mod_ms: col.mod_ms,
        schema_version: ver,
        config,
        notetypes,
        notetype_originals,
        decks,
        deck_configs,
        notes,
        cards,
        revlog,
        media: BTreeMap::new(),
        media_reader: AnkiImage::no_media_reader(),
        warnings: Vec::new(),
    };
    if image.config.sched_ver < 2 {
        upgrade_v1_scheduler(&mut image, warnings);
    }
    repair_references(&mut image, warnings)?;
    return_filtered_cards(&mut image, warnings);
    Ok(image)
}

fn collection_config(raw: Json) -> AnkiCollectionConfig {
    let opt_num = |v: Option<&Value>| -> Option<i64> {
        match v {
            Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()).map(|x| x as i64),
            _ => None,
        }
    };
    AnkiCollectionConfig {
        sched_ver: int(raw.get("schedVer"), 1),
        sched2021: jb(raw.get("sched2021")),
        rollover: int(raw.get("rollover"), 4),
        creation_offset: opt_num(raw.get("creationOffset")),
        local_offset: opt_num(raw.get("localOffset")),
        next_pos: int(raw.get("nextPos"), 1),
        fsrs: jb(raw.get("fsrs")),
        collapse_time: int(raw.get("collapseTime"), 1200),
        est_times: match raw.get("estTimes") {
            None => true,
            v => jb(v),
        },
        cur_deck: int(raw.get("curDeck"), 1),
        last_unburied: opt_num(raw.get("lastUnburied")),
        raw,
    }
}

// ── cards ──

fn card_from_row(r: &Row<'_>) -> Card {
    Card {
        id: col_i64(r, 0),
        nid: col_i64(r, 1),
        did: col_i64(r, 2),
        ord: col_i64(r, 3),
        mtime: col_i64(r, 4),
        usn: col_i64(r, 5),
        ctype: card_type_from_i64(col_i64(r, 6)),
        queue: card_queue_from_i64(col_i64(r, 7)),
        due: col_i64(r, 8),
        ivl: col_i64(r, 9),
        factor: col_i64(r, 10),
        reps: col_i64(r, 11),
        lapses: col_i64(r, 12),
        left: col_i64(r, 13),
        odue: col_i64(r, 14),
        odid: col_i64(r, 15),
        flags: col_i64(r, 16),
        data: parse_card_data(&col_str(r, 17)),
    }
}

/// Anki `CardData` (rslib/src/storage/card/data.rs): invalid JSON → empty; invalid values dropped.
/// Unknown keys are dropped too (read.ts:431), unlike `collection::rows::parse_card_data`.
pub fn parse_card_data(text: &str) -> CardData {
    if text.is_empty() || text == "{}" {
        return CardData::default();
    }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(text) else { return CardData::default() };
    let fnum = |k: &str| o.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let mut out = CardData { s: fnum("s"), d: fnum("d"), dr: fnum("dr"), decay: fnum("decay"), ..CardData::default() };
    out.pos = fnum("pos").map(|x| x as i64);
    out.lrt = fnum("lrt").map(|x| x as i64);
    out.cd = match o.get("cd") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(v @ Value::Object(_)) | Some(v @ Value::Array(_)) => Some(v.to_string()),
        _ => None,
    };
    out
}

// ───────────────────────── media ─────────────────────────

fn read_media_map(
    zip: &ZipArchive,
    layout: PackageLayout,
    read_entry: &dyn Fn(&str) -> Result<Vec<u8>>,
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<String, AnkiMediaEntry>> {
    let mut media = BTreeMap::new();
    if !zip.has("media") {
        return Ok(media); // tolerated by Anki (older AnkiDroid colpkgs)
    }
    let raw = read_entry("media")?;
    let (mut missing, mut duplicates) = (0, 0);
    let mut add = |name: String, entry: AnkiMediaEntry| {
        if !zip.has(&entry.zip_entry) {
            missing += 1;
            return;
        }
        match media.entry(name) {
            std::collections::btree_map::Entry::Occupied(_) => duplicates += 1,
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(entry);
            }
        }
    };
    if layout == PackageLayout::Latest {
        let entries = decode_media_entries(&zstd(&raw, "media")?)
            .map_err(|e| LociError::corrupt("The package media list is damaged.").with_detail(e.0))?;
        for (i, e) in entries.into_iter().enumerate() {
            // LATEST entries must already be safe and normalised, else Anki rejects the package as corrupt.
            if !filename_is_safe(&e.name) || normalize_media_filename(&e.name) != e.name {
                return Err(LociError::corrupt(format!("The package contains an invalid media filename: \"{}\".", e.name)));
            }
            let zip_entry = e.legacy_zip_filename.map(|n| n.to_string()).unwrap_or_else(|| i.to_string());
            let sha1 = if e.sha1.len() == 20 { Some(hex::encode(&e.sha1)) } else { None };
            add(e.name, AnkiMediaEntry { zip_entry, size: e.size, sha1 });
        }
    } else {
        let text = String::from_utf8_lossy(&raw);
        let text = text.trim();
        let map = obj(Some(&parse_json(if text.is_empty() { "{}" } else { text }, "media list")?));
        for (zip_name, value) in map {
            let Value::String(value) = value else { return Err(LociError::corrupt("The package media list is damaged.")) };
            let name = normalize_media_filename(&value);
            if !filename_is_safe(&name) {
                return Err(LociError::corrupt(format!("The package contains an invalid media filename: \"{value}\".")));
            }
            let size = zip.info(&zip_name).map(|i| i.uncompressed_size).unwrap_or(0);
            add(name, AnkiMediaEntry { zip_entry: zip_name, size, sha1: None });
        }
    }
    if missing > 0 {
        warnings.push(format!("{missing} media file(s) listed in the package are missing from it."));
    }
    if duplicates > 0 {
        warnings.push(format!("{duplicates} media file(s) had duplicate names after normalisation and were skipped."));
    }
    Ok(media)
}
