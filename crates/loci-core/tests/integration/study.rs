//! Port of recall `tests/unit/integration/study.test.ts`.

use super::helpers::*;
use loci_core::error::ErrorCode;
use loci_core::types::*;
use regex::Regex;

const DAY_MS: i64 = 86_400_000;

fn label_re() -> Regex {
    Regex::new(r"^(<1m|\d+m|\d+(\.\d)?h|\d+d|\d+(\.\d)?(mo|y))$").unwrap()
}

#[test]
fn starts_with_a_rendered_card_sane_preview_labels_and_counts() {
    let mut h = imported_fixture();
    let tree = h.col.deck_tree().unwrap();
    let sc = h.col.start_study(None).unwrap().expect("a card");
    let re = label_re();
    for r in Rating::ALL {
        assert!(re.is_match(sc.preview.labels.get(r)), "{}", sc.preview.labels.get(r));
        assert!(*sc.preview.seconds_until.get(r) > 0);
    }
    assert!(sc.preview.seconds_until.again <= sc.preview.seconds_until.good);
    assert!(sc.preview.seconds_until.good <= sc.preview.seconds_until.easy);
    assert_eq!(sc.rendered.card_id, sc.card.id);
    assert!(!sc.rendered.front.html.is_empty());
    assert!(!sc.deck_name.is_empty());
    let total = sc.counts.new + sc.counts.learn + sc.counts.review;
    assert!(total > 0);
    // All-decks counts are capped by the top-level limits like the deck tree root's children.
    assert!(sc.counts.review <= tree.review_count);
}

#[test]
fn answering_good_moves_the_card_writes_a_revlog_row_updates_counters_undo_reverts_all() {
    let mut h = imported_fixture();
    let sc = h.col.start_study(None).unwrap().expect("a card");
    let before = h.col.get_card(sc.card.id).unwrap();
    let revlog_before = h.count("SELECT count(*) FROM revlog");
    h.clock.advance(5000);
    let res = h.col.answer_card(sc.card.id, Rating::Good, 4200).unwrap();
    assert_eq!(res.undo_label, "Answer Good");
    let after = h.col.get_card(sc.card.id).unwrap();
    assert_eq!(after.reps, before.reps + 1);
    assert!(!(after.due == before.due && after.queue == before.queue));
    assert_eq!(h.count("SELECT count(*) FROM revlog"), revlog_before + 1);
    let rl: (i64, i64, i64) = h
        .db()
        .get("SELECT ease, time, cid FROM revlog ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .unwrap();
    assert_eq!(rl, (3, 4200, sc.card.id));
    let deck = h.col.get_deck(sc.card.did).unwrap();
    assert_eq!(deck.today.day, h.col.today().unwrap());
    assert_eq!(deck.today.time_ms, 4200);
    let next = res.next.expect("a next card");
    assert_ne!(next.card.id, sc.card.id);

    assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Answer Good"));
    h.col.undo().unwrap();
    assert_eq!(h.col.get_card(sc.card.id).unwrap(), before);
    assert_eq!(h.count("SELECT count(*) FROM revlog"), revlog_before);
    assert_eq!(h.col.get_deck(sc.card.did).unwrap().today.time_ms, 0);
    // The undone card comes back first.
    let again = h.col.next_card().unwrap().expect("a card");
    assert_eq!(again.card.id, sc.card.id);
}

#[test]
fn a_review_card_answered_good_gets_an_fsrs_interval_and_memory_state() {
    let mut h = imported_fixture();
    let review: i64 = h
        .db()
        .scalar("SELECT id FROM cards WHERE queue = 2 AND due <= 0 AND json_extract(data, '$.s') IS NOT NULL LIMIT 1", [])
        .unwrap()
        .or_else(|| h.db().scalar("SELECT id FROM cards WHERE queue = 2 AND due <= 0 LIMIT 1", []).unwrap())
        .expect("a due review");
    h.col.start_study(None).unwrap();
    let res = h.col.answer_card(review, Rating::Good, 3000).unwrap();
    let c = h.col.get_card(review).unwrap();
    assert_eq!(c.ctype, CardType::Review);
    assert_eq!(c.queue, CardQueue::Review);
    assert!(c.due > 0);
    assert!(c.data.s.unwrap() > 0.0);
    assert!(c.data.d.unwrap() >= 1.0);
    assert_eq!(c.data.lrt, Some(h.clock.ms().div_euclid(1000)));
    let rl: (i64, i64) = h
        .db()
        .get("SELECT type, ivl FROM revlog WHERE cid = ? ORDER BY id DESC LIMIT 1", [review], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .unwrap();
    assert_eq!(rl.0, RevlogType::Review as i64);
    assert_eq!(rl.1, c.ivl);
    assert_eq!(res.finished, res.next.is_none());
}

#[test]
fn studies_through_a_deck_and_summarises_the_session() {
    let mut h = imported_fixture();
    let deck_id = h.deck_id("Science::Physics");
    let mut sc = h.col.start_study(Some(deck_id)).unwrap();
    let mut answered = 0;
    while let Some(card) = sc.take() {
        if answered >= 200 {
            break;
        }
        h.clock.advance(6000);
        let rating = if answered % 5 == 0 { Rating::Again } else { Rating::Good };
        let r = h.col.answer_card(card.card.id, rating, 6000).unwrap();
        answered += 1;
        sc = r.next;
        if sc.is_none() {
            // learning cards come due later: jump ahead and continue
            h.clock.advance(11 * 60_000);
            sc = h.col.next_card().unwrap();
        }
    }
    assert!(answered > 0);
    let summary = h.col.end_study().unwrap();
    assert_eq!(summary.cards, answered);
    assert_eq!(summary.again + summary.hard + summary.good + summary.easy, answered);
    assert!(summary.accuracy > 0.5);
    assert_eq!(summary.seconds, answered * 6);
    assert!(summary.tomorrow_due >= 0);
    let tree = h.col.deck_tree().unwrap();
    let phys = find_node(&tree, deck_id).expect("Physics node");
    assert_eq!(phys.new_count + phys.review_count + phys.learn_count, 0);
}

#[test]
fn custom_study_review_ahead_preview_and_search() {
    let mut h = imported_fixture();
    let ahead = h.col.start_custom_study(None, CustomStudyKind::Ahead { days: 3 }).unwrap().expect("ahead card");
    assert!(ahead.card.due > 0);
    assert!(ahead.card.due <= 3);
    h.col.end_study().unwrap();
    let preview = h.col.start_custom_study(None, CustomStudyKind::Preview).unwrap().expect("preview card");
    assert_eq!(preview.card.ctype, CardType::New);
    let revlog = h.count("SELECT count(*) FROM revlog");
    let r = h.col.answer_card(preview.card.id, Rating::Good, 1000).unwrap();
    assert_eq!(h.count("SELECT count(*) FROM revlog"), revlog);
    assert_eq!(h.col.get_card(preview.card.id).unwrap().ctype, CardType::New);
    assert_ne!(r.next.map(|n| n.card.id), Some(preview.card.id));
    h.col.end_study().unwrap();
    let search = h
        .col
        .start_custom_study(None, CustomStudyKind::Search { query: "tag:leech".into(), limit: 5 })
        .unwrap()
        .expect("search card");
    assert!(search.is_leech);
    assert!(search.counts.new + search.counts.learn + search.counts.review <= 5);
}

#[test]
fn a_lapse_past_the_leech_threshold_tags_the_note_resolve_leech_keep_reset() {
    let mut h = make_collection();
    let basic = h.builtin(BuiltinKind::Basic);
    let deck = h.col.create_deck("Leeches", None).unwrap();
    let preset = h.col.get_preset(1).unwrap();
    let mut config = preset.config.clone();
    config.leech_threshold = 2;
    config.learn_steps = vec![];
    config.relearn_steps = vec![];
    h.col.save_preset(&DeckConfigPresetInput { id: Some(preset.id), name: preset.name.clone(), config }).unwrap();
    let added = h.col.add_note(basic.id, deck.id, &strs(&["q", "a"]), &[]).unwrap();
    let id = added.card_ids[0];
    h.col.start_study(Some(deck.id)).unwrap();
    h.col.answer_card(id, Rating::Good, 1000).unwrap();
    let mut became_leech = false;
    for _ in 0..3 {
        if became_leech {
            break;
        }
        h.clock.advance(40 * DAY_MS);
        h.col.start_study(Some(deck.id)).unwrap();
        let r = h.col.answer_card(id, Rating::Again, 1000).unwrap();
        became_leech = r.became_leech == Some(true);
    }
    assert!(became_leech);
    let card = h.col.get_card(id).unwrap();
    assert!(h.col.get_note(card.nid).unwrap().tags.contains(&"leech".to_string()));
    h.col.resolve_leech(id, LeechResolution::Keep).unwrap();
    assert!(!h.col.get_note(card.nid).unwrap().tags.contains(&"leech".to_string()));
    h.col.undo().unwrap();
    assert!(h.col.get_note(card.nid).unwrap().tags.contains(&"leech".to_string()));
    h.col.resolve_leech(id, LeechResolution::Reset).unwrap();
    let reset = h.col.get_card(id).unwrap();
    assert_eq!((reset.ctype, reset.queue, reset.reps, reset.lapses, reset.ivl), (CardType::New, CardQueue::New, 0, 0, 0));
    assert_eq!(reset.data.s, None);
    let last: (i64, i64, i64) = h
        .db()
        .get("SELECT type, ease, factor FROM revlog WHERE cid = ? ORDER BY id DESC LIMIT 1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .unwrap();
    assert_eq!(last, (RevlogType::Manual as i64, 0, 0));
}

#[test]
fn buries_siblings_when_the_preset_says_so_undoably() {
    let mut h = make_collection();
    let reversed = h.builtin(BuiltinKind::BasicReversed);
    let deck = h.col.create_deck("Pairs", None).unwrap();
    let p = h.col.get_preset(1).unwrap();
    let mut config = p.config.clone();
    config.bury_siblings_new = true;
    h.col.save_preset(&DeckConfigPresetInput { id: Some(p.id), name: p.name.clone(), config }).unwrap();
    let added = h.col.add_note(reversed.id, deck.id, &strs(&["front", "back"]), &[]).unwrap();
    let sc = h.col.start_study(Some(deck.id)).unwrap().expect("a card");
    assert_eq!(sc.counts.new, 1); // the sibling is left out at gather time
    let sibling = *added.card_ids.iter().find(|x| **x != sc.card.id).unwrap();
    assert_eq!(h.col.get_card(sibling).unwrap().queue, CardQueue::New);
    h.col.answer_card(sc.card.id, Rating::Good, 1000).unwrap();
    assert_eq!(h.col.get_card(sibling).unwrap().queue, CardQueue::SchedBuried); // written at answer time
    h.col.undo().unwrap();
    assert_eq!(h.col.get_card(sibling).unwrap().queue, CardQueue::New);
}

#[test]
fn explains_a_card_in_one_sentence() {
    let h = imported_fixture();
    let id: i64 = h.scalar("SELECT id FROM cards WHERE type = 2 AND json_extract(data, '$.s') IS NOT NULL LIMIT 1");
    let e = h.col.explain_card(id).unwrap();
    let r = e.retrievability.expect("retrievability");
    assert!(r > 0.0 && r <= 1.0);
    assert!(e.stability.unwrap() > 0.0);
    assert!(!e.history.is_empty());
    let re = Regex::new(r"^You last saw this (today|yesterday|\d+ days ago); about \d+% chance you still know it\.$").unwrap();
    assert!(re.is_match(&e.sentence), "{}", e.sentence);
    let new_id: i64 = h.scalar("SELECT id FROM cards WHERE type = 0 LIMIT 1");
    assert_eq!(h.col.explain_card(new_id).unwrap().retrievability, None);
}

// ── sibling burying (default presets) ──

#[test]
fn a_note_with_two_cards_serves_one_today_the_other_returns_at_rollover_or_on_unbury() {
    let mut h = make_collection();
    let preset = h.col.get_preset(1).unwrap();
    assert!(preset.config.bury_siblings_new);
    assert!(preset.config.bury_siblings_review);
    for id in [2, 3] {
        assert!(h.col.get_preset(id).unwrap().config.bury_siblings_new);
    }
    let nt = h.notetype_by_name("Basic (and reversed card)");
    let added = h.col.add_note(nt.id, 1, &strs(&["front", "back"]), &[]).unwrap();
    assert_eq!(added.card_ids.len(), 2);
    let first = h.col.start_study(None).unwrap().expect("a card");
    assert_eq!(first.counts.new, 1); // gathered once per note
    let mut served: Vec<i64> = Vec::new();
    let mut sc = Some(first.clone());
    for _ in 0..10 {
        let Some(card) = sc.take() else { break };
        served.push(card.card.id);
        let res = h.col.answer_card(card.card.id, Rating::Easy, 1000).unwrap();
        sc = res.next;
    }
    assert_eq!(served, vec![first.card.id]);
    let sibling = *added.card_ids.iter().find(|id| **id != first.card.id).unwrap();
    assert_eq!(h.col.get_card(sibling).unwrap().queue, CardQueue::SchedBuried);
    assert!(h.col.start_study(None).unwrap().is_none());

    // next day: unburied by the rollover
    h.clock.advance(DAY_MS);
    assert_eq!(h.col.deck_tree().unwrap().new_count, 1);
    assert_eq!(h.col.get_card(sibling).unwrap().queue, CardQueue::New);
    let next = h.col.start_study(None).unwrap().expect("a card");
    assert_eq!(next.card.id, sibling);

    // and on demand: bury again by answering, then unbury the deck
    let more = h.col.add_note(nt.id, 1, &strs(&["f2", "b2"]), &[]).unwrap();
    h.col.end_study().unwrap();
    let s2 = h
        .col
        .start_custom_study(None, CustomStudyKind::Search { query: format!("cid:{}", more.card_ids[0]), limit: 1 })
        .unwrap()
        .expect("a card");
    h.col.answer_card(s2.card.id, Rating::Easy, 1000).unwrap();
    assert_eq!(h.col.get_card(more.card_ids[1]).unwrap().queue, CardQueue::SchedBuried);
    assert!(h.col.unbury_deck(None).unwrap() >= 1);
    assert_eq!(h.col.get_card(more.card_ids[1]).unwrap().queue, CardQueue::New);
}

// ── extendTodayLimits (increase today's limits) ──

#[test]
fn serves_more_new_cards_today_in_the_deck_and_its_subdecks_undoable_resets_with_the_day() {
    let mut h = make_collection();
    let basic = h.notetype_by_name("Basic");
    let parent = h.col.create_deck("Lang", None).unwrap();
    let child = h.col.create_deck("Lang::Words", None).unwrap();
    for i in 0..25 {
        h.col.add_note(basic.id, child.id, &strs(&[&format!("q{i}"), &format!("a{i}")]), &[]).unwrap();
    }
    let new_count = |h: &Harness, id: i64| find_node(&h.col.deck_tree().unwrap(), id).map(|n| n.new_count).unwrap();
    assert_eq!(new_count(&h, parent.id), 20);
    // learn the day's 20 (Easy graduates at once)
    let mut sc = h.col.start_study(Some(parent.id)).unwrap();
    for _ in 0..40 {
        let Some(card) = sc.take() else { break };
        sc = h.col.answer_card(card.card.id, Rating::Easy, 500).unwrap().next;
    }
    assert!(sc.is_none());
    assert_eq!(new_count(&h, parent.id), 0);

    let counts = h.col.extend_today_limits(parent.id, 3, 0).unwrap();
    // collection.ts:339 refills the running session inside extendTodayLimits; the RPC layer does
    // the same (`rpc.rs`), the bare Collection API exposes the hook as `refill_session`.
    h.col.refill_session().unwrap();
    assert_eq!(counts, StudyCounts { new: 3, learn: 0, review: 0 });
    assert_eq!(h.col.get_deck(parent.id).unwrap().today.new, 20 - 3);
    assert_eq!(h.col.get_deck(child.id).unwrap().today.new, 20 - 3);
    // the running session picks them up
    let more = h.col.next_card().unwrap().expect("more cards");
    assert_eq!(more.counts.new, 3);

    assert_eq!(h.col.undo_label().unwrap().as_deref(), Some("Extend today's limits"));
    h.col.undo().unwrap();
    assert_eq!(new_count(&h, parent.id), 0);

    h.col.extend_today_limits(parent.id, 100, 0).unwrap();
    assert_eq!(new_count(&h, parent.id), 5); // only 5 new cards remain
    assert_eq!(h.col.extend_today_limits(parent.id, -1, 0).unwrap_err().code, ErrorCode::Invalid);
    assert_eq!(h.col.extend_today_limits(424242, 1, 0).unwrap_err().code, ErrorCode::NotFound);

    // tomorrow the usual limit applies again
    h.clock.advance(DAY_MS);
    assert_eq!(new_count(&h, parent.id), 5);
    h.col.extend_today_limits(parent.id, 2, 1).unwrap();
    let t = h.col.get_deck(parent.id).unwrap().today;
    assert_eq!((t.day, t.new, t.rev), (h.col.today().unwrap(), -2, -1)); // a fresh day's counters
}

#[test]
fn sciences_preset_at_zero_new_a_day_extending_science_serves_remaining_new_cards() {
    let mut h = imported_fixture();
    let p = h.col.list_presets().unwrap().into_iter().find(|x| x.name.starts_with("Sciences")).expect("Sciences preset");
    let mut config = p.config.clone();
    config.new_per_day = 0;
    h.col.save_preset(&DeckConfigPresetInput { id: Some(p.id), name: p.name.clone(), config }).unwrap();
    let tree = h.col.deck_tree().unwrap();
    let sci = tree.children.iter().find(|n| n.deck.name == "Science").expect("Science");
    assert_eq!(sci.new_count, 0);
    assert!(h.col.start_study(Some(sci.deck.id)).unwrap().is_none());
    h.col.end_study().unwrap();

    let remaining = expected().decks["Science::Physics"].new;
    let counts = h.col.extend_today_limits(sci.deck.id, 10, 0).unwrap();
    assert_eq!(counts.new, remaining.min(10));
    let t = h.col.get_deck(sci.deck.id).unwrap().today;
    assert_eq!((t.day, t.new), (h.col.today().unwrap(), -10));
    let sc = h.col.start_study(Some(sci.deck.id)).unwrap().expect("a card");
    assert_eq!(sc.counts.new, remaining.min(10));
    // a second extension adds to the first, capped by the cards there are
    assert_eq!(h.col.extend_today_limits(sci.deck.id, 10, 0).unwrap().new, remaining.min(20));
}
