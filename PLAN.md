# Loci in Rust — build plan

Written before the first line of Rust. The TypeScript implementation in the sibling
`recall` repository (published as ZahakJ/loci) is the specification: its `DESIGN.md`,
its 591 unit tests and 16 end-to-end tests, its fixtures and its behaviour. Everything
that is logic becomes Rust. The UI stays web technology (React) because Anki cards are
HTML and need a real HTML renderer; it is ported to call the Rust engine through Tauri.

## 0. Decisions that refine the brief

| Decision | Why |
|---|---|
| Pin Rust **1.99.0** through `rust-toolchain.toml` (installed as a separate toolchain, not by updating `stable`) | The machine's default `stable` is 1.86 and a sibling session is building against it right now; replacing it mid-build could break them. A pinned, separate toolchain is also what CI will use. |
| **Tauri 2.7** (`tauri = "2"`, `@tauri-apps/cli@2`), not the 3.0 alphas that crates.io lists as "latest" | The brief says Tauri 2; the 3.0 alphas are immature tooling and the Android target is the deliverable we must not gamble. |
| FSRS-6 is **ported by hand** from the TS port (itself a literal port of fsrs-rs v6.6.2), not taken from the `fsrs` crate | The `fsrs` crate drags in the `burn` tensor framework: minutes of extra compile, a much bigger Android binary, and a cross-compile risk, for formulas that are twenty lines each. The optimizer (Adam on log-loss with fsrs-rs's outlier filter, pretraining, L2 prior and acceptance test) is ported from `recall/src/core/scheduler/optimizer.ts`. The spec vectors in `recall/research/fsrs-spec.md` §7 and the recall tests verify both. |
| **Fuzz uses Anki's exact PRNG** (`StdRng::seed_from_u64(card.id + reps)`, ChaCha12) | The TS port had to approximate it; in Rust the real thing costs nothing. Test expectations that depended on the TS PRNG are regenerated and the change is recorded in DESIGN.md. |
| **Pure-Rust zstd decoding** (`ruzstd`) and a **hand-written protobuf field decoder** (port of `proto.ts`) | We only ever *read* `.anki21b`; a pure-Rust decoder removes one C cross-compile and `prost`/`protoc` would need Anki's AGPL `.proto` files. |
| One generic **JSON RPC** layer in the core (`rpc::dispatch(method, args) -> Result<Value>`) mirrors the TS worker protocol 1:1 | The same table serves the Tauri command, the CLI's `serve` bridge for end-to-end tests, and tests. The frontend's `CollectionApi` interface and `types.ts` stay byte-for-byte; serde `rename_all = "camelCase"` reproduces the JSON shapes. |
| **`loci serve`** (CLI) exposes the RPC over local HTTP for development and Playwright | Lets the ported UI run in a normal browser against the real Rust engine, so the 16 e2e tests port directly and screenshots are easy. Not shipped in the app. |
| Media are served to the webview as **blob URLs from `getMedia` bytes** (the recall path), with a custom URI scheme left as a later optimisation | Zero new risk; the recall UI already does it. |
| Backups, media and the collection are **real files** in the platform app-data directory (`dirs`), app-internal storage on Android | Replaces OPFS. Same layout as recall's Node CLI: `collection.sqlite`, `media/`, `backups/`. |
| UI-side logic tests (160 vitest tests on formatters, keymaps, routes, editor helpers) **stay in the frontend** | They test TypeScript that still exists; the brief's "logic that moved" is the core. |

## 1. Layout

```
loci/
  Cargo.toml                 workspace: crates/*, apps/loci/src-tauri
  rust-toolchain.toml        1.99.0 + Android targets
  verify                     release gate: fmt, clippy, cargo test, frontend typecheck+tests, desktop build
  PLAN.md  DESIGN.md  README.md  docs/
  fixtures/                  recall's synthetic .apkg/.colpkg packages + palace.expected.json
  crates/loci-core/          the engine (library, no UI)
  crates/loci-cli/           `loci` binary: import, export, due, study, stats, backup, integrity, serve
  apps/loci/                 Tauri 2 app: Vite + React frontend (ported from recall/src/ui) + src-tauri/
  .github/workflows/         ci.yml (Linux, verify), release.yml (tags v*: Linux, Windows, macOS, Android)
```

### loci-core modules (what is ported from where)

| Rust module | From `recall/src/core/` | Notes |
|---|---|---|
| `types` | `types.ts` | serde, camelCase; numeric enums via `serde_repr`; optional fields skipped when `None`. |
| `error` | `api.ts` `LociError` | codes: not-found, invalid, conflict, unsupported-package, corrupt, io, busy, template, media, internal. |
| `db` | `db/types.ts`, `db/node.ts` | rusqlite (`bundled`, FTS5); `Db::transaction`, `serialize` (backup API), open-from-bytes. |
| `blobs` | `db/blob-node.ts`, `blob-path.ts` | directory store: atomic put via temp+rename; media/, backups/. |
| `text` | `text.ts` | HTML stripping, field checksum (sha1 → first 8 hex), tags join/split, `plain`. |
| `scheduler::{fsrs,sm2,states,steps,queue,fuzz,convert,items,optimizer}` | `scheduler/*` | fsrs-spec §1–§9; queue gathering v3; SM-2 v3. |
| `anki::{zip,proto,read,write,schema11,deck_config,media_names,sqlite_bytes,image,sample_deck}` | `anki/*` | three layouts in, LEGACY_2 out. |
| `template::{parse,render,cloze,occlusion,typed,math,css,generate,check}` | `template/*` | sanitising stays in the UI (DOMPurify needs a DOM). |
| `search::{parse,sql,describe}` | `search/*` | Anki syntax → SQL. |
| `stats::{stats,insights,messages}` | `stats/*` | sentences through a small core English dictionary. |
| `backup::{rotation,integrity}` | `backup/*` | 12 daily / 10 weekly / 9 monthly; integrity check and repair. |
| `undo` | `undo/undo.ts` | inverse-operation log in `undo_log`. |
| `collection::*` | `collection/*` | `Collection` (lifecycle, settings, decks, presets, notetypes, notes, cards, media, backups) + `study`, `render`, `backlog`, `browse`, `import`, `export`, `optimise`, `full_backup`, `presets`, `settings`, `time`, `builtin_notetypes`. |
| `i18n` | `i18n/en.json` subset | only the keys the core's sentences use. |
| `rpc` | `worker/protocol.ts` | method table, `CHANGES`, JSON dispatch, progress channel trait. |

## 2. Phases and verification

**Phase 0 — foundation (this session, before any agent).** Workspace, toolchain pin,
`types.rs`, `error.rs`, `db.rs`, `blobs.rs`, `schema.sql`, `text.rs` skeleton, fixture
copy, PLAN.md, DESIGN.md, first commit. *Verify:* `cargo build -p loci-core`.

**Phase 1 — leaf modules (three agents in parallel, module ownership disjoint).**
A: `scheduler/*`. B: `anki/*` + `text`. C: `template/*`, `search/*`, `stats/*` (pure parts),
`backup/rotation`, `undo` payload types, `i18n`. Each ports the matching recall tests.
*Verify:* `cargo test -p loci-core` green per module; FSRS §7 vectors; all five fixture
packages read with counts equal to `palace.expected.json`; malformed packages rejected cleanly.

**Phase 2 — the Collection and the API (two agents) + frontend/Tauri shell (one agent).**
D: `collection` core (internals, schema/migrations, settings, time, decks, presets,
notetypes, notes, cards, media, backups, integrity, full backup, sample deck, undo).
E: `study`, `render`, `backlog`, `browse`, `import`, `export`, `optimise`, stats wiring,
`rpc` dispatch. F: `apps/loci` — copy `recall/src/ui`, replace the worker client with a
Tauri bridge (`invoke('rpc', {method,args})`, `Channel` for progress, byte commands for
import/export/media), add an HTTP bridge for `loci serve`, Tauri config, icons, dialog
and fs plugins, window close → backup. *Verify:* recall's collection/integration tests
pass in Rust; `pnpm typecheck && pnpm test` in the frontend; `cargo tauri dev` opens.

**Phase 3 — CLI, gate, measurements.** `loci-cli` commands; `./verify`; e2e tests ported
to Playwright against `loci serve` + `vite preview`; perf: cold start, 100k-card import,
browser 100k rows, answer round-trip. *Verify:* `./verify` green; numbers in README.

**Phase 4 — distribution.** Local Linux bundles (AppImage/deb/rpm) launched under a
timeout; Android release APK signed with a new keystore, checked with `aapt`/`apksigner`;
GitHub private repo, secrets, `ci.yml`, `release.yml`, tag `v0.1.0` once, watch, fix.

## 3. Risks and mitigations

- **Volume** (~12k lines of core TS, ~430 core tests). Mitigation: strict module
  ownership per agent, shared `types.rs`/`db.rs` written first, tests ported with code.
- **Behavioural drift** in a hand port (JS number semantics, regex dialect, Date/TZ).
  Mitigation: port tests before polishing; `chrono::Local` for the rollover; `regex`
  (and `fancy-regex` only where lookaround is unavoidable); f64 everywhere like the TS.
- **Android build.** NDK 27 + JDK 21 + SDK 35/36 are present; rusqlite's bundled SQLite is
  the only C code. Check `free -g` before building; never run the emulator under 8 GB free.
- **Tauri IPC with large byte payloads** (a 100k-card `.apkg` can be hundreds of MB).
  Mitigation: desktop import goes by *path* through the dialog plugin (no copy); the
  byte path remains for drag-and-drop and Android.
- **Sibling session load.** `CARGO_BUILD_JOBS=6`, at most three agents, one headless browser.
- **macOS unverifiable locally.** Built only by the release workflow; marked as such.

## 4. Done means

`./verify` green; release `v0.1.0` with Linux (AppImage, deb, rpm), Windows (msi, exe),
macOS (dmg ×2) and Android (arm64 + universal APK) assets; README install lines point at
those files; the owner's TS-app full backup imports into the Rust app.

## 5. Outcome (recorded after the build)

- Phases 0–3 went as planned; five agents in two waves (A scheduler, B Anki packages, C template/search/rotation/i18n; D Collection core, E study/browse/import/export/stats/rpc), each with disjoint file ownership and a scratch-copy compile loop while siblings were mid-edit.
- Tests: 455 lib + 56 integration in `loci-core` (two `#[ignore]`d perf tests), 183 vitest cases for the UI helpers, 16 Playwright flows against `loci serve`. `./verify` green locally and in CI.
- Measured (README): cold start 0.81–0.83 s, 100k-card import 4.6 s, browse page 86–91 ms at any offset, answer 0.4 ms.
- Deviations from §0: none. Extra: `unicode-normalization` replaced two hand-written tables; `serde_json`'s `float_roundtrip` is on so stored FSRS parameters read back exactly; `power()` is registered on every SQLite connection (the bundled build has no math functions).
- `tauri android build --target aarch64` writes its APK under `apk/universal/`; the release workflow renames by `find` rather than by the expected path.
