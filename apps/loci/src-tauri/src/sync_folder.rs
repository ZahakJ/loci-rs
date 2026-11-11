//! Which files the webview's folder sync may touch (docs/SYNC.md).
//!
//! The webview names a folder and a path inside it, but the webview renders imported content, so
//! the shell does not take its word for the folder: it accepts only the folder the user picked in
//! the native dialog (`pick_sync_folder`, remembered in `<data dir>/sync-folder`), only paths
//! under that folder's `loci-sync/` directory, and never through a symbolic link inside it.

use loci_core::LociError;
use std::path::{Path, PathBuf};

/// The file in the app's data directory that remembers the picked folder.
pub const REMEMBERED: &str = "sync-folder";

/// The folder the user picked last, if any.
pub fn load_picked(data_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data_dir.join(REMEMBERED)).ok()?;
    let path = PathBuf::from(text.trim_end_matches(['\n', '\r']));
    path.is_absolute().then_some(path)
}

/// Remembers the folder the user picked in the native dialog.
pub fn remember_picked(data_dir: &Path, folder: &Path) -> std::io::Result<()> {
    let text = folder.to_str().ok_or_else(|| std::io::Error::other("The folder's name is not valid Unicode"))?;
    std::fs::write(data_dir.join(REMEMBERED), format!("{text}\n"))
}

/// The file or directory `path` names inside `root`, when `root` is the picked folder and `path`
/// stays inside its `loci-sync/` directory.
pub fn resolve(picked: Option<&Path>, root: &str, path: &[String]) -> Result<PathBuf, LociError> {
    let Some(picked) = picked else {
        return Err(LociError::invalid("No sync folder has been chosen on this device; choose one in Settings › Sync"));
    };
    if !Path::new(root).is_absolute() || Path::new(root) != picked {
        return Err(LociError::invalid("This is not the sync folder chosen on this device; choose it again in Settings › Sync"));
    }
    if path.first().map(String::as_str) != Some("loci-sync") {
        return Err(LociError::invalid("Sync files live in the loci-sync folder"));
    }
    let mut p = picked.to_path_buf();
    for seg in path {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains(['/', '\\', '\0', ':']) {
            return Err(LociError::invalid(format!("Unusable name in the sync folder: {seg:?}")));
        }
        p.push(seg);
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(LociError::invalid(format!("The sync folder holds a link where a file was expected: {seg:?}")));
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loci-sync-folder-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_the_picked_folder() {
        let picked = Path::new("/srv/sync/Loci");
        assert_eq!(
            resolve(Some(picked), "/srv/sync/Loci", &p(&["loci-sync", "c1", "d1.loci-sync"])).unwrap(),
            PathBuf::from("/srv/sync/Loci/loci-sync/c1/d1.loci-sync")
        );
        assert!(resolve(Some(picked), "/srv/sync/Loci/", &p(&["loci-sync"])).is_ok());
        for other in ["/", "/home/user", "/srv/sync", "/srv/sync/Loci/loci-sync", "relative/Loci", ""] {
            assert!(resolve(Some(picked), other, &p(&["loci-sync", "x"])).is_err(), "{other}");
        }
        assert!(resolve(None, "/srv/sync/Loci", &p(&["loci-sync", "x"])).is_err());
    }

    #[test]
    fn only_inside_loci_sync() {
        let picked = Path::new("/srv/sync/Loci");
        let ok = |path: &[&str]| resolve(Some(picked), "/srv/sync/Loci", &p(path)).is_ok();
        assert!(!ok(&[]));
        assert!(!ok(&["notes.txt"]));
        assert!(!ok(&["loci-sync", ".."]));
        assert!(!ok(&["loci-sync", "..", "..", "etc"]));
        assert!(!ok(&["loci-sync", "a/../../b"]));
        assert!(!ok(&["loci-sync", "a\\b"]));
        assert!(!ok(&["loci-sync", "C:x"]));
        assert!(!ok(&["loci-sync", ""]));
        assert!(!ok(&["loci-sync", "a\0b"]));
        assert!(ok(&["loci-sync", "c1", "media", "0123abcd"]));
    }

    #[cfg(unix)]
    #[test]
    fn not_through_a_link() {
        let base = scratch();
        let picked = base.join("folder");
        let outside = base.join("outside");
        std::fs::create_dir_all(picked.join("loci-sync")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, picked.join("loci-sync").join("c1")).unwrap();
        let root = picked.to_str().unwrap();
        assert!(resolve(Some(&picked), root, &p(&["loci-sync", "c1", "d.loci-sync"])).is_err());
        assert!(resolve(Some(&picked), root, &p(&["loci-sync", "c2", "d.loci-sync"])).is_ok());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn remembers_the_pick() {
        let dir = scratch();
        assert_eq!(load_picked(&dir), None);
        let folder = dir.join("My Sync");
        remember_picked(&dir, &folder).unwrap();
        assert_eq!(load_picked(&dir), Some(folder));
        std::fs::write(dir.join(REMEMBERED), "relative/path\n").unwrap();
        assert_eq!(load_picked(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
