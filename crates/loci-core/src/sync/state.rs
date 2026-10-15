//! Per-collection sync state and the stamping pass (docs/SYNC.md "Stamps", "Change capture").
//!
//! Tables (created on every open, next to the Anki-compatible schema, which is unchanged):
//! - `sync_meta(key, value)`: this device's id, its clock, the folder, the last sync time.
//! - `sync_rows(tbl, key, hlc, dev, hash, deleted)`: the stamp of every synced object, its
//!   payload hash, and tombstones for deleted ones.
//! - `sync_vector(dev, hlc)`: what this collection has seen from other devices.
//! - `sync_peers(dev, vector, wants, seen)`: the last header read from each other device.
//! - `sync_dirty(tbl, key)`: objects written since the last stamping pass (filled by triggers).
//! - `sync_revlog_alias(id, key)`: reviews stored under another id because theirs was taken.

use super::clock::{hlc_next, Stamp, VersionVector};
use super::tables::{self, payload_hash, Ctx, Kind, Payload};
use crate::collection::internals::CollectionCore;
use crate::db::Db;
use crate::error::Result;
use std::collections::HashMap;

pub const TABLES_SQL: &str = "
CREATE TABLE IF NOT EXISTS sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sync_rows (
  tbl TEXT NOT NULL, key TEXT NOT NULL, hlc INTEGER NOT NULL, dev TEXT NOT NULL,
  hash INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (tbl, key)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS ix_sync_rows_dev ON sync_rows (dev, hlc);
CREATE TABLE IF NOT EXISTS sync_vector (dev TEXT PRIMARY KEY, hlc INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sync_peers (dev TEXT PRIMARY KEY, vector TEXT NOT NULL, wants TEXT NOT NULL DEFAULT '[]', seen INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sync_dirty (tbl TEXT NOT NULL, key TEXT NOT NULL, PRIMARY KEY (tbl, key)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS sync_revlog_alias (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE);
";

/// Everything the open path runs: tables, then the change-capture triggers.
pub fn schema_sql() -> String {
    format!("{TABLES_SQL}{}", tables::trigger_sql())
}

/// `sync_meta` keys.
pub mod key {
    pub const DEVICE: &str = "device";
    pub const HLC: &str = "hlc";
    /// "full" (stamp every unstamped or changed object), "pristine" (the same, at time zero) or
    /// "restamp" (give every object a new stamp).
    pub const SCAN: &str = "scan";
    pub const FOLDER: &str = "folder";
    pub const LAST_SYNC: &str = "last_sync";
    /// JSON list of ids this collection used before a restore gave it a new one.
    pub const FORMER: &str = "former";
}

pub fn get(db: &Db, k: &str) -> Result<Option<String>> {
    db.scalar("SELECT value FROM sync_meta WHERE key = ?", [k])
}

pub fn set(db: &Db, k: &str, v: Option<&str>) -> Result<()> {
    match v {
        Some(v) => db.run(
            "INSERT INTO sync_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [k, v],
        )?,
        None => db.run("DELETE FROM sync_meta WHERE key = ?", [k])?,
    };
    Ok(())
}

pub fn device(db: &Db) -> Result<Option<String>> {
    get(db, key::DEVICE)
}

pub fn hlc(db: &Db) -> Result<i64> {
    Ok(get(db, key::HLC)?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// Raises the stored clock to at least `seen`.
pub fn bump_hlc(db: &Db, seen: i64) -> Result<()> {
    if seen > hlc(db)? {
        set(db, key::HLC, Some(&seen.to_string()))?;
    }
    Ok(())
}

/// A fresh random device id (16 hex digits).
pub fn new_device_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

/// Turns sync on for this collection (idempotent): a device id and a first full scan.
pub fn enable(db: &Db) -> Result<String> {
    if let Some(d) = device(db)? {
        return Ok(d);
    }
    let d = new_device_id();
    // A collection with nothing studied stamps its starting rows at time zero, so the first sync
    // adopts the other device's decks, presets and settings instead of resetting them.
    let empty = db.scalar::<i64, _>("SELECT 1 FROM notes UNION ALL SELECT 1 FROM revlog LIMIT 1", [])?.is_none();
    db.transaction(|| {
        set(db, key::DEVICE, Some(&d))?;
        set(db, key::SCAN, Some(if empty { "pristine" } else { "full" }))
    })?;
    Ok(d)
}

pub fn ctx(core: &CollectionCore) -> Result<Ctx> {
    let crt = core.crt()?;
    Ok(Ctx { crt_day: (crt as f64 / 86_400.0).round() as i64, now_s: core.now_seconds() })
}

/// The stamp and hash recorded for one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowState {
    pub stamp: Stamp,
    pub hash: i64,
    pub deleted: bool,
}

pub fn load_row(db: &Db, kind: Kind, k: &str) -> Result<Option<RowState>> {
    db.get("SELECT hlc, dev, hash, deleted FROM sync_rows WHERE tbl = ? AND key = ?", [kind.name(), k], |r| {
        Ok(RowState { stamp: Stamp::new(r.get(0)?, r.get::<_, String>(1)?), hash: r.get(2)?, deleted: r.get::<_, i64>(3)? != 0 })
    })
}

pub fn load_rows(db: &Db, kind: Kind) -> Result<HashMap<String, RowState>> {
    let rows = db.all("SELECT key, hlc, dev, hash, deleted FROM sync_rows WHERE tbl = ?", [kind.name()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            RowState { stamp: Stamp::new(r.get(1)?, r.get::<_, String>(2)?), hash: r.get(3)?, deleted: r.get::<_, i64>(4)? != 0 },
        ))
    })?;
    Ok(rows.into_iter().collect())
}

pub fn store_row(db: &Db, kind: Kind, k: &str, s: &RowState) -> Result<()> {
    db.run(
        "INSERT INTO sync_rows (tbl, key, hlc, dev, hash, deleted) VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(tbl, key) DO UPDATE SET hlc = excluded.hlc, dev = excluded.dev, hash = excluded.hash, deleted = excluded.deleted",
        rusqlite::params![kind.name(), k, s.stamp.hlc, s.stamp.dev, s.hash, s.deleted as i64],
    )?;
    Ok(())
}

/// What this collection has seen: other devices' entries plus its own clock.
pub fn vector(db: &Db) -> Result<VersionVector> {
    let mut v: VersionVector =
        db.all("SELECT dev, hlc FROM sync_vector", [], |r| Ok((r.get(0)?, r.get(1)?)))?.into_iter().collect();
    if let Some(d) = device(db)? {
        v.insert(d, hlc(db)?);
    }
    Ok(v)
}

/// Stores the vector's entries for other devices.
pub fn store_vector(db: &Db, v: &VersionVector) -> Result<()> {
    let own = device(db)?;
    for (d, h) in v {
        if Some(d) == own.as_ref() {
            continue;
        }
        db.run(
            "INSERT INTO sync_vector (dev, hlc) VALUES (?, ?) ON CONFLICT(dev) DO UPDATE SET hlc = max(hlc, excluded.hlc)",
            rusqlite::params![d, h],
        )?;
    }
    Ok(())
}

/// Hands out this device's stamps during one pass.
struct Stamper {
    dev: String,
    hlc: i64,
    now_ms: i64,
    issued: usize,
}

impl Stamper {
    fn next(&mut self) -> Stamp {
        self.hlc = hlc_next(self.hlc, self.now_ms);
        self.issued += 1;
        Stamp::new(self.hlc, self.dev.clone())
    }
}

fn process(
    db: &Db,
    st: &mut Stamper,
    kind: Kind,
    k: &str,
    payload: Option<&Payload>,
    existing: Option<&RowState>,
    force: bool,
) -> Result<()> {
    match payload {
        Some(p) => {
            let hash = payload_hash(p);
            if !force && existing.is_some_and(|e| !e.deleted && e.hash == hash) {
                return Ok(());
            }
            store_row(db, kind, k, &RowState { stamp: st.next(), hash, deleted: false })
        }
        None => match existing {
            Some(e) if !e.deleted => {
                if kind == Kind::Revlog {
                    db.run("DELETE FROM sync_revlog_alias WHERE key = ? AND id NOT IN (SELECT id FROM revlog)", [k])?;
                }
                store_row(db, kind, k, &RowState { stamp: st.next(), hash: 0, deleted: true })
            }
            _ => Ok(()),
        },
    }
}

/// Turns pending changes into stamps: the objects triggers recorded in `sync_dirty`, or every
/// object after enabling sync or a restore. A no-op until sync is enabled. Returns how many
/// stamps were issued.
pub fn stamp_pending(core: &CollectionCore) -> Result<usize> {
    let db = core.db();
    let Some(dev) = device(db)? else { return Ok(0) };
    db.transaction(|| {
        let mut st = Stamper { dev: dev.clone(), hlc: hlc(db)?, now_ms: core.now(), issued: 0 };
        let ctx = ctx(core)?;
        match get(db, key::SCAN)? {
            Some(mode) => {
                let restamp = mode == "restamp";
                if mode == "pristine" {
                    st.now_ms = 0;
                }
                for kind in Kind::ALL {
                    let mut existing = load_rows(db, kind)?;
                    for (k, p) in tables::read_all(db, kind, &ctx)? {
                        let e = existing.remove(&k);
                        process(db, &mut st, kind, &k, Some(&p), e.as_ref(), restamp && kind != Kind::Revlog)?;
                    }
                    let mut gone: Vec<(String, RowState)> = existing.into_iter().collect();
                    gone.sort_by(|a, b| a.0.cmp(&b.0));
                    for (k, e) in gone {
                        process(db, &mut st, kind, &k, None, Some(&e), false)?;
                    }
                }
                set(db, key::SCAN, None)?;
                db.run("DELETE FROM sync_dirty", [])?;
            }
            None => {
                let dirty: Vec<(String, String)> =
                    db.all("SELECT tbl, key FROM sync_dirty ORDER BY tbl, key", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
                if dirty.is_empty() {
                    return Ok(0);
                }
                for (t, k) in &dirty {
                    let Some(kind) = Kind::from_name(t) else { continue };
                    let p = tables::read_one(db, kind, k, &ctx)?;
                    let e = load_row(db, kind, k)?;
                    process(db, &mut st, kind, k, p.as_ref(), e.as_ref(), false)?;
                }
                db.run("DELETE FROM sync_dirty", [])?;
            }
        }
        if st.issued > 0 {
            set(db, key::HLC, Some(&st.hlc.to_string()))?;
        }
        Ok(st.issued)
    })
}

/// Sync state that survives replacing the database (restore, reset).
#[derive(Debug, Clone, Default)]
pub struct Carry {
    pub device: Option<String>,
    pub hlc: i64,
    pub folder: Option<String>,
    pub former: Vec<String>,
}

/// Reads what `after_replace` needs from the database about to be replaced. Never fails.
pub fn carry(db: &Db) -> Carry {
    let has = db.scalar::<i64, _>("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sync_meta'", []).ok().flatten();
    if has.is_none() {
        return Carry::default();
    }
    Carry {
        device: device(db).ok().flatten(),
        hlc: hlc(db).unwrap_or(0),
        folder: get(db, key::FOLDER).ok().flatten(),
        former: get(db, key::FORMER).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default(),
    }
}

/// After the database was replaced by a backup or reset: the collection gets a new device id
/// (the file may be a copy of another device's) and every object a new stamp, so the restored
/// state wins over older changes everywhere. Reviews keep their stamps (they are never edited).
pub fn after_replace(core: &CollectionCore, carry: &Carry) -> Result<()> {
    let db = core.db();
    db.execute_batch(&schema_sql())?;
    let restored = device(db)?;
    if carry.device.is_none() && restored.is_none() {
        return Ok(());
    }
    db.transaction(|| {
        let restored_hlc = hlc(db)?;
        if let Some(d) = &restored {
            // The restored file holds every change its device made up to its clock.
            store_vector(db, &[(d.clone(), restored_hlc)].into_iter().collect())?;
        }
        let mut former = carry.former.clone();
        if let Some(d) = &carry.device {
            if !former.contains(d) {
                former.push(d.clone());
            }
        }
        set(db, key::FORMER, Some(&serde_json::to_string(&former)?))?;
        set(db, key::DEVICE, Some(&new_device_id()))?;
        set(db, key::HLC, Some(&restored_hlc.max(carry.hlc).to_string()))?;
        set(db, key::SCAN, Some("restamp"))?;
        if carry.device.is_some() {
            set(db, key::FOLDER, carry.folder.as_deref())?;
        }
        db.run("DELETE FROM sync_dirty", [])?;
        Ok(())
    })?;
    core.invalidate_caches();
    Ok(())
}
