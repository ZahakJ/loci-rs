# Contributing to Loci

Thank you for looking. Loci is small enough that one person can hold it in their head, and the aim is to keep it that way.

## Before you start

- Read `DESIGN.md`. It is the contract the code is built against; if code and contract disagree, fix one and say which in the pull request.
- Read the [manual](https://zahakj.github.io/loci-rs/manual/), especially *Architecture for contributors* and *Roadmap*. Several features are deferred on purpose with a design sketch; building one of those is welcome, but start from the sketch.
- For anything larger than a fix, open an issue first so the design can be agreed before the work.

## Setting up

```sh
git clone https://github.com/ZahakJ/loci-rs.git && cd loci-rs
cargo test --workspace                 # the engine
cd apps/loci && pnpm install && pnpm test   # the interface
pnpm tauri dev                          # the desktop app with hot reload
```

`./verify --quick` runs fmt, clippy, the Rust tests and the frontend checks; `./verify` adds the Tauri build and the Playwright flows and is the release gate. CI runs `verify` on every push.

## Rules of the code

- Logic lives in `crates/loci-core`. The interface never touches SQL; it calls the engine through the bridge.
- Every write path is a transaction. Any operation that changes more than one card records an undo entry.
- Times follow Anki's units (see `types.rs`), so import stays a mapping, not a translation.
- Behaviour comes with a test: scheduler changes against the FSRS vectors, importer changes against the fixture packages in `fixtures/`, interface changes in vitest or a Playwright flow.
- Fixture decks are synthetic. Never add a real collection to the repository.

## Pull requests

One change per pull request, with the template filled in. Commit messages say what changed and why, in the imperative. Keep the British spelling the project uses (optimise, colour, licence).

## Reporting problems

Bugs and feature requests go through the issue templates. Security problems go through [private vulnerability reporting](https://github.com/ZahakJ/loci-rs/security/advisories/new); see `SECURITY.md`.
