<p align="center"><img src="apps/loci/public/icons/icon.svg" width="72" alt=""></p>
<h1 align="center">Loci</h1>

Loci is a local-first spaced-repetition app, written in Rust, that imports your Anki collection with its history intact and asks less of your day.

## Install

Download one file for your platform from the [latest release](https://github.com/ZahakJ/loci-rs/releases/latest), or paste the matching line.

**Linux** (AppImage; `.deb` and `.rpm` are on the release page too)

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.1.0/Loci_0.1.0_amd64.AppImage && chmod +x Loci_0.1.0_amd64.AppImage && ./Loci_0.1.0_amd64.AppImage
```

**Windows** (PowerShell; `.msi` installer, or the `-setup.exe`)

```powershell
Invoke-WebRequest https://github.com/ZahakJ/loci-rs/releases/download/v0.1.0/Loci_0.1.0_x64_en-US.msi -OutFile Loci.msi; Start-Process .\Loci.msi
```

**macOS** (Apple silicon; Intel: replace `aarch64` with `x64`)

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.1.0/Loci_0.1.0_aarch64.dmg && open Loci_0.1.0_aarch64.dmg
```

The macOS build is not notarised: after dragging Loci to Applications, run `xattr -d com.apple.quarantine /Applications/Loci.app` once, or right-click → Open.

**Android** (arm64 phones; a universal APK for every ABI is on the release page)

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.1.0/loci-v0.1.0-android-arm64.apk && adb install loci-v0.1.0-android-arm64.apk
```

Or open the APK link on the phone and allow installs from your browser. Android 7.0 (API 24) or newer.

**From source**

```sh
git clone git@github.com:ZahakJ/loci-rs.git && cd loci-rs/apps/loci && pnpm install && pnpm tauri build
```

## Screenshots

All captures show the synthetic fixture collection (`fixtures/`, 402 notes, 590 cards, three years of simulated history) imported into the Rust engine and rendered by the app's own interface; no real deck was used. More in `docs/captures/`.

<p align="center"><img src="docs/captures/home-desktop-light.png" width="800" alt="Home: today's estimate, insights, the deck tree and momentum, light theme"></p>
<p align="center"><img src="docs/captures/study-back-desktop-dark.png" width="800" alt="Study screen in the dark theme: an Arabic card with its answer revealed and four fixed grade buttons"></p>
<p align="center">
  <img src="docs/captures/study-front-phone-light.png" width="196" alt="Study on a phone, question side">
  <img src="docs/captures/study-back-phone-dark.png" width="196" alt="Study on a phone, answer side, dark theme">
  <img src="docs/captures/home-phone-light.png" width="196" alt="Home on a phone">
  <img src="docs/captures/stats-phone-dark.png" width="196" alt="Statistics on a phone, dark theme">
</p>
<p align="center">
  <img src="docs/captures/browse-desktop-light.png" width="400" alt="Browse: sidebar, virtualised table with a selection and the bulk toolbar, editor pane">
  <img src="docs/captures/stats-desktop-dark.png" width="400" alt="Statistics with insights and a forecast, dark theme">
</p>
<p align="center">
  <img src="docs/captures/add-cloze-desktop-light.png" width="400" alt="Add sheet with a cloze note and live preview">
  <img src="docs/captures/backlog-desktop-light.png" width="400" alt="Catch-up: the backlog in minutes and four plans">
</p>

## What it is

- **A real Anki replacement.** Imports `.apkg` and `.colpkg` from every Anki version (legacy and current package layouts) with notes, note types, templates, media, tags, decks, review history and scheduling state. A card due tomorrow in Anki is due tomorrow here. Exports back to `.apkg` that Anki 2.1 through 26.x and AnkiDroid read.
- **FSRS-6 by default**, ported from the reference implementation and tested against its published vectors, with per-user parameter optimisation that runs itself. Anki's SM-2 is there for people who want it.
- **One app for desktop, laptop and phone.** Native builds for Linux, Windows, macOS and Android from one Rust engine and one interface; a study screen designed around the keyboard on a desk and the thumb on a phone; light and dark themes that are designed rather than inverted; typography that renders Arabic, mathematics and code well.
- **Humane about workload.** A catch-up mode for backlogs, a daily time budget instead of card caps, a workload forecast before you change settings, and momentum instead of streaks.
- **Data you own.** A real SQLite file in your app-data folder, automatic rotating backups, restore, an integrity check on open, and a full-backup zip (collection plus media) that Loci restores. No account, no server, nothing leaves the device.

Loci started as a TypeScript web app (ZahakJ/loci). This repository is the Rust rewrite; the TypeScript app's full-backup zip imports here unchanged, so nothing is lost moving over.

## Architecture

```
crates/loci-core    the engine: SQLite (rusqlite, FTS5), FSRS-6 and SM-2, Anki package import/export,
                    the Loci full-backup format, Anki search syntax, browser queries, statistics,
                    catch-up, rotating backups, integrity check. A pure library with its own tests.
crates/loci-cli     `loci`: import, export, due, study, stats, backup, integrity, and `serve`
                    (the engine over local HTTP for development and the end-to-end tests).
apps/loci           the Tauri 2 app for Linux, Windows, macOS and Android: React UI calling the
                    engine through Tauri `invoke`.
```

The engine is 100% Rust. The user interface stays web technology (React in a Tauri webview) because Anki cards are HTML and need a real HTML renderer; everything that is logic lives in the Rust crate, and the UI is the same interface the TypeScript app had, with its worker replaced by a thin bridge. The visual identity is unchanged: warm stone and paper by day, deep slate at night, one brass accent; never neon, never glassmorphism, never a 3D card flip (`DESIGN.md`).

Data lives in the platform app-data directory (`~/.local/share/space.avicenna.loci` on Linux, `%APPDATA%\space.avicenna.loci` on Windows, `~/Library/Application Support/space.avicenna.loci` on macOS, the app's internal storage on Android): `collection.sqlite`, `media/` as plain files, `backups/` rotating 12 daily, 10 weekly and 9 monthly copies.

### Decisions worth knowing

1. FSRS-6 is ported by hand (from the TypeScript port of fsrs-rs v6.6.2, verified against the spec vectors) rather than taken from the `fsrs` crate, which drags in a tensor framework for formulas that are twenty lines each. Fuzz uses Anki's exact PRNG, which the TypeScript app could only approximate.
2. Export writes the legacy `.apkg` layout that every Anki version imports; the newer zstd layout is read, never written.
3. FSRS on by default, auto-optimisation on by default (after 400 reviews per preset). SM-2 is a preset switch.
4. Filtered decks are not persistent objects; importing one returns its cards home. *Custom session* covers study-ahead, forgotten, preview and search.
5. Sync is deferred, but every table carries `usn`, deletions are recorded, and `docs/SYNC.md` has the plan.
6. iOS is out for now: no device, no signing (`docs/DEFERRED.md`).

## Performance

Measured on the development desktop (16 cores, 30 GB, Linux) with release builds, while another build job was running on the machine. The engine numbers come from `cargo test --release -p loci-core --test integration -- --ignored perf --nocapture` on a generated 100,000-card legacy `.apkg` (18 MB: 100,000 notes and cards, 437,735 review-log rows after de-duplication).

| Measurement | Result |
|---|---|
| Cold start (process start to the Home screen interactive, release binary under Xvfb with software rendering, three runs) | 806 ms, 818 ms, 827 ms |
| Import of the 100,000-card `.apkg` | 4.6 s |
| Deck tree with counts on 100,000 cards | 37 ms |
| Browse page of 100 rows sorted by due, offset 0 / offset 99,900 | 91 ms / 86 ms (the table is virtualised; scrolling only ever asks for one page) |
| Browse page sorted by retrievability (`prop:r<0.95`) | 134 ms |
| Study answer round-trip (`answerCard`, engine side, mean of 50) | 0.4 ms |
| Start of a study session on 100,000 cards | 34 ms |
| Statistics for the whole collection | 847 ms |
| Export of the whole collection to `.apkg` | 1.7 s |

Reading the 590-card fixture package takes 7 ms.

## Development

```sh
./verify            # the release gate: fmt, clippy, cargo test, frontend typecheck + vitest, Tauri build, e2e
./verify --quick    # fmt, clippy, cargo test, frontend typecheck + vitest
cargo run -p loci-cli -- import deck.apkg --dir .loci-dev && cargo run -p loci-cli -- due --dir .loci-dev
cd apps/loci && pnpm tauri dev                       # desktop app with hot reload
cd apps/loci && pnpm tauri android build --apk       # needs ANDROID_HOME, NDK_HOME, JAVA_HOME
```

Tests: `cargo test --workspace` (the engine, ported from the TypeScript suite), `pnpm test` in `apps/loci` (UI helpers), `pnpm test:e2e` (Playwright on the system Chrome against `loci serve`). Fixture decks are synthetic (`fixtures/`).

CI runs `verify` on every push (Linux). Tags `v*` build and attach Linux (AppImage, deb, rpm), Windows (msi, exe), macOS (dmg for Apple silicon and Intel) and Android (arm64 and universal APK, signed) to a GitHub release.

## Documents

`PLAN.md` (how the port was done), `DESIGN.md` (the design contract), `docs/DATA-SAFETY.md`, `docs/SEARCH.md`, `docs/SYNC.md`, `docs/DEFERRED.md`, `research/` (Anki format, FSRS spec, the complaints C1–C29 this app answers).

## Licence

MIT.
