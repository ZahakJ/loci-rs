//! `Collection`: the service that implements the `CollectionApi` over a `Db` and a `BlobStore`
//! (recall `src/core/collection/collection.ts`).
//!
//! This file owns lifecycle, settings, decks, presets, note types, notes, tags, media, card state,
//! undo, saved searches and the sample deck; `service_backups.rs` has backups, restore, integrity and reset. The integration agent adds `impl Collection`
//! blocks in its own files for: `start_study`, `next_card`, `answer_card`, `start_custom_study`,
//! `end_study` (study.rs); `preview_note`, `check_template`, `render_card`, `explain_card`
//! (render.rs); `backlog_report`, `apply_catch_up` (backlog.rs); `browse`, `search_ids`, `sidebar`,
//! `bulk` (browse.rs); `preview_import`, `import_package` (import.rs); `export_package` (export.rs);
//! `optimise`, `evaluate`, `maintenance`, `forecast_workload` (optimise.rs); `stats`, `insights`
//! (stats).

use crate::backup::integrity::sqlite_integrity;
use crate::blobs::BlobStore;
use crate::collection::builtin_notetypes::{builtin_notetype, BUILTIN_KINDS};
use crate::collection::cards;
use crate::collection::counter::{scheduler_deck_counts, CounterFn, QueueCounter};
use crate::collection::decks::{self, DeckField, DeleteDeckResult};
use crate::collection::internals::{meta, Clock, CollectionCore, IdTable, TzOffsetFn, DEFAULT_PRESET_ID};
use crate::collection::media::{self, MediaCheck};
use crate::collection::notes::{self, AddNoteResult, TagCount, UpdateNoteResult};
use crate::collection::notetypes;
use crate::collection::presets::{default_presets, normalise_deck_config};
use crate::collection::rows::parse_json_object;
use crate::collection::schema::{migrate, EXTRA_SQL, SCHEMA_SQL, SCHEMA_VERSION};
use crate::collection::settings::{default_settings, encode_patch, KEY_ROLLOVER_HOUR};
use crate::collection::study::StudySession;
use crate::collection::time::initial_crt;
use crate::db::Db;
use crate::error::{ErrorCode, LociError, Result};
use crate::text::PLAIN_VERSION;
use crate::types::*;
use crate::undo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `undo()` result: the label of the operation that was reverted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoResult {
    pub label: String,
}

/// The key under which `backlog.ts` stores a temporary catch-up retention inside a preset's
/// config JSON (`backlog.ts:22`). `save_preset` keeps the marker when the retention is unchanged.
pub const TEMP_RETENTION_KEY: &str = "lociTemporaryRetention";

/// `backlog.ts:182 readTemporaryRetention`: `{original, retention, untilDay}` or None.
pub(crate) fn read_temporary_retention(raw: &serde_json::Map<String, serde_json::Value>) -> Option<(f64, f64, i64)> {
    let o = raw.get(TEMP_RETENTION_KEY)?.as_object()?;
    let original = o.get("original")?.as_f64()?;
    let until_day = o.get("untilDay")?.as_f64()? as i64;
    let retention = o.get("retention").and_then(|v| v.as_f64()).unwrap_or(original);
    Some((original, retention, until_day))
}

pub struct Collection {
    /// Synchronous core for sibling modules (study, search, stats, import/export).
    pub(crate) core: CollectionCore,
    /// The active study session, if any (in memory; a reload starts a new one).
    pub(crate) session: Option<StudySession>,
    /// Path of `collection.sqlite` when file-backed (restore swaps the file); None for memory.
    pub(crate) db_path: Option<PathBuf>,
    /// Recorded in col_meta.created_with for new collections.
    pub(crate) app_version: String,
    /// Supplies deck-tree counts; defaults to the scheduler's v3 gathering (`counter.rs`).
    pub(crate) queue_counter: Option<QueueCounter>,
    pub(crate) closed: bool,
}

impl std::fmt::Debug for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collection").field("core", &self.core).field("db_path", &self.db_path).finish()
    }
}

/// File name of the database inside a collection directory (recall `scripts/loci.ts`).
pub const COLLECTION_FILE: &str = "collection.sqlite";

impl Collection {
    /// Opens (creating if needed) `<dir>/collection.sqlite` with blobs (`media/`, `backups/`) in
    /// the same directory. Call `open()` next.
    pub fn open_dir(dir: &Path, clock: Option<Clock>, tz: Option<TzOffsetFn>) -> Result<Collection> {
        let path = dir.join(COLLECTION_FILE);
        let db = Db::open(&path)?;
        let mut col = Collection::open_with(db, BlobStore::new(dir), clock, tz);
        col.db_path = Some(path);
        Ok(col)
    }

    /// Wraps an existing connection (in-memory tests, previews). Call `open()` next.
    pub fn open_with(db: Db, blobs: BlobStore, clock: Option<Clock>, tz: Option<TzOffsetFn>) -> Collection {
        let db_path = db.path().map(Path::to_path_buf);
        Collection {
            core: CollectionCore::new(db, blobs, clock, tz),
            session: None,
            db_path,
            app_version: "loci 0.2.1".to_string(),
            queue_counter: None,
            closed: false,
        }
    }

    pub fn with_app_version(mut self, version: impl Into<String>) -> Collection {
        self.app_version = version.into();
        self
    }

    /// Replaces the deck-tree counter (tests inject a fake one, as recall's `CollectionOptions.queueCounter`).
    pub fn with_queue_counter(mut self, counter: QueueCounter) -> Collection {
        self.queue_counter = Some(counter);
        self
    }

    pub fn core(&self) -> &CollectionCore {
        &self.core
    }

    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    // ───────────────────────── lifecycle ─────────────────────────

    /// Creates or upgrades the schema, seeds a new collection, rebuilds stale search text,
    /// unburies after a day rollover and runs SQLite's integrity check (`collection.ts openUnchecked`).
    pub fn open(&mut self) -> Result<IntegrityReport> {
        self.open_unchecked().map_err(|e| match e.code {
            ErrorCode::Internal => LociError::corrupt(format!("The collection database cannot be opened: {}", e.message)),
            _ => e,
        })
    }

    fn open_unchecked(&mut self) -> Result<IntegrityReport> {
        self.closed = false;
        let core = &self.core;
        core.db().execute_batch(SCHEMA_SQL)?;
        core.db().execute_batch(EXTRA_SQL)?;
        core.invalidate_caches();
        match core.get_meta(meta::SCHEMA_VERSION)? {
            None => self.seed()?,
            Some(raw) => {
                let version = raw.parse::<i64>().ok().filter(|v| *v >= 1);
                let Some(version) = version else { return Err(LociError::corrupt(format!("Invalid schema version {raw}"))) };
                if version > SCHEMA_VERSION {
                    return Err(LociError::invalid(format!(
                        "This collection was written by a newer version of Loci (schema {version})"
                    )));
                }
                for v in (version + 1)..=SCHEMA_VERSION {
                    core.db().transaction(|| {
                        migrate(core.db(), v)?;
                        core.set_meta(meta::SCHEMA_VERSION, Some(&v.to_string()))
                    })?;
                }
            }
        }
        let core = &self.core;
        // Derived search text written by older rules (e.g. cloze markup kept) is rebuilt once, in chunks.
        let plain_version = core.get_meta(meta::PLAIN_VERSION)?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(1);
        if plain_version < PLAIN_VERSION {
            notes::rebuild_plain(core, 1000)?;
            core.set_meta(meta::PLAIN_VERSION, Some(&PLAIN_VERSION.to_string()))?;
        }
        cards::unbury_if_day_rolled_over(core)?;
        let lines = sqlite_integrity(core.db());
        Ok(IntegrityReport {
            ok: lines.is_empty(),
            problems: lines.into_iter().map(|l| format!("Database: {l}")).collect(),
            repaired: vec![],
        })
    }

    /// Creates the meta keys, presets, Default deck and built-in note types of a new collection.
    fn seed(&self) -> Result<()> {
        let core = &self.core;
        core.db().transaction(|| {
            let now = core.now();
            let offset = core.tz_offset(now);
            let set_if_missing = |key: &str, value: String| -> Result<()> {
                if core.get_meta(key)?.is_none() {
                    core.set_meta(key, Some(&value))?;
                }
                Ok(())
            };
            set_if_missing(meta::COLLECTION_ID, uuid::Uuid::new_v4().to_string())?;
            set_if_missing(meta::CRT, initial_crt(now, offset, default_settings().rollover_hour).to_string())?;
            set_if_missing(meta::CREATION_OFFSET, offset.to_string())?;
            set_if_missing(meta::CREATED_WITH, self.app_version.clone())?;
            set_if_missing(meta::NEXT_POS, "1".to_string())?;
            set_if_missing(meta::PLAIN_VERSION, PLAIN_VERSION.to_string())?;
            set_if_missing(KEY_ROLLOVER_HOUR, default_settings().rollover_hour.to_string())?;
            for p in default_presets() {
                if core.db().scalar::<i64, _>("SELECT 1 FROM deck_configs WHERE id = ?", [p.id])?.is_none() {
                    core.db().run(
                        "INSERT INTO deck_configs (id, name, config, mtime, usn) VALUES (?, ?, ?, ?, -1)",
                        rusqlite::params![p.id, p.name, serde_json::to_string(&p.config)?, core.now_seconds()],
                    )?;
                }
            }
            decks::ensure_default_deck(core)?;
            if core.db().scalar::<i64, _>("SELECT 1 FROM notetypes LIMIT 1", [])?.is_none() {
                for kind in BUILTIN_KINDS {
                    notetypes::save_notetype(core, &builtin_notetype(kind))?;
                }
            }
            core.set_meta(meta::SCHEMA_VERSION, Some(&SCHEMA_VERSION.to_string()))
        })?;
        core.invalidate_caches();
        Ok(())
    }

    /// Closes the database connection. Further calls fail until `open()` re-opens a file-backed
    /// collection; idempotent.
    pub fn close(&mut self) -> Result<()> {
        self.session = None;
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let old = self.core.set_db(Db::memory()?);
        old.close()
    }

    /// Re-opens the connection after `close()` (file-backed collections only).
    pub(crate) fn reconnect_if_closed(&mut self) -> Result<()> {
        if !self.closed {
            return Ok(());
        }
        let Some(path) = &self.db_path else { return Err(LociError::busy("The collection is closed")) };
        let db = Db::open(path)?;
        self.core.set_db(db);
        self.closed = false;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.core.db().scalar::<i64, _>("SELECT 1 FROM notes LIMIT 1", [])?.is_none())
    }

    pub fn get_settings(&self) -> Result<CollectionSettings> {
        self.core.settings()
    }

    /// Applies a validated patch (`settings::parse_patch` turns the UI's JSON into one).
    pub fn set_settings(&self, patch: &CollectionSettingsPatch) -> Result<CollectionSettings> {
        let encoded = encode_patch(patch)?;
        self.core.db().transaction(|| {
            for (k, v) in &encoded {
                self.core.set_meta(k, Some(v))?;
            }
            Ok(())
        })?;
        self.get_settings()
    }

    pub fn today(&self) -> Result<CollectionDay> {
        self.rollover()?;
        self.core.today()
    }

    /// Day-rollover housekeeping before anything that reads queues or counts: unbury buried cards.
    pub fn rollover(&self) -> Result<()> {
        cards::unbury_if_day_rolled_over(&self.core).map(|_| ())
    }

    fn counter(&self) -> &CounterFn {
        match &self.queue_counter {
            Some(c) => c.as_ref(),
            None => &scheduler_deck_counts,
        }
    }

    // ───────────────────────── decks & presets ─────────────────────────

    pub fn deck_tree(&self) -> Result<DeckTreeNode> {
        self.rollover()?;
        decks::deck_tree(&self.core, self.counter())
    }
    pub fn get_deck(&self, id: DeckId) -> Result<Deck> {
        self.core.deck(id)
    }
    pub fn create_deck(&self, name: &str, config_id: Option<DeckConfigId>) -> Result<Deck> {
        decks::create_deck(&self.core, name, config_id)
    }
    pub fn rename_deck(&self, id: DeckId, name: &str) -> Result<()> {
        decks::rename_deck(&self.core, id, name)
    }
    pub fn delete_deck(&self, id: DeckId) -> Result<DeleteDeckResult> {
        decks::delete_deck(&self.core, id)
    }
    pub fn set_deck_collapsed(&self, id: DeckId, collapsed: bool) -> Result<()> {
        decks::set_deck_field(&self.core, id, DeckField::Collapsed(collapsed))
    }
    pub fn set_deck_preset(&self, id: DeckId, config_id: DeckConfigId) -> Result<()> {
        decks::set_deck_field(&self.core, id, DeckField::ConfigId(config_id))
    }
    pub fn set_deck_description(&self, id: DeckId, description: &str) -> Result<()> {
        decks::set_deck_field(&self.core, id, DeckField::Description(description.to_string()))
    }

    pub fn list_presets(&self) -> Result<Vec<DeckConfigPreset>> {
        self.core.all_presets()
    }
    pub fn get_preset(&self, id: DeckConfigId) -> Result<DeckConfigPreset> {
        self.core.preset(id)
    }
    pub fn save_preset(&self, preset: &DeckConfigPresetInput) -> Result<DeckConfigPreset> {
        let core = &self.core;
        let name = preset.name.trim().to_string();
        if name.is_empty() {
            return Err(LociError::invalid("Preset name is empty"));
        }
        let normalised = normalise_deck_config(&serde_json::to_value(&preset.config)?);
        core.db().transaction(|| {
            let mtime = core.now_seconds();
            let stored: Option<String> = match preset.id {
                Some(id) => core.db().scalar("SELECT config FROM deck_configs WHERE id = ?", [id])?,
                None => None,
            };
            let id = match (preset.id, stored) {
                (Some(id), Some(stored)) => {
                    // A temporary catch-up retention survives a save that keeps it; changing the value ends it.
                    let raw = parse_json_object(&stored);
                    let mut out = serde_json::to_value(&normalised)?;
                    if let Some((_, retention, _)) = read_temporary_retention(&raw) {
                        if normalised.desired_retention == retention {
                            if let (Some(o), Some(marker)) = (out.as_object_mut(), raw.get(TEMP_RETENTION_KEY)) {
                                o.insert(TEMP_RETENTION_KEY.to_string(), marker.clone());
                            }
                        }
                    }
                    core.db().run(
                        "UPDATE deck_configs SET name = ?, config = ?, mtime = ?, usn = -1 WHERE id = ?",
                        rusqlite::params![name, out.to_string(), mtime, id],
                    )?;
                    id
                }
                (given, _) => {
                    // An absent, zero or negative id allocates one; a positive unknown id is kept (collection.ts:195).
                    let id = match given {
                        Some(id) if id > 0 => id,
                        _ => core.new_id(IdTable::DeckConfigs)?,
                    };
                    core.db().run(
                        "INSERT INTO deck_configs (id, name, config, mtime, usn) VALUES (?, ?, ?, ?, -1)",
                        rusqlite::params![id, name, serde_json::to_string(&normalised)?, mtime],
                    )?;
                    id
                }
            };
            core.mark_changed()?;
            core.preset(id)
        })
    }
    pub fn delete_preset(&self, id: DeckConfigId) -> Result<()> {
        let core = &self.core;
        if id == DEFAULT_PRESET_ID {
            return Err(LociError::invalid("The default preset cannot be deleted"));
        }
        core.db().transaction(|| {
            core.preset(id)?;
            core.db().run(
                "UPDATE decks SET config_id = ?, mtime = ?, usn = -1 WHERE config_id = ?",
                rusqlite::params![DEFAULT_PRESET_ID, core.now_seconds(), id],
            )?;
            core.db().run("DELETE FROM deck_configs WHERE id = ?", [id])?;
            core.mark_changed()
        })
    }

    // ───────────────────────── note types ─────────────────────────

    pub fn list_notetypes(&self) -> Result<Vec<Notetype>> {
        notetypes::list_notetypes(&self.core)
    }
    pub fn get_notetype(&self, id: NotetypeId) -> Result<Notetype> {
        self.core.notetype(id)
    }
    pub fn save_notetype(&self, nt: &NotetypeInput) -> Result<Notetype> {
        notetypes::save_notetype(&self.core, nt)
    }
    pub fn delete_notetype(&self, id: NotetypeId) -> Result<()> {
        notetypes::delete_notetype(&self.core, id)
    }

    // ───────────────────────── notes & cards ─────────────────────────

    pub fn get_note(&self, id: NoteId) -> Result<Note> {
        notes::get_note(&self.core, id)
    }
    pub fn get_card(&self, id: CardId) -> Result<Card> {
        notes::get_card(&self.core, id)
    }
    pub fn get_cards_of_note(&self, id: NoteId) -> Result<Vec<Card>> {
        notes::get_cards_of_note(&self.core, id)
    }
    pub fn add_note(&self, ntid: NotetypeId, deck_id: DeckId, fields: &[String], tags: &[String]) -> Result<AddNoteResult> {
        notes::add_note(&self.core, ntid, deck_id, fields, tags)
    }
    pub fn update_note(&self, id: NoteId, fields: &[String], tags: &[String]) -> Result<UpdateNoteResult> {
        notes::update_note(&self.core, id, fields, tags)
    }
    pub fn delete_notes(&self, ids: &[NoteId]) -> Result<()> {
        notes::delete_notes(&self.core, ids)
    }
    pub fn find_duplicates(&self, ntid: NotetypeId, first_field: &str) -> Result<Vec<NoteId>> {
        notes::find_duplicates(&self.core, ntid, first_field)
    }
    pub fn all_tags(&self) -> Result<Vec<TagCount>> {
        notes::all_tags(&self.core)
    }
    pub fn rename_tag(&self, from: &str, to: &str) -> Result<i64> {
        notes::rename_tag(&self.core, from, to)
    }

    // ───────────────────────── media ─────────────────────────

    pub fn add_media(&self, name: &str, bytes: &[u8]) -> Result<String> {
        media::add_media(&self.core, name, bytes)
    }
    pub fn get_media(&self, name: &str) -> Result<Option<Vec<u8>>> {
        media::get_media(&self.core, name)
    }
    pub fn check_media(&self) -> Result<MediaCheck> {
        media::check_media(&self.core)
    }
    pub fn delete_media(&self, names: &[String]) -> Result<()> {
        media::delete_media(&self.core, names)
    }

    // ───────────────────────── card state ─────────────────────────

    pub fn suspend_cards(&self, ids: &[CardId], suspend: bool) -> Result<()> {
        cards::suspend_cards(&self.core, ids, suspend)
    }
    pub fn bury_cards(&self, ids: &[CardId]) -> Result<()> {
        cards::bury_cards(&self.core, ids)
    }
    pub fn set_flag(&self, ids: &[CardId], flag: i64) -> Result<()> {
        cards::set_flag(&self.core, ids, flag)
    }
    pub fn unbury_deck(&self, deck_id: Option<DeckId>) -> Result<i64> {
        cards::unbury_deck(&self.core, deck_id)
    }
    /// Raises today's limits, then refills a running non-custom session (collection.ts:339).
    pub fn extend_today_limits(&mut self, deck_id: DeckId, extra_new: i64, extra_review: i64) -> Result<StudyCounts> {
        self.rollover()?;
        let counts = decks::extend_today_limits(&self.core, self.counter(), deck_id, extra_new, extra_review)?;
        self.refill_session()?;
        Ok(counts)
    }
    pub fn resolve_leech(&self, card_id: CardId, action: LeechResolution) -> Result<()> {
        cards::resolve_leech(&self.core, card_id, action)
    }

    // ───────────────────────── undo ─────────────────────────

    /// Reverts the last operation; a running session is re-synced (collection.ts:356).
    pub fn undo(&mut self) -> Result<Option<UndoResult>> {
        let label = undo::undo(self.core.db())?;
        if let Some(label) = label {
            self.core.invalidate_caches();
            self.core.mark_changed()?;
            self.session_after_undo()?;
            return Ok(Some(UndoResult { label }));
        }
        Ok(None)
    }
    pub fn undo_label(&self) -> Result<Option<String>> {
        undo::peek_label(self.core.db())
    }

    // ───────────────────────── saved searches ─────────────────────────

    pub fn save_search(&self, name: &str, query: &str) -> Result<()> {
        let n = name.trim();
        if n.is_empty() {
            return Err(LociError::invalid("Saved search name is empty"));
        }
        self.core.db().run(
            "INSERT INTO saved_searches (name, query) VALUES (?, ?) ON CONFLICT(name) DO UPDATE SET query = excluded.query",
            [n, query],
        )?;
        Ok(())
    }
    pub fn delete_saved_search(&self, name: &str) -> Result<()> {
        self.core.db().run("DELETE FROM saved_searches WHERE name = ?", [name.trim()])?;
        Ok(())
    }

    // ───────────────────────── sample deck ─────────────────────────

    pub fn install_sample_deck(&self) -> Result<DeckId> {
        notes::install_sample_deck(&self.core)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Port of recall `tests/unit/collection/helpers.ts`: a fixed clock, UTC, a temp dir.
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    /// 2026-03-10 12:00:00 UTC
    pub const T0: Millis = 1_773_144_000_000;

    /// A controllable clock (ms), shared with the collection.
    #[derive(Clone)]
    pub struct TestClock(pub Arc<AtomicI64>);

    impl TestClock {
        pub fn ms(&self) -> Millis {
            self.0.load(Ordering::SeqCst)
        }
        pub fn set(&self, ms: Millis) {
            self.0.store(ms, Ordering::SeqCst);
        }
        pub fn advance(&self, ms: Millis) {
            self.0.fetch_add(ms, Ordering::SeqCst);
        }
        pub fn clock_fn(&self) -> Clock {
            let c = self.0.clone();
            Box::new(move || c.load(Ordering::SeqCst))
        }
    }

    pub struct Harness {
        pub col: Collection,
        pub clock: TestClock,
        /// Keeps the temp dir alive for the harness's lifetime.
        pub dir: tempfile::TempDir,
    }

    impl Harness {
        pub fn db_path(&self) -> PathBuf {
            self.dir.path().join(COLLECTION_FILE)
        }
        pub fn blobs(&self) -> &BlobStore {
            &self.col.core.blobs
        }
        /// A second collection on the same directory (after `close()`), sharing the clock.
        pub fn reopen(&self) -> Collection {
            let mut col = Collection::open_dir(self.dir.path(), Some(self.clock.clock_fn()), Some(Box::new(|_| 0))).unwrap();
            col.open().unwrap();
            col
        }
        pub fn count(&self, sql: &str) -> i64 {
            self.col.core.db().count(sql, []).unwrap()
        }
        pub fn count_p<P: rusqlite::Params>(&self, sql: &str, params: P) -> i64 {
            self.col.core.db().count(sql, params).unwrap()
        }
        pub fn meta(&self, key: &str) -> Option<String> {
            self.col.core.get_meta(key).unwrap()
        }
        pub fn notetype_by_name(&self, name: &str) -> Notetype {
            self.col
                .list_notetypes()
                .unwrap()
                .into_iter()
                .find(|n| n.name == name)
                .unwrap_or_else(|| panic!("no notetype {name}"))
        }
        pub fn add_note(&self, nt: &Notetype, deck: DeckId, fields: &[&str], tags: &[&str]) -> AddNoteResult {
            self.col.add_note(nt.id, deck, &strs(fields), &strs(tags)).unwrap()
        }
    }

    pub fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Opens a fresh file-backed collection in a temp dir, UTC time zone, fixed clock at `start`.
    pub fn make_collection_at(start: Millis) -> Harness {
        let dir = tempfile::tempdir().expect("tempdir");
        let clock = TestClock(Arc::new(AtomicI64::new(start)));
        let mut col = Collection::open_dir(dir.path(), Some(clock.clock_fn()), Some(Box::new(|_| 0))).expect("open_dir");
        let report = col.open().expect("open");
        assert!(report.ok, "open failed: {:?}", report.problems);
        Harness { col, clock, dir }
    }

    pub fn make_collection() -> Harness {
        make_collection_at(T0)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::collection::settings::parse_patch;
    use serde_json::json;

    const H: Millis = 3_600_000;
    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Millis {
        crate::collection::time::utc(y, mo, d, h, mi, 0)
    }

    // ── schema.test.ts ──
    #[test]
    fn creates_every_table_seeds_and_passes_integrity() {
        let h = make_collection();
        let tables: Vec<String> =
            h.col.core.db().all("SELECT name FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
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
            assert!(tables.iter().any(|x| x == t), "missing {t}");
        }
        assert_eq!(h.meta("schema_version").as_deref(), Some("1"));
        let id = h.meta("collection_id").unwrap();
        assert_eq!(id.len(), 36);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(h.meta("next_pos").as_deref(), Some("1"));
        // crt = most recent local 04:00 before T0 (UTC): 2026-03-10 04:00
        assert_eq!(h.meta("crt").unwrap(), (utc(2026, 3, 10, 4, 0) / 1000).to_string());
        assert_eq!(h.col.get_deck(1).unwrap().name, "Default");
        let presets: Vec<String> = h.col.list_presets().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(presets, ["Standard", "Relaxed", "Intense"]);
        let mut names: Vec<String> = h.col.list_notetypes().unwrap().into_iter().map(|n| n.name).collect();
        names.sort();
        assert_eq!(names, ["Basic", "Basic (and reversed card)", "Basic (type in the answer)", "Cloze", "Image Occlusion"]);
        assert!(h.col.is_empty().unwrap());
        assert_eq!(h.col.check_integrity().unwrap(), IntegrityReport { ok: true, problems: vec![], repaired: vec![] });
    }

    #[test]
    fn reopening_is_idempotent_and_keeps_data() {
        let mut h = make_collection();
        let basic = h.notetype_by_name("Basic");
        h.add_note(&basic, 1, &["q", "a"], &[]);
        h.col.close().unwrap();
        h.col.close().unwrap(); // idempotent
        let col2 = h.reopen();
        assert_eq!(col2.core.db().count("SELECT count(*) FROM notes", []).unwrap(), 1);
        assert_eq!(col2.list_notetypes().unwrap().len(), 5);
        assert_eq!(col2.list_presets().unwrap().len(), 3);
        h.col = col2;
    }

    #[test]
    fn refuses_a_collection_written_by_a_newer_schema() {
        let db = Db::memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut col = Collection::open_with(db, BlobStore::new(dir.path()), None, None);
        col.open().unwrap();
        col.core.db().run("UPDATE col_meta SET value = '99' WHERE key = 'schema_version'", []).unwrap();
        col.core.invalidate_caches();
        let err = col.open().unwrap_err();
        assert_eq!(err.code, ErrorCode::Invalid);
        assert_eq!(col.open().unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn built_in_note_types_match_anki_stock_definitions() {
        let h = make_collection();
        let by = |n: &str| h.notetype_by_name(n);
        let basic = by("Basic");
        assert_eq!(basic.templates[0].name, "Card 1");
        assert_eq!(basic.templates[0].qfmt, "{{Front}}");
        assert_eq!(basic.templates[0].afmt, "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}");
        let rev = by("Basic (and reversed card)");
        assert_eq!(rev.templates.iter().map(|t| t.qfmt.as_str()).collect::<Vec<_>>(), ["{{Front}}", "{{Back}}"]);
        assert_eq!(by("Basic (type in the answer)").templates[0].qfmt, "{{Front}}\n\n{{type:Back}}");
        let cloze = by("Cloze");
        assert_eq!(cloze.kind, NotetypeKind::Cloze);
        assert_eq!(cloze.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Text", "Back Extra"]);
        assert_eq!(cloze.templates[0].qfmt, "{{cloze:Text}}");
        assert_eq!(cloze.templates[0].afmt, "{{cloze:Text}}<br>\n{{Back Extra}}");
        let io = by("Image Occlusion");
        assert_eq!(io.kind, NotetypeKind::Cloze);
        assert_eq!(
            io.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["Occlusion", "Image", "Header", "Back Extra", "Comments"]
        );
        assert!(io.templates[0].qfmt.contains("{{cloze:Occlusion}}"));
        assert!(io.templates[0].qfmt.contains("{{Image}}"));
        assert!(io.templates[0].afmt.contains("<button id=\"toggle\">Toggle Masks</button>"));
        assert_eq!(io.config.builtin, Some(BuiltinKind::ImageOcclusion));
        assert_eq!(io.config.occlusion_mode, Some(OcclusionMode::HideAll));
        assert_eq!(basic.config.builtin, Some(BuiltinKind::Basic));
        assert_eq!(basic.config.req, vec![Requirement(0, ReqKind::Any, vec![0])]);
        assert!(basic.css.contains(".card"));
    }

    // ── settings-time.test.ts (Collection-level) ──
    #[test]
    fn settings_defaults_for_a_new_collection() {
        let h = make_collection();
        let s = h.col.get_settings().unwrap();
        assert_eq!(s, default_settings());
        assert_eq!(s.rollover_hour, 4);
        assert_eq!(s.learn_ahead_minutes, 20);
    }

    #[test]
    fn settings_persist_patches_and_validate_them() {
        let h = make_collection();
        let patch =
            parse_patch(&json!({"rolloverHour": 6, "theme": "dark", "cardFontSize": 24, "showKeyboardHints": false})).unwrap();
        let s = h.col.set_settings(&patch).unwrap();
        assert_eq!(
            (s.rollover_hour, s.theme, s.card_font_size, s.show_keyboard_hints, s.locale.as_str()),
            (6, ThemePref::Dark, 24, false, "en")
        );
        assert_eq!(h.meta("rollover_hour").as_deref(), Some("6"));
        assert_eq!(h.meta("theme").as_deref(), Some("dark"));
        h.col.core.invalidate_caches();
        assert_eq!(h.col.get_settings().unwrap().theme, ThemePref::Dark);
        let p24 = parse_patch(&json!({"rolloverHour": 24})).unwrap();
        assert_eq!(h.col.set_settings(&p24).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(parse_patch(&json!({"rolloverHour": 2.5})).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(parse_patch(&json!({"theme": "neon"})).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(parse_patch(&json!({"bogus": 1})).unwrap_err().code, ErrorCode::Invalid);
        // The failed patch left the stored value alone.
        assert_eq!(h.col.get_settings().unwrap().rollover_hour, 6);
    }

    #[test]
    fn settings_fall_back_to_defaults_for_malformed_stored_values() {
        let h = make_collection();
        h.col.core.set_meta("rollover_hour", Some("garbage")).unwrap();
        h.col.core.set_meta("theme", Some("purple")).unwrap();
        let s = h.col.get_settings().unwrap();
        assert_eq!(s.rollover_hour, 4);
        assert_eq!(s.theme, ThemePref::System);
    }

    #[test]
    fn today_advances_at_the_rollover_hour_and_follows_the_setting() {
        let h = make_collection_at(utc(2026, 3, 10, 12, 0));
        assert_eq!(h.col.today().unwrap(), 0);
        h.clock.set(utc(2026, 3, 11, 3, 59));
        assert_eq!(h.col.today().unwrap(), 0);
        h.clock.set(utc(2026, 3, 11, 4, 0));
        assert_eq!(h.col.today().unwrap(), 1);
        h.col.set_settings(&CollectionSettingsPatch { rollover_hour: Some(5), ..Default::default() }).unwrap();
        assert_eq!(h.col.today().unwrap(), 0);
        h.clock.advance(H);
        assert_eq!(h.col.today().unwrap(), 1);
        assert_eq!(h.col.core.next_day_at().unwrap(), utc(2026, 3, 12, 5, 0) / 1000);
    }

    // ── undo-cards.test.ts: saved searches ──
    #[test]
    fn saved_searches() {
        let h = make_collection();
        h.col.save_search("Leeches", "tag:leech").unwrap();
        h.col.save_search("Leeches", "tag:leech is:suspended").unwrap();
        let rows: Vec<(String, String)> =
            h.col.core.db().all("SELECT name, query FROM saved_searches", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(rows, vec![("Leeches".to_string(), "tag:leech is:suspended".to_string())]);
        h.col.delete_saved_search("Leeches").unwrap();
        assert_eq!(h.count("SELECT count(*) FROM saved_searches"), 0);
        assert_eq!(h.col.save_search("  ", "x").unwrap_err().code, ErrorCode::Invalid);
    }

    // ── decks.test.ts: presets ──
    #[test]
    fn presets_crud_and_decks_fall_back_to_preset_1() {
        let h = make_collection();
        let mut cfg = h.col.get_preset(1).unwrap().config;
        cfg.new_per_day = 50;
        let p = h.col.save_preset(&DeckConfigPresetInput { id: None, name: "Exam".into(), config: cfg }).unwrap();
        assert_eq!(p.config.new_per_day, 50);
        assert_eq!(p.config.fsrs_params.len(), 21);
        let d = h.col.create_deck("Exam", Some(p.id)).unwrap();
        let child = h.col.create_deck("Exam::Week 1", None).unwrap();
        assert_eq!(h.col.get_deck(child.id).unwrap().config_id, p.id);
        assert_eq!(h.col.core.deck_config(child.id).unwrap().new_per_day, 50);
        let mut cfg2 = p.config.clone();
        cfg2.desired_retention = 0.95;
        h.col.save_preset(&DeckConfigPresetInput { id: Some(p.id), name: "Exam 2".into(), config: cfg2 }).unwrap();
        assert_eq!(h.col.get_preset(p.id).unwrap().config.desired_retention, 0.95);
        assert_eq!(h.col.get_preset(p.id).unwrap().name, "Exam 2");
        h.col.delete_preset(p.id).unwrap();
        assert_eq!(h.col.get_deck(d.id).unwrap().config_id, 1);
        assert_eq!(h.col.delete_preset(1).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(h.col.set_deck_preset(d.id, 999).unwrap_err().code, ErrorCode::NotFound);
        h.col.set_deck_collapsed(d.id, true).unwrap();
        h.col.set_deck_description(d.id, "hello").unwrap();
        let deck = h.col.get_deck(d.id).unwrap();
        assert!(deck.collapsed);
        assert_eq!(deck.description, "hello");
        let empty = DeckConfigPresetInput { id: None, name: "  ".into(), config: DeckConfig::default() };
        assert_eq!(h.col.save_preset(&empty).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn save_preset_keeps_a_temporary_retention_marker_only_when_the_value_is_unchanged() {
        let h = make_collection();
        let p = h.col.get_preset(1).unwrap();
        let mut raw = serde_json::to_value(&p.config).unwrap();
        raw["desiredRetention"] = json!(0.8);
        raw[TEMP_RETENTION_KEY] = json!({"original": 0.9, "retention": 0.8, "untilDay": 7});
        h.col.core.db().run("UPDATE deck_configs SET config = ? WHERE id = 1", [raw.to_string()]).unwrap();
        let mut cfg = h.col.get_preset(1).unwrap().config;
        assert_eq!(cfg.desired_retention, 0.8);
        h.col.save_preset(&DeckConfigPresetInput { id: Some(1), name: "Standard".into(), config: cfg.clone() }).unwrap();
        let stored: String = h.col.core.db().scalar("SELECT config FROM deck_configs WHERE id = 1", []).unwrap().unwrap();
        assert!(parse_json_object(&stored).contains_key(TEMP_RETENTION_KEY));
        cfg.desired_retention = 0.9;
        h.col.save_preset(&DeckConfigPresetInput { id: Some(1), name: "Standard".into(), config: cfg }).unwrap();
        let stored: String = h.col.core.db().scalar("SELECT config FROM deck_configs WHERE id = 1", []).unwrap().unwrap();
        assert!(!parse_json_object(&stored).contains_key(TEMP_RETENTION_KEY));
    }
}
