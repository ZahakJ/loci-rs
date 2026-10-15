//! Statistics, optimiser, export and backup integration tests.

use super::helpers::*;
use loci_core::anki::read_package;
use loci_core::collection::Collection;
use loci_core::error::ErrorCode;
use loci_core::types::*;

/// serde_json's default float parser is not guaranteed to round-trip the shortest representation
/// exactly (it needs the `float_roundtrip` feature), so parameters read back from the stored JSON
/// may differ from the in-memory ones by an ulp.
fn assert_params_close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= 1e-12 * x.abs().max(1.0), "{x} vs {y}");
    }
}

fn all_decks() -> ExportOptions {
    ExportOptions { deck_id: None, include_scheduling: true, include_media: true }
}

// ── stats and insights on the fixture ──

#[test]
fn computes_every_panel() {
    let h = imported_fixture();
    let s = h.col.stats(&StatsRequest { deck_id: None }).unwrap();
    let max_reviews = s.retention.iter().map(|r| r.reviews).max().unwrap();
    let all = s.retention.iter().find(|r| r.reviews == max_reviews).unwrap();
    let tr = all.true_retention.unwrap();
    assert!(tr > 0.5 && tr < 1.0);
    assert_eq!(all.reviews, h.count("SELECT count(*) FROM revlog WHERE type = 1 AND ease BETWEEN 1 AND 4"));
    assert_eq!(s.retention.len(), 5);
    assert_eq!(s.forecast.len(), 90);
    assert_eq!(s.forecast[0].due, h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due <= 0"));
    assert_eq!(s.forecast[1].due, h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due = 1"));
    assert!(s.forecast.iter().all(|f| f.with_new >= f.due));
    assert!(s.forecast.iter().any(|f| f.with_new > f.due));
    assert_eq!(s.heatmap.len(), 365);
    assert_eq!(s.reviews_per_day.len(), 365);
    let expected_date =
        chrono::DateTime::from_timestamp_millis(h.clock.ms() - 4 * 3_600_000).unwrap().format("%Y-%m-%d").to_string();
    assert_eq!(s.heatmap[364].date, expected_date);
    let year_count: i64 = s.reviews_per_day.iter().map(|d| d.count).sum();
    let next_day_at = h.col.core().next_day_at().unwrap();
    assert_eq!(
        year_count,
        h.count_p("SELECT count(*) FROM revlog WHERE ease BETWEEN 1 AND 4 AND id >= ?", [(next_day_at - 365 * 86400) * 1000])
    );
    assert_eq!(s.intervals.iter().map(|b| b.count).sum::<i64>(), h.count("SELECT count(*) FROM cards WHERE type IN (2, 3)"));
    assert_eq!(
        s.stability.iter().map(|b| b.count).sum::<i64>(),
        h.count("SELECT count(*) FROM cards WHERE type != 0 AND json_extract(data, '$.s') IS NOT NULL")
    );
    assert_eq!(s.difficulty.len(), 10);
    assert_eq!(s.hourly.len(), 24);
    assert_eq!(s.hourly.iter().map(|b| b.reviews).sum::<i64>(), year_count);
    let b = &s.buttons;
    let presses: i64 = b.learn.iter().chain(&b.young).chain(&b.mature).sum();
    assert_eq!(
        presses,
        h.count_p(
            "SELECT count(*) FROM revlog WHERE ease BETWEEN 1 AND 4 AND type IN (0, 1, 2, 3) AND id >= ?",
            [(next_day_at - 365 * 86400) * 1000]
        )
    );
    let cc = &s.card_counts;
    assert_eq!(cc.new + cc.learning + cc.young + cc.mature + cc.suspended + cc.buried, expected().totals.cards);
    assert!(s.momentum.days_studied_last30 > 0);
    assert!(s.momentum.longest_run >= s.momentum.current_run);
    assert_eq!(
        s.today.studied,
        h.count_p("SELECT count(*) FROM revlog WHERE ease BETWEEN 1 AND 4 AND id >= ?", [(next_day_at - 86400) * 1000])
    );
    let deck_stats = h.col.stats(&StatsRequest { deck_id: Some(h.deck_id("History")) }).unwrap();
    let d = &deck_stats.card_counts;
    assert_eq!(d.new + d.mature + d.young + d.learning + d.suspended + d.buried, expected().decks["History"].total);
}

#[test]
fn produces_insights_with_actions() {
    let h = imported_fixture();
    let list = h.col.insights().unwrap();
    let ids: Vec<&str> = list.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&"backlog"), "{ids:?}");
    assert!(ids.contains(&"struggling"), "{ids:?}");
    assert!(ids.contains(&"never-optimised"), "{ids:?}");
    let struggling = list.iter().find(|i| i.id == "struggling").unwrap();
    assert_eq!(struggling.action.as_ref().unwrap().route, "#/browse?q=tag%3Aleech%20or%20prop%3Alapses%3E%3D8");
    for i in &list {
        assert!(i.text.len() > 10, "{}", i.id);
    }
}

#[test]
fn optimises_a_preset_with_the_keep_if_better_rule_and_logs_it_maintenance_auto_optimises() {
    let h = imported_fixture();
    let preset_id = h.col.get_deck(h.deck_id("Science::Physics")).unwrap().config_id;
    let mut progress: Vec<i64> = Vec::new();
    let r = h.col.optimise(preset_id, &mut |p| progress.push(p.done)).unwrap();
    assert!(!progress.is_empty());
    assert_eq!(r.config_id, preset_id);
    assert!(r.reviews > 0);
    assert_eq!(r.before.params.len(), 21);
    assert_eq!(r.after.params.len(), 21);
    if r.adopted {
        assert!(r.after.log_loss < r.before.log_loss);
        assert_params_close(&h.col.get_preset(preset_id).unwrap().config.fsrs_params, &r.after.params);
    } else {
        assert_eq!(h.col.get_preset(preset_id).unwrap().config.fsrs_params, r.before.params);
    }
    assert_eq!(h.count_p("SELECT count(*) FROM optimise_log WHERE config_id = ?", [preset_id]), 1);
    let ev = h.col.evaluate(preset_id).unwrap();
    assert!(ev.log_loss > 0.0);
    assert_eq!(ev.reviews, r.reviews);
    // maintenance optimises one due preset per call (the others here; not this one again)
    let m = h.col.maintenance().unwrap();
    assert_eq!(m.optimised.len(), 1);
    assert!(m.backed_up);
    let mut done = vec![preset_id, m.optimised[0].config_id];
    for _ in 0..10 {
        let next = h.col.maintenance().unwrap();
        let Some(first) = next.optimised.first() else { break };
        assert!(!done.contains(&first.config_id));
        done.push(first.config_id);
    }
    assert!(h.col.maintenance().unwrap().optimised.is_empty());
    assert_eq!(h.count("SELECT count(DISTINCT config_id) FROM optimise_log"), done.len() as i64);
}

#[test]
fn undo_after_an_adopted_optimisation_restores_the_parameters_and_memory_states() {
    let mut h = imported_fixture();
    let snapshot = |h: &Harness| rows(h.db(), "SELECT id, data FROM cards ORDER BY id");
    let mut adopted = false;
    for p in h.col.list_presets().unwrap() {
        if p.config.scheduler != SchedulerKind::Fsrs {
            continue;
        }
        let before = snapshot(&h);
        let r = h.col.optimise(p.id, &mut no_progress()).unwrap();
        if p.config.fsrs_params.len() == 21 {
            assert_eq!(r.before.params, p.config.fsrs_params);
        }
        if !r.adopted {
            assert_ne!(h.col.undo_label().unwrap().as_deref(), Some("Optimise parameters"));
            continue;
        }
        adopted = true;
        assert_params_close(&h.col.get_preset(p.id).unwrap().config.fsrs_params, &r.after.params);
        assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Optimise parameters"));
        assert_eq!(h.col.undo().unwrap().map(|u| u.label).as_deref(), Some("Optimise parameters"));
        assert_eq!(h.col.get_preset(p.id).unwrap().config.fsrs_params, p.config.fsrs_params);
        assert_eq!(snapshot(&h), before);
        break;
    }
    assert!(adopted);
}

#[test]
fn new_learned_counts_distinct_cards_first_answered_today_not_answers() {
    let mut h = imported_fixture();
    let before = h.col.stats(&StatsRequest { deck_id: None }).unwrap().today.learned;
    let deck: i64 = h.scalar("SELECT did FROM cards WHERE type = 0 AND queue = 0 LIMIT 1");
    let mut card = h.col.start_custom_study(Some(deck), CustomStudyKind::Search { query: "is:new".into(), limit: 1 }).unwrap();
    let id = card.as_ref().expect("a new card").card.id;
    // three answers on the same new card (Again, Again, Good) during learning
    for rating in [Rating::Again, Rating::Again, Rating::Good] {
        let Some(c) = card.take() else { break };
        let res = h.col.answer_card(c.card.id, rating, 1000).unwrap();
        card = match res.next {
            Some(n) => Some(n),
            None => h.col.next_card().unwrap(),
        };
        if card.as_ref().is_none_or(|c| c.card.id != id) {
            break;
        }
    }
    let answers = h.count_p("SELECT count(*) FROM revlog WHERE cid = ?", [id]);
    assert!(answers >= 2);
    let s = h.col.stats(&StatsRequest { deck_id: None }).unwrap();
    assert_eq!(s.today.learned, before + 1);
    let today_start = (h.col.core().next_day_at().unwrap() - 86400) * 1000;
    assert_eq!(
        s.today.learned,
        h.count_p(
            "SELECT count(DISTINCT r.cid) FROM revlog r WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4
               AND NOT EXISTS (SELECT 1 FROM revlog p WHERE p.cid = r.cid AND p.id < ? AND p.ease BETWEEN 1 AND 4)",
            [today_start, today_start]
        )
    );
}

#[test]
fn forecasts_the_workload_of_a_preset_change() {
    let h = imported_fixture();
    let p = h.col.get_preset(1).unwrap();
    let mut low_cfg = p.config.clone();
    low_cfg.new_per_day = 0;
    let mut high_cfg = p.config.clone();
    high_cfg.new_per_day = 50;
    let low = h.col.forecast_workload(None, &low_cfg).unwrap();
    let high = h.col.forecast_workload(None, &high_cfg).unwrap();
    assert_eq!(low.reviews_per_day.len(), 90);
    assert_eq!(high.minutes_per_day.len(), 90);
    assert!(high.reviews_per_day.iter().sum::<f64>() > low.reviews_per_day.iter().sum::<f64>());
    assert!(high.sentence.contains("reviews a day"), "{}", high.sentence);
}

// ── export round trip ──

struct Snapshot {
    notes: Vec<Vec<String>>,
    cards: Vec<Vec<String>>,
    revlog: Vec<Vec<String>>,
    media: Vec<Vec<String>>,
    decks: Vec<Vec<String>>,
}

fn snapshot(col: &Collection) -> Snapshot {
    let db = col.core().db();
    Snapshot {
        notes: rows(db, "SELECT id, guid, mtime, tags, flds, sfld, csum FROM notes ORDER BY id"),
        cards: rows(
            db,
            "SELECT id, nid, ord, type, queue, due, ivl, factor, reps, lapses, left, flags, data FROM cards ORDER BY id",
        ),
        revlog: rows(db, "SELECT id, cid, ease, ivl, last_ivl, factor, time, type FROM revlog ORDER BY id"),
        media: rows(db, "SELECT name, sha1, size FROM media ORDER BY name"),
        decks: rows(db, "SELECT name, description FROM decks ORDER BY name"),
    }
}

#[test]
fn export_then_re_import_into_a_second_collection_is_lossless() {
    let h = imported_fixture();
    let bytes = h.col.export_package(&all_decks(), &mut no_progress()).unwrap();
    let mut h2 = make_collection_at(h.clock.ms());
    let r = h2.col.import_package(bytes, &merge_options(), &mut no_progress()).unwrap();
    assert_eq!(r.notes_added, expected().totals.notes);
    let a = snapshot(&h.col);
    let b = snapshot(&h2.col);
    assert_eq!(b.notes, a.notes);
    assert_eq!(b.cards, a.cards);
    assert_eq!(b.revlog, a.revlog);
    assert_eq!(b.media, a.media);
    assert_eq!(b.decks, a.decks);
    let names: Vec<String> = h.db().all("SELECT name FROM media", [], |r| r.get(0)).unwrap();
    for m in names {
        assert_eq!(h2.col.get_media(&m).unwrap(), h.col.get_media(&m).unwrap(), "{m}");
    }
}

#[test]
fn keeps_the_source_anki_collections_crt_with_due_days_on_its_numbering() {
    let h = imported_fixture();
    let lang = h.deck_id("Languages");
    for deck_id in [None, Some(lang)] {
        let bytes = h
            .col
            .export_package(&ExportOptions { deck_id, include_scheduling: true, include_media: false }, &mut no_progress())
            .unwrap();
        let img = read_package(bytes).unwrap();
        assert_eq!(img.crt, expected().crt);
        let mut checked = 0;
        for s in &expected().cards {
            let Some(c) = img.cards.iter().find(|c| c.id == s.id) else { continue };
            assert_eq!(c.due, s.due, "{}", s.label); // as the fixture had it
            checked += 1;
        }
        assert!(checked > 0);
    }
    // a collection that did not start from an import exports its own crt
    let h2 = make_collection_at(h.clock.ms());
    h2.col.install_sample_deck().unwrap();
    let own = read_package(
        h2.col
            .export_package(&ExportOptions { deck_id: None, include_scheduling: true, include_media: false }, &mut no_progress())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(own.crt, h2.col.core().crt().unwrap());
}

#[test]
fn exports_one_deck_with_children_and_without_scheduling() {
    let h = imported_fixture();
    let lang = h.deck_id("Languages");
    let bytes = h
        .col
        .export_package(
            &ExportOptions { deck_id: Some(lang), include_scheduling: false, include_media: true },
            &mut no_progress(),
        )
        .unwrap();
    let p = h.col.preview_import(bytes).unwrap();
    let ed = &expected().decks;
    assert_eq!(p.cards, ed["Languages::Arabic"].total + ed["Languages::Arabic::Verbs"].total);
    assert_eq!(p.reviews, 0);
    assert!(!p.has_scheduling);
    for d in ["Languages", "Languages::Arabic", "Languages::Arabic::Verbs"] {
        assert!(p.decks.contains(&d.to_string()), "{d}");
    }
}

#[test]
fn installs_the_sample_deck_once_undoably() {
    let mut h = make_collection();
    let id = h.col.install_sample_deck().unwrap();
    let tree = h.col.deck_tree().unwrap();
    let node = tree.children.iter().find(|c| c.deck.id == id).unwrap();
    assert_eq!(node.total_cards, 30);
    assert_eq!(node.new_count, 20);
    h.col.install_sample_deck().unwrap();
    assert_eq!(h.count("SELECT count(*) FROM cards"), 30);
    h.col.undo().unwrap();
    assert_eq!(h.count("SELECT count(*) FROM cards"), 0);
}

// ── backup → corrupt → open → restore ──

#[test]
fn detects_the_damage_and_restores_equal_counts() {
    let mut h = imported_fixture();
    let counts = |h: &Harness| {
        (h.count("SELECT count(*) FROM notes"), h.count("SELECT count(*) FROM cards"), h.count("SELECT count(*) FROM revlog"))
    };
    let before = counts(&h);
    let info = h.col.backup_now().unwrap();
    h.col.close().unwrap();
    // Corrupt the live database file: overwrite pages in the middle of the file.
    let path = h.db_path();
    let mut bytes = std::fs::read(&path).unwrap();
    let end = bytes.len().min(4096 * 40);
    for (i, b) in bytes.iter_mut().enumerate().take(end).skip(4096) {
        *b = ((i * 7) & 0xff) as u8;
    }
    std::fs::write(&path, &bytes).unwrap();
    let mut col = h.reopen();
    let failed = match col.open() {
        Ok(report) => !report.ok,
        Err(e) => e.code == ErrorCode::Corrupt,
    };
    assert!(failed);
    let restored = col.restore_backup(&info.name).unwrap();
    assert!(restored.ok);
    h.col = col;
    assert_eq!(counts(&h), before);
    assert!(h.col.check_integrity().unwrap().ok);
}

// ── full backup download → restore from the file ──

#[test]
fn full_backup_replaces_the_collection_and_media_refuses_to_merge() {
    let mut h = imported_fixture();
    let zip = h.col.export_full_backup(&mut no_progress()).unwrap();
    let before = (
        h.count("SELECT count(*) FROM notes"),
        h.count("SELECT count(*) FROM cards"),
        h.count("SELECT count(*) FROM revlog"),
        h.count("SELECT count(*) FROM media"),
    );
    let p = h.col.preview_import(zip.clone()).unwrap();
    assert!(p.is_collection_package);
    assert_eq!((p.notes, p.cards, p.reviews, p.media), before);
    let err = h.col.import_package(zip.clone(), &merge_options(), &mut no_progress()).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
    h.col.reset_collection().unwrap();
    assert!(h.col.is_empty().unwrap());
    let r =
        h.col.import_package(zip, &ImportOptions { mode: ImportMode::Replace, ..merge_options() }, &mut no_progress()).unwrap();
    assert_eq!((r.notes_added, r.cards_added, r.reviews_added), (before.0, before.1, before.2));
    assert_eq!(h.count("SELECT count(*) FROM notes"), before.0);
    assert_eq!(h.count("SELECT count(*) FROM media"), before.3);
    assert!(h.col.check_media().unwrap().missing.is_empty());
}
