# Architecture for contributors

This chapter is a map of the code for someone about to change it. The two documents behind it are the design contract, [`DESIGN.md`](https://github.com/ZahakJ/loci-rs/blob/main/DESIGN.md), which every implementation builds against, and the build plan, [`PLAN.md`](https://github.com/ZahakJ/loci-rs/blob/main/PLAN.md), which records how the TypeScript Loci was ported and what changed. If the code and `DESIGN.md` disagree, one of them is wrong: fix one and say which.

## Three parts

```text
crates/loci-core    the engine: SQLite (rusqlite, FTS5), FSRS-6 and SM-2, Anki package import/export,
                    the Loci full-backup format, Anki search syntax, browser queries, statistics,
                    catch-up, rotating backups, integrity check. A pure library with its own tests.
crates/loci-cli     `loci`: import, export, due, study, stats, backup, integrity, and `serve`
                    (the engine over local HTTP for development and the end-to-end tests).
apps/loci           the Tauri 2 app for Linux, Windows, macOS and Android: React UI calling the
                    engine through Tauri `invoke`.
```

The engine is entirely Rust. The interface stays web technology, React in a Tauri webview, because Anki cards are HTML and need a real HTML renderer. Everything that is logic lives in `loci-core`, so the desktop app, the Android app and the CLI share one tested implementation. The three are members of one Cargo workspace (`crates/loci-core`, `crates/loci-cli`, `apps/loci/src-tauri`), and the toolchain is pinned to Rust 1.99.0 by `rust-toolchain.toml`.

## The engine's modules

```text
crates/loci-core/src/
  types.rs         domain types (serde camelCase; the JSON matches the frontend's types.ts)
  error.rs         LociError { code, message, detail }
  db.rs blobs.rs   rusqlite wrapper (transaction, serialize); directory blob store (media/, backups/)
  schema.sql       collection schema v1
  collection/      Collection: lifecycle, settings, decks, presets, notetypes, notes, cards, media,
                   backups + study, render, backlog, browse, import, export, optimise, full_backup
  scheduler/       fsrs, sm2, states, steps, queue, fuzz, convert, items, optimizer
  anki/            zip, proto, read, write, schema11, deck_config, media_names, sqlite_bytes, sample_deck
  template/        parse, render, cloze, occlusion, typed, math, css, generate, check
  search/          parse, sql, describe
  stats/           stats, insights, messages
  backup/          rotation, integrity
  undo.rs i18n.rs  inverse-operation log; the core's English sentences
  rpc.rs           method table + JSON dispatch
```

Each Rust module was ported from a file of the TypeScript Loci, and its doc comment names that file and the section of `DESIGN.md` or of the research specs it implements. Start there when you need to know why something is the way it is. The research behind the formats and formulas is in `research/`: `fsrs-spec.md` (FSRS-6 and Anki's SM-2, with the reference test vectors) and `anki-format.md` (the package layouts and schemas).

Errors carry one of a fixed set of codes: `not-found`, `invalid`, `conflict`, `unsupported-package`, `corrupt`, `io`, `busy`, `template`, `media`, `internal`. The message is a sentence a user can read.

## The rules

These hold everywhere, and a change that breaks one is a bug:

1. **The UI never touches SQL.** It calls the `CollectionApi` interface through the bridge. Tests call `Collection` directly.
2. **Every write is a transaction.** `Db::transaction` wraps each write path (`BEGIN IMMEDIATE … COMMIT`, rolled back on any error; nested calls join the outer transaction).
3. **Undo entries.** Any operation that changes more than one card records an undo entry, written inside the same transaction as the change, so the two commit or vanish together. The stack keeps the last 50 operations.
4. **Anki's units.** All times in the database follow Anki's units (see [The data model](data-model.md)).

## The RPC layer

`rpc.rs` defines `Engine`, the one seam that the Tauri shell, the CLI's `serve` bridge and the tests share:

```text
let mut e = Engine::open(dir)?;                       // opens <dir>/collection.sqlite, media/, backups/
e.dispatch("open", &[], None, &mut |_| {})?;          // -> Payload::Json(IntegrityReport)
e.dispatch("deckTree", &[], None, &mut |_| {})?;      // -> Payload::Json
e.dispatch("getMedia", &[json!("a.png")], None, ..)?; // -> Payload::Bytes(Option<Vec<u8>>)
e.dispatch("importPackage", &[json!(opts)], Some(&bytes), &mut |p| ...)?;
```

Method names and argument order are exactly those of the TypeScript `CollectionApi`: `method(args...)` becomes `dispatch(method, &args)`. Byte-valued arguments travel separately as `bytes`, byte-valued results come back as `Payload::Bytes`, and progress callbacks become the `progress` sink. Unknown methods and malformed arguments fail with `invalid`.

Because the JSON is the TypeScript app's JSON (serde `rename_all = "camelCase"`), the frontend's `CollectionApi` interface and `types.ts` did not change in the port.

## The bridge

`apps/loci/src/bridge/` gives the UI a `CollectionApi` whose every method is one RPC call over the active transport:

- **Tauri** (`tauri.ts`), in the desktop and Android apps. The shell in `apps/loci/src-tauri/src/lib.rs` defines four commands: `stage_bytes` (raw bytes in, a token out), `rpc` (JSON in and out, optional staged bytes, a progress `Channel`), `rpc_bytes` (JSON in, raw bytes out) and `save_bytes` (a native save dialog on desktop, the app's downloads folder on Android).
- **HTTP** (`http.ts`), against `loci serve`, used by `pnpm dev` without Tauri and by the Playwright suite. Progress is not streamed over HTTP.

`protocol.ts` holds the table of which methods change what (`CHANGES`). After a successful call the bridge emits `changed` events from that table, so views refresh without knowing which method ran. Media reach the webview as blob URLs built from `getMedia` bytes, cached per name.

The shell opens the collection in the platform app-data directory at start-up and, when the window closes, writes an automatic backup if anything changed.

## Tests

| Command | What it runs |
|---|---|
| `cargo test --workspace` | the engine's tests, ported from the TypeScript suite: unit tests inside each module and integration tests in `crates/loci-core/tests/integration/` |
| `pnpm test` (in `apps/loci`) | vitest on the UI helpers (formatters, keymaps, routes, editor helpers) in `apps/loci/tests/ui/` |
| `pnpm test:e2e` (in `apps/loci`) | Playwright on the system Chrome against `loci serve`: the flows in `apps/loci/tests/e2e/` |
| `./verify` | the release gate, below |
| `./verify --quick` | fmt, clippy, cargo test, frontend typecheck and vitest |

At v0.1.0 the counts were 455 library tests and 56 integration tests in `loci-core` (two of them `#[ignore]`d performance tests), 183 vitest cases and 16 Playwright flows. Fixture decks are synthetic, in `fixtures/`: the same collection in all three Anki layouts and as a `.colpkg`, a `palace.expected.json` with the counts every reader must reproduce, and five broken packages the importer must reject.

`./verify` runs, in order: `cargo fmt --check`; `cargo clippy` with warnings denied; `cargo test`; the frontend typecheck and vitest; the frontend build; clippy on the app; the Tauri desktop build (debug); the CLI build; and the end-to-end suite unless `LOCI_VERIFY_SKIP_E2E=1`. Everything must pass before a tag is pushed.

## CI and releases

- **`ci.yml`** runs `./verify` on every push and pull request, on Linux, with `LOCI_VERIFY_SKIP_E2E=1` (the end-to-end suite runs locally).
- **`release.yml`** runs on tags matching `v*`. It builds Linux (AppImage, deb, rpm), Windows (msi and the NSIS `-setup.exe`) and macOS (dmg for Apple silicon and for Intel) with `tauri-action`, and a separate job builds the Android APKs (arm64 and universal), signed with the release keystore from the repository's secrets. All are attached to a GitHub release named after the tag.
- **`pages.yml`** builds the project site and this manual (mdBook 0.5.4, `docs/build-site.sh`) on every push to `main` and publishes them to GitHub Pages.

## How the captures are made

The screenshots in `docs/captures/` and in this manual come from `apps/loci/scripts/capture.ts`:

```sh
pnpm build && cargo build -p loci-cli && node --experimental-strip-types scripts/capture.ts
```

It starts `loci serve --multi-session` and `vite preview`, imports the synthetic `palace-legacy2.apkg` fixture through the interface into the real Rust engine (never a mock), and captures each screen at desktop size (1440×900) and phone size (390×844, device scale 2), in the light and the dark theme, as `<screen>-<desktop|phone>-<light|dark>.png`. It uses one headless Chrome.

## Conventions visible in the code

- **Formatting:** `rustfmt.toml` sets `max_width = 130` and `use_small_heuristics = "Max"`. `cargo fmt --check` is the first step of the gate.
- **Lints:** clippy runs with `-D warnings` on every crate and target.
- **Numbers:** f64 everywhere, as in the TypeScript, including FSRS (fsrs-rs uses f32, so results agree to about 1e-5 relative). `serde_json`'s `float_roundtrip` is on, so stored FSRS parameters read back exactly.
- **Provenance:** module doc comments cite the TypeScript file they port and the spec section they implement (for example `research/fsrs-spec.md §1`).
- **JSON shapes:** serde `rename_all = "camelCase"`, numeric enums through `serde_repr`, optional fields skipped when `None`.
- **Strings:** the interface's text goes through `t()` with keys in `apps/loci/src/core/i18n/en.json`; the engine's sentences (statistics, insights) come from a small English dictionary in `crates/loci-core/src/i18n/`.
- **SQLite:** rusqlite with the bundled SQLite (the only C code in the build); `power()` is registered on every connection, because the bundled build has no math functions.
