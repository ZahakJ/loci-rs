# Introduction

Loci is a spaced-repetition app for Linux, Windows, macOS, Android and the [browser](browser.md). It imports Anki collections with their review history, schedules with FSRS-6 or SM-2, and exports back to `.apkg`. Downloads are on the [project page](../); the source is at [github.com/ZahakJ/loci-rs](https://github.com/ZahakJ/loci-rs).

<img class="capture" src="../captures/home-desktop-light.png" alt="The Home screen in the light theme: today's time estimate, an insights strip, the deck tree with new, learning and due counts, and the momentum panel">

## Features

- **Anki import and export.** `.apkg` and `.colpkg` in every package layout: notes, note types, templates, media, tags, decks, review history and scheduling state. Due dates carry over. Exports `.apkg` for Anki 2.1 through 26.x and AnkiDroid.
- **FSRS-6.** 21 parameters, fitted per preset to your reviews by the optimiser. SM-2 per preset. See [FSRS-6](fsrs.md).
- **Workload.** A daily time budget instead of card caps, catch-up for backlogs, a 90-day forecast before changing new cards per day or desired retention, and days studied in the last 30 instead of streaks.
- **Storage.** One SQLite file in the app-data directory, rotating automatic backups, an integrity check on open, and a full-backup zip. No account.
- **Sync.** Through a shared folder or a sync file; no server. See [Sync](sync.md).
- **Cards.** Light and dark themes; Arabic, maths (KaTeX) and code render in cards.

## Chapters

- Using Loci: [Getting started](getting-started.md), [Loci in the browser](browser.md), [Sync](sync.md), [Import, export and backups](import-export-backups.md), [Search syntax](search.md).
- Scheduling: [FSRS-6](fsrs.md).
- Scripting: [The CLI](cli.md).
- Code: [Architecture](architecture.md), [The data model](data-model.md).

MIT licence.
