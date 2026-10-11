//! Port of recall `tests/unit/integration/import.test.ts`.

use super::helpers::*;
use loci_core::collection::builtin_notetypes::notetype_to_input;
use loci_core::error::ErrorCode;
use loci_core::types::*;

#[test]
fn imports_every_note_card_review_and_media_file() {
    let mut h = make_collection();
    let report = import_fixture(&mut h, "palace-legacy2.apkg");
    let e = &expected().totals;
    assert_eq!(report.notes_added, e.notes);
    assert_eq!(report.cards_added, e.cards);
    assert_eq!(report.reviews_added, e.revlog);
    assert_eq!(report.media_added, e.media);
    assert_eq!(h.count("SELECT count(*) FROM notes"), e.notes);
    assert_eq!(h.count("SELECT count(*) FROM cards"), e.cards);
    assert_eq!(h.count("SELECT count(*) FROM revlog"), e.revlog);
    assert_eq!(h.count("SELECT count(*) FROM media"), e.media);
}

#[test]
fn deck_tree_counts_equal_the_expected_due_counts_for_today() {
    let h = imported_fixture();
    assert_eq!(h.col.today().unwrap(), 0);
    let tree = h.col.deck_tree().unwrap();
    let nodes: std::collections::HashMap<String, &DeckTreeNode> =
        flatten(&tree).into_iter().filter(|n| n.deck.id != 0).map(|n| (n.deck.name.clone(), n)).collect();
    for (name, e) in &expected().decks {
        if e.total == 0 {
            continue; // empty parents/Default
        }
        let n = nodes.get(name).unwrap_or_else(|| panic!("deck {name} missing"));
        // leaf decks: own counts (children are tested on their own)
        if !n.children.is_empty() {
            continue;
        }
        assert_eq!(n.review_count, e.review_due, "{name} review");
        assert_eq!(n.learn_count, e.learn_due_now + e.interday_due, "{name} learn");
        // The generator adds the deck's `extendNew` to the limit; in Anki that key is only the custom-study
        // dialog's remembered value (extending limits decrements newToday instead), so it is not a limit.
        let config: String = h.scalar_p("SELECT config FROM decks WHERE id = ?", [n.deck.id]);
        let extra: serde_json::Value = serde_json::from_str(&config).unwrap();
        let extend_new = extra.get("extendNew").and_then(|v| v.as_i64()).unwrap_or(0);
        assert_eq!(n.new_count, (e.new_limited - extend_new).max(0), "{name} new");
        assert_eq!(n.total_cards, e.total, "{name} total");
    }
    // A parent deck includes its children.
    let arabic = nodes["Languages::Arabic"];
    let verbs = nodes["Languages::Arabic::Verbs"];
    let ed = &expected().decks;
    assert_eq!(arabic.total_cards, ed["Languages::Arabic"].total + ed["Languages::Arabic::Verbs"].total);
    assert_eq!(arabic.review_count, ed["Languages::Arabic"].review_due + verbs.review_count);
    assert!(tree.estimated_minutes > 0);
}

#[test]
fn keeps_due_days_relative_to_today_fsrs_memory_state_and_sample_card_states() {
    let h = imported_fixture();
    let shift = expected().today; // fixture day T is our day 0
    for s in &expected().cards {
        let c = h.col.get_card(s.id).unwrap();
        assert_eq!((c.ctype as i64, c.queue as i64), (s.ctype, s.queue), "{}", s.label);
        if s.queue == 2 || s.queue == 3 || s.ctype == 2 {
            assert_eq!(c.due, s.due - shift, "{}", s.label);
        } else {
            assert_eq!(c.due, s.due, "{}", s.label);
        }
        assert_eq!(c.ivl, s.ivl, "{}", s.label);
    }
    assert_eq!(
        h.count(
            "SELECT count(*) FROM cards WHERE json_extract(data, '$.s') IS NOT NULL AND json_extract(data, '$.d') IS NOT NULL"
        ),
        expected().totals.fsrs_cards
    );
    assert_eq!(h.count("SELECT count(*) FROM notes WHERE tags LIKE '% leech %'"), expected().totals.leech_notes);
}

#[test]
fn palace_latest_legacy1_and_colpkg_give_identical_collections() {
    struct Snapshot {
        cards: Vec<Vec<String>>,
        notes: Vec<Vec<String>>,
        revlog: Vec<Vec<String>>,
        media: Vec<Vec<String>>,
        decks: Vec<Vec<String>>,
        tree: Vec<(String, i64, i64, i64, i64)>,
    }
    let snapshot = |name: &str| -> Snapshot {
        let x = imported_fixture_named(name);
        let tree = x.col.deck_tree().unwrap();
        Snapshot {
            cards: rows(
                x.db(),
                "SELECT id, nid, ord, type, queue, due, ivl, factor, reps, lapses, left, flags, data FROM cards ORDER BY id",
            ),
            notes: rows(x.db(), "SELECT id, guid, flds, tags FROM notes ORDER BY id"),
            revlog: rows(x.db(), "SELECT * FROM revlog ORDER BY id"),
            media: rows(x.db(), "SELECT name, sha1, size FROM media ORDER BY name"),
            decks: rows(x.db(), "SELECT name FROM decks ORDER BY name"),
            tree: flatten(&tree)
                .iter()
                .map(|n| (n.deck.name.clone(), n.new_count, n.learn_count, n.review_count, n.total_cards))
                .collect(),
        }
    };
    let base = snapshot("palace-legacy2.apkg");
    for other in ["palace-latest.apkg", "palace.colpkg", "palace-legacy1.apkg"] {
        let s = snapshot(other);
        assert_eq!(s.notes, base.notes, "{other} notes");
        assert_eq!(s.cards, base.cards, "{other} cards");
        assert_eq!(s.revlog, base.revlog, "{other} revlog");
        assert_eq!(s.media, base.media, "{other} media");
        assert_eq!(s.decks, base.decks, "{other} decks");
        assert_eq!(s.tree, base.tree, "{other} tree");
    }
}

#[test]
fn previews_a_package_without_writing() {
    let mut h = make_collection();
    let p = h.col.preview_import(fixture("palace-latest.apkg")).unwrap();
    let e = &expected().totals;
    assert_eq!(p.layout, PackageLayout::Latest);
    assert_eq!(p.notes, e.notes);
    assert_eq!(p.cards, e.cards);
    assert_eq!(p.reviews, e.revlog);
    assert_eq!(p.fsrs_cards, e.fsrs_cards);
    assert!(p.has_scheduling);
    assert_eq!((p.notes_existing, p.notes_new), (0, e.notes));
    assert!(h.col.is_empty().unwrap());
    // after an import every note is already here
    import_fixture(&mut h, "palace-latest.apkg");
    let again = h.col.preview_import(fixture("palace-legacy2.apkg")).unwrap();
    assert_eq!((again.notes_existing, again.notes_new), (e.notes, 0));
}

#[test]
fn re_importing_is_idempotent_and_conflict_rules_apply() {
    let mut h = imported_fixture();
    let again = import_fixture(&mut h, "palace-legacy2.apkg");
    assert_eq!(again.notes_added, 0);
    assert_eq!(again.cards_added, 0);
    assert_eq!(again.notes_skipped, expected().totals.notes);
    assert_eq!(h.count("SELECT count(*) FROM cards"), expected().totals.cards);
    // Edit one note locally, then 'theirs' restores the package version.
    let nid: i64 = h.scalar("SELECT id FROM notes ORDER BY id LIMIT 1");
    let note = h.col.get_note(nid).unwrap();
    let mut fields = note.fields.clone();
    fields[0] = "changed here".into();
    h.col.update_note(nid, &fields, &note.tags).unwrap();
    let keep = h
        .col
        .import_package(
            fixture("palace-legacy2.apkg"),
            &ImportOptions { conflict: ConflictPolicy::Keep, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(keep.notes_updated, 0);
    assert_eq!(h.col.get_note(nid).unwrap().fields[0], "changed here");
    let theirs = h
        .col
        .import_package(
            fixture("palace-legacy2.apkg"),
            &ImportOptions { conflict: ConflictPolicy::Theirs, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(theirs.notes_updated, 1);
    assert_eq!(h.col.get_note(nid).unwrap().fields[0], note.fields[0]);
}

#[test]
fn adopts_ankis_stock_note_types_in_place_of_locis_empty_built_ins() {
    let mut h = make_collection();
    let builtins = h.col.list_notetypes().unwrap();
    assert!(builtins.iter().any(|n| n.name == "Basic"));
    import_fixture(&mut h, "palace-legacy2.apkg");
    let after = h.col.list_notetypes().unwrap();
    let names: Vec<String> = after.iter().map(|n| n.name.clone()).collect();
    assert!(names.iter().all(|n| !n.contains('+')), "{names:?}");
    let lower: std::collections::HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
    assert_eq!(lower.len(), names.len());
    for name in ["Basic", "Basic (and reversed card)", "Basic (type in the answer)", "Cloze", "Image Occlusion"] {
        let nt = after.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("{name} missing"));
        // the Anki note type (its id) now carries the name and the notes; the empty built-in is gone
        assert!(!builtins.iter().any(|b| b.id == nt.id), "{name}");
        assert!(h.count_p("SELECT count(*) FROM notes WHERE ntid = ?", [nt.id]) > 0, "{name}");
    }
    // Loci markers survive on identical shapes (the editor relies on them)
    assert_eq!(after.iter().find(|n| n.name == "Cloze").unwrap().config.builtin, Some(BuiltinKind::Cloze));
    assert_eq!(
        after.iter().find(|n| n.name == "Basic (type in the answer)").unwrap().config.builtin,
        Some(BuiltinKind::BasicTyped)
    );
    for b in &builtins {
        assert!(!after.iter().any(|n| n.id == b.id), "{}", b.name);
    }

    // the sample deck still finds its note types after the swap
    let sample = h.col.install_sample_deck().unwrap();
    assert_eq!(h.count_p("SELECT count(*) FROM cards WHERE did = ?", [sample]), 30);
    assert_eq!(h.col.list_notetypes().unwrap().len(), after.len());

    // re-import: nothing new
    let again = import_fixture(&mut h, "palace-legacy2.apkg");
    assert_eq!(again.notetypes_added, 0);
    let mut now: Vec<String> = h.col.list_notetypes().unwrap().into_iter().map(|n| n.name).collect();
    now.sort();
    let mut want = names.clone();
    want.sort();
    assert_eq!(now, want);
}

#[test]
fn keeps_a_used_local_note_type_and_suffixes_a_different_incoming_one_of_the_same_name() {
    let mut h = make_collection();
    let basic = h.notetype_by_name("Basic");
    // make the local "Basic" used and different (three fields)
    let mut input = notetype_to_input(&basic);
    let mut extra = basic.fields[1].clone();
    extra.ord = 2;
    extra.name = "Extra".into();
    input.fields.push(extra);
    h.col.save_notetype(&input).unwrap();
    h.col.add_note(basic.id, 1, &strs(&["q", "a", "x"]), &[]).unwrap();
    import_fixture(&mut h, "palace-legacy2.apkg");
    let names: Vec<String> = h.col.list_notetypes().unwrap().into_iter().map(|n| n.name).collect();
    assert!(names.contains(&"Basic".to_string()));
    assert!(names.contains(&"Basic+".to_string()), "{names:?}");
    assert_eq!(h.col.get_notetype(basic.id).unwrap().fields.len(), 3);
}

#[test]
fn into_an_empty_collection_ankis_default_preset_takes_over_preset_1_undo_restores_standard() {
    let mut h = make_collection();
    assert_eq!(h.col.get_preset(1).unwrap().name, "Standard");
    import_fixture(&mut h, "palace-legacy2.apkg");
    let presets = h.col.list_presets().unwrap();
    let one = presets.iter().find(|p| p.id == 1).unwrap();
    assert_eq!(one.name, "Default");
    assert_eq!(presets.iter().filter(|p| p.name == "Default").count(), 1);
    // the decks that used Anki's Default now use preset 1
    let used = h.count("SELECT count(*) FROM decks WHERE config_id = 1 AND id IN (SELECT DISTINCT did FROM cards)");
    assert!(used > 0);
    h.col.undo().unwrap();
    assert_eq!(h.col.get_preset(1).unwrap().name, "Standard");

    // a collection with notes keeps its Standard preset
    let mut h2 = make_collection();
    h2.col.install_sample_deck().unwrap();
    import_fixture(&mut h2, "palace-legacy2.apkg");
    assert_eq!(h2.col.get_preset(1).unwrap().name, "Standard");
    assert!(h2.col.list_presets().unwrap().iter().any(|p| p.name == "Default"));
}

#[test]
fn imports_without_scheduling_as_new_cards() {
    let mut h = make_collection();
    let r = h
        .col
        .import_package(
            fixture("palace-legacy2.apkg"),
            &ImportOptions { keep_scheduling: false, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(r.reviews_added, 0);
    assert_eq!(h.count("SELECT count(*) FROM cards WHERE type != 0"), 0);
    assert_eq!(h.count("SELECT count(*) FROM notes WHERE tags LIKE '% leech %'"), 0);
}

#[test]
fn undoes_an_import() {
    let mut h = make_collection();
    h.col
        .import_package(
            fixture("palace-legacy2.apkg"),
            &ImportOptions { keep_scheduling: false, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Import"));
    h.col.undo().unwrap();
    assert_eq!(h.count("SELECT count(*) FROM notes"), 0);
    assert_eq!(h.count("SELECT count(*) FROM cards"), 0);
}

#[test]
fn replace_mode_wipes_and_imports_keeping_a_pre_restore_backup() {
    let mut h = make_collection();
    h.col.install_sample_deck().unwrap();
    let r = h
        .col
        .import_package(
            fixture("palace.colpkg"),
            &ImportOptions { mode: ImportMode::Replace, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap();
    assert_eq!(r.notes_added, expected().totals.notes);
    assert_eq!(h.count("SELECT count(*) FROM notes"), expected().totals.notes);
    assert!(h.col.list_backups().unwrap().iter().any(|b| b.kind == BackupKind::PreRestore));
}

#[test]
fn rejects_a_broken_package_cleanly() {
    let mut h = make_collection();
    let err = h.col.import_package(fixture("broken-truncated.apkg"), &merge_options(), &mut no_progress()).unwrap_err();
    assert_eq!(err.code, ErrorCode::Corrupt);
    let err = h
        .col
        .import_package(
            fixture("broken-not-a-zip.apkg"),
            &ImportOptions { mode: ImportMode::Replace, ..merge_options() },
            &mut no_progress(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedPackage);
    assert!(h.col.is_empty().unwrap());
}
