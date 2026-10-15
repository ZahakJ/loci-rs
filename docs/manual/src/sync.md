# Sync

Keep the same cards, reviews and settings on your computer and your phone. There is no account
and no server: your devices exchange **sync files**.

## With a shared folder (desktop, and browsers that can open folders)

1. Pick a folder that your devices already share: a Syncthing, Dropbox, OneDrive or Google Drive
   folder, or a network drive.
2. In **Settings › Sync**, choose **Choose folder** and select it.
3. Do the same on your other computers.

Loci syncs with the folder when it opens, when it closes and after each study session.
**Sync now** does it at any time. Loci keeps its files in a `loci-sync` folder inside the one
you chose.

## With a sync file (any device, including Android)

1. On the device that is ahead, choose **Settings › Sync › Save sync file**.
2. Move the file to the other device (share sheet, cable, email, a shared folder).
3. There, choose **Open sync file** and pick it.

Do it the other way round as well when both devices have new reviews.

## Starting on a new device

Install Loci and open the sync file or choose the sync folder before adding cards. The new
device takes your decks, presets and settings.

## What happens when both devices changed something

- Reviews from every device are kept.
- A card's schedule comes from its most recent review.
- When the same note, deck or setting changed on two devices, the later change is kept.
- A deletion and an edit: whichever happened later wins.
- Two decks with the same name: the newer one becomes "Name (2)".
- Two different images with the same name: both are kept; the older one gets a longer name.

Before a sync changes anything, Loci writes an automatic backup (**Settings › Backups**).

## How it works

The rest of this page is the repository's `docs/SYNC.md`.

{{#include ../../SYNC.md:2:}}
