//! JSON method dispatch mirroring the TypeScript worker protocol (recall `src/worker/protocol.ts`
//! and `src/worker/db.worker.ts`).
//!
//! `Engine` is the one seam the Tauri shell, the CLI's `serve` bridge and the tests use:
//!
//! ```text
//! let mut e = Engine::open(dir)?;                       // opens <dir>/collection.sqlite, media/, backups/
//! e.dispatch("open", &[], None, &mut |_| {})?;          // -> Payload::Json(IntegrityReport)
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
//! Unknown methods and malformed arguments fail with `ErrorCode::Invalid` (the worker's
//! "unknown method" and the deserialisation errors of the TypeScript API).

use crate::collection::settings::parse_patch;
use crate::collection::Collection;
use crate::error::{LociError, Result};
use crate::sync::SyncExportOptions;
use crate::types::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
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
    "addMedia",
    "addNote",
    "allTags",
    "answerCard",
    "applyCatchUp",
    "backlogReport",
    "backupNow",
    "browse",
    "bulk",
    "buryCards",
    "checkIntegrity",
    "checkMedia",
    "checkTemplate",
    "close",
    "createDeck",
    "deckTree",
    "deleteDeck",
    "deleteMedia",
    "deleteNotes",
    "deleteNotetype",
    "deletePreset",
    "deleteSavedSearch",
    "endStudy",
    "evaluate",
    "explainCard",
    "exportFullBackup",
    "exportPackage",
    "extendTodayLimits",
    "findDuplicates",
    "forecastWorkload",
    "getCard",
    "getCardsOfNote",
    "getDeck",
    "getMedia",
    "getNote",
    "getNotetype",
    "getPreset",
    "getSettings",
    "importPackage",
    "insights",
    "installSampleDeck",
    "isEmpty",
    "listBackups",
    "listNotetypes",
    "listPresets",
    "maintenance",
    "nextCard",
    "open",
    "optimise",
    "previewImport",
    "previewNote",
    "renameDeck",
    "renameTag",
    "renderCard",
    "resetCollection",
    "resolveLeech",
    "restoreBackup",
    "saveNotetype",
    "savePreset",
    "saveSearch",
    "searchIds",
    "setDeckCollapsed",
    "setDeckDescription",
    "setDeckPreset",
    "setFlag",
    "setSettings",
    "sidebar",
    "startCustomStudy",
    "startStudy",
    "stats",
    "suspendCards",
    "today",
    "unburyDeck",
    "undo",
    "undoLabel",
    "updateNote",
    "backupOnLeave",
    "autoBackup",
    "syncStatus",
    "setSyncFolder",
    "exportSyncBundle",
    "mergeSyncBundle",
    "syncMediaState",
    "addSyncMedia",
];

/// Methods whose byte argument travels outside `args`.
pub const TAKES_BYTES: &[&str] = &["previewImport", "importPackage", "addMedia", "mergeSyncBundle", "addSyncMedia"];
/// Methods whose result is `Payload::Bytes`.
pub const RETURNS_BYTES: &[&str] = &["getMedia", "exportPackage", "exportFullBackup", "exportSyncBundle"];

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
        "allTags"
            | "backlogReport"
            | "browse"
            | "checkTemplate"
            | "deckTree"
            | "evaluate"
            | "explainCard"
            | "exportFullBackup"
            | "exportPackage"
            | "findDuplicates"
            | "forecastWorkload"
            | "getCard"
            | "getCardsOfNote"
            | "getDeck"
            | "getMedia"
            | "getNote"
            | "getNotetype"
            | "getPreset"
            | "getSettings"
            | "insights"
            | "isEmpty"
            | "listBackups"
            | "listNotetypes"
            | "listPresets"
            | "nextCard"
            | "open"
            | "previewImport"
            | "previewNote"
            | "renderCard"
            | "searchIds"
            | "sidebar"
            | "stats"
            | "today"
            | "undoLabel"
            | "close"
            | "startStudy"
            | "startCustomStudy"
            | "endStudy"
            | "syncStatus"
            | "syncMediaState"
            | "exportSyncBundle"
    )
}

/// The engine behind one collection directory.
pub struct Engine {
    dir: PathBuf,
    collection: Collection,
    opened: bool,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").field("dir", &self.dir).field("opened", &self.opened).finish()
    }
}

fn arg<T: DeserializeOwned>(args: &[Value], i: usize, method: &str, name: &str) -> Result<T> {
    let v = args.get(i).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| LociError::invalid(format!("{method}: argument \"{name}\" is invalid: {e}")))
}

fn json<T: Serialize>(v: T) -> Result<Payload> {
    Ok(Payload::Json(serde_json::to_value(v)?))
}

fn unit() -> Result<Payload> {
    Ok(Payload::Json(Value::Null))
}

fn need_bytes<'a>(bytes: Option<&'a [u8]>, method: &str) -> Result<&'a [u8]> {
    bytes.ok_or_else(|| LociError::invalid(format!("{method}: this method needs a byte payload")))
}

impl Engine {
    /// Opens (creating if needed) the collection in `dir`: `collection.sqlite`, `media/`, `backups/`.
    /// The schema is created or migrated by the first `open` call (or lazily by the first method).
    pub fn open(dir: impl AsRef<Path>) -> Result<Engine> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let collection = Collection::open_dir(&dir, None, None)?;
        Ok(Engine { dir, collection, opened: false })
    }

    /// Wraps an already constructed (and possibly opened) collection, e.g. one with a test clock.
    pub fn from_collection(dir: impl AsRef<Path>, collection: Collection, opened: bool) -> Engine {
        Engine { dir: dir.as_ref().to_path_buf(), collection, opened }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn collection(&self) -> &Collection {
        &self.collection
    }

    pub fn collection_mut(&mut self) -> &mut Collection {
        &mut self.collection
    }

    fn ensure_open(&mut self) -> Result<()> {
        if !self.opened {
            self.collection.open()?;
            self.opened = true;
        }
        Ok(())
    }

    /// Runs one method. See the module docs for the calling convention.
    pub fn dispatch(
        &mut self,
        method: &str,
        args: &[Value],
        bytes: Option<&[u8]>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Payload> {
        if !is_method(method) {
            return Err(LociError::invalid(format!("Unknown method {method}")));
        }
        match method {
            "open" => {
                let report = self.collection.open()?;
                self.opened = true;
                return json(report);
            }
            // The TypeScript worker closes the database; the shells here keep the engine usable
            // (the next call reopens nothing because nothing was closed), so only the session ends.
            "close" => {
                self.collection.end_study().ok();
                return unit();
            }
            _ => {}
        }
        self.ensure_open()?;
        let out = self.dispatch_open(method, args, bytes, progress)?;
        if is_write(method) && method != "mergeSyncBundle" {
            self.collection.sync_after_write();
        }
        Ok(out)
    }

    fn dispatch_open(
        &mut self,
        method: &str,
        args: &[Value],
        bytes: Option<&[u8]>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Payload> {
        let c = &mut self.collection;
        let m = method;
        match method {
            "addMedia" => json(c.add_media(&arg::<String>(args, 0, m, "name")?, need_bytes(bytes, m)?)?),
            "addNote" => json(c.add_note(
                arg(args, 0, m, "ntid")?,
                arg(args, 1, m, "deckId")?,
                &arg::<Vec<String>>(args, 2, m, "fields")?,
                &arg::<Vec<String>>(args, 3, m, "tags")?,
            )?),
            "allTags" => json(c.all_tags()?),
            "answerCard" => {
                let rating: i64 = arg(args, 1, m, "rating")?;
                let rating = Rating::from_i64(rating).ok_or_else(|| LociError::invalid(format!("Invalid rating {rating}")))?;
                json(c.answer_card(arg(args, 0, m, "cardId")?, rating, arg(args, 2, m, "elapsedMs")?)?)
            }
            "applyCatchUp" => json(c.apply_catch_up(arg(args, 0, m, "deckId")?, &arg::<CatchUpPlan>(args, 1, m, "plan")?)?),
            "backlogReport" => json(c.backlog_report(arg(args, 0, m, "deckId")?)?),
            "backupNow" => json(c.backup_now()?),
            "browse" => json(c.browse(&arg::<BrowseRequest>(args, 0, m, "request")?)?),
            "bulk" => json(c.bulk(&arg::<Vec<CardId>>(args, 0, m, "cardIds")?, &arg::<BulkAction>(args, 1, m, "action")?)?),
            "buryCards" => {
                c.bury_cards(&arg::<Vec<CardId>>(args, 0, m, "ids")?)?;
                unit()
            }
            "checkIntegrity" => json(c.check_integrity()?),
            "checkMedia" => json(c.check_media()?),
            "checkTemplate" => json(c.check_template(
                arg(args, 0, m, "ntid")?,
                &arg::<String>(args, 1, m, "qfmt")?,
                &arg::<String>(args, 2, m, "afmt")?,
            )?),
            "createDeck" => json(c.create_deck(&arg::<String>(args, 0, m, "name")?, arg(args, 1, m, "configId")?)?),
            "deckTree" => json(c.deck_tree()?),
            "deleteDeck" => json(c.delete_deck(arg(args, 0, m, "id")?)?),
            "deleteMedia" => {
                c.delete_media(&arg::<Vec<String>>(args, 0, m, "names")?)?;
                unit()
            }
            "deleteNotes" => {
                c.delete_notes(&arg::<Vec<NoteId>>(args, 0, m, "ids")?)?;
                unit()
            }
            "deleteNotetype" => {
                c.delete_notetype(arg(args, 0, m, "id")?)?;
                unit()
            }
            "deletePreset" => {
                c.delete_preset(arg(args, 0, m, "id")?)?;
                unit()
            }
            "deleteSavedSearch" => {
                c.delete_saved_search(&arg::<String>(args, 0, m, "name")?)?;
                unit()
            }
            "endStudy" => json(c.end_study()?),
            "evaluate" => json(c.evaluate(arg(args, 0, m, "configId")?)?),
            "explainCard" => json(c.explain_card(arg(args, 0, m, "id")?)?),
            "exportFullBackup" => Ok(Payload::Bytes(Some(c.export_full_backup(progress)?))),
            "exportPackage" => {
                Ok(Payload::Bytes(Some(c.export_package(&arg::<ExportOptions>(args, 0, m, "options")?, progress)?)))
            }
            "extendTodayLimits" => json(
                c.extend_today_limits(arg(args, 0, m, "deckId")?, arg(args, 1, m, "extraNew")?, arg(args, 2, m, "extraReview")?)
                    .and_then(|counts| {
                        // collection.ts:339: a running session for this deck picks the extra cards up
                        c.refill_session()?;
                        Ok(counts)
                    })?,
            ),
            "findDuplicates" => json(c.find_duplicates(arg(args, 0, m, "ntid")?, &arg::<String>(args, 1, m, "firstField")?)?),
            "forecastWorkload" => {
                json(c.forecast_workload(arg(args, 0, m, "deckId")?, &arg::<DeckConfig>(args, 1, m, "config")?)?)
            }
            "getCard" => json(c.get_card(arg(args, 0, m, "id")?)?),
            "getCardsOfNote" => json(c.get_cards_of_note(arg(args, 0, m, "id")?)?),
            "getDeck" => json(c.get_deck(arg(args, 0, m, "id")?)?),
            "getMedia" => Ok(Payload::Bytes(c.get_media(&arg::<String>(args, 0, m, "name")?)?)),
            "getNote" => json(c.get_note(arg(args, 0, m, "id")?)?),
            "getNotetype" => json(c.get_notetype(arg(args, 0, m, "id")?)?),
            "getPreset" => json(c.get_preset(arg(args, 0, m, "id")?)?),
            "getSettings" => json(c.get_settings()?),
            "importPackage" => {
                let options: ImportOptions = arg(args, 0, m, "options")?;
                let bytes = need_bytes(bytes, m)?.to_vec();
                json(c.import_package(bytes, &options, progress)?)
            }
            "insights" => json(c.insights()?),
            "installSampleDeck" => json(c.install_sample_deck()?),
            "isEmpty" => json(c.is_empty()?),
            "listBackups" => json(c.list_backups()?),
            "listNotetypes" => json(c.list_notetypes()?),
            "listPresets" => json(c.list_presets()?),
            "maintenance" => json(c.maintenance()?),
            "nextCard" => json(c.next_card()?),
            "optimise" => json(c.optimise(arg(args, 0, m, "configId")?, progress)?),
            "previewImport" => json(c.preview_import(need_bytes(bytes, m)?.to_vec())?),
            "previewNote" => json(c.preview_note(
                arg(args, 0, m, "ntid")?,
                &arg::<Vec<String>>(args, 1, m, "fields")?,
                arg(args, 2, m, "ord")?,
            )?),
            "renameDeck" => {
                c.rename_deck(arg(args, 0, m, "id")?, &arg::<String>(args, 1, m, "name")?)?;
                unit()
            }
            "renameTag" => json(c.rename_tag(&arg::<String>(args, 0, m, "from")?, &arg::<String>(args, 1, m, "to")?)?),
            "renderCard" => json(c.render_card(arg(args, 0, m, "id")?)?),
            "resetCollection" => {
                c.reset_collection()?;
                unit()
            }
            "resolveLeech" => {
                c.resolve_leech(arg(args, 0, m, "cardId")?, arg::<LeechResolution>(args, 1, m, "action")?)?;
                unit()
            }
            "restoreBackup" => json(c.restore_backup(&arg::<String>(args, 0, m, "name")?)?),
            "saveNotetype" => json(c.save_notetype(&arg::<NotetypeInput>(args, 0, m, "notetype")?)?),
            "savePreset" => json(c.save_preset(&arg::<DeckConfigPresetInput>(args, 0, m, "preset")?)?),
            "saveSearch" => {
                c.save_search(&arg::<String>(args, 0, m, "name")?, &arg::<String>(args, 1, m, "query")?)?;
                unit()
            }
            "searchIds" => json(c.search_ids(&arg::<String>(args, 0, m, "query")?, arg::<BrowseMode>(args, 1, m, "mode")?)?),
            "setDeckCollapsed" => {
                c.set_deck_collapsed(arg(args, 0, m, "id")?, arg(args, 1, m, "collapsed")?)?;
                unit()
            }
            "setDeckDescription" => {
                c.set_deck_description(arg(args, 0, m, "id")?, &arg::<String>(args, 1, m, "description")?)?;
                unit()
            }
            "setDeckPreset" => {
                c.set_deck_preset(arg(args, 0, m, "id")?, arg(args, 1, m, "configId")?)?;
                unit()
            }
            "setFlag" => {
                c.set_flag(&arg::<Vec<CardId>>(args, 0, m, "ids")?, arg(args, 1, m, "flag")?)?;
                unit()
            }
            "setSettings" => {
                let patch = parse_patch(args.first().unwrap_or(&Value::Null))?;
                json(c.set_settings(&patch)?)
            }
            "sidebar" => json(c.sidebar()?),
            "startCustomStudy" => {
                json(c.start_custom_study(arg(args, 0, m, "deckId")?, arg::<CustomStudyKind>(args, 1, m, "kind")?)?)
            }
            "startStudy" => json(c.start_study(arg(args, 0, m, "deckId")?)?),
            "stats" => {
                let req: Option<StatsRequest> = arg(args, 0, m, "request")?;
                json(c.stats(&req.unwrap_or_default())?)
            }
            "suspendCards" => {
                c.suspend_cards(&arg::<Vec<CardId>>(args, 0, m, "ids")?, arg(args, 1, m, "suspend")?)?;
                unit()
            }
            "today" => json(c.today()?),
            "unburyDeck" => json(c.unbury_deck(arg(args, 0, m, "deckId")?)?),
            "undo" => {
                let r = c.undo()?;
                if r.is_some() {
                    // collection.ts:356: a running session re-synchronises after an undo
                    c.session_after_undo()?;
                }
                json(r)
            }
            "undoLabel" => json(c.undo_label()?),
            "updateNote" => json(c.update_note(
                arg(args, 0, m, "id")?,
                &arg::<Vec<String>>(args, 1, m, "fields")?,
                &arg::<Vec<String>>(args, 2, m, "tags")?,
            )?),
            "backupOnLeave" => json(c.backup_on_leave()?),
            "autoBackup" => json(c.auto_backup()?),
            "syncStatus" => json(c.sync_status()?),
            "setSyncFolder" => json(c.set_sync_folder(arg::<Option<String>>(args, 0, m, "folder")?.as_deref())?),
            "exportSyncBundle" => {
                let options: Option<SyncExportOptions> = arg(args, 0, m, "options")?;
                Ok(Payload::Bytes(Some(c.export_sync_bundle(&options.unwrap_or_default())?)))
            }
            "mergeSyncBundle" => json(c.merge_sync_bundle(need_bytes(bytes, m)?)?),
            "syncMediaState" => json(c.sync_media_state()?),
            "addSyncMedia" => json(c.add_sync_media(&arg::<String>(args, 0, m, "sha1")?, need_bytes(bytes, m)?)?),
            other => Err(LociError::invalid(format!("Unknown method {other}"))),
        }
    }

    /// Closes the collection (flushes and releases the file).
    pub fn close(mut self) -> Result<()> {
        self.collection.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_table_is_complete_and_sorted_enough() {
        assert_eq!(METHODS.len(), 84);
        assert!(is_method("answerCard"));
        assert!(!is_method("dropTables"));
        assert!(takes_bytes("addMedia") && returns_bytes("getMedia"));
        assert!(is_write("answerCard") && !is_write("deckTree"));
    }

    #[test]
    fn unknown_methods_and_bad_arguments_are_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = Engine::open(dir.path()).unwrap();
        let err = e.dispatch("constructor", &[], None, &mut |_| {}).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Invalid);
        let err = e.dispatch("getDeck", &[Value::String("x".into())], None, &mut |_| {}).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Invalid);
        assert!(err.message.contains("getDeck"));
        let err = e
            .dispatch("importPackage", &[serde_json::json!({"mode": "merge", "keepScheduling": true})], None, &mut |_| {})
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::Invalid);
        // open was implied by the first real call
        assert_eq!(e.dispatch("isEmpty", &[], None, &mut |_| {}).unwrap(), Payload::Json(Value::Bool(true)));
    }
}
