# Getting started

This chapter takes you from download to a first study session: install, the first-run screen, bringing your Anki collection over, studying, adding notes, and where the files end up.

## Install

Every build is attached to the [latest release](https://github.com/ZahakJ/loci-rs/releases/latest). Download one file for your platform, or paste the matching line. The lines below are for v0.2.0, the current release.

**Linux.** The AppImage runs on most distributions without installation. A `.deb` (`Loci_0.2.0_amd64.deb`) and an `.rpm` (`Loci-0.2.0-1.x86_64.rpm`) are on the release page too.

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.2.0/Loci_0.2.0_amd64.AppImage && chmod +x Loci_0.2.0_amd64.AppImage && ./Loci_0.2.0_amd64.AppImage
```

**Windows.** In PowerShell, this downloads and runs the `.msi` installer. The other installer, `Loci_0.2.0_x64-setup.exe`, does the same job.

```powershell
Invoke-WebRequest https://github.com/ZahakJ/loci-rs/releases/download/v0.2.0/Loci_0.2.0_x64_en-US.msi -OutFile Loci.msi; Start-Process .\Loci.msi
```

**macOS.** The line below is for Apple silicon (`Loci_0.2.0_aarch64.dmg`). For an Intel Mac, replace `aarch64` with `x64` (`Loci_0.2.0_x64.dmg`).

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.2.0/Loci_0.2.0_aarch64.dmg && open Loci_0.2.0_aarch64.dmg
```

The macOS build is not notarised. After dragging Loci to Applications, run this once, or right-click the app and choose Open:

```sh
xattr -d com.apple.quarantine /Applications/Loci.app
```

**Android.** The line below installs the arm64 APK, which is the one for arm64 phones; `loci-v0.2.0-android-universal.apk` on the release page contains every ABI. With the phone connected and `adb` available:

```sh
curl -LO https://github.com/ZahakJ/loci-rs/releases/download/v0.2.0/loci-v0.2.0-android-arm64.apk && adb install loci-v0.2.0-android-arm64.apk
```

Or open the APK link on the phone itself and allow installs from your browser. Loci needs Android 7.0 (API 24) or newer.

**From source.** The Rust toolchain is pinned in `rust-toolchain.toml` (1.99.0); the frontend needs Node and pnpm, and Linux needs the WebKitGTK libraries Tauri builds against.

```sh
git clone git@github.com:ZahakJ/loci-rs.git && cd loci-rs/apps/loci && pnpm install && pnpm tauri build
```

## The first-run screen

An empty collection opens on a single screen with three choices and no account and no tour:

- **Import from Anki.** A drop zone for an `.apkg` or `.colpkg`. Your reviews come with the cards and stay due on the same days.
- **Try a sample deck.** Thirty bundled cards about memory itself, with cloze, image, maths and Arabic cards, so you can feel a session before committing to anything.
- **Start empty.** Name a first deck and add cards as you go.

## Bringing your Anki collection over

In Anki, use *File › Export* and choose either *Anki Collection Package (.colpkg)*, which is the whole collection, or *Anki Deck Package (.apkg)* for one deck. Drop the file on Loci's import screen (or on the first-run drop zone).

Loci reads the whole package first and shows a preview: notes, cards, decks, reviews, media, and how many cards carry FSRS memory state. Nothing is written yet. Three choices sit under the preview:

- **Destination.** *Add to my collection* merges by Anki's note ID, so importing the same deck twice does not duplicate it. *Replace my collection* swaps the whole collection; the current one is backed up first.
- **When a note already exists.** *Newest edit wins* (recommended), *Keep mine*, or *Use the imported version*.
- **Keep scheduling.** On by default. Off resets the imported cards to new, which is the right choice for a shared deck you want to learn from scratch.

What comes across with scheduling kept:

- **Due dates.** Review due days are shifted by the difference between the two collections' creation days, so every card falls due on the same calendar day it would have in Anki.
- **Review history.** Every review-log row of the imported cards.
- **FSRS state.** The stability and difficulty Anki stored for each card are kept verbatim, so intervals continue exactly where Anki left them.
- **Media.** Every file in the package, with Anki's filename normalisation.

Cards sitting in an Anki filtered deck return to their home deck, because Loci has no persistent filtered decks; *Custom session* covers the same uses. If any part of the package cannot be read, nothing is imported and the error says why. The [import chapter](import-export-backups.md) has the full rules.

## Studying

<img class="capture" src="../captures/study-back-desktop-light.png" alt="The study screen in the light theme with the answer revealed and four fixed buttons along the bottom: Again, Hard, Good and Easy, each showing the interval it would give">

Choose a deck on Home and press Start. The card's front appears; press space or enter (or tap *Show answer*) to reveal the back. Four buttons then appear, always in the same order and the same place:

| Key | Button | Meaning |
|---|---|---|
| `1` | Again | You did not remember. |
| `2` | Hard | You remembered, with effort. |
| `3` | Good | You remembered. Space and enter also mean Good. |
| `4` | Easy | You remembered without effort. |

Each button shows the interval it would give before you press it. Hard is a pass, not a fail: if you did not remember, the honest answer is Again.

The other study keys:

| Key | Action |
|---|---|
| `z`, `u` or `ctrl+z` | Undo the last answer |
| `e` or `o` | Edit this note |
| `s` or `@` | Suspend the card |
| `!` | Suspend the note (all its cards) |
| `b` or `-` | Bury the card until tomorrow |
| `=` | Bury the note until tomorrow |
| `f` | Toggle the flag |
| `ctrl+1` … `ctrl+7` | Set a flag colour |
| `m` or `*` | Mark the note (adds the tag `marked`) |
| `ctrl+shift+d` | Set the due date |
| `ctrl+delete` | Delete the note |
| `i` | Why this interval? |
| `r` | Replay audio |
| `t` | Focus the typed-answer box |
| `?` | Show every shortcut |
| `esc` | Leave the session |

**Why this interval?** Press `i` on any card. The panel shows the chance you would recall it right now, its stability (the number of days until that chance falls to 90%), its difficulty from 1 to 10, its due date and interval, its reviews and lapses, and its history. The [FSRS chapter](fsrs.md) explains where each number comes from.

A session ends with a quiet summary: cards, minutes, accuracy, roughly how many cards tomorrow brings, and one insight if there is one.

## Adding notes

Press `a` from any screen outside a study session to open the Add sheet: a side sheet on desktop, a full screen on a phone. Choose a note type and a deck (both remember your last choice), fill in the fields, and press `ctrl+enter` to add. Focus returns to the first field, ready for the next note. Fields marked sticky keep their value between notes, and a duplicate first field is flagged. On desktop a live preview of the cards sits under the fields.

The built-in note types are *Basic*, *Basic (and reversed card)*, *Basic (type in the answer)*, *Cloze* and *Image Occlusion*; imported Anki note types work as they did in Anki.

**Cloze.** Select text and press `ctrl+shift+c` to wrap it in the next cloze number, or `ctrl+alt+shift+c` to reuse the current number (two gaps on the same card). Nested and overlapping clozes work as in Anki.

**Image occlusion.** Paste or choose an image, then draw rectangles, ellipses or polygons over the parts to hide, and choose *Hide all, guess one* or *Hide one, guess one*. The shapes are stored in Anki's own image-occlusion format, so the notes survive a trip through Anki unchanged.

Other editor keys: `ctrl+shift+x` toggles the HTML source and `ctrl+m` inserts maths.

## Where your data lives

Everything lives in the platform app-data directory:

| Platform | Directory |
|---|---|
| Linux | `~/.local/share/space.avicenna.loci` |
| Windows | `%APPDATA%\space.avicenna.loci` |
| macOS | `~/Library/Application Support/space.avicenna.loci` |
| Android | the app's internal storage |

Inside it are `collection.sqlite` (the whole collection in one SQLite file), `media/` (images and sounds as plain files), and `backups/` (rotating copies: 12 daily, 10 weekly and 9 monthly). Copying that directory copies everything.
