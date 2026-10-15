//! The `Collection` service and its sibling modules.
//!
//! `internals::CollectionCore` is the shared synchronous core; the other modules
//! (decks, notetypes, notes, cards, media, study, render, backlog, browse, import, export,
//! optimise, full_backup, sched_context, builtin_notetypes) and the `Collection` service build on it.
pub mod backlog;
pub mod browse;
pub mod builtin_notetypes;
pub mod cards;
pub mod counter;
pub mod decks;
pub mod export;
pub mod full_backup;
pub mod import;
pub mod internals;
pub mod media;
pub mod notes;
pub mod notetypes;
pub mod optimise;
pub mod presets;
pub mod render;
pub mod reschedule;
pub mod rows;
pub mod sched_context;
pub mod schema;
pub mod service;
pub mod service_backups;
pub mod settings;
pub mod study;
pub mod time;

pub use service::Collection;
