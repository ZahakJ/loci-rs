# Security

Loci is a local application: no account, no server, nothing leaves the device. The surface that matters is what it reads: Anki packages, card HTML and media from decks you did not write.

## Reporting a vulnerability

Please report privately through [GitHub's vulnerability reporting](https://github.com/ZahakJ/loci-rs/security/advisories/new) rather than in a public issue. Include the Loci version and platform, the steps or a minimal package that triggers the problem, and what an attacker gains. You will get an acknowledgement within a week and a fix or a reasoned answer as soon as one exists. There is no bounty programme.

## What is defended

- Card content is sanitised with DOMPurify and rendered in a Shadow DOM; scripts in cards do not run.
- Anki packages are read by explicit layout; a malformed package (not a zip, not SQLite, truncated, a schema that is too new) is rejected before anything is written, and the broken fixtures in `fixtures/` keep it that way.
- Every write is a transaction; the collection is integrity-checked on open and backed up before a restore.

## Supported versions

The latest release only. Releases are built by the repository's workflow; the macOS build is not notarised and the Android APK is signed with the project's own key.
