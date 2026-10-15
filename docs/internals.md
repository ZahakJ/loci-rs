# Loci internals

Intended behaviour and the numbers behind it. The map of the code is the manual's
[architecture chapter](manual/src/architecture.md). Code comments cite its sections as `internals §N`. When
the code and this file disagree, fix one of them.

## 1. Scope

In: import `.apkg`/`.colpkg` (all three package layouts), FSRS-6 and Anki SM-2, the study
screen, add/edit notes (Basic, Basic reversed, Basic typed, Cloze, Image Occlusion, custom),
a browser with Anki search syntax and bulk edit, statistics with insights, catch-up mode,
backups and restore, `.apkg` export, Loci full-backup zip import and export, light and dark
themes, English UI with i18n plumbing, RTL and maths rendering, folder and file sync. Native
builds for Linux, Windows, macOS and Android; a browser build (installable, offline).

Out, with sketches in `docs/DEFERRED.md`: iOS, assisted card creation, a shared-deck
library, a plugin API, change-note-type, persistent filtered decks (import returns filtered
cards home; *Custom session* covers the common uses), FSRS-7.

## 2. Technology

| Part | Choice |
|---|---|
| Engine | Rust library `crates/loci-core`, shared by the desktop app, the Android app and the CLI. |
| Storage | `rusqlite` (`bundled`, FTS5 trigram); Anki's tables map one to one. The only C code in the build. |
| Scheduler | FSRS-6 ported by hand (no `fsrs` crate, which pulls in `burn`); checked against the spec vectors. Optimiser: fsrs-rs's algorithm (Adam on log-loss). |
| Packages | `zip` + `ruzstd` (decode only); a field-number protobuf decoder for `PackageMetadata`, `MediaEntries` and the schema-18 config blobs. |
| Shell | Tauri 2 (`apps/loci`): native windows and an APK around a web UI, since Anki cards are HTML. |
| UI | React 19 + Vite, calling the engine through one generic `rpc` command; `loci serve` speaks the same JSON over local HTTP for development and Playwright. |
| Rendering | KaTeX (+mhchem), DOMPurify + Shadow DOM, `@tanstack/react-virtual`, `@fontsource`. |
| Tests | `cargo test`, vitest for UI helpers, Playwright against `loci serve`. |

The engine runs in-process. In the browser build, `crates/loci-web` compiles it to WebAssembly
and a dedicated worker runs it (`apps/loci/src/bridge/wasm.ts`); SQLite uses sqlite-wasm-rs with
the OPFS SyncAccessHandle-pool VFS, the blob store is a table in `blobs.sqlite`, and a service
worker precaches the build.

## 3. Architecture

```
crates/loci-core/src/
  types.rs         domain types (serde camelCase; the JSON matches the frontend's types.ts)
  error.rs         LociError { code, message, detail }
  db.rs blobs.rs   rusqlite wrapper (transaction, serialize); blob store (media/, backups/)
  schema.sql       collection schema v1
  collection/      Collection: lifecycle, settings, decks, presets, notetypes, notes, cards, media,
                   backups + study, render, backlog, browse, import, export, optimise, full_backup
  scheduler/       fsrs, sm2, states, steps, queue, fuzz, convert, items, optimizer
  anki/            zip, proto, read, write, schema11, deck_config, media_names, sqlite_bytes, sample_deck
  template/        parse, render, cloze, occlusion, typed, math, css, generate, check
  search/          parse, sql, describe
  stats/           stats, insights, messages
  backup/          rotation, integrity
  sync/            clock, state, tables, bundle, merge (docs/SYNC.md)
  undo.rs i18n.rs  inverse-operation log; the core's English sentences
  rpc.rs           method table + JSON dispatch
crates/loci-cli/   loci: import, export, due, study, stats, backup, integrity, serve
crates/loci-web/   the engine as WebAssembly for the browser build
apps/loci/
  src/             React UI + bridge/ (tauri.ts, http.ts, wasm.ts) + core/types.ts, api.ts, i18n
  src-tauri/       Tauri app: rpc command, byte commands, progress channels, close → backup, Android config
```

Rules: the UI never touches SQL; it calls `CollectionApi` through the bridge, which mirrors
the interface one to one over `invoke`. Tests run `Collection` directly. Times in the database
follow Anki's units (see `types.rs`). Every write path is a transaction. Any operation that
changes more than one card records an undo entry.

## 4. Data model

`crates/loci-core/src/schema.sql` is authoritative. Column names and units are Anki's, so
import is a mapping: `notes(id ms, guid, ntid, mtime s, usn, tags, flds 0x1f-separated, sfld,
csum, plain)`, `cards(id ms, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps,
lapses, left, odue, odid, flags, data JSON)`, `revlog(id ms, cid, usn, ease, ivl, last_ivl,
factor, time ms, type)`. Note types, fields, templates, decks and presets are normalised
tables with typed JSON config. FTS5 `notes_fts` (trigram) indexes `notes.plain` and tags.
`col_meta` holds `crt`, the rollover hour, the schema version and the collection id. Media are
files in `<data dir>/media/`, indexed by `media(name, sha1, size)`.

FSRS memory state lives in `cards.data` as Anki writes it (`s`, `d`, `dr`, `decay`, `pos`), so a
round trip through Anki keeps both schedulers' state.

## 5. Scheduler

- FSRS-6 (21 parameters) ported from fsrs-rs v6.6.2 (`research/fsrs-spec.md` §1); 17- and
  19-parameter vectors are accepted via §2.3. Anki's fuzz with Anki's PRNG
  (`StdRng::seed_from_u64(card.id + reps)`), learn-ahead limit, day rollover, learning and
  relearning steps, and display orders as in §3, §4, §9.
- SM-2 as Anki's v3 scheduler (§8), selectable per preset.
- Optimiser: outlier filter, pretraining of w0–w3, Adam on log-loss with the spec's bounds and
  L2 prior (§6). Runs on a worker thread with progress. Auto-runs when a preset has ≥400 new
  reviews since its last optimisation. Reports log-loss and RMSE(bins) before and after and keeps
  the better set.
- Every vector in spec §7 passes to the stated precision.
- Default preset "Standard": FSRS on, desired retention 0.90, 20 new/day, 9999 reviews/day with
  the time budget acting instead, learning steps 1m 10m, relearning 10m, maximum interval 36500,
  leech threshold 8. "Relaxed" (0.85, 10 new) and "Intense" (0.93, 40 new) differ only there.
- All three presets bury siblings of new and review cards (interday learning off): one card of a
  note per day, so a reversed card never follows its forward card in a session. Buried cards
  return at the next rollover or with *Unbury*. Imported Anki presets keep their own settings.

## 6. Study screen

Top bar 44px: back, deck name (truncates), three counts in Anki's colours (new blue, learning
orange, due green), overflow menu. A zero count is a quiet `0` in `--faint`, never a dash.

The card is `max-width: 42rem`. On desktop the stage is the flat `--bg` desk and the card is a
`--surface` sheet with radius 16 and `--paper-shadow`, sized by its content (40/48px padding,
32/24 at 720–1023px, at least 7rem), placed at the optical centre (45/55). Phones have no sheet:
text runs edge to edge with 20px gutters; a short card is centred the same way, a tall one starts
at the top and scrolls. Body text Newsreader 1.375rem desktop, 1.25rem phone, line-height 1.45;
Arabic uses Noto Naskh Arabic at 1.15×; `dir=auto` on every field; maths through KaTeX; audio
autoplays per Anki's rules, `r` replays.

Motion: the answer appears below a hairline that draws from the centre (200ms) while it rises
8px and fades in (240ms). Grading slides the card 24px down and out (160ms); the next fades in
(160ms). Reduced motion: opacity only.

The bottom bar keeps one height so buttons never move. Front: one wide *Show answer* (space,
enter). Back: Again · Hard · Good · Easy in fixed positions (1 2 3 4; space = Good), each with
the interval it would give and a 2px top rule in the grade colour. Keycaps show on pointer
devices for the first 12 answers of a session, then fade (200ms); they return while the pointer
is over the bar or once `?` has been opened that session, and never with keyboard hints off.
On phones the grade buttons are 56px tall, 8px apart, 12px gutter, safe-area inset below. Tap
targets ≥ 48px.

Keys: `space/enter` show or Good · `1–4` grade · `z` undo · `e` edit · `s` suspend · `b` bury ·
`f` flag, `ctrl+1..7` flag colours · `m` mark (tag `marked`) · `i` card info · `r` replay audio ·
`t` focus the typed answer · `?` help · `esc` leave.

`i` answers "why this interval?": retrievability now, stability, difficulty, last reviews, one
sentence each. The session ends on a summary: cards, minutes, accuracy, "tomorrow about N", and
one insight if there is one.

Typed answers compare after normalisation (case, diacritics, whitespace; configurable) and show
a character diff in grade colours. Cloze supports nested and overlapping clozes as Anki does,
plus one-by-one reveal per note type. Image occlusion renders Anki's IO shapes as an SVG
overlay, in hide-all-guess-one and hide-one modes.

## 7. Workload

- Time budget instead of card caps: each preset has a daily minutes target (default 20); Home
  shows "about N min today" from recent seconds per card.
- Catch-up (view `Backlog`) triggers when due > 2× the recent daily average or anything is more
  than 3 days overdue. It shows the debt in minutes and offers: spread over N days (cards
  postponed by retrievability, highest R first), just today's share, lower retention for a week,
  or study everything. All undoable.
- Changing new/day or desired retention shows a sentence and a sparkline of predicted
  reviews/day over 90 days, simulated on the actual deck.
- Momentum instead of streaks: days studied in the last 30 and a 12-month heatmap. A missed day
  is shown, never red; nothing resets.

## 8. Add and edit

Add is a side sheet on desktop and a full screen on phones, opened by `a` anywhere. Note type and
deck remember the last choice; `ctrl+enter` adds and returns focus to the first field; sticky
fields keep their value. Fields are `contenteditable` with a small toolbar (bold, italic,
underline, sub/sup, list, link, image, audio, maths, cloze, clear formatting) and an HTML source
toggle; paste strips styles by default. `ctrl+shift+c` wraps the selection in the next cloze
number, `ctrl+alt+shift+c` reuses the current one. Image occlusion: paste or pick an image, draw
rectangles, ellipses or polygons, label, choose hide-all or hide-one; stored in Anki's IO format.
Duplicate check on the first field. Live preview below the fields on desktop.

Note types ship with a stylesheet; card font, size, alignment and colours are set in Settings
without CSS. Templates stay editable with a live preview and inline errors (unknown field,
unclosed section).

## 9. Browse

Three panes on desktop (sidebar · virtual table · editor), two levels on phones. Sidebar: saved
searches, Today (due, added, studied, forgotten), flags, decks, note types, tags. The search box
takes Anki syntax (`docs/SEARCH.md`). The table toggles cards and notes; columns sort; selection
is keyboard-driven (`j/k`, shift, `ctrl+a`). Bulk actions: change deck, add/remove tags,
suspend, bury, flag, reposition new, set due date, forget, delete, find and replace (field
scoped, regex). All undoable.

## 10. Statistics

Each panel is a chart, one sentence of meaning and, where it applies, one action. Panels:
Today; Forecast (30/90 days, with and without new cards); True retention (by period, against
desired retention); Reviews and time per day (12 months); Intervals and stability; Difficulty;
Hourly performance; Answer buttons. The insights strip on Home and Stats comes from rules in
`stats/insights.rs`, e.g. "True retention 83% vs target 90%: optimise parameters".

## 11. Settings

Simple: theme, card font and size, daily time budget, new cards per day, desired retention
(slider with a retention/workload sentence), preset (Relaxed / Standard / Intense), day start.
Advanced (collapsed): every Anki deck option with a one-line explanation and a *Recommended*
mark on the default. FSRS parameters are visible, with Optimise and Evaluate and the last run's
metrics.

## 12. Data safety

Details in `docs/DATA-SAFETY.md`.

- Every mutation is a transaction; `PRAGMA integrity_check` on open, with a repair offer.
- Automatic backups on closing or hiding the window (`autoBackup` also offers a 30-minute
  rule); rotation keeps 12 daily, 10 weekly, 9 monthly in `<data dir>/backups/`. Each is a
  `.sqlite` file written to a temporary name and renamed.
- *Save full backup* writes a Loci zip (collection, media, manifest) that Import restores. Anki
  interoperability goes through *Export collection* (`.apkg`).
- Restore from a listed backup or a file; the current collection is backed up first.
- Import never overwrites: a new collection, or merge by guid with newest wins. The importer
  refuses to proceed if it cannot read every note.

## 13. Import and export

Formats in `research/anki-format.md`. Import accepts `.apkg` (LEGACY_1, LEGACY_2, LATEST) and
`.colpkg`: preview (notes, cards, decks, reviews, media, scheduler state), import with progress,
report. Review `due` is shifted by the `crt` day difference; filtered-deck cards return home;
FSRS state comes from `card.data`; SM-2 history stays in the revlog; media are written to
`media/` with Anki's name normalisation. Export (collection or deck, with or without scheduling)
writes LEGACY_2 with `schedVer: 2`, `meta=08 02`, a JSON media map and the dummy
`collection.anki2`, which Anki 2.1 through 26.x and AnkiDroid import. Round trip import →
export → import keeps counts, due days and revlog identical.

## 14. Accessibility and i18n

Tab order covers everything; focus rings are 2px accent, offset 2; grade and card changes are
announced in a live region; `prefers-reduced-motion` is honoured; contrast is AA in both themes;
tap targets ≥ 44px. All strings go through `t()`; CSS uses logical properties; `dir` on `<html>`
follows the UI locale; card content is always `dir=auto`.

## 15. Visual design

Tokens in `apps/loci/src/ui/design/tokens.css`, colours in OKLCH.

- Light: `--bg oklch(97.5% .006 85)`, `--surface oklch(99% .004 85)`, `--ink oklch(22% .02 260)`,
  `--muted oklch(50% .02 260)`, `--line oklch(88% .01 85)`, `--accent oklch(62% .13 70)` (brass).
- Dark: `--bg oklch(17% .015 260)`, `--surface oklch(21% .015 260)`, `--ink oklch(93% .01 85)`,
  `--muted oklch(68% .015 260)`, `--line oklch(30% .015 260)`, `--accent oklch(78% .12 75)`.
- `--accent-fill` / `--on-accent-fill` fill primary buttons, switches and checked marks: light
  `oklch(54% .14 70)` with white, dark `oklch(55% .12 70)` with `oklch(98% .006 85)`.
  `--accent-strong` is for text, icons and rules.
- Grades (light): again `oklch(62% .17 20)`, hard `oklch(66% .14 65)`, good `oklch(63% .14 150)`,
  easy `oklch(64% .12 230)`; dark +8% lightness. Counts (light): new `oklch(54% .15 250)`, learn
  `oklch(55% .16 50)`, due `oklch(52% .14 150)`. Light danger `oklch(56% .19 25)`. Pill text
  mixes 68% tone into ink. `pnpm contrast` checks every pair (`docs/design/contrast-0.2.md`).
- Type scale 10 / 11 / 12 / 13 / 14 (UI base) / 16 / 20 / 24 / 32, in rem; 10 and 11 only for
  keycaps and chart axes. Card body 22 desktop, 20 phone. `.num` / `.num-lg`: JetBrains Mono,
  tabular, weight 500, for numbers read as data. One Arabic stack, `--font-arabic`.
- Radii 6 / 10 / 16. Spacing on a 4-point grid. Light uses a 1px `--line` border, not shadows;
  dark lifts with `--surface`. `--paper-shadow` on the study card and previews. Motion 120 / 160 /
  180ms, `cubic-bezier(.2,.8,.2,1)`. Icons inline SVG, 1.5px stroke, 20px.
- In dark the cloze colour is `--accent`. Image-occlusion masks: `--line-strong` 85% over surface
  with a 1px `--muted` edge; the asked mask brass 22% with a 2px brass rule.
- Charts: brass is the one series colour; `accent-soft` and `slate` carry secondary series, all
  3:1 or more against the panel. Grade colours only on the answer-buttons chart.
- Browse selection: `--ink-2` 11% over surface with a 2px `--accent-strong` rule at the inline
  start; header 40px.

The theme follows the system by default. The dark theme lowers chroma on large surfaces and
raises it on the accent. The native window follows the same preference (`setTheme`, or `null`
for the OS while the setting is "system"), so the title bar, file dialogs and the macOS menu
bar match.

### Window and menus

- macOS: a native menu bar (Loci, File, Edit, View, Window, Help). The Edit menu makes
  Cmd+C/V/X/Z/A work in a webview; app items (Settings Cmd+,, Add Card Cmd+N, Import, Decks /
  Browse / Statistics Cmd+1–3, Appearance, Manual, Shortcuts) emit their command id to the page.
- Linux and Windows: no menu bar. Every command is in the sidebar, the keys and the command
  palette (Ctrl/Cmd+K, or *Commands* in the sidebar). The OS title bar stays.
- Android: no menu; the phone tab bar.
- The window remembers its size, position and maximised state (`tauri-plugin-window-state`).

## 16. First run

One screen: the wordmark, one sentence, three choices: *Import from Anki* (drop zone), *Try a
sample deck* (30 cards about memory, with cloze, image, maths and Arabic cards), *Start empty*.
After import: a preview, then Home with *Start* focused. The footer links the manual, opened in
the system browser.

## 17. Tests

- `scheduler`: all spec §7 vectors; property tests (interval monotone in stability; R in (0,1];
  SM-2 ease ≥ 1300).
- `anki`: all three fixture layouts; counts match `palace.expected.json`; due days equal Anki's;
  export round trip; malformed packages rejected.
- `template`: sections, cloze nesting, hints, type, special fields, IO shapes, maths conversion.
- `search`: each operator against a fixture collection.
- `backup`: rotation and restore; backup → corrupt → open → restore keeps counts.
- End to end (Playwright, system Chrome, Vite against `loci serve`): first run → import fixture →
  study five cards → add a cloze note → search → stats → export.
- Captures at 1440×900 and 390×844, light and dark, in `docs/captures/`.
- Performance (`docs/manual/src/performance.md`): cold start, 100k-card import, 100k-row
  browse, answer round trip.

## 18. Decisions

1. Data lives in the platform app-data directory (app-internal storage on Android) as real
   files.
2. Export writes the legacy `.apkg` layout; never `.anki21b`.
3. FSRS on by default, SM-2 available, auto-optimisation on.
4. Filtered decks are not persistent objects.
5. Sync needs no account or server: devices exchange sync files through a folder or by hand,
   merged per row by hybrid-logical-clock stamps (`docs/SYNC.md`).
