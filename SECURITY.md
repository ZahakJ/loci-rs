# Security

Report vulnerabilities privately through [GitHub's vulnerability reporting](https://github.com/ZahakJ/loci-rs/security/advisories/new), not in a public issue. Include the Loci version and platform, the steps or a minimal package that triggers the problem, and the impact.

## Scope

Loci has no server or account. The attack surface is its input: Anki packages, card HTML and media.

- Card content is sanitised with DOMPurify and rendered in a Shadow DOM; scripts in cards do not run.
- Malformed packages (not a zip, not SQLite, truncated, a schema too new) are rejected before anything is written; `fixtures/broken-*` cover each case.
- Every write is a transaction; the collection is integrity-checked on open and backed up before a restore.

## Supported versions

The latest release. The macOS build is not notarised; the Android APK is signed with the project's key.
