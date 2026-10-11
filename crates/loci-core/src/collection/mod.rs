//! The `Collection` service and its sibling modules (recall `src/core/collection/`).
//!
//! `internals::CollectionCore` is the shared synchronous core; the phase-2 modules
//! (decks, notetypes, notes, cards, media, study, render, backlog, browse, import, export,
//! optimise, full_backup, sched_context, builtin_notetypes) and the `Collection` service build on it.
pub mod internals;
pub mod presets;
pub mod rows;
pub mod schema;
pub mod settings;
pub mod time;
