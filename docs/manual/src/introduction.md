# Introduction

Loci is a local-first spaced-repetition app. It imports your Anki collection with its history intact, schedules reviews with FSRS-6, and runs as one app on Linux, Windows, macOS and Android. The name comes from the *method of loci*, the memory palace of Simonides and Cicero.

This manual is for two readers: someone moving their cards over from Anki, and someone who wants to read or change the code. The [project page](../) has the downloads and screenshots; the source is at [github.com/ZahakJ/loci-rs](https://github.com/ZahakJ/loci-rs).

<img class="capture" src="../captures/home-desktop-light.png" alt="The Home screen in the light theme: today's time estimate, an insights strip, the deck tree with new, learning and due counts, and the momentum panel">

## What Loci is

**A real Anki replacement.** Loci reads `.apkg` and `.colpkg` files from every Anki version, legacy and current package layouts alike, with notes, note types, templates, media, tags, decks, review history and scheduling state. A card due tomorrow in Anki is due tomorrow in Loci. It exports back to an `.apkg` that Anki 2.1 through 26.x and AnkiDroid import, so nobody is locked in.

**FSRS-6 by default.** The scheduler is FSRS-6, ported from the reference implementation (fsrs-rs v6.6.2) and tested against its published vectors. Its 21 parameters can be fitted to your own review history, per preset, by an optimiser ported from the same source. Anki's SM-2 is still there, one switch away in each preset. The [FSRS chapter](fsrs.md) explains the model from the ground up.

**One app for desktop and phone.** One Rust engine and one interface build native apps for Linux, Windows, macOS and Android. The study screen is designed around the keyboard on a desk and the thumb on a phone. The light and dark themes are each designed rather than one being an inversion of the other, and the card typography handles Arabic, mathematics and code.

**Humane about workload.** A daily time budget in minutes replaces card caps. A catch-up mode turns a backlog into a plan instead of a wall of red. Changing new cards per day or desired retention shows a workload forecast first. Momentum (days studied in the last 30) replaces streaks, so a missed day is shown and nothing resets.

**Data you own.** The collection is a real SQLite file in your app-data folder. Loci keeps rotating automatic backups, checks the database's integrity when it opens, and writes a full-backup zip (collection plus media) that it can restore. There is no account and no server; nothing leaves the device.

## Where Loci came from

Loci started as a TypeScript web app (ZahakJ/loci). This repository is the Rust rewrite: everything that is logic lives in the `loci-core` crate, and the interface is the same React interface the web app had, running in a Tauri webview. The web app's full-backup zip imports here unchanged, so moving over loses nothing. The [data model chapter](data-model.md) describes that migration path.

## How to read this manual

- To start using Loci, read [Getting started](getting-started.md), then [Import, export and backups](import-export-backups.md).
- To understand why a card was scheduled when it was, read [FSRS-6, from first principles](fsrs.md).
- To find cards, see [Search syntax](search.md).
- To script against a collection, see [The CLI](cli.md).
- To work on the code, read [Architecture for contributors](architecture.md).

Loci is free software under the MIT licence.
