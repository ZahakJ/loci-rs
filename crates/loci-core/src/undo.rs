//! Undo stack persisted in `undo_log` (recall `src/core/undo/undo.ts`). Each entry stores the
//! inverse of one user-level operation as row snapshots: the rows as they were before the change
//! (restored on undo) plus the ids the operation inserted (deleted on undo). Capped at 50 entries.
//!
//! Usage from any module, inside the operation's transaction and BEFORE mutating:
//! ```text
//! let before = undo::snapshot_rows(db, Table::Cards, &ids)?;
//! …mutate…
//! undo::record(db, now_ms, "Suspend", UndoSnapshot { cards: before, ..Default::default() })?;
//! ```

use crate::collection::rows::{CARD_COLUMNS, DECK_COLUMNS, NOTE_COLUMNS, REVLOG_COLUMNS};
use crate::db::Db;
use crate::error::{LociError, Result};
use crate::text::split_tags;
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Maximum number of undo entries kept.
pub const UNDO_CAP: i64 = 50;
/// SQLite's default bound-parameter limit is 32766; stay well below it.
pub const CHUNK: usize = 500;

/// A row snapshot: column name → JSON value (integers, floats, text, null).
pub type RowSnapshot = Map<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Cards,
    Notes,
    Revlog,
    Decks,
}

impl Table {
    pub fn name(self) -> &'static str {
        match self {
            Table::Cards => "cards",
            Table::Notes => "notes",
            Table::Revlog => "revlog",
            Table::Decks => "decks",
        }
    }
    pub fn columns(self) -> &'static [&'static str] {
        match self {
            Table::Cards => &CARD_COLUMNS,
            Table::Notes => &NOTE_COLUMNS,
            Table::Revlog => &REVLOG_COLUMNS,
            Table::Decks => &DECK_COLUMNS,
        }
    }
}

/// A `deck_configs` row before the operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetRow {
    pub id: i64,
    pub name: String,
    pub config: String,
    pub mtime: i64,
    pub usn: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraveRow {
    pub oid: i64,
    #[serde(rename = "type")]
    pub gtype: i64,
}

/// The inverse of one operation. Every key is optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UndoSnapshot {
    /// Rows as they were before the operation; restored verbatim on undo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<RowSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<RowSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revlog: Vec<RowSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decks: Vec<RowSnapshot>,
    /// Ids the operation inserted; deleted on undo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inserted_cards: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inserted_notes: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inserted_revlog: Vec<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inserted_decks: Vec<i64>,
    /// Grave rows the operation added; removed on undo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub graves: Vec<GraveRow>,
    /// col_meta values before the operation (null = key was absent).
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub meta: Map<String, Value>,
    /// deck_configs rows before the operation; restored verbatim.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetRow>,
}

fn sql_to_json(v: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef as V;
    match v {
        V::Null => Value::Null,
        V::Integer(i) => Value::from(i),
        V::Real(f) => Value::from(f),
        V::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        V::Blob(b) => Value::Array(b.iter().map(|x| Value::from(*x)).collect()),
    }
}

fn json_to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else {
                SqlValue::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Array(a) => SqlValue::Blob(a.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect()),
        Value::Object(_) => SqlValue::Text(v.to_string()),
    }
}

fn unique(ids: &[i64]) -> Vec<i64> {
    let mut v = ids.to_vec();
    v.sort_unstable();
    v.dedup();
    v
}

pub fn marks(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn rows_from_query(db: &Db, sql: &str, params: &[i64], columns: &[&str]) -> Result<Vec<RowSnapshot>> {
    db.all(sql, rusqlite::params_from_iter(params.iter()), |r| {
        let mut m = Map::new();
        for (i, c) in columns.iter().enumerate() {
            m.insert((*c).to_string(), sql_to_json(r.get_ref(i)?));
        }
        Ok(m)
    })
}

/// Selects full rows by id, in chunks.
pub fn snapshot_rows(db: &Db, table: Table, ids: &[i64]) -> Result<Vec<RowSnapshot>> {
    let cols: Vec<String> = table.columns().iter().map(|c| format!("\"{c}\"")).collect();
    let mut out = Vec::new();
    for part in unique(ids).chunks(CHUNK) {
        let sql = format!("SELECT {} FROM {} WHERE id IN ({})", cols.join(", "), table.name(), marks(part.len()));
        out.extend(rows_from_query(db, &sql, part, table.columns())?);
    }
    Ok(out)
}

/// All cards of the given notes.
pub fn snapshot_cards_of_notes(db: &Db, note_ids: &[i64]) -> Result<Vec<RowSnapshot>> {
    let cols: Vec<String> = CARD_COLUMNS.iter().map(|c| format!("\"{c}\"")).collect();
    let mut out = Vec::new();
    for part in unique(note_ids).chunks(CHUNK) {
        let sql = format!("SELECT {} FROM cards WHERE nid IN ({})", cols.join(", "), marks(part.len()));
        out.extend(rows_from_query(db, &sql, part, &CARD_COLUMNS)?);
    }
    Ok(out)
}

/// Deletes rows by id, in chunks. Returns the number deleted.
pub fn delete_rows_by_id(db: &Db, table: &str, ids: &[i64]) -> Result<usize> {
    let mut n = 0;
    for part in unique(ids).chunks(CHUNK) {
        n +=
            db.run(&format!("DELETE FROM {table} WHERE id IN ({})", marks(part.len())), rusqlite::params_from_iter(part.iter()))?;
    }
    Ok(n)
}

/// Writes full rows back: UPDATE when the id exists, INSERT otherwise. (Not INSERT OR REPLACE:
/// REPLACE does not fire the notes delete trigger, which would desync `notes_fts`.)
pub fn restore_rows(db: &Db, table: Table, rows: &[RowSnapshot]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let cols = table.columns();
    let rest: Vec<&str> = cols.iter().copied().filter(|c| *c != "id").collect();
    let update = format!(
        "UPDATE {} SET {} WHERE id = ?",
        table.name(),
        rest.iter().map(|c| format!("\"{c}\" = ?")).collect::<Vec<_>>().join(", ")
    );
    let insert = crate::collection::rows::insert_sql(table.name(), cols);
    for row in rows {
        let id = row.get("id").cloned().unwrap_or(Value::Null);
        let mut values: Vec<SqlValue> = rest.iter().map(|c| json_to_sql(row.get(*c).unwrap_or(&Value::Null))).collect();
        values.push(json_to_sql(&id));
        let changed = db.run(&update, rusqlite::params_from_iter(values.iter()))?;
        if changed == 0 {
            let all: Vec<SqlValue> = cols.iter().map(|c| json_to_sql(row.get(*c).unwrap_or(&Value::Null))).collect();
            db.run(&insert, rusqlite::params_from_iter(all.iter()))?;
        }
    }
    Ok(())
}

/// Pushes an undo entry. Must be called inside the operation's transaction so the entry commits
/// (or rolls back) with the change. Returns the entry id.
pub fn record(db: &Db, now_ms: i64, label: &str, snapshot: &UndoSnapshot) -> Result<i64> {
    let payload = serde_json::to_string(snapshot)?;
    db.run("INSERT INTO undo_log (at, label, payload) VALUES (?, ?, ?)", rusqlite::params![now_ms, label, payload])?;
    let id = db.last_insert_rowid();
    db.run("DELETE FROM undo_log WHERE id <= ?", [id - UNDO_CAP])?;
    db.run(&format!("DELETE FROM undo_log WHERE id NOT IN (SELECT id FROM undo_log ORDER BY id DESC LIMIT {UNDO_CAP})"), [])?;
    Ok(id)
}

/// Label of the operation `undo()` would revert, or None.
pub fn peek_label(db: &Db) -> Result<Option<String>> {
    db.scalar("SELECT label FROM undo_log ORDER BY id DESC LIMIT 1", [])
}

/// Number of entries on the stack.
pub fn size(db: &Db) -> Result<i64> {
    db.count("SELECT count(*) FROM undo_log", [])
}

/// Reverts the most recent operation in one transaction. Returns its label, or None when empty.
pub fn undo(db: &Db) -> Result<Option<String>> {
    db.transaction(|| {
        let row = db.get("SELECT id, label, payload FROM undo_log ORDER BY id DESC LIMIT 1", [], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let Some((id, label, payload)) = row else { return Ok(None) };
        let snap: UndoSnapshot = match serde_json::from_str(&payload) {
            Ok(s) => s,
            Err(e) => {
                db.run("DELETE FROM undo_log WHERE id = ?", [id])?;
                return Err(LociError::corrupt(format!("Undo entry \"{label}\" is unreadable")).with_detail(e));
            }
        };
        apply(db, &snap)?;
        db.run("DELETE FROM undo_log WHERE id = ?", [id])?;
        Ok(Some(label))
    })
}

/// Empties the stack (after restore, reset, or a schema-changing operation).
pub fn clear(db: &Db) -> Result<()> {
    db.run("DELETE FROM undo_log", [])?;
    Ok(())
}

fn apply(db: &Db, s: &UndoSnapshot) -> Result<()> {
    // 1. Remove what the operation inserted.
    delete_rows_by_id(db, "revlog", &s.inserted_revlog)?;
    delete_rows_by_id(db, "cards", &s.inserted_cards)?;
    delete_rows_by_id(db, "notes", &s.inserted_notes)?;
    delete_rows_by_id(db, "decks", &s.inserted_decks)?;
    // 2. Restore previous rows.
    restore_rows(db, Table::Decks, &s.decks)?;
    restore_rows(db, Table::Notes, &s.notes)?;
    restore_rows(db, Table::Cards, &s.cards)?;
    restore_rows(db, Table::Revlog, &s.revlog)?;
    for p in &s.presets {
        let changed = db.run(
            "UPDATE deck_configs SET name = ?, config = ?, mtime = ?, usn = ? WHERE id = ?",
            rusqlite::params![p.name, p.config, p.mtime, p.usn, p.id],
        )?;
        if changed == 0 {
            db.run(
                "INSERT INTO deck_configs (id, name, config, mtime, usn) VALUES (?, ?, ?, ?, ?)",
                rusqlite::params![p.id, p.name, p.config, p.mtime, p.usn],
            )?;
        }
    }
    // 3. Drop graves the operation wrote.
    for g in &s.graves {
        db.run(
            "DELETE FROM graves WHERE rowid = (SELECT rowid FROM graves WHERE oid = ? AND type = ? ORDER BY rowid DESC LIMIT 1)",
            rusqlite::params![g.oid, g.gtype],
        )?;
    }
    // 4. Meta values.
    for (key, value) in &s.meta {
        match value.as_str() {
            None => db.run("DELETE FROM col_meta WHERE key = ?", [key])?,
            Some(v) => db.run(
                "INSERT INTO col_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, v],
            )?,
        };
    }
    // 5. Restored notes may carry tags that were unregistered meanwhile.
    for n in &s.notes {
        if let Some(tags) = n.get("tags").and_then(|t| t.as_str()) {
            for tag in split_tags(tags) {
                db.run("INSERT OR IGNORE INTO tags (tag) VALUES (?)", [tag])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        let db = Db::memory().unwrap();
        db.execute_batch(crate::SCHEMA_SQL).unwrap();
        db.run(
            "INSERT INTO cards (id, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) VALUES (1, 1, 1, 0, 0, -1, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, '{}')",
            [],
        )
        .unwrap();
        db
    }

    #[test]
    fn snapshot_restore_and_undo() {
        let db = db();
        let before = snapshot_rows(&db, Table::Cards, &[1, 1, 99]).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0]["due"], 5);
        db.run("UPDATE cards SET due = 9, queue = -1 WHERE id = 1", []).unwrap();
        db.run("INSERT INTO col_meta VALUES ('k', 'new')", []).unwrap();
        let mut meta = Map::new();
        meta.insert("k".into(), Value::Null);
        record(&db, 1000, "Suspend", &UndoSnapshot { cards: before, meta, ..Default::default() }).unwrap();
        assert_eq!(peek_label(&db).unwrap().as_deref(), Some("Suspend"));
        assert_eq!(undo(&db).unwrap().as_deref(), Some("Suspend"));
        assert_eq!(db.scalar::<i64, _>("SELECT due FROM cards WHERE id = 1", []).unwrap(), Some(5));
        assert_eq!(db.count("SELECT count(*) FROM col_meta WHERE key = 'k'", []).unwrap(), 0);
        assert_eq!(undo(&db).unwrap(), None);
    }

    #[test]
    fn inserted_rows_are_deleted_and_the_cap_holds() {
        let db = db();
        db.run("INSERT INTO cards (id, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) VALUES (2, 1, 1, 1, 0, -1, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, '{}')", []).unwrap();
        record(&db, 1, "Add", &UndoSnapshot { inserted_cards: vec![2], ..Default::default() }).unwrap();
        for i in 0..60 {
            record(&db, i, "x", &UndoSnapshot::default()).unwrap();
        }
        assert_eq!(size(&db).unwrap(), UNDO_CAP);
        clear(&db).unwrap();
        assert_eq!(size(&db).unwrap(), 0);
    }

    #[test]
    fn restore_inserts_missing_rows_through_the_fts_trigger() {
        let db = db();
        db.run("INSERT INTO notes (id, guid, ntid, mtime, usn, tags, flds, sfld, csum, plain) VALUES (7, 'g', 1, 0, -1, ' a ', 'hello', 'hello', 0, 'hello')", []).unwrap();
        let snap = snapshot_rows(&db, Table::Notes, &[7]).unwrap();
        db.run("DELETE FROM notes WHERE id = 7", []).unwrap();
        db.run("DELETE FROM tags", []).unwrap();
        record(&db, 1, "Delete", &UndoSnapshot { notes: snap, ..Default::default() }).unwrap();
        undo(&db).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'hello'", []).unwrap(), 1);
        assert_eq!(db.count("SELECT count(*) FROM tags WHERE tag = 'a'", []).unwrap(), 1);
    }
}
