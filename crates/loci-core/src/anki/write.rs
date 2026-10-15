//! `write_legacy_package`: `AnkiImage` → LEGACY_2 .apkg bytes that Anki 2.1.x–26.x and AnkiDroid
//! import (research/anki-format.md §9): schema-11 `collection.anki21` (deflate), the dummy
//! `collection.anki2`, `meta` = 08 02, a JSON `media` map and numbered media files.

use super::deck_config::{dconf_to_deck_config, deck_config_to_dconf, json_num, obj};
use super::image::*;
use super::schema11::SCHEMA11_DDL;
use super::zip::{create_zip, ZipData, ZipInput};
use crate::db::Db;
use crate::error::{LociError, Result};
use crate::text::{field_checksum, join_fields, join_tags, strip_html_preserving_media};
use crate::types::*;
use regex::Regex;
use rusqlite::types::Value as SqlValue;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteLegacyOptions {
    /// Keep review state, revlog and deck counters; false resets every card to new (Anki "without scheduling").
    pub include_scheduling: bool,
    /// Include media bodies (read through `image.media_reader`).
    pub include_media: bool,
    /// "Now" in ms for col.mod/scm (default: the clock); fixed by tests for reproducible bytes.
    pub now: Option<i64>,
}

/// `meta` for LEGACY_2: PackageMetadata { version: VERSION_LEGACY_2 }.
pub const LEGACY2_META: [u8; 2] = [0x08, 0x02];

const DEFAULT_CSS: &str = ".card {\n    font-family: arial;\n    font-size: 20px;\n    line-height: 1.5;\n    text-align: center;\n    color: black;\n    background-color: white;\n}\n";

fn builtin_stock_kind(b: BuiltinKind) -> i64 {
    match b {
        BuiltinKind::Basic => 1,
        BuiltinKind::BasicReversed => 2,
        BuiltinKind::BasicTyped => 4,
        BuiltinKind::Cloze => 5,
        BuiltinKind::ImageOcclusion => 6,
    }
}

fn set(j: &mut Json, key: &str, v: impl Into<Value>) {
    j.insert(key.to_string(), v.into());
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Writes a LEGACY_2 package.
pub fn write_legacy_package(image: &AnkiImage, opts: WriteLegacyOptions) -> Result<Vec<u8>> {
    let now = opts.now.unwrap_or_else(now_ms);
    let collection = build_schema11_collection(image, opts, now)?;
    let dummy = build_dummy_collection(now)?;

    let names: Vec<String> = if opts.include_media { image.media.keys().cloned().collect() } else { Vec::new() };
    let mut media_map = Json::new();
    for (i, name) in names.iter().enumerate() {
        set(&mut media_map, &i.to_string(), name.as_str());
    }

    // The lazy entries borrow the image's reader; `create_zip` resolves them in order.
    let reader: &MediaReader = &image.media_reader;
    let mut entries: Vec<ZipInput<'_>> = vec![
        ZipInput::new("meta", LEGACY2_META.to_vec(), false),
        ZipInput::new("collection.anki21", collection, true),
        ZipInput::new("collection.anki2", dummy, false),
    ];
    for (i, name) in names.iter().enumerate() {
        entries.push(ZipInput { name: i.to_string(), data: ZipData::Lazy(Box::new(move || reader(name))), compress: false });
    }
    entries.push(ZipInput::new("media", serde_json::to_string(&Value::Object(media_map))?.into_bytes(), false));
    create_zip(entries)
}

/// Builds the schema-11 collection database bytes (`collection.anki21`).
pub fn build_schema11_collection(image: &AnkiImage, opts: WriteLegacyOptions, now: i64) -> Result<Vec<u8>> {
    let with_sched = opts.include_scheduling;
    let note_ids: HashSet<i64> = image.notes.iter().map(|n| n.id).collect();
    let nt_by_id: HashMap<i64, &Notetype> = image.notetypes.iter().map(|nt| (nt.id, nt)).collect();
    for note in &image.notes {
        if !nt_by_id.contains_key(&note.ntid) {
            return Err(LociError::invalid(format!("note {} refers to missing note type {}", note.id, note.ntid)));
        }
    }
    let mut cards: Vec<Card> = image.cards.iter().filter(|c| note_ids.contains(&c.nid)).cloned().collect();
    let decks = complete_decks(&image.decks, &mut cards, with_sched);
    let deck_ids: HashSet<i64> = decks.iter().map(|d| d.id).collect();
    for c in &mut cards {
        if !deck_ids.contains(&c.did) {
            c.did = 1;
        }
        if c.odid != 0 && !deck_ids.contains(&c.odid) {
            c.odid = 0;
            c.odue = 0;
        }
    }
    let mut next_pos = image.config.next_pos.max(1);
    if with_sched {
        for c in &cards {
            if c.ctype == CardType::New && c.queue >= CardQueue::UserBuried {
                next_pos = next_pos.max(c.due + 1);
            }
        }
    } else {
        next_pos = reset_to_new(&mut cards, next_pos);
    }
    let card_ids: HashSet<i64> = cards.iter().map(|c| c.id).collect();
    let revlog: Vec<&RevlogEntry> =
        if with_sched { image.revlog.iter().filter(|r| card_ids.contains(&r.cid)).collect() } else { Vec::new() };

    let db = Db::memory()?;
    db.execute_batch(SCHEMA11_DDL)?;
    db.transaction(|| {
        insert_col(&db, image, &decks, with_sched, next_pos, now)?;
        insert_notes(&db, image, &nt_by_id, with_sched)?;
        insert_cards(&db, &cards)?;
        insert_revlog(&db, &revlog)
    })?;
    let bytes = db.serialize()?;
    db.close()?;
    Ok(bytes)
}

// ───────────────────────── col ─────────────────────────

fn insert_col(db: &Db, image: &AnkiImage, decks: &[AnkiDeck], with_sched: bool, next_pos: i64, now: i64) -> Result<()> {
    let cfg = &image.config;
    let deck_ids: HashSet<i64> = decks.iter().map(|d| d.id).collect();
    let cur_deck = if deck_ids.contains(&cfg.cur_deck) { cfg.cur_deck } else { 1 };
    let mut conf = Json::new();
    set(&mut conf, "activeDecks", Value::Array(vec![1.into()]));
    set(&mut conf, "curDeck", 1);
    set(&mut conf, "newSpread", 0);
    set(&mut conf, "collapseTime", 1200);
    set(&mut conf, "timeLim", 0);
    set(&mut conf, "estTimes", true);
    set(&mut conf, "dueCounts", true);
    set(&mut conf, "curModel", image.notetypes.first().map(|nt| Value::from(nt.id)).unwrap_or(Value::Null));
    set(&mut conf, "nextPos", 1);
    set(&mut conf, "sortType", "noteFld");
    set(&mut conf, "sortBackwards", false);
    set(&mut conf, "addToCur", true);
    set(&mut conf, "dayLearnFirst", false);
    conf.extend(cfg.raw.clone());
    set(&mut conf, "schedVer", 2);
    set(&mut conf, "sched2021", true);
    set(&mut conf, "rollover", cfg.rollover);
    set(&mut conf, "nextPos", next_pos);
    set(&mut conf, "collapseTime", cfg.collapse_time);
    set(&mut conf, "estTimes", cfg.est_times);
    set(&mut conf, "fsrs", cfg.fsrs);
    set(&mut conf, "curDeck", cur_deck);
    set(&mut conf, "activeDecks", Value::Array(vec![cur_deck.into()]));
    match cfg.creation_offset {
        Some(v) => set(&mut conf, "creationOffset", v),
        None => {
            conf.remove("creationOffset");
        }
    }
    if let Some(v) = cfg.local_offset {
        set(&mut conf, "localOffset", v);
    }
    if !with_sched {
        conf.remove("lastUnburied");
    }

    let mut models = Json::new();
    for nt in &image.notetypes {
        set(
            &mut models,
            &nt.id.to_string(),
            Value::Object(notetype_to_json(nt, image.notetype_originals.get(&nt.id), &deck_ids)),
        );
    }

    let mut deck_json = Json::new();
    for d in decks {
        set(&mut deck_json, &d.id.to_string(), Value::Object(deck_to_json(d, with_sched)));
    }

    let mut used_config_ids: HashSet<i64> = decks.iter().filter(|d| !d.filtered).map(|d| d.config_id).collect();
    used_config_ids.insert(1);
    let mut dconf = Json::new();
    for p in &image.deck_configs {
        if !used_config_ids.contains(&p.id) && p.id != 1 {
            continue;
        }
        set(
            &mut dconf,
            &p.id.to_string(),
            Value::Object(deck_config_to_dconf(p.id, &p.name, p.mtime, p.usn, &p.config, p.original_json.as_ref())),
        );
    }
    if !dconf.contains_key("1") {
        let mut ignored = Vec::new();
        let cfg1 = dconf_to_deck_config(&Json::new(), cfg.fsrs, cfg.est_times, &mut ignored);
        set(&mut dconf, "1", Value::Object(deck_config_to_dconf(1, "Default", 0, 0, &cfg1, None)));
    }
    for id in &used_config_ids {
        if !dconf.contains_key(&id.to_string()) {
            return Err(LociError::invalid(format!("deck options preset {id} is used by a deck but missing from the image")));
        }
    }

    let mut tags = Json::new();
    for note in &image.notes {
        for t in note_tags(&note.tags, with_sched) {
            set(&mut tags, &t, 0);
        }
    }

    db.run(
        "INSERT INTO col (id, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf, tags) VALUES (1, ?, ?, ?, 11, 0, 0, 0, ?, ?, ?, ?, ?)",
        rusqlite::params![
            image.crt,
            now,
            now,
            serde_json::to_string(&Value::Object(conf))?,
            serde_json::to_string(&Value::Object(models))?,
            serde_json::to_string(&Value::Object(deck_json))?,
            serde_json::to_string(&Value::Object(dconf))?,
            serde_json::to_string(&Value::Object(tags))?,
        ],
    )?;
    Ok(())
}

fn json_arr(j: &Json, key: &str) -> Vec<Json> {
    match j.get(key) {
        Some(Value::Array(a)) => a.iter().map(|x| obj(Some(x))).collect(),
        _ => Vec::new(),
    }
}

fn find_original(list: &[Json], name: &str, ord: i64) -> Json {
    list.iter()
        .find(|x| x.get("name").and_then(Value::as_str) == Some(name))
        .or_else(|| list.iter().find(|x| x.get("ord").and_then(Value::as_f64) == Some(ord as f64)))
        .cloned()
        .unwrap_or_default()
}

/// Schema-11 note type JSON (research/anki-format.md §4.1), merging keys Loci does not model from `original`.
pub fn notetype_to_json(nt: &Notetype, original: Option<&Json>, deck_ids: &HashSet<i64>) -> Json {
    let o = original.cloned().unwrap_or_default();
    let o_fields = json_arr(&o, "flds");
    let o_tmpls = json_arr(&o, "tmpls");
    let stock = nt.config.original_stock_kind.or_else(|| nt.config.builtin.map(builtin_stock_kind));
    let is_io = stock == Some(6);
    let flds: Vec<Value> = nt
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let of = find_original(&o_fields, &f.name, f.ord);
            let mut out = Json::new();
            set(&mut out, "media", Value::Array(Vec::new()));
            out.extend(of.clone());
            set(&mut out, "name", f.name.as_str());
            set(&mut out, "ord", i as i64);
            set(&mut out, "sticky", f.config.sticky);
            set(&mut out, "rtl", f.config.rtl);
            set(&mut out, "font", if f.config.font.is_empty() { "Arial" } else { f.config.font.as_str() });
            set(&mut out, "size", if f.config.size == 0 { 20 } else { f.config.size });
            set(&mut out, "description", f.config.description.as_str());
            set(&mut out, "plainText", f.config.plain_text);
            set(&mut out, "collapsed", f.config.collapsed);
            set(&mut out, "excludeFromSearch", of.get("excludeFromSearch").and_then(Value::as_bool).unwrap_or(false));
            if is_io && i < 5 {
                if !out.contains_key("tag") {
                    set(&mut out, "tag", i as i64);
                }
                if !out.contains_key("preventDeletion") {
                    set(&mut out, "preventDeletion", i < 4);
                }
            }
            Value::Object(out)
        })
        .collect();
    let tmpls: Vec<Value> = nt
        .templates
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let ot = find_original(&o_tmpls, &t.name, t.ord);
            let mut out = ot.clone();
            set(&mut out, "name", t.name.as_str());
            set(&mut out, "ord", i as i64);
            set(&mut out, "qfmt", t.qfmt.as_str());
            set(&mut out, "afmt", t.afmt.as_str());
            set(&mut out, "bqfmt", t.bqfmt.as_str());
            set(&mut out, "bafmt", t.bafmt.as_str());
            let did = match t.config.deck_override {
                Some(d) if deck_ids.contains(&d) => Value::from(d),
                _ => Value::Null,
            };
            set(&mut out, "did", did);
            set(&mut out, "bfont", ot.get("bfont").and_then(Value::as_str).unwrap_or(""));
            set(&mut out, "bsize", ot.get("bsize").and_then(Value::as_f64).map(json_num).unwrap_or(Value::from(0)));
            Value::Object(out)
        })
        .collect();
    let mut json = o.clone();
    set(&mut json, "id", nt.id);
    set(&mut json, "name", nt.name.as_str());
    set(&mut json, "type", nt.kind as i64);
    set(&mut json, "mod", nt.mtime);
    set(&mut json, "usn", nt.usn);
    set(&mut json, "sortf", nt.sort_field);
    let did = match o.get("did").and_then(Value::as_i64) {
        Some(d) if deck_ids.contains(&d) => d,
        _ => 1,
    };
    set(&mut json, "did", did);
    set(&mut json, "tmpls", Value::Array(tmpls));
    set(&mut json, "flds", Value::Array(flds));
    set(&mut json, "css", if nt.css.is_empty() { DEFAULT_CSS } else { nt.css.as_str() });
    set(&mut json, "latexPre", nt.latex_pre.as_str());
    set(&mut json, "latexPost", nt.latex_post.as_str());
    set(&mut json, "latexsvg", o.get("latexsvg").and_then(Value::as_bool).unwrap_or(false));
    let req = if nt.config.req.is_empty() { fallback_req(nt) } else { nt.config.req.clone() };
    set(&mut json, "req", serde_json::to_value(&req).unwrap_or(Value::Array(Vec::new())));
    set(&mut json, "tags", o.get("tags").filter(|v| v.is_array()).cloned().unwrap_or(Value::Array(Vec::new())));
    set(&mut json, "vers", o.get("vers").filter(|v| v.is_array()).cloned().unwrap_or(Value::Array(Vec::new())));
    if let Some(s) = stock.filter(|s| *s > 0) {
        set(&mut json, "originalStockKind", s);
    }
    json
}

static FIELD_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^}]+)\}\}").unwrap());

/// Used only when the note type carries no `req` (modern Anki recomputes it on import; only very old
/// clients read it): a template requires 'any' of the fields its question side references.
fn fallback_req(nt: &Notetype) -> Vec<Requirement> {
    nt.templates
        .iter()
        .map(|t| {
            let mut refs = HashSet::new();
            for caps in FIELD_REF.captures_iter(&t.qfmt) {
                let inner = caps[1].trim();
                let inner = inner.strip_prefix(['#', '^', '/']).unwrap_or(inner);
                refs.insert(inner.rsplit(':').next().unwrap_or("").trim().to_string());
            }
            let ords: Vec<i64> = nt.fields.iter().filter(|f| refs.contains(&f.name)).map(|f| f.ord).collect();
            Requirement(t.ord, if ords.is_empty() { ReqKind::None } else { ReqKind::Any }, ords)
        })
        .collect()
}

// ───────────────────────── decks ─────────────────────────

/// Adds Default (id 1) and any missing parent decks; drops filtered decks when scheduling is excluded.
fn complete_decks(decks: &[AnkiDeck], cards: &mut [Card], with_sched: bool) -> Vec<AnkiDeck> {
    let mut out: Vec<AnkiDeck> = decks.to_vec();
    if !with_sched {
        let dyn_ids: HashSet<i64> = out.iter().filter(|d| d.filtered).map(|d| d.id).collect();
        for c in cards.iter_mut() {
            if dyn_ids.contains(&c.did) && c.odid == 0 {
                c.did = 1;
            }
        }
        out.retain(|d| !d.filtered);
    }
    if !out.iter().any(|d| d.id == 1) {
        let name = if out.iter().any(|d| d.name.eq_ignore_ascii_case("default")) { "Default (1)" } else { "Default" };
        out.insert(0, AnkiDeck::blank(1, name));
    }
    let mut by_name: HashSet<String> = out.iter().map(|d| d.name.to_lowercase()).collect();
    let mut next_id = out.iter().map(|d| d.id).max().unwrap_or(1).max(1) + 1;
    for d in out.clone() {
        let parts: Vec<&str> = d.name.split("::").collect();
        for i in 1..parts.len() {
            let parent = parts[..i].join("::");
            if !by_name.contains(&parent.to_lowercase()) {
                out.push(AnkiDeck::blank(next_id, parent.clone()));
                next_id += 1;
                by_name.insert(parent.to_lowercase());
            }
        }
    }
    out
}

fn deck_to_json(d: &AnkiDeck, with_sched: bool) -> Json {
    let o = d.original_json.clone().unwrap_or_default();
    let t = if with_sched { d.today.clone() } else { EMPTY_DECK_TODAY };
    let mut common = Json::new();
    set(&mut common, "id", d.id);
    set(&mut common, "name", d.name.as_str());
    set(&mut common, "mod", d.mtime);
    set(&mut common, "usn", d.usn);
    set(&mut common, "desc", d.description.as_str());
    set(&mut common, "collapsed", d.collapsed);
    set(&mut common, "browserCollapsed", d.browser_collapsed);
    let pair = |v: i64| Value::Array(vec![t.day.into(), v.into()]);
    set(&mut common, "newToday", pair(t.new));
    set(&mut common, "revToday", pair(t.rev));
    set(&mut common, "lrnToday", pair(t.lrn));
    set(&mut common, "timeToday", pair(t.time_ms));
    if d.filtered {
        let mut json = Json::new();
        set(&mut json, "separate", true);
        set(&mut json, "delays", Value::Null);
        set(&mut json, "previewDelay", 10);
        set(&mut json, "previewAgainSecs", 60);
        set(&mut json, "previewHardSecs", 600);
        set(&mut json, "previewGoodSecs", 0);
        json.extend(o.clone());
        json.extend(common);
        set(&mut json, "dyn", 1);
        set(&mut json, "resched", o.get("resched").and_then(Value::as_bool).unwrap_or(true));
        set(&mut json, "terms", o.get("terms").filter(|v| v.is_array()).cloned().unwrap_or(serde_json::json!([["", 100, 0]])));
        return json;
    }
    let mut json = o;
    json.extend(common);
    set(&mut json, "dyn", 0);
    set(&mut json, "conf", d.config_id);
    set(&mut json, "extendNew", d.extend_new);
    set(&mut json, "extendRev", d.extend_rev);
    if !with_sched {
        json.remove("reviewLimitToday");
        json.remove("newLimitToday");
    }
    match d.desired_retention {
        Some(dr) => set(&mut json, "desiredRetention", (dr * 100.0).round() as i64),
        None => {
            json.remove("desiredRetention");
        }
    }
    json
}

// ───────────────────────── notes, cards, revlog ─────────────────────────

fn note_tags(tags: &[String], with_sched: bool) -> Vec<String> {
    tags.iter()
        .filter(|t| with_sched || !(t.eq_ignore_ascii_case("leech") || t.eq_ignore_ascii_case("marked")))
        .cloned()
        .collect()
}

static INTEGER_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d{1,15}$").unwrap());

/// sfld: stripped sort field; integer-looking values are stored as integers (Anki's INTEGER affinity).
fn sort_field_value(text: &str) -> SqlValue {
    if INTEGER_LIKE.is_match(text) {
        if let Ok(n) = text.parse::<i64>() {
            return SqlValue::Integer(n);
        }
    }
    SqlValue::Text(text.to_string())
}

fn insert_notes(db: &Db, image: &AnkiImage, nt_by_id: &HashMap<i64, &Notetype>, with_sched: bool) -> Result<()> {
    for note in &image.notes {
        let Some(nt) = nt_by_id.get(&note.ntid) else { continue };
        let sort_idx = (nt.sort_field.max(0) as usize).min(note.fields.len().saturating_sub(1));
        let sfld = strip_html_preserving_media(note.fields.get(sort_idx).map(String::as_str).unwrap_or(""));
        let csum = field_checksum(&strip_html_preserving_media(note.fields.first().map(String::as_str).unwrap_or("")));
        db.run(
            "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, '')",
            rusqlite::params![
                note.id,
                note.guid,
                note.ntid,
                note.mtime,
                note.usn,
                join_tags(&note_tags(&note.tags, with_sched)),
                join_fields(&note.fields),
                sort_field_value(&sfld),
                csum as i64,
            ],
        )?;
    }
    Ok(())
}

/// `Number(x.toFixed(dp))`: decimal rounding on the exact binary value.
fn round_dp(x: f64, dp: usize) -> f64 {
    format!("{x:.dp$}").parse::<f64>().unwrap_or(x)
}

/// Anki's card.data JSON with Anki's rounding (s 4 dp, d 3 dp, dr 2 dp, decay 3 dp); '' when empty.
pub fn serialize_card_data(d: &CardData) -> String {
    let mut out = Json::new();
    if let Some(pos) = d.pos {
        set(&mut out, "pos", pos);
    }
    if let Some(s) = d.s {
        set(&mut out, "s", json_num(round_dp(s, 4)));
    }
    if let Some(x) = d.d {
        set(&mut out, "d", json_num(round_dp(x, 3)));
    }
    if let Some(dr) = d.dr {
        set(&mut out, "dr", json_num(round_dp(dr, 2)));
    }
    if let Some(decay) = d.decay {
        set(&mut out, "decay", json_num(round_dp(decay, 3)));
    }
    if let Some(lrt) = d.lrt {
        set(&mut out, "lrt", lrt);
    }
    if let Some(cd) = &d.cd {
        set(&mut out, "cd", cd.as_str());
    }
    if out.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&Value::Object(out)).unwrap_or_default()
    }
}

fn insert_cards(db: &Db, cards: &[Card]) -> Result<()> {
    for c in cards {
        db.run(
            "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                c.id,
                c.nid,
                c.did,
                c.ord,
                c.mtime,
                c.usn,
                c.ctype as i64,
                c.queue as i64,
                c.due,
                c.ivl,
                c.factor,
                c.reps,
                c.lapses,
                c.left,
                c.odue,
                c.odid,
                c.flags,
                serialize_card_data(&c.data),
            ],
        )?;
    }
    Ok(())
}

fn insert_revlog(db: &Db, revlog: &[&RevlogEntry]) -> Result<()> {
    for r in revlog {
        db.run(
            "INSERT OR IGNORE INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![r.id, r.cid, r.usn, r.ease, r.ivl, r.last_ivl, r.factor, r.time, r.rtype as i64],
        )?;
    }
    Ok(())
}

/// "Without scheduling" (Anki gather.rs): every card becomes new. Cards that are already new keep their
/// position; others get their remembered `data.pos`, else the next free position. Flags, filtered-deck
/// state and FSRS memory are cleared. Returns the next free position.
fn reset_to_new(cards: &mut [Card], next_pos: i64) -> i64 {
    let mut pos = next_pos;
    for c in cards.iter() {
        if c.ctype == CardType::New && c.queue >= CardQueue::UserBuried {
            pos = pos.max(c.due + 1);
        }
    }
    for c in cards.iter_mut() {
        if c.odid != 0 {
            c.did = c.odid;
            if c.odue != 0 {
                c.due = c.odue;
            }
        }
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
    pos
}

// ───────────────────────── dummy collection.anki2 ─────────────────────────

pub const DUMMY_NOTE_TEXT: &str = "Please update to the latest Anki version, then import the .colpkg/.apkg file again.";

/// The schema-11 "please update" collection Anki puts in every package as collection.anki2 (page size 512).
pub fn build_dummy_collection(now: i64) -> Result<Vec<u8>> {
    let db = Db::memory()?;
    db.execute_batch("PRAGMA page_size = 512;")?;
    db.execute_batch(SCHEMA11_DDL)?;
    let nt_id: i64 = 1342697561419;
    let model = serde_json::json!({
        "id": nt_id, "name": "Basic", "type": 0, "mod": 0, "usn": 0, "sortf": 0, "did": 1, "css": DEFAULT_CSS, "latexPre": "", "latexPost": "",
        "latexsvg": false, "req": [[0, "any", [0]]], "tags": [], "vers": [],
        "flds": [
            { "name": "Front", "ord": 0, "sticky": false, "rtl": false, "font": "Arial", "size": 20, "media": [] },
            { "name": "Back", "ord": 1, "sticky": false, "rtl": false, "font": "Arial", "size": 20, "media": [] },
        ],
        "tmpls": [{ "name": "Card 1", "ord": 0, "qfmt": "{{Front}}", "afmt": "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}", "bqfmt": "", "bafmt": "", "did": null, "bfont": "", "bsize": 0 }],
    });
    let deck = serde_json::json!({
        "id": 1, "name": "Default", "mod": 0, "usn": 0, "desc": "", "dyn": 0, "conf": 1, "collapsed": false, "browserCollapsed": false,
        "newToday": [0, 0], "revToday": [0, 0], "lrnToday": [0, 0], "timeToday": [0, 0], "extendNew": 0, "extendRev": 0,
    });
    let mut ignored = Vec::new();
    let dconf = deck_config_to_dconf(1, "Default", 0, 0, &dconf_to_deck_config(&Json::new(), false, true, &mut ignored), None);
    let conf = serde_json::json!({ "activeDecks": [1], "curDeck": 1, "nextPos": 2, "schedVer": 2, "sortType": "noteFld", "estTimes": true, "collapseTime": 1200 });
    db.run(
        "INSERT INTO col VALUES (1, ?, ?, ?, 11, 0, 0, 0, ?, ?, ?, ?, ?)",
        rusqlite::params![
            now.div_euclid(1000),
            now,
            now,
            conf.to_string(),
            serde_json::json!({ nt_id.to_string(): model }).to_string(),
            serde_json::json!({ "1": deck }).to_string(),
            serde_json::json!({ "1": dconf }).to_string(),
            "{}",
        ],
    )?;
    let fields = [DUMMY_NOTE_TEXT.to_string(), String::new()];
    db.run(
        "INSERT INTO notes VALUES (1, 'ankiupdate', ?, 0, 0, '', ?, ?, ?, 0, '')",
        rusqlite::params![nt_id, join_fields(&fields), DUMMY_NOTE_TEXT, field_checksum(DUMMY_NOTE_TEXT) as i64],
    )?;
    db.run("INSERT INTO cards VALUES (1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, '')", [])?;
    let bytes = db.serialize()?;
    db.close()?;
    Ok(bytes)
}
