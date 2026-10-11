//! JSON method dispatch mirroring the TypeScript worker protocol (recall `src/worker/protocol.ts`).
//!
//! `Engine` is the one seam the Tauri shell, the CLI's `serve` bridge and the tests use:
//!
//! ```text
//! let mut e = Engine::open(dir)?;                       // opens <dir>/collection.sqlite, media/, backups/
//! e.dispatch("deckTree", &[], None, &mut |_| {})?;      // -> Payload::Json
//! e.dispatch("getMedia", &[json!("a.png")], None, ..)?; // -> Payload::Bytes(Option<Vec<u8>>)
//! e.dispatch("importPackage", &[json!(opts)], Some(&bytes), &mut |p| ...)?;
//! ```
//!
//! Method names and argument order are exactly those of the TypeScript `CollectionApi`
//! (`method(args...)` becomes `dispatch(method, &args)`), with byte-valued arguments removed from
//! `args` and passed as `bytes` (`previewImport(bytes)`, `importPackage(bytes, options)`,
//! `addMedia(name, bytes)`), and byte-valued results returned as `Payload::Bytes`
//! (`getMedia`, `exportPackage`, `exportFullBackup`). Progress callbacks become the `progress` sink.
//!
//! PHASE-2 STUB: `Engine` here only records the directory and answers every method with
//! `ErrorCode::Internal` ("engine not built yet"). The collection agent replaces the body of this
//! file with the real dispatcher over `collection::Collection`, keeping every `pub` item below.

use crate::error::{LociError, Result};
use crate::types::Progress;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A method result: JSON, or raw bytes (None = the TypeScript `null`).
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Json(Value),
    Bytes(Option<Vec<u8>>),
}

impl Payload {
    pub fn into_json(self) -> Result<Value> {
        match self {
            Payload::Json(v) => Ok(v),
            Payload::Bytes(_) => Err(LociError::internal("Expected a JSON result, got bytes")),
        }
    }
    pub fn into_bytes(self) -> Result<Option<Vec<u8>>> {
        match self {
            Payload::Bytes(b) => Ok(b),
            Payload::Json(_) => Err(LociError::internal("Expected bytes, got a JSON result")),
        }
    }
}

/// Every method the engine serves: the whole `CollectionApi` plus `backupOnLeave` and `autoBackup`.
pub const METHODS: &[&str] = &[
    "addMedia", "addNote", "allTags", "answerCard", "applyCatchUp", "backlogReport", "backupNow", "browse", "bulk", "buryCards",
    "checkIntegrity", "checkMedia", "checkTemplate", "close", "createDeck", "deckTree", "deleteDeck", "deleteMedia", "deleteNotes",
    "deleteNotetype", "deletePreset", "deleteSavedSearch", "endStudy", "evaluate", "explainCard", "exportFullBackup", "exportPackage",
    "extendTodayLimits", "findDuplicates", "forecastWorkload", "getCard", "getCardsOfNote", "getDeck", "getMedia", "getNote",
    "getNotetype", "getPreset", "getSettings", "importPackage", "insights", "installSampleDeck", "isEmpty", "listBackups",
    "listNotetypes", "listPresets", "maintenance", "nextCard", "open", "optimise", "previewImport", "previewNote", "renameDeck",
    "renameTag", "renderCard", "resetCollection", "resolveLeech", "restoreBackup", "saveNotetype", "savePreset", "saveSearch",
    "searchIds", "setDeckCollapsed", "setDeckDescription", "setDeckPreset", "setFlag", "setSettings", "sidebar", "startCustomStudy",
    "startStudy", "stats", "suspendCards", "today", "unburyDeck", "undo", "undoLabel", "updateNote", "backupOnLeave", "autoBackup",
];

/// Methods whose byte argument arrives through `bytes`.
pub const TAKES_BYTES: &[&str] = &["previewImport", "importPackage", "addMedia"];
/// Methods that return `Payload::Bytes`.
pub const RETURNS_BYTES: &[&str] = &["getMedia", "exportPackage", "exportFullBackup"];

pub fn is_method(name: &str) -> bool {
    METHODS.contains(&name)
}
pub fn takes_bytes(name: &str) -> bool {
    TAKES_BYTES.contains(&name)
}
pub fn returns_bytes(name: &str) -> bool {
    RETURNS_BYTES.contains(&name)
}

/// Methods that write (the shell may schedule a backup check after them).
pub fn is_write(name: &str) -> bool {
    !matches!(
        name,
        "allTags" | "backlogReport" | "browse" | "checkTemplate" | "deckTree" | "evaluate" | "explainCard" | "exportFullBackup"
            | "exportPackage" | "findDuplicates" | "forecastWorkload" | "getCard" | "getCardsOfNote" | "getDeck" | "getMedia" | "getNote"
            | "getNotetype" | "getPreset" | "getSettings" | "insights" | "isEmpty" | "listBackups" | "listNotetypes" | "listPresets"
            | "nextCard" | "open" | "previewImport" | "previewNote" | "renderCard" | "searchIds" | "sidebar" | "stats" | "today"
            | "undoLabel" | "close" | "startStudy" | "startCustomStudy" | "endStudy"
    )
}

/// The engine behind one collection directory.
pub struct Engine {
    dir: PathBuf,
}

impl Engine {
    /// Opens (creating if needed) the collection in `dir`: `collection.sqlite`, `media/`, `backups/`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Engine> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Engine { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Runs one method. See the module docs for the calling convention.
    pub fn dispatch(&mut self, method: &str, _args: &[Value], _bytes: Option<&[u8]>, _progress: &mut dyn FnMut(Progress)) -> Result<Payload> {
        if !is_method(method) {
            return Err(LociError::invalid(format!("Unknown method {method}")));
        }
        Err(LociError::internal("The engine is not built yet (phase 2 stub)"))
    }

    /// Closes the collection (flushes and releases the file).
    pub fn close(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_table_is_complete_and_sorted_enough() {
        assert_eq!(METHODS.len(), 81);
        assert!(is_method("answerCard"));
        assert!(!is_method("dropTables"));
        assert!(takes_bytes("addMedia") && returns_bytes("getMedia"));
        assert!(is_write("answerCard") && !is_write("deckTree"));
    }
}
