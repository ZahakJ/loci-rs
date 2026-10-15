//! Queue tests, plus the SQLite loaders.

use super::{local_ms, make_card, make_config};
use crate::collection::rows::{card_params, insert_sql, CARD_COLUMNS};
use crate::db::Db;
use crate::scheduler::clock::SchedClock;
use crate::scheduler::convert::collection_day;
use crate::scheduler::queue::*;
use crate::types::*;
use std::sync::atomic::{AtomicI64, Ordering};

const ROLL: i64 = 4;
fn crt() -> i64 {
    local_ms(2022, 1, 1, 4, 0) / 1000
}
fn now() -> i64 {
    local_ms(2023, 6, 10, 12, 0)
}
fn today() -> i64 {
    collection_day(now(), crt(), ROLL)
}
fn now_s() -> i64 {
    now() / 1000
}

static NEXT_ID: AtomicI64 = AtomicI64::new(1000);
fn card(f: impl FnOnce(&mut Card)) -> Card {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut c = Card { id, nid: id, ..make_card() };
    f(&mut c);
    c
}
fn rev(did: i64, due: i64, f: impl FnOnce(&mut Card)) -> Card {
    card(|c| {
        c.did = did;
        c.ctype = CardType::Review;
        c.queue = CardQueue::Review;
        c.due = due;
        c.ivl = 10;
        c.factor = 2500;
        f(c);
    })
}
fn fresh(did: i64, due: i64, f: impl FnOnce(&mut Card)) -> Card {
    card(|c| {
        c.did = did;
        c.due = due;
        f(c);
    })
}
fn deck(id: i64, name: &str, f: impl FnOnce(&mut DeckConfig)) -> QueueDeck {
    let mut config = make_config();
    f(&mut config);
    QueueDeck { id, name: name.to_string(), config, today: None }
}
fn base<'a>(cards: &'a [Card], decks: &'a [QueueDeck], deck_id: Option<i64>) -> QueueInput<'a> {
    QueueInput {
        cards,
        decks,
        deck_id,
        now_ms: now(),
        today: today(),
        crt: crt(),
        rollover_hour: ROLL,
        learn_ahead_secs: None,
        seed: Some(7),
        apply_all_parent_limits: false,
        order_config: None,
    }
}
fn ids(cards: &[&Card]) -> Vec<i64> {
    cards.iter().map(|c| c.id).collect()
}
fn counts(n: i64, l: i64, r: i64) -> StudyCounts {
    StudyCounts { new: n, learn: l, review: r }
}

#[test]
fn intersperser_spreads_the_shorter_list_evenly() {
    assert_eq!(intersperse(&[1, 2, 3, 4], &[10, 20]), vec![1, 10, 2, 3, 20, 4]);
    assert_eq!(intersperse::<i32>(&[], &[1]), vec![1]);
    assert_eq!(intersperse(&[1, 2], &[]), vec![1, 2]);
}

#[test]
fn intraday_learning_due_now_first_learn_ahead_last_later_excluded_counts() {
    let l_now = card(|c| {
        c.ctype = CardType::Learn;
        c.queue = CardQueue::Learn;
        c.due = now_s() - 5;
    });
    let l_ahead = card(|c| {
        c.ctype = CardType::Learn;
        c.queue = CardQueue::Learn;
        c.due = now_s() + 600;
    });
    let l_later = card(|c| {
        c.ctype = CardType::Learn;
        c.queue = CardQueue::Learn;
        c.due = now_s() + 3600;
    });
    let dl = card(|c| {
        c.ctype = CardType::Learn;
        c.queue = CardQueue::DayLearn;
        c.due = today();
    });
    let r1 = rev(1, today() - 1, |_| {});
    let r2 = rev(1, today(), |_| {});
    let n1 = fresh(1, 1, |_| {});
    let susp = rev(1, today(), |c| c.queue = CardQueue::Suspended);
    let future = rev(1, today() + 1, |_| {});
    let cards =
        vec![l_ahead.clone(), l_later.clone(), l_now.clone(), dl.clone(), r1.clone(), r2.clone(), n1.clone(), susp, future];
    let decks = vec![deck(1, "Default", |c| {
        c.new_review_mix = NewReviewMix::ReviewFirst;
        c.interday_learning_mix = NewReviewMix::ReviewFirst;
    })];
    let q = build_queue(&base(&cards, &decks, Some(1)));
    assert_eq!(q.cards, ids(&[&l_now, &r1, &r2, &dl, &n1, &l_ahead]));
    assert_eq!(q.counts, counts(1, 3, 2));
    assert_eq!(q.later_learning, vec![LaterLearning { id: l_later.id, due: l_later.due }]);
}

#[test]
fn learn_ahead_limit_is_configurable() {
    let l = card(|c| {
        c.ctype = CardType::Learn;
        c.queue = CardQueue::Learn;
        c.due = now_s() + 600;
    });
    let cards = vec![l];
    let decks = vec![deck(1, "D", |_| {})];
    let mut zero = base(&cards, &decks, Some(1));
    zero.learn_ahead_secs = Some(0);
    assert_eq!(build_queue(&zero).counts.learn, 0);
    assert_eq!(build_queue(&base(&cards, &decks, Some(1))).counts.learn, 1);
}

#[test]
fn parent_limits_cap_children_and_child_limits_cap_themselves() {
    let decks = vec![
        deck(1, "P", |c| {
            c.reviews_per_day = 5;
            c.new_per_day = 3;
        }),
        deck(2, "P::A", |c| {
            c.reviews_per_day = 4;
            c.new_per_day = 10;
        }),
        deck(3, "P::B", |c| {
            c.reviews_per_day = 100;
            c.new_per_day = 10;
        }),
    ];
    let mut cards = Vec::new();
    for i in 0..6 {
        cards.push(rev(2, today() - i, |_| {}));
    }
    for i in 0..6 {
        cards.push(rev(3, today() - i, |_| {}));
    }
    for i in 0..5 {
        cards.push(fresh(2, i, |_| {}));
    }
    let parent = build_queue(&base(&cards, &decks, Some(1)));
    assert_eq!(parent.counts.review, 5);
    assert_eq!(parent.counts.new, 0); // the review limit also caps new cards
    let child = build_queue(&base(&cards, &decks, Some(2)));
    assert_eq!(child.counts.review, 4);
    assert_eq!(child.counts.new, 0);
    let mut all = base(&cards, &decks, Some(3));
    all.apply_all_parent_limits = true;
    assert_eq!(build_queue(&all).counts.review, 5);
    let looser = vec![
        deck(1, "P", |c| {
            c.reviews_per_day = 50;
            c.new_per_day = 3;
        }),
        deck(2, "P::A", |c| {
            c.reviews_per_day = 20;
            c.new_per_day = 10;
        }),
        decks[2].clone(),
    ];
    assert_eq!(build_queue(&base(&cards, &looser, Some(1))).counts, counts(3, 0, 12));
    let dc = due_counts(&base(&cards, &looser, None));
    assert_eq!(dc[&1], counts(3, 0, 12));
    assert_eq!(dc[&2], counts(5, 0, 6));
    assert_eq!(dc[&3], counts(0, 0, 6));
    // the child's exhausted review limit leaves no room for its new cards
    assert_eq!(due_counts(&base(&cards, &decks, None))[&2], counts(0, 0, 4));
}

#[test]
fn respects_todays_already_studied_counters() {
    let mut d = deck(1, "D", |c| c.new_per_day = 5);
    d.today = Some(DeckToday { day: today(), new: 4, rev: 0, lrn: 0, time_ms: 0 });
    let cards: Vec<Card> = (0..5).map(|i| fresh(1, i, |_| {})).collect();
    let decks = vec![d.clone()];
    assert_eq!(build_queue(&base(&cards, &decks, Some(1))).counts.new, 1);
    let mut stale = d;
    stale.today.as_mut().unwrap().day = today() - 1;
    let decks = vec![stale];
    assert_eq!(build_queue(&base(&cards, &decks, Some(1))).counts.new, 5);
}

#[test]
fn review_orders() {
    let a = rev(1, today() - 5, |c| {
        c.ivl = 3;
        c.factor = 2000;
    });
    let b = rev(1, today() - 1, |c| {
        c.ivl = 30;
        c.factor = 2600;
    });
    let c = rev(1, today() - 3, |c| {
        c.ivl = 10;
        c.factor = 2300;
    });
    let cards = vec![a.clone(), b.clone(), c.clone()];
    let order = |ro: ReviewOrder| {
        let decks = vec![deck(1, "D", |cfg| {
            cfg.review_order = ro;
            cfg.scheduler = SchedulerKind::Sm2;
        })];
        build_queue(&base(&cards, &decks, Some(1))).cards
    };
    assert_eq!(order(ReviewOrder::DueThenRandom), ids(&[&a, &c, &b]));
    assert_eq!(order(ReviewOrder::AscendingIntervals), ids(&[&a, &c, &b]));
    assert_eq!(order(ReviewOrder::DescendingIntervals), ids(&[&b, &c, &a]));
    assert_eq!(order(ReviewOrder::AscendingEase), ids(&[&a, &c, &b]));
    assert_eq!(order(ReviewOrder::DescendingEase), ids(&[&b, &c, &a]));
    assert_eq!(order(ReviewOrder::Added), ids(&[&a, &b, &c]));
    assert_eq!(order(ReviewOrder::ReverseAdded), ids(&[&c, &b, &a]));
    // relative overdueness (SM-2): a = 5/3 overdue, c = 3/10, b = 1/30
    assert_eq!(order(ReviewOrder::RelativeOverdue), ids(&[&a, &c, &b]));
    assert_eq!(order(ReviewOrder::Random), order(ReviewOrder::Random));
}

#[test]
fn relative_overdueness_under_fsrs_uses_retrievability_vs_desired_retention() {
    let lrt = now_s() - 20 * 86400;
    let shaky = rev(1, today() - 2, |c| {
        c.ivl = 18;
        c.data = CardData { s: Some(5.0), d: Some(5.0), lrt: Some(lrt), ..CardData::default() };
    });
    let solid = rev(1, today() - 10, |c| {
        c.ivl = 10;
        c.data = CardData { s: Some(200.0), d: Some(5.0), lrt: Some(lrt), ..CardData::default() };
    });
    let cards = vec![solid.clone(), shaky.clone()];
    let decks = vec![deck(1, "D", |c| c.review_order = ReviewOrder::RelativeOverdue)];
    assert_eq!(build_queue(&base(&cards, &decks, Some(1))).cards, ids(&[&shaky, &solid]));
}

#[test]
fn new_gather_and_sort_options() {
    let z1 = fresh(2, 1, |_| {});
    let a5 = fresh(3, 5, |_| {});
    let a2 = fresh(3, 2, |c| c.ord = 1);
    let a3 = fresh(3, 3, |_| {});
    let cards = vec![z1.clone(), a5.clone(), a2.clone(), a3.clone()];
    let run = |gather: NewCardGather, sort: NewCardSort| {
        let decks = vec![
            deck(1, "D", |c| {
                c.new_gather = gather;
                c.new_sort = sort;
            }),
            deck(2, "D::Z", |_| {}),
            deck(3, "D::A", |_| {}),
        ];
        build_queue(&base(&cards, &decks, Some(1))).cards
    };
    assert_eq!(run(NewCardGather::Deck, NewCardSort::Order), ids(&[&a2, &a3, &a5, &z1]));
    assert_eq!(run(NewCardGather::Deck, NewCardSort::TemplateThenOrder), ids(&[&a3, &a5, &z1, &a2]));
    assert_eq!(run(NewCardGather::Ascending, NewCardSort::Order), ids(&[&z1, &a2, &a3, &a5]));
    assert_eq!(run(NewCardGather::Descending, NewCardSort::Order), ids(&[&a5, &a3, &a2, &z1]));
    let r = run(NewCardGather::RandomCards, NewCardSort::Order);
    let mut sorted = r.clone();
    sorted.sort();
    let mut expected = ids(&[&z1, &a5, &a2, &a3]);
    expected.sort();
    assert_eq!(sorted, expected);
    assert_eq!(run(NewCardGather::RandomCards, NewCardSort::Order), r);
}

#[test]
fn new_review_mix() {
    let rs: Vec<Card> = (0..4).map(|i| rev(1, today() - i, |_| {})).collect();
    let ns: Vec<Card> = (0..2).map(|i| fresh(1, i, |_| {})).collect();
    let cards: Vec<Card> = rs.iter().chain(ns.iter()).cloned().collect();
    let q = |mix: NewReviewMix| {
        let decks = vec![deck(1, "D", |c| {
            c.new_review_mix = mix;
            c.review_order = ReviewOrder::DueThenRandom;
        })];
        build_queue(&base(&cards, &decks, Some(1))).cards
    };
    let rev_ids = ids(&[&rs[3], &rs[2], &rs[1], &rs[0]]);
    let mut review_first = rev_ids.clone();
    review_first.extend([ns[0].id, ns[1].id]);
    assert_eq!(q(NewReviewMix::ReviewFirst), review_first);
    let mut new_first = vec![ns[0].id, ns[1].id];
    new_first.extend(rev_ids.iter());
    assert_eq!(q(NewReviewMix::NewFirst), new_first);
    assert_eq!(q(NewReviewMix::Mix), vec![rev_ids[0], ns[0].id, rev_ids[1], rev_ids[2], ns[1].id, rev_ids[3]]);
}

#[test]
fn bury_siblings_earlier_kinds_bury_later_kinds() {
    let nid = 99;
    let l = card(|c| {
        c.nid = nid;
        c.ctype = CardType::Learn;
        c.queue = CardQueue::Learn;
        c.due = now_s() - 1;
    });
    let r = rev(1, today(), |c| {
        c.nid = nid;
        c.ord = 1;
    });
    let n = fresh(1, 1, |c| {
        c.nid = nid;
        c.ord = 2;
    });
    let r2 = rev(1, today(), |c| c.nid = 100);
    let r3 = rev(1, today(), |c| {
        c.nid = 100;
        c.ord = 1;
    });
    let cards = vec![l.clone(), r.clone(), n.clone(), r2, r3];
    let on_decks = vec![deck(1, "D", |c| {
        c.bury_siblings_new = true;
        c.bury_siblings_review = true;
    })];
    let on = build_queue(&base(&cards, &on_decks, Some(1)));
    assert!(on.cards.contains(&l.id));
    assert!(!on.cards.contains(&r.id));
    assert!(!on.cards.contains(&n.id));
    assert_eq!(on.counts.review, 1);
    let off_decks = vec![deck(1, "D", |_| {})];
    assert_eq!(build_queue(&base(&cards, &off_decks, Some(1))).counts, counts(1, 1, 3));
}

#[test]
fn unknown_deck_is_empty() {
    let decks = vec![deck(1, "D", |_| {})];
    assert_eq!(build_queue(&base(&[], &decks, Some(42))).cards, Vec::<i64>::new());
}

#[test]
fn sqlite_loaders_select_todays_candidates() {
    let db = Db::memory().unwrap();
    db.execute_batch(crate::SCHEMA_SQL).unwrap();
    let clock = SchedClock::at(now(), crt(), ROLL, 1200);
    let in_scope = [
        fresh(1, 1, |_| {}),
        rev(1, today(), |_| {}),
        rev(1, today() - 3, |_| {}),
        card(|c| {
            c.ctype = CardType::Learn;
            c.queue = CardQueue::Learn;
            c.due = clock.next_day_at - 1;
        }),
        card(|c| {
            c.ctype = CardType::Learn;
            c.queue = CardQueue::DayLearn;
            c.due = today();
        }),
    ];
    let out_of_scope = [
        rev(1, today() + 1, |_| {}),
        rev(1, today(), |c| c.queue = CardQueue::Suspended),
        card(|c| {
            c.ctype = CardType::Learn;
            c.queue = CardQueue::Learn;
            c.due = clock.next_day_at;
        }),
        rev(2, today(), |_| {}),
    ];
    let sql = insert_sql("cards", &CARD_COLUMNS);
    for c in in_scope.iter().chain(out_of_scope.iter()) {
        db.run(&sql, rusqlite::params_from_iter(card_params(c))).unwrap();
    }
    let mut got: Vec<i64> = load_queue_cards(&db, Some(&[1]), &clock).unwrap().into_iter().map(|c| c.id).collect();
    got.sort();
    let mut want: Vec<i64> = in_scope.iter().map(|c| c.id).collect();
    want.sort();
    assert_eq!(got, want);
    assert_eq!(load_queue_cards(&db, None, &clock).unwrap().len(), in_scope.len() + 1);
    assert_eq!(load_queue_cards(&db, Some(&[]), &clock).unwrap().len(), 0);
    assert_eq!(seconds_per_card(&db, now(), None).unwrap(), DEFAULT_SECONDS_PER_CARD);
    assert_eq!(card_revlog(&db, in_scope[0].id).unwrap(), vec![]);
    assert_eq!(deck_filter_sql(Some(&[1, 2]), "did"), ("did IN (?,?)".to_string(), vec![1, 2]));
    assert_eq!(expected_answers(&counts(2, 3, 4), 2), 11);
    assert_eq!(minutes_for(11, 8.0), 2);
    assert_eq!(minutes_for(0, 8.0), 0);
}

#[test]
fn siblings_to_bury_follows_the_preset() {
    let db = Db::memory().unwrap();
    db.execute_batch(crate::SCHEMA_SQL).unwrap();
    let answered = rev(1, today(), |c| c.nid = 500);
    let sib_new = fresh(1, 1, |c| c.nid = 500);
    let sib_rev = rev(1, today(), |c| c.nid = 500);
    let sib_future = rev(1, today() + 2, |c| c.nid = 500);
    let sql = insert_sql("cards", &CARD_COLUMNS);
    for c in [&answered, &sib_new, &sib_rev, &sib_future] {
        db.run(&sql, rusqlite::params_from_iter(card_params(c))).unwrap();
    }
    let none = make_config();
    assert_eq!(siblings_to_bury(&db, 500, answered.id, &none, today()).unwrap(), Vec::<i64>::new());
    let mut both = make_config();
    both.bury_siblings_new = true;
    both.bury_siblings_review = true;
    let mut got = siblings_to_bury(&db, 500, answered.id, &both, today()).unwrap();
    got.sort();
    let mut want = vec![sib_new.id, sib_rev.id];
    want.sort();
    assert_eq!(got, want);
}
