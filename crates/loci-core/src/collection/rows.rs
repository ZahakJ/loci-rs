//! Row shapes of the collection tables and their mappers to/from domain types
//! (recall `src/core/collection/rows.ts`). Shared with the scheduler, search, stats and
//! import/export modules. Column names and units: `schema.sql`.

use crate::collection::presets::normalise_deck_config;
use crate::text::{field_checksum, join_fields, join_tags, plain_text, split_fields, split_tags, strip_html_preserving_media};
use crate::types::*;
use rusqlite::Row;

/// Column lists in schema order, for INSERT statements and snapshots.
pub const CARD_COLUMNS: [&str; 18] = [
    "id", "nid", "did", "ord", "mtime", "usn", "type", "queue", "due", "ivl", "factor", "reps", "lapses", "left", "odue", "odid",
    "flags", "data",
];
pub const NOTE_COLUMNS: [&str; 10] = ["id", "guid", "ntid", "mtime", "usn", "tags", "flds", "sfld", "csum", "plain"];
pub const REVLOG_COLUMNS: [&str; 9] = ["id", "cid", "usn", "ease", "ivl", "last_ivl", "factor", "time", "type"];
pub const DECK_COLUMNS: [&str; 10] =
    ["id", "name", "kind", "config_id", "description", "collapsed", "today", "config", "mtime", "usn"];

/// `INSERT INTO <table> ("cols") VALUES (?, …)`. To overwrite existing rows use the undo module's
/// `restore_rows` (UPDATE-then-INSERT): INSERT OR REPLACE would bypass the notes delete trigger
/// and desynchronise `notes_fts`.
pub fn insert_sql(table: &str, columns: &[&str]) -> String {
    let cols: Vec<String> = columns.iter().map(|c| format!("\"{c}\"")).collect();
    let marks: Vec<&str> = columns.iter().map(|_| "?").collect();
    format!("INSERT INTO {table} ({}) VALUES ({})", cols.join(", "), marks.join(", "))
}

/// `SELECT <cols> FROM <table>` prefix with columns in schema order.
pub fn select_sql(table: &str, columns: &[&str]) -> String {
    let cols: Vec<String> = columns.iter().map(|c| format!("\"{c}\"")).collect();
    format!("SELECT {} FROM {table}", cols.join(", "))
}

/// Parses a JSON object column; returns `{}` for invalid or non-object values.
pub fn parse_json_object(text: &str) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

// ───────────── cards ─────────────

pub fn parse_card_data(text: &str) -> CardData {
    let o = parse_json_object(text);
    // Typed keys with wrong JSON types are dropped (as the TypeScript did); unknown keys kept.
    let mut d = CardData::default();
    for (k, v) in o {
        match k.as_str() {
            "s" => d.s = v.as_f64(),
            "d" => d.d = v.as_f64(),
            "dr" => d.dr = v.as_f64(),
            "decay" => d.decay = v.as_f64(),
            "pos" => d.pos = v.as_f64().map(|x| x as i64),
            "lrt" => d.lrt = v.as_f64().map(|x| x as i64),
            "cd" => d.cd = v.as_str().map(str::to_string),
            _ => {
                d.extra.insert(k, v);
            }
        }
    }
    d
}

pub fn card_data_to_json(d: &CardData) -> String {
    serde_json::to_string(d).unwrap_or_else(|_| "{}".to_string())
}

/// Maps a row selected with `CARD_COLUMNS` (in that order) to a `Card`.
pub fn row_to_card(r: &Row<'_>) -> rusqlite::Result<Card> {
    let ctype: i64 = r.get(6)?;
    let queue: i64 = r.get(7)?;
    let data: String = r.get::<_, Option<String>>(17)?.unwrap_or_default();
    Ok(Card {
        id: r.get(0)?,
        nid: r.get(1)?,
        did: r.get(2)?,
        ord: r.get(3)?,
        mtime: r.get(4)?,
        usn: r.get(5)?,
        ctype: card_type_from_i64(ctype),
        queue: card_queue_from_i64(queue),
        due: r.get(8)?,
        ivl: r.get(9)?,
        factor: r.get(10)?,
        reps: r.get(11)?,
        lapses: r.get(12)?,
        left: r.get(13)?,
        odue: r.get(14)?,
        odid: r.get(15)?,
        flags: r.get(16)?,
        data: parse_card_data(&data),
    })
}

pub fn card_type_from_i64(v: i64) -> CardType {
    match v {
        1 => CardType::Learn,
        2 => CardType::Review,
        3 => CardType::Relearn,
        _ => CardType::New,
    }
}

pub fn card_queue_from_i64(v: i64) -> CardQueue {
    match v {
        -3 => CardQueue::UserBuried,
        -2 => CardQueue::SchedBuried,
        -1 => CardQueue::Suspended,
        1 => CardQueue::Learn,
        2 => CardQueue::Review,
        3 => CardQueue::DayLearn,
        4 => CardQueue::Preview,
        _ => CardQueue::New,
    }
}

/// Values of a card in `CARD_COLUMNS` order, for `insert_sql("cards", &CARD_COLUMNS)`.
pub fn card_params(c: &Card) -> [rusqlite::types::Value; 18] {
    use rusqlite::types::Value as V;
    [
        V::Integer(c.id),
        V::Integer(c.nid),
        V::Integer(c.did),
        V::Integer(c.ord),
        V::Integer(c.mtime),
        V::Integer(c.usn),
        V::Integer(c.ctype as i64),
        V::Integer(c.queue as i64),
        V::Integer(c.due),
        V::Integer(c.ivl),
        V::Integer(c.factor),
        V::Integer(c.reps),
        V::Integer(c.lapses),
        V::Integer(c.left),
        V::Integer(c.odue),
        V::Integer(c.odid),
        V::Integer(c.flags),
        V::Text(card_data_to_json(&c.data)),
    ]
}

// ───────────── notes ─────────────

/// Maps a row selected with `NOTE_COLUMNS` to a `Note`.
pub fn row_to_note(r: &Row<'_>) -> rusqlite::Result<Note> {
    let tags: String = r.get(5)?;
    let flds: String = r.get(6)?;
    Ok(Note {
        id: r.get(0)?,
        guid: r.get(1)?,
        ntid: r.get(2)?,
        mtime: r.get(3)?,
        usn: r.get(4)?,
        tags: split_tags(&tags),
        fields: split_fields(&flds),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteDerived {
    pub sfld: String,
    pub csum: i64,
    pub plain: String,
}

/// Derived note columns (Anki semantics).
pub fn note_derived_columns(fields: &[String], sort_field: i64) -> NoteDerived {
    let sort_idx = if sort_field >= 0 && (sort_field as usize) < fields.len() { sort_field as usize } else { 0 };
    let first = fields.first().map(String::as_str).unwrap_or("");
    NoteDerived {
        sfld: strip_html_preserving_media(fields.get(sort_idx).map(String::as_str).unwrap_or("")),
        csum: field_checksum(&strip_html_preserving_media(first)) as i64,
        plain: fields.iter().map(|f| plain_text(f)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "),
    }
}

/// Values of a note in `NOTE_COLUMNS` order; `sort_field` is the note type's sort field index.
pub fn note_params(n: &Note, sort_field: i64) -> [rusqlite::types::Value; 10] {
    use rusqlite::types::Value as V;
    let d = note_derived_columns(&n.fields, sort_field);
    [
        V::Integer(n.id),
        V::Text(n.guid.clone()),
        V::Integer(n.ntid),
        V::Integer(n.mtime),
        V::Integer(n.usn),
        V::Text(join_tags(&n.tags)),
        V::Text(join_fields(&n.fields)),
        V::Text(d.sfld),
        V::Integer(d.csum),
        V::Text(d.plain),
    ]
}

// ───────────── revlog ─────────────

pub fn revlog_type_from_i64(v: i64) -> RevlogType {
    match v {
        1 => RevlogType::Review,
        2 => RevlogType::Relearn,
        3 => RevlogType::Filtered,
        4 => RevlogType::Manual,
        5 => RevlogType::Rescheduled,
        _ => RevlogType::Learn,
    }
}

/// Maps a row selected with `REVLOG_COLUMNS` to a `RevlogEntry`.
pub fn row_to_revlog(r: &Row<'_>) -> rusqlite::Result<RevlogEntry> {
    let rtype: i64 = r.get(8)?;
    Ok(RevlogEntry {
        id: r.get(0)?,
        cid: r.get(1)?,
        usn: r.get(2)?,
        ease: r.get(3)?,
        ivl: r.get(4)?,
        last_ivl: r.get(5)?,
        factor: r.get(6)?,
        time: r.get(7)?,
        rtype: revlog_type_from_i64(rtype),
    })
}

pub fn revlog_params(e: &RevlogEntry) -> [rusqlite::types::Value; 9] {
    use rusqlite::types::Value as V;
    [
        V::Integer(e.id),
        V::Integer(e.cid),
        V::Integer(e.usn),
        V::Integer(e.ease),
        V::Integer(e.ivl),
        V::Integer(e.last_ivl),
        V::Integer(e.factor),
        V::Integer(e.time),
        V::Integer(e.rtype as i64),
    ]
}

// ───────────── note types ─────────────

pub fn parse_field_config(text: &str) -> FieldConfig {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn parse_template_config(text: &str) -> TemplateConfig {
    let mut c: TemplateConfig = serde_json::from_str(text).unwrap_or_default();
    if c.deck_override.is_some_and(|d| d <= 0) {
        c.deck_override = None;
    }
    c
}

pub fn parse_notetype_config(text: &str) -> NotetypeConfig {
    serde_json::from_str(text).unwrap_or_default()
}

/// Row of the `notetypes` table.
#[derive(Debug, Clone)]
pub struct NotetypeRow {
    pub id: i64,
    pub name: String,
    pub kind: i64,
    pub css: String,
    pub latex_pre: String,
    pub latex_post: String,
    pub sort_field: i64,
    pub config: String,
    pub mtime: i64,
    pub usn: i64,
}

pub const NOTETYPE_COLUMNS: [&str; 10] =
    ["id", "name", "kind", "css", "latex_pre", "latex_post", "sort_field", "config", "mtime", "usn"];

pub fn row_to_notetype_row(r: &Row<'_>) -> rusqlite::Result<NotetypeRow> {
    Ok(NotetypeRow {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        css: r.get(3)?,
        latex_pre: r.get(4)?,
        latex_post: r.get(5)?,
        sort_field: r.get(6)?,
        config: r.get(7)?,
        mtime: r.get(8)?,
        usn: r.get(9)?,
    })
}

#[derive(Debug, Clone)]
pub struct FieldRow {
    pub ntid: i64,
    pub ord: i64,
    pub name: String,
    pub config: String,
}

#[derive(Debug, Clone)]
pub struct TemplateRow {
    pub ntid: i64,
    pub ord: i64,
    pub name: String,
    pub qfmt: String,
    pub afmt: String,
    pub bqfmt: String,
    pub bafmt: String,
    pub config: String,
}

pub fn row_to_field_row(r: &Row<'_>) -> rusqlite::Result<FieldRow> {
    Ok(FieldRow { ntid: r.get(0)?, ord: r.get(1)?, name: r.get(2)?, config: r.get(3)? })
}

pub fn row_to_template_row(r: &Row<'_>) -> rusqlite::Result<TemplateRow> {
    Ok(TemplateRow {
        ntid: r.get(0)?,
        ord: r.get(1)?,
        name: r.get(2)?,
        qfmt: r.get(3)?,
        afmt: r.get(4)?,
        bqfmt: r.get(5)?,
        bafmt: r.get(6)?,
        config: r.get(7)?,
    })
}

pub fn rows_to_notetype(r: &NotetypeRow, mut fields: Vec<FieldRow>, mut templates: Vec<TemplateRow>) -> Notetype {
    fields.sort_by_key(|f| f.ord);
    templates.sort_by_key(|t| t.ord);
    Notetype {
        id: r.id,
        name: r.name.clone(),
        kind: if r.kind == 1 { NotetypeKind::Cloze } else { NotetypeKind::Standard },
        css: r.css.clone(),
        latex_pre: r.latex_pre.clone(),
        latex_post: r.latex_post.clone(),
        sort_field: r.sort_field,
        fields: fields.into_iter().map(|f| Field { ord: f.ord, name: f.name, config: parse_field_config(&f.config) }).collect(),
        templates: templates
            .into_iter()
            .map(|t| Template {
                ord: t.ord,
                name: t.name,
                qfmt: t.qfmt,
                afmt: t.afmt,
                bqfmt: t.bqfmt,
                bafmt: t.bafmt,
                config: parse_template_config(&t.config),
            })
            .collect(),
        config: parse_notetype_config(&r.config),
        mtime: r.mtime,
        usn: r.usn,
    }
}

pub fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
}

// ───────────── decks & presets ─────────────

pub fn parse_deck_today(text: &str) -> DeckToday {
    let o = parse_json_object(text);
    let n = |k: &str| o.get(k).and_then(|v| v.as_f64()).map(|x| x as i64).unwrap_or(0);
    DeckToday { day: n("day"), new: n("new"), rev: n("rev"), lrn: n("lrn"), time_ms: n("time") }
}

/// Serialises DeckToday with the stored key names (`{day, new, rev, lrn, time}`).
pub fn deck_today_to_json(t: &DeckToday) -> String {
    serde_json::json!({"day": t.day, "new": t.new, "rev": t.rev, "lrn": t.lrn, "time": t.time_ms}).to_string()
}

/// Maps a row selected with `DECK_COLUMNS` to a `Deck` (the `kind` and `config` columns are
/// read but not surfaced).
pub fn row_to_deck(r: &Row<'_>) -> rusqlite::Result<Deck> {
    let collapsed: i64 = r.get(5)?;
    let today: String = r.get(6)?;
    Ok(Deck {
        id: r.get(0)?,
        name: r.get(1)?,
        config_id: r.get(3)?,
        description: r.get(4)?,
        collapsed: collapsed != 0,
        today: parse_deck_today(&today),
        mtime: r.get(8)?,
        usn: r.get(9)?,
    })
}

/// Values of a deck in `DECK_COLUMNS` order; `extra` is the opaque DeckExtra JSON column.
pub fn deck_params(d: &Deck, extra: &str) -> [rusqlite::types::Value; 10] {
    use rusqlite::types::Value as V;
    [
        V::Integer(d.id),
        V::Text(d.name.clone()),
        V::Integer(0),
        V::Integer(d.config_id),
        V::Text(d.description.clone()),
        V::Integer(if d.collapsed { 1 } else { 0 }),
        V::Text(deck_today_to_json(&d.today)),
        V::Text(extra.to_string()),
        V::Integer(d.mtime),
        V::Integer(d.usn),
    ]
}

pub const DECK_CONFIG_COLUMNS: [&str; 5] = ["id", "name", "config", "mtime", "usn"];

/// Maps a row selected with `DECK_CONFIG_COLUMNS` to a preset (config normalised and complete).
pub fn row_to_preset(r: &Row<'_>) -> rusqlite::Result<DeckConfigPreset> {
    let config: String = r.get(2)?;
    let value: serde_json::Value = serde_json::from_str(&config).unwrap_or(serde_json::Value::Null);
    Ok(DeckConfigPreset {
        id: r.get(0)?,
        name: r.get(1)?,
        config: normalise_deck_config(&value),
        mtime: r.get(3)?,
        usn: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_data_round_trip() {
        let d = parse_card_data(r#"{"s":1.5,"d":"bad","pos":3,"cd":"{\"x\":1}","zzz":true}"#);
        assert_eq!(d.s, Some(1.5));
        assert_eq!(d.d, None);
        assert_eq!(d.pos, Some(3));
        assert_eq!(d.extra["zzz"], true);
        assert_eq!(parse_card_data("garbage").s, None);
    }

    #[test]
    fn derived_columns() {
        let d = note_derived_columns(&["<b>test</b>".into(), "x".into()], 1);
        assert_eq!(d.sfld, "x");
        assert_eq!(d.csum, 2840236005);
        assert_eq!(d.plain, "test x");
        assert_eq!(note_derived_columns(&["a".into()], 9).sfld, "a");
    }

    #[test]
    fn deck_today_json_keys() {
        let t = DeckToday { day: 3, new: 1, rev: 2, lrn: 0, time_ms: 500 };
        let s = deck_today_to_json(&t);
        assert!(s.contains("\"time\":500"));
        assert_eq!(parse_deck_today(&s), t);
    }

    #[test]
    fn insert_sql_shape() {
        assert_eq!(insert_sql("x", &["a", "b"]), "INSERT INTO x (\"a\", \"b\") VALUES (?, ?)");
    }

    #[test]
    fn card_rows_round_trip_through_sqlite() {
        let db = crate::db::Db::memory().unwrap();
        db.execute_batch(crate::SCHEMA_SQL).unwrap();
        let c = Card {
            id: 7,
            nid: 1,
            did: 1,
            ord: 0,
            mtime: 5,
            usn: -1,
            ctype: CardType::Review,
            queue: CardQueue::Suspended,
            due: 12,
            ivl: 3,
            factor: 2500,
            reps: 2,
            lapses: 1,
            left: 0,
            odue: 0,
            odid: 0,
            flags: 2,
            data: parse_card_data(r#"{"s":2.5,"d":4.1}"#),
        };
        db.run(&insert_sql("cards", &CARD_COLUMNS), rusqlite::params_from_iter(card_params(&c))).unwrap();
        let back = db.get(&format!("{} WHERE id = 7", select_sql("cards", &CARD_COLUMNS)), [], row_to_card).unwrap().unwrap();
        assert_eq!(back, c);
    }
}
