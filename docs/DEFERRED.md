# Deferred features

Not in the current release. Each has a sketch.

## 1. iOS
Needs a Mac with Xcode, an Apple developer account and a device. The Tauri project
generates the iOS scaffold (`pnpm tauri ios init`); what remains is signing, a
`save_bytes` path for the Files app, and testing on a device.

## 2. Assisted card creation
Turn source material into cards.
- **Input**: pasted text, a PDF page range, a URL, or a photo of a page.
- **Pipeline**: chunk → extract atomic facts → draft cards in the user's note types
  (Basic, Cloze, typed), one fact per card → review each draft in the Add sheet
  (accept, edit, skip) → flag duplicates by first-field checksum and FTS similarity.
- **Model**: a user-supplied API key stored locally, never synced. Requests are sent
  only on an explicit click; prompts live in the source tree.
- Study and editing never depend on it.

## 3. Shared-deck library
`.apkg` import already covers Anki's shared decks. A library would add per-card error
reports, opt-in retention aggregates and curated starter decks. Needs a server.

## 4. Plugin API
Plugins as sandboxed modules with a message-passing API mirroring `CollectionApi`
(read by default, write by permission) and declarative UI slots (study toolbar,
card-info panel, stats panel). An AnkiConnect-compatible local HTTP shim for Yomitan
and mining tools.

## 5. Change note type
Map fields and templates between note types, with a preview. `saveNotetype` already
regenerates cards; the mapping UI is missing.

## 6. Persistent filtered decks
Imported filtered decks return their cards home; *Custom session* covers study
ahead, forgotten cards, preview and arbitrary searches. Persistent filtered decks
would reuse the `odid`/`odue` columns.

## 7. FSRS-7
fsrs-rs defaults to FSRS-7 (34 parameters); Anki 26.09 ships FSRS-6. The forgetting
curve and `step` sit behind an interface, so FSRS-7 is a second implementation and a
parameter-count switch.

## 8. Editor
Tables, LaTeX environment snippets, drawing on images, recording audio.
