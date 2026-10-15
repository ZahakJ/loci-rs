//! `rpc::Engine::dispatch` tests: method dispatch, progress, errors, bytes (the CLI is a thin
//! shell over it).

use super::helpers::*;
use loci_core::collection::Collection;
use loci_core::error::ErrorCode;
use loci_core::rpc::{Engine, Payload, METHODS};
use loci_core::types::Progress;
use serde_json::{json, Value};

fn engine_at(start: i64) -> (Engine, Clock, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let clock = Clock::new(start);
    let col = Collection::open_dir(dir.path(), Some(clock.boxed()), Some(Box::new(|_| 0))).unwrap();
    let mut e = Engine::from_collection(dir.path(), col, false);
    let report = e.dispatch("open", &[], None, &mut |_| {}).unwrap().into_json().unwrap();
    assert_eq!(report["ok"], Value::Bool(true));
    (e, clock, dir)
}

fn call(e: &mut Engine, method: &str, args: &[Value]) -> Value {
    e.dispatch(method, args, None, &mut |_| {}).unwrap_or_else(|err| panic!("{method}: {err}")).into_json().unwrap()
}

// ── cli: import, due, study --auto 5, export, stats, backup, integrity ──

#[test]
fn cli_flow_import_due_study_export_stats_backup_integrity_over_the_engine() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    let bytes = fixture("palace-legacy2.apkg");
    let report = e.dispatch("importPackage", &[json!(merge_options())], Some(&bytes), &mut |_| {}).unwrap().into_json().unwrap();
    assert_eq!(report["notesAdded"], json!(402));
    assert_eq!(report["cardsAdded"], json!(590));

    // `loci due` prints the deck tree
    let tree = call(&mut e, "deckTree", &[]);
    let names: Vec<String> =
        tree["children"].as_array().unwrap().iter().map(|c| c["deck"]["name"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"History".to_string()), "{names:?}");
    let total = tree["newCount"].as_i64().unwrap() + tree["learnCount"].as_i64().unwrap() + tree["reviewCount"].as_i64().unwrap();
    assert!(total > 0);

    // `loci study --auto 5`: answer five cards Good
    let mut card = call(&mut e, "startStudy", &[Value::Null]);
    let mut answered = 0;
    while answered < 5 && !card.is_null() {
        let id = card["card"]["id"].clone();
        let res = call(&mut e, "answerCard", &[id, json!(3), json!(3000)]);
        assert_eq!(res["undoLabel"], json!("Answer Good"));
        answered += 1;
        card = res["next"].clone();
    }
    assert_eq!(answered, 5);

    let out = e
        .dispatch("exportPackage", &[json!({"deckId": null, "includeScheduling": true, "includeMedia": true})], None, &mut |_| {})
        .unwrap();
    let exported = out.into_bytes().unwrap().unwrap();
    assert!(exported.len() > 10_000);

    let stats = call(&mut e, "stats", &[json!({"deckId": null})]);
    assert!(stats["today"]["studied"].as_i64().unwrap() >= 5);
    let backup = call(&mut e, "backupNow", &[]);
    assert!(backup["name"].as_str().unwrap().starts_with("loci-"));
    let integrity = call(&mut e, "checkIntegrity", &[]);
    assert_eq!(integrity["ok"], Value::Bool(true));

    // the export re-imports into a second directory
    let (mut e2, _c2, _d2) = engine_at(expected().now * 1000);
    let again =
        e2.dispatch("importPackage", &[json!(merge_options())], Some(&exported), &mut |_| {}).unwrap().into_json().unwrap();
    assert_eq!(again["notesAdded"], json!(402));
    assert_eq!(again["reviewsAdded"], json!(7774 + 5));
}

#[test]
fn cli_errors_carry_a_code() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    let bytes = fixture("broken-not-a-zip.apkg");
    let err = e.dispatch("importPackage", &[json!(merge_options())], Some(&bytes), &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedPackage);
    let err = e.dispatch("bogus", &[], None, &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    assert!(err.message.contains("bogus"));
}

// ── worker ──

#[test]
fn serves_calls_and_streams_progress() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    assert_eq!(call(&mut e, "isEmpty", &[]), Value::Bool(true));
    let mut progress: Vec<Progress> = Vec::new();
    let bytes = fixture("palace-legacy2.apkg");
    let report = e
        .dispatch(
            "importPackage",
            &[json!({"mode": "merge", "keepScheduling": false, "conflict": "newer"})],
            Some(&bytes),
            &mut |p| progress.push(p),
        )
        .unwrap()
        .into_json()
        .unwrap();
    assert!(report["notesAdded"].as_i64().unwrap() > 0);
    let phases: Vec<&str> = progress.iter().map(|p| p.phase.as_str()).collect();
    for phase in ["Reading package", "Notes", "Cards", "Review history", "Media", "Checking integrity", "Done"] {
        assert!(phases.contains(&phase), "{phase} missing in {phases:?}");
    }
    let tree = call(&mut e, "deckTree", &[]);
    assert!(!tree["children"].as_array().unwrap().is_empty());
    let card = call(&mut e, "startStudy", &[Value::Null]);
    assert!(!card.is_null());
    let res = call(&mut e, "answerCard", &[card["card"]["id"].clone(), json!(3), json!(2000)]);
    assert_eq!(res["undoLabel"], json!("Answer Good"));
}

#[test]
fn returns_bytes_and_structured_errors() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    let bytes = fixture("palace-legacy2.apkg");
    e.dispatch("importPackage", &[json!(merge_options())], Some(&bytes), &mut |_| {}).unwrap();
    let out = e
        .dispatch(
            "exportPackage",
            &[json!({"deckId": null, "includeScheduling": true, "includeMedia": false})],
            None,
            &mut |_| {},
        )
        .unwrap();
    match out {
        Payload::Bytes(Some(b)) => assert!(b.len() > 1000),
        other => panic!("expected bytes, got {other:?}"),
    }
    let err = e.dispatch("getDeck", &[json!(424242)], None, &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    // the engine refuses unknown methods sent on the wire
    let err = e.dispatch("constructor", &[], None, &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    let err = e.dispatch("notAMethod", &[], None, &mut |_| {}).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
}

#[test]
fn backs_up_after_writes_on_demand_and_on_leave() {
    let (mut e, clock, _dir) = engine_at(expected().now * 1000);
    let bytes = fixture("palace-legacy2.apkg");
    e.dispatch("importPackage", &[json!(merge_options())], Some(&bytes), &mut |_| {}).unwrap();
    // the first write of the session triggers the automatic backup
    let auto = call(&mut e, "autoBackup", &[]);
    assert_eq!(auto["kind"], json!("daily"));
    let info = call(&mut e, "backupNow", &[]);
    assert!(info["name"].as_str().unwrap().ends_with(".sqlite"));
    assert_eq!(call(&mut e, "backupOnLeave", &[]), Value::Null); // nothing changed since
    clock.advance(1100); // backup times have one-second resolution
    call(&mut e, "createDeck", &[json!("After backup"), Value::Null]);
    let leave = call(&mut e, "backupOnLeave", &[]);
    assert!(leave["name"].as_str().unwrap().ends_with("-auto.sqlite"), "{leave}");
}

#[test]
fn serves_media_bytes() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    let png = [137u8, 80, 78, 71];
    let name = e.dispatch("addMedia", &[json!("dot.png")], Some(&png), &mut |_| {}).unwrap().into_json().unwrap();
    let name = name.as_str().unwrap().to_string();
    let got = e.dispatch("getMedia", &[json!(name)], None, &mut |_| {}).unwrap().into_bytes().unwrap();
    assert_eq!(got, Some(png.to_vec()));
    let missing = e.dispatch("getMedia", &[json!("missing-file.png")], None, &mut |_| {}).unwrap().into_bytes().unwrap();
    assert_eq!(missing, None);
}

/// Every name in `METHODS` reaches a handler (no "unknown method"): the table and the dispatcher agree.
#[test]
fn every_method_in_the_table_dispatches() {
    let (mut e, _clock, _dir) = engine_at(expected().now * 1000);
    e.dispatch("installSampleDeck", &[], None, &mut |_| {}).unwrap();
    let deck = call(&mut e, "deckTree", &[])["children"][0]["deck"]["id"].clone();
    let card_id = call(&mut e, "searchIds", &[json!("deck:*"), json!("cards")])[0].clone();
    let note_id = call(&mut e, "getCard", std::slice::from_ref(&card_id))["nid"].clone();
    let nt = call(&mut e, "listNotetypes", &[])[0].clone();
    let preset = call(&mut e, "getPreset", &[json!(1)]);
    let config = preset["config"].clone();
    let media = e.dispatch("addMedia", &[json!("x.png")], Some(&[1, 2, 3]), &mut |_| {}).unwrap().into_json().unwrap();
    let apkg = fixture("palace-legacy2.apkg");
    let backup = call(&mut e, "backupNow", &[])["name"].clone();
    let args = |method: &str| -> Vec<Value> {
        match method {
            "addMedia" => vec![json!("y.png")],
            "addNote" => vec![nt["id"].clone(), deck.clone(), json!(["new q", "new a"]), json!([])],
            "answerCard" => vec![card_id.clone(), json!(3), json!(1000)],
            "applyCatchUp" => vec![Value::Null, json!({"kind": "study-all"})],
            "backlogReport" | "startStudy" | "unburyDeck" => vec![Value::Null],
            "browse" => vec![
                json!({"query": "", "mode": "cards", "sort": {"column": "due", "descending": false}, "offset": 0, "limit": 5}),
            ],
            "bulk" => vec![json!([card_id.clone()]), json!({"kind": "flag", "flag": 1})],
            "buryCards" | "deleteNotes" => vec![json!([])],
            "checkTemplate" => vec![nt["id"].clone(), json!("{{Front}}"), json!("{{Back}}")],
            "createDeck" => vec![json!("Dispatch test"), Value::Null],
            "deleteDeck" | "getDeck" | "setDeckCollapsed" | "setDeckDescription" | "setDeckPreset" => {
                let mut v = vec![deck.clone()];
                match method {
                    "setDeckCollapsed" => v.push(json!(false)),
                    "setDeckDescription" => v.push(json!("hello")),
                    "setDeckPreset" => v.push(json!(1)),
                    _ => {}
                }
                v
            }
            "deleteMedia" => vec![json!([media.clone()])],
            "deleteNotetype" | "getNotetype" => vec![nt["id"].clone()],
            "deletePreset" => vec![json!(999_999)],
            "deleteSavedSearch" => vec![json!("none")],
            "evaluate" | "optimise" | "getPreset" => vec![json!(1)],
            "explainCard" | "getCard" | "renderCard" => vec![card_id.clone()],
            "exportPackage" => vec![json!({"deckId": null, "includeScheduling": false, "includeMedia": false})],
            "extendTodayLimits" => vec![deck.clone(), json!(1), json!(0)],
            "findDuplicates" => vec![nt["id"].clone(), json!("x")],
            "forecastWorkload" => vec![Value::Null, config.clone()],
            "getCardsOfNote" | "getNote" => vec![note_id.clone()],
            "getMedia" => vec![media.clone()],
            "importPackage" => vec![json!(merge_options())],
            "previewNote" => vec![nt["id"].clone(), json!(["a", "b"]), json!(0)],
            "renameDeck" => vec![deck.clone(), json!("Renamed")],
            "renameTag" => vec![json!("nope"), json!("nope2")],
            "resolveLeech" => vec![card_id.clone(), json!("keep")],
            "restoreBackup" => vec![backup.clone()],
            "saveNotetype" => vec![nt.clone()],
            "savePreset" => vec![json!({"name": "P", "config": config.clone()})],
            "saveSearch" => vec![json!("s"), json!("deck:*")],
            "searchIds" => vec![json!("deck:*"), json!("cards")],
            "setFlag" => vec![json!([]), json!(1)],
            "setSettings" => vec![json!({"theme": "dark"})],
            "startCustomStudy" => vec![Value::Null, json!({"kind": "preview"})],
            "stats" => vec![json!({"deckId": null})],
            "suspendCards" => vec![json!([]), json!(true)],
            "updateNote" => vec![note_id.clone(), json!(["q2", "a2"]), json!([])],
            _ => vec![],
        }
    };
    // destructive or state-changing calls run last, in a safe order
    let late = ["deleteDeck", "deleteNotetype", "resetCollection", "restoreBackup", "deletePreset", "deleteMedia", "close"];
    let mut order: Vec<&str> = METHODS.iter().copied().filter(|m| !late.contains(m)).collect();
    order.extend(late);
    for method in order {
        let bytes: Option<&[u8]> = match method {
            "importPackage" | "previewImport" => Some(&apkg),
            "addMedia" => Some(&[9, 9]),
            _ => None,
        };
        let r = e.dispatch(method, &args(method), bytes, &mut |_| {});
        if let Err(err) = &r {
            assert!(!err.message.starts_with("Unknown method"), "{method}: {err}");
            // Methods whose preconditions this smoke test cannot satisfy may fail, but with a real code.
            assert!(matches!(err.code, ErrorCode::NotFound | ErrorCode::Invalid | ErrorCode::Conflict), "{method}: {err}");
        }
    }
}
