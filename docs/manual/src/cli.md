# The CLI

`loci` is the engine on the command line. It works on a collection directory with the same layout the app uses (`collection.sqlite`, `media/`, `backups/`), which makes it useful for scripting, for inspecting a collection, and for development. It lives in `crates/loci-cli`.

Build and run it from a checkout:

```sh
cargo run -p loci-cli -- import deck.apkg --dir .loci-dev
cargo run -p loci-cli -- due --dir .loci-dev
```

or build the binary once (`cargo build -p loci-cli`, which writes `target/debug/loci`) and call it directly. The examples below write `loci` for either.

## Global options

| Option | Meaning |
|---|---|
| `--dir <dir>` | The collection directory. Default `./.loci-dev`. Created on first use. |
| `--json` | Print the full result as pretty-printed JSON instead of the human summary. |
| `--help`, or `help` as the command | Print the usage text. |

Options can come before or after the command, and `--name=value` works as well as `--name value`.

To work on the app's own collection, point `--dir` at the app-data directory from [Getting started](getting-started.md#where-your-data-lives), with the app closed so only one program writes to the file.

Before every command except `integrity` and `serve`, the CLI opens the collection and runs its integrity check. If the check fails, it prints the problems, suggests `integrity` or a backup, and exits with status 2.

## import

```text
loci import <file> [--replace] [--no-scheduling]
```

Imports an Anki `.apkg` or `.colpkg`, or a Loci full-backup `.zip`. By default it merges by note guid, and an existing note is replaced only if the incoming one was edited more recently.

- `--replace` replaces the whole collection instead of merging (a `pre-restore` backup is written first). A Loci full backup must be imported with `--replace`; it cannot be merged.
- `--no-scheduling` imports the cards as new, without due dates or review history.

```sh
loci import ~/Downloads/collection.colpkg --dir ~/loci-test
```

It prints what was added: notes (with how many were updated or skipped), cards, reviews, media files, decks, the time taken, and any warnings. On a terminal it also shows progress on standard error.

## export

```text
loci export <out.apkg> [--deck <name>] [--no-scheduling] [--no-media]
```

Writes a legacy `.apkg` that Anki 2.1 to 26.x and AnkiDroid import.

- `--deck <name>` exports one deck with its subdecks. The name is the full name, with `::` between levels. Without it, the whole collection is exported.
- `--no-scheduling` exports every card as new, for sharing.
- `--no-media` leaves the media files out.

```sh
loci export arabic.apkg --deck "Languages::Arabic" --no-scheduling
```

## due

```text
loci due
```

Prints the deck tree with today's new, learning and due counts and an estimate in minutes for each deck, then the totals.

```sh
loci due --dir ~/loci-test
```

## study

```text
loci study --auto <N> [--deck <name>]
```

Answers up to N due cards with Good, each with an answer time of 8 seconds, and prints each card's id with the interval it was given. It is meant for testing and for quick experiments with the scheduler, not for learning. `--auto` is required and must be at least 1. `--deck` limits the session to one deck; without it, the session covers every deck. If fewer than N cards are due, it stops and says so.

```sh
loci study --auto 5 --deck Default
```

## stats

```text
loci stats [--deck <name>]
```

Prints today's answers, time and accuracy; card counts (new, learning, young, mature, suspended, buried); true retention by period; the due counts for the next seven days; momentum (days studied of the last 30, the current run and the longest); and the insights.

```sh
loci stats --json > stats.json
```

## backup

```text
loci backup
```

Writes a manual backup into `<dir>/backups/` and prints its name, size and card count. Manual backups are never deleted by rotation.

```sh
loci backup --dir ~/loci-test
```

## integrity

```text
loci integrity
```

Runs the full integrity check and repairs what is safe to repair (see [Import, export and backups](import-export-backups.md#the-integrity-check)). It prints `OK`, with a list of anything repaired, or the problems it found. It exits with status 1 when problems remain.

```sh
loci integrity --dir ~/loci-test
```

## serve

```text
loci serve [--port 5741] [--multi-session]
```

Serves the engine's RPC over HTTP on `127.0.0.1` (port 5741 by default). It exists for development and for the end-to-end tests: it lets the web interface run in an ordinary browser against the real Rust engine. It is not shipped in the app.

| Endpoint | Body | Returns |
|---|---|---|
| `POST /rpc` | `{"method", "args"}` | the JSON result, or `{"error": {code, message}}` |
| `POST /rpc-bytes` | `{"method", "args"}` | raw bytes (404 when the result is null) |
| `POST /rpc-with-bytes?method=…&args=…` | raw bytes | the JSON result, for methods that take bytes (`previewImport`, `importPackage`, `addMedia`) |
| `GET /health` | | `{"ok": true}` |

With `--multi-session`, every distinct `x-loci-session` header gets its own empty collection in a temporary directory, which is what the Playwright suite needs (each browser context starts fresh). Without it, one engine serves `--dir`. Requests are handled one at a time.

```sh
loci serve --dir .loci-dev
```

Then, in `apps/loci`, `pnpm dev` serves the interface on port 5740 and talks to it.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Success |
| 1 | An error (printed as `loci: <message>`), or `integrity` found problems |
| 2 | The collection failed its integrity check when opened |
| 64 | Unknown command |
