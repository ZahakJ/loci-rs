# Loci — design contract (Rust edition)

This file is the contract every implementation agent builds against. It was written
for the TypeScript build (the sibling `recall` repository) and adapted for the Rust
rewrite; every paragraph that changed is marked **[changed in Rust]** and the reason is
given. If code and this file disagree, fix one and say which. Research behind each
decision lives in `recall/research/` (complaints C1–C29 are defined in
`recall/research/anki-complaints.md`).

## 1. Name and identity

**Loci** (LOH-sye). From the *method of loci*: the memory palace of Simonides and
Cicero, the oldest mnemonic technique we know. Short, pronounceable, a good command
(`loci`), free under `ZahakJ`, and no spaced-repetition product uses it. Tagline:
*"A memory palace for everything you learn."*

Visual identity follows the name: architectural calm, warm stone and paper in
light, deep slate at night, a single brass accent. Never neon, never glassmorphism,
never a 3D card flip.

## 2. Product principles

1. **A real replacement.** Years of Anki history import losslessly and are due on
   the same day. Export back to `.apkg` at any time. Nobody is locked in.
2. **Right by default, deep when asked.** A new user studies within sixty seconds of
   first launch and never sees a learning step. A veteran finds every option, with a
   one-line explanation and a "recommended" mark, under *Advanced*.
3. **Data is sacred.** SQLite transactions, rotating automatic backups, restore UI,
   integrity check on open, export anywhere. Offline always.
4. **Humane.** The app never shames. Backlogs are a planning problem it helps solve,
   not a wall of red. Momentum replaces fragile streaks.
5. **One app.** Same code, same look, desktop, laptop and phone. Keyboard-first on
   desktop, thumb-first on phone.

## 3. Scope of this build (vertical slice)

In: import `.apkg`/`.colpkg` (all three package layouts), FSRS-6 + Anki SM-2,
study screen, add/edit notes (Basic, Basic reversed, Basic typed, Cloze, Image
Occlusion, custom), browser with Anki search syntax and bulk edit, statistics with
insights, catch-up mode, backups and restore, export `.apkg`, Loci full-backup zip
import and export (so data from the TypeScript app carries over), light and dark
themes, English UI with i18n plumbing, RTL and maths rendering, a11y baseline.
**[changed in Rust]** Native executables for Linux, Windows and macOS and an Android
APK replace "PWA install"; iOS is out (no device, no signing).

Out (designed, documented in `recall/docs/DEFERRED.md`): sync, iOS,
AI-assisted card creation, shared-deck library, plugin API, change-note-type, filtered
decks as persistent objects (import converts them back to home decks; a lightweight
*Custom session* covers the common uses), FSRS-7.

## 4. Technology **[changed in Rust]**

| Choice | Why |
|---|---|
| **Rust** engine (`crates/loci-core`), pure library, no UI | Every piece of logic in one tested crate that the desktop app, the Android app and the CLI share. |
| `rusqlite` (`bundled`, FTS5) | A real SQLite file on disk: atomic transactions, `integrity_check`, FTS5 trigram search; Anki's tables map one-to-one. The only C code in the build. |
| **FSRS-6 ported by hand**, not the `fsrs` crate | The crate pulls in the `burn` tensor framework (long compiles, big Android binary); the formulas are small and the spec vectors verify them. The optimizer is the TS port's Adam on log-loss, itself fsrs-rs's algorithm. |
| `zip` + `ruzstd` (pure-Rust zstd decoding) | Read every package layout, write the legacy one. No zstd compression is ever needed. |
| Hand-written protobuf field decoder | Only `PackageMetadata`, `MediaEntries` and the schema-18 config blobs are read; Anki's `.proto` files are AGPL, so fields are decoded by number. |
| **Tauri 2** (`apps/loci`) for Linux, Windows, macOS and Android | One Rust engine, one web UI, native windows and a signed APK. The UI stays web technology because Anki cards are HTML and need a real HTML renderer. |
| React 19 + Vite frontend ported from `recall/src/ui`, calling Tauri `invoke` through one generic `rpc` command | The `CollectionApi` interface and `types.ts` are unchanged; the worker client is replaced by a bridge. The same JSON protocol is served over local HTTP by `loci serve` for development and Playwright. |
| KaTeX (+mhchem), DOMPurify + Shadow DOM, `@tanstack/react-virtual`, `@fontsource` fonts | Unchanged from the TypeScript build: these are renderer concerns. |
| `cargo test` + `vitest` + Playwright (system Chrome, against `loci serve`) | Core behaviour in Rust tests; UI helpers in vitest; the 16 end-to-end flows against the real engine. |

No service worker, no OPFS, no `sqlite-wasm`, no worker RPC: the engine runs in-process.
The browser build is the exception: `crates/loci-web` compiles the engine to WebAssembly, a
dedicated worker runs it (`apps/loci/src/bridge/wasm.ts`), SQLite uses sqlite-wasm-rs with the
OPFS SyncAccessHandle-pool VFS, the blob store becomes a table in `blobs.sqlite`, and a service
worker precaches the build.

## 5. Architecture **[changed in Rust]**

```
crates/loci-core/src/
  types.rs         domain types (serde camelCase; the JSON is recall's types.ts)
  error.rs         LociError { code, message, detail }
  db.rs blobs.rs   rusqlite wrapper (transaction, serialize); directory blob store (media/, backups/)
  schema.sql       collection schema v1 (unchanged)
  collection/      Collection: lifecycle, settings, decks, presets, notetypes, notes, cards, media,
                   backups + study, render, backlog, browse, import, export, optimise, full_backup
  scheduler/       fsrs, sm2, states, steps, queue, fuzz, convert, items, optimizer
  anki/            zip, proto, read, write, schema11, deck_config, media_names, sqlite_bytes, sample_deck
  template/        parse, render, cloze, occlusion, typed, math, css, generate, check
  search/          parse, sql, describe
  stats/           stats, insights, messages
  backup/          rotation, integrity
  undo.rs i18n.rs  inverse-operation log; the core's English sentences
  rpc.rs           method table + JSON dispatch (the worker protocol, now in Rust)
crates/loci-cli/   loci: import, export, due, study, stats, backup, integrity, serve
apps/loci/
  src/             React UI (recall/src/ui) + bridge/ (tauri.ts, http.ts) + shared core/types.ts, api.ts, i18n
  src-tauri/       Tauri 2 app: rpc command, byte commands, progress channels, close → backup, Android config
```

**Rules.** The UI never touches SQL; it calls `CollectionApi` through the bridge, which
mirrors the interface 1:1 as async methods over `invoke`. Tests run `Collection`
directly. All times in the DB follow Anki's units (see `types.rs`). Every write path is a
transaction. Any operation that changes more than one card records an undo entry.

## 6. Data model (summary; `src/core/schema.sql` is authoritative)

Anki-compatible column names and units so import is a mapping, not a translation:
`notes(id ms, guid, ntid, mtime s, usn, tags, flds 0x1f-separated, sfld, csum, plain)`,
`cards(id ms, nid, did, ord, mtime, usn, type, queue, due, ivl, factor, reps, lapses,
left, odue, odid, flags, data JSON)`, `revlog(id ms, cid, usn, ease, ivl, last_ivl,
factor, time ms, type)`. Note types, fields, templates, decks and deck presets are
normalised tables with typed JSON config. FTS5 `notes_fts` (trigram) indexes
`notes.plain` (HTML-stripped fields) and tags. `col_meta` holds `crt`, rollover hour,
schema version and collection id. Media bodies live as files in `<data dir>/media/`
**[changed in Rust: files, not OPFS]**, indexed by `media(name, sha1, size)`.

FSRS memory state lives in `cards.data` as Anki writes it (`s`, `d`, `dr`, `decay`,
`pos`), so a round-trip through Anki keeps both schedulers' state.

## 7. Scheduler

- FSRS-6 (21 parameters) ported literally from fsrs-rs v6.6.2 (`recall/research/fsrs-spec.md`
  §1), accepting 17/19-parameter vectors via §2.3. Anki's fuzz **[changed in Rust: the
  exact PRNG, `StdRng::seed_from_u64(card.id + reps)`, which the TS port could only
  approximate]**, learn-ahead limit,
  day rollover, learning steps, relearning steps, and display orders as in §3, §4, §9.
- SM-2 exactly as Anki's v3 scheduler (§8), selectable per preset.
- Optimizer in Rust: outlier filter, pretrain w0–w3, Adam on log-loss with the
  spec's bounds and L2 prior (§6). Runs on a worker thread with progress reports. Auto-runs
  when a preset has ≥400 new reviews since its last optimisation (default on). Reports
  log-loss and RMSE(bins) before/after and keeps the better set.
- Reference tests: every vector in §7 must pass to the stated precision.
- Default preset ("Standard"): FSRS on, desired retention 0.90, new/day 20, max
  reviews/day 9999 with the *time budget* acting instead, learning steps 1m 10m,
  relearning 10m, max interval 36500, leech threshold 8 → "struggling" flow.
  Presets "Relaxed" (DR 0.85, 10 new) and "Intense" (DR 0.93, 40 new) differ only
  there.
- All three presets bury siblings of new and review cards (`burySiblingsNew`,
  `burySiblingsReview` on; interday learning off): one card of a note per day, so a
  reversed card never follows its forward card in the same session. Buried cards
  return at the next day rollover (or with *Unbury*). Imported Anki presets keep their own
  settings.

## 8. Study screen (the most refined thing in the app)

Layout: full-bleed canvas. Top bar 44px: back chevron, deck name (truncates), three
count pills in Anki's familiar colours (new blue, learning orange, due green) that
update as you go, overflow menu. Centre: the card, `max-width: 42rem`, on desktop a
`surface` panel with 1px `line` border and radius 16; on phones no panel, text
edge-to-edge with 20px gutters. Body text Newsreader 1.375rem desktop / 1.25rem
phone, line-height 1.45; Arabic falls to Noto Naskh Arabic at 1.15× size; `dir=auto`
on every field; maths via KaTeX; images constrained to the panel; audio autoplays
per Anki rules with a replay key (`r`). Answer appears below a hairline that *draws*
from the centre (200ms) while the answer rises 8px and fades in (240ms). Grading
slides the card 24px down and out (160ms) and the next card fades in (160ms).
Reduced motion: opacity only.

Bottom bar (always the same height, so buttons never move): front = one wide
"Show answer" button (space / enter). Back = four buttons, *fixed order and fixed
positions* Again · Hard · Good · Easy (keys 1 2 3 4, space = Good), each showing the
interval it would give, tinted by a 2px top rule in the grade colour (not filled; the
screen stays calm). Keyboard caps shown on pointer devices, hidden on touch. Tap
targets ≥ 48px.
Keycaps teach, then step back: they show for the first 12 answers of a session, then fade
(opacity, 200ms) and return while the pointer is over the bar or once `?` has been opened
that session; never with Keyboard hints off. On phones the grade buttons are 56px tall,
8px apart, with a 12px gutter and the safe-area inset below. **[changed in 0.2]**

Desk and paper **[changed in 0.2]**: the stage is the flat `--bg` desk; on desktop the card is
a `--surface` sheet with radius 16 and `--paper-shadow` (light: hairline plus a 1–2px lift; dark:
hairline plus a faint inset top highlight), sized by its content (40/48px padding, 32/24 at
720–1023px, at least 7rem), placed at the optical centre (45/55). Phones have no sheet; a short
card is centred the same way, a tall one starts at the top and scrolls. Top-bar counts show a
zero as a quiet `0` in `--faint`, never a dash.

Keys: `space/enter` show or Good · `1–4` grade · `z` undo · `e` edit · `s` suspend ·
`b` bury · `f` toggle flag, `ctrl+1..7` flag colours · `m` mark (tag `marked`) · `i`
card info · `r` replay audio · `t` type-answer focus · `?` help · `esc` leave.

Every card can answer "why this interval?" (`i`): retrievability now, stability,
difficulty, last reviews. Explainers are one sentence each, no jargon first.

**Session end** is a quiet summary: cards, minutes, accuracy, "tomorrow about N",
one insight if there is one. No confetti.

**Typed answers** compare after normalisation (case, diacritics, whitespace; configurable)
and show a character diff in grade colours. **Cloze** supports nested and overlapping
clozes exactly as Anki, plus *one-by-one reveal* as a per-note-type option. **Image
occlusion** renders Anki's IO shapes as an SVG overlay; hide-all-guess-one and
hide-one modes.

## 9. Humane workload (C8, C9, C28)

- **Time budget**, not card caps: each preset has a daily minutes target (default 20);
  Home shows "about N min today" from recent seconds-per-card.
- **Catch-up mode** (view `Backlog`): triggers when due > 2× the recent daily average
  or anything is >3 days overdue. Shows the debt in minutes, then offers: *Spread over
  N days* (cards postponed by retrievability — highest R first, so nothing likely
  forgotten waits), *Just today's share*, *Lower retention for a week*, or *Study
  everything*. All reversible (undo). FSRS makes postponement cheap and we say so.
- **Workload forecast** when changing new/day or desired retention: a sentence and a
  small sparkline of predicted reviews/day over 90 days (simulation on the actual deck).
- **Momentum** instead of streaks: days studied in the last 30 and a 12-month heatmap.
  A missed day is shown, never red; nothing resets.

## 10. Add and edit (C13, C14, C15)

Add is a side sheet (desktop) or a full screen (phone), opened by `a` anywhere. Note
type and deck pickers remember the last choice; `ctrl+enter` adds and keeps focus in
the first field; sticky fields keep their value. Fields are `contenteditable` with a
minimal toolbar (bold, italic, underline, sub/sup, list, link, image, audio, maths,
cloze, clear formatting) and an HTML source toggle; paste strips styles by default.
Cloze: select text, `ctrl+shift+c` wraps with the next number, `ctrl+alt+shift+c`
reuses the current number; cloze markers are highlighted in the editor. Image
occlusion: paste or pick an image, draw rectangles/ellipses/polygons, label, toggle
hide-all/hide-one; stored in Anki's IO format so it syncs through Anki unchanged.
Duplicate check on the first field. Live card preview below the fields on desktop.

Note types ship with a designed stylesheet; users change card font, size, alignment,
and colours in Settings without CSS. Templates remain editable for power users with
a live preview and errors shown inline (unknown field, unclosed section).

## 11. Browse (C16, C17)

Three-pane on desktop (sidebar · virtual table · editor), two levels on phone. Sidebar:
saved searches, Today (due, added today, studied today, forgotten), flags, decks,
note types, tags. Search box accepts Anki syntax (`docs/SEARCH.md`): `deck:`,
`tag:`, `note:`, `card:`, `is:due|new|learn|review|suspended|buried`, `flag:n`,
`prop:ivl>21`, `prop:due<=2`, `prop:r<0.8`, `rated:n[:ease]`, `added:n`, `edited:n`,
`introduced:n`, `nid:`, `cid:`, `"phrase"`, `-neg`, `OR`, parentheses, `re:`, `nc:`.
Table toggles cards/notes mode; columns sort; selection is keyboard-driven
(`j/k`, shift, `ctrl+a`). Bulk actions: change deck, add/remove tags, suspend,
bury, flag, reposition new, set due date, forget, delete, find and replace (field
scoped, regex). Everything undoable.

## 12. Statistics that say what to do (C18)

Each panel is chart + one sentence of meaning + (if applicable) one action link.
Panels: Today; Forecast (30/90 days, with and without new cards); True retention
(by period, vs desired retention); Reviews and time per day (12 months, heatmap);
Intervals and stability distribution; Difficulty distribution; Hourly performance;
Answer buttons. The **Insights** strip at the top of Home and Stats is produced by
rules in `core/stats/insights.ts`, e.g. "True retention 83% vs target 90%: optimise
parameters" · "Deck X has 14 struggling cards" · "At your pace the backlog clears in
4 days; catch-up can make it 2."

## 13. Settings (C5, C7)

Two layers. **Simple**: theme, card font and size, daily time budget, new cards per
day, desired retention (slider with retention/workload sentence), learning preset
(Relaxed / Standard / Intense), day starts at. **Advanced** (collapsed): every Anki
deck option, each with a one-line explanation, and a *Recommended* chip on the
default. FSRS parameters are visible, with Optimise and Evaluate buttons and the
last run's metrics.

## 14. Data safety (C22) — `docs/DATA-SAFETY.md`

- Every mutation is a transaction; `PRAGMA integrity_check` on open, repair offer on
  failure.
- Automatic backups: on first change after 30 minutes since the last backup and on
  closing the window; rotation keeps 12 daily, 10 weekly, 9 monthly in
  `<data dir>/backups/` **[changed in Rust: a directory, not OPFS]**. Each is a `.sqlite`
  file written to a temp name then renamed.
- *Save full backup* produces a Loci zip (collection + media + manifest) that Loci
  restores through Import — the same format the TypeScript app writes, so its data carries
  over; Anki interoperability goes through *Export collection* (`.apkg`).
- Restore: pick a backup (list with dates and card counts) or a file; the current
  collection is backed up first.
- Import never overwrites: new collection or merge-by-guid with newest-wins, and the
  importer refuses to proceed if it cannot read every note.
- Tested: backup → corrupt → open → restore → counts equal (unit and e2e).

## 15. Import and export (`research/anki-format.md`)

Import accepts `.apkg` (LEGACY_1, LEGACY_2, LATEST) and `.colpkg`. Preview screen
(notes, cards, decks, reviews, media, scheduler state) → import with progress → report.
Review `due` is shifted by the `crt` day difference; filtered-deck cards are returned
home; FSRS state preserved from `card.data`; SM-2 history preserved in revlog; media
written to `media/` with Anki's name normalisation. Export (collection or deck,
with or without scheduling) writes a LEGACY_2 package with `schedVer: 2`, `meta=08 02`,
JSON media map, and the dummy `collection.anki2`, so Anki 2.1.x–26.x and AnkiDroid
import it. Round-trip test: import fixture → export → import → identical counts,
identical due days, identical revlog.

## 16. Accessibility and i18n (C24, C25)

Tab order covers everything; visible focus rings (2px accent, offset 2); grade and
card changes announced via a live region; `prefers-reduced-motion` honoured; contrast
AA in both themes; tap targets ≥ 44px. All strings through `t()`; CSS uses logical
properties; `dir` on `<html>` follows the UI locale; card content always `dir=auto`.

## 17. Visual design system (`src/ui/design/tokens.css`)

Colour in OKLCH. Light: `--bg oklch(97.5% .006 85)`, `--surface oklch(99% .004 85)`,
`--ink oklch(22% .02 260)`, `--muted oklch(50% .02 260)`, `--line oklch(88% .01 85)`,
`--accent oklch(62% .13 70)` (brass), `--on-accent white`. Dark: `--bg oklch(17% .015 260)`,
`--surface oklch(21% .015 260)`, `--ink oklch(93% .01 85)`, `--muted oklch(68% .015 260)`,
`--line oklch(30% .015 260)`, `--accent oklch(78% .12 75)`. Grades: again
`oklch(62% .17 20)`, hard `oklch(72% .14 65)`, good `oklch(66% .14 150)`, easy
`oklch(68% .12 230)` (dark theme: +8% lightness). Counts: new `oklch(60% .15 250)`,
learn `oklch(68% .16 50)`, due `oklch(60% .14 150)`.
Type scale: 12 / 13 / 14 (UI base) / 16 / 20 / 24 / 32; card body 22 desktop, 20
phone. Radii 6 / 10 / 16. Spacing 4-point grid. Shadows: none in light theme beyond a
1px `--line` border; dark theme uses `--surface` lift only. Motion: 120 / 160 / 180ms (0.1: 120 / 200 / 320) **[changed in 0.2]**,
`cubic-bezier(.2,.8,.2,1)`. Icons: inline SVG, 1.5px stroke, 20px. Theme switch is
instant with a 200ms colour cross-fade on `html`.

Additions in 0.2 **[changed in 0.2]**:
- `--accent-fill` / `--on-accent-fill` (and `--accent-fill-hover`): the fill of primary buttons, switches and checked marks. Light = `oklch(54% .14 70)` with white; dark = `oklch(55% .12 70)` with `oklch(98% .006 85)`, a brass block on slate rather than a pale slab. `--accent-strong` stays for text, icons and rules. **[changed in 0.2]**
- `--text-2xs` 0.6875rem (11) and `--text-3xs` 0.625rem (10, keycaps and chart axes only); no `px` font sizes. **[changed in 0.2]**
- `.num` / `.num-lg`: JetBrains Mono, tabular, weight 500, for numbers read as data. **[changed in 0.2]**
- One Arabic stack, `--font-arabic: 'Noto Naskh Arabic', serif`, referenced by every other stack and by the card (`--loci-arabic-font`). **[changed in 0.2]**
- `--paper-shadow` (study card, card previews) in both themes; in dark the cloze colour is `--accent` (`--accent-strong` is too pale on slate paper). Image-occlusion masks: `--line-strong` 85% over surface with a 1px `--muted` edge; the asked mask brass 22% with a 2px brass rule. **[changed in 0.2]**
- Contrast (`pnpm contrast`, `docs/design/contrast-0.2.md`): light counts darkened to new `oklch(54% .15 250)`, learn `oklch(55% .16 50)`, due `oklch(52% .14 150)`; light grades to hard `oklch(66% .14 65)`, good `oklch(63% .14 150)`, easy `oklch(64% .12 230)`; light danger `oklch(56% .19 25)`; pill text mixes 68% tone into ink (was 82%). Hues unchanged. **[changed in 0.2]**

- Charts: brass (`--accent`) is the one series colour; a second brass (`accent-soft`, `--accent-strong` 72% over surface) and a slate (`slate`, `--ink-2` 58% over surface) carry secondary series; all three are 3:1 or more against the panel in both themes (`pnpm contrast`). Grade colours only on the answer-buttons chart, count colours only on card counts. **[changed in 0.2]**
- Browse selection: a slate tint (`--ink-2` 11% over surface) with a 2px `--accent-strong` rule at the inline start; header 40px. **[changed in 0.2]**

Theme follows the system by default; both themes are designed (not inverted): the
dark theme lowers chroma on large surfaces and raises it on the accent. The native window
follows the same preference (`setTheme(light|dark)`, or `null` = the OS while it is "system"),
so the title bar, file dialogs and the macOS menu bar match the page. **[changed in 0.2]**

### 17a. Window and menus **[changed in 0.2]**

- **macOS**: a native menu bar (Loci, File, Edit, View, Window, Help). The Edit menu is what makes
  Cmd+C/V/X/Z/A work in a webview; app items (Settings Cmd+,, Add Card Cmd+N, Import, Decks /
  Browse / Statistics Cmd+1–3, Appearance, Manual, Shortcuts) emit their command id to the page.
- **Linux and Windows**: no menu bar. A GTK or Win32 menu strip above a designed page is the
  "Windows app" look the owner rejected, and every command already lives in the sidebar, the
  keys, and the command palette (Ctrl/Cmd+K, also a "Commands" button in the sidebar). The OS
  title bar stays (client-side decorations would fight GNOME, KDE and tiling WMs).
- **Android**: no menu; the phone tab bar.
- The window remembers its size, position and maximised state (`tauri-plugin-window-state`).
- Icon: the Loci mark (brass portico on slate) for every platform's icon set.

## 18. First run (C3)

A single calm screen: the wordmark, one sentence, three cards: *Import from Anki*
(drop zone), *Try a sample deck* (bundled 30-card deck about memory itself, with
cloze, image, maths and Arabic cards), *Start empty*. No account, no tour. After
import: a preview, then Home with "Start" focused. The footer links the manual for people new
to spaced repetition (opened in the system browser). **[changed in 0.2]**

## 19. Testing contract

- `core/scheduler`: all spec §7 vectors; property tests (interval monotone in
  stability; R in (0,1]; SM-2 ease ≥ 1300).
- `core/anki`: reads all three fixture layouts; counts match the generator; due days
  equal Anki's; export round-trip equality; a malformed package is rejected cleanly.
- `core/template`: Anki template suite (sections, cloze nesting, hints, type, special
  fields, IO shapes, maths conversion, sanitiser keeps media and drops scripts).
- `core/search`: each operator produces expected ids on a fixture collection.
- `core/backup`: rotation policy and restore.
- e2e (Playwright, system Chrome, the UI served by Vite against `loci serve`)
  **[changed in Rust]**: first run → import fixture → study five cards → add a cloze note
  → browse search → stats renders → export; captures at 1440×900 and 390×844, light and
  dark, saved to `docs/captures/`.
- Performance (recorded in README): cold start, 100k-card import, 100k-row browse, answer
  round-trip.

## 20. Decisions the owner should know (also in README) **[changed in Rust]**

1. Native apps through Tauri 2 (Linux, Windows, macOS, Android), the engine in Rust, the
   UI in a webview because cards are HTML. Data lives in the platform app-data directory
   (app-internal storage on Android) as real files.
2. Export is the legacy `.apkg` layout, which every Anki version imports; we do not
   write `.anki21b`.
3. FSRS on by default, SM-2 available. Auto-optimisation on by default.
4. Filtered decks are not persistent objects in v1.
5. Sync is deferred, but every table carries `usn` and `graves` exist, and
   `recall/docs/SYNC.md` has the plan.
6. The TypeScript app's *full backup* zip imports here unchanged; that is the migration path.
