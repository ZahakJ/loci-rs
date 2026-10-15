//! What is synced, and how a database row maps to a sync payload (docs/SYNC.md "Rows").
//!
//! Every synced object is `(kind, key) -> payload`, where the payload is a JSON array of the
//! object's synced columns. Local-only columns (`usn`, `decks.today`, `media.mtime`) are left
//! out. Card due days are stored relative to the epoch day instead of the collection's creation
//! day, so collections created on different days agree on what a payload means.

use crate::db::Db;
use crate::error::{LociError, Result};
use crate::undo::{json_to_sql, sql_to_json};
use rusqlite::types::Value as SqlValue;
use serde_json::Value;

pub type Payload = Vec<Value>;

/// Kinds of synced objects, in the order a merge applies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Notetypes,
    Presets,
    Decks,
    Config,
    Tags,
    Notes,
    Cards,
    Revlog,
    Media,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Notetypes,
        Kind::Presets,
        Kind::Decks,
        Kind::Config,
        Kind::Tags,
        Kind::Notes,
        Kind::Cards,
        Kind::Revlog,
        Kind::Media,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Notetypes => "notetypes",
            Kind::Presets => "presets",
            Kind::Decks => "decks",
            Kind::Config => "config",
            Kind::Tags => "tags",
            Kind::Notes => "notes",
            Kind::Cards => "cards",
            Kind::Revlog => "revlog",
            Kind::Media => "media",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.name() == s)
    }
}

/// Collection settings that follow the collection to every device. Theme, language, keyboard
/// hints and motion stay per device.
pub const SYNCED_SETTINGS: [&str; 7] = [
    "rollover_hour",
    "learn_ahead_minutes",
    "card_font",
    "card_font_size",
    "card_align",
    "typed_answer_ignore_case",
    "typed_answer_ignore_diacritics",
];

/// Facts a mapping needs about the local collection.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    /// Epoch day of the collection's creation (`round(crt / 86400)`).
    pub crt_day: i64,
    /// Now, seconds (local mtime of written media rows).
    pub now_s: i64,
}

/// A short, stable hash of a payload (first 8 bytes of SHA-1 of its JSON).
pub fn payload_hash(p: &Payload) -> i64 {
    let json = serde_json::to_string(p).unwrap_or_default();
    let d = crate::text::sha1(json.as_bytes());
    i64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}

/// ASCII lowercase, matching SQLite's NOCASE.
pub fn tag_key(tag: &str) -> String {
    tag.to_ascii_lowercase()
}

/// Tables whose objects are one row with a single key column.
struct Simple {
    table: &'static str,
    key: &'static str,
    key_is_int: bool,
    /// Synced columns, key first.
    cols: &'static [&'static str],
    /// Extra columns and SQL values for an INSERT.
    extra: &'static [(&'static str, &'static str)],
}

const NOTES: Simple = Simple {
    table: "notes",
    key: "id",
    key_is_int: true,
    cols: &["id", "guid", "ntid", "mtime", "tags", "flds", "sfld", "csum", "plain"],
    extra: &[("usn", "-1")],
};
const CARDS: Simple = Simple {
    table: "cards",
    key: "id",
    key_is_int: true,
    cols: &[
        "id", "nid", "did", "ord", "mtime", "type", "queue", "due", "ivl", "factor", "reps", "lapses", "left", "odue", "odid",
        "flags", "data",
    ],
    extra: &[("usn", "-1")],
};
const CARD_TYPE: usize = 5;
const CARD_DUE: usize = 7;
const DECKS: Simple = Simple {
    table: "decks",
    key: "id",
    key_is_int: true,
    cols: &["id", "name", "kind", "config_id", "description", "collapsed", "config", "mtime"],
    extra: &[("usn", "-1"), ("today", "'{}'")],
};
const PRESETS: Simple = Simple {
    table: "deck_configs",
    key: "id",
    key_is_int: true,
    cols: &["id", "name", "config", "mtime"],
    extra: &[("usn", "-1")],
};
const TAGS: Simple =
    Simple { table: "tags", key: "tag", key_is_int: false, cols: &["tag", "collapsed"], extra: &[("usn", "-1")] };
const MEDIA: Simple =
    Simple { table: "media", key: "name", key_is_int: false, cols: &["name", "sha1", "size"], extra: &[("mtime", "0")] };

fn simple(kind: Kind) -> Option<&'static Simple> {
    match kind {
        Kind::Notes => Some(&NOTES),
        Kind::Cards => Some(&CARDS),
        Kind::Decks => Some(&DECKS),
        Kind::Presets => Some(&PRESETS),
        Kind::Tags => Some(&TAGS),
        Kind::Media => Some(&MEDIA),
        _ => None,
    }
}

fn row_values(r: &rusqlite::Row<'_>, n: usize) -> rusqlite::Result<Payload> {
    (0..n).map(|i| Ok(sql_to_json(r.get_ref(i)?))).collect()
}

fn key_param(s: &Simple, key: &str) -> Result<SqlValue> {
    if s.key_is_int {
        key.parse::<i64>().map(SqlValue::Integer).map_err(|_| LociError::corrupt(format!("Bad {} key {key:?}", s.table)))
    } else {
        Ok(SqlValue::Text(key.to_string()))
    }
}

fn key_of(kind: Kind, p: &Payload) -> String {
    match &p[0] {
        Value::String(s) if kind == Kind::Tags => tag_key(s),
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

/// A card's due is a day number (not a position or a timestamp).
fn day_based(card_type: i64, due: i64) -> bool {
    card_type != 0 && due.abs() < 1_000_000_000
}

fn shift_due(p: &mut Payload, by: i64) {
    let ty = p[CARD_TYPE].as_i64().unwrap_or(0);
    if let Some(due) = p[CARD_DUE].as_i64() {
        if day_based(ty, due) {
            p[CARD_DUE] = Value::from(due + by);
        }
    }
}

fn notetype_select(filter: &str) -> String {
    format!("SELECT id, name, kind, css, latex_pre, latex_post, sort_field, config, mtime FROM notetypes {filter} ORDER BY id")
}

/// `(ntid, [values…])` rows of `fields` or `templates`.
type Parts = Vec<(i64, Value)>;

fn notetype_parts(db: &Db, ntid: Option<i64>) -> Result<(Parts, Parts)> {
    let (f_sql, t_sql) = match ntid {
        Some(_) => (
            "SELECT ntid, ord, name, config FROM fields WHERE ntid = ?1 ORDER BY ord",
            "SELECT ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config FROM templates WHERE ntid = ?1 ORDER BY ord",
        ),
        None => (
            "SELECT ntid, ord, name, config FROM fields ORDER BY ntid, ord",
            "SELECT ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config FROM templates ORDER BY ntid, ord",
        ),
    };
    let params: Vec<i64> = ntid.into_iter().collect();
    let fields = db.all(f_sql, rusqlite::params_from_iter(params.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, Value::Array((1..4).map(|i| r.get_ref(i).map(sql_to_json)).collect::<rusqlite::Result<_>>()?)))
    })?;
    let templates = db.all(t_sql, rusqlite::params_from_iter(params.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, Value::Array((1..8).map(|i| r.get_ref(i).map(sql_to_json)).collect::<rusqlite::Result<_>>()?)))
    })?;
    Ok((fields, templates))
}

fn read_notetypes(db: &Db, ntid: Option<i64>) -> Result<Vec<(String, Payload)>> {
    let rows = match ntid {
        Some(id) => db.all(&notetype_select("WHERE id = ?"), [id], |r| row_values(r, 9))?,
        None => db.all(&notetype_select(""), [], |r| row_values(r, 9))?,
    };
    let (fields, templates) = notetype_parts(db, ntid)?;
    let mut out = Vec::with_capacity(rows.len());
    for mut p in rows {
        let id = p[0].as_i64().unwrap_or(0);
        p.push(Value::Array(fields.iter().filter(|(n, _)| *n == id).map(|(_, v)| v.clone()).collect()));
        p.push(Value::Array(templates.iter().filter(|(n, _)| *n == id).map(|(_, v)| v.clone()).collect()));
        out.push((id.to_string(), p));
    }
    Ok(out)
}

const REVLOG_SELECT: &str = "SELECT r.id, r.cid, r.ease, r.ivl, r.last_ivl, r.factor, r.time, r.type, a.key FROM revlog r LEFT JOIN sync_revlog_alias a ON a.id = r.id";

fn revlog_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Payload)> {
    let mut p = row_values(r, 8)?;
    let alias: Option<String> = r.get(8)?;
    let key = alias.unwrap_or_else(|| format!("{}:{}", p[0], p[1]));
    if let Some(orig) = key.split(':').next().and_then(|s| s.parse::<i64>().ok()) {
        p[0] = Value::from(orig);
    }
    Ok((key, p))
}

/// Local revlog id of a sync key ("<original id>:<card id>").
fn revlog_local_id(db: &Db, key: &str) -> Result<Option<i64>> {
    if let Some(id) = db.scalar::<i64, _>("SELECT id FROM sync_revlog_alias WHERE key = ?", [key])? {
        return Ok(Some(id));
    }
    let mut parts = key.split(':');
    let (Some(id), Some(cid)) =
        (parts.next().and_then(|s| s.parse::<i64>().ok()), parts.next().and_then(|s| s.parse::<i64>().ok()))
    else {
        return Err(LociError::corrupt(format!("Bad revlog key {key:?}")));
    };
    let aliased = db.scalar::<i64, _>("SELECT 1 FROM sync_revlog_alias WHERE id = ?", [id])?.is_some();
    if aliased {
        return Ok(None);
    }
    db.scalar::<i64, _>("SELECT id FROM revlog WHERE id = ? AND cid = ?", [id, cid])
}

/// Every object of a kind, as `(key, payload)`.
pub fn read_all(db: &Db, kind: Kind, ctx: &Ctx) -> Result<Vec<(String, Payload)>> {
    if let Some(s) = simple(kind) {
        let sql = format!("SELECT {} FROM {}", s.cols.join(", "), s.table);
        let mut rows = db.all(&sql, [], |r| row_values(r, s.cols.len()))?;
        if kind == Kind::Cards {
            for p in &mut rows {
                shift_due(p, ctx.crt_day);
            }
        }
        return Ok(rows.into_iter().map(|p| (key_of(kind, &p), p)).collect());
    }
    match kind {
        Kind::Notetypes => read_notetypes(db, None),
        Kind::Config => {
            let sql = format!("SELECT key, value FROM col_meta WHERE key IN ({})", crate::undo::marks(SYNCED_SETTINGS.len()));
            db.all(&sql, rusqlite::params_from_iter(SYNCED_SETTINGS.iter()), |r| {
                Ok((r.get(0)?, vec![sql_to_json(r.get_ref(1)?)]))
            })
        }
        Kind::Revlog => db.all(REVLOG_SELECT, [], revlog_row),
        _ => unreachable!("simple kinds are handled above"),
    }
}

/// One object, or None when it does not exist.
pub fn read_one(db: &Db, kind: Kind, key: &str, ctx: &Ctx) -> Result<Option<Payload>> {
    if let Some(s) = simple(kind) {
        let sql = format!("SELECT {} FROM {} WHERE {} = ?", s.cols.join(", "), s.table, s.key);
        let mut p = db.get(&sql, [key_param(s, key)?], |r| row_values(r, s.cols.len()))?;
        if let (Kind::Cards, Some(p)) = (kind, p.as_mut()) {
            shift_due(p, ctx.crt_day);
        }
        return Ok(p);
    }
    match kind {
        Kind::Notetypes => {
            let id = key.parse::<i64>().map_err(|_| LociError::corrupt(format!("Bad note type key {key:?}")))?;
            Ok(read_notetypes(db, Some(id))?.into_iter().next().map(|(_, p)| p))
        }
        Kind::Config => {
            if !SYNCED_SETTINGS.contains(&key) {
                return Ok(None);
            }
            Ok(db.get("SELECT value FROM col_meta WHERE key = ?", [key], |r| Ok(vec![sql_to_json(r.get_ref(0)?)]))?)
        }
        Kind::Revlog => match revlog_local_id(db, key)? {
            Some(id) => Ok(db.get(&format!("{REVLOG_SELECT} WHERE r.id = ?"), [id], revlog_row)?.map(|(_, p)| p)),
            None => Ok(None),
        },
        _ => unreachable!("simple kinds are handled above"),
    }
}

/// What a write did, for the merge report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Written {
    pub inserted: bool,
}

fn expect_len(kind: Kind, p: &Payload, n: usize) -> Result<()> {
    if p.len() != n {
        return Err(LociError::corrupt(format!("A {} row in the sync file has {} values, expected {n}", kind.name(), p.len())));
    }
    Ok(())
}

fn write_simple(db: &Db, s: &Simple, p: &Payload) -> Result<Written> {
    let values: Vec<SqlValue> = p.iter().map(json_to_sql).collect();
    let set = s.cols[1..].iter().map(|c| format!("\"{c}\" = ?")).collect::<Vec<_>>().join(", ");
    let mut params: Vec<SqlValue> = values[1..].to_vec();
    params.push(values[0].clone());
    let changed =
        db.run(&format!("UPDATE {} SET {set} WHERE {} = ?", s.table, s.key), rusqlite::params_from_iter(params.iter()))?;
    if changed > 0 {
        return Ok(Written { inserted: false });
    }
    let mut cols: Vec<String> = s.cols.iter().map(|c| format!("\"{c}\"")).collect();
    let mut marks: Vec<String> = vec!["?".to_string(); s.cols.len()];
    for (c, v) in s.extra {
        cols.push(format!("\"{c}\""));
        marks.push((*v).to_string());
    }
    db.run(
        &format!("INSERT INTO {} ({}) VALUES ({})", s.table, cols.join(", "), marks.join(", ")),
        rusqlite::params_from_iter(values.iter()),
    )?;
    Ok(Written { inserted: true })
}

/// Creates or replaces an object from a payload.
pub fn write(db: &Db, kind: Kind, key: &str, payload: &Payload, ctx: &Ctx) -> Result<Written> {
    if let Some(s) = simple(kind) {
        expect_len(kind, payload, s.cols.len())?;
        let mut p = payload.clone();
        if kind == Kind::Cards {
            shift_due(&mut p, -ctx.crt_day);
        }
        let w = write_simple(db, s, &p)?;
        if kind == Kind::Media {
            db.run("UPDATE media SET mtime = ? WHERE name = ?", rusqlite::params![ctx.now_s, key])?;
        }
        return Ok(w);
    }
    match kind {
        Kind::Notetypes => {
            expect_len(kind, payload, 11)?;
            let id = key.parse::<i64>().map_err(|_| LociError::corrupt(format!("Bad note type key {key:?}")))?;
            let row: Vec<SqlValue> = payload[..9].iter().map(json_to_sql).collect();
            let existed = db.run("DELETE FROM notetypes WHERE id = ?", [id])? > 0;
            db.run(
                "INSERT INTO notetypes (id, name, kind, css, latex_pre, latex_post, sort_field, config, mtime, usn) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, -1)",
                rusqlite::params_from_iter(row.iter()),
            )?;
            db.run("DELETE FROM fields WHERE ntid = ?", [id])?;
            db.run("DELETE FROM templates WHERE ntid = ?", [id])?;
            for f in payload[9].as_array().into_iter().flatten() {
                let v: Vec<SqlValue> = f.as_array().into_iter().flatten().map(json_to_sql).collect();
                if v.len() != 3 {
                    return Err(LociError::corrupt("A note type field in the sync file is malformed"));
                }
                db.run(
                    "INSERT INTO fields (ntid, ord, name, config) VALUES (?, ?, ?, ?)",
                    rusqlite::params![id, v[0], v[1], v[2]],
                )?;
            }
            for t in payload[10].as_array().into_iter().flatten() {
                let v: Vec<SqlValue> = t.as_array().into_iter().flatten().map(json_to_sql).collect();
                if v.len() != 7 {
                    return Err(LociError::corrupt("A note type template in the sync file is malformed"));
                }
                db.run(
                    "INSERT INTO templates (ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    rusqlite::params![id, v[0], v[1], v[2], v[3], v[4], v[5], v[6]],
                )?;
            }
            Ok(Written { inserted: !existed })
        }
        Kind::Config => {
            expect_len(kind, payload, 1)?;
            if !SYNCED_SETTINGS.contains(&key) {
                return Ok(Written::default());
            }
            let value = match &payload[0] {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            let existed = db.scalar::<i64, _>("SELECT 1 FROM col_meta WHERE key = ?", [key])?.is_some();
            db.run(
                "INSERT INTO col_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value.as_str()],
            )?;
            Ok(Written { inserted: !existed })
        }
        Kind::Revlog => {
            expect_len(kind, payload, 8)?;
            let values: Vec<SqlValue> = payload.iter().map(json_to_sql).collect();
            if let Some(local) = revlog_local_id(db, key)? {
                db.run(
                    "UPDATE revlog SET ease = ?, ivl = ?, last_ivl = ?, factor = ?, time = ?, type = ? WHERE id = ?",
                    rusqlite::params![values[2], values[3], values[4], values[5], values[6], values[7], local],
                )?;
                return Ok(Written { inserted: false });
            }
            let orig = payload[0].as_i64().ok_or_else(|| LociError::corrupt("A review in the sync file has no id"))?;
            let mut id = orig;
            while db.scalar::<i64, _>("SELECT 1 FROM revlog WHERE id = ?", [id])?.is_some() {
                id += 1;
            }
            db.run(
                "INSERT INTO revlog (id, cid, usn, ease, ivl, last_ivl, factor, time, type) VALUES (?, ?, -1, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![id, values[1], values[2], values[3], values[4], values[5], values[6], values[7]],
            )?;
            if id != orig {
                db.run("INSERT OR REPLACE INTO sync_revlog_alias (id, key) VALUES (?, ?)", rusqlite::params![id, key])?;
            }
            Ok(Written { inserted: true })
        }
        _ => unreachable!("simple kinds are handled above"),
    }
}

/// Removes an object (no-op when absent). Returns true when something was removed.
pub fn delete(db: &Db, kind: Kind, key: &str) -> Result<bool> {
    if let Some(s) = simple(kind) {
        return Ok(db.run(&format!("DELETE FROM {} WHERE {} = ?", s.table, s.key), [key_param(s, key)?])? > 0);
    }
    match kind {
        Kind::Notetypes => {
            let id = key.parse::<i64>().map_err(|_| LociError::corrupt(format!("Bad note type key {key:?}")))?;
            db.run("DELETE FROM fields WHERE ntid = ?", [id])?;
            db.run("DELETE FROM templates WHERE ntid = ?", [id])?;
            Ok(db.run("DELETE FROM notetypes WHERE id = ?", [id])? > 0)
        }
        Kind::Config => {
            if !SYNCED_SETTINGS.contains(&key) {
                return Ok(false);
            }
            Ok(db.run("DELETE FROM col_meta WHERE key = ?", [key])? > 0)
        }
        Kind::Revlog => match revlog_local_id(db, key)? {
            Some(id) => {
                db.run("DELETE FROM sync_revlog_alias WHERE id = ?", [id])?;
                Ok(db.run("DELETE FROM revlog WHERE id = ?", [id])? > 0)
            }
            None => Ok(false),
        },
        _ => unreachable!("simple kinds are handled above"),
    }
}

/// SQL run on every open: the change-capture triggers (recreated, so a newer definition replaces
/// an older one). Each records `(kind, key)` in
/// `sync_dirty` once sync is on for this collection; the next stamping pass turns it into a stamp
/// if the payload really changed.
pub fn trigger_sql() -> String {
    const ON: &str = "EXISTS (SELECT 1 FROM sync_meta WHERE key = 'device')";
    // Not `INSERT OR IGNORE`: a trigger takes the conflict policy of the statement that fired it.
    let mark = |kind: &str, k: &str, when: &str| {
        format!(
            "INSERT INTO sync_dirty (tbl, key) SELECT '{kind}', {k} WHERE NOT EXISTS (SELECT 1 FROM sync_dirty WHERE tbl = '{kind}' AND key = {k}){when};"
        )
    };
    let mut out = String::new();
    let mut add = |name: &str, table: &str, kind: &str, key: &str, when: &str| {
        let w = |side: &str| if when.is_empty() { String::new() } else { format!(" AND {}", when.replace('$', side)) };
        let k = |side: &str| key.replace('$', side);
        out.push_str(&format!(
            "DROP TRIGGER IF EXISTS sync_{name}_ai; CREATE TRIGGER sync_{name}_ai AFTER INSERT ON {table} WHEN {ON} BEGIN {} END;\n",
            mark(kind, &k("new"), &w("new"))
        ));
        out.push_str(&format!(
            "DROP TRIGGER IF EXISTS sync_{name}_au; CREATE TRIGGER sync_{name}_au AFTER UPDATE ON {table} WHEN {ON} BEGIN {} {} END;\n",
            mark(kind, &k("new"), &w("new")),
            mark(kind, &k("old"), &w("old"))
        ));
        out.push_str(&format!(
            "DROP TRIGGER IF EXISTS sync_{name}_ad; CREATE TRIGGER sync_{name}_ad AFTER DELETE ON {table} WHEN {ON} BEGIN {} END;\n",
            mark(kind, &k("old"), &w("old"))
        ));
    };
    add("notes", "notes", "notes", "CAST($.id AS TEXT)", "");
    add("cards", "cards", "cards", "CAST($.id AS TEXT)", "");
    add("decks", "decks", "decks", "CAST($.id AS TEXT)", "");
    add("presets", "deck_configs", "presets", "CAST($.id AS TEXT)", "");
    add("notetypes", "notetypes", "notetypes", "CAST($.id AS TEXT)", "");
    add("fields", "fields", "notetypes", "CAST($.ntid AS TEXT)", "");
    add("templates", "templates", "notetypes", "CAST($.ntid AS TEXT)", "");
    add("tags", "tags", "tags", "lower($.tag)", "");
    add("media", "media", "media", "$.name", "");
    let settings = SYNCED_SETTINGS.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
    add("config", "col_meta", "config", "$.key", &format!("$.key IN ({settings})"));
    let revlog_key =
        "COALESCE((SELECT key FROM sync_revlog_alias WHERE id = $.id), CAST($.id AS TEXT) || ':' || CAST($.cid AS TEXT))";
    add("revlog", "revlog", "revlog", revlog_key, "");
    out
}
