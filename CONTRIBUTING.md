# Contributing

- Read `docs/internals.md` and the manual's [architecture chapter](https://zahakj.github.io/loci-rs/manual/architecture.html). Deferred features have sketches in `docs/DEFERRED.md`; start from the sketch.
- Open an issue before anything larger than a fix.

## Setup

```sh
git clone https://github.com/ZahakJ/loci-rs && cd loci-rs
cargo test --workspace                       # engine
cd apps/loci && pnpm install && pnpm test    # UI helpers
pnpm tauri dev                               # app with hot reload
```

`./verify --quick` runs fmt, clippy, the Rust tests and the frontend checks; `./verify` adds the Tauri build and Playwright and is the release gate.

## Rules

- Logic lives in `crates/loci-core`. The UI never touches SQL; it calls the engine through the bridge.
- Every write path is a transaction. An operation that changes more than one card records an undo entry.
- Times follow Anki's units (see `types.rs`).
- Behaviour comes with a test: scheduler changes against the FSRS vectors, importer changes against `fixtures/`, UI changes in vitest or Playwright.
- Fixture decks are synthetic. No real collections in the repository.
- British spelling (optimise, colour, licence).

## Pull requests

One change per pull request, template filled in. Imperative commit messages.

Security problems: see `SECURITY.md`.
