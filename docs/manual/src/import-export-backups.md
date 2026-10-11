# Import, export and backups

Years of review history are irreplaceable, so every path that moves data in or out of Loci is built to fail safely: read everything first, write in one transaction, keep a copy before anything is replaced. This chapter describes the rules, then the full-backup file format, then reproduces the data-safety document that lists every guarantee and the test that proves it.

## Import

Loci imports four kinds of file: an Anki `.apkg` (a deck or several), an Anki `.colpkg` (a whole collection), in any of the three package layouts, and Loci's own full-backup `.zip`.

**Import never overwrites by surprise.** There are two destinations:

- **Merge** (*Add to my collection*) adds the package to what you have. Notes are matched by their Anki `guid`. A note that is new is added; a note you already have is updated or skipped according to the conflict rule, which by default is *newest edit wins*: the incoming note replaces yours only if its modification time is newer. The other rules are *Keep mine* and *Use the imported version*. Importing the same deck twice does not duplicate it.
- **Replace** (*Replace my collection*) swaps the whole collection for the package. The current collection is saved as a `pre-restore` backup first.

**Import refuses rather than half-imports.** The whole package is read and validated before anything is written. If any part cannot be read (a truncated zip, a damaged database, a media file whose checksum does not match, a package from a newer Anki than Loci understands), the import stops with an error that says why, and the collection is untouched. The write itself is one transaction: media files first, then notes, cards and review history together.

**What the merge does with scheduling.** Review due days are shifted by the difference between the two collections' creation days, so every card falls due on the same calendar day. FSRS state in `cards.data` is kept verbatim. Review-log rows of imported cards are added. Cards in filtered decks go back to their home decks. Media keep Anki's names; a name already taken by a different file gets Anki's `-<sha1>` suffix and the imported notes are rewritten to match. With *Keep scheduling* off, imported cards start as new, as with Anki's "without scheduling" export.

**Undo.** An import of up to 20,000 rows is one undo entry. A larger import into a non-empty collection is too big for the undo log, so Loci writes a `pre-restore` backup before it instead.

## Export

*Export* writes an Anki package of one deck (with its subdecks) or of the whole collection, with or without scheduling and with or without media. The file is the legacy `.apkg` layout that Anki 2.1 through 26.x, AnkiDroid and AnkiMobile import; the [data model chapter](data-model.md#package-layouts) explains why that layout. Without scheduling, cards go out as new cards, which is what you want when sharing a deck.

*Download full backup* writes Loci's own `.zip` (below). Use it to keep a complete copy; use `.apkg` to go to Anki.

## Automatic backups

A backup is a copy of `collection.sqlite`, serialised from the open database, written under a temporary name and then renamed, so a backup is never half-written. Backups live in `backups/` inside the data directory, named by their UTC time and kind:

```text
loci-20261011-041106-auto.sqlite
loci-20261011-093000-manual.sqlite
loci-20261011-101500-pre-restore.sqlite
```

**When they run.**

- When you leave the app: when its window closes, and when the interface is hidden (which on Android is the last reliable moment), Loci writes an automatic backup if anything changed since the last one.
- When you press *Back up now* (a `manual` backup).
- Before anything destructive: restoring a backup, *Reset collection*, an import in *Replace* mode, restoring a full backup, and a large import into a non-empty collection all write a `pre-restore` backup first.

The engine also implements the 30-minute rule from the design: an automatic backup whenever something changed and the last backup is at least 30 minutes old. It is exposed as the `autoBackup` method and as part of the `maintenance` pass. In v0.1.0 the interface does not call either method on its own, so the automatic backups you see come from leaving the app.

**Rotation.** Loci keeps every automatic backup from today, then the newest of each previous day up to 12 days, then one per week for 10 more weeks, then one per month for 9 more months: 12 daily, 10 weekly and 9 monthly. Manual and pre-restore backups are never deleted automatically.

## Restore

*Settings › Backups* lists every backup, newest first, with its date, kind, size and card count. Restoring one first verifies it with `PRAGMA quick_check`; a damaged backup is refused and nothing changes. A healthy one replaces the collection, after the current collection has been saved as a `pre-restore` backup, so a restore can itself be undone by restoring that copy. Media files are kept; a restore deletes none of them.

*Restore from a file* takes a full-backup `.zip`, and works the same way.

## The integrity check

Every time the collection opens, Loci runs `PRAGMA integrity_check` and reports problems instead of silently continuing. *Settings › Check integrity* (or `loci integrity`) goes further: it verifies the full-text index and the collection's consistency, and repairs what is safe to repair. It rebuilds indexes and the search index, recreates the Default deck and preset and any missing parent decks, removes cards whose note or template is gone, moves cards from deleted decks to Default, pads notes with missing fields, generates cards for notes that have none, re-registers tags, and fixes the new-card position counter. It never deletes notes.

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

This is the same format the TypeScript Loci writes, which is why its backups restore here unchanged.

## The data-safety document

The repository keeps one document, `docs/DATA-SAFETY.md`, listing every guarantee, what to do when something goes wrong, and the test behind each claim. It is reproduced here so there is one source of truth. It was written for the TypeScript edition; its first line says how to read it for this one. The test paths it names are the TypeScript suite's, which the Rust tests port, and its command-line examples have Rust equivalents in [The CLI](cli.md).

<div class="included">

{{#include ../../DATA-SAFETY.md::1}}

{{#include ../../DATA-SAFETY.md:4:}}

</div>
