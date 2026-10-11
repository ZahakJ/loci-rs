//! `BlobStore`: media bodies and backups as files in a directory (recall `src/core/db/blob-node.ts`).
//!
//! Paths are relative, '/'-separated and never contain '.', '..' or empty segments. `put` is
//! atomic: bytes go to a temporary sibling that is renamed over the target. Temporary files
//! (name contains `.loci-tmp-`) are never listed.

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

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// `root` holds all blobs; it is created on first write.
    pub fn new(root: impl Into<PathBuf>) -> BlobStore {
        BlobStore { root: root.into() }
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
        match std::fs::read(self.full(path)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::IsADirectory => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        match std::fs::remove_file(self.full(path)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.full(path).map(|p| p.is_file()).unwrap_or(false)
    }

    /// Absolute path of a blob (for callers that stream files directly, e.g. a media server).
    pub fn path_of(&self, path: &str) -> Result<PathBuf> {
        self.full(path)
    }

    /// Lists files whose path starts with `prefix` (e.g. "media/"), recursively, sorted by path.
    pub fn list(&self, prefix: &str) -> Result<Vec<BlobEntry>> {
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
}
