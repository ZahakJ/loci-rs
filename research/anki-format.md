# Anki collection and package formats

Sources read on 2026-10-06. Source of truth is the Anki Rust core (`rslib`) at commit
`a13d8a6e9e91d87eb046bf7b26ee64880a4eb544` (main, Oct 2026, ≈ Anki 26.09.x). All `rslib`/`proto` links below are
permalinks to that commit, so line content quoted here can be re-checked. Where Anki's own code and community docs
disagree, the code wins and the disagreement is noted.

Notation: `[R:path]` = `https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/path`.
(Every `[R:...]` tag below is a full citation under that rule; the URL list at the end repeats the most-used ones verbatim.)

---

## 0. Summary

1. **Three package layouts**, chosen by the `meta` zip entry (protobuf `PackageMetadata.version`), or by which collection
   file exists when `meta` is absent:
   | version | collection file | DB schema | collection compression | `media` map | media file bodies |
   |---|---|---|---|---|---|
   | 1 `LEGACY_1` (no `meta`, `collection.anki2`) | `collection.anki2` | 11 | none (zip deflate) | JSON `{"0":"a.jpg",...}` | raw |
   | 2 `LEGACY_2` (`meta`=`08 02` or absent + `collection.anki21`) | `collection.anki21` | 11 | none (zip deflate) | JSON | raw |
   | 3 `LATEST` (`meta`=`08 03`) | `collection.anki21b` | **18** | **zstd** (zip "stored") | **zstd(protobuf `MediaEntries`)** | **zstd each** |
   Every package Anki writes (legacy or latest) *also* contains a dummy `collection.anki2` (one note "please update
   Anki") so very old clients don't crash. [R:rslib/src/import_export/package/meta.rs], [R:rslib/src/import_export/package/colpkg/export.rs]
2. **Reading**: unzip → decode `meta` → pick collection file → zstd-decode if v3 → open in sql.js → if schema 18,
   strip `COLLATE unicase` from `sqlite_master` first (sql.js can't register Anki's custom collation; see §2.4).
3. **Writing for maximum compatibility** (Anki 2.1.0 … 26.x, AnkiDroid): write `collection.anki21` (schema 11) +
   JSON `media` + numbered media files, optionally `meta = 08 02`. **Put `"schedVer": 2` in `col.conf`** or Anki will
   treat the file as v1-scheduler data and *rewrite revlog eases* on import (§8.3).
4. Notes match on **`guid`** across collections; review-card `due` is **days since the collection's own `crt`
   day**, so it is shifted by `(srcDaysElapsed − dstDaysElapsed)` on import (§8.4).

---

## 1. Zip layout (.apkg and .colpkg)

`.apkg` (deck/selection export) and `.colpkg` (whole-collection backup) share the same container; the difference is
semantic (colpkg *replaces* the user's collection; apkg is *merged*). Source: `export_collection()`
[R:rslib/src/import_export/package/colpkg/export.rs]; apkg export calls the same function
[R:rslib/src/import_export/package/apkg/export.rs].

Entry order and options as written by Anki ≥ 2.1.50:

```
meta                    stored  — protobuf PackageMetadata (always written by modern Anki, both legacy & latest)
collection.anki21b      stored  — zstd(SQLite schema 18)            [latest]
  or collection.anki21  deflate — raw SQLite schema 11              [legacy export, "support older Anki"]
collection.anki2        stored  — dummy schema-11 collection, page_size 512, one Basic note "too new" text
0, 1, 2, ...            stored  — media file bodies; latest: each zstd-compressed; legacy: raw bytes
media                   stored  — media map; latest: zstd(protobuf MediaEntries); legacy: JSON object
```

Code (verbatim excerpts):

```rust
// colpkg/export.rs
zip.start_file("meta", file_options_stored())?;
meta.encode(&mut meta_bytes)?;  zip.write_all(&meta_bytes)?;
write_collection(&meta, &mut zip, col, col_size)?;   // zstd + stored if latest, else deflate
write_dummy_collection(&mut zip, tr)?;               // always "collection.anki2"
write_media(&meta, &mut zip, media, progress)?;      // files "0","1",... then "media"

fn write_media_map(...) {
    zip.start_file("media", file_options_stored())?;
    let encoded_bytes = if meta.media_list_is_hashmap() {           // legacy
        let map: HashMap<String, &str> = media_entries.iter().enumerate()
            .map(|(k, entry)| (k.to_string(), entry.name.as_str())).collect();
        serde_json::to_vec(&map)?
    } else { MediaEntries { entries: media_entries }.encode(&mut buf)?; buf };
    if meta.zstd_compressed() { zstd_copy(&mut cursor, zip, size)?; } else { io::copy(&mut cursor, zip)?; }
}
```

Media naming inside the zip: the zip entry name is the **decimal index** (`index.to_string()`), the real filename is
in the map. Legacy maps may have gaps (hence `legacy_zip_filename` in the proto). For latest packages the *index in
the `entries` array* is the zip entry name. [R:rslib/src/import_export/package/media.rs]

Zstd details: Anki uses `zstd::Encoder::new(writer, 0)` (level 0 = library default, 3) for the collection and the
media map, and a reused raw encoder per media file; all are ordinary zstd frames, decoded with streaming
`copy_decode`, so frames **without** a content-size header are fine. `MediaEntry.size` and `.sha1` describe the
**uncompressed** bytes (`MediaCopier::copy` hashes before encoding). [R:rslib/src/import_export/package/media.rs]

> Browser tip: you do not need a zstd *encoder* to write v3. A zstd frame made only of "raw" (uncompressed) blocks is
> valid and Anki's decoder accepts any valid frame; the ankipack TS library does exactly this
> (https://github.com/ImGajeed76/ankipack/blob/main/src/util/zstd.ts). For decoding, `fzstd` (pure JS) is used by
> ankipack and anki-apkg-parser; srs-converter uses `@hpcc-js/wasm-zstd`.

Missing `media` entry: tolerated by modern Anki (treated as `{}`; "Older AnkiDroid versions wrote colpkg files without
a media map"). **Anki 2.1.x ≤ 2.1.49's Python importer did `z.read("media")` unconditionally** — always write it.
[R:rslib/src/import_export/package/media.rs], https://github.com/ankitects/anki/blob/2.1.49/pylib/anki/importing/apkg.py

### 1.1 `meta` and `media` protobufs (verbatim, `proto/anki/import_export.proto`, `package anki.import_export; proto3`)

```proto
message PackageMetadata {
  enum Version {
    VERSION_UNKNOWN = 0;
    // When `meta` missing, and collection.anki2 file present.
    VERSION_LEGACY_1 = 1;
    // When `meta` missing, and collection.anki21 file present.
    VERSION_LEGACY_2 = 2;
    // Implies MediaEntry media map, and zstd compression.
    // collection.21b file
    VERSION_LATEST = 3;
  }

  Version version = 1;
}

message MediaEntries {
  message MediaEntry {
    string name = 1;
    uint32 size = 2;
    bytes sha1 = 3;

    /// Legacy media maps may include gaps in the media list, so the original
    /// file index is recorded when importing from a HashMap. This field is not
    /// set when exporting.
    optional uint32 legacy_zip_filename = 255;
  }

  repeated MediaEntry entries = 1;
}
```
[R:proto/anki/import_export.proto]

Wire bytes: `meta` for latest = `08 03`; legacy-2 = `08 02`. Version-detection logic (verbatim from `meta.rs`):

```rust
let meta = if let Some(bytes) = meta_bytes {
    let meta: Meta = Message::decode(&*bytes)?;
    if meta.version() == Version::Unknown { return Err(ImportError::TooNew) }   // includes unknown future enum values
    meta
} else {
    Meta { version: if archive.by_name("collection.anki21").is_ok() { Version::Legacy2 } else { Version::Legacy1 } as i32 }
};
// Legacy1 -> "collection.anki2", Legacy2 -> "collection.anki21", Latest -> "collection.anki21b"
// Legacy1|Legacy2 -> SchemaVersion::V11 ; Latest -> SchemaVersion::V18
// zstd_compressed() = !is_legacy();  media_list_is_hashmap() = is_legacy()
```
[R:rslib/src/import_export/package/meta.rs]

Important reader rule: if `meta` decodes to a version you don't know, **do not** fall back to `collection.anki2` —
that's the dummy "please update" collection.

### 1.2 Media filename safety & normalization (applied on import)

* `filename_is_safe`: path must be exactly one normal component (no `/`, `..`, absolute paths).
  [R:rslib/io/src/lib.rs]
* `normalize_filename`: NFC-normalize; remove `[ ] < > : " / ? * ^ \ |`, ASCII control chars and unassigned code
  points; NBSP→space; Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`, followed by `.` or end)
  get `_` appended to the stem; trailing space/period gets `_` appended; truncate to **120 bytes** keeping extension.
  Legacy map names are normalized silently; a *latest* `MediaEntry` name that isn't already normalized makes the
  whole package `Corrupt`. [R:rslib/src/media/files.rs], [R:rslib/src/import_export/package/media.rs]

---

## 2. SQL schemas

### 2.1 Schema 11 (legacy; `collection.anki2` / `.anki21`; still what Anki creates first)

Verbatim `rslib/src/storage/schema11.sql` [R:rslib/src/storage/schema11.sql]:

```sql
CREATE TABLE col (
  id integer PRIMARY KEY,
  crt integer NOT NULL,
  mod integer NOT NULL,
  scm integer NOT NULL,
  ver integer NOT NULL,
  dty integer NOT NULL,
  usn integer NOT NULL,
  ls integer NOT NULL,
  conf text NOT NULL,
  models text NOT NULL,
  decks text NOT NULL,
  dconf text NOT NULL,
  tags text NOT NULL
);
CREATE TABLE notes (
  id integer PRIMARY KEY,
  guid text NOT NULL,
  mid integer NOT NULL,
  mod integer NOT NULL,
  usn integer NOT NULL,
  tags text NOT NULL,
  flds text NOT NULL,
  -- The use of type integer for sfld is deliberate, because it means that integer values in this
  -- field will sort numerically.
  sfld integer NOT NULL,
  csum integer NOT NULL,
  flags integer NOT NULL,
  data text NOT NULL
);
CREATE TABLE cards (
  id integer PRIMARY KEY,
  nid integer NOT NULL,
  did integer NOT NULL,
  ord integer NOT NULL,
  mod integer NOT NULL,
  usn integer NOT NULL,
  type integer NOT NULL,
  queue integer NOT NULL,
  due integer NOT NULL,
  ivl integer NOT NULL,
  factor integer NOT NULL,
  reps integer NOT NULL,
  lapses integer NOT NULL,
  left integer NOT NULL,
  odue integer NOT NULL,
  odid integer NOT NULL,
  flags integer NOT NULL,
  data text NOT NULL
);
CREATE TABLE revlog (
  id integer PRIMARY KEY,
  cid integer NOT NULL,
  usn integer NOT NULL,
  ease integer NOT NULL,
  ivl integer NOT NULL,
  lastIvl integer NOT NULL,
  factor integer NOT NULL,
  time integer NOT NULL,
  type integer NOT NULL
);
CREATE TABLE graves (
  usn integer NOT NULL,
  oid integer NOT NULL,
  type integer NOT NULL
);
-- syncing
CREATE INDEX ix_notes_usn ON notes (usn);
CREATE INDEX ix_cards_usn ON cards (usn);
CREATE INDEX ix_revlog_usn ON revlog (usn);
-- card spacing, etc
CREATE INDEX ix_cards_nid ON cards (nid);
-- scheduling and deck limiting
CREATE INDEX ix_cards_sched ON cards (did, queue, due);
-- revlog by card
CREATE INDEX ix_revlog_cid ON revlog (cid);
-- field uniqueness
CREATE INDEX ix_notes_csum ON notes (csum);
INSERT INTO col
VALUES (
    1,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    '{}',
    '{}',
    '{}',
    '{}',
    '{}'
  );
```

`col` row semantics (schema 11): `id`=1 (single row); `crt` = creation time, **seconds**, Anki sets it to the most
recent local 4am (`v1_creation_date()`); `mod` = last modified **ms**; `scm` = schema-modified **ms** (differs →
full sync); `ver` = 11; `dty` = 0 (unused); `usn` = 0 for local; `ls` = last sync ms; `conf/models/decks/dconf/tags` =
JSON text (§5, §6). [R:rslib/src/storage/sqlite.rs], [R:rslib/src/scheduler/timing.rs],
https://github.com/ankidroid/Anki-Android/wiki/Database-Structure

`graves.type`: 0 card, 1 note, 2 deck (AnkiDroid wiki). Not needed in packages.

### 2.2 Schema 18 (current; `collection.anki21b`)

A schema-18 DB is the schema-11 DDL above **plus** the following upgrade scripts applied in order 14→15→17→18
(16 is data-only). Verbatim [R:rslib/src/storage/upgrades/schema14_upgrade.sql],
[R:rslib/src/storage/upgrades/schema15_upgrade.sql], [R:rslib/src/storage/upgrades/schema17_upgrade.sql],
[R:rslib/src/storage/upgrades/schema18_upgrade.sql]:

```sql
-- schema14_upgrade.sql
CREATE TABLE deck_config (
  id integer PRIMARY KEY NOT NULL,
  name text NOT NULL COLLATE unicase,
  mtime_secs integer NOT NULL,
  usn integer NOT NULL,
  config blob NOT NULL
);
CREATE TABLE config (
  KEY text NOT NULL PRIMARY KEY,
  usn integer NOT NULL,
  mtime_secs integer NOT NULL,
  val blob NOT NULL
) without rowid;
CREATE TABLE tags (
  tag text NOT NULL PRIMARY KEY COLLATE unicase,
  usn integer NOT NULL
) without rowid;
UPDATE col
SET ver = 14;

-- schema15_upgrade.sql
CREATE TABLE fields (
  ntid integer NOT NULL,
  ord integer NOT NULL,
  name text NOT NULL COLLATE unicase,
  config blob NOT NULL,
  PRIMARY KEY (ntid, ord)
) without rowid;
CREATE UNIQUE INDEX idx_fields_name_ntid ON fields (name, ntid);
CREATE TABLE templates (
  ntid integer NOT NULL,
  ord integer NOT NULL,
  name text NOT NULL COLLATE unicase,
  mtime_secs integer NOT NULL,
  usn integer NOT NULL,
  config blob NOT NULL,
  PRIMARY KEY (ntid, ord)
) without rowid;
CREATE UNIQUE INDEX idx_templates_name_ntid ON templates (name, ntid);
CREATE INDEX idx_templates_usn ON templates (usn);
CREATE TABLE notetypes (
  id integer NOT NULL PRIMARY KEY,
  name text NOT NULL COLLATE unicase,
  mtime_secs integer NOT NULL,
  usn integer NOT NULL,
  config blob NOT NULL
);
CREATE UNIQUE INDEX idx_notetypes_name ON notetypes (name);
CREATE INDEX idx_notetypes_usn ON notetypes (usn);
CREATE TABLE decks (
  id integer PRIMARY KEY NOT NULL,
  name text NOT NULL COLLATE unicase,
  mtime_secs integer NOT NULL,
  usn integer NOT NULL,
  common blob NOT NULL,
  kind blob NOT NULL
);
CREATE UNIQUE INDEX idx_decks_name ON decks (name);
CREATE INDEX idx_notes_mid ON notes (mid);
CREATE INDEX idx_cards_odid ON cards (odid)
WHERE odid != 0;
UPDATE col
SET ver = 15;
ANALYZE;

-- schema17_upgrade.sql
DROP TABLE tags;
CREATE TABLE tags (
  tag text NOT NULL PRIMARY KEY COLLATE unicase,
  usn integer NOT NULL,
  collapsed boolean NOT NULL,
  config blob NULL
) without rowid;

-- schema18_upgrade.sql
ALTER TABLE graves
  RENAME TO graves_old;
CREATE TABLE graves (
  oid integer NOT NULL,
  type integer NOT NULL,
  usn integer NOT NULL,
  PRIMARY KEY (oid, type)
) WITHOUT ROWID;
INSERT
  OR IGNORE INTO graves (oid, type, usn)
SELECT oid,
  type,
  usn
FROM graves_old;
DROP TABLE graves_old;
CREATE INDEX idx_graves_pending ON graves (usn);
UPDATE col
SET ver = 18;
```

After upgrade the JSON columns in `col` are emptied: `update col set conf=''`, `models = ''`, `decks = ''`,
`dconf=''`, `tags=''` (empty string, not `{}`). [R:rslib/src/storage/config/mod.rs],
[R:rslib/src/storage/notetype/mod.rs], [R:rslib/src/storage/deck/mod.rs], [R:rslib/src/storage/deckconfig/mod.rs],
[R:rslib/src/storage/tag/mod.rs]

Blob encodings in schema 18 (from the `row_to_*` / `encode` calls):

| table.column | encoding |
|---|---|
| `notetypes.config` | protobuf `anki.notetypes.Notetype.Config` |
| `fields.config` | protobuf `Notetype.Field.Config` (ord & name are columns) |
| `templates.config` | protobuf `Notetype.Template.Config` (ord, name, mtime_secs, usn are columns) |
| `decks.common` | protobuf `anki.decks.Deck.Common` |
| `decks.kind` | protobuf `anki.decks.Deck.KindContainer` (oneof normal=1 / filtered=2) |
| `deck_config.config` | protobuf `anki.deck_config.DeckConfig.Config` |
| `config.val` | **JSON** bytes (e.g. `2`, `true`, `"noteFld"`, `[1]`) stored as blob |
| `tags.config` | unused (NULL); `collapsed` boolean column |
| `decks.name` | **native name: components joined by `\x1f`**, not `::` |

[R:rslib/src/storage/deck/mod.rs] (`DeckCommon::decode(col 4)`, `DeckKindContainer::decode(col 5)`),
[R:rslib/src/storage/notetype/mod.rs], [R:rslib/src/storage/deckconfig/mod.rs], [R:rslib/src/storage/config/mod.rs],
[R:rslib/src/decks/name.rs] (`split("::")…join("\x1f")`).

Every proto message also has `bytes other = 255;` = JSON-serialized map of unknown legacy keys, preserved for
round-trip to schema 11 (`other_to_bytes`/`parse_other_fields`). [R:rslib/src/notetype/schema11.rs]

### 2.3 Proto definitions used for DB blobs (verbatim)

`proto/anki/notetypes.proto` (`package anki.notetypes;`) [R:proto/anki/notetypes.proto]:

```proto
message Notetype {
  message Config {
    enum Kind {
      KIND_NORMAL = 0;
      KIND_CLOZE = 1;
    }
    message CardRequirement {
      enum Kind {
        KIND_NONE = 0;
        KIND_ANY = 1;
        KIND_ALL = 2;
      }
      uint32 card_ord = 1;
      Kind kind = 2;
      repeated uint32 field_ords = 3;
    }

    Kind kind = 1;
    uint32 sort_field_idx = 2;
    string css = 3;
    // This is now stored separately; retrieve with DefaultsForAdding()
    int64 target_deck_id_unused = 4;
    string latex_pre = 5;
    string latex_post = 6;
    bool latex_svg = 7;
    repeated CardRequirement reqs = 8;
    // Only set on notetypes created with Anki 2.1.62+.
    StockNotetype.OriginalStockKind original_stock_kind = 9;
    // the id in the source collection for imported notetypes (Anki 23.10)
    optional int64 original_id = 10;

    bytes other = 255;
  }
  message Field {
    message Config {
      bool sticky = 1;
      bool rtl = 2;
      string font_name = 3;
      uint32 font_size = 4;
      string description = 5;
      bool plain_text = 6;
      bool collapsed = 7;
      bool exclude_from_search = 8;
      // used for merging notetypes on import (Anki 23.10)
      optional int64 id = 9;
      // Can be used to uniquely identify required fields.
      optional uint32 tag = 10;
      bool prevent_deletion = 11;

      bytes other = 255;
    }
    generic.UInt32 ord = 1;
    string name = 2;
    Config config = 5;
  }
  message Template {
    message Config {
      string q_format = 1;
      string a_format = 2;
      string q_format_browser = 3;
      string a_format_browser = 4;
      int64 target_deck_id = 5;
      string browser_font_name = 6;
      uint32 browser_font_size = 7;
      // used for merging notetypes on import (Anki 23.10)
      optional int64 id = 8;

      bytes other = 255;
    }

    generic.UInt32 ord = 1;
    string name = 2;
    int64 mtime_secs = 3;
    sint32 usn = 4;
    Config config = 5;
  }

  int64 id = 1;
  string name = 2;
  int64 mtime_secs = 3;
  sint32 usn = 4;
  Config config = 7;
  repeated Field fields = 8;
  repeated Template templates = 9;
}

message StockNotetype {
  enum Kind {
    KIND_BASIC = 0;
    KIND_BASIC_AND_REVERSED = 1;
    KIND_BASIC_OPTIONAL_REVERSED = 2;
    KIND_BASIC_TYPING = 3;
    KIND_CLOZE = 4;
    KIND_IMAGE_OCCLUSION = 5;
  }
  enum OriginalStockKind {
    ORIGINAL_STOCK_KIND_UNKNOWN = 0;
    ORIGINAL_STOCK_KIND_BASIC = 1;
    ORIGINAL_STOCK_KIND_BASIC_AND_REVERSED = 2;
    ORIGINAL_STOCK_KIND_BASIC_OPTIONAL_REVERSED = 3;
    ORIGINAL_STOCK_KIND_BASIC_TYPING = 4;
    ORIGINAL_STOCK_KIND_CLOZE = 5;
    ORIGINAL_STOCK_KIND_IMAGE_OCCLUSION = 6;
  }
  Kind kind = 1;
  // ...
}

// proto/anki/generic.proto
message UInt32 {
  uint32 val = 1;
}
```

`proto/anki/decks.proto` (`package anki.decks;`) [R:proto/anki/decks.proto]:

```proto
message Deck {
  message Common {
    bool study_collapsed = 1;
    bool browser_collapsed = 2;

    uint32 last_day_studied = 3;
    int32 new_studied = 4;
    int32 review_studied = 5;
    int32 milliseconds_studied = 7;

    // previously set in the v1 scheduler,
    // but not currently used for anything
    int32 learning_studied = 6;

    reserved 8 to 13;

    bytes other = 255;
  }
  message Normal {
    message DayLimit {
      uint32 limit = 1;
      uint32 today = 2;
    }
    int64 config_id = 1;
    uint32 extend_new = 2;
    uint32 extend_review = 3;
    string description = 4;
    bool markdown_description = 5;
    optional uint32 review_limit = 6;
    optional uint32 new_limit = 7;
    DayLimit review_limit_today = 8;
    DayLimit new_limit_today = 9;
    // Deck-specific desired retention override
    optional float desired_retention = 10;

    reserved 12 to 15;
  }
  message Filtered {
    message SearchTerm {
      enum Order {
        OLDEST_REVIEWED_FIRST = 0;
        RANDOM = 1;
        INTERVALS_ASCENDING = 2;
        INTERVALS_DESCENDING = 3;
        LAPSES = 4;
        ADDED = 5;
        DUE = 6;
        REVERSE_ADDED = 7;
        RETRIEVABILITY_ASCENDING = 8;
        RETRIEVABILITY_DESCENDING = 9;
        RELATIVE_OVERDUENESS = 10;
      }

      string search = 1;
      uint32 limit = 2;
      Order order = 3;
    }

    bool reschedule = 1;
    repeated SearchTerm search_terms = 2;
    // v1 scheduler only
    repeated float delays = 3;
    // v2 and old v3 scheduler only
    uint32 preview_delay = 4;
    // recent v3 scheduler only; 0 means card will be returned
    uint32 preview_again_secs = 7;
    // recent v3 scheduler only; 0 means card will be returned
    uint32 preview_hard_secs = 5;
    // recent v3 scheduler only; 0 means card will be returned
    uint32 preview_good_secs = 6;
  }
  // a container to store the deck specifics in the DB
  // as a tagged enum
  message KindContainer {
    oneof kind {
      Normal normal = 1;
      Filtered filtered = 2;
    }
  }

  int64 id = 1;
  string name = 2;
  int64 mtime_secs = 3;
  int32 usn = 4;
  Common common = 5;
  // the specifics are inlined here when sending data to clients,
  // as otherwise an extra level of indirection would be required
  oneof kind {
    Normal normal = 6;
    Filtered filtered = 7;
  }
}
```

`proto/anki/deck_config.proto` (`package anki.deck_config;`) [R:proto/anki/deck_config.proto]:

```proto
message DeckConfig {
  message Config {
    enum NewCardInsertOrder {
      NEW_CARD_INSERT_ORDER_DUE = 0;
      NEW_CARD_INSERT_ORDER_RANDOM = 1;
    }
    enum NewCardGatherPriority {
      NEW_CARD_GATHER_PRIORITY_DECK = 0;
      NEW_CARD_GATHER_PRIORITY_DECK_THEN_RANDOM_NOTES = 5;
      NEW_CARD_GATHER_PRIORITY_LOWEST_POSITION = 1;
      NEW_CARD_GATHER_PRIORITY_HIGHEST_POSITION = 2;
      NEW_CARD_GATHER_PRIORITY_RANDOM_NOTES = 3;
      NEW_CARD_GATHER_PRIORITY_RANDOM_CARDS = 4;
    }
    enum NewCardSortOrder {
      NEW_CARD_SORT_ORDER_TEMPLATE = 0;
      NEW_CARD_SORT_ORDER_NO_SORT = 1;
      NEW_CARD_SORT_ORDER_TEMPLATE_THEN_RANDOM = 2;
      NEW_CARD_SORT_ORDER_RANDOM_NOTE_THEN_TEMPLATE = 3;
      NEW_CARD_SORT_ORDER_RANDOM_CARD = 4;
    }
    enum ReviewCardOrder {
      REVIEW_CARD_ORDER_DAY = 0;
      REVIEW_CARD_ORDER_DAY_THEN_DECK = 1;
      REVIEW_CARD_ORDER_DECK_THEN_DAY = 2;
      REVIEW_CARD_ORDER_INTERVALS_ASCENDING = 3;
      REVIEW_CARD_ORDER_INTERVALS_DESCENDING = 4;
      REVIEW_CARD_ORDER_EASE_ASCENDING = 5;
      REVIEW_CARD_ORDER_EASE_DESCENDING = 6;
      REVIEW_CARD_ORDER_RETRIEVABILITY_ASCENDING = 7;
      REVIEW_CARD_ORDER_RETRIEVABILITY_DESCENDING = 11;
      REVIEW_CARD_ORDER_RELATIVE_OVERDUENESS = 12;
      REVIEW_CARD_ORDER_RANDOM = 8;
      REVIEW_CARD_ORDER_ADDED = 9;
      REVIEW_CARD_ORDER_REVERSE_ADDED = 10;
    }
    enum ReviewMix {
      REVIEW_MIX_MIX_WITH_REVIEWS = 0;
      REVIEW_MIX_AFTER_REVIEWS = 1;
      REVIEW_MIX_BEFORE_REVIEWS = 2;
    }
    enum LeechAction {
      LEECH_ACTION_SUSPEND = 0;
      LEECH_ACTION_TAG_ONLY = 1;
    }
    enum AnswerAction {
      ANSWER_ACTION_BURY_CARD = 0;
      ANSWER_ACTION_ANSWER_AGAIN = 1;
      ANSWER_ACTION_ANSWER_GOOD = 2;
      ANSWER_ACTION_ANSWER_HARD = 3;
      ANSWER_ACTION_SHOW_REMINDER = 4;
    }
    enum QuestionAction {
      QUESTION_ACTION_SHOW_ANSWER = 0;
      QUESTION_ACTION_SHOW_REMINDER = 1;
    }
    repeated float learn_steps = 1;
    repeated float relearn_steps = 2;

    repeated float fsrs_params_4 = 3;
    repeated float fsrs_params_5 = 5;
    repeated float fsrs_params_6 = 6;

    // consider saving remaining ones for fsrs param changes
    reserved 7 to 8;

    uint32 new_per_day = 9;
    uint32 reviews_per_day = 10;

    // not currently used
    uint32 new_per_day_minimum = 35;

    float initial_ease = 11;
    float easy_multiplier = 12;
    float hard_multiplier = 13;
    float lapse_multiplier = 14;
    float interval_multiplier = 15;

    uint32 maximum_review_interval = 16;
    uint32 minimum_lapse_interval = 17;

    uint32 graduating_interval_good = 18;
    uint32 graduating_interval_easy = 19;

    NewCardInsertOrder new_card_insert_order = 20;
    NewCardGatherPriority new_card_gather_priority = 34;
    NewCardSortOrder new_card_sort_order = 32;
    ReviewMix new_mix = 30;

    ReviewCardOrder review_order = 33;

    ReviewMix interday_learning_mix = 31;

    LeechAction leech_action = 21;
    uint32 leech_threshold = 22;

    bool disable_autoplay = 23;
    uint32 cap_answer_time_to_secs = 24;
    bool show_timer = 25;
    bool stop_timer_on_answer = 38;
    float seconds_to_show_question = 41;
    float seconds_to_show_answer = 42;
    QuestionAction question_action = 36;
    AnswerAction answer_action = 43;
    bool wait_for_audio = 44;
    bool skip_question_when_replaying_answer = 26;

    bool bury_new = 27;
    bool bury_reviews = 28;
    bool bury_interday_learning = 29;

    // for fsrs
    float desired_retention = 37;
    string ignore_revlogs_before_date = 46;
    repeated float easy_days_percentages = 4;
    // used for fsrs_reschedule in the past
    reserved 39;
    float historical_retention = 40;
    string param_search = 45;

    bytes other = 255;
  }

  int64 id = 1;
  string name = 2;
  int64 mtime_secs = 3;
  int32 usn = 4;
  Config config = 5;
}
```
(Comments on the `NewCardGatherPriority`/`NewCardSortOrder` values trimmed; values are verbatim.)

Proto3 note for writers: packed repeated floats (`learn_steps` etc.) are encoded packed by default; Anki (prost)
accepts both packed and unpacked. Use generated code (`@bufbuild/protobuf` + `protoc-gen-es` as ankipack does, or
`protobufjs`) from the `.proto` files rather than hand-rolling.

### 2.4 sql.js / browser gotchas when opening Anki DBs

* **`COLLATE unicase`**: schema-18 tables declare a custom collation registered in Rust as
  `UniCase::new(s1).cmp(&UniCase::new(s2))` [R:rslib/src/storage/sqlite.rs]. sql.js has no
  `create_collation`, and querying the `tags` table (WITHOUT ROWID keyed on a unicase column) fails with "no query
  solution". Both TS libraries that read anki21b patch the schema text before use:
  ```ts
  // ImGajeed76/ankipack src/collection/read.ts (abridged)
  const rows = db.exec("SELECT name, sql FROM sqlite_master WHERE sql LIKE '%collate unicase%'");
  db.run("PRAGMA writable_schema = ON");
  for (const [name, sql] of rows[0].values)
    db.run("UPDATE sqlite_master SET sql = ? WHERE name = ?", [String(sql).replace(/\s+collate\s+unicase/gi, ""), name]);
  db.run("PRAGMA writable_schema = OFF");
  return db.export();   // reopen from these bytes
  ```
  https://github.com/ImGajeed76/ankipack/blob/main/src/collection/read.ts,
  https://github.com/eikowagenknecht/srs-converter/blob/main/src/anki/database.ts .
  When *writing* schema 18 without the collation, enforce case-insensitive uniqueness of deck/notetype/field/
  template names yourself (Anki's unique indexes are unicase).
* **Journal mode**: Anki closes a V11 file with `journal_mode=delete`, but a V18 file is closed without that switch
  (`has_journal_mode_delete()` is true only for V11) after `wal_checkpoint(truncate)`, so `collection.anki21b` bytes may
  carry the WAL flag (header bytes 18/19 = 2). [R:rslib/src/storage/sqlite.rs], [R:rslib/src/storage/mod.rs].
  Unverified: whether sql.js opens such bytes on every build; defensive fix is `bytes[18] = bytes[19] = 1` before
  `new SQL.Database(bytes)`.
* Anki registers extra SQL functions (`field_at_index`, `regexp_fields`, `process_text`, `fnvhash`,
  `extract_custom_data`, `extract_fsrs_*` …) — only needed if you want to run Anki's own search SQL; plain table reads
  don't need them. [R:rslib/src/storage/sqlite.rs]
* `sfld` has INTEGER affinity: numeric-looking sort fields come back as JS numbers. `csum` is an integer.

---

## 3. Notes, cards, revlog — column semantics

### 3.1 `notes`

| col | meaning |
|---|---|
| `id` | ms timestamp of creation (unique; importer bumps by +999 on collision) |
| `guid` | globally unique id string, base91 of random u64 (§9.1); the cross-collection identity |
| `mid` | notetype id |
| `mod` | modified, **seconds** |
| `usn` | update sequence number; −1 = needs push to server; importer overwrites with target usn |
| `tags` | space-separated, **with leading and trailing space** (`" a b "`), or `""` if none. Separators on read: ASCII space or U+3000 |
| `flds` | fields joined by `\x1f` (U+001F); count must equal notetype field count or import errors |
| `sfld` | sort field: `strip_html_preserving_media_filenames(fields[sortf])` |
| `csum` | `u32` from first 4 bytes (big-endian) of SHA-1 of `strip_html_preserving_media_filenames(fields[0])` |
| `flags` | 0 (unused) |
| `data` | `""` (unused) |

Sources: `join_tags` → `format!(" {} ", tags.join(" "))` and `split_tags` on `' '|'\u{3000}'`
[R:rslib/src/tags/mod.rs]; `join_fields` → `fields.join("\x1f")` [R:rslib/src/storage/note/mod.rs];
`prepare_for_update`, `field_checksum` [R:rslib/src/notes/mod.rs].

Field normalization on save: control chars other than `\n`/`\t` are removed; if config `normalize_note_text`
(default **true**) the field is NFC-normalized. [R:rslib/src/notes/mod.rs], [R:rslib/src/config/bool.rs]

### 3.2 `cards`

```rust
// rslib/src/card/mod.rs
pub enum CardType { New = 0, Learn = 1, Review = 2, Relearn = 3 }
pub enum CardQueue {
    New = 0,            // due is the order cards are shown in
    Learn = 1,          // due is a unix timestamp
    Review = 2,         // due is days since creation date
    DayLearn = 3,
    PreviewRepeat = 4,  // due is a unix timestamp. preview cards only placed here when failed.
    Suspended = -1, SchedBuried = -2, UserBuried = -3,
}
```
[R:rslib/src/card/mod.rs]

| col | meaning / units |
|---|---|
| `id` | ms timestamp of creation |
| `nid`, `did`, `ord` | note id; deck id; template index (normal) or cloze number − 1 (cloze, capped at 499) |
| `mod` | seconds; `usn` as notes |
| `type` | 0 new, 1 learning, 2 review, 3 relearning |
| `queue` | −3 user-buried, −2 sched-buried (v1: "buried"), −1 suspended, 0 new, 1 intraday learn, 2 review, 3 interday (day) learn, 4 preview-repeat |
| `due` | queue 0: **position** (int, new-card order); queue 1/4: **unix seconds**; queue 2/3: **day number = days elapsed since collection creation** (§6.1). Suspended/buried keep the value appropriate to `type`. In a filtered deck: position starting at −100000 (only if original due > 0) |
| `ivl` | **days** (u32). 0 for learning; for relearning the review interval the card returns to. (Negative-seconds encoding exists only in `revlog`, contrary to some summaries.) |
| `factor` | ease in **permille** (2500 = 250%); 0 for new cards. Under FSRS the card column keeps SM-2 ease; FSRS state lives in `data` |
| `reps`, `lapses` | counts |
| `left` | legacy v1/v2: `a*1000 + b` (a = steps left today, b = steps left to graduation); v3 writes plain `b`. Read with `left % 1000` |
| `odue` | original due while in a filtered deck; else 0 |
| `odid` | original deck id while in a filtered deck; else 0 |
| `flags` | low 3 bits = user flag 0–7 (1 red, 2 orange, 3 green, 4 blue, 5 pink, 6 turquoise, 7 purple); other bits preserved |
| `data` | JSON object (see below); `""` or `"{}"` when empty |

`left` decoding: `remaining_steps() = self.remaining_steps % 1000` [R:rslib/src/card/mod.rs]; v3 learning writes
`self.card.remaining_steps = next.remaining_steps` and `due = timestamp` (queue 1) or `days_elapsed + days` (queue 3)
[R:rslib/src/scheduler/answering/learning.rs]; `a*1000+b` meaning from AnkiDroid wiki
https://github.com/ankidroid/Anki-Android/wiki/Database-Structure . Filtered position base `-100_000`
[R:rslib/src/scheduler/filtered/mod.rs]; `move_into_filtered_deck` [R:rslib/src/scheduler/filtered/card.rs].
Flags: `(self.flags & !0b111) | flag`, `flag < 8` [R:rslib/src/card/mod.rs].
`is_unix_epoch_timestamp(due) = due > 1_000_000_000` (used to tell timestamp vs day number for learn cards)
[R:rslib/src/scheduler/timing.rs].

**`cards.data` JSON** (all keys optional, written only when set) — `CardData` [R:rslib/src/storage/card/data.rs]:

| key | type | meaning |
|---|---|---|
| `pos` | u32 | original new-card position before leaving the new queue (restored on "forget") |
| `s` | f32 (4 dp) | FSRS stability, days |
| `d` | f32 (3 dp) | FSRS difficulty, 1.0–10.0 |
| `dr` | f32 (2 dp) | desired retention used when last scheduled |
| `decay` | f32 (3 dp) | FSRS decay (FSRS-6) |
| `lrt` | i64 | last review time, unix seconds |
| `cd` | **string** containing a JSON object | custom scheduling data; keys ≤ 8 bytes, whole string < 100 bytes |

Memory state exists only if both `s` and `d` are present. Invalid JSON → treated as empty.

### 3.3 `revlog`

`RevlogEntry` [R:rslib/src/revlog/mod.rs]:

| col | meaning |
|---|---|
| `id` | ms timestamp of the review (unique; insert-or-ignore on import) |
| `cid` | card id; `usn` |
| `ease` | button: 1 Again, 2 Hard, 3 Good, 4 Easy; **0 = manual reschedule/reset** (v1 learning used 1–3, 3 = easy) |
| `ivl` | new interval: **positive = days, negative = seconds** |
| `lastIvl` | previous interval, same encoding |
| `factor` | ease after answer, permille. With FSRS: difficulty normalized to 100–1100 (`(d−1)/9 + 0.1`, ×1000) |
| `time` | answer time, ms (capped by deck option `maxTaken`, default 60 s) |
| `type` | 0 Learning, 1 Review, 2 Relearning, 3 Filtered ("cram/early"), 4 Manual, 5 Rescheduled |

`is_reset()` = type 4 && factor 0; `is_cramming()` = type 3 && factor 0. [R:rslib/src/revlog/mod.rs]
AnkiDroid wiki agrees on units; its "learn/relearn: 1 wrong, 2 ok, 3 easy" applies to the **v1** scheduler only.

---
## 4. Note types (models)

### 4.1 Schema-11 JSON (`col.models`: object keyed by notetype id as a string)

Exact serde shape Anki reads/writes — `NotetypeSchema11` [R:rslib/src/notetype/schema11.rs]. "Required" = no serde
default, so the key must be present (value may be `null` where `default_on_invalid` is used); unknown keys are kept in
`other` and round-tripped.

```jsonc
{
  "1700000000000": {
    "id": 1700000000000,          // REQUIRED; number or numeric string
    "name": "Basic",              // REQUIRED
    "type": 0,                    // REQUIRED; 0 standard, 1 cloze
    "mod": 1700000000,            // REQUIRED; seconds
    "usn": -1,                    // REQUIRED
    "sortf": 0,                   // REQUIRED; sort field index
    "did": 1,                     // REQUIRED key (null allowed) — legacy "last deck"; stored as target_deck_id_unused
    "tmpls": [ /* see below */ ], // REQUIRED
    "flds":  [ /* see below */ ], // REQUIRED
    "css": ".card {...}",         // REQUIRED key (invalid -> "")
    "latexPre": "...",            // optional (default "")
    "latexPost": "...",           // optional (default "")
    "latexsvg": false,            // optional
    "req": [[0, "any", [0]]],     // optional; recomputed by Anki on import (see 4.4)
    "originalStockKind": 1,       // optional, Anki 2.1.62+ (enum OriginalStockKind)
    "originalId": 123,            // optional, Anki 23.10+ (id in the collection it was imported from)
    "tags": [], "vers": []        // legacy keys; not read by Rust but old Python clients/genanki write them
  }
}
```

Field object (`NoteFieldSchema11`): REQUIRED `name`, `ord` (int or null), `sticky` (bool, any truthy accepted),
`rtl`, `font` (string), `size` (int); optional `description`, `plainText`, `collapsed`, `excludeFromSearch`,
`id` (i64, random, 23.10+ — used to match fields when merging notetypes), `tag` (u32; IO uses it),
`preventDeletion`; legacy `media: []` is passed through.

Template object (`CardTemplateSchema11`): REQUIRED `name`, `ord`, `qfmt`; optional `afmt`, `bqfmt`, `bafmt`
(browser formats), `did` (deck override or null), `bfont`, `bsize`, `id` (i64, 23.10+).

Default values for new notetypes: field font `"Arial"`, size 20 [R:rslib/src/notetype/fields.rs]; CSS
[R:rslib/src/notetype/styling.css]:
```css
.card {
    font-family: arial;
    font-size: 20px;
    line-height: 1.5;
    text-align: center;
    color: black;
    background-color: white;
}
```
Cloze notetypes append [R:rslib/src/notetype/cloze_styling.css]
`.cloze { font-weight: bold; color: blue; } .nightMode .cloze { color: lightblue; }`.
LaTeX defaults [R:rslib/src/notetype/header.tex], footer `\end{document}` [R:rslib/src/notetype/mod.rs]:
```tex
\documentclass[12pt]{article}
\special{papersize=3in,5in}
\usepackage[utf8]{inputenc}
\usepackage{amssymb,amsmath}
\pagestyle{empty}
\setlength{\parindent}{0in}
\begin{document}
```
Stock Basic: fields `Front`,`Back`; template `Card 1`: qfmt `{{Front}}`, afmt
`{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}`. Stock Cloze: fields `Text`,`Back Extra`; qfmt `{{cloze:Text}}`,
afmt `{{cloze:Text}}<br>\n{{Back Extra}}`. [R:rslib/src/notetype/stock.rs]

### 4.2 Schema-18 mapping

`notetypes(id, name, mtime_secs, usn, config=Notetype.Config)`; `fields(ntid, ord, name, config=Field.Config)`;
`templates(ntid, ord, name, mtime_secs, usn, config=Template.Config)`. JSON→proto mapping (`From<NotetypeSchema11>`):
`type→kind`, `sortf→sort_field_idx`, `did→target_deck_id_unused`, `latexPre/Post/svg`, `req→reqs`,
`font→font_name`, `size→font_size`, `qfmt→q_format`, `afmt→a_format`, `bqfmt/bafmt→*_browser`,
`did (tmpl)→target_deck_id`, `bfont/bsize→browser_font_name/size`, leftover keys → `other` (JSON bytes).
[R:rslib/src/notetype/schema11.rs]

### 4.3 Special field names & reserved names

`SPECIAL_FIELDS = FrontSide, Card, CardFlag, Deck, Subdeck, Tags, Type, CardID` [R:rslib/src/notetype/mod.rs].
At render time (only if the note has no real field of that name): `Tags` = tags joined by space; `Type` = notetype
name; `Deck` = full deck name with `::` (original deck if in a filtered deck); `Subdeck` = last `::` component;
`CardFlag` = `"flag0"`…`"flag7"`; `Card` = template name; `CardID` = card id; and `c{ord+1}` = `"1"` (so
`{{#c2}}…{{/c2}}` works on cloze card 2). [R:rslib/src/notetype/render.rs]

### 4.4 Which cards exist ("req" and empty-field logic)

Modern Anki does **not** use the stored `req` to decide card generation; it re-parses the **question** template and
asks "does it render anything with these non-empty fields?" [R:rslib/src/notetype/cardgen.rs]:

* **Normal notetypes**: for each template ord not already present, generate a card iff
  `template_is_empty(nonempty_fields, qfmt_nodes, check_negated=true)` is false. `nonempty_fields` = note fields
  whose value is not "empty" (`field_is_empty`: only whitespace and/or `<br>`, `<br/>`, `<div>`, `</div>` tags;
  regex `^(?:[[:space:]]|</?(?:br|div)\ ?/?>)*$`, case-insensitive) **plus** every special field name not shadowed
  by a real field, except `FrontSide`, and `Tags` only if the note has tags.
  [R:rslib/src/template.rs], [R:rslib/src/notetype/cardgen.rs]
* `template_is_empty` (verbatim semantics): text/comments ignored; a `{{Field}}` replacement whose key is non-empty
  ⇒ not empty; `{{#F}}…{{/F}}` contributes only if F non-empty and children not empty; `{{^F}}…{{/F}}` contributes
  only if F is empty (when `check_negated`) and children not empty. Note the key of `{{text:F}}` is `F` (filters
  ignored), and `{{FrontSide}}` on the question side has key `FrontSide` which is never in the set.
* If a brand-new note would get zero cards, card ord 0 is generated anyway (`ensure_not_empty`) — normal *and* cloze.
* **Cloze notetypes**: one card per distinct cloze number N>0 found in **any** field (nested clozes included):
  `ord = min(N − 1, 499)`. `c0` never creates a card. [R:rslib/src/cloze.rs], [R:rslib/src/notetype/cardgen.rs]
* New cards get `due` = the note's existing new position if a sibling is new, else `nextPos` (then incremented);
  deck = template `did` override, else the target deck.

`req` (still computed and stored for old clients) — `ParsedTemplate::requirements()` with
`renders_with_fields_for_reqs` (negated conditionals always resolve to children): first, `Any` = set of single
fields that alone make the template render; if none, `All` = fields that cannot be removed; else `None`.
JSON shape: `[card_ord, "any"|"all"|"none", [field_ords...]]`. On import Anki calls `prepare_for_update`, which
recomputes `reqs`, so a writer can emit an approximate or empty `req` safely for modern Anki. Older Python-era clients
(before card generation moved to Rust) consulted `req` directly — emit it correctly if you target very old versions
(exact cut-over version not verified). [R:rslib/src/template.rs], [R:rslib/src/notetype/mod.rs],
[R:rslib/src/import_export/package/apkg/import/notes.rs]

---

## 5. The template language

Implementation: tokenizer/parser/renderer [R:rslib/src/template.rs], filters [R:rslib/src/template_filters.rs],
cloze [R:rslib/src/cloze.rs].

### 5.1 Lexing & parsing

* Tokens: text, `<!-- comment -->` (kept verbatim in output), and handlebars `{{ … }}` (non-greedy to first `}}`).
  If the template (after leading whitespace) starts with `{{=<% %>=}}`, the delimiters become `<% … %>` (legacy).
* Inside a handlebar: leading `{` chars are stripped, then trimmed. If length < 2 → plain replacement. Prefix `#` →
  open conditional, `/` → close, `^` → open negated conditional (name trimmed). Otherwise a replacement.
* Replacement `a:b:Field` is split with `rsplit(':')`: **key = last segment**, filters = remaining segments in
  right-to-left order, i.e. **applied innermost-first** (`{{text:cloze:Text}}` → cloze then text).
* Conditionals must close with the same name; mismatches are errors ("Found {{/X}}, but expected {{/Y}}").
* Unknown field in a replacement → error "Found '{{X}}', but there is no field called 'X'". Unknown conditional key →
  error, **except** `c<digits>` which is always allowed (cloze conditionals).

### 5.2 Rendering

* `{{Field}}` → field HTML verbatim (no escaping).
* `{{#Field}}…{{/Field}}` renders children iff field non-empty (per `field_is_empty`); `{{^Field}}` iff empty.
* `{{FrontSide}}` (answer side) → the fully rendered question HTML (first rendered text node of the question).
* Question "blank" detection: normal notetypes → if the qfmt doesn't render with the non-empty fields, Anki shows
  "The front of this card is blank"; cloze → if no field contains cloze `card_ord+1`, "No cloze N found".
* Filters (built-ins), from `apply_filter`:
  | filter | effect |
  |---|---|
  | `text` | `strip_html` (tags removed, entities decoded, NBSP→space) |
  | `furigana` | ` ?([^ >]+?)\[(.+?)\]` → `<ruby><rb>$1</rb><rt>$2</rt></ruby>` (skips `[sound:…]`; `&nbsp;`→space first) |
  | `kana` | same regex → `$2` |
  | `kanji` | same regex → `$1` |
  | `hint` | if non-blank (whitespace/newlines in the real output differ slightly): `<a class=hint href="#" onclick="this.style.display='none';document.getElementById('hint{id}').style.display='block';return false;" draggable=false>{FieldName}</a><div id="hint{id}" class=hint style="display: none">{text}</div>`, `id` = hex of first 8 bytes of blake3(text‖fieldName) |
  | `type` | outermost only: emits placeholder `[[type:Field]]`; modifiers before it (`cloze`, `nc`, `ci`) are folded in canonical order: `{{type:cloze:Text}}` → `[[type:cloze:Text]]`, `{{type:nc:F}}` → `[[type:nc:F]]` (`nc` = ignore combining chars). The GUI replaces the placeholder with an `<input>` on the front and a diff on the back. `type-cloze`/`type-nc` legacy spellings accepted |
  | `cloze` | cloze rendering for this card (5.3); then strips HTML inside `\(..\)`/`\[..\]` MathJax |
  | `cloze-only` | only the cloze texts for this ordinal, joined with `", "` (used by TTS) |
  | `tts <opts>` | `[anki:tts lang=<opts>]text[/anki:tts]` |
  | `` (empty, from `::`) | no-op |
  | anything else | unknown; in Anki it's passed to Python add-on filter hooks, otherwise ignored |
* `{{Tags}}`, `{{Deck}}`, `{{Subdeck}}`, `{{CardFlag}}`, `{{Card}}`, `{{Type}}`, `{{CardID}}`: special fields (§4.3).

### 5.3 Cloze syntax and nesting

* Marker: `{{c<N>[,<M>...]::text[::hint]}}`. Multiple ordinals `{{c1,2::x}}` (deduped, sorted) are active on both
  cards. Hint = text after the first `::` inside the cloze's first text chunk. Nesting allowed up to **10 levels**
  (deeper opens are ignored). A `}}` inside a MathJax `\( … \)` / `\[ … \]` started inside the cloze does not close
  it. Stray `}}` outside any cloze is literal text. [R:rslib/src/cloze.rs]
* Output for card ordinal *k* (= `card.ord + 1`):
  - active, question: `<span class="cloze" data-cloze="{attr-escaped revealed inner html}" data-ordinal="1">[hint-or-...]</span>`
  - active, answer: `<span class="cloze" data-ordinal="1">inner</span>`
  - inactive (either side): `<span class="cloze-inactive" data-ordinal="2">inner</span>` (inner clozes still processed)
  - if no cloze with ordinal *k* exists in the field, `{{cloze:Field}}` renders **empty string**.
  Test vectors (verbatim from Anki's tests): `reveal_cloze_text("foo {{c1::bar {{c2::baz}}}}", 1, false)` =
  `foo <span class="cloze" data-ordinal="1">bar <span class="cloze-inactive" data-ordinal="2">baz</span></span>`;
  stripped `("foo {{c1::bar {{c2::baz}}::qux}}", 1, question)` = `foo [qux]`; `(…, 2, question)` = `foo bar [...]`.
* `cloze_numbers_in_string("{{c2::te}}{{c1::s}}t{{")` = {1,2}; `{{c0::…}}` is parsed but never counted.
* Type-in for cloze: answer = all active cloze texts; identical ones collapse to one, else joined by `", "`.

### 5.4 Image Occlusion (built-in since 23.10)

Stock IO notetype = **cloze kind**, `originalStockKind = 6`, fields (in order, with `Field.Config.tag`):
`Occlusion` (tag 0), `Image` (1), `Header` (2), `Back Extra` (3), `Comments` (4); first four
`prevent_deletion=true`. Anki identifies IO notetypes by `original_stock_kind == ImageOcclusion` and ≥4 fields.
Template (names localized; English shown):
```
{{#Header}}<div>{{Header}}</div>{{/Header}}
<div style="display: none">{{cloze:Occlusion}}</div>
<div id="err"></div>
<div id="image-occlusion-container">
    {{Image}}
    <canvas id="image-occlusion-canvas"></canvas>
</div>
<script>
try {
    anki.imageOcclusion.setup();
} catch (exc) {
    document.getElementById("err").innerHTML = `Error loading image occlusion. Is your Anki version up to date?<br><br>${exc}`;
}
</script>
```
afmt = qfmt + `\n<div><button id="toggle">Toggle Masks</button></div>\n{{#Back Extra}}<div>{{Back Extra}}</div>{{/Back Extra}}\n`.
[R:rslib/src/image_occlusion/notetype.rs], [R:proto/anki/image_occlusion.proto] (`enum ImageOcclusionField`).

`Image` field holds `<img src="file.png">`. `Occlusion` field holds one cloze per shape, each followed by `<br>`:
```
{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}<br>
{{c2::image-occlusion:ellipse:left=.1:top=.2:width=.1:height=.2:rx=.05:ry=.1}}<br>
{{c3::image-occlusion:polygon:points=.1,.1 .2,.2 .3,.1}}<br>
{{c0::image-occlusion:text:text=Label\:x:left=.4:top=.5:scale=1:fs=.05}}<br>
```
Rules (TS writer `shapeOrShapesToCloze` + Rust parser `parse_image_cloze`): format
`image-occlusion:<shape>(:<key>=<value>)*`; shape ∈ `rect|ellipse|polygon|text`; in values `\` and `:` are escaped
as `\\` and `\:`; current editor writes coordinates **normalized to 0–1 of the image size** (`shape.toNormal(size)`),
a code comment in `imageocclusion.rs` shows pixel values were once emitted (the renderer accepts either;
it passes numbers through as `data-*` attributes); keys rendered: `left top angle fill width height rx ry points oi(=occludeInactive) text scale fs`;
grouped shapes share one ordinal and are emitted as consecutive clozes with the same `cN`; text labels use `c0`
(no card). Rendering emits `<div class="cloze|cloze-inactive|cloze-highlight" data-ordinal=".." data-shape=".." data-left=".." …></div>`
consumed by `anki.imageOcclusion.setup()`. Hint parsing (`::`) is disabled for `image-occlusion:` clozes.
[R:ts/routes/image-occlusion/shapes/to-cloze.ts], [R:rslib/src/image_occlusion/imageocclusion.rs], [R:rslib/src/cloze.rs]
(Legacy "Image Occlusion Enhanced" add-on notes are a different, add-on-specific notetype with SVG masks in fields;
treat them as ordinary notes.)

### 5.5 Media & LaTeX markup inside fields

* Audio/video: `[sound:filename.mp3]` (regex `\[sound:(.+?)\]`). Anki converts to `[anki:play:q:N]` placeholders at
  render time. Images/other: `<img src="…">`, `<audio|video|object|source src|data="…">` — references are found with
  `HTML_MEDIA_TAGS` (quoted or unquoted `src`/`data`), and entity-decoded (`&amp;` → `&`) to get the filename.
  Remote `http(s)://` URLs are not media. [R:rslib/src/text.rs]
* Files starting with `_` are "static" (shared by templates, e.g. fonts/JS referenced from templates/CSS); files
  starting with `latex-` are LaTeX renders. Both are exempt from per-note usage tracking on import. [R:rslib/src/import_export/package/apkg/import/media.rs]
* LaTeX: `[latex]…[/latex]` (verbatim LaTeX), `[$]…[/$]` → `$…$`, `[$$]…[/$$]` →
  `\begin{displaymath}…\end{displaymath}`; regex case-insensitive, non-greedy, dot-all. Before hashing, `<br>`,
  `<br />`, `<div>` → `\n`, then `strip_html`. Rendered filename = `latex-{sha1hex(latex_text)}.{png|svg}` (svg if
  notetype `latexsvg`), and the markup is replaced by `<img class=latex alt="{attr-escaped latex}" src="{fname}">`.
  Test vector: `a[latex]one<br>and<div>two[/latex]b` → file `latex-ef30b3f4141c33a5bf7044b0d1961d3399c05d50.png`.
  Clozes inside LaTeX are expanded front+back per ordinal before extraction. [R:rslib/src/latex.rs]
* MathJax `\(…\)` and `\[…\]` are rendered client-side (no files).

---

## 6. Collection config, decks, deck options

### 6.1 Day boundary / timing (needed to interpret `due` for review cards)

`days_elapsed` ("today" as a day number) is computed from `col.crt` and config [R:rslib/src/scheduler/timing.rs],
[R:rslib/src/scheduler/mod.rs]:

* `schedVer` missing/1 ⇒ **v1**: `days_elapsed = floor((now − crt) / 86400)`.
* v2/v3 and `creationOffset` missing ⇒ legacy v2: `crt_at_rollover` = crt's date at `rollover` hour in the *current*
  UTC offset; `days = floor((now − crt_at_rollover)/86400)`.
* v2/v3 with `creationOffset` (minutes **west** of UTC at creation; e.g. UTC+10 → −600) ⇒ new: count calendar-day
  changes between creation local date (in creationOffset) and now's local date (current offset, `localOffset` if
  set), minus one if today's rollover hour hasn't passed yet; min 0.
* `rollover` = hour 0–23 (default 4; Anki writes 4 if missing on a v2 collection).
* Review-card `due` (queue 2/3 and type 2) = `days_elapsed` value on which it's due.
* Anki sets `crt` to the most recent local 04:00 when creating a collection.

### 6.2 Collection config keys (`col.conf` JSON in schema 11; one row per key in `config` table in schema 18)

Defaults Anki writes for a new collection — verbatim `schema11_config_as_string()` [R:rslib/src/config/schema11.rs]:

```json
{
  "activeDecks": [1], "curDeck": 1, "newSpread": 0, "collapseTime": 1200, "timeLim": 0,
  "estTimes": true, "dueCounts": true, "curModel": null, "nextPos": 1, "sortType": "noteFld",
  "sortBackwards": false, "addToCur": true, "dayLearnFirst": false, "schedVer": 2,
  "creationOffset": <minutes west, or null on server>, "sched2021": true
}
```

| key | meaning |
|---|---|
| `curDeck` | current deck id; `activeDecks` legacy selection list |
| `curModel` | last-used notetype id (string or number) |
| `nextPos` | next new-card position (`due` for the next new card) |
| `newSpread` | 0 mix new with reviews, 1 reviews first, 2 new first (legacy v2) |
| `collapseTime` | learn-ahead limit, seconds (default 1200) |
| `timeLim` | answer timebox, seconds (0 off) |
| `dayLearnFirst` | show interday learning before reviews (v2) |
| `estTimes`, `dueCounts`, `addToCur`, `sortType`, `sortBackwards` | UI prefs |
| `schedVer` | 1 or 2 (2 = v2/v3). **Missing ⇒ v1** |
| `sched2021` | true ⇒ v3 scheduler (with schedVer 2) |
| `rollover` | day rollover hour (v2+) |
| `creationOffset` | UTC offset at creation (minutes west) — enables new day-cutoff logic |
| `localOffset` | current UTC offset (minutes west), maintained by clients |
| `lastUnburied` | day number of last auto-unbury |
| `fsrs` | bool: FSRS enabled globally |
| `normalize_note_text` | bool (default true): NFC normalize on save |
| others | `firstDayOfWeek`, `backups`, `updateNotes`, `updateNotetypes`, `mergeNotetypes`, `withScheduling`, `withDeckConfigs`, `applyAllParentLimits`, `newCardsIgnoreReviewLimit`, `loadBalancerEnabled`, `cardStateCustomizer`, per-deck `_deck_{id}_lastNotetype`, per-notetype `_nt_{id}_lastDeck` … |

[R:rslib/src/config/mod.rs], [R:rslib/src/config/bool.rs], [R:rslib/src/config/string.rs],
[R:rslib/src/config/deck.rs], [R:rslib/src/config/notetype.rs]. Schema-14 upgrade copies every `conf` key into
`config(KEY, usn=0, mtime_secs=0, val=JSON)` [R:rslib/src/storage/config/mod.rs].

### 6.3 Deck JSON (`col.decks`, keyed by deck id string) — `DeckSchema11` [R:rslib/src/decks/schema11.rs]

Variant chosen by `dyn` (0/false normal, 1/true filtered; REQUIRED). Common keys:

| key | req? | notes |
|---|---|---|
| `id` | yes | number or numeric string; default deck = 1 |
| `name` | yes | human name, hierarchy with `::` (schema 18 stores `\x1f`) |
| `usn` | yes | |
| `mod` | no (default 0) | seconds |
| `lrnToday`, `revToday`, `newToday`, `timeToday` | yes | `[dayNumber, amount]`; time amount in ms. On upgrade only entries whose day equals the max day keep their amounts |
| `collapsed` | yes | study-screen collapsed; `browserCollapsed` optional |
| `desc` | no | HTML description; `md` (bool) = markdown description |
| `dyn` | yes | |

Normal-only: `conf` (REQUIRED, deck-config id), `extendNew`, `extendRev` (custom-study extensions), `reviewLimit`,
`newLimit`, `reviewLimitToday`, `newLimitToday` (`{"limit":N,"today":dayNumber}`), `desiredRetention` (**integer
percent**; becomes float 0–1 in proto).
Filtered-only: `resched` (REQUIRED), `terms` (REQUIRED, array of `[search, limit, order]`), `separate` (legacy, keep
`true`), `delays` (v1, null or array), `previewDelay` (minutes, v2), `previewAgainSecs`, `previewHardSecs`,
`previewGoodSecs`.

### 6.4 Deck-options JSON (`col.dconf`, keyed by id string) — `DeckConfSchema11` [R:rslib/src/deckconfig/schema11.rs]

Required top-level: `id`, `mod` (secs), `name`, `usn`, `maxTaken` (secs), `autoplay`, `timer` (0/1), `new`, `rev`,
`lapse`. Defaults in parentheses:

* `new`: `delays` (minutes, [1,10]), `ints` ([good grad days, easy grad days, unused]; [1,4,0]),
  `initialFactor` (permille, 2500), `order` (0 random, **1 due**), `perDay` (20), `bury` (false).
* `rev`: `perDay` (200), `ease4` (easy bonus 1.3), `ivlFct` (interval modifier 1.0), `maxIvl` (days 36500),
  `hardFactor` (1.2), `bury` (false). Legacy `fuzz`, `minSpace` pass through.
* `lapse`: `delays` (minutes, [10]), `mult` (new interval after lapse, 0.0), `minInt` (days, 1), `leechFails` (8),
  `leechAction` (0 suspend, **1 tag only**).
* v3/FSRS extras persisted at top level: `newMix`, `newPerDayMinimum`, `interdayLearningMix`, `reviewOrder`,
  `newSortOrder`, `newGatherPriority`, `buryInterdayLearning`, `fsrsWeights` (FSRS-4 params), `fsrsParams5`,
  `fsrsParams6`, `desiredRetention` (float, 0.9), `sm2Retention` (historical retention 0.9), `weightSearch`,
  `ignoreRevlogsBeforeDate`, `easyDaysPercentages` ([1.0×7]), `stopTimerOnAnswer`, `secondsToShowQuestion`,
  `secondsToShowAnswer`, `questionAction`, `answerAction`, `waitForAudio` (true), `replayq` (true), `dyn` (false).

Mapping to proto: `learn_steps=new.delays`, `relearn_steps=lapse.delays`, `initial_ease=initialFactor/1000`,
`easy_multiplier=ease4`, `hard_multiplier=hardFactor`, `lapse_multiplier=mult`, `interval_multiplier=ivlFct`,
`maximum_review_interval=maxIvl`, `minimum_lapse_interval=minInt`, `graduating_interval_good/easy=ints[0]/[1]`,
`leech_threshold=leechFails`, `desired_retention=desiredRetention`, `historical_retention=sm2Retention`.

**FSRS storage summary**: on/off = global config `fsrs`; parameters + desired retention per preset in deck config
(`fsrs_params_6` field 6 / JSON `fsrsParams6`, etc.); optional per-deck retention override
(`Deck.Normal.desired_retention` / JSON `desiredRetention` int %); per-card memory state in `cards.data` (`s`,`d`,
`dr`,`decay`,`lrt`); revlog `factor` holds shifted difficulty when FSRS graded the review.

---
## 7. Reading a package (algorithm)

```
1. unzip (fflate). entries = map name -> bytes
2. if "meta" exists: version = decode PackageMetadata (field 1 varint). 0 or >3 -> "too new", stop.
   else version = "collection.anki21" in entries ? 2 : 1
3. colBytes = entries[{1:"collection.anki2",2:"collection.anki21",3:"collection.anki21b"}[version]]
   if version==3: colBytes = zstdDecompress(colBytes)
4. db = sql.js(colBytes); if sqlite_master mentions "unicase": strip collation (§2.4)
5. ver = SELECT ver FROM col.  11 -> parse JSON columns (models/decks/dconf/conf/tags).
                               18 -> read notetypes/fields/templates/decks/deck_config/config/tags tables + protobuf blobs
   (Anki itself accepts 11..18 except 12/13; versions 14-17 are intermediate layouts of the 18 tables.)
6. media map: raw = entries["media"] ?? "{}" ; if version==3: raw = zstdDecompress(raw), decode MediaEntries,
   zipName(i) = String(entries[i].legacy_zip_filename ?? i) ; else JSON {zipName: filename}
   body = entries[zipName]; if version==3: body = zstdDecompress(body)
   validate: single path component, normalize (NFC etc., §1.2); verify sha1 if present.
7. notes: fields = flds.split("\x1f"); tags = tags.split(/[ 　]/).filter(Boolean)
8. cards/revlog: interpret due per queue/type (§3.2) against the *package's* days_elapsed (§6.1), not yours.
```

Schema versions Anki can open: `SCHEMA_MIN_VERSION = 11`, `SCHEMA_MAX_VERSION = 18`; 12 and 13 are rejected
("must perform clean shutdown to return to schema 11"). [R:rslib/src/storage/upgrades/mod.rs], [R:rslib/src/storage/sqlite.rs]

---

## 8. How Anki imports an .apkg into an existing collection (Rust importer, 2.1.55+ default; only importer since 24.11 UI / 26.09 API)

Release history: new Rust import/export became default in **2.1.55**; 23.10 added notetype merging, "exclude
scheduling", and made the new (`anki21b`) format the default for .apkg export; 23.12 added with/without deck
configs; 24.11 removed the legacy import option; 26.09 removed the Python `anki.importing/anki.exporting` modules.
https://github.com/ankitects/anki/releases (tags 2.1.55, 23.10, 23.12, 24.11, 26.09)

Pipeline (`Context::import`): `prepare_media` → `import_notes_and_notetypes` → `import_decks_and_configs` →
`import_cards_and_revlog` → `copy_media`, inside one undoable transaction. Options: `merge_notetypes`,
`update_notes` / `update_notetypes` ∈ {IfNewer, Always, Never}, `with_scheduling`, `with_deck_configs`.
[R:rslib/src/import_export/package/apkg/import/mod.rs]

### 8.1 Preparing the source

* Source DB opened as a full collection (schema upgraded to 18); `maybe_fix_invalid_ids()`; and
  **if any card has `queue != 0` and source `schedVer` ≠ 2, the v1→v2 upgrade runs on the source**: revlog
  `UPDATE revlog SET ease = ease + 1 WHERE ease IN (2,3) AND type IN (0,2)`, relearning cards get type 3, filtered-deck
  fixes, and `creationOffset` is set. [R:rslib/src/import_export/package/apkg/import/mod.rs],
  [R:rslib/src/scheduler/upgrade.rs], [R:rslib/src/storage/revlog/v2_upgrade.sql]
* `gather_data`: `days_elapsed` of the **source** collection (its own crt/rollover/offset) is recorded.
  Without scheduling: every card is reset to new (positions from the source's `nextPos`), flags cleared,
  `marked`/`leech` tags removed, deck study counters/limits cleared, filtered decks turned into normal decks.
  Without deck configs: every deck's `config_id` becomes 1. With scheduling, cards in filtered decks are restored to
  their home deck unless all original decks are included. Any card/note/revlog id > now+1 day ⇒ `InvalidId` error
  (the importer first tries `maybe_fix_invalid_ids`). [R:rslib/src/import_export/gather.rs]

### 8.2 Notetypes (matched by **id**)

* Not present in target ⇒ added with the same id (name made unique; `prepare_for_update` recomputes `reqs`).
* Present, no-merge mode: "same schema" = same number of fields and templates and each pair `is_match` (by
  `config.id` when both have one, else by **name**). Same schema ⇒ update the target notetype iff
  `existing.mtime < incoming.mtime` (IfNewer). Different schema ⇒ reuse a notetype previously created from this one
  (`original_id == incoming.id` and same schema) or add a **copy with a new id** and remap the incoming notes to it.
* Merge mode: cloze vs normal mismatch ⇒ error; fields/templates are unioned (matching by id/name), ords remapped for
  note fields and card template indexes; previously-duplicated sibling notetypes are folded back in.
* Every imported notetype gets `config.original_id = source id`.
[R:rslib/src/import_export/package/apkg/import/notes.rs], [R:rslib/src/notetype/merge.rs]

### 8.3 Notes (matched by **guid**)

* guid not in target ⇒ add: id kept unless taken (then `+999` until free), media refs rewritten (8.6), tags
  canonified, fields normalized, **`sfld`/`csum` recomputed** (package values are not trusted), usn = target usn.
* guid in target: if (no-merge) the target note's notetype ≠ incoming (possibly remapped) notetype ⇒ logged
  "conflicting", skipped. Else `should_update` — IfNewer: `existing.mtime < incoming.mtime`; Always:
  `existing.mtime != incoming.mtime`; Never ⇒ update fields+tags **keeping the incoming mtime** (so successive exports
  re-import cleanly), else logged "duplicate".
* Legacy Python importer (≤2.1.54 default, optional until 24.11): same guid rule, update only if `oldMod < mod` and
  same mid; notetype matching by id + `scmhash` (= checksum of concatenated field names + template names), bumping
  the mid by +1 until a free or matching id is found. https://github.com/ankitects/anki/blob/2.1.49/pylib/anki/importing/anki2.py,
  https://github.com/ankitects/anki/blob/2.1.49/pylib/anki/models.py

### 8.4 Decks & deck configs

* Deck configs: `INSERT OR IGNORE` by **id** — an existing preset with the same id is never overwritten.
  [R:rslib/src/storage/deckconfig/add_if_unique.sql]
* Decks are processed parents-first (sorted by depth) and matched by **name** (case-insensitive via unicase), not
  id. Same kind ⇒ update in place: description replaced if incoming non-empty, `config_id` replaced unless incoming is
  1, limits taken from incoming when set. Kind differs (normal vs filtered) ⇒ incoming renamed `"<name> <unix-secs>"`
  and its children re-parented. If the first existing ancestor is filtered, a uniquely-named normal parent is created.
  New decks get a **new id** (source ids are not preserved) and the old→new map is applied to cards (`did` and
  `odid`). "Today" limits are re-based: if limit.today == source today ⇒ target today; else min(today, targetToday−1).
  [R:rslib/src/import_export/package/apkg/import/decks.rs]

### 8.5 Cards & revlog — the `due` shift

* Only cards whose note was added/updated are considered; a card is skipped if the target already has a card with the
  same `(nid, ord)` (existing cards are never updated). Template index remapped if notetypes were merged.
* `collection_delta = source.days_elapsed − target.days_elapsed`; for cards with `queue ∈ {2,3}` or `type == 2`:
  `due -= delta`; for `type == 2` and `odue != 0`: `odue -= delta`. Id `+999` until unique; `INSERT OR IGNORE`.
* Revlog rows imported only for imported cards, `cid` remapped, usn = target, `INSERT OR IGNORE` by id.
* Target on v1 scheduler ⇒ `SchedulerUpgradeRequired` error.
[R:rslib/src/import_export/package/apkg/import/cards.rs]

```rust
fn shift_collection_relative_dates(&mut self, delta: i32) {
    if self.due_in_days_since_collection_creation() { self.due -= delta; }
    if self.original_due_in_days_since_collection_creation() && self.original_due != 0 { self.original_due -= delta; }
}
fn due_in_days_since_collection_creation(&self) -> bool {
    matches!(self.queue, CardQueue::Review | CardQueue::DayLearn) || self.ctype == CardType::Review
}
fn original_due_in_days_since_collection_creation(&self) -> bool { self.ctype == CardType::Review }
```

Legacy Python did the same with `aheadBy = src.sched.today - dst.sched.today`, but shifted any non-zero `odue` and
converted filtered-deck cards back to normal (`due = odue`, learn → new). (2.1.49 `anki2.py` `_importCards`.)

### 8.6 Media

* Static files (name starts with `_` or `latex-`): copied only if no file with that name exists (content ignored).
* Other files: name absent in target ⇒ copy (if referenced by an imported note). Same name, **same SHA-1** ⇒ reuse.
  Same name, different content ⇒ rename to `"{stem}-{sha1hex}.{ext}"` (stem truncated so total ≤ 120 bytes) and
  rewrite references in imported notes (`replace_media_refs`, HTML-escaping the new name if the old ref was
  entity-encoded). Only entries actually referenced by imported/updated notes are copied.
  [R:rslib/src/import_export/package/apkg/import/media.rs], [R:rslib/src/media/files.rs],
  [R:rslib/src/import_export/package/apkg/import/notes.rs]
* Legacy Python: on content mismatch renamed to `"{name}_{mid}{ext}"`, reusing an existing `_{mid}` file.
  (2.1.49 `anki2.py` `_mungeMedia`.) — **conflict**: dedupe suffix differs between importers.
* .colpkg import replaces the whole collection file, runs `integrity_check`, bumps `scm`/`mod`, and restores media
  files whose size/sha1 differ. [R:rslib/src/import_export/package/colpkg/import.rs]

---

## 9. Writing a legacy .apkg that Anki 2.1.x … 26.x will import

Reference implementation that has worked for years: genanki
(https://github.com/kerrickstaley/genanki/blob/main/genanki/package.py,
https://github.com/kerrickstaley/genanki/blob/main/genanki/apkg_col.py,
https://github.com/kerrickstaley/genanki/blob/main/genanki/model.py). It writes `collection.anki2`, a JSON `media`
map, numbered files, and no `meta`; it carries only new cards, so it never hits the v1/v2 issue. For a *recall*
exporter that carries scheduling, do the following.

1. **DB**: create SQLite with the schema-11 DDL of §2.1 (journal mode DELETE, the sql.js default).
2. **`col` row** — all 13 columns NOT NULL:
   ```
   id=1, crt=<collection creation, unix secs; best: local 04:00 of creation day>, mod=<now ms>, scm=<now ms>,
   ver=11, dty=0, usn=0, ls=0,
   conf=JSON (below), models=JSON, decks=JSON, dconf=JSON, tags='{}'  (or {"tag": usn,...})
   ```
   `conf` minimal recommended:
   ```json
   {"activeDecks":[1],"curDeck":1,"newSpread":0,"collapseTime":1200,"timeLim":0,"estTimes":true,
    "dueCounts":true,"curModel":null,"nextPos":<max new due + 1>,"sortType":"noteFld","sortBackwards":false,
    "addToCur":true,"dayLearnFirst":false,"schedVer":2,"sched2021":true,"rollover":4,
    "creationOffset":<minutes west of UTC at crt>}
   ```
   **`schedVer: 2` is mandatory when any card has `queue != 0`**, otherwise Anki runs the v1→v2 conversion and adds
   +1 to learn/relearn revlog eases 2/3 (§8.1). `rollover` + `creationOffset` make the importer compute the source
   "today" exactly as you did when you wrote `due` values.
3. **models**: one entry per notetype per §4.1, with all required keys (`id,name,type,mod,usn,sortf,did,tmpls,flds,css`;
   fields: `name,ord,sticky,rtl,font,size`; templates: `name,ord,qfmt` + `afmt,bqfmt,bafmt,did,bfont,bsize`). Add
   `latexPre`, `latexPost`, `latexsvg`, `req`, `tags:[]`, `vers:[]` for old clients. Cloze type must have exactly one
   template.
4. **decks**: include `"1"` Default plus every deck referenced by `cards.did`/`odid` and every **parent** deck
   (`A::B` needs `A`). Each needs `id,name,usn,mod,collapsed,browserCollapsed,desc,dyn:0,conf,extendNew,extendRev,
   newToday,revToday,lrnToday,timeToday` (`[0,0]` is fine).
5. **dconf**: `"1"` Default plus any preset referenced by decks, with all of `id,mod,name,usn,maxTaken,autoplay,timer,
   replayq,dyn,new{delays,ints,initialFactor,order,perDay,bury},rev{perDay,ease4,ivlFct,maxIvl,hardFactor,bury},
   lapse{delays,mult,minInt,leechFails,leechAction}`; add FSRS keys (§6.4) to round-trip FSRS presets. Old Python
   clients may also expect legacy `new.separate`, `rev.fuzz`, `rev.minSpace` (genanki writes them).
6. **notes**: `id` (ms, ≤ now), `guid`, `mid`, `mod` (secs), `usn=-1`, `tags=" a b "`, `flds` joined by `\x1f`,
   `sfld`, `csum` (§10 — recomputed by modern Anki, but old clients and duplicate-checks use them), `flags=0`, `data=""`.
7. **cards**: one per generated ord (§4.4). New: `type=0,queue=0,due=position,ivl=0,factor=0,reps=0,lapses=0,left=0`.
   Review: `type=2,queue=2,due=dayNumber` where dayNumber uses *this file's* crt/rollover/creationOffset;
   `ivl` days, `factor` permille. Learning (intraday): `type=1|3, queue=1, due=unix secs, left=steps remaining`.
   `odue=0, odid=0` unless filtered, `flags`, `data` JSON (FSRS `s`,`d`,`dr`,`decay`,`lrt`).
8. **revlog**: as §3.3; ids in ms, ≤ now + 1 day.
9. **zip**: `collection.anki21` (deflate) — or `collection.anki2` if you also want Anki 2.0; `media` = JSON
   `{"0":"img.png",...}` (**always present**, `{}` if no media); files `"0"`,`"1"`… with raw bytes; optionally
   `meta` = bytes `08 02`. Do not emit `collection.anki21b` unless you write a valid schema-18 DB.
   - With only `collection.anki2` (no meta), Anki 2.1.x Python treats the file as **v1** (`importing_v2=False`) and
     upgrades it if scheduled cards exist and `schedVer` isn't 2; Anki 2.1.x into a v1 target collection with an
     `.anki21` file raises `V2ImportIntoV1`. (2.1.49 `anki2.py`, `apkg.py`.)
10. Media filenames in the map must already be normalized (§1.2) and unique; reference them in fields exactly.

Writing **v3** (`anki21b`) instead: create schema 11, apply the four upgrade scripts (§2.2), fill the new tables with
protobuf blobs (decks names with `\x1f`), set `col.ver=18` and the JSON columns to `''`, put config keys as JSON
blobs in `config`, zstd the DB, write `MediaEntries` (name, size, sha1 of uncompressed bytes) zstd-compressed, zstd
each media file, `meta = 08 03`, and add a dummy `collection.anki2`. The zstd/protobuf package format shipped with
the reworked .colpkg in 2.1.50 ("resulting .colpkg will not be readable by older Anki clients",
https://github.com/ankitects/anki/releases/tag/2.1.50); the Rust apkg importer that reads it became default in 2.1.55
(https://github.com/ankitects/anki/releases/tag/2.1.55). Treat v3 .apkg as requiring Anki ≥ 2.1.50 (≥ 2.1.55 to be safe).

---

## 10. GUIDs, checksums, HTML stripping (with TS)

### 10.1 GUID

Anki: `guid = anki_base91(rand::random::<u64>())`, alphabet
``abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~`` (91 chars), most
significant digit first, `0 → ""`. Test vectors: `1→"b"`, `1234567890→"saAKk"`, `u64::MAX→"Rj&Z5m[>Zp"`.
[R:rslib/src/notes/mod.rs]. genanki makes deterministic guids: first 8 bytes of SHA-256 of `"__".join(values)` as a
big-endian u64, same alphabet (https://github.com/kerrickstaley/genanki/blob/main/genanki/util.py). Anki treats guid
as an opaque string — any unique string works on import (e.g. preserve ours).

```ts
const B91 = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~";
export function base91(n: bigint): string {
  let s = "";
  while (n > 0n) { s = B91[Number(n % 91n)] + s; n /= 91n; }
  return s;
}
export function newGuid(): string {
  const b = crypto.getRandomValues(new Uint8Array(8));
  return base91(b.reduce((acc, x) => (acc << 8n) | BigInt(x), 0n));
}
```

### 10.2 `csum` and `sfld`

```rust
pub(crate) fn field_checksum(text: &str) -> u32 {          // text = strip_html_preserving_media_filenames(field0)
    let digest = Sha1::digest(text);  u32::from_be_bytes(digest[..4].try_into().unwrap())
}
// tests: field_checksum("test") == 2840236005; field_checksum("今日") == 1464653051
```
[R:rslib/src/notes/mod.rs]. (AnkiDroid wiki: "integer representation of first 8 digits of sha1 hash of the first
field" — same thing: 8 hex digits = 4 bytes.)

```ts
export async function fieldChecksum(stripped: string): Promise<number> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-1", new TextEncoder().encode(stripped)));
  return ((d[0] << 24) | (d[1] << 16) | (d[2] << 8) | d[3]) >>> 0;
}
```

### 10.3 `strip_html_preserving_media_filenames` (used for `sfld`, `csum`, duplicate checks)

```
1. HTML_MEDIA_TAGS.replace_all(html, " ${1}${2}${3} ")   // <img src="x.jpg"> -> " x.jpg "  (src/data attr of img|audio|video|object|source)
2. strip_html:
   a. remove (?si) <!--.*?--> | <style.*?>.*?</style> | <script.*?>.*?</script> | <.*?>
   b. if contains '&': decode HTML entities (htmlescape::decode_html; on failure keep text); then U+00A0 -> ' '
```
No trimming, no `[sound:]` removal, no whitespace collapse. Regexes verbatim in [R:rslib/src/text.rs] (§5.5 above for
`HTML_MEDIA_TAGS`). A TS port:

```ts
const MEDIA = /<\b(?:img|audio|video|object|source)\b(?:[^>]|"[^"]+?"|'[^']+?')+?\b(?:src|data)\b=(?:"([^"]+?)"[^>]*>|'([^']+?)'[^>]*>|([^ >]+?)(?: [^>]*>|>))/gis;
const HTML  = /(<!--.*?-->)|(<style.*?>.*?<\/style>)|(<script.*?>.*?<\/script>)|(<.*?>)/gis;
export function stripHtmlPreservingMedia(s: string): string {
  s = s.replace(MEDIA, (_m, a, b, c) => ` ${a ?? ""}${b ?? ""}${c ?? ""} `);
  s = s.replace(HTML, "");
  if (s.includes("&")) s = decodeEntities(s).replace(/ /g, " ");  // use a full HTML5 entity table
  return s;
}
```
Caveat: Rust's `regex` and JS differ in edge cases (e.g. Rust `.` in `(?s)` vs JS `s` flag are equivalent; `\b`
semantics are ASCII in JS vs Unicode in Rust). Test against Anki-produced csums.

### 10.4 Empty-field test (card generation)

```ts
const EMPTY = /^(?:\s|<\/?(?:br|div) ?\/?>)*$/i;   // Rust: (?xsi)^(?:[[:space:]]|</?(?:br|div)\ ?/?>)*$
export const fieldIsEmpty = (s: string) => EMPTY.test(s);
```
(`[[:space:]]` is ASCII whitespace in Rust; JS `\s` also matches Unicode spaces incl. NBSP — use
`[ \t\n\v\f\r]` for exact parity.)

---

## 11. Existing third-party readers/writers (for cross-checking)

| project | lang | reads | writes | notes |
|---|---|---|---|---|
| genanki (kerrickstaley) | Python | – | legacy `collection.anki2` | canonical minimal writer; new cards only |
| anki-apkg-export (repeat-space) | JS | – | legacy | sql.js-based; https://github.com/repeat-space/anki-apkg-export |
| ankipack (ImGajeed76) | TS | 1/2/3 | 1/2/3 | generated protos via `@bufbuild/protobuf`, `fzstd`, raw-block zstd writer, unicase key folding; https://github.com/ImGajeed76/ankipack |
| srs-converter (eikowagenknecht) | TS | 1/2/3 | 1/2/3 | sql.js + `@hpcc-js/wasm-zstd`; hand-written protobuf wire codec; has its own format notes in `docs/formats/anki.md`; https://github.com/eikowagenknecht/srs-converter |
| anki-apkg-parser (74Genesis) | TS/Node | 1/2/3 | – | node sqlite3 + protobufjs; https://github.com/74Genesis/anki-apkg-parser |
| anki-reader (ewei068) | JS | legacy | – | browser+node; https://github.com/ewei068/anki-reader |
| AnkiDroid | Kotlin | all | all | uses the same Rust backend (rsdroid) → same semantics as desktop |

---

## 12. Conflicts between sources & corrections to common summaries

* **cards.ivl negative = seconds?** No. `cards.ivl` is `u32` days; negative-seconds encoding is a **revlog** thing
  (`ivl`, `lastIvl`). [R:rslib/src/card/mod.rs] vs [R:rslib/src/revlog/mod.rs]
* **`left`**: AnkiDroid wiki documents `a*1000+b`; v3 writes the plain remaining-step count; read with `% 1000`.
* **revlog `ease` for learning**: wiki "1 wrong, 2 ok, 3 easy" is v1-only; v2+ uses 1–4 everywhere. 0 = manual.
* **queue −2**: "sched buried" in v2/v3; "buried" in v1 (wiki).
* **new-card `due` "starts from 1"** (wiki) vs Rust `get_and_update_next_card_position()` default 0 if `nextPos`
  missing; schema-11 default config sets `nextPos: 1`.
* **Deck matching on import**: legacy Python matched by name via `decks.id(name)` too, but also copied a non-default
  dconf by id and *overwrote* it (`update_config`); the Rust importer never overwrites an existing preset id.
* **Media dedupe suffix**: legacy `name_{mid}.ext`, modern `name-{sha1}.ext`.
* **Media-name lowercasing**: 26.05 added lowercasing for *newly added* files via the editor
  (`add_data_to_folder_uniquely`), not for apkg import (apkg import path still uses the names verbatim).
  [R:rslib/src/media/files.rs], https://github.com/ankitects/anki/pull/4860

## 13. Gaps / not verified

* Whether every sql.js build opens WAL-flagged `anki21b` bytes without patching header bytes 18–19 (not tested; no
  builds run per instructions).
* Exact version where Python stopped using `req` for card generation (Rust cardgen) — not pinned.
* The `ci` type-answer modifier's exact semantics (listed in `TYPE_MODIFIERS = ["cloze","nc","ci"]`) — presumably
  case-insensitive comparison; not traced into the GUI code.
* `htmlescape::decode_html` entity coverage vs. a browser's HTML5 entity table (affects csum for exotic entities).
* Default values of `ImportAnkiPackageOptions` presented in the GUI (with_scheduling etc.) were not traced.
* AnkiDroid wiki "Database Structure" content predates FSRS fields in parts; FSRS keys taken from Rust only.

## 14. Primary URLs (verbatim)

Base for all `[R:...]`: https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/

* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/meta.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/media.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/colpkg/export.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/export.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/import/mod.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/import/notes.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/import/cards.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/import/decks.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/package/apkg/import/media.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/import_export/gather.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/schema11.sql
* https://github.com/ankitects/anki/tree/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/upgrades
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/sqlite.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/import_export.proto
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/notetypes.proto
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/decks.proto
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/proto/anki/deck_config.proto
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/card/mod.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/storage/card/data.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/revlog/mod.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/notes/mod.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/notetype/schema11.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/notetype/cardgen.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/notetype/render.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/template.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/template_filters.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/cloze.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/image_occlusion/imageocclusion.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/image_occlusion/notetype.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/ts/routes/image-occlusion/shapes/to-cloze.ts
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/text.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/latex.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/decks/schema11.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/deckconfig/schema11.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/config/schema11.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/timing.rs
* https://github.com/ankitects/anki/blob/a13d8a6e9e91d87eb046bf7b26ee64880a4eb544/rslib/src/scheduler/upgrade.rs
* https://github.com/ankitects/anki/blob/2.1.49/pylib/anki/importing/anki2.py
* https://github.com/ankitects/anki/blob/2.1.49/pylib/anki/importing/apkg.py
* https://github.com/ankidroid/Anki-Android/wiki/Database-Structure
* https://github.com/kerrickstaley/genanki
* https://github.com/ankitects/anki/releases
