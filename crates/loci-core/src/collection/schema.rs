//! Schema version, extra indexes and migrations.

use crate::db::Db;
use crate::error::Result;

/// Current schema version stored in `col_meta.schema_version`.
pub const SCHEMA_VERSION: i64 = 1;

/// The collection schema (`schema.sql`).
pub const SCHEMA_SQL: &str = crate::SCHEMA_SQL;

/// Statements run after `SCHEMA_SQL` on every open: indexes that are not in `schema.sql`
/// but that the collection service relies on for speed.
pub const EXTRA_SQL: &str = "CREATE INDEX IF NOT EXISTS ix_media_sha1 ON media (sha1);";

/// Migration hook: `migrate(db, n)` upgrades a collection from version n − 1 to n. Each runs in
/// its own transaction, after which `schema_version` is set to n. Nothing to do at version 1.
pub fn migrate(_db: &Db, _to_version: i64) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_every_table() {
        let db = Db::memory().unwrap();
        db.execute_batch(SCHEMA_SQL).unwrap();
        db.execute_batch(EXTRA_SQL).unwrap();
        let tables: Vec<String> = db.all("SELECT name FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
        for t in [
            "col_meta",
            "notetypes",
            "fields",
            "templates",
            "decks",
            "deck_configs",
            "notes",
            "cards",
            "revlog",
            "tags",
            "graves",
            "media",
            "saved_searches",
            "undo_log",
            "optimise_log",
            "notes_fts",
        ] {
            assert!(tables.iter().any(|x| x == t), "missing table {t}");
        }
        // Idempotent.
        db.execute_batch(SCHEMA_SQL).unwrap();
    }
}
