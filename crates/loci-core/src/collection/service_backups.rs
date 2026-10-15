//! `Collection`: backups, restore, full backups, integrity and reset (the lifecycle
//! and data methods are in `service.rs`).

use crate::backup::backup as backups;
use crate::backup::integrity::check_integrity;
use crate::backup::{BackupFileKind, BACKUP_MIN_INTERVAL_SECONDS, DEFAULT_BACKUP_LIMITS};
use crate::collection::full_backup::{self, LociBackupContents};
use crate::collection::internals::meta;
use crate::collection::service::Collection;
use crate::db::Db;
use crate::error::{ErrorCode, LociError, Result};
use crate::types::*;
use crate::undo;

impl Collection {
    // ───────────────────────── backups ─────────────────────────

    pub fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        backups::list_backups(&self.core, DEFAULT_BACKUP_LIMITS)
    }
    pub fn backup_now(&self) -> Result<BackupInfo> {
        backups::backup_now(&self.core, BackupFileKind::Manual)
    }
    /// The 30-minute rule on its own (internals §12): an automatic backup when something changed and
    /// the last backup is at least 30 minutes old. Cheap when a recent backup exists.
    pub fn auto_backup(&self) -> Result<Option<BackupInfo>> {
        let last = self.core.get_meta(meta::LAST_BACKUP)?.and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
        if let Some(last) = last {
            if (self.core.now_seconds() as f64) - last < BACKUP_MIN_INTERVAL_SECONDS as f64 {
                return Ok(None);
            }
        }
        backups::auto_backup(&self.core)
    }
    /// Writes an automatic backup now regardless of the 30-minute rule (e.g. when the app is hidden).
    pub fn backup_on_leave(&self) -> Result<Option<BackupInfo>> {
        let last = self.core.get_meta(meta::LAST_BACKUP)?;
        let changed = backups::last_change_seconds(&self.core)?;
        let Some(changed) = changed else { return Ok(None) };
        if let Some(last) = last {
            // `Number(last) >= changed`: an unparsable value is NaN and never satisfies it.
            if last.parse::<f64>().ok().is_some_and(|l| l >= changed as f64) {
                return Ok(None);
            }
        }
        backups::backup_now(&self.core, BackupFileKind::Auto).map(Some)
    }
    /// The backup half of `maintenance()`: `autoBackup` after the optimiser ran.
    pub fn maintenance_backup(&self) -> Result<Option<BackupInfo>> {
        backups::auto_backup(&self.core)
    }

    pub fn restore_backup(&mut self, name: &str) -> Result<IntegrityReport> {
        let bytes = backups::read_backup(&self.core, name)?;
        self.session = None;
        self.backup_before_replacing()?;
        self.replace_database(&bytes)?;
        let report = self.open()?;
        undo::clear(self.core.db())?;
        Ok(report)
    }

    pub fn export_full_backup(&self, progress: &mut dyn FnMut(Progress)) -> Result<Vec<u8>> {
        backups::export_full_backup(&self.core, progress)
    }

    pub fn check_integrity(&self) -> Result<IntegrityReport> {
        check_integrity(&self.core, true)
    }

    /// Restores a Loci full backup (collection.sqlite + media/): validates the database, writes a
    /// pre-restore backup, copies media files that differ, then replaces the collection.
    /// The importer calls this for `mode: replace`.
    pub fn restore_full_backup(
        &mut self,
        b: &LociBackupContents,
        options: &ImportOptions,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<ImportReport> {
        if options.mode != ImportMode::Replace {
            return Err(LociError::invalid(
                "This file is a Loci full backup. Restore it with \"Replace collection\"; it cannot be merged.",
            ));
        }
        self.session = None;
        let started = self.core.now();
        progress(Progress { phase: "read".into(), done: 0, total: 3 });
        let counts = full_backup::validate_loci_database(&b.database)?;
        self.backup_before_replacing()?;
        progress(Progress { phase: "media".into(), done: 1, total: 3 });
        let media_added = full_backup::restore_backup_media(&self.core, b)?;
        progress(Progress { phase: "collection".into(), done: 2, total: 3 });
        self.replace_database(&b.database)?;
        let report = self.open()?;
        undo::clear(self.core.db())?;
        progress(Progress { phase: "done".into(), done: 3, total: 3 });
        Ok(ImportReport {
            notes_added: counts.notes,
            notes_updated: 0,
            notes_skipped: 0,
            cards_added: counts.cards,
            reviews_added: counts.revlog,
            media_added,
            decks_added: 0,
            notetypes_added: 0,
            warnings: if report.ok { vec![] } else { report.problems },
            duration_ms: self.core.now() - started,
        })
    }

    /// Wipes every table and starts a fresh collection (new id, Default deck, presets, built-in
    /// note types). A pre-restore backup is written first. Media files are kept on disk
    /// (unreferenced) so restoring that backup brings the collection back whole.
    pub fn reset_collection(&mut self) -> Result<()> {
        self.session = None;
        self.backup_before_replacing()?;
        let last_backup = self.core.get_meta(meta::LAST_BACKUP)?;
        let carry = crate::sync::state::carry(self.core.db());
        {
            let db = self.core.db();
            db.transaction(|| {
                let objects = || {
                    db.all("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'", [], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
                    })
                };
                for (t, name, _) in objects()? {
                    if t == "trigger" {
                        db.execute_batch(&format!("DROP TRIGGER IF EXISTS \"{name}\""))?;
                    }
                }
                for (t, name, sql) in objects()? {
                    if t == "table" && sql.as_deref().is_some_and(is_virtual_table_sql) {
                        db.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
                    }
                }
                for (t, name, _) in objects()? {
                    if t == "table" {
                        db.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
                    }
                }
                Ok(())
            })?;
            db.execute_batch("VACUUM")?;
        }
        self.core.invalidate_caches();
        self.open()?;
        crate::sync::state::after_replace(&self.core, &carry)?;
        if let Some(lb) = last_backup {
            self.core.set_meta(meta::LAST_BACKUP, Some(&lb))?;
        }
        Ok(())
    }

    // ───────────────────────── helpers ─────────────────────────

    /// Pre-restore safety copy. A damaged live database must not block a restore.
    fn backup_before_replacing(&self) -> Result<()> {
        match backups::backup_now(&self.core, BackupFileKind::PreRestore) {
            Ok(_) => Ok(()),
            Err(e) if e.code == ErrorCode::Busy => Err(e),
            Err(_) => Ok(()),
        }
    }

    /// Replaces the live database with `bytes` (restore): closes the current connection, swaps
    /// the file atomically when file-backed (else opens a private copy), drops caches and the
    /// undo stack. `open()` must follow.
    pub fn replace_database(&mut self, bytes: &[u8]) -> Result<()> {
        self.session = None;
        self.reconnect_if_closed().ok();
        let carry = crate::sync::state::carry(self.core.db());
        let old = self.core.set_db(Db::memory()?);
        old.close()?;
        self.closed = true;
        let next = match &self.db_path {
            Some(path) => Db::replace_file(path, bytes)?,
            None => Db::open_from_bytes(bytes)?,
        };
        self.core.set_db(next);
        self.closed = false;
        undo::clear(self.core.db())?;
        crate::sync::state::after_replace(&self.core, &carry)?;
        Ok(())
    }
}

fn is_virtual_table_sql(sql: &str) -> bool {
    let s = sql.trim_start().to_ascii_uppercase();
    s.starts_with("CREATE") && s["CREATE".len()..].trim_start().starts_with("VIRTUAL")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blobs::BlobStore;
    use crate::collection::service::test_support::*;

    #[test]
    fn replace_database_on_a_memory_collection_opens_a_private_copy() {
        let dir = tempfile::tempdir().unwrap();
        let mut col = Collection::open_with(
            Db::memory().unwrap(),
            BlobStore::new(dir.path()),
            Some(Box::new(|| T0)),
            Some(Box::new(|_| 0)),
        )
        .with_app_version("loci-test 9.9");
        col.open().unwrap();
        assert_eq!(col.core().get_meta(meta::CREATED_WITH).unwrap().as_deref(), Some("loci-test 9.9"));
        let basic = col.list_notetypes().unwrap().into_iter().find(|n| n.name == "Basic").unwrap();
        col.add_note(basic.id, 1, &strs(&["a", "b"]), &[]).unwrap();
        let image = col.core().db().serialize().unwrap();
        col.add_note(basic.id, 1, &strs(&["c", "d"]), &[]).unwrap();
        assert_eq!(col.core().db().count("SELECT count(*) FROM notes", []).unwrap(), 2);
        col.replace_database(&image).unwrap();
        assert!(col.open().unwrap().ok);
        assert_eq!(col.core().db().count("SELECT count(*) FROM notes", []).unwrap(), 1);
        assert_eq!(col.undo_label().unwrap(), None);
        assert_eq!(col.replace_database(b"garbage").unwrap_err().code, ErrorCode::Corrupt);
    }

    #[test]
    fn a_closed_collection_refuses_work_until_reopened() {
        let mut h = make_collection();
        h.col.close().unwrap();
        assert!(h.col.is_empty().is_err());
        assert!(h.col.open().unwrap().ok);
        assert!(h.col.is_empty().unwrap());
    }
}
