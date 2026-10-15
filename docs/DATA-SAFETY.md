> Rust edition: where this document says the collection lives in the browser's origin-private file system, it now lives as real files in the platform app-data directory (`collection.sqlite`, `media/`, `backups/`), app-internal storage on Android. Everything else is unchanged.
>
> The browser build (`crates/loci-web`, served at /loci-rs/app/) keeps `collection.sqlite` and `blobs.sqlite` (media and backups as rows, each write one transaction) in the origin-private file system through sqlite-wasm-rs's SyncAccessHandle-pool VFS (pool directory `.loci-rs`), takes the Web Lock `loci-rs-collection`, asks for `navigator.storage.persist()` on the first write, and restores a backup through SQLite's backup API in one transaction instead of a file rename. Its paths are tested by `crates/loci-web/tests/smoke.mjs` (Node, in-memory VFS) and `apps/loci/tests/web/pwa.spec.ts` (Chrome, OPFS).

# Data safety

Years of review history are irreplaceable. This page describes how Loci protects them, what
to do when something goes wrong, and which tests prove each guarantee.

## Where the data lives

| | Browser (PWA) | Node CLI (`scripts/loci.ts`) |
|---|---|---|
| Collection | SQLite file `collection.sqlite` in the origin-private file system (the app-data directory), VFS `app-data directory-sahpool`, pool directory `.loci` | `<dir>/collection.sqlite` (default `./.loci-dev`) |
| Media | the app-data directory directory `loci/media/` | `<dir>/media/` |
| Backups | the app-data directory directory `loci/backups/` | `<dir>/backups/` |

Nothing is sent to a server. The production server (`scripts/serve.mjs`) only serves the app's
static files. When the app-data directory is unavailable (some private modes, very old browsers) the worker runs
the collection in memory and reports `persistent: false`, so the UI can tell the user to export
before closing the tab.

## Guarantees

### Every change is one transaction
Each write goes through `Db.transaction` (`BEGIN IMMEDIATE … COMMIT`, rolled back on any
error; nested calls join the outer transaction). The undo entry of an operation is written
inside the same transaction, so a change and its undo record commit or vanish together. The
journal mode is `DELETE` (rollback journal), which survives a crash or a closed tab mid-write:
SQLite rolls back the incomplete transaction the next time the file is opened.

Imports read and validate the whole package before writing anything to the database; media
files are written first, then notes, cards and review history in a single transaction.

### One owner at a time
The browser client takes a Web Lock (`loci-collection`) before starting the database worker.
A second tab finds the lock taken and shows "Loci is open in another tab" instead of opening
the file, so two writers can never interleave. Inside the worker, calls run strictly one after
another.

### Integrity check on open
`open()` runs `PRAGMA integrity_check`. Problems are reported to the UI (the open report has
`ok: false` and the problem lines) instead of silently continuing. *Check database*
(`checkIntegrity`) also verifies the full-text index and the collection's consistency and repairs
what is safe to repair: rebuilds indexes and the search index, recreates the Default deck and
preset and missing parent decks, removes cards whose note or template is gone, moves cards from
deleted decks to Default, pads notes with missing fields, gives notes without cards their
cards, re-registers tags, fixes the new-card position counter. It never deletes notes.
Opening also refreshes derived data written by older versions: the search text (`notes.plain`)
is rebuilt in chunks of 1,000 notes when `col_meta.plain_version` is older than the current rules
(no note's content or modification time changes), and on the first open of a new collection day
buried cards return to their queues (Anki's day-rollover unbury).

### Automatic backups
- **When:** after any write, if something changed and the last backup is at least 30 minutes old
  (the worker checks after every write; `maintenance()` checks too); and when the app is hidden
  or closed (`installLeaveBackup()` in the client calls `backupOnLeave`, which backs up whenever
  anything changed since the last backup).
- **How:** the database is serialised (`sqlite3_serialize`) and written to a temporary file that
  is then moved over the final name, so a backup is never half-written.
- **Rotation:** every automatic backup from today, then the newest of each of the previous days
  up to 12 days, then one per week for 10 more weeks, then one per month for 9 more months.
  Manual backups (*Back up now*) and *pre-restore* backups are never deleted automatically.
- **Before anything destructive:** restoring a backup, *Reset collection*, importing in
  *Replace* mode, restoring a full-backup file, and importing a large package (more than 20,000
  rows, too big for the undo log) into a non-empty collection all write a `pre-restore` backup
  first.

### Undo
The last 50 operations can be undone (`z` in study, the undo button elsewhere): answers,
edits, deletions, bulk actions, catch-up plans, imports (up to 20,000 rows), the sample deck,
extending today's limits, and an *Optimise* that adopted new parameters (the preset's previous
parameters and every refreshed memory state come back; automatic optimisation is not put on the
stack, so `z` while studying keeps undoing answers). Undo restores the exact rows that were changed.

### Sync
Merging another device's sync file (`docs/SYNC.md`) is one transaction, preceded by an automatic
backup whenever it will change something this device has not backed up yet. Reviews are never
dropped by a merge (only *undo* on the device that made a review removes it). A merge that changed
anything clears the undo stack, so undo never reverts another device's work. Restoring a backup
gives the collection a new device id and makes the restored state the newest everywhere.

### Export, anywhere, any time
- **Anki package (.apkg):** a deck with its subdecks or the whole collection, with or without
  scheduling and media, in the legacy layout every Anki version from 2.1 to 26.x and AnkiDroid
  import. Notes, cards (all scheduling columns and FSRS state), review history and media round-trip
  exactly through Loci. When the collection began with an import from Anki, exports carry that
  Anki collection's creation time (`crt`) with due days renumbered to it, so a package going back
  to Anki has the day numbering it left with; otherwise the package carries Loci's own `crt`.
  Either way day numbers and `crt` always agree, so every importer lands cards on the same date.
- **Full backup (.zip):** `collection.sqlite` (the exact database), every media file under
  `media/`, and `manifest.json`. Restore it with *Import* in *Replace* mode. This file is
  Loci's own format; to move to Anki, export an .apkg.

### Persistent storage
On the first write the client asks the browser to keep the data (`navigator.storage.persist()`).
Chrome grants it silently for engaged sites, Firefox asks, Safari grants it to apps added to the
Home Screen. Browsers can still lose the app-data directory data (clearing site data, cleaning software), which is
why the full-backup download exists: keep a recent copy outside the browser.

## When something goes wrong

| Situation | What happens | What to do |
|---|---|---|
| The tab closed or crashed during a write | SQLite rolls back the unfinished transaction on the next open. | Nothing. At most the last answer is missing. |
| A wrong bulk edit, catch-up plan or import | The change is a single undo entry. | Undo. For a large import, restore the `pre-restore` backup listed in *Backups*. |
| *Open* reports database problems | The collection still opens read-write; nothing is repaired silently. | *Settings › Backups › Check database*. If problems remain, restore the newest backup that is older than the problem (the list shows dates and card counts). Your current file is kept as a `pre-restore` backup first. |
| The database file is unreadable | `open()` fails with code `corrupt`. | Restore a backup (each one is verified with `PRAGMA quick_check` before it replaces anything), or import your last full-backup download in *Replace* mode. |
| A backup itself is damaged | Restoring it is refused (`corrupt`); nothing changes. | Pick an older backup. |
| Browser storage was cleared or evicted | The app starts empty. | *Import* your full-backup download (*Replace*), or your last .apkg export. |
| Loci is open in another tab | The second tab refuses to open the database. | Close the other tab and reload. |
| Media files are missing | Cards show without them; *Check media* lists missing and unused files. | Re-import the original package: identical files are deduplicated, missing ones restored. |
| A sync merged something unwanted | An automatic backup was written before the merge. | *Backups*: restore the one from just before the sync; the next sync makes it the current state everywhere. |
| A package fails to import | Nothing is written; the error says why (not a package, damaged, newer Anki). | Re-export from Anki; try the *.colpkg* if the *.apkg* fails. |

From the command line (development collection):

```sh
node --experimental-strip-types scripts/loci.ts integrity --dir <dir>   # check and repair
node --experimental-strip-types scripts/loci.ts backup --dir <dir>      # manual backup into <dir>/backups
node --experimental-strip-types scripts/loci.ts export out.apkg --dir <dir>
```

## What is tested

| Guarantee | Test |
|---|---|
| Backup → corrupt the live file → open reports failure → restore → identical note/card/review counts → integrity ok | `tests/unit/integration/stats-export.test.ts` ("backup → corrupt → open → restore") |
| Full-backup download restores collection and media; merging one is refused | `tests/unit/integration/stats-export.test.ts` ("full backup download") |
| Export → import into a second collection is lossless (notes, every scheduling column, FSRS data, revlog, media bytes) | `tests/unit/integration/stats-export.test.ts` ("export round trip") |
| All three Anki package layouts and the .colpkg import to identical collections | `tests/unit/integration/import.test.ts` |
| Broken packages are rejected before anything is written | `tests/unit/integration/import.test.ts`, `tests/unit/anki/errors.test.ts` |
| Replace-mode import keeps a pre-restore backup; imports are undoable | `tests/unit/integration/import.test.ts` |
| Answers, catch-up plans, bulk actions and leech actions undo exactly | `tests/unit/integration/study.test.ts`, `tests/unit/integration/backlog-browse.test.ts` |
| Automatic backup after writes (30-minute rule) and on leave, through the worker | `tests/unit/integration/worker.test.ts` |
| Rotation policy, backup listing, pre-restore backups, restore | `tests/unit/collection/backup.test.ts` |
| Integrity check and repairs | `tests/unit/collection/integrity.test.ts` |
| Undo stack mechanics (snapshots, cap) | `tests/unit/collection/undo-cards.test.ts` |
| Sync: merge order never matters, reviews are never lost, backup before merge, restore wins | `crates/loci-core/src/sync/tests.rs` |
| The CLI imports, studies, exports, backs up and checks integrity | `tests/unit/integration/cli.test.ts` |

Not covered by unit tests (browser-only): the the app-data directory paths of the worker (sqlite-wasm's
`app-data directory-sahpool`, the the app-data directory blob store), the Web Lock, `navigator.storage.persist()` and the
`pagehide` backup. The worker protocol itself is tested in Node with sqlite-wasm in memory;
the browser-only paths need the Playwright end-to-end suite.
