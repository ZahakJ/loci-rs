# Deferred features and their design sketches

The first release is a complete vertical slice (DESIGN.md §3). These are the pieces
left out on purpose, each with enough design that a future session can start building.

## 1. Sync between devices
See `docs/SYNC.md`. Hooks (`usn`, `graves`, `mtime`) are already in the schema.

## 2. iOS
The Rust edition ships native Linux, Windows, macOS and Android builds through Tauri 2.
iOS is the one platform left out: it needs a Mac with Xcode and an Apple developer
account to sign, and there is no device to test on. The Tauri project already has the
iOS scaffold (`pnpm tauri ios init` works from the same `src-tauri`); what remains is
signing, a `save_bytes` path for the Files app, and a test pass on a device.

## 3. AI-assisted card creation (C13)
Goal: turn source material into *good* cards, not just many cards.
- **Input**: pasted text, a PDF page range, a URL, or a photo of a page.
- **Pipeline**: chunk → extract atomic facts → draft cards with the user's own note
  types (Basic, Cloze, typed) following the "minimum information principle" → the user
  reviews each draft in the Add sheet with keyboard accept/edit/skip → duplicates
  detected against the collection by first-field checksum and FTS similarity.
- **Model**: the owner's Claude API key stored locally (never synced); requests sent
  from the client; nothing runs without an explicit click. Default model
  `claude-sonnet-5-5` for drafting, with `claude-fable-5-1` optional for difficult
  material. Prompts live in `src/core/ai/prompts/` so they can be audited.
- **Offline guarantee**: the feature is a side door; study and editing never depend on
  it.

## 4. Shared-deck library (C20)
Import from Anki's shared decks already works via `.apkg`. A Loci library would add
quality signals that AnkiWeb lacks: error reports per card, "cards per hour" and true
retention aggregates from users who opt in, and curated "top 300" starter decks per
language. Needs sync infrastructure first.

## 5. Plugin API (C19)
Everything a typical add-on provides is built in. For the long tail, a sandboxed API:
plugins are ES modules loaded in a Worker with a message-passing API mirroring
`CollectionApi` (read access by default, write by permission), plus declarative UI
slots (study toolbar button, card-info panel, stats panel). An AnkiConnect-compatible
local HTTP shim (for Yomitan and mining pipelines) can be a small Node service in
`deploy/` that talks to the same collection file.

## 6. Change note type
Mapping fields and templates between note types with a preview. The data path exists
(`saveNotetype` regenerates cards); the UI for mapping is the work.

## 7. Persistent filtered decks
v1 imports filtered decks by returning cards home, and offers *Custom session*
(study ahead, review forgotten, preview new, arbitrary search) for the common uses.
Persistent filtered decks would reuse `odid/odue` columns already present.

## 8. FSRS-7
fsrs-rs main defaults to FSRS-7 (34 parameters) but Anki 26.09 ships FSRS-6. The
forgetting curve and `step` are behind an interface; adding FSRS-7 means a second
implementation and a parameter-count switch.

## 9. Richer editor features
Tables, LaTeX environment snippets, drawing on images (beyond occlusion), and
recording audio in the editor.
