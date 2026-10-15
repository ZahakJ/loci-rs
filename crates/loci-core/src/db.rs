//! `Db`: the SQLite connection the engine works on.
//!
//! A thin layer over `rusqlite::Connection` that gives sibling modules
//! nested-transaction joining (an inner `transaction` call joins the outer
//! one; an error propagates to the outermost, which rolls back), serialisation to bytes
//! (backups, exports) and opening from bytes (package reading, restore validation).
//!
//! Anki's `unicase` collation is registered on every connection, so schema-18 package databases
//! open unchanged.

use crate::error::{LociError, Result};
use rusqlite::{Connection, OpenFlags, MAIN_DB};
use std::cell::Cell;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// The 16-byte magic string every SQLite 3 database file starts with.
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// True when `bytes` starts with the SQLite 3 file-format magic header.
pub fn is_sqlite_image(bytes: &[u8]) -> bool {
    bytes.len() >= 100 && &bytes[..16] == SQLITE_MAGIC
}

/// Forces a database image out of WAL mode (header bytes 18/19 → 1), as a lone file without its
/// `-wal` cannot use WAL. Returns a copy only when a change is needed.
pub fn without_wal_header(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if bytes.len() < 20 || (bytes[18] != 2 && bytes[19] != 2) {
        return std::borrow::Cow::Borrowed(bytes);
    }
    let mut copy = bytes.to_vec();
    copy[18] = 1;
    copy[19] = 1;
    std::borrow::Cow::Owned(copy)
}

/// Unicode-aware case-insensitive comparison approximating Anki's `unicase` collation
/// (full case folding, then code-point order).
pub fn unicase_compare(a: &str, b: &str) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    let fa = fold_case(a);
    let fb = fold_case(b);
    fa.chars().cmp(fb.chars())
}

fn fold_case(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        s.to_uppercase().to_lowercase()
    }
}

pub struct Db {
    conn: Connection,
    depth: Cell<u32>,
    /// File path when opened from disk (None for memory/bytes).
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("path", &self.path).field("depth", &self.depth.get()).finish()
    }
}

impl Db {
    fn configure(conn: Connection) -> Result<Connection> {
        conn.create_collation("unicase", unicase_compare)?;
        // `power()` for the retrievability column/sort (the bundled SQLite lacks the math functions).
        crate::search::register_sql_functions(&conn)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    /// Opens (creating if needed) a database file. Parent directories are created.
    pub fn open(path: impl AsRef<Path>) -> Result<Db> {
        let path = path.as_ref().to_path_buf();
        // wasm32 has no file system: the path names a file in SQLite's default VFS.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Db { conn: Self::configure(conn)?, depth: Cell::new(0), path: Some(path) })
    }

    /// A private in-memory database.
    pub fn memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        Ok(Db { conn: Self::configure(conn)?, depth: Cell::new(0), path: None })
    }

    /// Opens a private, writable, in-memory copy of a database image.
    pub fn open_from_bytes(bytes: &[u8]) -> Result<Db> {
        if !is_sqlite_image(bytes) {
            return Err(LociError::corrupt("Not an SQLite database"));
        }
        let fixed = without_wal_header(bytes);
        let mut conn = Connection::open_in_memory()?;
        conn.deserialize_read_exact(MAIN_DB, Cursor::new(&fixed[..]), fixed.len(), false)
            .map_err(|e| LociError::corrupt(format!("Cannot open database image: {e}")))?;
        Ok(Db { conn: Self::configure(conn)?, depth: Cell::new(0), path: None })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The raw connection, for prepared statements and queries.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    /// Runs a statement with parameters; returns the number of changed rows.
    pub fn run<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<usize> {
        let mut st = self.conn.prepare_cached(sql)?;
        Ok(st.execute(params)?)
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.conn.last_insert_rowid()
    }

    /// Fetches one row mapped by `f`; `None` when the query returns nothing.
    pub fn get<T, P, F>(&self, sql: &str, params: P, f: F) -> Result<Option<T>>
    where
        P: rusqlite::Params,
        F: FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    {
        let mut st = self.conn.prepare_cached(sql)?;
        let mut rows = st.query(params)?;
        match rows.next()? {
            Some(row) => Ok(Some(f(row)?)),
            None => Ok(None),
        }
    }

    /// Fetches all rows mapped by `f`.
    pub fn all<T, P, F>(&self, sql: &str, params: P, mut f: F) -> Result<Vec<T>>
    where
        P: rusqlite::Params,
        F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    {
        let mut st = self.conn.prepare_cached(sql)?;
        let mut rows = st.query(params)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(f(row)?);
        }
        Ok(out)
    }

    /// A single scalar (first column of the first row).
    pub fn scalar<T: rusqlite::types::FromSql, P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Option<T>> {
        self.get(sql, params, |r| r.get(0))
    }

    /// `count(*)`-style query: 0 when there is no row.
    pub fn count<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<i64> {
        Ok(self.scalar::<i64, _>(sql, params)?.unwrap_or(0))
    }

    /// Runs `f` inside `BEGIN IMMEDIATE` / `COMMIT`, rolling back on error. Nested calls join
    /// the outer transaction.
    pub fn transaction<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        if self.depth.get() > 0 {
            self.depth.set(self.depth.get() + 1);
            let out = f();
            self.depth.set(self.depth.get() - 1);
            return out;
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        self.depth.set(1);
        let out = f();
        self.depth.set(0);
        match out {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                if !self.conn.is_autocommit() {
                    let _ = self.conn.execute_batch("ROLLBACK");
                }
                Err(e)
            }
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.depth.get() > 0
    }

    /// Serialises the whole database to bytes (for backups and export), WAL flag cleared.
    pub fn serialize(&self) -> Result<Vec<u8>> {
        if self.depth.get() > 0 {
            return Err(LociError::busy("Cannot serialise the database inside a transaction"));
        }
        let data = self.conn.serialize(MAIN_DB)?;
        Ok(without_wal_header(&data).into_owned())
    }

    pub fn close(self) -> Result<()> {
        self.conn.close().map_err(|(_, e)| LociError::from(e))
    }

    /// Atomically replaces the database file at `path` with `bytes` and opens it. Every
    /// connection to `path` must be closed first. Native: temp file + rename. wasm32 (no file
    /// system): `replace_contents`.
    pub fn replace_file(path: impl AsRef<Path>, bytes: &[u8]) -> Result<Db> {
        #[cfg(target_arch = "wasm32")]
        return Db::replace_contents(path, bytes);
        #[cfg(not(target_arch = "wasm32"))]
        Db::replace_file_by_rename(path.as_ref(), bytes)
    }

    /// Replaces the contents of the database at `path` with `bytes` through SQLite's backup API, in
    /// one write transaction of the destination (a crash leaves the old contents, rolled back from
    /// the journal), and returns the open database. Works in any VFS.
    pub fn replace_contents(path: impl AsRef<Path>, bytes: &[u8]) -> Result<Db> {
        let src = Db::open_from_bytes(bytes)?;
        let mut dst = Db::open(path)?;
        {
            let backup = rusqlite::backup::Backup::new(&src.conn, &mut dst.conn)?;
            match backup.step(-1)? {
                rusqlite::backup::StepResult::Done => {}
                _ => return Err(LociError::busy("The database is in use and cannot be replaced")),
            }
        }
        src.close()?;
        Ok(dst)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn replace_file_by_rename(path: &Path, bytes: &[u8]) -> Result<Db> {
        if !is_sqlite_image(bytes) {
            return Err(LociError::corrupt("Not an SQLite database"));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), chrono::Utc::now().timestamp_millis()));
        std::fs::write(&tmp, without_wal_header(bytes))?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut side = path.as_os_str().to_owned();
            side.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(side));
        }
        std::fs::rename(&tmp, path)?;
        Db::open(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_transactions_join_and_roll_back_together() {
        let db = Db::memory().unwrap();
        db.execute_batch("CREATE TABLE t (x)").unwrap();
        let r: Result<()> = db.transaction(|| {
            db.run("INSERT INTO t VALUES (1)", [])?;
            db.transaction(|| {
                db.run("INSERT INTO t VALUES (2)", [])?;
                Err(LociError::invalid("boom"))
            })
        });
        assert!(r.is_err());
        assert_eq!(db.count("SELECT count(*) FROM t", []).unwrap(), 0);
        db.transaction(|| db.run("INSERT INTO t VALUES (3)", []).map(|_| ())).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM t", []).unwrap(), 1);
    }

    #[test]
    fn serialize_round_trips_through_open_from_bytes() {
        let db = Db::memory().unwrap();
        db.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (42)").unwrap();
        let bytes = db.serialize().unwrap();
        assert!(is_sqlite_image(&bytes));
        let copy = Db::open_from_bytes(&bytes).unwrap();
        assert_eq!(copy.scalar::<i64, _>("SELECT x FROM t", []).unwrap(), Some(42));
        copy.run("INSERT INTO t VALUES (43)", []).unwrap();
        assert_eq!(copy.count("SELECT count(*) FROM t", []).unwrap(), 2);
        assert!(Db::open_from_bytes(b"not a database at all, far too short").is_err());
    }

    #[test]
    fn fts5_and_unicase_are_available() {
        let db = Db::memory().unwrap();
        db.execute_batch("CREATE VIRTUAL TABLE f USING fts5(a, tokenize='trigram case_sensitive 0')").unwrap();
        db.execute_batch("CREATE TABLE u (name TEXT COLLATE unicase)").unwrap();
        db.run("INSERT INTO u VALUES ('Straße')", []).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM u WHERE name = 'STRASSE'", []).unwrap(), 1);
    }

    #[test]
    fn replace_contents_swaps_the_database_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.sqlite");
        let live = Db::open(&path).unwrap();
        live.execute_batch("CREATE TABLE old (x); INSERT INTO old VALUES (1)").unwrap();
        live.close().unwrap();
        let image = Db::memory().unwrap();
        image.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (7), (8)").unwrap();
        let bytes = image.serialize().unwrap();
        let db = Db::replace_contents(&path, &bytes).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM t", []).unwrap(), 2);
        assert!(db.count("SELECT count(*) FROM old", []).is_err());
        db.close().unwrap();
        let reopened = Db::open(&path).unwrap();
        assert_eq!(reopened.scalar::<i64, _>("SELECT max(x) FROM t", []).unwrap(), Some(8));
        assert!(Db::replace_contents(&path, b"garbage").is_err());
    }

    #[test]
    fn wal_header_is_cleared() {
        let mut bytes = vec![0u8; 100];
        bytes[..16].copy_from_slice(SQLITE_MAGIC);
        bytes[18] = 2;
        bytes[19] = 2;
        let fixed = without_wal_header(&bytes);
        assert_eq!((fixed[18], fixed[19]), (1, 1));
    }
}
