//! `AnkiImage`: an in-memory, Loci-typed picture of one Anki package (collection + media index).
//!
//! `read_package` produces it from bytes; `write_legacy_package` turns it back into an `.apkg`.
//! The importer (collection module) maps it into a Loci collection; the exporter builds one from
//! Loci. Units follow Anki and `types.rs`: ids ms, mtime s, crt s, review `due` = days since the
//! crt day, learning `due` = unix seconds, new `due` = position.

use crate::error::Result;
use crate::types::*;
use std::collections::{BTreeMap, HashMap};

/// A schema-11 JSON object (`col.conf`, a deck, a note type, a dconf preset) with key order kept.
pub type Json = serde_json::Map<String, serde_json::Value>;

/// Collection-wide Anki settings (`col.conf` JSON in schema 11, the `config` table in schema 18).
#[derive(Debug, Clone, PartialEq)]
pub struct AnkiCollectionConfig {
    /// 1 = v1 scheduler (converted on read, so `read_package` always reports 2), 2 = v2/v3.
    pub sched_ver: i64,
    /// v3 scheduler flag (`sched2021`).
    pub sched2021: bool,
    /// Day rollover hour, 0–23 (`rollover`, default 4).
    pub rollover: i64,
    /// UTC offset at creation, minutes WEST of UTC (`creationOffset`), or None (legacy day logic).
    pub creation_offset: Option<i64>,
    /// Current UTC offset, minutes west (`localOffset`), or None.
    pub local_offset: Option<i64>,
    /// Next new-card position (`nextPos`).
    pub next_pos: i64,
    /// FSRS enabled collection-wide (`fsrs`).
    pub fsrs: bool,
    /// Learn-ahead limit, seconds (`collapseTime`, default 1200).
    pub collapse_time: i64,
    /// Show next intervals on answer buttons (`estTimes`).
    pub est_times: bool,
    /// Current deck id (`curDeck`).
    pub cur_deck: DeckId,
    /// Day number of the last automatic unbury (`lastUnburied`), if known.
    pub last_unburied: Option<i64>,
    /// Every key of the original config, verbatim (JSON values).
    pub raw: Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnkiDeck {
    pub id: DeckId,
    /// Full name with `::` separators (schema 18's U+001F separators are converted).
    pub name: String,
    pub description: String,
    /// Deck options preset; for filtered decks 0.
    pub config_id: DeckConfigId,
    /// true for a filtered (dynamic) deck (Anki `dyn`). `read_package` drops filtered decks, so
    /// only the writer and hand-built images see true.
    pub filtered: bool,
    pub collapsed: bool,
    pub browser_collapsed: bool,
    /// Study counters of the last day studied (Anki `newToday` etc.; time in ms).
    pub today: DeckToday,
    pub extend_new: i64,
    pub extend_rev: i64,
    /// Per-deck desired-retention override (0–1), if set.
    pub desired_retention: Option<f64>,
    pub mtime: Seconds,
    pub usn: i64,
    /// Schema-11 JSON object of this deck as found in the package (schema 18: an equivalent object
    /// built from the protobuf blobs, unknown legacy keys included). Used by the writer to
    /// round-trip keys Loci does not model. None when the deck was not read from a package.
    pub original_json: Option<Json>,
}

impl AnkiDeck {
    /// A fresh normal deck with Anki's defaults.
    pub fn blank(id: DeckId, name: impl Into<String>) -> AnkiDeck {
        AnkiDeck {
            id,
            name: name.into(),
            description: String::new(),
            config_id: 1,
            filtered: false,
            collapsed: false,
            browser_collapsed: false,
            today: EMPTY_DECK_TODAY,
            extend_new: 0,
            extend_rev: 0,
            desired_retention: None,
            mtime: 0,
            usn: 0,
            original_json: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnkiDeckConfig {
    pub id: DeckConfigId,
    pub name: String,
    /// Mapped preset (see `deck_config.rs` for the dconf ⇄ DeckConfig table).
    pub config: DeckConfig,
    pub mtime: Seconds,
    pub usn: i64,
    /// Schema-11 dconf JSON as read (schema 18: rebuilt from the protobuf), or None.
    pub original_json: Option<Json>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnkiMediaEntry {
    /// Zip entry holding the body ("0", "1", …).
    pub zip_entry: String,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-1 of the body when the package records it (LATEST layout).
    pub sha1: Option<String>,
}

/// Reads one media body by normalised name (decompressed, SHA-1 verified when known).
/// Fails with `not-found` for unknown names.
pub type MediaReader = Box<dyn Fn(&str) -> Result<Vec<u8>> + Send + Sync>;

pub struct AnkiImage {
    pub layout: PackageLayout,
    /// Collection creation time, unix seconds (Anki `col.crt`).
    pub crt: Seconds,
    /// Collection modification time, ms (`col.mod`).
    pub mod_ms: i64,
    /// Anki schema version of the collection file (11 or 14–18).
    pub schema_version: i64,
    pub config: AnkiCollectionConfig,
    pub notetypes: Vec<Notetype>,
    /// Schema-11 JSON of each note type (fields/templates included) as read, for lossless re-export.
    pub notetype_originals: HashMap<NotetypeId, Json>,
    pub decks: Vec<AnkiDeck>,
    pub deck_configs: Vec<AnkiDeckConfig>,
    pub notes: Vec<Note>,
    pub cards: Vec<Card>,
    pub revlog: Vec<RevlogEntry>,
    /// Normalised media filename → where its body is.
    pub media: BTreeMap<String, AnkiMediaEntry>,
    pub media_reader: MediaReader,
    /// Non-fatal problems found while reading (human sentences).
    pub warnings: Vec<String>,
}

impl AnkiImage {
    /// A reader for images without media bodies (hand-built images, tests).
    pub fn no_media_reader() -> MediaReader {
        Box::new(|name: &str| Err(crate::error::LociError::not_found(format!("No media file named \"{name}\" in the package."))))
    }

    /// A reader over an in-memory map of bodies.
    pub fn map_media_reader(bodies: BTreeMap<String, Vec<u8>>) -> MediaReader {
        Box::new(move |name: &str| {
            bodies
                .get(name)
                .cloned()
                .ok_or_else(|| crate::error::LociError::not_found(format!("No media file named \"{name}\" in the package.")))
        })
    }

    pub fn read_media(&self, name: &str) -> Result<Vec<u8>> {
        (self.media_reader)(name)
    }
}

impl std::fmt::Debug for AnkiImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnkiImage")
            .field("layout", &self.layout)
            .field("crt", &self.crt)
            .field("schema_version", &self.schema_version)
            .field("notetypes", &self.notetypes.len())
            .field("decks", &self.decks.len())
            .field("deck_configs", &self.deck_configs.len())
            .field("notes", &self.notes.len())
            .field("cards", &self.cards.len())
            .field("revlog", &self.revlog.len())
            .field("media", &self.media.len())
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

/// Anki's DeckToday counters for a deck that was never studied.
pub const EMPTY_DECK_TODAY: DeckToday = DeckToday { day: 0, new: 0, rev: 0, lrn: 0, time_ms: 0 };
