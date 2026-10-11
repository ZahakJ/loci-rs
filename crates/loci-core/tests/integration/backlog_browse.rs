//! Port of recall `tests/unit/integration/backlog-browse.test.ts`.

use super::helpers::*;
use loci_core::error::ErrorCode;
use loci_core::text::split_tags;
use loci_core::types::*;
use regex::Regex;
use std::collections::BTreeMap;

const DAY_MS: i64 = 86_400_000;

fn browse_req(query: &str, mode: BrowseMode, column: BrowseColumn, descending: bool, offset: i64, limit: i64) -> BrowseRequest {
    BrowseRequest { query: query.into(), mode, sort: BrowseSort { column, descending }, offset, limit }
}

fn cell(r: &BrowseRow, c: BrowseColumn) -> &str {
    r.cells.get(&c).map(String::as_str).unwrap_or("")
}

// ── backlog and catch-up on the fixture ──

#[test]
fn reports_the_backlog_history_is_more_than_3_days_overdue() {
    let h = imported_fixture();
    let r = h.col.backlog_report(None).unwrap();
    let due_sql = h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due <= 0");
    assert_eq!(r.due, due_sql);
    assert_eq!(r.overdue, h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due < 0"));
    assert!(r.max_overdue_days > 3);
    assert!(r.triggered);
    assert!(r.daily_average > 0.0);
    assert!(r.estimated_minutes > 0);
    assert!(r.days_at_current_pace >= 1);
}

#[test]
fn spread_over_n_days_leaves_about_one_nth_of_todays_reviews_undo_restores() {
    let mut h = imported_fixture();
    let before = h.col.backlog_report(None).unwrap();
    let reviews_due = h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due <= 0");
    assert_eq!(before.due, reviews_due);
    let n = 3;
    let res = h.col.apply_catch_up(None, &CatchUpPlan::Spread { days: n }).unwrap();
    assert_eq!(res.postponed + res.kept_today, before.due);
    assert_eq!(res.undo_label, "Catch-up plan");
    let after = h.col.backlog_report(None).unwrap();
    assert_eq!(after.due, (reviews_due as f64 / n as f64).ceil() as i64);
    assert_eq!(after.due, res.kept_today);
    // the day after tomorrow got cards too, never beyond N-1 days
    assert!(h.count_p("SELECT count(*) FROM cards WHERE queue = 2 AND due = ?", [n - 1]) > 0);
    assert_eq!(h.count_p("SELECT count(*) FROM revlog WHERE type = ?", [RevlogType::Rescheduled as i64]), res.postponed);
    h.col.undo().unwrap();
    assert_eq!(h.col.backlog_report(None).unwrap().due, before.due);
    assert_eq!(h.count_p("SELECT count(*) FROM revlog WHERE type = ?", [RevlogType::Rescheduled as i64]), 0);
}

#[test]
fn todays_share_keeps_the_daily_average_and_spreads_the_rest_over_a_week() {
    let mut h = imported_fixture();
    let before = h.col.backlog_report(None).unwrap();
    let res = h.col.apply_catch_up(None, &CatchUpPlan::TodayShare).unwrap();
    let interday = h.count("SELECT count(*) FROM cards WHERE queue = 3 AND due <= 0");
    assert_eq!(res.kept_today + res.postponed, before.due);
    assert_eq!(res.kept_today, before.due.min((before.daily_average.round() as i64 - interday).max(0)));
    assert_eq!(h.col.backlog_report(None).unwrap().due, res.kept_today);
    assert_eq!(
        h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due > 7 AND id IN (SELECT cid FROM revlog WHERE type = 5)"),
        0
    );
}

#[test]
fn lower_retention_for_n_days_restored_by_maintenance_afterwards_undoable() {
    let mut h = imported_fixture();
    let deck_id = h.deck_id("History");
    let preset_id = h.col.get_deck(deck_id).unwrap().config_id;
    let original = h.col.get_preset(preset_id).unwrap().config.desired_retention;
    let r = h.col.apply_catch_up(Some(deck_id), &CatchUpPlan::LowerRetention { retention: 0.8, days: 7 }).unwrap();
    assert_eq!(r.postponed, 0);
    assert_eq!(h.col.get_preset(preset_id).unwrap().config.desired_retention, 0.8);
    h.col.undo().unwrap();
    assert_eq!(h.col.get_preset(preset_id).unwrap().config.desired_retention, original);
    h.col.apply_catch_up(Some(deck_id), &CatchUpPlan::LowerRetention { retention: 0.8, days: 7 }).unwrap();
    h.col.maintenance().unwrap();
    assert_eq!(h.col.get_preset(preset_id).unwrap().config.desired_retention, 0.8);
    h.clock.advance(7 * DAY_MS);
    h.col.maintenance().unwrap();
    assert_eq!(h.col.get_preset(preset_id).unwrap().config.desired_retention, original);
}

// ── "due today" means the same on every surface ──

#[test]
fn backlog_report_equals_its_insight_and_stats_forecast_day_0_home_pills_below_browse_is_due() {
    let h = imported_fixture();
    let d = h.count("SELECT count(*) FROM cards WHERE queue = 2 AND due <= 0");
    assert!(d > 0);
    let report = h.col.backlog_report(None).unwrap();
    assert_eq!(report.due, d);
    let stats = h.col.stats(&StatsRequest { deck_id: None }).unwrap();
    assert_eq!(stats.forecast[0].due, d);
    let insights = h.col.insights().unwrap();
    let insight = insights.iter().find(|i| i.id == "backlog");
    if report.triggered {
        let text = &insight.expect("backlog insight").text;
        assert!(text.starts_with(&format!("{d} reviews are waiting")), "{text}");
    }
    let tree = h.col.deck_tree().unwrap();
    assert!(tree.review_count <= d);
    // Browse "Due" is Anki's is:due: due reviews + interday learning due today + learning due now
    let learn_due_today = h.count_p(
        "SELECT count(*) FROM cards WHERE (queue = 3 AND due <= 0) OR (queue IN (1, 4) AND due <= ?)",
        [h.clock.ms().div_euclid(1000)],
    );
    assert_eq!(h.col.search_ids("is:due", BrowseMode::Cards).unwrap().len() as i64, d + learn_due_today);
    // the same relations hold per deck
    for n in &tree.children {
        let r = h.col.backlog_report(Some(n.deck.id)).unwrap();
        let s = h.col.stats(&StatsRequest { deck_id: Some(n.deck.id) }).unwrap();
        assert_eq!(s.forecast[0].due, r.due, "{}", n.deck.name);
        assert!(n.review_count <= r.due, "{}", n.deck.name);
    }
}

// ── browse cells read as text ──

#[test]
fn browse_cells_have_no_markup_data_urls_or_long_media_names() {
    let h = make_collection();
    h.col.install_sample_deck().unwrap();
    let res = h.col.browse(&browse_req("deck:*", BrowseMode::Cards, BrowseColumn::SortField, false, 0, 100)).unwrap();
    assert_eq!(res.rows.len(), 30);
    let bad = Regex::new(r"\{\{c\d|data:|%3Csvg|<\w").unwrap();
    for r in &res.rows {
        for col in [BrowseColumn::SortField, BrowseColumn::Question, BrowseColumn::Answer] {
            let v = cell(r, col);
            assert!(!bad.is_match(v), "{col:?}: {v}");
            assert!(v.chars().count() <= 120);
        }
    }
    let palace = res.rows.iter().find(|r| cell(r, BrowseColumn::SortField).contains("memory-palace route")).expect("palace row");
    assert!(cell(palace, BrowseColumn::SortField).starts_with("[image] On a memory-palace route"));
    assert!(res.rows.iter().any(|r| cell(r, BrowseColumn::SortField).starts_with("Cicero and Quintilian")));

    // image occlusion: the shapes field has no text, so the sort column shows the next field with some
    let fx = imported_fixture();
    let io =
        fx.col.browse(&browse_req("\"note:Image Occlusion\"", BrowseMode::Notes, BrowseColumn::SortField, false, 0, 50)).unwrap();
    assert!(!io.rows.is_empty());
    let io_bad = Regex::new(r"image-occlusion|\{\{c").unwrap();
    for r in &io.rows {
        let v = cell(r, BrowseColumn::SortField);
        assert_ne!(v, "");
        assert!(!io_bad.is_match(v), "{v}");
    }
}

// ── browse on the fixture ──

fn cards_of(h: &Harness, nids: &[i64]) -> Vec<i64> {
    if nids.is_empty() {
        return vec![];
    }
    let list = nids.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    h.ids(&format!("SELECT id FROM cards WHERE nid IN ({list})"))
}

#[test]
fn deck_languages_is_due() {
    let h = imported_fixture();
    let now = h.clock.ms().div_euclid(1000);
    let want: Vec<i64> = sorted(
        h.db()
            .all(
                "SELECT c.id FROM cards c JOIN decks d ON d.id = c.did WHERE (d.name = 'Languages' OR d.name LIKE 'Languages::%')
                  AND ((c.queue IN (2, 3) AND c.due <= 0) OR (c.queue IN (1, 4) AND c.due <= ?))",
                [now],
                |r| r.get(0),
            )
            .unwrap(),
    );
    assert!(!want.is_empty());
    assert_eq!(sorted(h.col.search_ids("deck:Languages is:due", BrowseMode::Cards).unwrap()), want);
}

#[test]
fn tag_hierarchy_quoted_phrase_prop_r_and_flag_searches() {
    let h = imported_fixture();
    let notes: Vec<(i64, String, String)> =
        h.db().all("SELECT id, tags, plain FROM notes", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    let tagged: Vec<i64> = notes
        .iter()
        .filter(|(_, tags, _)| {
            split_tags(tags).iter().any(|t| {
                let l = t.to_lowercase();
                l == "arabic::verbs" || l.starts_with("arabic::verbs::")
            })
        })
        .map(|(id, _, _)| *id)
        .collect();
    assert!(!tagged.is_empty());
    assert_eq!(sorted(h.col.search_ids("tag:arabic::verbs", BrowseMode::Cards).unwrap()), cards_of(&h, &tagged));
    assert_eq!(sorted(h.col.search_ids("tag:*::verbs::*", BrowseMode::Cards).unwrap()), cards_of(&h, &tagged));

    // a two-word phrase taken from a note
    let two_words = Regex::new(r"^([A-Za-z]+ [A-Za-z]+)").unwrap();
    let phrase = notes
        .iter()
        .find_map(|(_, _, plain)| two_words.captures(plain.trim()).map(|c| c[1].to_string()))
        .expect("a two-word note");
    let with_phrase: Vec<i64> = notes
        .iter()
        .filter(|(_, _, plain)| plain.to_lowercase().contains(&phrase.to_lowercase()))
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(sorted(h.col.search_ids(&format!("\"{phrase}\""), BrowseMode::Notes).unwrap()), sorted(with_phrase));

    // R for every card with a memory state, whatever its queue: days since lrt, else since the
    // review card's last due-based review, else since its latest graded revlog row
    let cards: Vec<(i64, i64, i64, i64, String, Option<i64>)> = h
        .db()
        .all(
            "SELECT id, type, due, ivl, data, (SELECT max(id) FROM revlog r WHERE r.cid = cards.id AND r.ease BETWEEN 1 AND 4) AS last FROM cards",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    let now_ms = h.clock.ms();
    let low_r: Vec<i64> = cards
        .iter()
        .filter(|(_, ctype, due, ivl, data, last)| {
            let d: serde_json::Value = serde_json::from_str(data).unwrap_or(serde_json::Value::Null);
            let s = d.get("s").and_then(|v| v.as_f64());
            let dd = d.get("d").and_then(|v| v.as_f64());
            let (Some(s), Some(_)) = (s.filter(|s| *s > 0.0), dd) else { return false };
            let decay = d.get("decay").and_then(|v| v.as_f64()).unwrap_or(0.1542);
            let lrt = d.get("lrt").and_then(|v| v.as_f64()).filter(|l| *l > 0.0);
            let elapsed = if let Some(lrt) = lrt {
                ((now_ms as f64 / 1000.0 - lrt) / 86400.0).max(0.0)
            } else if *due < 1_000_000_000 && (*ctype == 2 || *ctype == 3) {
                (0 - (due - ivl)).max(0) as f64
            } else if let Some(last) = last {
                ((now_ms - last) as f64 / 86_400_000.0).max(0.0)
            } else {
                0.0
            };
            (1.0 + (0.9f64.powf(-1.0 / decay) - 1.0) * elapsed / s).powf(-decay) < 0.9
        })
        .map(|c| c.0)
        .collect();
    assert!(!low_r.is_empty());
    assert_eq!(sorted(h.col.search_ids("prop:r<0.9", BrowseMode::Cards).unwrap()), sorted(low_r));

    let flagged = h.ids("SELECT id FROM cards WHERE flags & 7 = 1");
    assert_eq!(sorted(h.col.search_ids("flag:1", BrowseMode::Cards).unwrap()), flagged);
}

#[test]
fn browse_pages_sorts_and_renders_cells_notes_mode_groups_cards() {
    let h = imported_fixture();
    let res = h.col.browse(&browse_req("deck:*", BrowseMode::Cards, BrowseColumn::Interval, true, 0, 25)).unwrap();
    assert_eq!(res.total, h.count("SELECT count(*) FROM cards"));
    assert_eq!(res.rows.len(), 25);
    let ivls: Vec<i64> = res.rows.iter().map(|r| h.col.get_card(r.card_id).unwrap().ivl).collect();
    let mut desc = ivls.clone();
    desc.sort_by(|a, b| b.cmp(a));
    assert_eq!(desc, ivls);
    let row = &res.rows[0];
    assert!(!cell(row, BrowseColumn::Question).is_empty());
    assert!(cell(row, BrowseColumn::Question).chars().count() <= 120);
    assert!(!cell(row, BrowseColumn::Deck).is_empty());
    assert!(Regex::new(r"^\(?\d{4}-\d\d-\d\d").unwrap().is_match(cell(row, BrowseColumn::Due)));
    let page2 = h.col.browse(&browse_req("deck:*", BrowseMode::Cards, BrowseColumn::Interval, true, 25, 25)).unwrap();
    assert_eq!(page2.rows[0].id, res.ids[25]);
    let notes = h.col.browse(&browse_req("deck:*", BrowseMode::Notes, BrowseColumn::SortField, false, 0, 10)).unwrap();
    assert_eq!(notes.total, h.count("SELECT count(*) FROM notes"));
    assert!(Regex::new(r"^\d+$").unwrap().is_match(cell(&notes.rows[0], BrowseColumn::CardCount)));
    for col in [
        BrowseColumn::Due,
        BrowseColumn::Retrievability,
        BrowseColumn::Deck,
        BrowseColumn::Notetype,
        BrowseColumn::Template,
        BrowseColumn::Tags,
        BrowseColumn::Created,
        BrowseColumn::Modified,
        BrowseColumn::CardCount,
        BrowseColumn::Stability,
        BrowseColumn::Difficulty,
        BrowseColumn::Ease,
        BrowseColumn::Reps,
        BrowseColumn::Lapses,
        BrowseColumn::Question,
    ] {
        let mode = if col == BrowseColumn::CardCount { BrowseMode::Notes } else { BrowseMode::Cards };
        let r = h.col.browse(&browse_req("re:.", mode, col, false, 0, 3)).unwrap();
        assert_eq!(r.rows.len(), 3, "{col:?}");
    }
    let err = h.col.browse(&browse_req("deck:(", BrowseMode::Cards, BrowseColumn::Due, false, 0, 1)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Invalid);
}

#[test]
fn due_cells_learning_today_shows_the_time_later_the_date_new_cards_their_position() {
    let h = imported_fixture();
    let due = |q: &str| -> Vec<String> {
        h.col
            .browse(&browse_req(q, BrowseMode::Cards, BrowseColumn::Due, false, 0, 500))
            .unwrap()
            .rows
            .iter()
            .map(|r| cell(r, BrowseColumn::Due).to_string())
            .collect()
    };
    let learning = due("is:learn");
    assert!(!learning.is_empty());
    let next_day_at = h.col.core().next_day_at().unwrap();
    let today_re = Regex::new(r"^\(?Today \d\d:\d\d\)?$").unwrap();
    let date_re = Regex::new(r"^\(?\d{4}-\d\d-\d\d\)?$").unwrap();
    for r in h.col.browse(&browse_req("is:learn", BrowseMode::Cards, BrowseColumn::Due, false, 0, 500)).unwrap().rows {
        let c = h.col.get_card(r.card_id).unwrap();
        let v = cell(&r, BrowseColumn::Due);
        if c.due > 1_000_000_000 && c.due >= next_day_at - 86_400 && c.due < next_day_at {
            assert!(today_re.is_match(v), "{v}");
        } else {
            assert!(date_re.is_match(v), "{v}");
        }
    }
    assert!(learning.iter().any(|d| d.contains("Today")));
    // a learning card pushed past today shows its date, not a time
    let id = h.col.search_ids("is:learn", BrowseMode::Cards).unwrap()[0];
    h.db().run("UPDATE cards SET due = ? WHERE id = ?", rusqlite::params![next_day_at + 3 * 3600, id]).unwrap();
    let row = h.col.browse(&browse_req(&format!("cid:{id}"), BrowseMode::Cards, BrowseColumn::Due, false, 0, 1)).unwrap();
    assert!(Regex::new(r"^\d{4}-\d\d-\d\d$").unwrap().is_match(cell(&row.rows[0], BrowseColumn::Due)));
    assert!(Regex::new(r"^\(?New #\d+\)?$").unwrap().is_match(&due("is:new")[0]));
    assert!(date_re.is_match(&due("is:review is:due")[0]));
}

#[test]
fn sidebar_lists_saved_searches_today_flags_decks_note_types_and_tags() {
    let h = imported_fixture();
    h.col.save_search("Struggling", "tag:leech").unwrap();
    let items = h.col.sidebar().unwrap();
    let kinds: Vec<SidebarKind> = items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [SidebarKind::Saved, SidebarKind::Today, SidebarKind::Flag, SidebarKind::Deck, SidebarKind::Notetype, SidebarKind::Tag]
    );
    let saved = &items[0].children.as_ref().unwrap()[0];
    assert_eq!((saved.label.as_str(), saved.query.as_str()), ("Struggling", "tag:leech"));
    let decks = items[3].children.as_ref().unwrap();
    assert!(decks
        .iter()
        .any(|d| d.label == "Languages" && d.children.as_ref().is_some_and(|c| c.iter().any(|x| x.label == "Arabic"))));
    for d in decks {
        assert_eq!(h.col.search_ids(&d.query, BrowseMode::Cards).unwrap().len() as i64, d.count.unwrap(), "{}", d.query);
    }
    let tags = items[5].children.as_ref().unwrap();
    assert!(!tags.is_empty());
    let due = items[1].children.as_ref().unwrap().iter().find(|c| c.query == "is:due").unwrap();
    assert_eq!(due.count, Some(h.col.search_ids("is:due", BrowseMode::Cards).unwrap().len() as i64));
}

#[test]
fn bulk_set_due_suspend_and_find_replace_each_undoable() {
    let mut h = imported_fixture();
    let target: Vec<i64> = h.col.search_ids("is:new", BrowseMode::Cards).unwrap().into_iter().take(5).collect();
    let before: BTreeMap<i64, Card> = target.iter().map(|id| (*id, h.col.get_card(*id).unwrap())).collect();
    let sd = h.col.bulk(&target, &BulkAction::SetDue { days: "3-7".into() }).unwrap();
    assert_eq!((sd.affected, sd.undo_label.as_str()), (5, "Set due date"));
    for id in &target {
        let c = h.col.get_card(*id).unwrap();
        assert_eq!(c.queue, CardQueue::Review);
        assert!(c.due >= 3 && c.due <= 7);
        assert_eq!(c.ivl, c.due);
        assert_eq!(c.data.pos, Some(before[id].due));
    }
    h.col.undo().unwrap();
    for id in &target {
        assert_eq!(h.col.get_card(*id).unwrap(), before[id]);
    }

    let review: (i64, i64, i64) = h
        .db()
        .get("SELECT id, ivl, due FROM cards WHERE queue = 2 AND due > 5 LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .unwrap();
    h.col.bulk(&[review.0], &BulkAction::SetDue { days: "1".into() }).unwrap();
    let moved = h.col.get_card(review.0).unwrap();
    assert_eq!(moved.due, 1);
    if moved.data.s.is_none() {
        assert_eq!(moved.ivl, (review.1 + 1 - review.2).max(1));
    }
    h.col.bulk(&[review.0], &BulkAction::SetDue { days: "2!".into() }).unwrap();
    let forced = h.col.get_card(review.0).unwrap();
    assert_eq!((forced.due, forced.ivl), (2, 2));

    let sus = h.col.bulk(&target, &BulkAction::Suspend).unwrap();
    assert_eq!(sus.affected, 5);
    assert_eq!(sus.undo_label, "Suspend");
    let list = target.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    assert_eq!(h.col.search_ids(&format!("cid:{list} is:suspended"), BrowseMode::Cards).unwrap().len(), 5);
    h.col.undo().unwrap();
    assert_eq!(h.col.search_ids(&format!("cid:{list} is:suspended"), BrowseMode::Cards).unwrap().len(), 0);

    let card = h.col.get_card(target[0]).unwrap();
    let note = h.col.get_note(card.nid).unwrap();
    let nt = h.col.get_notetype(note.ntid).unwrap();
    let plain = Regex::new(r"<[^>]*>").unwrap().replace_all(&note.fields[0], "").into_owned();
    let word = Regex::new(r"[A-Za-z]{4,}").unwrap().find(&plain).expect("a word").as_str().to_string();
    let fr = h
        .col
        .bulk(
            &[card.id],
            &BulkAction::FindReplace {
                find: word.clone(),
                replace: "ZZZ".into(),
                regex: false,
                field: Some(nt.fields[0].name.clone()),
                match_case: true,
            },
        )
        .unwrap();
    assert_eq!(fr.affected, 1);
    assert!(h.col.get_note(note.id).unwrap().fields[0].contains("ZZZ"));
    assert_eq!(h.col.search_ids("ZZZ", BrowseMode::Notes).unwrap(), vec![note.id]);
    h.col.undo().unwrap();
    assert_eq!(h.col.get_note(note.id).unwrap().fields, note.fields);
    let rx = h
        .col
        .bulk(
            &[card.id],
            &BulkAction::FindReplace { find: "^(.)".into(), replace: "[$1]".into(), regex: true, field: None, match_case: false },
        )
        .unwrap();
    assert_eq!(rx.affected, 1);
    assert_eq!(rx.undo_label, "Find and replace");
    assert!(h.col.get_note(note.id).unwrap().fields[0].starts_with('['));
    // counts notes, not cards: every card of two notes selected → 2
    let two_notes: Vec<i64> = h.col.search_ids("is:review", BrowseMode::Notes).unwrap().into_iter().take(2).collect();
    let nid_list = two_notes.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let their_cards = h.col.search_ids(&format!("nid:{nid_list}"), BrowseMode::Cards).unwrap();
    let all = h
        .col
        .bulk(
            &their_cards,
            &BulkAction::FindReplace { find: "^".into(), replace: "~".into(), regex: true, field: None, match_case: false },
        )
        .unwrap();
    assert_eq!(all.affected, 2);
    // nothing matched: 0 and still a label (no undo entry recorded)
    let label_before = h.col.undo_label().unwrap();
    let none = h
        .col
        .bulk(
            &[card.id],
            &BulkAction::FindReplace {
                find: "no-such-text-anywhere".into(),
                replace: "x".into(),
                regex: false,
                field: None,
                match_case: false,
            },
        )
        .unwrap();
    assert_eq!((none.affected, none.undo_label.as_str()), (0, "Find and replace"));
    assert_eq!(h.col.undo_label().unwrap(), label_before);
}

#[test]
fn bulk_reposition_forget_tags_flags_and_change_deck() {
    let mut h = imported_fixture();
    let fresh: Vec<i64> = h.col.search_ids("is:new", BrowseMode::Cards).unwrap().into_iter().take(4).collect();
    let rp = h.col.bulk(&fresh, &BulkAction::Reposition { start: 0, step: 1, randomize: false, shift: true }).unwrap();
    assert_eq!(rp.affected, 4);
    let dues: Vec<i64> = fresh.iter().map(|id| h.col.get_card(*id).unwrap().due).collect();
    assert_eq!(dues.iter().min(), Some(&0));
    h.col.undo().unwrap();

    let rev: Vec<i64> = h.col.search_ids("is:review -is:suspended", BrowseMode::Cards).unwrap().into_iter().take(3).collect();
    let fg = h.col.bulk(&rev, &BulkAction::Forget { reset_counts: true, restore_position: true }).unwrap();
    assert_eq!(fg.affected, 3);
    for id in &rev {
        let c = h.col.get_card(*id).unwrap();
        assert_eq!((c.ctype, c.queue, c.reps, c.lapses), (CardType::New, CardQueue::New, 0, 0));
    }
    h.col.undo().unwrap();
    for id in &rev {
        assert_ne!(h.col.get_card(*id).unwrap().ctype, CardType::New);
    }

    let t = h.col.bulk(&rev, &BulkAction::AddTags { tags: strs(&["catchup::later"]) }).unwrap();
    assert!(t.affected > 0);
    assert!(h.col.search_ids("tag:catchup::later", BrowseMode::Cards).unwrap().len() >= 3);
    h.col.bulk(&rev, &BulkAction::RemoveTags { tags: strs(&["catchup::*"]) }).unwrap();
    assert_eq!(h.col.search_ids("tag:catchup::later", BrowseMode::Cards).unwrap().len(), 0);
    h.col.bulk(&rev, &BulkAction::Flag { flag: 4 }).unwrap();
    assert!(h.col.search_ids("flag:4", BrowseMode::Cards).unwrap().len() >= 3);
    let deck = h.col.create_deck("Moved", None).unwrap();
    let cd = h.col.bulk(&rev, &BulkAction::ChangeDeck { deck_id: deck.id }).unwrap();
    assert_eq!(cd.affected, 3);
    assert_eq!(sorted(h.col.search_ids("deck:Moved", BrowseMode::Cards).unwrap()), sorted(rev.clone()));
    h.col.undo().unwrap();
    assert_eq!(h.col.search_ids("deck:Moved", BrowseMode::Cards).unwrap().len(), 0);
}
