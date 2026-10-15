# Data safety

How Loci protects a collection, what to do when something goes wrong, and the tests behind each
guarantee.

## Where the data lives

The platform app-data directory (app-internal storage on Android) holds `collection.sqlite`,
`media/` and `backups/`. The CLI uses the same layout under `--dir`. There is no server.

The browser build keeps `collection.sqlite` and `blobs.sqlite` (media and backups as rows) in the
site's origin-private file system. It holds the Web Lock `loci-rs-collection`, so only one tab
opens the collection, and asks the browser to keep its data (`navigator.storage.persist()`) on
the first write. Clearing the site's data deletes the collection: keep a full backup elsewhere.

## Guarantees

### Every change is one transaction
Each write goes through `Db::transaction` (`BEGIN IMMEDIATE … COMMIT`, rolled back on any error;
nested calls join the outer transaction). An operation's undo entry is written in the same
transaction. The journal mode is `DELETE`: after a crash mid-write SQLite rolls the incomplete
transaction back on the next open.

Imports read and validate the whole package before writing; media files go first, then notes,
cards and review history in one transaction.

### Integrity check on open
Opening runs `PRAGMA integrity_check` and reports problems instead of continuing silently.
*Check database* also verifies the full-text index and repairs what is safe to repair: rebuilds
indexes and the search index, recreates the Default deck and preset and missing parent decks,
removes cards whose note or template is gone, moves cards from deleted decks to Default, pads
notes with missing fields, generates cards for notes without any, re-registers tags and fixes the
new-card position counter. It never deletes notes.

### Automatic backups
- **When:** on closing the window or hiding the app, if anything changed since the last backup.
  The engine's `autoBackup` applies a 30-minute rule for callers that want periodic backups.
- **How:** the database is serialised and written to a temporary file, then renamed.
- **Rotation:** every automatic backup from today, then the newest of each previous day up to 12
  days, one per week for 10 more weeks, one per month for 9 more months. Manual and
  `pre-restore` backups are never deleted automatically.
- **Before anything destructive** (restoring a backup, *Reset collection*, a *Replace* import,
  restoring a full backup, an import of more than 20,000 rows into a non-empty collection): a
  `pre-restore` backup.

### Undo
The last 50 operations: answers, edits, deletions, bulk actions, catch-up plans, imports (up to
20,000 rows), the sample deck, extending today's limits, and an *Optimise* that adopted new
parameters. Automatic optimisation is not put on the stack. Undo restores the exact rows changed.

### Sync
Merging another device's sync file (`docs/SYNC.md`) is one transaction, preceded by an automatic
backup when it will change something not yet backed up. A merge never drops a review; only *undo*
on the device that made it does. A merge that changed anything clears the undo stack. Restoring a
backup makes the restored state the newest on every device.

### Export
- **`.apkg`:** a deck with its subdecks or the whole collection, with or without scheduling and
  media, in the legacy layout that Anki 2.1 through 26.x and AnkiDroid import. When the collection
  began with an Anki import, exports carry that collection's `crt` and renumber due days to it.
- **Full backup (`.zip`):** `collection.sqlite`, every media file under `media/`, and
  `manifest.json`. Restore with *Import* in *Replace* mode.

## When something goes wrong

| Situation | What happens | What to do |
|---|---|---|
| The app closed or crashed during a write | SQLite rolls back the unfinished transaction on the next open. | Nothing. At most the last answer is missing. |
| A wrong bulk edit, catch-up plan or import | It is one undo entry. | Undo. For a large import, restore its `pre-restore` backup. |
| Opening reports database problems | The collection still opens; nothing is repaired silently. | *Settings › Backups › Check database*. If problems remain, restore the newest backup older than the problem. |
| The database file is unreadable | Opening fails with `corrupt`. | Restore a backup (each is checked with `PRAGMA quick_check` first) or a full backup in *Replace* mode. |
| A backup is damaged | Restoring it is refused; nothing changes. | Pick an older one. |
| Media files are missing | Cards show without them; *Check media* lists missing and unused files. | Re-import the original package; identical files are deduplicated. |
| Browser storage was cleared | The browser build starts empty. | Import a full backup in *Replace* mode. |
| Loci is open in another tab | The second tab does not open the collection. | Close one tab and reload. |
| A sync merged something unwanted | A backup was written before the merge. | Restore it; the next sync makes it current everywhere. |
| A package fails to import | Nothing is written; the error says why. | Re-export from Anki; try `.colpkg` if `.apkg` fails. |

From the command line:

```sh
loci integrity --dir <dir>          # check and repair
loci backup --dir <dir>             # manual backup into <dir>/backups
loci export out.apkg --dir <dir>
```

## Tests

| Guarantee | Test |
|---|---|
| Backup → corrupt → open reports failure → restore → equal counts | `backup::backup` `backup_corrupt_open_restore_counts_equal`, `tests/integration/stats_export.rs` `detects_the_damage_and_restores_equal_counts` |
| Full backup restores collection and media; merging one is refused | `stats_export.rs` `full_backup_replaces_the_collection_and_media_refuses_to_merge` |
| Export → import into a second collection is lossless | `stats_export.rs` `export_then_re_import_into_a_second_collection_is_lossless` |
| All package layouts and `.colpkg` import to identical collections | `tests/integration/import.rs` |
| Broken packages are rejected before anything is written | `import.rs` `rejects_a_broken_package_cleanly`, `anki::tests_fixtures` |
| *Replace* keeps a `pre-restore` backup; imports undo | `import.rs` |
| Answers, catch-up plans and bulk actions undo exactly | `tests/integration/study.rs`, `tests/integration/backlog_browse.rs` |
| Backups after writes, on demand and on leave | `tests/integration/rpc.rs` `backs_up_after_writes_on_demand_and_on_leave` |
| Rotation, listing, restore | `backup::rotation`, `backup::backup` |
| Integrity check and repairs | `backup::integrity` |
| Sync: merge order never matters, reviews are never lost, backup before merge, restore wins | `crates/loci-core/src/sync/tests.rs` |
| Browser build: storage, backups and restore | `crates/loci-web/tests/smoke.mjs`, `apps/loci/tests/web/pwa.spec.ts` |
