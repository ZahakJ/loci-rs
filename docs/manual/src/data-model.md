# The data model and Anki compatibility

Loci's collection is one SQLite file, `collection.sqlite`, whose tables use Anki's column names and Anki's units. That choice makes import a mapping rather than a translation: a review that Anki stored as "interval 14, ease 2500, due on day 812" is stored by Loci as exactly those numbers. This chapter describes the schema, which Anki packages Loci reads and writes, and what survives a round trip.

The authoritative schema is [`crates/loci-core/src/schema.sql`](https://github.com/ZahakJ/loci-rs/blob/main/crates/loci-core/src/schema.sql). What follows is a summary.

## Units

Anki's units are unusual, so they are worth stating once:

| Column | Unit |
|---|---|
| `notes.id`, `cards.id`, `revlog.id` | milliseconds since the Unix epoch (the creation or review time) |
| `mtime` | seconds since the epoch |
| `cards.due` | depends on the queue: a position for new cards, epoch seconds for intraday learning, a day number counted from the collection's creation day for review and day-learning cards |
| `cards.ivl` | days (negative values are seconds, for learning cards) |
| `cards.factor` | SM-2 ease in permille (2500 means 2.5) |
| `revlog.time` | milliseconds spent answering |
| `revlog.ivl`, `revlog.last_ivl` | days (negative = seconds) |

The collection's creation day, `crt`, is the zero point for review due days. It lives in `col_meta`, together with the day rollover hour (4 by default), the schema version and a collection id.

## The main tables

```text
notes   (id ms, guid, ntid, mtime s, usn, tags, flds 0x1f-separated, sfld, csum, plain)
cards   (id ms, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps, lapses,
         left, odue, odid, flags, data JSON)
revlog  (id ms, cid, usn, ease, ivl, last_ivl, factor, time ms, type)
```

- **Notes** hold the fields, joined by the unit separator U+001F as Anki does, and tags as a space-padded string (`' tag1 tag2 '`). `sfld` is the HTML-stripped sort field; `csum` is the first 8 hex digits of the SHA-1 of the stripped first field, used for duplicate checks; `plain` is all fields stripped of HTML, which feeds full-text search.
- **Cards** carry the scheduling columns. `type` is 0 new, 1 learning, 2 review, 3 relearning. `queue` adds the states a card can be parked in: −1 suspended, −2 buried as a sibling, −3 buried by hand, plus 3 for day-learning and 4 for preview. The low three bits of `flags` are the flag colour.
- **Revlog** has one row per answer. `ease` is the button (1 Again to 4 Easy; 0 for a manual reschedule), and `type` is 0 learn, 1 review, 2 relearn, 3 filtered/early, 4 manual, 5 rescheduled.

Note types, fields, templates, decks and deck presets are normalised tables (`notetypes`, `fields`, `templates`, `decks`, `deck_configs`) with typed JSON configuration. Media bodies are plain files in the `media/` directory, indexed by a `media(name, sha1, size, mtime)` table.

Other tables: `tags`, `saved_searches`, `undo_log` (one row per undoable step, holding the inverse operation), `optimise_log` (every optimiser run, with parameters and metrics before and after), and `graves` (deleted ids, kept for a future sync).

**Search** uses an FTS5 table, `notes_fts`, over `notes.plain` and the tags, with the trigram tokenizer. Trigrams give Anki-like substring matching for Latin, Arabic and CJK text alike; queries shorter than three characters fall back to `LIKE` on `notes.plain`.

**Sync hooks.** Every syncable row carries `usn` (update sequence number; −1 means changed locally) and `mtime`, and deletions are recorded in `graves`. Nothing syncs yet; the [sync plan](sync.md) shows how these columns will be used.

## FSRS state lives where Anki puts it

FSRS memory state is stored in `cards.data` as JSON, with the keys Anki itself writes: `s` (stability in days), `d` (difficulty, 1 to 10), `dr` (desired retention in effect), `decay`, and `pos` (the original new-card position). Because the keys are Anki's, a card that goes Loci → Anki → Loci keeps both its FSRS state and its SM-2 columns.

## Package layouts

Anki has written three package layouts over the years. A zip entry called `meta` (a small protobuf) says which one a file uses; when it is absent, the name of the collection file inside decides.

| Layout | Collection file | Schema | Compression | Media map |
|---|---|---|---|---|
| `LEGACY_1` (no `meta`) | `collection.anki2` | 11 | zip deflate | JSON |
| `LEGACY_2` (`meta` = `08 02`, or absent with `collection.anki21`) | `collection.anki21` | 11 | zip deflate | JSON |
| `LATEST` (`meta` = `08 03`) | `collection.anki21b` | 18 | zstd | zstd-compressed protobuf |

**Loci reads all three**, as `.apkg` (a deck) or `.colpkg` (a whole collection). The zstd layout is decoded with a pure-Rust decoder, and the protobuf fields Loci needs are decoded by field number.

**Loci writes one: `LEGACY_2`.** An export is a schema-11 `collection.anki21`, `meta` = `08 02`, a JSON media map with numbered media files, `"schedVer": 2` in the collection config, and the small dummy `collection.anki2` that every Anki package carries for very old clients. Every Anki version from 2.1 to 26.x imports this layout, and so does AnkiDroid. Loci never writes `.anki21b`; reading it is enough, and writing the older layout is what keeps every Anki version able to open the result.

## What round-trips

Exporting from Loci and importing the result into a second Loci collection is lossless: notes, every scheduling column of every card, FSRS data, the review log and media bytes come back identical. The test suite checks exactly this (import a fixture, export, import again, compare counts, due days and revlog).

Going out to Anki and back works the same way, with two points to know:

- **The day numbering.** If the collection began with an import from Anki, exports carry that Anki collection's creation time (`crt`) and renumber due days to it, so a package returning to Anki has the day numbering it left with. Otherwise the package carries Loci's own `crt`. Either way the day numbers and `crt` agree, so any importer lands each card on the same date.
- **Deck presets** are written as Anki deck options. A preset that came from Anki keeps its original Anki JSON, so options Loci does not model are not lost.

What does not round-trip:

- **Filtered decks.** Loci has no persistent filtered decks. Importing one returns its cards to their home decks. The `odid` and `odue` columns stay in the schema, and persistent filtered decks would reuse them. Use *Custom session* for study-ahead, forgotten cards, previews or an arbitrary search.
- **The `.anki21b` layout.** A collection imported from a `LATEST` package goes back out as `LEGACY_2`. The content is the same; the container is the older one.

## The migration path from the TypeScript Loci

The first Loci was a TypeScript web app that kept its collection in the browser. Its *full backup* is a zip containing `collection.sqlite`, the `media/` files and a `manifest.json`; the schema is the same version 1 schema this edition uses. To move over, download a full backup in the web app and import it in the Rust app with *Replace my collection*. A full backup can only replace a collection, never merge into one. The exact zip format is in [Import, export and backups](import-export-backups.md#the-loci-full-backup-format).
