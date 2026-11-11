# Security

Report vulnerabilities privately through [GitHub's vulnerability reporting](https://github.com/ZahakJ/loci-rs/security/advisories/new), not in a public issue. Include the Loci version and platform, the steps or a minimal package that triggers the problem, and the impact.

## Scope

Loci has no server or account. The attack surface is its input: Anki packages, card HTML and media.

- Card content is sanitised with DOMPurify and rendered in a Shadow DOM; scripts in cards do not run.
- Field HTML shown in the note editor (from imported decks, synced collections or a paste) goes through the same DOMPurify allowlist, minus buttons and ids, before it reaches the page.
- The desktop and Android app run under a Content-Security-Policy (`src-tauri/tauri.conf.json`): the app's own scripts only, no plugins or frames, and network access limited to images and media. The e2e suite runs the production build under the same policy.
- Folder sync touches only `<folder>/loci-sync/` inside the folder the user picked in the native dialog; the shell remembers that folder itself and refuses any other, and does not follow links inside it.
- Malformed packages (not a zip, not SQLite, truncated, a schema too new) are rejected before anything is written; `fixtures/broken-*` cover each case.
- Every write is a transaction; the collection is integrity-checked on open and backed up before a restore.

## Supported versions

The latest release. The macOS build is not notarised; the Android APK is signed with the project's key.
