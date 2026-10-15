//! Loci core: the spaced-repetition engine behind the Loci apps and CLI.
//!
//! Module map (see PLAN.md §1): `types` and `error` are the contract; `db`/`blobs` are storage;
//! `scheduler`, `anki`, `template`, `search`, `stats`, `backup`, `undo` are pure logic; the
//! `collection` module ties them into the `Collection` service; `rpc` exposes it as JSON.
pub mod anki;
pub mod backup;
pub mod blobs;
pub mod collection;
pub mod db;
pub mod error;
pub mod i18n;
pub mod rpc;
pub mod scheduler;
pub mod search;
pub mod stats;
pub mod sync;
pub mod template;
pub mod text;
pub mod types;
pub mod undo;

pub use error::{ErrorCode, LociError, Result};

/// The collection schema (version 1), unchanged from the TypeScript build.
pub const SCHEMA_SQL: &str = include_str!("schema.sql");
