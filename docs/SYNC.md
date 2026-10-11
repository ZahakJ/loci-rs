# Sync — design sketch (deferred)

Loci ships local-only. Sync is designed in from the start so the first version does
not paint us into a corner. This document is the plan; nothing here is implemented.

## Goals

- No account required to use the app. Sync is opt-in and works with a self-hosted
  relay *or* a user-owned folder (any WebDAV/S3/Drive via the File System Access API).
- Never ask "upload or download?". Two devices that both studied offline merge
  without losing a review.
- Media sync is part of the same protocol, not a separate opaque step (Anki C21).

## Why not CRDTs or a generic sync engine

Spaced-repetition data has a very specific shape: the **review log is an append-only
set** and almost all scheduling state is a **pure function of that log** plus
parameters. The only real conflicts are edits to the same note field on two devices.
A field-level last-writer-wins with a tombstone table covers that, and the revlog
union covers scheduling. cr-sqlite / PowerSync / Electric (research/stack.md §13)
would add megabytes and a server dependency for a problem that is mostly set union.

## Data model hooks already present

- Every row has `usn` (update sequence number, Anki semantics: -1 = modified locally,
  else the server sequence that last saw it) and `mtime`.
- `graves(oid, type, usn)` records deletions.
- `revlog.id` is a millisecond timestamp per device; collisions are resolved by
  `(id, cid)`.
- `col_meta.collection_id` identifies the collection; a device id is added at sync
  enrolment.

## Protocol (per sync)

1. **Handshake**: client sends `collection_id`, `device_id`, `last_usn`, schema
   version. Server replies with its `usn` and whether a full upload/download is required
   (only when schema versions differ or the collection ids differ).
2. **Pull changes** since `last_usn`: rows from notetypes, fields, templates, decks,
   deck_configs, notes, cards, tags, graves, media manifest, with `usn > last_usn`.
3. **Merge locally**, in one transaction:
   - *Notes*: if both sides changed since the common ancestor, merge **per field**:
     the newer `mtime` wins per field, tags are unioned minus tombstoned tags. Loser's
     text is kept in `undo_log` as "sync conflict" so nothing is silently lost.
   - *Cards*: scheduling columns are **recomputed from the merged revlog** when both
     sides reviewed the same card (replay the scheduler over the union of entries in
     time order). Otherwise take the side that has reviews.
   - *Revlog*: set union by `(id, cid)`.
   - *Decks, note types, presets*: newer `mtime` wins per row; renames propagate;
     a note type schema change (fields/templates added/removed) requires the other
     side to regenerate cards, which the merge does.
   - *Graves*: a delete beats an edit unless the edit is newer than the delete, in
     which case the row is resurrected (and the user is told).
4. **Push** local rows with `usn = -1`, receive assigned `usn`s, set `last_usn`.
5. **Media**: manifest of `(name, sha1, size)`; content-addressed upload of missing
   blobs; name collisions with different sha1 are renamed `name-<sha1[:8]>.ext` and
   fields rewritten, matching Anki's behaviour.

Everything above is idempotent; a sync interrupted at any step can be retried.

## Transports

- **Loci relay**: a small Hono service (same stack as the owner's other services)
  storing per-collection change logs in SQLite, authenticated by a device key pair
  (Ed25519) issued when a QR code pairs devices. No email, no password.
- **Folder sync**: the same change log written as dated JSONL files into a folder the
  user owns (WebDAV, Syncthing, Drive). Slower, zero infrastructure.
- **Anki compatibility**: not planned. AnkiWeb's protocol is not public and Anki
  itself is the interoperability layer via `.apkg`.

## UI

Settings → Sync: pair a device by QR, show last sync, show a conflict list (rare) with
both versions and a one-tap choice. Sync runs on app open, on background, and after
a study session; never blocks study.
