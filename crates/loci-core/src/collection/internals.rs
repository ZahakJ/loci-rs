//! `CollectionCore`: the synchronous core shared by the `Collection` service and by the sibling
//! modules (study, search, stats, import/export) — recall `src/core/collection/internals.ts`.
//! Everything here runs inside the caller's transaction when there is one. Caches use interior
//! mutability so every module can hold `&CollectionCore`.

use crate::blobs::BlobStore;
use crate::collection::presets::{normalise_deck_config, standard_deck_config};
use crate::collection::rows::{self, DECK_COLUMNS, DECK_CONFIG_COLUMNS, NOTETYPE_COLUMNS};
use crate::collection::settings::{decode_settings, KEY_ROLLOVER_HOUR};
use crate::collection::time::{sched_timing, system_tz_offset, SchedTiming};
use crate::db::{unicase_compare, Db};
use crate::error::{LociError, Result};
use crate::types::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

/// Tables whose ids are allocated from the clock (ms), Anki-style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdTable {
    Notes,
    Cards,
    Revlog,
    Notetypes,
    Decks,
    DeckConfigs,
}

impl IdTable {
    pub fn name(self) -> &'static str {
        match self {
            IdTable::Notes => "notes",
            IdTable::Cards => "cards",
            IdTable::Revlog => "revlog",
            IdTable::Notetypes => "notetypes",
            IdTable::Decks => "decks",
            IdTable::DeckConfigs => "deck_configs",
        }
    }
}

/// col_meta keys owned by the collection core.
pub mod meta {
    pub const SCHEMA_VERSION: &str = "schema_version";
    pub const COLLECTION_ID: &str = "collection_id";
    pub const CRT: &str = "crt";
    /// UTC offset at creation, minutes west (Anki creationOffset).
    pub const CREATION_OFFSET: &str = "creation_offset";
    pub const CREATED_WITH: &str = "created_with";
    /// Next new-card position (Anki nextPos).
    pub const NEXT_POS: &str = "next_pos";
    /// Seconds of the last automatic or manual backup.
    pub const LAST_BACKUP: &str = "last_backup";
    /// Seconds of the last user-visible change.
    pub const LAST_CHANGE: &str = "last_change";
    pub const LAST_OPTIMISE: &str = "last_optimise";
    /// Version of the plain-text rules `notes.plain` was built with (`text::PLAIN_VERSION`; absent = 1).
    pub const PLAIN_VERSION: &str = "plain_version";
    /// Collection day on which buried cards were last returned to their queues (day rollover).
    pub const LAST_UNBURIED: &str = "last_unburied";
    /// JSON {crt, creationOffset} of the Anki collection the first import into an empty
    /// collection came from; exports write that crt so a package going back to Anki keeps its day numbering.
    pub const SOURCE_CRT: &str = "source_crt";
}

/// Id of the Default deck (Anki convention).
pub const DEFAULT_DECK_ID: DeckId = 1;
/// Id of the default preset every deck falls back to.
pub const DEFAULT_PRESET_ID: DeckConfigId = 1;

/// Clock in ms; injectable for tests.
pub type Clock = Box<dyn Fn() -> Millis + Send + Sync>;
/// UTC offset in minutes west at an instant (ms); injectable for tests.
pub type TzOffsetFn = Box<dyn Fn(Millis) -> i64 + Send + Sync>;

pub struct CollectionCore {
    db: Db,
    pub blobs: BlobStore,
    clock: Clock,
    tz: TzOffsetFn,
    last_ids: RefCell<HashMap<IdTable, i64>>,
    notetype_cache: RefCell<Option<BTreeMap<NotetypeId, Notetype>>>,
    settings_cache: RefCell<Option<CollectionSettings>>,
    meta_cache: RefCell<HashMap<String, Option<String>>>,
}

impl std::fmt::Debug for CollectionCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectionCore").field("db", &self.db).field("blobs", &self.blobs.root()).finish()
    }
}

/// Strips characters Anki does not allow in tags and empty `::` components.
fn clean_tag(raw: &str) -> String {
    let s: String = raw.chars().filter(|c| !(c.is_control() || *c == '"' || *c == '\u{3000}')).collect();
    s.trim().split("::").map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("::")
}

impl CollectionCore {
    pub fn new(db: Db, blobs: BlobStore, clock: Option<Clock>, tz: Option<TzOffsetFn>) -> CollectionCore {
        CollectionCore {
            db,
            blobs,
            clock: clock.unwrap_or_else(|| Box::new(|| chrono::Utc::now().timestamp_millis())),
            tz: tz.unwrap_or_else(|| Box::new(system_tz_offset)),
            last_ids: RefCell::new(HashMap::new()),
            notetype_cache: RefCell::new(None),
            settings_cache: RefCell::new(None),
            meta_cache: RefCell::new(HashMap::new()),
        }
    }

    /// The live connection (replaced after a restore).
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Swaps the live connection (restore/reset) and drops caches. Returns the old one.
    pub fn set_db(&mut self, db: Db) -> Db {
        let old = std::mem::replace(&mut self.db, db);
        self.last_ids.borrow_mut().clear();
        self.invalidate_caches();
        old
    }

    /// Current time in ms.
    pub fn now(&self) -> Millis {
        (self.clock)()
    }
    /// Current time in whole seconds.
    pub fn now_seconds(&self) -> Seconds {
        self.now().div_euclid(1000)
    }
    /// UTC offset in minutes west at `ms`.
    pub fn tz_offset(&self, ms: Millis) -> i64 {
        (self.tz)(ms)
    }
    /// Collection creation time (seconds), the origin of review-card day numbers.
    pub fn crt(&self) -> Result<Seconds> {
        self.get_meta(meta::CRT)?.and_then(|v| v.parse::<i64>().ok()).ok_or_else(|| LociError::internal("Collection is not open"))
    }
    fn timing(&self) -> Result<SchedTiming> {
        let now = self.now();
        let crt = self.crt()?;
        let creation_offset = self
            .get_meta(meta::CREATION_OFFSET)?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or_else(|| self.tz_offset(crt * 1000));
        Ok(sched_timing(crt, creation_offset, now, self.tz_offset(now), self.settings()?.rollover_hour))
    }
    /// Collection day number for now, honouring the rollover hour.
    pub fn today(&self) -> Result<CollectionDay> {
        Ok(self.timing()?.days_elapsed)
    }
    /// Start of the next collection day, seconds since epoch.
    pub fn next_day_at(&self) -> Result<Seconds> {
        Ok(self.timing()?.next_day_at)
    }

    pub fn settings(&self) -> Result<CollectionSettings> {
        if let Some(s) = self.settings_cache.borrow().as_ref() {
            return Ok(s.clone());
        }
        let s = decode_settings(|k| self.get_meta(k).ok().flatten());
        *self.settings_cache.borrow_mut() = Some(s.clone());
        Ok(s)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        if let Some(v) = self.meta_cache.borrow().get(key) {
            return Ok(v.clone());
        }
        let v: Option<String> = self.db.scalar("SELECT value FROM col_meta WHERE key = ?", [key])?;
        self.meta_cache.borrow_mut().insert(key.to_string(), v.clone());
        Ok(v)
    }

    /// Sets (or with None deletes) a col_meta key.
    pub fn set_meta(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            None => self.db.run("DELETE FROM col_meta WHERE key = ?", [key])?,
            Some(v) => self.db.run(
                "INSERT INTO col_meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, v],
            )?,
        };
        self.meta_cache.borrow_mut().insert(key.to_string(), value.map(str::to_string));
        if key == KEY_ROLLOVER_HOUR || crate::collection::settings::SETTING_KEYS.contains(&key) {
            *self.settings_cache.borrow_mut() = None;
        }
        Ok(())
    }

    /// A fresh unique id for `table`: max(now ms, last issued + 1), skipping existing ids.
    pub fn new_id(&self, table: IdTable) -> Result<i64> {
        let last = self.last_ids.borrow().get(&table).copied().unwrap_or(0);
        let mut id = self.now().max(last + 1);
        let sql = format!("SELECT 1 FROM {} WHERE id = ?", table.name());
        while self.db.scalar::<i64, _>(&sql, [id])?.is_some() {
            id += 1;
        }
        self.last_ids.borrow_mut().insert(table, id);
        Ok(id)
    }

    /// Allocates the next new-card position (col_meta next_pos), incrementing it.
    pub fn allocate_new_position(&self) -> Result<i64> {
        let pos = self
            .get_meta(meta::NEXT_POS)?
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| v.floor() as i64)
            .unwrap_or(1);
        self.set_meta(meta::NEXT_POS, Some(&(pos + 1).to_string()))?;
        Ok(pos)
    }

    /// Records that user data changed now (drives the 30-minute backup rule).
    pub fn mark_changed(&self) -> Result<()> {
        self.set_meta(meta::LAST_CHANGE, Some(&self.now_seconds().to_string()))
    }

    fn load_notetypes(&self) -> Result<BTreeMap<NotetypeId, Notetype>> {
        if let Some(c) = self.notetype_cache.borrow().as_ref() {
            return Ok(c.clone());
        }
        let nts = self.db.all(&rows::select_sql("notetypes", &NOTETYPE_COLUMNS), [], rows::row_to_notetype_row)?;
        let fields = self.db.all("SELECT ntid, ord, name, config FROM fields ORDER BY ntid, ord", [], rows::row_to_field_row)?;
        let templates = self.db.all(
            "SELECT ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config FROM templates ORDER BY ntid, ord",
            [],
            rows::row_to_template_row,
        )?;
        let mut f_by: HashMap<i64, Vec<rows::FieldRow>> = HashMap::new();
        for f in fields {
            f_by.entry(f.ntid).or_default().push(f);
        }
        let mut t_by: HashMap<i64, Vec<rows::TemplateRow>> = HashMap::new();
        for t in templates {
            t_by.entry(t.ntid).or_default().push(t);
        }
        let mut map = BTreeMap::new();
        for r in nts {
            let nt = rows::rows_to_notetype(&r, f_by.remove(&r.id).unwrap_or_default(), t_by.remove(&r.id).unwrap_or_default());
            map.insert(r.id, nt);
        }
        *self.notetype_cache.borrow_mut() = Some(map.clone());
        Ok(map)
    }

    pub fn notetype(&self, id: NotetypeId) -> Result<Notetype> {
        self.load_notetypes()?.remove(&id).ok_or_else(|| LociError::not_found(format!("Note type {id} not found")))
    }

    /// All note types sorted by name (unicase).
    pub fn all_notetypes(&self) -> Result<Vec<Notetype>> {
        let mut v: Vec<Notetype> = self.load_notetypes()?.into_values().collect();
        v.sort_by(|a, b| unicase_compare(&a.name, &b.name));
        Ok(v)
    }

    pub fn deck(&self, id: DeckId) -> Result<Deck> {
        self.db
            .get(&format!("{} WHERE id = ?", rows::select_sql("decks", &DECK_COLUMNS)), [id], rows::row_to_deck)?
            .ok_or_else(|| LociError::not_found(format!("Deck {id} not found")))
    }

    pub fn all_decks(&self) -> Result<Vec<Deck>> {
        self.db.all(&rows::select_sql("decks", &DECK_COLUMNS), [], rows::row_to_deck)
    }

    pub fn deck_id_by_name(&self, name: &str) -> Result<Option<DeckId>> {
        self.db.scalar("SELECT id FROM decks WHERE name = ?", [name])
    }

    pub fn preset(&self, id: DeckConfigId) -> Result<DeckConfigPreset> {
        self.db
            .get(&format!("{} WHERE id = ?", rows::select_sql("deck_configs", &DECK_CONFIG_COLUMNS)), [id], rows::row_to_preset)?
            .ok_or_else(|| LociError::not_found(format!("Preset {id} not found")))
    }

    pub fn all_presets(&self) -> Result<Vec<DeckConfigPreset>> {
        self.db.all(&format!("{} ORDER BY id", rows::select_sql("deck_configs", &DECK_CONFIG_COLUMNS)), [], rows::row_to_preset)
    }

    /// The complete config of the preset used by a deck (falls back to preset 1, then defaults).
    pub fn deck_config(&self, deck_id: DeckId) -> Result<DeckConfig> {
        let joined = self.db.get(
            "SELECT c.id, c.name, c.config, c.mtime, c.usn FROM decks d JOIN deck_configs c ON c.id = d.config_id WHERE d.id = ?",
            [deck_id],
            rows::row_to_preset,
        )?;
        if let Some(p) = joined {
            return Ok(p.config);
        }
        let fallback = self.db.get(
            &format!("{} WHERE id = ?", rows::select_sql("deck_configs", &DECK_CONFIG_COLUMNS)),
            [DEFAULT_PRESET_ID],
            rows::row_to_preset,
        )?;
        Ok(fallback
            .map(|p| p.config)
            .unwrap_or_else(|| normalise_deck_config(&serde_json::to_value(standard_deck_config()).unwrap_or_default())))
    }

    /// Drops cached note types/decks/settings (after bulk writes by other modules).
    pub fn invalidate_caches(&self) {
        *self.notetype_cache.borrow_mut() = None;
        *self.settings_cache.borrow_mut() = None;
        self.meta_cache.borrow_mut().clear();
    }

    /// Normalises tags: split on whitespace, clean, dedupe case-insensitively, reuse registered
    /// casing, sort (unicase).
    pub fn canonical_tags(&self, tags: &[String]) -> Result<Vec<String>> {
        let mut out: Vec<(String, String)> = Vec::new();
        for raw in tags {
            for part in raw.split_whitespace() {
                let t = clean_tag(part);
                if t.is_empty() {
                    continue;
                }
                let key = t.to_lowercase();
                if out.iter().any(|(k, _)| *k == key) {
                    continue;
                }
                let registered: Option<String> = self.db.scalar("SELECT tag FROM tags WHERE tag = ?", [&t])?;
                out.push((key, registered.unwrap_or(t)));
            }
        }
        let mut v: Vec<String> = out.into_iter().map(|(_, t)| t).collect();
        v.sort_by(|a, b| unicase_compare(a, b));
        Ok(v)
    }

    /// Adds tags to the tag registry (no-op for registered ones).
    pub fn register_tags(&self, tags: &[String]) -> Result<()> {
        for t in tags {
            if !t.is_empty() {
                self.db.run("INSERT OR IGNORE INTO tags (tag) VALUES (?)", [t])?;
            }
        }
        Ok(())
    }

    /// Inserts new cards for `ords` of `note` (no existence check). Deck: the template's deck
    /// override when that deck exists, else `deck_id`. Due: the position of an existing new
    /// sibling, else a freshly allocated one. Returns the card ids.
    pub fn insert_new_cards(&self, note: &Note, nt: &Notetype, ords: &[i64], deck_id: DeckId) -> Result<Vec<CardId>> {
        if ords.is_empty() {
            return Ok(Vec::new());
        }
        let sibling: Option<i64> = self.db.scalar(
            "SELECT due FROM cards WHERE nid = ? AND type = ? ORDER BY ord LIMIT 1",
            rusqlite::params![note.id, CardType::New as i64],
        )?;
        let due = match sibling {
            Some(d) => d,
            None => self.allocate_new_position()?,
        };
        let mtime = self.now_seconds();
        let insert = rows::insert_sql("cards", &rows::CARD_COLUMNS);
        let mut ids = Vec::with_capacity(ords.len());
        for &ord in ords {
            let tmpl =
                if nt.kind == NotetypeKind::Cloze { nt.templates.first() } else { nt.templates.iter().find(|t| t.ord == ord) };
            let override_deck = tmpl.and_then(|t| t.config.deck_override);
            let did = match override_deck {
                Some(o) if self.db.scalar::<i64, _>("SELECT 1 FROM decks WHERE id = ?", [o])?.is_some() => o,
                _ => deck_id,
            };
            let id = self.new_id(IdTable::Cards)?;
            let card = Card {
                id,
                nid: note.id,
                did,
                ord,
                mtime,
                usn: -1,
                ctype: CardType::New,
                queue: CardQueue::New,
                due,
                ivl: 0,
                factor: 0,
                reps: 0,
                lapses: 0,
                left: 0,
                odue: 0,
                odid: 0,
                flags: 0,
                data: CardData::default(),
            };
            self.db.run(&insert, rusqlite::params_from_iter(rows::card_params(&card)))?;
            ids.push(id);
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> CollectionCore {
        let db = Db::memory().unwrap();
        db.execute_batch(crate::SCHEMA_SQL).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::new(dir.keep());
        let core = CollectionCore::new(db, blobs, Some(Box::new(|| 1_773_144_000_000)), Some(Box::new(|_| 0)));
        core.set_meta(meta::CRT, Some("1773115200")).unwrap(); // 2026-03-10 04:00 UTC
        core.set_meta(meta::CREATION_OFFSET, Some("0")).unwrap();
        core
    }

    #[test]
    fn meta_cache_and_settings_invalidate() {
        let c = core();
        assert_eq!(c.get_meta("nope").unwrap(), None);
        c.set_meta("rollover_hour", Some("6")).unwrap();
        assert_eq!(c.settings().unwrap().rollover_hour, 6);
        c.set_meta("rollover_hour", None).unwrap();
        assert_eq!(c.settings().unwrap().rollover_hour, 4);
        assert_eq!(c.today().unwrap(), 0);
        assert_eq!(c.next_day_at().unwrap(), 1773115200 + 86400);
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let c = core();
        let a = c.new_id(IdTable::Cards).unwrap();
        let b = c.new_id(IdTable::Cards).unwrap();
        assert_eq!(a, 1_773_144_000_000);
        assert_eq!(b, a + 1);
        assert_eq!(c.allocate_new_position().unwrap(), 1);
        assert_eq!(c.allocate_new_position().unwrap(), 2);
    }

    #[test]
    fn tags_are_cleaned_deduped_and_sorted() {
        let c = core();
        c.register_tags(&["Marked".into()]).unwrap();
        let t = c.canonical_tags(&["b marked".into(), "\"a\"".into(), "x::y::".into(), "A".into()]).unwrap();
        assert_eq!(t, vec!["a", "b", "Marked", "x::y"]);
    }

    #[test]
    fn deck_config_falls_back_to_defaults() {
        let c = core();
        assert_eq!(c.deck_config(42).unwrap().new_per_day, 20);
        assert!(c.deck(1).is_err());
    }
}
