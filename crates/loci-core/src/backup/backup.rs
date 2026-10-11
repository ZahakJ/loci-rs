//! Automatic and manual backups in the BlobStore under `backups/` (DESIGN.md §14; recall
//! `src/core/backup/backup.ts`): listing with card counts, the 30-minute rule, and the full-backup zip.

use crate::anki::{create_zip, ZipInput};
use crate::backup::rotation::{
    backup_file_name, parse_backup_name, plan_rotation, should_backup, BackupFileKind, BackupLimits, ParsedBackupName,
};
use crate::collection::internals::{meta, CollectionCore};
use crate::collection::media::MEDIA_PREFIX;
use crate::collection::schema::SCHEMA_VERSION;
use crate::db::{is_sqlite_image, Db};
use crate::error::{LociError, Result};
use crate::types::{BackupInfo, BackupKind, Progress, Seconds};
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

pub const BACKUP_PREFIX: &str = "backups/";

fn backup_path(name: &str) -> String {
    format!("{BACKUP_PREFIX}{name}")
}

/// Card counts per backup file (keyed by absolute path; backups are immutable). The TypeScript kept
/// a WeakMap per core; a process-wide map keyed by path is the same thing without a core field.
static CARD_COUNT_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<i64>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_key(core: &CollectionCore, name: &str) -> Option<PathBuf> {
    core.blobs.path_of(&backup_path(name)).ok()
}

fn cached_count(core: &CollectionCore, name: &str) -> Option<Option<i64>> {
    let key = cache_key(core, name)?;
    CARD_COUNT_CACHE.lock().ok()?.get(&key).copied()
}

fn remember_count(core: &CollectionCore, name: &str, cards: Option<i64>) {
    if let (Some(key), Ok(mut m)) = (cache_key(core, name), CARD_COUNT_CACHE.lock()) {
        m.insert(key, cards);
    }
}

fn forget_count(core: &CollectionCore, name: &str) {
    if let (Some(key), Ok(mut m)) = (cache_key(core, name), CARD_COUNT_CACHE.lock()) {
        m.remove(&key);
    }
}

struct ListedBackup {
    parsed: ParsedBackupName,
    bytes: i64,
}

fn list_parsed(core: &CollectionCore) -> Result<Vec<ListedBackup>> {
    let files = core.blobs.list(BACKUP_PREFIX)?;
    let mut out = Vec::new();
    for f in files {
        let name = &f.path[BACKUP_PREFIX.len()..];
        if let Some(p) = parse_backup_name(name) {
            out.push(ListedBackup { parsed: p, bytes: f.bytes as i64 });
        }
    }
    Ok(out)
}

/// Cards in a backup image; None when unreadable.
fn count_cards_in_image(bytes: &[u8]) -> Option<i64> {
    if !is_sqlite_image(bytes) {
        return None;
    }
    let db = Db::open_from_bytes(bytes).ok()?;
    db.count("SELECT count(*) FROM cards", []).ok()
}

fn kind_of(file_kind: BackupFileKind) -> BackupKind {
    match file_kind {
        BackupFileKind::Auto => BackupKind::Daily,
        BackupFileKind::Manual => BackupKind::Manual,
        BackupFileKind::PreRestore => BackupKind::PreRestore,
    }
}

/// Lists backups newest first, with their rotation kind and card count.
pub fn list_backups(core: &CollectionCore, limits: BackupLimits) -> Result<Vec<BackupInfo>> {
    let mut parsed = list_parsed(core)?;
    let names: Vec<ParsedBackupName> = parsed.iter().map(|p| p.parsed.clone()).collect();
    let tz = |ms| core.tz_offset(ms);
    let plan = plan_rotation(&names, core.now_seconds(), &tz, limits);
    parsed.sort_by_key(|a| std::cmp::Reverse(a.parsed.at));
    let mut out = Vec::new();
    for p in parsed {
        let cards = match cached_count(core, &p.parsed.name) {
            Some(c) => c,
            None => {
                let cards = core.blobs.get(&backup_path(&p.parsed.name))?.and_then(|b| count_cards_in_image(&b));
                remember_count(core, &p.parsed.name, cards);
                cards
            }
        };
        let kind = plan.get(&p.parsed.name).copied().flatten().unwrap_or(kind_of(p.parsed.file_kind));
        out.push(BackupInfo { name: p.parsed.name, at: p.parsed.at, bytes: p.bytes, cards, kind });
    }
    Ok(out)
}

/// Deletes automatic backups the rotation no longer keeps. Returns the deleted names.
pub fn prune_backups(core: &CollectionCore, limits: BackupLimits) -> Result<Vec<String>> {
    let parsed = list_parsed(core)?;
    let names: Vec<ParsedBackupName> = parsed.iter().map(|p| p.parsed.clone()).collect();
    let tz = |ms| core.tz_offset(ms);
    let plan = plan_rotation(&names, core.now_seconds(), &tz, limits);
    let mut deleted = Vec::new();
    for (name, kind) in plan {
        if kind.is_none() {
            core.blobs.delete(&backup_path(&name))?;
            forget_count(core, &name);
            deleted.push(name);
        }
    }
    Ok(deleted)
}

/// Writes the current database to `backups/loci-…-<kind>.sqlite` (atomic put), records
/// col_meta.last_backup and prunes old automatic backups.
pub fn backup_now(core: &CollectionCore, kind: BackupFileKind) -> Result<BackupInfo> {
    let bytes = core.db().serialize()?;
    let cards = core.db().count("SELECT count(*) FROM cards", [])?;
    let mut at = core.now_seconds();
    let mut name = backup_file_name(at, kind);
    while core.blobs.exists(&backup_path(&name)) {
        at += 1;
        name = backup_file_name(at, kind);
    }
    core.blobs.put(&backup_path(&name), &bytes)?;
    remember_count(core, &name, Some(cards));
    core.set_meta(meta::LAST_BACKUP, Some(&core.now_seconds().to_string()))?;
    prune_backups(core, crate::backup::rotation::DEFAULT_BACKUP_LIMITS)?;
    Ok(BackupInfo { name, at, bytes: bytes.len() as i64, cards: Some(cards), kind: kind_of(kind) })
}

/// Reads and validates a backup's bytes.
pub fn read_backup(core: &CollectionCore, name: &str) -> Result<Vec<u8>> {
    if parse_backup_name(name).is_none() {
        return Err(LociError::invalid(format!("Not a backup name: {name}")));
    }
    let Some(bytes) = core.blobs.get(&backup_path(name))? else {
        return Err(LociError::not_found(format!("Backup {name} not found")));
    };
    if !is_sqlite_image(&bytes) {
        return Err(LociError::corrupt(format!("Backup {name} is not a database")));
    }
    let ok = Db::open_from_bytes(&bytes).and_then(|db| quick_check_ok(&db)).unwrap_or(false);
    if !ok {
        return Err(LociError::corrupt(format!("Backup {name} is damaged")));
    }
    Ok(bytes)
}

/// `PRAGMA quick_check` reports exactly one row reading `ok`.
pub fn quick_check_ok(db: &Db) -> Result<bool> {
    let rows: Vec<String> = db.all("PRAGMA quick_check", [], |r| r.get(0))?;
    Ok(rows.len() == 1 && rows[0] == "ok")
}

/// Seconds of the latest user-visible change: col_meta.last_change or any row's mtime/review time.
pub fn last_change_seconds(core: &CollectionCore) -> Result<Option<Seconds>> {
    let row: Option<i64> = core.db().scalar(
        "SELECT max(
      coalesce((SELECT max(mtime) FROM cards), 0),
      coalesce((SELECT max(mtime) FROM notes), 0),
      coalesce((SELECT max(mtime) FROM decks), 0),
      coalesce((SELECT max(mtime) FROM notetypes), 0),
      coalesce((SELECT max(mtime) FROM deck_configs), 0),
      coalesce((SELECT max(id) / 1000 FROM revlog), 0))",
        [],
    )?;
    let meta_v =
        core.get_meta(meta::LAST_CHANGE)?.and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0) as i64;
    let m = row.unwrap_or(0).max(meta_v);
    Ok(if m > 0 { Some(m) } else { None })
}

/// Applies the 30-minute rule and backs up when due.
pub fn auto_backup(core: &CollectionCore) -> Result<Option<BackupInfo>> {
    let last_backup =
        core.get_meta(meta::LAST_BACKUP)?.and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).map(|v| v as i64);
    if !should_backup(last_backup, last_change_seconds(core)?, core.now_seconds()) {
        return Ok(None);
    }
    backup_now(core, BackupFileKind::Auto).map(Some)
}

/// File extensions whose content is already compressed (stored without deflate).
static COMPRESSED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\.(?:png|jpe?g|gif|webp|avif|heic|mp3|m4a|aac|ogg|oga|opus|flac|mp4|m4v|webm|mov|mkv|zip|gz|zst|woff2?|pdf)$",
    )
    .expect("valid regex")
});

/// Full backup: a zip with `collection.sqlite`, every `media/<name>` file and a `manifest.json`
/// ({format, version, schemaVersion, collectionId, createdAt, notes, cards, media}).
pub fn export_full_backup(core: &CollectionCore, progress: &mut dyn FnMut(Progress)) -> Result<Vec<u8>> {
    let db = core.db().serialize()?;
    let files: Vec<_> =
        core.blobs.list(MEDIA_PREFIX)?.into_iter().filter(|f| !f.path[MEDIA_PREFIX.len()..].contains('/')).collect();
    let total = files.len() as i64 + 2;
    progress(Progress { phase: "collection".into(), done: 0, total });
    let mut inputs: Vec<ZipInput<'static>> = vec![ZipInput::new("collection.sqlite", db, true)];
    let mut done = 1;
    for f in &files {
        if let Some(bytes) = core.blobs.get(&f.path)? {
            inputs.push(ZipInput::new(f.path.clone(), bytes, !COMPRESSED.is_match(&f.path)));
        }
        done += 1;
        if done % 25 == 0 || done == files.len() as i64 + 1 {
            progress(Progress { phase: "media".into(), done, total });
        }
    }
    let manifest = serde_json::json!({
        "format": "loci-backup",
        "version": 1,
        "schemaVersion": SCHEMA_VERSION,
        "collectionId": core.get_meta(meta::COLLECTION_ID)?,
        "createdAt": core.now_seconds(),
        "notes": core.db().count("SELECT count(*) FROM notes", [])?,
        "cards": core.db().count("SELECT count(*) FROM cards", [])?,
        "media": files.len(),
    });
    inputs.push(ZipInput::new("manifest.json", serde_json::to_string_pretty(&manifest)?.into_bytes(), true));
    let out = create_zip(inputs)?;
    progress(Progress { phase: "done".into(), done: total, total });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anki::ZipArchive;
    use crate::collection::service::test_support::*;
    use crate::collection::Collection;
    use crate::error::ErrorCode;
    use crate::types::Notetype;

    fn add_notes(h: &Harness, basic: &Notetype, n: usize, prefix: &str) {
        for i in 0..n {
            h.add_note(basic, 1, &[&format!("{prefix}{i}"), "back"], &[]);
        }
    }

    #[test]
    fn maintenance_backup_half_applies_the_30_minute_rule() {
        let h = make_collection();
        assert!(h.col.maintenance_backup().unwrap().is_some());
        assert!(h.col.maintenance_backup().unwrap().is_none());
        h.clock.advance(60_000);
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["x", ""], &[]);
        h.clock.advance(10 * 60_000);
        assert!(h.col.maintenance_backup().unwrap().is_none());
        h.clock.advance(20 * 60_000);
        assert!(h.col.maintenance_backup().unwrap().is_some());
        assert_eq!(h.col.list_backups().unwrap().len(), 2);
        // `auto_backup` short-circuits on a fresh last_backup; `backup_on_leave` only needs a change.
        assert!(h.col.auto_backup().unwrap().is_none());
        assert!(h.col.backup_on_leave().unwrap().is_none());
        h.clock.advance(1_000);
        h.add_note(&basic, 1, &["y", ""], &[]);
        assert!(h.col.backup_on_leave().unwrap().is_some());
    }

    #[test]
    fn backup_now_writes_a_valid_standalone_database_and_list_backups_shows_card_counts() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 3, "n");
        let info = h.col.backup_now().unwrap();
        assert_eq!(
            (info.kind, info.cards, info.name.as_str()),
            (BackupKind::Manual, Some(3), "loci-20260310-120000-manual.sqlite")
        );
        let second = h.col.backup_now().unwrap();
        assert_eq!(second.name, "loci-20260310-120001-manual.sqlite");
        let bytes = h.blobs().get(&format!("backups/{}", info.name)).unwrap().unwrap();
        assert_eq!(bytes.len() as i64, info.bytes);
        let db = Db::open_from_bytes(&bytes).unwrap();
        let check: Vec<String> = db.all("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
        assert_eq!(check, ["ok"]);
        assert_eq!(db.count("SELECT count(*) FROM notes", []).unwrap(), 3);
        let list = h.col.list_backups().unwrap();
        assert_eq!(
            list.iter().map(|b| (b.name.as_str(), b.cards, b.kind)).collect::<Vec<_>>(),
            [(second.name.as_str(), Some(3), BackupKind::Manual), (info.name.as_str(), Some(3), BackupKind::Manual)]
        );
        assert_eq!(h.meta("last_backup").unwrap(), (h.clock.ms() / 1000).to_string());
    }

    #[test]
    fn restore_round_trip_replaces_the_file_keeps_a_pre_restore_backup_counts_equal() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 3, "keep");
        let info = h.col.backup_now().unwrap();
        h.clock.advance(5_000);
        add_notes(&h, &basic, 2, "later");
        assert_eq!(h.count("SELECT count(*) FROM cards"), 5);
        let report = h.col.restore_backup(&info.name).unwrap();
        assert!(report.ok);
        assert_eq!(h.count("SELECT count(*) FROM cards"), 3);
        assert_eq!(h.col.undo_label().unwrap(), None);
        let list = h.col.list_backups().unwrap();
        let pre = list.iter().find(|b| b.kind == BackupKind::PreRestore).unwrap();
        assert_eq!(pre.cards, Some(5));
        // The file on disk was swapped: a fresh connection sees the restored data.
        let db = Db::open(h.db_path()).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM notes", []).unwrap(), 3);
        drop(db);
        // The collection keeps working after the swap.
        add_notes(&h, &basic, 1, "after");
        assert_eq!(h.count("SELECT count(*) FROM notes"), 4);
        assert_eq!(h.col.restore_backup("loci-20000101-000000-manual.sqlite").unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(h.col.restore_backup("../etc/passwd").unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn backup_corrupt_open_restore_counts_equal() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 40, "note with some text to fill pages ");
        let info = h.col.backup_now().unwrap();
        let cards = h.count("SELECT count(*) FROM cards");
        h.col.close().unwrap();
        // Overwrite pages 3..6 with garbage.
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(h.db_path()).unwrap();
            f.seek(SeekFrom::Start(4096 * 2)).unwrap();
            f.write_all(&vec![0xa5u8; 4096 * 4]).unwrap();
        }
        let mut col = Collection::open_dir(h.dir.path(), Some(h.clock.clock_fn()), Some(Box::new(|_| 0))).unwrap();
        let damaged = match col.open() {
            Ok(report) => !report.ok,
            Err(e) => {
                assert!(matches!(e.code, ErrorCode::Corrupt | ErrorCode::Internal));
                true
            }
        };
        assert!(damaged);
        let report = col.restore_backup(&info.name).unwrap();
        assert!(report.ok);
        assert_eq!(col.core().db().count("SELECT count(*) FROM cards", []).unwrap(), cards);
        assert!(col.check_integrity().unwrap().ok);
        h.col = col;
    }

    #[test]
    fn reset_collection_wipes_data_keeps_a_pre_restore_backup_and_reseeds() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 2, "x");
        h.col.create_deck("Gone", None).unwrap();
        let old_id = h.meta("collection_id");
        h.col.reset_collection().unwrap();
        assert!(h.col.is_empty().unwrap());
        assert!(h.col.core().deck_id_by_name("Gone").unwrap().is_none());
        assert_eq!(h.col.list_notetypes().unwrap().len(), 5);
        assert_ne!(h.meta("collection_id"), old_id);
        assert!(h.col.list_backups().unwrap().iter().any(|b| b.kind == BackupKind::PreRestore && b.cards == Some(2)));
        assert!(h.col.check_integrity().unwrap().ok);
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 1, "fresh");
        assert_eq!(h.count("SELECT count(*) FROM notes_fts WHERE notes_fts MATCH 'fresh'"), 1);
    }

    #[test]
    fn export_full_backup_zips_the_collection_media_and_a_manifest() {
        let h = make_collection();
        let basic = h.notetype_by_name("Basic");
        add_notes(&h, &basic, 2, "z");
        h.col.add_media("pic.png", &[1, 2, 3]).unwrap();
        h.col.add_media("notes.txt", b"hello").unwrap();
        let mut phases: Vec<String> = Vec::new();
        let zip = h.col.export_full_backup(&mut |p| phases.push(p.phase)).unwrap();
        let archive = ZipArchive::open(zip).unwrap();
        let mut names = archive.names();
        names.sort();
        assert_eq!(names, ["collection.sqlite", "manifest.json", "media/notes.txt", "media/pic.png"]);
        let manifest: serde_json::Value = serde_json::from_slice(&archive.read("manifest.json").unwrap()).unwrap();
        assert_eq!(manifest["format"], "loci-backup");
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["notes"], 2);
        assert_eq!(manifest["cards"], 2);
        assert_eq!(manifest["media"], 2);
        assert_eq!(archive.read("media/pic.png").unwrap(), vec![1, 2, 3]);
        // Already-compressed extensions are stored, text is deflated.
        assert_eq!(archive.info("media/pic.png").unwrap().compressed_size, 3);
        let db = Db::open_from_bytes(&archive.read("collection.sqlite").unwrap()).unwrap();
        assert_eq!(db.count("SELECT count(*) FROM notes", []).unwrap(), 2);
        assert_eq!(phases.first().map(String::as_str), Some("collection"));
        assert_eq!(phases.last().map(String::as_str), Some("done"));
    }

    #[test]
    fn read_backup_rejects_damaged_images() {
        let h = make_collection();
        let info = h.col.backup_now().unwrap();
        let path = format!("backups/{}", info.name);
        let mut bytes = h.blobs().get(&path).unwrap().unwrap();
        for b in bytes.iter_mut().skip(4096) {
            *b = 0xa5;
        }
        h.blobs().put(&path, &bytes).unwrap();
        assert_eq!(read_backup(h.col.core(), &info.name).unwrap_err().code, ErrorCode::Corrupt);
        h.blobs().put(&path, b"not sqlite").unwrap();
        assert_eq!(read_backup(h.col.core(), &info.name).unwrap_err().code, ErrorCode::Corrupt);
    }
}
