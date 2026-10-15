# Import, export and backups

Every path that moves data in or out reads everything first, writes in one transaction, and keeps a copy before anything is replaced.

## Import

Loci imports Anki `.apkg` (decks) and `.colpkg` (a whole collection) in any package layout, and its own full-backup `.zip`.

Two destinations:

- **Merge** (*Add to my collection*) adds the package to what you have. Notes are matched by their Anki `guid`. A note that is new is added; a note you already have is updated or skipped according to the conflict rule, which by default is *newest edit wins*: the incoming note replaces yours only if its modification time is newer. The other rules are *Keep mine* and *Use the imported version*. Importing the same deck twice does not duplicate it.
- **Replace** (*Replace my collection*) swaps the whole collection for the package. The current collection is saved as a `pre-restore` backup first.

**All or nothing.** The whole package is read and validated before anything is written. If any part cannot be read (a truncated zip, a damaged database, a media file whose checksum does not match, a package from a newer Anki than Loci understands), the import stops with an error that says why, and the collection is untouched. The write itself is one transaction: media files first, then notes, cards and review history together.

**What the merge does with scheduling.** Review due days are shifted by the difference between the two collections' creation days, so every card falls due on the same calendar day. FSRS state in `cards.data` is kept verbatim. Review-log rows of imported cards are added. Cards in filtered decks go back to their home decks. Media keep Anki's names; a name already taken by a different file gets Anki's `-<sha1>` suffix and the imported notes are rewritten to match. With *Keep scheduling* off, imported cards start as new, as with Anki's "without scheduling" export.

**Undo.** An import of up to 20,000 rows is one undo entry. A larger import into a non-empty collection writes a `pre-restore` backup instead.

## Export

*Export* writes an Anki package of one deck (with its subdecks) or of the whole collection, with or without scheduling and with or without media. The file is the legacy `.apkg` layout that Anki 2.1 through 26.x, AnkiDroid and AnkiMobile import; see [package layouts](data-model.md#package-layouts). Without scheduling, cards go out as new.

*Download full backup* writes Loci's own `.zip` (below); `.apkg` is for Anki.

## Automatic backups

A backup is `collection.sqlite` serialised from the open database, written under a temporary name and renamed. Backups live in `backups/` inside the data directory, named by their UTC time and kind:

```text
loci-20261011-041106-auto.sqlite
loci-20261011-093000-manual.sqlite
loci-20261011-101500-pre-restore.sqlite
```

**When they run.**

- On leaving the app (window closed or interface hidden), if anything changed since the last backup.
- *Back up now* (a `manual` backup).
- Before anything destructive: restoring a backup, *Reset collection*, an import in *Replace* mode, restoring a full backup, and a large import into a non-empty collection all write a `pre-restore` backup first.

The engine's `autoBackup` method (also part of `maintenance`) backs up when something changed and the last backup is at least 30 minutes old; the app does not call it by itself.

**Rotation.** Loci keeps every automatic backup from today, then the newest of each previous day up to 12 days, then one per week for 10 more weeks, then one per month for 9 more months: 12 daily, 10 weekly and 9 monthly. Manual and pre-restore backups are never deleted automatically.

## Restore

*Settings › Backups* lists every backup, newest first, with its date, kind, size and card count. Restoring one first verifies it with `PRAGMA quick_check`; a damaged backup is refused and nothing changes. A healthy one replaces the collection, after the current collection has been saved as a `pre-restore` backup, so a restore can be undone. Media files are kept; a restore deletes none of them.

*Restore from a file* takes a full-backup `.zip`, and works the same way.

## The integrity check

Opening runs `PRAGMA integrity_check` and reports problems. *Settings › Check integrity* (or `loci integrity`) goes further: it verifies the full-text index and the collection's consistency, and repairs what is safe to repair. It rebuilds indexes and the search index, recreates the Default deck and preset and any missing parent decks, removes cards whose note or template is gone, moves cards from deleted decks to Default, pads notes with missing fields, generates cards for notes that have none, re-registers tags, and fixes the new-card position counter. It never deletes notes.

## The Loci full-backup format

A full backup is an ordinary zip archive. `backup::export_full_backup` in `crates/loci-core/src/backup/backup.rs` writes these entries:

```text
collection.sqlite      the database, serialised from the open collection (deflated)
media/<name>           every media file, one entry each, under its own name
manifest.json          what this file is (deflated)
```

Media files whose content is already compressed (images such as `.png`, `.jpg`, `.webp`; audio and video such as `.mp3`, `.ogg`, `.mp4`; archives, fonts and PDFs) are stored without deflate; everything else is deflated. Only files directly inside `media/` are included.

`manifest.json` is pretty-printed JSON with eight keys:

| Key | Value |
|---|---|
| `format` | always `"loci-backup"` |
| `version` | `1`, the version of this zip format |
| `schemaVersion` | the collection schema version (currently `1`) |
| `collectionId` | the collection's id from `col_meta` |
| `createdAt` | when the backup was made, in Unix seconds |
| `notes` | number of notes |
| `cards` | number of cards |
| `media` | number of media files |

Its shape, with placeholders for the values:

```text
{
  "format": "loci-backup",
  "version": 1,
  "schemaVersion": 1,
  "collectionId": "<collection id>",
  "createdAt": <unix seconds>,
  "notes": <count>,
  "cards": <count>,
  "media": <count>
}
```

**Reading it back** (`collection/full_backup.rs`). A file is treated as a Loci full backup when it is a zip that contains both `manifest.json` and `collection.sqlite` and the manifest's `format` is `"loci-backup"`; anything else is handed to the Anki package reader. Before anything is replaced, the database image must start with the SQLite header, pass `PRAGMA quick_check`, and contain a `schema_version` in `col_meta`; otherwise the file is refused as corrupt. Media entry names must already be in Anki's normalised form and contain no further `/`. A full backup can only replace a collection, never merge into one. On restore, media files that are missing or differ (by size and SHA-1) are written; identical ones are left alone.

## The data-safety document

`docs/DATA-SAFETY.md`, included as it stands:

<div class="included">

{{#include ../../DATA-SAFETY.md:2:}}

</div>
