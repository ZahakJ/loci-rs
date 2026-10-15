-- Loci collection schema, version 1.
-- Column names and units follow Anki where a column has an Anki equivalent, so that
-- import/export is a mapping rather than a translation. Units:
--   *.id for notes, cards, revlog: milliseconds since epoch (Anki convention)
--   mtime: seconds since epoch
--   cards.due: new -> position; learn -> seconds since epoch; review/relearn(day) -> days since crt-day
--   cards.ivl: days (negative = seconds, learning); cards.factor: permille (SM-2 ease); 0 under FSRS-only
--   revlog.time: milliseconds spent; revlog.ivl/last_ivl: days (negative = seconds)
PRAGMA journal_mode = DELETE;
PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS col_meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- keys: schema_version ('1'), collection_id (uuid), crt (seconds, local day start when
-- created), rollover_hour ('4'), created_with (app version), last_backup (seconds),
-- last_optimise (seconds), locale, theme, ui JSON (misc UI prefs), undo cursor etc.

CREATE TABLE IF NOT EXISTS notetypes (
  id          INTEGER PRIMARY KEY,         -- ms epoch, Anki-compatible
  name        TEXT NOT NULL COLLATE NOCASE,
  kind        INTEGER NOT NULL DEFAULT 0,  -- 0 standard, 1 cloze
  css         TEXT NOT NULL DEFAULT '',
  latex_pre   TEXT NOT NULL DEFAULT '',
  latex_post  TEXT NOT NULL DEFAULT '',
  sort_field  INTEGER NOT NULL DEFAULT 0,
  config      TEXT NOT NULL DEFAULT '{}',  -- JSON NotetypeConfig (req, originalId, builtin, clozeOneByOne, ...)
  mtime       INTEGER NOT NULL,
  usn         INTEGER NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS fields (
  ntid    INTEGER NOT NULL,
  ord     INTEGER NOT NULL,
  name    TEXT NOT NULL,
  config  TEXT NOT NULL DEFAULT '{}',      -- JSON FieldConfig {sticky, rtl, font, size, description, plainText, collapsed}
  PRIMARY KEY (ntid, ord)
);

CREATE TABLE IF NOT EXISTS templates (
  ntid    INTEGER NOT NULL,
  ord     INTEGER NOT NULL,
  name    TEXT NOT NULL,
  qfmt    TEXT NOT NULL,
  afmt    TEXT NOT NULL,
  bqfmt   TEXT NOT NULL DEFAULT '',
  bafmt   TEXT NOT NULL DEFAULT '',
  config  TEXT NOT NULL DEFAULT '{}',      -- JSON TemplateConfig {deckOverride?, browserFontOverride?}
  PRIMARY KEY (ntid, ord)
);

CREATE TABLE IF NOT EXISTS decks (
  id          INTEGER PRIMARY KEY,         -- ms epoch; 1 = Default
  name        TEXT NOT NULL UNIQUE COLLATE NOCASE,  -- '::' separated hierarchy
  kind        INTEGER NOT NULL DEFAULT 0,  -- 0 normal (filtered decks are not persisted in v1)
  config_id   INTEGER NOT NULL DEFAULT 1,  -- deck_configs.id (preset)
  description TEXT NOT NULL DEFAULT '',
  collapsed   INTEGER NOT NULL DEFAULT 0,
  today       TEXT NOT NULL DEFAULT '{}',  -- JSON DeckToday {day, new, rev, lrn, time} counters for the current day
  config      TEXT NOT NULL DEFAULT '{}',  -- JSON DeckExtra {extendNew, extendRev, originalAnkiJson?}
  mtime       INTEGER NOT NULL,
  usn         INTEGER NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS deck_configs (
  id      INTEGER PRIMARY KEY,             -- 1 = Standard (default preset)
  name    TEXT NOT NULL COLLATE NOCASE,
  config  TEXT NOT NULL,                   -- JSON DeckConfig (see types.ts); complete, typed, defaults filled
  mtime   INTEGER NOT NULL,
  usn     INTEGER NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS notes (
  id     INTEGER PRIMARY KEY,
  guid   TEXT NOT NULL,
  ntid   INTEGER NOT NULL,
  mtime  INTEGER NOT NULL,
  usn    INTEGER NOT NULL DEFAULT -1,
  tags   TEXT NOT NULL DEFAULT '',         -- ' tag1 tag2 ' with leading and trailing space (Anki convention); '' when none
  flds   TEXT NOT NULL,                    -- fields joined by U+001F
  sfld   TEXT NOT NULL DEFAULT '',         -- sort field, HTML stripped
  csum   INTEGER NOT NULL DEFAULT 0,       -- first 8 hex digits of sha1(stripped first field) as integer
  plain  TEXT NOT NULL DEFAULT ''          -- all fields HTML-stripped, joined by ' ', for FTS
);
CREATE INDEX IF NOT EXISTS ix_notes_guid ON notes (guid);
CREATE INDEX IF NOT EXISTS ix_notes_ntid ON notes (ntid);
CREATE INDEX IF NOT EXISTS ix_notes_csum ON notes (csum);

CREATE TABLE IF NOT EXISTS cards (
  id      INTEGER PRIMARY KEY,
  nid     INTEGER NOT NULL,
  did     INTEGER NOT NULL,
  ord     INTEGER NOT NULL,
  mtime   INTEGER NOT NULL,
  usn     INTEGER NOT NULL DEFAULT -1,
  type    INTEGER NOT NULL DEFAULT 0,      -- 0 new, 1 learn, 2 review, 3 relearn
  queue   INTEGER NOT NULL DEFAULT 0,      -- -3 user (manually) buried, -2 sched (sibling) buried, -1 suspended, 0 new, 1 learn, 2 review, 3 day-learn, 4 preview
  due     INTEGER NOT NULL DEFAULT 0,
  ivl     INTEGER NOT NULL DEFAULT 0,
  factor  INTEGER NOT NULL DEFAULT 0,
  reps    INTEGER NOT NULL DEFAULT 0,
  lapses  INTEGER NOT NULL DEFAULT 0,
  left    INTEGER NOT NULL DEFAULT 0,      -- remaining learning steps (Anki encoding: steps_left + 1000*steps_today in v2; v3 uses plain count)
  odue    INTEGER NOT NULL DEFAULT 0,
  odid    INTEGER NOT NULL DEFAULT 0,
  flags   INTEGER NOT NULL DEFAULT 0,      -- low 3 bits = flag colour 0..7
  data    TEXT NOT NULL DEFAULT '{}'       -- JSON CardData {s, d, dr, decay, pos, cd}
);
CREATE INDEX IF NOT EXISTS ix_cards_nid ON cards (nid);
CREATE INDEX IF NOT EXISTS ix_cards_sched ON cards (did, queue, due);
CREATE INDEX IF NOT EXISTS ix_cards_odid ON cards (odid) WHERE odid != 0;

CREATE TABLE IF NOT EXISTS revlog (
  id        INTEGER PRIMARY KEY,           -- ms epoch of the review
  cid       INTEGER NOT NULL,
  usn       INTEGER NOT NULL DEFAULT -1,
  ease      INTEGER NOT NULL,              -- 1 again, 2 hard, 3 good, 4 easy; 0 for manual/reschedule
  ivl       INTEGER NOT NULL,
  last_ivl  INTEGER NOT NULL,
  factor    INTEGER NOT NULL,              -- SM-2 ease permille; under FSRS Anki stores round(difficulty*100)... see fsrs-spec §5.2
  time      INTEGER NOT NULL,              -- ms
  type      INTEGER NOT NULL               -- 0 learn, 1 review, 2 relearn, 3 filtered/early, 4 manual, 5 rescheduled
);
CREATE INDEX IF NOT EXISTS ix_revlog_cid ON revlog (cid);

CREATE TABLE IF NOT EXISTS tags (
  tag        TEXT PRIMARY KEY COLLATE NOCASE,
  collapsed  INTEGER NOT NULL DEFAULT 0,
  usn        INTEGER NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS graves (                -- deletions, as Anki keeps them
  oid   INTEGER NOT NULL,
  type  INTEGER NOT NULL,                  -- 0 card, 1 note, 2 deck
  usn   INTEGER NOT NULL DEFAULT -1
);

CREATE TABLE IF NOT EXISTS media (
  name   TEXT PRIMARY KEY,                 -- filename as referenced by fields (Anki-normalised)
  sha1   TEXT NOT NULL,
  size   INTEGER NOT NULL,
  mtime  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS saved_searches (
  name   TEXT PRIMARY KEY,
  query  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS undo_log (              -- one row per undoable step; payload is the inverse operation
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  at       INTEGER NOT NULL,               -- ms
  label    TEXT NOT NULL,
  payload  TEXT NOT NULL                   -- JSON UndoPayload
);

CREATE TABLE IF NOT EXISTS optimise_log (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  config_id  INTEGER NOT NULL,
  at         INTEGER NOT NULL,             -- seconds
  reviews    INTEGER NOT NULL,
  before     TEXT NOT NULL,                -- JSON {params, logLoss, rmse}
  after      TEXT NOT NULL,
  adopted    INTEGER NOT NULL
);

-- Full-text search over HTML-stripped fields and tags. Trigram tokenizer gives Anki-like
-- substring matching for Latin, Arabic and CJK alike. Queries shorter than 3 characters
-- fall back to LIKE on notes.plain.
CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
  plain, tags, content='notes', content_rowid='id', tokenize='trigram case_sensitive 0'
);
CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
  INSERT INTO notes_fts(rowid, plain, tags) VALUES (new.id, new.plain, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, plain, tags) VALUES ('delete', old.id, old.plain, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, plain, tags) VALUES ('delete', old.id, old.plain, old.tags);
  INSERT INTO notes_fts(rowid, plain, tags) VALUES (new.id, new.plain, new.tags);
END;
