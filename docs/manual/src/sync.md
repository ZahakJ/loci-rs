# Sync plan

Loci ships local-only. Sync is designed but not built: the schema already carries the hooks it needs (`usn` and `mtime` on every row, deletions recorded in `graves`), because sync was designed in from the start so that the first version would not paint the project into a corner. The design below is the repository's `docs/SYNC.md`, included as it stands.

{{#include ../../SYNC.md:2:}}
