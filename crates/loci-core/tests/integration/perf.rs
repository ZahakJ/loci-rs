//! Large-collection timings (recall `tests/unit/integration/perf.test.ts`). Ignored by default:
//! `cargo test --release -p loci-core --test integration -- --ignored --nocapture perf`.
//! Imports the 100k-card package named by `LOCI_PERF_APKG` (default `fixtures/large-100k.apkg`,
//! not committed; generate one with recall's `buildLargeCollection`) and skips when absent.

use super::helpers::*;
use loci_core::types::*;
use std::time::Instant;

const DEFAULT_APKG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/large-100k.apkg");

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let r = f();
    println!("{:<32} {:>8.1} ms", label, t0.elapsed().as_secs_f64() * 1000.0);
    r
}

#[test]
#[ignore = "perf: run in release with --ignored --nocapture"]
fn imports_counts_studies_browses_and_computes_stats_on_a_large_collection() {
    let path = std::env::var("LOCI_PERF_APKG").unwrap_or_else(|_| DEFAULT_APKG.to_string());
    let Ok(bytes) = std::fs::read(&path) else {
        println!("perf: {path} not found; skipping");
        return;
    };
    let mut h = make_collection_at(1_750_680_000_000); // 2025-06-23 12:00 UTC
    let n = bytes.len();
    let report = time(&format!("import ({} MB package)", n / 1_000_000), || {
        h.col.import_package(bytes, &merge_options(), &mut no_progress()).unwrap()
    });
    println!("  notes {} cards {} revlog {}", report.notes_added, report.cards_added, report.reviews_added);
    time("deckTree", || h.col.deck_tree().unwrap());
    let sc = time("startStudy", || h.col.start_study(None).unwrap());
    let mut cur = sc;
    let t0 = Instant::now();
    let mut answered = 0;
    for _ in 0..50 {
        let Some(c) = cur.take() else { break };
        cur = h.col.answer_card(c.card.id, Rating::Good, 3000).unwrap().next;
        answered += 1;
    }
    println!("{:<32} {:>8.1} ms", "answer (mean of 50)", t0.elapsed().as_secs_f64() * 1000.0 / answered.max(1) as f64);
    let req = |offset: i64, column: BrowseColumn, query: &str| BrowseRequest {
        query: query.into(),
        mode: BrowseMode::Cards,
        sort: BrowseSort { column, descending: false },
        offset,
        limit: 100,
    };
    time("browse page (sort by due, 0)", || h.col.browse(&req(0, BrowseColumn::Due, "deck:*")).unwrap());
    time("browse page (sort by due, 99900)", || h.col.browse(&req(99_900, BrowseColumn::Due, "deck:*")).unwrap());
    time("browse page (retrievability)", || h.col.browse(&req(0, BrowseColumn::Retrievability, "prop:r<0.95")).unwrap());
    time("stats", || h.col.stats(&StatsRequest { deck_id: None }).unwrap());
    time("insights", || h.col.insights().unwrap());
    time("backlogReport", || h.col.backlog_report(None).unwrap());
    let out = time("export", || {
        h.col
            .export_package(&ExportOptions { deck_id: None, include_scheduling: true, include_media: false }, &mut no_progress())
            .unwrap()
    });
    println!("  export {} MB", out.len() / 1_000_000);
}
