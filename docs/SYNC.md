# Sync

No account, no server. Each device writes a **sync file** with its view of the collection;
other devices merge it. Engine: `crates/loci-core/src/sync/`. Pure logic, no file or network
I/O (it compiles for wasm32); shells move the bytes.

## Transports

- **Sync folder** (desktop app, browser with the File System Access API). Any folder another
  tool keeps in step (Syncthing, Dropbox, Drive, a USB stick). Layout:
  `<folder>/loci-sync/<collection-id>/<device-id>.loci-sync` and `…/<collection-id>/media/<sha1>`.
  A device writes only its own file, reads every other `.loci-sync` file (in every collection
  directory), merges them, rewrites its own, copies media it has that the folder lacks and takes
  media it lacks. Runs on open, on close, after a study session and on *Sync now*. The
  orchestration is `apps/loci/src/ui/sync/folderSync.ts`; folder I/O is Tauri commands
  (`sync_folder_*`, desktop) or a `FileSystemDirectoryHandle` (browser).
- **File hand-off** (everywhere, including Android and phones): *Save sync file* (share or
  download) and *Open sync file* (merge). Carries every row and the media files the other side
  may lack.
- LAN pairing is not built: the two transports above cover phone ↔ desktop.

## Stamps

- Device id: 16 random hex digits in `sync_meta`, created the first time sync is used.
- Stamp = `(hlc, device)`, hlc = `ms << 16 | counter` (hybrid logical clock: never goes back,
  jumps past every stamp seen in a merge). Total order: hlc, then device id.
- Change capture: triggers on every synced table add `(kind, key)` to `sync_dirty`. After every
  engine write (`rpc::Engine::dispatch`), before an export and before a merge, the stamping pass
  compares each dirty object's payload hash with `sync_rows.hash` and issues a stamp only when
  it changed (so `decks.today`, `usn`, backups and other local columns never cause traffic).
  Every write path is covered, including undo, imports, integrity repairs and optimisation.
- First use: a full scan stamps everything. A collection with no notes and no reviews is
  stamped at time zero, so a new device adopts the other device's decks, presets and settings.
- Restore, reset and replace-import (`Collection::replace_database`, `reset_collection`): the
  collection gets a new device id (the file may be another device's copy) and every object is
  restamped (reviews keep theirs), so the restored state wins over older changes elsewhere. The
  old id is listed in `formerDevices`; folder sync deletes its file.

Side tables (the Anki-compatible schema is unchanged): `sync_meta`, `sync_rows(tbl, key, hlc,
dev, hash, deleted)`, `sync_vector(dev, hlc)`, `sync_peers(dev, vector, wants, seen)`,
`sync_dirty`, `sync_revlog_alias`.

## Synced objects

| kind | key | payload | not synced |
|---|---|---|---|
| notetypes | id | row + fields + templates (one object) | usn |
| presets | id | id, name, config, mtime | usn |
| decks | id | id, name, kind, config_id, description, collapsed, config, mtime | today, usn |
| config | setting | rollover hour, learn-ahead, card font/size/alignment, typed-answer rules | theme, language, keyboard hints, motion |
| tags | lowercase tag | tag, collapsed | usn |
| notes | id | id, guid, ntid, mtime, tags, flds, sfld, csum, plain | usn |
| cards | id | every column; day-based `due` as an epoch day | usn |
| revlog | `id:cid` | id, cid, ease, ivl, last_ivl, factor, time, type | usn |
| media | name | name, sha1, size | mtime |

Card due days are converted to and from epoch days (`due ± round(crt/86400)`), so collections
created on different days agree.

## Bundle

A zip: `sync.json` (deflated) and `media/<sha1>` (stored). `sync.json`:
`{format: 1, collection, device, created, vector, base, wants, rows: [[kind, key, hlc, dev,
deleted, payload|null], …]}`. A newer `format` is refused; unknown kinds are skipped.

- `vector`: per device, the highest hlc whose changes the sender has seen (its own: its clock).
- `base`: rows whose stamp it covers were left out. Hand-off files use an empty base (all rows).
  Folder files use the meet of the vectors of the devices with a file in the folder, falling
  back to empty when one of them is unknown.
- `wants`: SHA-1s the sender has rows for but no file. Hand-off files carry the media whose rows
  are not covered by the meet of every known peer, plus everything a peer wants.

## Merge

One transaction, after an automatic backup (when something will change and this device changed
something since its newest backup).

1. Per object, the higher stamp wins. Tombstones (`deleted = 1`) are objects too, so delete vs
   edit is decided by stamp. Reviews are a union: a deletion (only *undo* deletes a review) beats
   the review, otherwise the stamp decides. A review whose id is taken here by another card's
   review is stored under the next free id (`sync_revlog_alias`).
2. A card's schedule is the winning card row; reviews are not replayed.
3. Note types are one object: the newer one wins; cards are regenerated by the repairs.
4. Media: the newer stamp owns the name. When this device's file loses, it is kept as
   `name-<sha1>.ext` (Anki's rule) and synced as a new file; fields are not rewritten.
5. Deck names stay unique: within a name, the smallest id keeps it, the others become
   `Name (2)`, `Name (3)`….
6. Repairs, only when the bundle is complete for this device (its vector covers `base`): note
   types with the same name, kind, fields and templates become one (notes move to the one with
   most notes); notes without a note type and cards without a note are deleted; cards in a
   missing deck go to Default; decks with a missing preset get preset 1; notes get the cards
   their note type calls for (id `nid + ord` or the next free, position and deck from the merged
   state); duplicate cards of one template keep the one with most reviews; tags are registered.
7. Repairs and renames are this device's changes and get its stamps. Then the vector absorbs the
   sender's (complete bundles only), the peer's header is recorded, the collection id becomes
   the sender's on this device's first sync and the smaller of the two after that, the undo
   stack is cleared if anything changed, and a running study session re-syncs.

Properties: without repairs the merge is a join (commutative, associative, idempotent; merging a
bundle twice changes nothing). Repairs depend only on the merged state, so every device makes the
same ones; when three devices race on one note, they agree after one more exchange.

## API

`syncStatus`, `setSyncFolder(folder|null)`, `exportSyncBundle({folder?, peers?}) → bytes`,
`mergeSyncBundle(bytes) → {fromDevice, rowsIn, conflictsResolved, reviewsAdded, mediaAdded,
mediaNeeded, complete}`, `syncMediaState → {have, need}`, `addSyncMedia(sha1, bytes)`.

## Tests

`crates/loci-core/src/sync/tests.rs` (collections compared by a canonical dump of every synced
object): commutativity, idempotence and associativity over 30 seeded three-device workloads;
convergence and review preservation over 60 seeded workloads with deletes, undo and deck-name
races; delete vs edit both ways and on a clock tie; engine-time stamping; media name conflicts;
hand-off and folder media; bundle bytes round trip; folder deltas and catch-up; backup, undo and
reports; restore; collections created on different days; adoption by a new device; deck names.
`apps/loci/src/ui/sync/*.test.ts`: folder orchestration over an in-memory folder.
