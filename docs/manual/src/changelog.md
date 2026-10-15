# Changelog

## v0.2.1 — 12 October 2026

- macOS: fixed "Loci is damaged and can't be opened". The bundle is ad-hoc signed (`bundle.macOS.signingIdentity: "-"`) and the release job checks it with `codesign --verify --deep --strict`. Still not notarised ([details](https://github.com/ZahakJ/loci-rs/blob/main/docs/macos-gatekeeper.md)).
- Android versionCode 2001.

## v0.2.0 — 11 October 2026

Interface release; collections from 0.1.0 open unchanged. [Release page](https://github.com/ZahakJ/loci-rs/releases/tag/v0.2.0).

- Custom selects, radios, checkboxes, sliders and tooltips in both themes; no `title=` tooltips.
- Inter for the interface, Newsreader and Noto Naskh Arabic for cards, JetBrains Mono tabular numerals for counts and intervals.
- Native dark mode: title bar, file dialogs and macOS menus follow the theme. `pnpm contrast` checks every colour pair for WCAG AA ([results](https://github.com/ZahakJ/loci-rs/blob/main/docs/design/contrast-0.2.md)).
- Study screen: the card is a sheet sized by its content; zero counts shown as `0`; keycap hints fade after the first answers; phone layout reworked.
- Command palette (Ctrl/Cmd+K, or *Commands* in the sidebar).
- macOS menu bar (Edit, View, Window, Help). No menu bar on Linux and Windows.
- The window remembers its size, position and maximised state.
- Loci icon on every platform, including an Android adaptive icon; the Android launch background follows the theme.
- Browse selection and bulk bar restyled; charts in brass and slate, grade colours only for grades.
- First run links the manual.

## v0.1.0 — 11 October 2026

First release. [Release page](https://github.com/ZahakJ/loci-rs/releases/tag/v0.1.0).

**Builds.** Linux (`Loci_0.1.0_amd64.AppImage`, `Loci_0.1.0_amd64.deb`, `Loci-0.1.0-1.x86_64.rpm`), Windows (`Loci_0.1.0_x64_en-US.msi`, `Loci_0.1.0_x64-setup.exe`), macOS (`Loci_0.1.0_aarch64.dmg` for Apple silicon, `Loci_0.1.0_x64.dmg` for Intel; not notarised) and Android 7.0 or newer (`loci-v0.1.0-android-arm64.apk`, `loci-v0.1.0-android-universal.apk`, signed).

**Contents.**

- The Rust engine (`loci-core`): the SQLite collection with Anki's columns and units, undo, settings and presets.
- The scheduler: FSRS-6 ported from fsrs-rs v6.6.2 and checked against its spec vectors, Anki's SM-2 per preset, learning steps, the queue, Anki's fuzz with Anki's exact PRNG, the optimiser and the workload simulation.
- Anki packages: reading all three layouts and `.colpkg`, writing the legacy `.apkg` layout, deck options, media name normalisation, and the bundled sample deck.
- The template engine (including cloze, image occlusion, typed answers and maths), Anki's search syntax, backup rotation, the integrity check, and the Loci full-backup format.
- Study, card rendering, catch-up (backlog), Browse, import and export, optimisation, statistics with insights, and the RPC dispatch.
- The Tauri 2 app with the React interface, and Android release signing.
- The `loci` CLI: `import`, `export`, `due`, `study`, `stats`, `backup`, `integrity` and `serve`.
- The `./verify` release gate, CI on every push, and the release workflow for all four platforms.
- Screenshots of every main screen.
