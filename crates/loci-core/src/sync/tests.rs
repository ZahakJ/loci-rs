//! Sync tests: merge algebra, convergence under random concurrent work, the review-log union,
//! delete/edit races, media name conflicts, the bundle format, restores and the RPC surface.
//! Collections are compared by a canonical dump of every synced object.

use super::tables::{self, Kind};
use super::*;
use crate::blobs::BlobStore;
use crate::collection::service::test_support::{TestClock, T0};
use crate::db::Db;
use crate::types::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use std::collections::BTreeSet;

struct Dev {
    col: Collection,
    dir: tempfile::TempDir,
    clock: TestClock,
}

fn clock() -> TestClock {
    TestClock(std::sync::Arc::new(std::sync::atomic::AtomicI64::new(T0)))
}

fn new_dev(clock: &TestClock) -> Dev {
    let dir = tempfile::tempdir().unwrap();
    let mut col =
        Collection::open_with(Db::memory().unwrap(), BlobStore::new(dir.path()), Some(clock.clock_fn()), Some(Box::new(|_| 0)));
    col.open().unwrap();
    Dev { col, dir, clock: clock.clone() }
}

/// An exact copy of a device's collection and media (the same device, another timeline).
fn fork(d: &Dev) -> Dev {
    let bytes = d.col.core().db().serialize().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let media = d.dir.path().join("media");
    if media.exists() {
        std::fs::create_dir_all(dir.path().join("media")).unwrap();
        for f in std::fs::read_dir(&media).unwrap() {
            let f = f.unwrap();
            std::fs::copy(f.path(), dir.path().join("media").join(f.file_name())).unwrap();
        }
    }
    let mut col = Collection::open_with(
        Db::open_from_bytes(&bytes).unwrap(),
        BlobStore::new(dir.path()),
        Some(d.clock.clock_fn()),
        Some(Box::new(|_| 0)),
    );
    col.open().unwrap();
    Dev { col, dir, clock: d.clock.clone() }
}

fn export(d: &Dev) -> Vec<u8> {
    d.col.export_sync_bundle(&SyncExportOptions::default()).unwrap()
}

fn merge(d: &mut Dev, bytes: &[u8]) -> SyncReport {
    d.col.merge_sync_bundle(bytes).unwrap()
}

/// Every synced object, sorted, as text.
fn dump(d: &Dev) -> String {
    let core = d.col.core();
    let ctx = state::ctx(core).unwrap();
    let mut out = String::new();
    for kind in Kind::ALL {
        let mut rows = tables::read_all(core.db(), kind, &ctx).unwrap();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        out.push_str(kind.name());
        out.push('\n');
        for (k, p) in rows {
            out.push_str(&format!("{k} {}\n", serde_json::Value::Array(p)));
        }
    }
    out
}

fn revlog_keys(d: &Dev) -> BTreeSet<String> {
    let ctx = state::ctx(d.col.core()).unwrap();
    tables::read_all(d.col.core().db(), Kind::Revlog, &ctx).unwrap().into_iter().map(|(k, _)| k).collect()
}

fn basic(d: &Dev) -> NotetypeId {
    d.col.list_notetypes().unwrap().into_iter().find(|n| n.name == "Basic").unwrap().id
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn tick(d: &Dev, ms: i64) {
    d.clock.advance(ms);
}

/// What the engine does after every write: stamp the change now.
fn stamp(d: &Dev) {
    d.col.sync_after_write();
}

/// Three devices that share one starting collection: A made it, B and C started empty and
/// merged it, A merged theirs back.
fn trio(seed_notes: usize) -> (Dev, Dev, Dev) {
    let c = clock();
    let a = new_dev(&c);
    let nt = basic(&a);
    let spanish = a.col.create_deck("Spanish", None).unwrap().id;
    for i in 0..seed_notes {
        tick(&a, 7);
        let deck = if i % 2 == 0 { 1 } else { spanish };
        a.col.add_note(nt, deck, &strs(&[&format!("front {i}"), &format!("back {i}")]), &strs(&["seed"])).unwrap();
    }
    tick(&a, 100);
    let mut a = a;
    let mut b = new_dev(&c);
    let mut cc = new_dev(&c);
    let base = export(&a);
    merge(&mut b, &base);
    merge(&mut cc, &base);
    let bb = export(&b);
    let bc = export(&cc);
    merge(&mut a, &bb);
    merge(&mut a, &bc);
    let all = export(&a);
    merge(&mut b, &all);
    merge(&mut cc, &all);
    assert_eq!(dump(&a), dump(&b));
    assert_eq!(dump(&a), dump(&cc));
    (a, b, cc)
}

fn note_ids(d: &Dev) -> Vec<i64> {
    d.col.core().db().all("SELECT id FROM notes ORDER BY id", [], |r| r.get(0)).unwrap()
}

fn card_ids(d: &Dev, sql_filter: &str) -> Vec<i64> {
    d.col.core().db().all(&format!("SELECT id FROM cards {sql_filter} ORDER BY id"), [], |r| r.get(0)).unwrap()
}

fn deck_ids(d: &Dev) -> Vec<i64> {
    d.col.core().db().all("SELECT id FROM decks ORDER BY id", [], |r| r.get(0)).unwrap()
}

fn pick<T: Copy>(rng: &mut StdRng, v: &[T]) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[rng.random_range(0..v.len())])
    }
}

/// One random user action. `wild` adds actions whose concurrent combinations need repairs:
/// deleting notes, decks with the same name on two devices.
fn random_op(d: &mut Dev, rng: &mut StdRng, label: &str, n: usize, wild: bool) {
    tick(d, rng.random_range(1..400));
    let words = ["amor", "casa", "perro", "gato", "libro", "sol", "luna"];
    let op = rng.random_range(0..if wild { 12 } else { 10 });
    let _ = match op {
        0 => {
            let decks = deck_ids(d);
            let deck = pick(rng, &decks).unwrap_or(1);
            let front = format!("{} {label}{n}", words[rng.random_range(0..words.len())]);
            d.col.add_note(basic(d), deck, &strs(&[&front, "x"]), &[]).map(|_| ())
        }
        1 => match pick(rng, &note_ids(d)) {
            Some(id) => {
                let back = format!("{} {label}{n}", words[rng.random_range(0..words.len())]);
                let tags = if rng.random_range(0..2) == 0 { strs(&["t1"]) } else { strs(&["t2", label]) };
                d.col.update_note(id, &strs(&["edited", &back]), &tags).map(|_| ())
            }
            None => Ok(()),
        },
        2..=4 => match pick(rng, &card_ids(d, "WHERE queue >= 0")) {
            Some(id) => d.col.answer_card(id, Rating::from_i64(rng.random_range(1..5)).unwrap(), 4000).map(|_| ()),
            None => Ok(()),
        },
        5 => match pick(rng, &card_ids(d, "")) {
            Some(id) => d.col.suspend_cards(&[id], rng.random_range(0..2) == 0),
            None => Ok(()),
        },
        6 => match pick(rng, &card_ids(d, "")) {
            Some(id) => d.col.set_flag(&[id], rng.random_range(0..4)),
            None => Ok(()),
        },
        7 => match (pick(rng, &card_ids(d, "")), pick(rng, &deck_ids(d))) {
            (Some(c), Some(deck)) => d.col.bulk(&[c], &BulkAction::ChangeDeck { deck_id: deck }).map(|_| ()),
            _ => Ok(()),
        },
        8 => d.col.create_deck(&format!("{label} deck {n}"), None).map(|_| ()),
        9 => d.col.undo().map(|_| ()),
        10 => match pick(rng, &note_ids(d)) {
            Some(id) => d.col.delete_notes(&[id]),
            None => Ok(()),
        },
        _ => d.col.create_deck(&format!("Shared {}", rng.random_range(0..3)), None).map(|_| ()),
    };
    stamp(d);
}

#[test]
fn merge_is_commutative_idempotent_and_associative() {
    for seed in 0..30u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let (mut a, mut b, mut c) = trio(6);
        for n in 0..15 {
            random_op(&mut a, &mut rng, "a", n, false);
            random_op(&mut b, &mut rng, "b", n, false);
            random_op(&mut c, &mut rng, "c", n, false);
        }
        let (ba, bb, bc) = (export(&a), export(&b), export(&c));

        // A ⊔ B == B ⊔ A
        let mut ab = fork(&a);
        merge(&mut ab, &bb);
        let mut ba_ = fork(&b);
        merge(&mut ba_, &ba);
        assert_eq!(dump(&ab), dump(&ba_), "seed {seed}: A⊔B != B⊔A");

        // Idempotence: merging the same bundle again, or a device's own bundle, changes nothing.
        let before = dump(&ab);
        assert_eq!(merge(&mut ab, &bb).rows_in, 0);
        let own = export(&ab);
        assert_eq!(merge(&mut ab, &own).rows_in, 0);
        let mut aa = fork(&a);
        assert_eq!(merge(&mut aa, &ba).rows_in, 0);
        assert_eq!(dump(&ab), before);
        assert_eq!(dump(&aa), dump(&a));

        // (A ⊔ B) ⊔ C == A ⊔ (B ⊔ C) == C ⊔ (A ⊔ B)
        let mut left = fork(&ab);
        merge(&mut left, &bc);
        let mut bc_ = fork(&b);
        merge(&mut bc_, &bc);
        let mut right = fork(&a);
        merge(&mut right, &export(&bc_));
        let mut other = fork(&c);
        merge(&mut other, &export(&ab));
        assert_eq!(dump(&left), dump(&right), "seed {seed}: (A⊔B)⊔C != A⊔(B⊔C)");
        assert_eq!(dump(&left), dump(&other), "seed {seed}: (A⊔B)⊔C != C⊔(A⊔B)");
        let _ = (&mut a, &mut b, &mut c);
    }
}

/// Exchanges full bundles between every pair until nothing changes.
fn settle(devs: &mut [Dev]) {
    for _ in 0..8 {
        let bundles: Vec<Vec<u8>> = devs.iter().map(export).collect();
        let mut changed = 0;
        for (i, d) in devs.iter_mut().enumerate() {
            for (j, b) in bundles.iter().enumerate() {
                if i != j {
                    changed += merge(d, b).rows_in;
                }
            }
        }
        if changed == 0 {
            return;
        }
    }
    panic!("devices did not settle");
}

fn assert_consistent(d: &Dev) {
    let db = d.col.core().db();
    assert_eq!(db.count("SELECT count(*) FROM cards WHERE nid NOT IN (SELECT id FROM notes)", []).unwrap(), 0, "orphan cards");
    assert_eq!(
        db.count("SELECT count(*) FROM notes WHERE id NOT IN (SELECT nid FROM cards)", []).unwrap(),
        0,
        "notes without cards"
    );
    assert_eq!(
        db.count("SELECT count(*) FROM cards WHERE did NOT IN (SELECT id FROM decks)", []).unwrap(),
        0,
        "cards in no deck"
    );
    assert_eq!(
        db.count("SELECT count(*) FROM (SELECT 1 FROM cards GROUP BY nid, ord HAVING count(*) > 1)", []).unwrap(),
        0,
        "duplicate cards"
    );
    assert_eq!(db.count("SELECT count(*) FROM decks WHERE instr(name, char(31)) > 0", []).unwrap(), 0, "parked deck names");
}

#[test]
fn random_concurrent_work_converges_and_keeps_every_review() {
    for seed in 0..60u64 {
        let mut rng = StdRng::seed_from_u64(1000 + seed);
        let (a, b, c) = trio(5);
        let mut devs = vec![a, b, c];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut undone: BTreeSet<String> = BTreeSet::new();
        for round in 0..4 {
            for step in 0..10 {
                for i in 0..devs.len() {
                    let before = revlog_keys(&devs[i]);
                    random_op(&mut devs[i], &mut rng, ["a", "b", "c"][i], round * 100 + step, true);
                    let after = revlog_keys(&devs[i]);
                    undone.extend(before.difference(&after).cloned());
                    seen.extend(after);
                }
            }
            // Some devices exchange some bundles, in a random order.
            for _ in 0..rng.random_range(0..4) {
                let from = rng.random_range(0..devs.len());
                let to = rng.random_range(0..devs.len());
                if from != to {
                    let bytes = export(&devs[from]);
                    merge(&mut devs[to], &bytes);
                    let k = revlog_keys(&devs[to]);
                    seen.extend(k);
                }
            }
        }
        settle(&mut devs);
        let d0 = dump(&devs[0]);
        for d in &devs[1..] {
            assert_eq!(dump(d), d0, "seed {seed}: devices differ after settling");
        }
        for d in &devs {
            assert_consistent(d);
        }
        let expected: BTreeSet<String> = seen.difference(&undone).cloned().collect();
        assert_eq!(revlog_keys(&devs[0]), expected, "seed {seed}: reviews lost or resurrected");
    }
}

#[test]
fn reviews_of_one_card_on_two_devices_are_both_kept() {
    let (mut a, mut b, _) = trio(2);
    let card = card_ids(&a, "")[0];
    tick(&a, 1000);
    a.col.answer_card(card, Rating::Good, 3000).unwrap();
    stamp(&a);
    tick(&b, 1000);
    b.col.answer_card(card, Rating::Again, 3000).unwrap();
    stamp(&b);
    let (ba, bb) = (export(&a), export(&b));
    let ra = merge(&mut a, &bb);
    let rb = merge(&mut b, &ba);
    assert_eq!((ra.reviews_added, rb.reviews_added), (1, 1));
    assert_eq!(ra.conflicts_resolved, 1);
    assert_eq!(revlog_keys(&a).len(), 2);
    assert_eq!(dump(&a), dump(&b));
    // The card's schedule is the later answer's (B's), not a replay.
    let reps: i64 = a.col.core().db().scalar("SELECT reps FROM cards WHERE id = ?", [card]).unwrap().unwrap();
    assert_eq!(reps, 1);
    let queue: i64 = a.col.core().db().scalar("SELECT type FROM cards WHERE id = ?", [card]).unwrap().unwrap();
    assert_eq!(queue, CardType::Learn as i64);
}

#[test]
fn a_later_edit_beats_an_earlier_delete_and_a_later_delete_beats_an_edit() {
    // Delete first, edit later: the note survives with the edit, and gets a card back.
    let (mut a, mut b, _) = trio(2);
    let n = note_ids(&a)[0];
    tick(&a, 100);
    a.col.delete_notes(&[n]).unwrap();
    stamp(&a);
    tick(&b, 100);
    b.col.update_note(n, &strs(&["kept", "yes"]), &[]).unwrap();
    stamp(&b);
    let (ba, bb) = (export(&a), export(&b));
    merge(&mut a, &bb);
    merge(&mut b, &ba);
    let mut devs = [a, b];
    settle(&mut devs);
    let [a, b] = devs;
    assert_eq!(dump(&a), dump(&b));
    let flds: String = a.col.core().db().scalar("SELECT flds FROM notes WHERE id = ?", [n]).unwrap().unwrap();
    assert_eq!(flds, "kept\u{1f}yes");
    assert_consistent(&a);

    // Edit first, delete later: the note is gone everywhere.
    let (mut a, mut b, _) = trio(2);
    let n = note_ids(&a)[0];
    tick(&b, 100);
    b.col.update_note(n, &strs(&["edited", "x"]), &[]).unwrap();
    stamp(&b);
    tick(&a, 100);
    a.col.delete_notes(&[n]).unwrap();
    stamp(&a);
    let (ba, bb) = (export(&a), export(&b));
    merge(&mut a, &bb);
    merge(&mut b, &ba);
    assert_eq!(dump(&a), dump(&b));
    assert!(!note_ids(&b).contains(&n));
    assert_consistent(&b);
}

/// Sets the stamp of every row of `kind`/`key` in a bundle.
fn restamp(bytes: &[u8], kind: &str, key: &str, hlc: i64) -> Vec<u8> {
    let mut b = bundle::decode(bytes).unwrap();
    for r in &mut b.rows {
        if r.0 == kind && r.1 == key {
            r.2 = hlc;
        }
    }
    bundle::encode(&b).unwrap()
}

#[test]
fn a_delete_and_an_edit_with_the_same_clock_are_decided_by_device_id() {
    let (a, b, _) = trio(2);
    let n = note_ids(&a)[0];
    tick(&a, 100);
    a.col.delete_notes(&[n]).unwrap();
    tick(&b, 100);
    b.col.update_note(n, &strs(&["same", "time"]), &[]).unwrap();
    let tie = (T0 + 10_000_000) << clock::COUNTER_BITS;
    let ba = restamp(&export(&a), "notes", &n.to_string(), tie);
    let bb = restamp(&export(&b), "notes", &n.to_string(), tie);
    let da = state::device(a.col.core().db()).unwrap().unwrap();
    let db_ = state::device(b.col.core().db()).unwrap().unwrap();
    // Two fresh replicas that only know the bundles, merged in both orders.
    let mut p = new_dev(&a.clock);
    merge(&mut p, &ba);
    merge(&mut p, &bb);
    let mut q = new_dev(&a.clock);
    merge(&mut q, &bb);
    merge(&mut q, &ba);
    assert_eq!(dump(&p), dump(&q));
    assert_eq!(note_ids(&p).contains(&n), db_ > da, "the larger device id wins a tie");
}

#[test]
fn stamps_are_taken_when_the_engine_writes_not_when_it_syncs() {
    let c = clock();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let open = |dir: &std::path::Path| {
        let col = Collection::open_dir(dir, Some(c.clock_fn()), Some(Box::new(|_| 0))).unwrap();
        let mut e = crate::rpc::Engine::from_collection(dir, col, false);
        e.dispatch("open", &[], None, &mut |_| {}).unwrap();
        e
    };
    let mut ea = open(dir_a.path());
    let mut eb = open(dir_b.path());
    let call = |e: &mut crate::rpc::Engine, m: &str, args: Vec<serde_json::Value>, bytes: Option<&[u8]>| {
        e.dispatch(m, &args, bytes, &mut |_| {}).unwrap()
    };
    let nt = ea.collection().list_notetypes().unwrap().into_iter().find(|n| n.name == "Basic").unwrap().id;
    let added = call(&mut ea, "addNote", vec![nt.into(), 1.into(), serde_json::json!(["q", "a"]), serde_json::json!([])], None)
        .into_json()
        .unwrap();
    let nid = added["noteId"].as_i64().unwrap();
    let first = call(&mut ea, "exportSyncBundle", vec![], None).into_bytes().unwrap().unwrap();
    call(&mut eb, "mergeSyncBundle", vec![], Some(&first));
    let back = call(&mut eb, "exportSyncBundle", vec![], None).into_bytes().unwrap().unwrap();
    call(&mut ea, "mergeSyncBundle", vec![], Some(&back));
    // A edits first, B edits later, A syncs last: B's edit is the later one and wins.
    c.advance(1000);
    call(&mut ea, "updateNote", vec![nid.into(), serde_json::json!(["from a", "a"]), serde_json::json!([])], None);
    c.advance(1000);
    call(&mut eb, "updateNote", vec![nid.into(), serde_json::json!(["from b", "b"]), serde_json::json!([])], None);
    c.advance(60_000);
    let from_b = call(&mut eb, "exportSyncBundle", vec![], None).into_bytes().unwrap().unwrap();
    c.advance(60_000);
    let from_a = call(&mut ea, "exportSyncBundle", vec![serde_json::json!({})], None).into_bytes().unwrap().unwrap();
    let report = call(&mut ea, "mergeSyncBundle", vec![], Some(&from_b)).into_json().unwrap();
    assert_eq!(report["rowsIn"], 1);
    assert_eq!(report["conflictsResolved"], 1);
    call(&mut eb, "mergeSyncBundle", vec![], Some(&from_a));
    for e in [&ea, &eb] {
        let flds: String = e.collection().core().db().scalar("SELECT flds FROM notes WHERE id = ?", [nid]).unwrap().unwrap();
        assert_eq!(flds, "from b\u{1f}b");
    }
    let status = call(&mut ea, "syncStatus", vec![], None).into_json().unwrap();
    assert_eq!(status["enabled"], true);
    assert_eq!(status["peers"].as_array().unwrap().len(), 1);
}

#[test]
fn media_with_the_same_name_keeps_both_files() {
    let (mut a, mut b, _) = trio(1);
    tick(&a, 10);
    assert_eq!(a.col.add_media("pic.png", b"from a").unwrap(), "pic.png");
    stamp(&a);
    tick(&b, 10);
    assert_eq!(b.col.add_media("pic.png", b"from b").unwrap(), "pic.png");
    stamp(&b);
    let (ba, bb) = (export(&a), export(&b));
    let ra = merge(&mut a, &bb);
    let rb = merge(&mut b, &ba);
    assert!(ra.media_added >= 1);
    assert!(rb.media_needed.is_empty());
    let mut devs = [a, b];
    settle(&mut devs);
    let [a, b] = devs;
    assert_eq!(dump(&a), dump(&b));
    // The later file (B's) owns the name everywhere; A's file stays under its hashed name.
    let kept = crate::anki::add_hash_suffix("pic.png", &crate::text::sha1_hex(b"from a"));
    for d in [&a, &b] {
        assert_eq!(d.col.get_media("pic.png").unwrap().as_deref(), Some(&b"from b"[..]));
        assert_eq!(d.col.get_media(&kept).unwrap().as_deref(), Some(&b"from a"[..]));
        assert!(d.col.sync_media_state().unwrap().need.is_empty());
    }
}

#[test]
fn media_files_travel_in_hand_off_bundles_and_folder_bundles_list_them() {
    let (a, mut b, _) = trio(1);
    a.col.add_media("song.mp3", b"la la").unwrap();
    // A folder bundle leaves files out; B then needs the file and can take it by checksum.
    let folder = a.col.export_sync_bundle(&SyncExportOptions { folder: true, peers: vec![] }).unwrap();
    let r = merge(&mut b, &folder);
    assert_eq!(r.media_needed, vec![SyncMediaRef { name: "song.mp3".into(), sha1: crate::text::sha1_hex(b"la la") }]);
    assert!(b.col.add_sync_media(&r.media_needed[0].sha1, b"tampered").is_err());
    assert_eq!(b.col.add_sync_media(&r.media_needed[0].sha1, b"la la").unwrap(), 1);
    // A hand-off bundle carries the file itself.
    let mut c2 = new_dev(&a.clock);
    let r = merge(&mut c2, &export(&a));
    assert_eq!(r.media_added, 1);
    assert_eq!(c2.col.get_media("song.mp3").unwrap().as_deref(), Some(&b"la la"[..]));
}

#[test]
fn bundle_bytes_round_trip_and_merge_the_same() {
    let (mut a, mut b, _) = trio(4);
    a.col.add_media("x.png", b"img").unwrap();
    let card = card_ids(&a, "")[0];
    a.col.answer_card(card, Rating::Easy, 2000).unwrap();
    let bytes = export(&a);
    let decoded = bundle::decode(&bytes).unwrap();
    assert_eq!(bundle::encode(&decoded).unwrap(), bytes);
    assert_eq!(decoded.header.device, state::device(a.col.core().db()).unwrap().unwrap());
    assert!(decoded.rows.iter().any(|r| r.0 == "revlog"));
    assert_eq!(decoded.media.len(), 1);
    let mut x = fork(&b);
    let mut y = fork(&b);
    merge(&mut x, &bytes);
    merge(&mut y, &bundle::encode(&decoded).unwrap());
    assert_eq!(dump(&x), dump(&y));
    assert_eq!(dump(&x), dump(&a));
    assert!(b.col.merge_sync_bundle(b"PK\x03\x04 not really").is_err());
}

#[test]
fn folder_bundles_leave_out_what_peers_have_and_new_devices_catch_up() {
    let (a, mut b, _) = trio(30);
    let dev_b = state::device(b.col.core().db()).unwrap().unwrap();
    // A knows B's vector (from the trio exchange). After one edit, the folder bundle is tiny.
    let n = note_ids(&a)[3];
    tick(&a, 50);
    a.col.update_note(n, &strs(&["changed", "x"]), &[]).unwrap();
    let full = export(&a);
    let delta = a.col.export_sync_bundle(&SyncExportOptions { folder: true, peers: vec![dev_b.clone()] }).unwrap();
    let full_rows = bundle::decode(&full).unwrap().rows.len();
    let delta_rows = bundle::decode(&delta).unwrap().rows.len();
    assert!(delta_rows * 4 < full_rows && full_rows > 60, "delta {delta_rows}, full {full_rows}");
    let r = merge(&mut b, &delta);
    assert!(r.complete);
    assert_eq!(dump(&a), dump(&b));
    // A device that has seen nothing gets an incomplete picture from a delta, then the rest.
    let mut d = new_dev(&a.clock);
    let r = merge(&mut d, &delta);
    assert!(!r.complete);
    assert!(state::vector(d.col.core().db()).unwrap().len() == 1, "an incomplete merge does not adopt the sender's vector");
    let r = merge(&mut d, &full);
    assert!(r.complete);
    assert_eq!(dump(&d), dump(&a));
}

#[test]
fn merging_backs_up_first_clears_undo_and_reports() {
    let (a, mut b, _) = trio(2);
    let n = note_ids(&a)[0];
    tick(&a, 10);
    a.col.update_note(n, &strs(&["new", "x"]), &[]).unwrap();
    tick(&b, 10);
    b.col.set_flag(&card_ids(&b, ""), 2).unwrap();
    assert!(b.col.undo_label().unwrap().is_some());
    let backups = b.col.list_backups().unwrap().len();
    let r = merge(&mut b, &export(&a));
    assert_eq!(r.rows_in, 1);
    assert_eq!(b.col.list_backups().unwrap().len(), backups + 1);
    assert_eq!(b.col.undo_label().unwrap(), None);
    // Nothing new: no backup.
    let r = merge(&mut b, &export(&a));
    assert_eq!(r.rows_in, 0);
    assert_eq!(b.col.list_backups().unwrap().len(), backups + 1);
}

#[test]
fn a_restored_backup_wins_over_older_changes_elsewhere() {
    let (mut a, mut b, _) = trio(2);
    let n = note_ids(&a)[0];
    let old_device = state::device(a.col.core().db()).unwrap().unwrap();
    tick(&a, 10);
    a.col.update_note(n, &strs(&["before backup", "x"]), &[]).unwrap();
    let snapshot = a.col.core().db().serialize().unwrap();
    tick(&a, 10);
    a.col.update_note(n, &strs(&["after backup", "x"]), &[]).unwrap();
    merge(&mut b, &export(&a));
    tick(&b, 10);
    b.col.update_note(n, &strs(&["edited on b", "x"]), &[]).unwrap();
    export(&b);
    tick(&a, 1000);
    a.col.replace_database(&snapshot).unwrap();
    a.col.open().unwrap();
    let st = a.col.sync_status().unwrap();
    assert_ne!(st.device_id.as_deref(), Some(old_device.as_str()));
    assert_eq!(st.former_devices, vec![old_device]);
    let (ba, bb) = (export(&a), export(&b));
    merge(&mut a, &bb);
    merge(&mut b, &ba);
    for d in [&a, &b] {
        let flds: String = d.col.core().db().scalar("SELECT flds FROM notes WHERE id = ?", [n]).unwrap().unwrap();
        assert_eq!(flds, "before backup\u{1f}x");
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn due_days_survive_collections_created_on_different_days() {
    let c = clock();
    let mut a = new_dev(&c);
    let nt = basic(&a);
    a.col.add_note(nt, 1, &strs(&["q", "a"]), &[]).unwrap();
    let card = card_ids(&a, "")[0];
    a.col.answer_card(card, Rating::Easy, 1000).unwrap();
    c.advance(3 * 86_400_000);
    let mut b = new_dev(&c);
    merge(&mut b, &export(&a));
    let due = |d: &Dev| -> i64 {
        let due: i64 = d.col.core().db().scalar("SELECT due FROM cards WHERE id = ?", [card]).unwrap().unwrap();
        due - d.col.core().today().unwrap()
    };
    assert_ne!(a.col.core().crt().unwrap(), b.col.core().crt().unwrap());
    assert_eq!(due(&a), due(&b));
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn a_new_device_adopts_presets_and_settings_and_built_in_note_types_merge() {
    let c = clock();
    let mut a = new_dev(&c);
    let nt = basic(&a);
    a.col.add_note(nt, 1, &strs(&["q", "a"]), &[]).unwrap();
    let mut preset = a.col.get_preset(1).unwrap();
    preset.config.new_per_day = 7;
    a.col.save_preset(&DeckConfigPresetInput { id: Some(1), name: preset.name.clone(), config: preset.config.clone() }).unwrap();
    a.col.set_settings(&CollectionSettingsPatch { card_font_size: Some(30), ..Default::default() }).unwrap();
    c.advance(5000);
    // B starts later with its own built-in note types, then shares its file first.
    let mut b = new_dev(&c);
    let from_b = export(&b);
    merge(&mut a, &from_b);
    merge(&mut b, &export(&a));
    let mut devs = [a, b];
    settle(&mut devs);
    let [a, b] = devs;
    assert_eq!(b.col.get_preset(1).unwrap().config.new_per_day, 7);
    assert_eq!(b.col.get_settings().unwrap().card_font_size, 30);
    let names: Vec<String> = a.col.list_notetypes().unwrap().into_iter().map(|n| n.name).collect();
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(names.len(), unique.len(), "duplicate note types: {names:?}");
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn decks_with_the_same_name_get_distinct_names() {
    let (mut a, mut b, _) = trio(1);
    tick(&a, 10);
    let da = a.col.create_deck("French", None).unwrap().id;
    tick(&b, 10);
    let db_ = b.col.create_deck("French", None).unwrap().id;
    let (ba, bb) = (export(&a), export(&b));
    merge(&mut a, &bb);
    merge(&mut b, &ba);
    let mut devs = [a, b];
    settle(&mut devs);
    let [a, b] = devs;
    assert_eq!(dump(&a), dump(&b));
    assert_eq!(a.col.get_deck(da.min(db_)).unwrap().name, "French");
    assert_eq!(a.col.get_deck(da.max(db_)).unwrap().name, "French (2)");
}

#[test]
fn stamping_ignores_local_only_columns_and_status_reports() {
    let (a, _, _) = trio(2);
    let st = a.col.sync_status().unwrap();
    assert!(st.enabled && st.last_sync.is_some() && st.peers.len() == 2);
    let count = |d: &Dev| d.col.core().db().count("SELECT count(*) FROM sync_rows", []).unwrap();
    let hlc = state::hlc(a.col.core().db()).unwrap();
    let before = count(&a);
    // Collapsing nothing, a backup, a day counter: no new stamps.
    a.col.core().db().run("UPDATE decks SET today = '{\"day\":1}'", []).unwrap();
    a.col.backup_now().unwrap();
    assert_eq!(state::stamp_pending(a.col.core()).unwrap(), 0);
    assert_eq!(state::hlc(a.col.core().db()).unwrap(), hlc);
    assert_eq!(count(&a), before);
    let folder = a.col.set_sync_folder(Some("/tmp/x")).unwrap();
    assert_eq!(folder.folder.as_deref(), Some("/tmp/x"));
}
