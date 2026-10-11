//! Port of recall `tests/unit/integration/helpers.ts`.

use loci_core::collection::Collection;
use loci_core::db::Db;
use loci_core::types::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(FIXTURES).join(name)
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedTotals {
    pub notes: i64,
    pub cards: i64,
    pub revlog: i64,
    pub media: i64,
    pub notetypes: i64,
    pub decks_after_import: i64,
    pub fsrs_cards: i64,
    pub filtered_cards: i64,
    pub leech_notes: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedDeck {
    pub total: i64,
    pub new: i64,
    pub new_limited: i64,
    pub learn_intraday: i64,
    pub learn_due_now: i64,
    pub interday_due: i64,
    pub review_due: i64,
    pub review_overdue: i64,
    pub review_future: i64,
    pub suspended: i64,
    pub buried: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedCard {
    pub label: String,
    pub id: i64,
    pub deck: String,
    #[serde(rename = "type")]
    pub ctype: i64,
    pub queue: i64,
    pub due: i64,
    pub ivl: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
pub struct Expected {
    pub crt: i64,
    pub now: i64,
    pub today: i64,
    pub next_day_at: i64,
    pub totals: ExpectedTotals,
    pub decks: BTreeMap<String, ExpectedDeck>,
    pub cards: Vec<ExpectedCard>,
}

static EXPECTED: LazyLock<Expected> =
    LazyLock::new(|| serde_json::from_slice(&fixture("palace.expected.json")).expect("palace.expected.json parses"));

pub fn expected() -> &'static Expected {
    &EXPECTED
}

/// A controllable clock (ms) shared with the collection.
#[derive(Clone)]
pub struct Clock(pub Arc<AtomicI64>);

impl Clock {
    pub fn new(ms: i64) -> Clock {
        Clock(Arc::new(AtomicI64::new(ms)))
    }
    pub fn ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
    pub fn boxed(&self) -> Box<dyn Fn() -> i64 + Send + Sync> {
        let c = self.0.clone();
        Box::new(move || c.load(Ordering::SeqCst))
    }
}

pub struct Harness {
    pub col: Collection,
    pub clock: Clock,
    pub dir: tempfile::TempDir,
}

impl Harness {
    pub fn db(&self) -> &Db {
        self.col.core().db()
    }
    pub fn db_path(&self) -> PathBuf {
        self.dir.path().join("collection.sqlite")
    }
    pub fn count(&self, sql: &str) -> i64 {
        self.db().count(sql, []).expect("count")
    }
    pub fn count_p<P: rusqlite::Params>(&self, sql: &str, params: P) -> i64 {
        self.db().count(sql, params).expect("count")
    }
    pub fn scalar<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.db().scalar::<T, _>(sql, []).expect("scalar").expect("a row")
    }
    pub fn scalar_p<T: rusqlite::types::FromSql, P: rusqlite::Params>(&self, sql: &str, params: P) -> T {
        self.db().scalar::<T, _>(sql, params).expect("scalar").expect("a row")
    }
    pub fn ids(&self, sql: &str) -> Vec<i64> {
        let mut v: Vec<i64> = self.db().all(sql, [], |r| r.get(0)).expect("ids");
        v.sort_unstable();
        v
    }
    pub fn deck_id(&self, name: &str) -> i64 {
        self.scalar_p("SELECT id FROM decks WHERE name = ?", [name])
    }
    pub fn notetype_by_name(&self, name: &str) -> Notetype {
        self.col.list_notetypes().unwrap().into_iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no note type {name}"))
    }
    pub fn builtin(&self, kind: BuiltinKind) -> Notetype {
        self.col
            .list_notetypes()
            .unwrap()
            .into_iter()
            .find(|n| n.config.builtin == Some(kind))
            .unwrap_or_else(|| panic!("no builtin {kind:?}"))
    }
    /// Re-opens a second `Collection` on the same directory (after `close()`), sharing the clock.
    pub fn reopen(&self) -> Collection {
        Collection::open_dir(self.dir.path(), Some(self.clock.boxed()), Some(Box::new(|_| 0))).expect("open_dir")
    }
}

pub fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

/// Every row of a query as debug strings (for snapshot comparisons across collections).
pub fn rows(db: &Db, sql: &str) -> Vec<Vec<String>> {
    let mut st = db.conn().prepare(sql).expect("prepare");
    let n = st.column_count();
    let out = st
        .query_map([], |r| {
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                v.push(format!("{:?}", r.get_ref(i)?));
            }
            Ok(v)
        })
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    out
}

pub fn no_progress() -> impl FnMut(Progress) {
    |_| {}
}

/// A fresh file-backed collection whose clock starts at `start` (ms), UTC.
pub fn make_collection_at(start: i64) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = Clock::new(start);
    let mut col = Collection::open_dir(dir.path(), Some(clock.boxed()), Some(Box::new(|_| 0))).expect("open_dir");
    let report = col.open().expect("open");
    assert!(report.ok, "open failed: {:?}", report.problems);
    Harness { col, clock, dir }
}

/// A fresh collection whose clock starts at the fixture's "now".
pub fn make_collection() -> Harness {
    make_collection_at(expected().now * 1000)
}

pub fn merge_options() -> ImportOptions {
    ImportOptions { mode: ImportMode::Merge, keep_scheduling: true, conflict: ConflictPolicy::Newer }
}

pub fn import_fixture(h: &mut Harness, name: &str) -> ImportReport {
    h.col.import_package(fixture(name), &merge_options(), &mut no_progress()).unwrap_or_else(|e| panic!("import {name}: {e}"))
}

/// A collection with the legacy-2 fixture imported (with scheduling).
pub fn imported_fixture() -> Harness {
    imported_fixture_named("palace-legacy2.apkg")
}

pub fn imported_fixture_named(name: &str) -> Harness {
    let mut h = make_collection();
    import_fixture(&mut h, name);
    h
}

pub fn flatten(n: &DeckTreeNode) -> Vec<&DeckTreeNode> {
    let mut out = vec![n];
    for c in &n.children {
        out.extend(flatten(c));
    }
    out
}

pub fn find_node(n: &DeckTreeNode, id: DeckId) -> Option<&DeckTreeNode> {
    if n.deck.id == id {
        return Some(n);
    }
    n.children.iter().find_map(|c| find_node(c, id))
}
