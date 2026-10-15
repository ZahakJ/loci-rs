//! Media files: bodies in the BlobStore under
//! `media/<name>`, indexed by the `media` table (name, sha1, size, mtime). Names follow Anki's
//! normalisation (`anki::media_names`).

use crate::anki::{
    add_hash_suffix, extract_media_references, extract_template_media_references, normalize_media_filename, percent_decode,
};
use crate::collection::internals::CollectionCore;
use crate::error::{LociError, Result};
use crate::text::sha1_hex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

pub const MEDIA_PREFIX: &str = "media/";

fn blob_path(name: &str) -> String {
    format!("{MEDIA_PREFIX}{name}")
}

/// `checkMedia` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MediaCheck {
    pub unused: Vec<String>,
    pub missing: Vec<String>,
}

/// Media references found in notes (fields) and note types (templates, CSS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReferencedMedia {
    pub from_notes: BTreeSet<String>,
    pub from_templates: BTreeSet<String>,
}

/// Stores bytes and returns the name to reference in a field. Identical content already stored
/// (same sha1) returns the existing name; a name taken by different content gets Anki's `-<sha1>`
/// stem suffix.
pub fn add_media(core: &CollectionCore, raw_name: &str, bytes: &[u8]) -> Result<String> {
    let normalised = normalize_media_filename(raw_name);
    if normalised.is_empty() {
        return Err(LociError::media(format!("Unusable media filename: {raw_name:?}")));
    }
    let sha1 = sha1_hex(bytes);
    let same: Option<String> = core
        .db()
        .scalar("SELECT name FROM media WHERE sha1 = ? ORDER BY name = ? DESC LIMIT 1", rusqlite::params![sha1, normalised])?;
    if let Some(same) = same {
        if core.blobs.exists(&blob_path(&same)) {
            return Ok(same);
        }
    }
    let mut name = normalised.clone();
    let taken = |n: &str| -> Result<bool> {
        Ok(core.db().scalar::<i64, _>("SELECT 1 FROM media WHERE name = ?", [n])?.is_some() || core.blobs.exists(&blob_path(n)))
    };
    if taken(&name)? {
        name = add_hash_suffix(&normalised, &sha1);
        // The suffixed name holds this exact content by construction unless a stale row disagrees.
        let row: Option<String> = core.db().scalar("SELECT sha1 FROM media WHERE name = ?", [&name])?;
        if row.as_deref() == Some(sha1.as_str()) && core.blobs.exists(&blob_path(&name)) {
            return Ok(name);
        }
    }
    core.blobs.put(&blob_path(&name), bytes)?;
    core.db().transaction(|| {
        core.db().run(
            "INSERT INTO media (name, sha1, size, mtime) VALUES (?, ?, ?, ?) ON CONFLICT(name) DO UPDATE SET sha1 = excluded.sha1, size = excluded.size, mtime = excluded.mtime",
            rusqlite::params![name, sha1, bytes.len() as i64, core.now_seconds()],
        )?;
        core.mark_changed()
    })?;
    Ok(name)
}

pub fn get_media(core: &CollectionCore, name: &str) -> Result<Option<Vec<u8>>> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Ok(None);
    }
    core.blobs.get(&blob_path(name))
}

pub fn delete_media(core: &CollectionCore, names: &[String]) -> Result<()> {
    for name in names {
        if name.is_empty() || name.contains('/') {
            continue;
        }
        core.blobs.delete(&blob_path(name))?;
    }
    core.db().transaction(|| {
        for name in names {
            core.db().run("DELETE FROM media WHERE name = ?", [name])?;
        }
        core.mark_changed()
    })
}

/// All media references in notes (fields) and note types (templates, CSS).
pub fn referenced_media(core: &CollectionCore) -> Result<ReferencedMedia> {
    let mut out = ReferencedMedia::default();
    let flds: Vec<String> =
        core.db().all("SELECT flds FROM notes WHERE flds LIKE '%<%' OR flds LIKE '%[sound:%'", [], |r| r.get(0))?;
    for f in &flds {
        out.from_notes.extend(extract_media_references(f));
    }
    let templates: Vec<(String, String)> =
        core.db().all("SELECT qfmt, afmt FROM templates", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    for (q, a) in &templates {
        out.from_templates.extend(extract_template_media_references(&format!("{q}{a}")));
    }
    let css: Vec<String> = core.db().all("SELECT css FROM notetypes", [], |r| r.get(0))?;
    for c in &css {
        out.from_templates.extend(extract_template_media_references(c));
    }
    Ok(out)
}

/// Reconciles the media table with the files present (adds rows for untracked files, drops rows
/// whose file is gone), then reports files nothing references (`_`-prefixed shared files are never
/// reported, as in Anki) and references with no file.
pub fn check_media(core: &CollectionCore) -> Result<MediaCheck> {
    let files: Vec<(String, String, i64)> = core
        .blobs
        .list(MEDIA_PREFIX)?
        .into_iter()
        .map(|f| (f.path[MEDIA_PREFIX.len()..].to_string(), f.path, f.mtime))
        .filter(|(name, _, _)| !name.is_empty() && !name.contains('/'))
        .collect();
    let present: BTreeSet<String> = files.iter().map(|(n, _, _)| n.clone()).collect();
    let rows: HashMap<String, ()> =
        core.db().all("SELECT name FROM media", [], |r| r.get::<_, String>(0))?.into_iter().map(|n| (n, ())).collect();
    let mut untracked: Vec<(String, String, i64, i64)> = Vec::new();
    for (name, path, mtime) in &files {
        if rows.contains_key(name) {
            continue;
        }
        if let Some(bytes) = core.blobs.get(path)? {
            untracked.push((name.clone(), sha1_hex(&bytes), bytes.len() as i64, mtime.div_euclid(1000)));
        }
    }
    core.db().transaction(|| {
        for (name, sha1, size, mtime) in &untracked {
            core.db().run(
                "INSERT OR REPLACE INTO media (name, sha1, size, mtime) VALUES (?, ?, ?, ?)",
                rusqlite::params![name, sha1, size, mtime],
            )?;
        }
        for name in rows.keys() {
            if !present.contains(name) {
                core.db().run("DELETE FROM media WHERE name = ?", [name])?;
            }
        }
        Ok(())
    })?;

    let refs = referenced_media(core)?;
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    for r in refs.from_notes.iter().chain(refs.from_templates.iter()) {
        let decoded = percent_decode(r);
        let candidates = [r.clone(), decoded.clone(), normalize_media_filename(&decoded)];
        match candidates.iter().find(|c| !c.is_empty() && present.contains(*c)) {
            Some(hit) => {
                used.insert(hit.clone());
            }
            None => {
                if refs.from_notes.contains(r) {
                    missing.insert(decoded);
                }
            }
        }
    }
    let unused: Vec<String> = present.iter().filter(|n| !used.contains(*n) && !n.starts_with('_')).cloned().collect();
    Ok(MediaCheck { unused, missing: missing.into_iter().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    fn b(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn stores_dedupes_by_sha1_and_suffixes_conflicting_names() {
        let h = make_collection();
        let n1 = h.col.add_media("cat?.png", &b("meow")).unwrap();
        assert_eq!(n1, "cat.png");
        assert_eq!(h.col.get_media("cat.png").unwrap(), Some(b("meow")));
        assert_eq!(h.col.add_media("other-name.png", &b("meow")).unwrap(), "cat.png");
        let n3 = h.col.add_media("cat.png", &b("woof")).unwrap();
        assert_eq!(n3, format!("cat-{}.png", sha1_hex(&b("woof"))));
        assert_eq!(h.col.add_media("cat.png", &b("woof")).unwrap(), n3);
        assert_eq!(h.count("SELECT count(*) FROM media"), 2);
        let row: (String, i64) = h
            .col
            .core()
            .db()
            .get("SELECT sha1, size FROM media WHERE name = ?", ["cat.png"], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .unwrap();
        assert_eq!(row, (sha1_hex(&b("meow")), 4));
        assert_eq!(h.col.add_media("", &b("x")).unwrap_err().code, ErrorCode::Media);
        assert_eq!(h.col.get_media("../x").unwrap(), None);
        assert_eq!(h.col.get_media("nope.png").unwrap(), None);
    }

    #[test]
    fn check_media_reports_unused_files_and_missing_references_and_delete_media_removes() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.col.add_media("used.png", &b("1")).unwrap();
        h.col.add_media("unused.png", &b("2")).unwrap();
        h.col.add_media("_font.woff2", &b("3")).unwrap();
        h.col.add_media("my file.mp3", &b("4")).unwrap();
        h.blobs().put("media/untracked.png", &b("5")).unwrap();
        h.add_note(&basic, 1, &["<img src=\"used.png\"> [sound:my%20file.mp3]", "<img src=\"gone.jpg\">"], &[]);
        let r = h.col.check_media().unwrap();
        assert_eq!(r, MediaCheck { unused: strs(&["untracked.png", "unused.png"]), missing: strs(&["gone.jpg"]) });
        assert_eq!(h.count("SELECT count(*) FROM media WHERE name = 'untracked.png'"), 1);
        h.col.delete_media(&strs(&["unused.png"])).unwrap();
        assert_eq!(h.col.get_media("unused.png").unwrap(), None);
        assert_eq!(h.count("SELECT count(*) FROM media WHERE name = 'unused.png'"), 0);
        // A row whose file vanished is dropped by the next check.
        h.blobs().delete("media/_font.woff2").unwrap();
        h.col.check_media().unwrap();
        assert_eq!(h.count("SELECT count(*) FROM media WHERE name = '_font.woff2'"), 0);
    }

    #[test]
    fn referenced_media_covers_fields_templates_and_css() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["<img src=\"a.png\">", "[sound:b.mp3]"], &[]);
        h.col
            .core()
            .db()
            .run("UPDATE notetypes SET css = '.card { background: url(_bg.png); }' WHERE id = ?", [basic.id])
            .unwrap();
        h.col
            .core()
            .db()
            .run("UPDATE templates SET qfmt = '<img src=\"_logo.svg\">{{Front}}' WHERE ntid = ?", [basic.id])
            .unwrap();
        let r = referenced_media(h.col.core()).unwrap();
        assert_eq!(r.from_notes, ["a.png", "b.mp3"].into_iter().map(String::from).collect());
        assert_eq!(r.from_templates, ["_bg.png", "_logo.svg"].into_iter().map(String::from).collect());
    }
}
