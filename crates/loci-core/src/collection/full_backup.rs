//! Reading Loci's own full-backup zip (`backup::export_full_backup`: `collection.sqlite`,
//! `media/<name>` files and `manifest.json` with format 'loci-backup'), so a downloaded backup can
//! be restored from a file (internals §12).
//!
//! The database image is validated (SQLite header, `PRAGMA quick_check`, a Loci schema) before
//! anything is replaced; `Collection::restore_full_backup` performs the swap (with a pre-restore
//! backup first).

use crate::anki::{looks_like_zip, normalize_media_filename, ZipArchive};
use crate::backup::backup::quick_check_ok;
use crate::collection::internals::CollectionCore;
use crate::collection::media::MEDIA_PREFIX;
use crate::db::{is_sqlite_image, Db};
use crate::error::{LociError, Result};
use crate::text::sha1_hex;
use crate::types::*;
use serde_json::{Map, Value};

pub const FULL_BACKUP_FORMAT: &str = "loci-backup";

pub struct LociBackupContents {
    pub manifest: Map<String, Value>,
    pub database: Vec<u8>,
    /// Media entry names (without the `media/` prefix).
    pub media_names: Vec<String>,
    zip: ZipArchive,
}

impl std::fmt::Debug for LociBackupContents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LociBackupContents")
            .field("media", &self.media_names.len())
            .field("database_bytes", &self.database.len())
            .finish()
    }
}

impl LociBackupContents {
    pub fn read_media(&self, name: &str) -> Result<Vec<u8>> {
        self.zip.read(&format!("{MEDIA_PREFIX}{name}")).map_err(|e| {
            LociError::corrupt(format!("The backup file is damaged: media \"{name}\" cannot be read.")).with_detail(e)
        })
    }
}

/// Opens a Loci full backup; None when the bytes are not one (e.g. an Anki package).
pub fn open_loci_backup(bytes: &[u8]) -> Result<Option<LociBackupContents>> {
    if !looks_like_zip(bytes) {
        return Ok(None);
    }
    let Ok(zip) = ZipArchive::open(bytes.to_vec()) else { return Ok(None) };
    if !zip.has("manifest.json") || !zip.has("collection.sqlite") {
        return Ok(None);
    }
    let manifest: Map<String, Value> = match zip.read("manifest.json").ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    {
        Some(Value::Object(m)) => m,
        _ => return Ok(None),
    };
    if manifest.get("format").and_then(|v| v.as_str()) != Some(FULL_BACKUP_FORMAT) {
        return Ok(None);
    }
    let database = zip
        .read("collection.sqlite")
        .map_err(|e| LociError::corrupt("The backup file is damaged: its collection cannot be read.").with_detail(e))?;
    let media_names: Vec<String> = zip
        .names()
        .into_iter()
        .filter(|n| n.starts_with(MEDIA_PREFIX) && n.len() > MEDIA_PREFIX.len())
        .map(|n| n[MEDIA_PREFIX.len()..].to_string())
        .filter(|n| !n.contains('/') && normalize_media_filename(n) == *n)
        .collect();
    Ok(Some(LociBackupContents { manifest, database, media_names, zip }))
}

/// Counts of a validated database image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseCounts {
    pub notes: i64,
    pub cards: i64,
    pub revlog: i64,
    pub crt: Seconds,
    pub guids: Vec<String>,
}

/// Validates a database image; returns its note/card/revlog counts. `corrupt` when it is not a
/// healthy Loci collection.
pub fn validate_loci_database(bytes: &[u8]) -> Result<DatabaseCounts> {
    if !is_sqlite_image(bytes) {
        return Err(LociError::corrupt("The backup does not contain a database."));
    }
    let db =
        Db::open_from_bytes(bytes).map_err(|e| LociError::corrupt("The backup database cannot be opened.").with_detail(e))?;
    let damaged = |e: LociError| LociError::corrupt("The backup database is damaged.").with_detail(e);
    if !quick_check_ok(&db).map_err(damaged)? {
        return Err(LociError::corrupt("The backup database is damaged."));
    }
    let version: Option<String> = db.scalar("SELECT value FROM col_meta WHERE key = 'schema_version'", []).map_err(damaged)?;
    if version.is_none() {
        return Err(LociError::corrupt("The backup is not a Loci collection."));
    }
    let n = |sql: &str| db.count(sql, []).map_err(damaged);
    let crt: Option<String> = db.scalar("SELECT value FROM col_meta WHERE key = 'crt'", []).map_err(damaged)?;
    Ok(DatabaseCounts {
        notes: n("SELECT count(*) FROM notes")?,
        cards: n("SELECT count(*) FROM cards")?,
        revlog: n("SELECT count(*) FROM revlog")?,
        crt: crt.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as i64,
        guids: db.all("SELECT guid FROM notes", [], |r| r.get(0)).map_err(damaged)?,
    })
}

/// How many of `guids` already exist here, and how many would be new.
pub fn guid_overlap(core: &CollectionCore, guids: &[String]) -> Result<(i64, i64)> {
    let mut unique: Vec<&String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for g in guids {
        if seen.insert(g.as_str()) {
            unique.push(g);
        }
    }
    let mut existing = 0;
    for part in unique.chunks(500) {
        let marks = vec!["?"; part.len()].join(",");
        existing += core.db().count(
            &format!("SELECT count(*) FROM notes WHERE guid IN ({marks})"),
            rusqlite::params_from_iter(part.iter().map(|s| s.as_str())),
        )?;
    }
    Ok((existing, guids.len() as i64 - existing))
}

/// Import preview for a Loci full backup.
pub fn preview_loci_backup(core: &CollectionCore, b: &LociBackupContents) -> Result<ImportPreview> {
    let counts = validate_loci_database(&b.database)?;
    let (notes_existing, notes_new) = guid_overlap(core, &counts.guids)?;
    Ok(ImportPreview {
        layout: PackageLayout::Latest,
        is_collection_package: true,
        notes: counts.notes,
        cards: counts.cards,
        decks: vec![],
        notetypes: vec![],
        reviews: counts.revlog,
        media: b.media_names.len() as i64,
        has_scheduling: counts.revlog > 0,
        fsrs_cards: 0,
        source_crt: counts.crt,
        warnings: vec!["This is a Loci full backup: it can only replace the whole collection.".to_string()],
        notes_existing,
        notes_new,
    })
}

/// Writes the backup's media files that are missing or different here. Returns how many were written.
pub fn restore_backup_media(core: &CollectionCore, b: &LociBackupContents) -> Result<i64> {
    let mut written = 0;
    for name in &b.media_names {
        let bytes = b.read_media(name)?;
        let path = format!("{MEDIA_PREFIX}{name}");
        if let Some(existing) = core.blobs.get(&path)? {
            if existing.len() == bytes.len() && sha1_hex(&existing) == sha1_hex(&bytes) {
                continue;
            }
        }
        core.blobs.put(&path, &bytes)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anki::{create_zip, ZipInput};
    use crate::collection::service::test_support::*;
    use crate::error::ErrorCode;

    #[test]
    fn opens_previews_and_restores_a_full_backup() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        let kept = h.add_note(&basic, 1, &["kept", "x"], &[]);
        h.col.add_media("pic.png", &[1, 2, 3]).unwrap();
        let zip = h.col.export_full_backup(&mut |_| {}).unwrap();
        let b = open_loci_backup(&zip).unwrap().expect("a loci backup");
        assert_eq!(b.media_names, ["pic.png"]);
        assert_eq!(b.manifest["format"], FULL_BACKUP_FORMAT);
        assert_eq!(b.read_media("pic.png").unwrap(), vec![1, 2, 3]);
        let counts = validate_loci_database(&b.database).unwrap();
        assert_eq!((counts.notes, counts.cards, counts.revlog), (1, 1, 0));
        assert_eq!(counts.crt, h.meta("crt").unwrap().parse::<i64>().unwrap());
        // Change the live collection, then preview and restore.
        h.add_note(&basic, 1, &["later", "y"], &[]);
        h.col.delete_media(&strs(&["pic.png"])).unwrap();
        let preview = preview_loci_backup(h.col.core(), &b).unwrap();
        assert_eq!((preview.notes_existing, preview.notes_new, preview.media, preview.is_collection_package), (1, 0, 1, true));
        assert_eq!(preview.layout, PackageLayout::Latest);
        let merge = ImportOptions { mode: ImportMode::Merge, ..Default::default() };
        assert_eq!(h.col.restore_full_backup(&b, &merge, &mut |_| {}).unwrap_err().code, ErrorCode::Invalid);
        let mut phases = Vec::new();
        let replace = ImportOptions { mode: ImportMode::Replace, ..Default::default() };
        let report = h.col.restore_full_backup(&b, &replace, &mut |p| phases.push(p.phase)).unwrap();
        assert_eq!((report.notes_added, report.cards_added, report.media_added), (1, 1, 1));
        assert_eq!(phases, ["read", "media", "collection", "done"]);
        assert_eq!(h.count("SELECT count(*) FROM notes"), 1);
        assert!(h.col.get_note(kept.note_id).is_ok());
        assert_eq!(h.col.get_media("pic.png").unwrap(), Some(vec![1, 2, 3]));
        assert!(h.col.list_backups().unwrap().iter().any(|x| x.kind == BackupKind::PreRestore));
        assert_eq!(h.col.undo_label().unwrap(), None);
    }

    #[test]
    fn rejects_what_is_not_a_loci_backup() {
        assert!(open_loci_backup(b"plain bytes").unwrap().is_none());
        let other = create_zip([ZipInput::new("collection.anki2", vec![0u8; 10], false)]).unwrap();
        assert!(open_loci_backup(&other).unwrap().is_none());
        let wrong_format = create_zip([
            ZipInput::new("manifest.json", br#"{"format":"anki"}"#.to_vec(), false),
            ZipInput::new("collection.sqlite", vec![0u8; 10], false),
        ])
        .unwrap();
        assert!(open_loci_backup(&wrong_format).unwrap().is_none());
        let bad_db = create_zip([
            ZipInput::new("manifest.json", br#"{"format":"loci-backup"}"#.to_vec(), false),
            ZipInput::new("collection.sqlite", vec![0u8; 200], false),
            ZipInput::new("media/../evil", vec![1u8], false),
            ZipInput::new("media/sub/x.png", vec![1u8], false),
        ])
        .unwrap();
        let b = open_loci_backup(&bad_db).unwrap().unwrap();
        assert!(b.media_names.is_empty());
        assert_eq!(validate_loci_database(&b.database).unwrap_err().code, ErrorCode::Corrupt);
        // A healthy SQLite file that is not a Loci collection.
        let db = Db::memory().unwrap();
        db.execute_batch("CREATE TABLE t (x)").unwrap();
        let err = validate_loci_database(&db.serialize().unwrap()).unwrap_err();
        assert_eq!(err.code, ErrorCode::Corrupt);
    }
}
