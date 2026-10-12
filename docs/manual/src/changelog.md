# Changelog

## v0.2.0 — 11 October 2026

A design release: the same engine and data, a finished interface. Builds carry the same names with `0.2.0` (`Loci_0.2.0_amd64.AppImage`, `loci-v0.2.0-android-arm64.apk` and so on) on the [release page](https://github.com/ZahakJ/loci-rs/releases/tag/v0.2.0). Collections from 0.1.0 open unchanged.

- **No native widgets.** Selects, radios, checkboxes, sliders and tooltips are designed controls in both themes; every `title=` tooltip is gone.
- **Type and numerals.** Inter for the interface, Newsreader and Noto Naskh Arabic for cards, JetBrains Mono tabular numerals for counts, intervals and figures; one Arabic stack everywhere.
- **Dark mode is native.** The window's title bar, file dialogs and (on macOS) menus follow the in-app theme, or the OS while the theme is "System". Primary buttons in dark are a brass block on slate. Every colour pair is checked for WCAG AA by `pnpm contrast` in both themes ([results](https://github.com/ZahakJ/loci-rs/blob/main/docs/design/contrast-0.2.md)).
- **Study screen.** A sheet of paper on a desk, sized by its content; counts never shown as dashes; intervals in mono; keycap hints fade after the first answers; better proportions on phones.
- **Command palette** (Ctrl/Cmd+K, or *Commands* in the sidebar): every place, every action, and the theme.
- **macOS menu bar** with Edit (so copy and paste work as expected), View, Window and Help. Linux and Windows keep the app's own chrome, without a menu strip.
- **The window remembers** its size, position and maximised state.
- **Icons.** The Loci mark on every platform, including the Android launcher (0.1.0 shipped Tauri's default there) as an adaptive icon; the Android window background matches the theme, so launch no longer flashes white.
- **Browse and statistics.** A slate selection with a brass rule, a quieter bulk bar; charts drawn in brass and slate, with grade colours only where a mark is a grade.
- **First run** links the manual for people new to spaced repetition.

## v0.1.0 — 11 October 2026

The first release of the Rust edition, published on GitHub on 11 October 2026 (UTC). The release page carries the binaries; the notes on it point to the README for install lines, so this entry is drawn from the README and the commit history.

**Builds.** Native apps for Linux (`Loci_0.1.0_amd64.AppImage`, `Loci_0.1.0_amd64.deb`, `Loci-0.1.0-1.x86_64.rpm`), Windows (`Loci_0.1.0_x64_en-US.msi`, `Loci_0.1.0_x64-setup.exe`), macOS (`Loci_0.1.0_aarch64.dmg` for Apple silicon, `Loci_0.1.0_x64.dmg` for Intel; not notarised) and Android 7.0 or newer (`loci-v0.1.0-android-arm64.apk`, `loci-v0.1.0-android-universal.apk`, signed). All are on the [release page](https://github.com/ZahakJ/loci-rs/releases/tag/v0.1.0).

**What it contains.**

- A Rust engine (`loci-core`) ported from the TypeScript Loci: the SQLite collection with Anki's columns and units, undo, settings and presets.
- The scheduler: FSRS-6 ported by hand from fsrs-rs v6.6.2 and verified against its spec vectors, Anki's SM-2 per preset, learning steps, the queue, Anki's fuzz with Anki's exact PRNG, the optimiser and the workload simulation.
- Anki packages: reading all three layouts and `.colpkg`, writing the legacy `.apkg` layout, deck options, media name normalisation, and the bundled sample deck.
- The template engine (including cloze, image occlusion, typed answers and maths), Anki's search syntax, backup rotation, the integrity check, and the Loci full-backup format, so the TypeScript app's backups import unchanged.
- Study, card rendering, catch-up (backlog), Browse, import and export, optimisation, statistics with insights, and the RPC dispatch.
- The Tauri 2 app with the React interface ported from the TypeScript Loci, Android release signing, and a cold-start ready hook.
- The `loci` CLI: `import`, `export`, `due`, `study`, `stats`, `backup`, `integrity` and `serve`.
- The `./verify` release gate, CI on every push, and the release workflow for all four platforms.
- Screenshots of every main screen, captured from the real engine, and the performance numbers in the [performance record](performance.md).

**After the tag** (on `main`, not in the v0.1.0 binaries): the performance test finds its large package through `LOCI_PERF_APKG` or `fixtures/large-100k.apkg`, with no machine-specific default.
