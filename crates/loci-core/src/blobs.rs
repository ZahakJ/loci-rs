//! `BlobStore`: media bodies and backups.
//!
//! Paths are relative, '/'-separated and never contain '.', '..' or empty segments.
//!
//! Two backends behind one API:
//! - **Directory** (native): files under `root`. `put` is atomic: bytes go to a temporary sibling
//!   that is renamed over the target. Temporary files (name contains `.loci-tmp-`) are never listed.
//! - **SQLite** (wasm32, where there is no file system): one table `blobs(path, data, mtime)` in
//!   `<root>/blobs.sqlite`, opened through SQLite's default VFS (the browser build installs an
//!   OPFS VFS as the default). Each `put` is one transaction, so it is atomic too.

use crate::error::{LociError, Result};
use std::path::{Path, PathBuf};

/// Suffix marker of in-flight temporary files.
pub const TEMP_MARKER: &str = ".loci-tmp-";

/// Splits and validates a blob path.
pub fn blob_segments(path: &str) -> Result<Vec<&str>> {
    let bad = || LociError::invalid(format!("Unsafe blob path: {path:?}"));
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(bad());
    }
    let segs: Vec<&str> = path.split('/').collect();
    for s in &segs {
        if s.is_empty() || *s == "." || *s == ".." {
            return Err(bad());
        }
    }
    Ok(segs)
}

pub fn is_temp_name(name: &str) -> bool {
    name.contains(TEMP_MARKER)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEntry {
    pub path: String,
    pub bytes: u64,
    /// milliseconds since epoch
    pub mtime: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Dir,
    Sqlite,
}

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
    backend: Backend,
}

/// File name of the SQLite blob database inside `root` (SQLite backend).
pub const BLOB_DB_FILE: &str = "blobs.sqlite";

impl BlobStore {
    /// `root` holds all blobs; it is created on first write. Files on native targets, a SQLite
    /// table on wasm32.
    pub fn new(root: impl Into<PathBuf>) -> BlobStore {
        let backend = if cfg!(target_arch = "wasm32") { Backend::Sqlite } else { Backend::Dir };
        BlobStore { root: root.into(), backend }
    }

    /// A store kept in `<root>/blobs.sqlite` (the wasm32 default; usable anywhere).
    pub fn sqlite(root: impl Into<PathBuf>) -> BlobStore {
        BlobStore { root: root.into(), backend: Backend::Sqlite }
    }

    fn sql<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> Result<T>) -> Result<T> {
        sqlite_backend::with(&self.root.join(BLOB_DB_FILE), f)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn full(&self, path: &str) -> Result<PathBuf> {
        let mut p = self.root.clone();
        for seg in blob_segments(path)? {
            p.push(seg);
        }
        Ok(p)
    }

    pub fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if self.backend == Backend::Sqlite {
            blob_segments(path)?;
            return self.sql(|c| sqlite_backend::put(c, path, bytes));
        }
        let target = self.full(path)?;
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = temp_sibling(&target);
        let write = (|| -> std::io::Result<()> {
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &target)
        })();
        if let Err(e) = write {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }

    pub fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if self.backend == Backend::Sqlite {
            blob_segments(path)?;
            return self.sql(|c| sqlite_backend::get(c, path));
        }
        match std::fs::read(self.full(path)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        if self.backend == Backend::Sqlite {
            blob_segments(path)?;
            return self.sql(|c| sqlite_backend::delete(c, path));
        }
        match std::fs::remove_file(self.full(path)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        if self.backend == Backend::Sqlite {
            return blob_segments(path).is_ok() && self.sql(|c| sqlite_backend::exists(c, path)).unwrap_or(false);
        }
        self.full(path).map(|p| p.is_file()).unwrap_or(false)
    }

    /// Absolute path of a blob (for callers that stream files directly, e.g. a media server). With
    /// the SQLite backend this names no real file; it is only a stable key.
    pub fn path_of(&self, path: &str) -> Result<PathBuf> {
        self.full(path)
    }

    /// Lists files whose path starts with `prefix` (e.g. "media/"), recursively, sorted by path.
    pub fn list(&self, prefix: &str) -> Result<Vec<BlobEntry>> {
        if self.backend == Backend::Sqlite {
            return self.sql(|c| sqlite_backend::list(c, prefix));
        }
        let base_rel = match prefix.rfind('/') {
            Some(i) => &prefix[..i],
            None => "",
        };
        let mut out = Vec::new();
        self.walk(base_rel, prefix, &mut out)?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn walk(&self, rel: &str, prefix: &str, out: &mut Vec<BlobEntry>) -> Result<()> {
        let abs =
            if rel.is_empty() { self.root.clone() } else { self.root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)) };
        let entries = match std::fs::read_dir(&abs) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let ft = match ent.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                if child_rel.starts_with(prefix) || prefix.starts_with(&format!("{child_rel}/")) {
                    self.walk(&child_rel, prefix, out)?;
                }
            } else if ft.is_file() && !is_temp_name(&name) && child_rel.starts_with(prefix) {
                if let Ok(md) = ent.metadata() {
                    let mtime = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    out.push(BlobEntry { path: child_rel, bytes: md.len(), mtime });
                }
            }
        }
        Ok(())
    }
}

/// The SQLite backend: one cached connection per database path and thread.
mod sqlite_backend {
    use super::BlobEntry;
    use crate::error::Result;
    use rusqlite::{params, Connection, OptionalExtension};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    thread_local! {
        static OPEN: RefCell<HashMap<PathBuf, Connection>> = RefCell::new(HashMap::new());
    }

    pub fn with<T>(db: &Path, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        OPEN.with(|open| {
            let mut open = open.borrow_mut();
            if !open.contains_key(db) {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(dir) = db.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)?;
                }
                let conn = Connection::open(db)?;
                conn.busy_timeout(std::time::Duration::from_secs(5))?;
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS blobs (path TEXT PRIMARY KEY, data BLOB NOT NULL, mtime INTEGER NOT NULL) WITHOUT ROWID",
                )?;
                open.insert(db.to_path_buf(), conn);
            }
            f(&open[db])
        })
    }

    pub fn put(c: &Connection, path: &str, bytes: &[u8]) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        c.prepare_cached("INSERT OR REPLACE INTO blobs (path, data, mtime) VALUES (?1, ?2, ?3)")?
            .execute(params![path, bytes, now])?;
        Ok(())
    }

    pub fn get(c: &Connection, path: &str) -> Result<Option<Vec<u8>>> {
        Ok(c.prepare_cached("SELECT data FROM blobs WHERE path = ?1")?.query_row([path], |r| r.get(0)).optional()?)
    }

    pub fn delete(c: &Connection, path: &str) -> Result<()> {
        c.prepare_cached("DELETE FROM blobs WHERE path = ?1")?.execute([path])?;
        Ok(())
    }

    pub fn exists(c: &Connection, path: &str) -> Result<bool> {
        Ok(c.prepare_cached("SELECT 1 FROM blobs WHERE path = ?1")?.query_row([path], |_| Ok(())).optional()?.is_some())
    }

    pub fn list(c: &Connection, prefix: &str) -> Result<Vec<BlobEntry>> {
        let mut st =
            c.prepare_cached("SELECT path, length(data), mtime FROM blobs WHERE substr(path, 1, length(?1)) = ?1 ORDER BY path")?;
        let rows =
            st.query_map([prefix], |r| Ok(BlobEntry { path: r.get(0)?, bytes: r.get::<_, i64>(1)? as u64, mtime: r.get(2)? }))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn temp_sibling(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    name.push_str(TEMP_MARKER);
    name.push_str(&format!("{:x}-{:x}", std::process::id(), nanos));
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_paths() {
        for bad in ["", "/abs", "a/../b", "a//b", "./x", "a\\b", "media/."] {
            assert!(blob_segments(bad).is_err(), "{bad}");
        }
        assert_eq!(blob_segments("media/a.png").unwrap(), vec!["media", "a.png"]);
    }

    #[test]
    fn put_get_list_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path());
        store.put("media/a.png", b"aaa").unwrap();
        store.put("media/b.mp3", b"bb").unwrap();
        store.put("backups/x.sqlite", b"c").unwrap();
        std::fs::write(dir.path().join("media").join(format!("junk{TEMP_MARKER}1")), b"t").unwrap();
        assert_eq!(store.get("media/a.png").unwrap().unwrap(), b"aaa");
        assert!(store.get("media/none").unwrap().is_none());
        let names: Vec<String> = store.list("media/").unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(names, vec!["media/a.png", "media/b.mp3"]);
        assert_eq!(store.list("backups/").unwrap().len(), 1);
        assert_eq!(store.list("").unwrap().len(), 3);
        assert!(store.exists("media/a.png"));
        store.delete("media/a.png").unwrap();
        store.delete("media/a.png").unwrap();
        assert!(!store.exists("media/a.png"));
    }

    #[test]
    fn sqlite_backend_put_get_list_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::sqlite(dir.path());
        store.put("media/a.png", b"aaa").unwrap();
        store.put("media/b.mp3", b"bb").unwrap();
        store.put("media/a.png", b"aaaa").unwrap();
        store.put("backups/x.sqlite", b"c").unwrap();
        assert!(store.put("../x", b"no").is_err());
        assert_eq!(store.get("media/a.png").unwrap().unwrap(), b"aaaa");
        assert!(store.get("media/none").unwrap().is_none());
        let listed = store.list("media/").unwrap();
        assert_eq!(listed.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), vec!["media/a.png", "media/b.mp3"]);
        assert_eq!(listed[0].bytes, 4);
        assert!(listed[0].mtime > 0);
        assert_eq!(store.list("").unwrap().len(), 3);
        assert!(store.exists("media/a.png"));
        store.delete("media/a.png").unwrap();
        store.delete("media/a.png").unwrap();
        assert!(!store.exists("media/a.png"));
        // a second handle on the same root sees the same data
        assert_eq!(BlobStore::sqlite(dir.path()).get("backups/x.sqlite").unwrap().unwrap(), b"c");
        assert!(dir.path().join(BLOB_DB_FILE).is_file());
    }
}
