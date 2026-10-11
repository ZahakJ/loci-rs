//! Backups (recall `src/core/backup/`): the pure rotation policy (`rotation`), the
//! Collection-level backup/restore operations (`backup`) and the integrity check and repair
//! (`integrity`).
#[allow(clippy::module_inception)]
pub mod backup;
pub mod integrity;
pub mod rotation;

pub use backup::{
    auto_backup, backup_now, export_full_backup, last_change_seconds, list_backups, prune_backups, read_backup, BACKUP_PREFIX,
};
pub use integrity::{check_integrity, sqlite_integrity};
pub use rotation::{
    backup_file_name, parse_backup_name, plan_rotation, should_backup, BackupFileKind, BackupLimits, ParsedBackupName,
    TzOffsetFn, BACKUP_MIN_INTERVAL_SECONDS, DEFAULT_BACKUP_LIMITS,
};
