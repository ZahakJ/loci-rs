//! Writer and round-trip tests.

use super::image::AnkiImage;
use super::read::read_package;
use super::tests_fixtures::{fixture, ids, read_fixture, rezip, strip_decks, strip_presets};
use super::write::{write_legacy_package, WriteLegacyOptions, DUMMY_NOTE_TEXT};
use super::zip::ZipArchive;
use crate::db::Db;
use crate::text::{field_checksum, sha1_hex, strip_html_preserving_media};
use crate::types::*;
use chrono::{Local, TimeZone, Timelike};
use serde_json::Value;

const WITH_ALL: WriteLegacyOptions = WriteLegacyOptions { include_scheduling: true, include_media: true, now: None };
const SCHED_NO_MEDIA: WriteLegacyOptions = WriteLegacyOptions { include_scheduling: true, include_media: false, now: None };

struct Written {
    source: AnkiImage,
    written: Vec<u8>,
    zip: ZipArchive,
}

fn written() -> Written {
    let source = read_fixture("palace-latest.apkg");
    let written = write_legacy_package(&source, WITH_ALL).unwrap();
    let zip = ZipArchive::open(written.clone()).unwrap();
    Written { source, written, zip }
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn has_keys(v: &Value, keys: &[&str], what: &str) {
    for k in keys {
        assert!(v.get(k).is_some(), "{what}.{k} missing in {v}");
    }
}

#[test]
fn writes_the_legacy2_container() {
    let w = written();
    let names = w.zip.names();
    assert_eq!(&names[..3], ["meta", "collection.anki21", "collection.anki2"]);
    assert_eq!(names.last().unwrap(), "media");
    assert_eq!(w.zip.read("meta").unwrap(), vec![0x08, 0x02]);
    let Value::Object(map) = json(std::str::from_utf8(&w.zip.read("media").unwrap()).unwrap()) else { panic!("media map") };
    let keys: Vec<&String> = map.keys().collect();
    assert_eq!(keys, (0..map.len()).map(|i| i.to_string()).collect::<Vec<_>>().iter().collect::<Vec<_>>());
    let mut values: Vec<&str> = map.values().map(|v| v.as_str().unwrap()).collect();
    values.sort();
    assert_eq!(values, w.source.media.keys().map(String::as_str).collect::<Vec<_>>());
    for (entry, name) in &map {
        assert_eq!(sha1_hex(&w.zip.read(entry).unwrap()), sha1_hex(&w.source.read_media(name.as_str().unwrap()).unwrap()));
    }
    let dummy_bytes = w.zip.read("collection.anki2").unwrap();
    assert_eq!(u16::from_be_bytes([dummy_bytes[16], dummy_bytes[17]]), 512); // page size in the file header
    let dummy = Db::open_from_bytes(&dummy_bytes).unwrap();
    let flds: Vec<String> = dummy.all("SELECT flds FROM notes", [], |r| r.get(0)).unwrap();
    assert_eq!(flds.len(), 1);
    assert!(flds[0].starts_with(DUMMY_NOTE_TEXT));
    assert_eq!(dummy.count("SELECT count(*) FROM cards", []).unwrap(), 1);
}

#[test]
fn round_trips_notes_cards_revlog_note_types_decks_presets_and_media() {
    let w = written();
    let back = read_package(w.written.clone()).unwrap();
    assert_eq!(back.layout, PackageLayout::Legacy2);
    assert_eq!(back.notes, w.source.notes);
    assert_eq!(back.cards, w.source.cards);
    assert_eq!(back.revlog, w.source.revlog);
    assert_eq!(back.notetypes, w.source.notetypes);
    assert_eq!(strip_decks(&back), strip_decks(&w.source));
    assert_eq!(strip_presets(&back), strip_presets(&w.source));
    assert_eq!(back.crt, w.source.crt);
    assert_eq!(back.config.sched_ver, 2);
    assert_eq!(back.config.rollover, w.source.config.rollover);
    assert_eq!(back.config.creation_offset, w.source.config.creation_offset);
    assert_eq!(back.config.next_pos, w.source.config.next_pos);
    assert!(back.config.fsrs);
    assert_eq!(back.media.keys().collect::<Vec<_>>(), w.source.media.keys().collect::<Vec<_>>());
    for name in back.media.keys() {
        assert_eq!(sha1_hex(&back.read_media(name).unwrap()), sha1_hex(&w.source.read_media(name).unwrap()));
    }
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
}

#[test]
fn writes_the_col_row_and_json_that_anki_requires() {
    let w = written();
    let db = Db::open_from_bytes(&w.zip.read("collection.anki21").unwrap()).unwrap();
    let (id, crt, modv, scm, ver, dty, usn, ls, conf, models, decks, dconf) = db
        .get("SELECT id, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf FROM col", [], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
            ))
        })
        .unwrap()
        .unwrap();
    assert_eq!((id, crt, ver, dty, usn, ls), (1, w.source.crt, 11, 0, 0, 0));
    assert!(modv > 1_700_000_000_000 && scm > 1_700_000_000_000);
    let conf = json(&conf);
    has_keys(
        &conf,
        &[
            "activeDecks",
            "curDeck",
            "newSpread",
            "collapseTime",
            "timeLim",
            "estTimes",
            "dueCounts",
            "curModel",
            "nextPos",
            "sortType",
            "sortBackwards",
            "addToCur",
            "dayLearnFirst",
            "schedVer",
            "sched2021",
            "rollover",
            "creationOffset",
        ],
        "conf",
    );
    assert_eq!(conf["schedVer"], 2);
    let max_new_due: i64 = db.scalar("SELECT max(due) FROM cards WHERE type = 0", []).unwrap().unwrap();
    assert!(conf["nextPos"].as_i64().unwrap() > max_new_due);

    let models = json(&models);
    let mut model_ids: Vec<&String> = models.as_object().unwrap().keys().collect();
    model_ids.sort();
    let mut nt_ids: Vec<String> = w.source.notetypes.iter().map(|n| n.id.to_string()).collect();
    nt_ids.sort();
    assert_eq!(model_ids, nt_ids.iter().collect::<Vec<_>>());
    for m in models.as_object().unwrap().values() {
        has_keys(
            m,
            &[
                "id",
                "name",
                "type",
                "mod",
                "usn",
                "sortf",
                "did",
                "tmpls",
                "flds",
                "css",
                "latexPre",
                "latexPost",
                "latexsvg",
                "req",
                "tags",
                "vers",
            ],
            "model",
        );
        for f in m["flds"].as_array().unwrap() {
            has_keys(f, &["name", "ord", "sticky", "rtl", "font", "size"], "field");
        }
        for t in m["tmpls"].as_array().unwrap() {
            has_keys(t, &["name", "ord", "qfmt", "afmt", "bqfmt", "bafmt", "did", "bfont", "bsize"], "tmpl");
        }
        if m["type"] == 1 {
            assert_eq!(m["tmpls"].as_array().unwrap().len(), 1);
        }
    }
    let io = &models[ids::NT_IO.to_string()];
    assert_eq!(io["originalStockKind"], 6);
    let io_fields: Vec<(i64, bool)> = io["flds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["tag"].as_i64().unwrap(), f["preventDeletion"].as_bool().unwrap()))
        .collect();
    assert_eq!(io_fields, vec![(0, true), (1, true), (2, true), (3, true), (4, false)]);

    let decks = json(&decks);
    assert!(decks.get("1").is_some());
    let names: std::collections::HashSet<&str> =
        decks.as_object().unwrap().values().map(|d| d["name"].as_str().unwrap()).collect();
    for d in decks.as_object().unwrap().values() {
        has_keys(
            d,
            &[
                "id",
                "name",
                "usn",
                "mod",
                "collapsed",
                "browserCollapsed",
                "desc",
                "dyn",
                "conf",
                "extendNew",
                "extendRev",
                "newToday",
                "revToday",
                "lrnToday",
                "timeToday",
            ],
            "deck",
        );
        let parts: Vec<&str> = d["name"].as_str().unwrap().split("::").collect();
        for i in 1..parts.len() {
            assert!(names.contains(parts[..i].join("::").as_str()));
        }
    }
    let dconf = json(&dconf);
    assert!(dconf.get("1").is_some());
    for d in dconf.as_object().unwrap().values() {
        has_keys(
            d,
            &[
                "id",
                "mod",
                "name",
                "usn",
                "maxTaken",
                "autoplay",
                "timer",
                "replayq",
                "dyn",
                "new",
                "rev",
                "lapse",
                "desiredRetention",
                "sm2Retention",
                "fsrsParams6",
                "fsrsParams5",
                "fsrsWeights",
            ],
            "dconf",
        );
        has_keys(&d["new"], &["delays", "ints", "initialFactor", "order", "perDay", "bury"], "new");
        has_keys(&d["rev"], &["perDay", "ease4", "ivlFct", "maxIvl", "hardFactor", "bury"], "rev");
        has_keys(&d["lapse"], &["delays", "mult", "minInt", "leechFails", "leechAction"], "lapse");
    }
    let med = dconf.as_object().unwrap().values().find(|d| d["name"].as_str().unwrap().starts_with("Medicine")).unwrap();
    assert_eq!(med["fsrsParams5"].as_array().unwrap().len(), 19);
    assert_eq!(med["fsrsParams6"], serde_json::json!([]));
    assert_eq!(med["desiredRetention"], 0.92);

    // notes: sfld/csum like Anki
    let notes: Vec<(String, i64, i64, String)> =
        db.all("SELECT flds, csum, flags, data FROM notes", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    assert!(!notes.is_empty());
    for (flds, csum, flags, data) in notes {
        let first = flds.split('\u{1f}').next().unwrap();
        assert_eq!(csum, field_checksum(&strip_html_preserving_media(first)) as i64);
        assert_eq!((flags, data.as_str()), (0, ""));
    }
    let card_data: String = db.scalar("SELECT data FROM cards WHERE data LIKE '%\"s\":%' LIMIT 1", []).unwrap().unwrap();
    let parsed = json(&card_data);
    let keys: Vec<&str> = parsed.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["pos", "s", "d", "dr", "decay", "lrt"]);
}

#[test]
fn stores_integer_looking_sort_fields_as_sqlite_integers() {
    let mut img = read_fixture("palace-latest.apkg");
    for n in &mut img.notes {
        if n.ntid == ids::NT_TYPED {
            n.fields.swap(0, 1);
        }
    }
    let bytes = write_legacy_package(&img, SCHED_NO_MEDIA).unwrap();
    let db = Db::open_from_bytes(&ZipArchive::open(bytes).unwrap().read("collection.anki21").unwrap()).unwrap();
    let rows: Vec<(String, rusqlite::types::Value)> = db
        .all(&format!("SELECT typeof(sfld), sfld FROM notes WHERE mid = {}", ids::NT_TYPED), [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(rows.iter().any(|(t, _)| t == "integer"));
    for (t, v) in rows {
        if t == "integer" {
            assert!(matches!(v, rusqlite::types::Value::Integer(_)));
        }
    }
}

#[test]
fn without_scheduling_every_card_is_new_no_revlog_no_leech_tags_no_media() {
    let source = read_fixture("palace-latest.apkg");
    let bytes =
        write_legacy_package(&source, WriteLegacyOptions { include_scheduling: false, include_media: false, now: None }).unwrap();
    let back = read_package(bytes).unwrap();
    assert!(back.revlog.is_empty());
    assert_eq!(back.cards.len(), source.cards.len());
    for c in &back.cards {
        assert_eq!((c.ctype, c.queue, c.ivl, c.factor, c.reps, c.lapses, c.left), (CardType::New, CardQueue::New, 0, 0, 0, 0, 0));
        assert_eq!((c.odue, c.odid, c.flags), (0, 0, 0));
        assert_eq!(c.data, CardData::default());
    }
    // positions: new cards keep theirs, reviewed cards get their remembered original position
    let src: std::collections::HashMap<i64, &Card> = source.cards.iter().map(|c| (c.id, c)).collect();
    for c in &back.cards {
        let s = src[&c.id];
        if s.ctype == CardType::New {
            assert_eq!(c.due, s.due);
        } else if let Some(pos) = s.data.pos {
            assert_eq!(c.due, pos);
        }
    }
    assert!(!back.notes.iter().any(|n| n.tags.iter().any(|t| t == "leech" || t == "marked")));
    assert!(back.media.is_empty());
    assert!(back.decks.iter().all(|d| d.today.new == 0 && d.today.rev == 0));
    assert!(back.config.next_pos > back.cards.iter().map(|c| c.due).max().unwrap());
}

#[test]
fn adds_missing_parent_decks_and_default() {
    let mut img = read_fixture("palace-latest.apkg");
    img.decks.retain(|d| d.name != "Languages" && d.id != 1);
    let back = read_package(write_legacy_package(&img, SCHED_NO_MEDIA).unwrap()).unwrap();
    assert!(back.decks.iter().any(|d| d.name == "Languages"));
    assert!(back.decks.iter().any(|d| d.id == 1 && d.name == "Default"));
}

#[test]
fn deterministic_bytes_given_a_fixed_now() {
    let source = read_fixture("palace-legacy2.apkg");
    let opts = WriteLegacyOptions { include_scheduling: true, include_media: true, now: Some(1_790_850_600_000) };
    let a = write_legacy_package(&source, opts).unwrap();
    let b = write_legacy_package(&source, opts).unwrap();
    assert_eq!(a, b);
}

// ───────────────────────── v1 scheduler collections ─────────────────────────

fn make_v1() -> (Vec<u8>, i64) {
    let mut relearn_id = 0;
    let bytes = rezip(fixture("palace-legacy2.apkg"), |name, data| {
        if name != "collection.anki21" {
            return Some(data);
        }
        let db = Db::open_from_bytes(&data).unwrap();
        let conf: String = db.scalar("SELECT conf FROM col", []).unwrap().unwrap();
        let Value::Object(mut conf) = json(&conf) else { panic!("conf") };
        for k in ["schedVer", "sched2021", "creationOffset", "rollover"] {
            conf.remove(k);
        }
        db.run("UPDATE col SET conf = ?", [Value::Object(conf).to_string()]).unwrap();
        let id: i64 = db.scalar("SELECT id FROM cards WHERE type = 2 AND queue = 2 LIMIT 1", []).unwrap().unwrap();
        relearn_id = id;
        db.run("UPDATE cards SET queue = 1, due = 1790850000 WHERE id = ?", [id]).unwrap(); // v1 relearning: type 2 in queue 1
        Some(db.serialize().unwrap())
    });
    (bytes, relearn_id)
}

#[test]
fn upgrades_like_anki_learning_eases_plus_one_relearning_type_sched_ver_2_rollover_from_crt() {
    let original = read_fixture("palace-legacy2.apkg");
    let (bytes, relearn_id) = make_v1();
    let image = read_package(bytes).unwrap();
    assert_eq!(image.config.sched_ver, 2);
    assert_eq!(image.config.rollover, Local.timestamp_opt(image.crt, 0).single().unwrap().hour() as i64);
    assert_eq!(image.config.creation_offset, Some(-(Local::now().offset().local_minus_utc() as i64) / 60));
    assert!(image.warnings.iter().any(|w| w.contains("v1 scheduler")));
    let before: std::collections::HashMap<i64, &RevlogEntry> = original.revlog.iter().map(|r| (r.id, r)).collect();
    let mut shifted = 0;
    for r in &image.revlog {
        let o = before[&r.id];
        let expect_shift = (o.ease == 2 || o.ease == 3) && (o.rtype == RevlogType::Learn || o.rtype == RevlogType::Relearn);
        assert_eq!(r.ease, if expect_shift { o.ease + 1 } else { o.ease });
        if expect_shift {
            shifted += 1;
        }
    }
    assert!(shifted > 100);
    let card = image.cards.iter().find(|c| c.id == relearn_id).unwrap();
    assert_eq!((card.ctype, card.queue), (CardType::Relearn, CardQueue::Learn));
}
