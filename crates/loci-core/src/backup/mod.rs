//! Backups (recall `src/core/backup/`). Phase 1 ports the pure rotation policy; phase 2 adds
//! `integrity` and `backup` (the Collection-level backup/restore operations).
pub mod rotation;

pub use rotation::{
    backup_file_name, parse_backup_name, plan_rotation, should_backup, BackupFileKind, BackupLimits, ParsedBackupName,
    TzOffsetFn, BACKUP_MIN_INTERVAL_SECONDS, DEFAULT_BACKUP_LIMITS,
};
