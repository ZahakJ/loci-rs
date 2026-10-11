//! Port of recall `tests/unit/search/search.test.ts` (parser cases, and the SQL cases over an
//! in-memory `Db` seeded with the same fixture as the TypeScript test).

mod parse_tests {
    use crate::error::LociError;
    use crate::search::parse::*;

    fn text(s: &str) -> SearchNode {
        SearchNode::Text { text: s.into() }
    }

    #[test]
    fn escapes_literal_wildcards() {
        assert_eq!(glob_to_like("a\\*b_c%"), "a*b_c\\%");
        assert_eq!(glob_to_regex_source("a\\_b*", "."), "a_b.*");
        assert_eq!(glob_to_regex_source("a.b/c-d", "."), "a\\.b/c-d");
        assert!(has_wildcards("a*") && has_wildcards("a_b") && !has_wildcards("a\\*b"));
        assert_eq!(unescape_text("a\\*b\\\\c"), "a*b\\c");
        assert_eq!(unescape_quotes("a\\\"b\\*"), "a\"b\\*");
    }

    #[test]
    fn reports_positions() {
        let err = |q: &str| parse_search(q).err();
        assert_eq!(err("(dog").map(|e| e.start), Some(0));
        assert_eq!(err("dog)").map(|e| (e.start, e.end)), Some((3, 4)));
        assert_eq!(err("\"open").map(|e| (e.start, e.end)), Some((0, 5)));
        assert_eq!(err("dog or").map(|e| (e.start, e.end)), Some((4, 6)));
        assert!(err("is:foo").unwrap().message.contains("Unknown state"));
        assert!(err("flag:9").unwrap().message.contains("0 to 7"));
        assert!(err("prop:ivl>x").unwrap().message.contains("needs a number"));
        assert!(err("prop:foo>1").unwrap().message.contains("Unknown property"));
        assert!(err("rated:0").is_some());
        assert!(err("rated:2:5").is_some());
        assert!(err("nid:abc").is_some());
        assert!(err("()").is_some());
        assert!(err("- dog").is_none()); // a lone "-" followed by space is plain text
        assert!(err("and dog").is_some());
        assert!(err("dog and").is_some());
        assert!(err("-").is_none());
        assert!(err(":x").is_some());
        assert!(err("a\"b\"").is_some());
        assert!(err("prop:cds:v>1").is_some());
        assert!(err("prop:r>2").is_some());
        assert!(err("prop:ivl>1.5").is_some());
        assert!(err("prop:rated=1").is_some());
    }

    #[test]
    fn parses_and_tighter_than_or() {
        assert_eq!(
            parse_search("a b or c").unwrap(),
            SearchNode::Or { children: vec![SearchNode::And { children: vec![text("a"), text("b")] }, text("c")] }
        );
        assert_eq!(parse_search("").unwrap(), SearchNode::All);
        assert_eq!(parse_search("-dog").unwrap(), SearchNode::Not { child: Box::new(text("dog")) });
        assert_eq!(parse_search("(dog)").unwrap(), text("dog"));
    }

    #[test]
    fn parses_every_term_kind() {
        let p = |q: &str| parse_search(q).unwrap();
        assert_eq!(p("\"card:Card 2\""), SearchNode::CardName { name: "Card 2".into() });
        assert_eq!(p("card:\"Card 1\""), SearchNode::CardName { name: "Card 1".into() });
        assert_eq!(p("card:2"), SearchNode::CardOrd { ord: 1 });
        assert_eq!(p("deck:current"), SearchNode::DeckCurrent);
        assert_eq!(p("Deck:French"), SearchNode::Deck { name: "French".into() });
        assert_eq!(p("tag:none"), SearchNode::TagNone);
        assert_eq!(p("tag:re:^geo$"), SearchNode::TagRegex { pattern: "^geo$".into() });
        assert_eq!(p("is:buried-sibling"), SearchNode::State { state: StateKind::BuriedSibling });
        assert_eq!(p("flag:3"), SearchNode::Flag { flag: 3 });
        assert_eq!(
            p("prop:ivl>=10"),
            SearchNode::Prop { prop: PropKind::Ivl, op: CompareOp::Ge, value: PropValue::Num(10.0), key: None }
        );
        assert_eq!(
            p("prop:cdn:n>5"),
            SearchNode::Prop { prop: PropKind::Cdn, op: CompareOp::Gt, value: PropValue::Num(5.0), key: Some("n".into()) }
        );
        assert_eq!(
            p("prop:cds:v!=x"),
            SearchNode::Prop {
                prop: PropKind::Cds,
                op: CompareOp::Ne,
                value: PropValue::Text("x".into()),
                key: Some("v".into())
            }
        );
        assert_eq!(p("rated:7:1"), SearchNode::Rated { days: 7, ease: Some(1) });
        assert_eq!(p("rated:7"), SearchNode::Rated { days: 7, ease: None });
        assert_eq!(p("resched:2"), SearchNode::Resched { days: 2 });
        assert_eq!(p("introduced:30"), SearchNode::Introduced { days: 30 });
        assert_eq!(p("added:1"), SearchNode::Added { days: 1 });
        assert_eq!(p("edited:3"), SearchNode::Edited { days: 3 });
        assert_eq!(p("nid:1,2"), SearchNode::Ids { target: IdTarget::Nid, ids: vec![1, 2] });
        assert_eq!(p("did:11"), SearchNode::Ids { target: IdTarget::Did, ids: vec![11] });
        assert_eq!(p("dupe:100,dog"), SearchNode::Dupe { ntid: 100, text: "dog".into() });
        assert_eq!(p("has-cd:v"), SearchNode::HasCd { key: "v".into() });
        assert_eq!(p("preset:Intense"), SearchNode::Preset { name: "Intense".into() });
        // parse.ts:139 stops a bare term at `(`/`)`, so `re:(?-i)X` is three terms; quote the pattern.
        assert_eq!(p("re:\"(?-i)X\""), SearchNode::Regex { pattern: "(?-i)X".into() });
        assert_eq!(
            p("re:(?-i)X"),
            SearchNode::And { children: vec![SearchNode::Regex { pattern: String::new() }, text("?-i"), text("X")] }
        );
        assert_eq!(p("nc:uber"), SearchNode::Nc { text: "uber".into() });
        assert_eq!(p("w:do*"), SearchNode::Word { text: "do*".into() });
        assert_eq!(p("sc:\"Paris is\""), SearchNode::Sc { text: "Paris is".into() });
        assert_eq!(p("front:re:^c"), SearchNode::Field { field: "front".into(), value: "^c".into(), mode: FieldMode::Regex });
        assert_eq!(p("front:nc:uber"), SearchNode::Field { field: "front".into(), value: "uber".into(), mode: FieldMode::Nc });
        assert_eq!(p("fr*:über"), SearchNode::Field { field: "fr*".into(), value: "über".into(), mode: FieldMode::Glob });
        assert_eq!(p("note:Cloze"), SearchNode::Notetype { name: "Cloze".into() });
        assert_eq!(p("\"is in\""), text("is in"));
        assert_eq!(p("dog OR chat"), SearchNode::Or { children: vec![text("dog"), text("chat")] });
        assert_eq!(p("\"and\""), text("and"));
    }

    #[test]
    fn search_error_converts_to_loci_error() {
        let e: LociError = parse_search("(dog").unwrap_err().into();
        assert_eq!(e.code, crate::error::ErrorCode::Invalid);
        assert_eq!(e.detail.as_deref(), Some("0:4"));
        assert_eq!(js_number(" 12 "), Some(12.0));
        assert_eq!(js_number("0x10"), Some(16.0));
        assert_eq!(js_number("abc"), None);
        assert_eq!(js_number(""), Some(0.0));
    }
}

mod sql_tests {
    //! Port of recall `tests/unit/search/search.test.ts` over an in-memory `Db` with the same fixture.
    use crate::db::Db;
    use crate::search::sql::*;
    use crate::text::field_checksum;
    use crate::text::plain_text;
    use crate::SCHEMA_SQL;
    use rusqlite::types::Value;

    const CRT: i64 = 1_700_000_000 - (1_700_000_000 % 86400) + 4 * 3600; // a day start at 04:00 UTC
    const TODAY: i64 = 100;
    const NEXT_DAY: i64 = CRT + (TODAY + 1) * 86400;
    const NOW_MS: i64 = (NEXT_DAY - 5 * 3600) * 1000;
    const NOW_S: i64 = NOW_MS / 1000;
    const DAY_MS: i64 = 86_400_000;
    const C51: i64 = NOW_MS - 10_000; // card added today

    struct Fixture {
        db: Db,
        ctx: SearchContext,
    }

    fn fixture() -> Fixture {
        let db = Db::memory().unwrap();
        db.execute_batch(SCHEMA_SQL).unwrap();
        let t = NOW_S;
        db.execute_batch(
            "INSERT INTO deck_configs (id, name, config, mtime) VALUES (1, 'Standard', '{}', 0), (2, 'Intense', '{}', 0);",
        )
        .unwrap();
        db.execute_batch("INSERT INTO decks (id, name, config_id, mtime) VALUES (1, 'Default', 1, 0), (10, 'French', 1, 0), (11, 'French::Verbs', 1, 0), (12, 'Spanish', 2, 0), (13, 'Frenchy', 1, 0);").unwrap();
        db.execute_batch("INSERT INTO notetypes (id, name, kind, mtime) VALUES (100, 'Basic (and reversed card)', 0, 0), (200, 'Cloze', 1, 0);").unwrap();
        db.execute_batch("INSERT INTO fields (ntid, ord, name) VALUES (100, 0, 'Front'), (100, 1, 'Back'), (200, 0, 'Text'), (200, 1, 'Back Extra');").unwrap();
        db.execute_batch("INSERT INTO templates (ntid, ord, name, qfmt, afmt) VALUES (100, 0, 'Card 1', '{{Front}}', ''), (100, 1, 'Card 2', '{{Back}}', ''), (200, 0, 'Cloze', '{{cloze:Text}}', '');").unwrap();
        let note = |id: i64, ntid: i64, fields: &[&str], tags: &str, mtime: i64| {
            let plain: Vec<String> = fields.iter().map(|f| plain_text(f)).collect();
            db.run(
                "INSERT INTO notes (id, guid, ntid, mtime, tags, flds, sfld, csum, plain) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    id,
                    format!("g{id}"),
                    ntid,
                    mtime,
                    tags,
                    fields.join("\u{1f}"),
                    fields[0],
                    field_checksum(fields[0]) as i64,
                    plain.join(" ")
                ],
            )
            .unwrap();
        };
        note(1000, 100, &["dog", "chien"], " animal::mammal ", 0);
        note(2000, 100, &["cat<b>s</b>", "chat"], " animal ", 0);
        note(3000, 100, &["über", "over"], "", 0);
        note(4000, 200, &["{{c1::Paris}} is in {{c2::France}}", "extra"], " geo Marked ", 0);
        note(5000, 100, &["a", "b"], " ani::x ", t - 60);
        #[derive(Default)]
        struct O {
            ctype: i64,
            queue: i64,
            due: i64,
            ivl: i64,
            factor: i64,
            reps: i64,
            lapses: i64,
            flags: i64,
            data: Option<&'static str>,
        }
        let card = |id: i64, nid: i64, did: i64, ord: i64, o: O| {
            db.run(
                "INSERT INTO cards (id, nid, did, ord, mtime, type, queue, due, ivl, factor, reps, lapses, flags, data) VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![id, nid, did, ord, o.ctype, o.queue, o.due, o.ivl, o.factor, o.reps, o.lapses, o.flags, o.data.unwrap_or("{}")],
            )
            .unwrap();
        };
        card(11, 1000, 10, 0, O { due: 1, ..O::default() });
        card(
            12,
            1000,
            11,
            1,
            O {
                ctype: 2,
                queue: 2,
                due: TODAY - 1,
                ivl: 30,
                factor: 2500,
                reps: 5,
                lapses: 1,
                flags: 1,
                data: Some(r#"{"s":30,"d":5,"decay":0.1542}"#),
            },
        );
        card(
            21,
            2000,
            12,
            0,
            O {
                ctype: 2,
                queue: 2,
                due: TODAY + 5,
                ivl: 10,
                factor: 2300,
                reps: 2,
                data: Some(r#"{"s":5,"d":7}"#),
                ..O::default()
            },
        );
        card(31, 3000, 1, 0, O { ctype: 1, queue: 1, due: t - 60, ..O::default() });
        card(
            41,
            4000,
            10,
            0,
            O {
                ctype: 2,
                queue: -1,
                due: TODAY + 1,
                ivl: 3,
                data: Some(r#"{"cd":"{\"v\":\"reschedule\",\"n\":7}"}"#),
                ..O::default()
            },
        );
        card(42, 4000, 10, 1, O { ctype: 2, queue: -2, due: TODAY, ivl: 3, ..O::default() });
        card(C51, 5000, 1, 0, O { queue: -3, due: 2, ..O::default() });
        let rev = |id: i64, cid: i64, ease: i64, rtype: i64| {
            db.run(
                "INSERT INTO revlog (id, cid, ease, ivl, last_ivl, factor, time, type) VALUES (?, ?, ?, 0, 0, 0, 1000, ?)",
                rusqlite::params![id, cid, ease, rtype],
            )
            .unwrap();
        };
        rev(NOW_MS - 1000, 12, 3, 1);
        rev(NOW_MS - 3 * DAY_MS, 21, 1, 1);
        rev(NOW_MS - 10 * DAY_MS, 21, 3, 0);
        rev(NOW_MS - 2000, 11, 0, 4);
        let ctx = SearchContext {
            today: TODAY,
            now_ms: NOW_MS,
            crt: CRT,
            rollover_hour: 4,
            current_deck_id: Some(10),
            next_day_at: None,
            fields: Some(load_field_index(&db).unwrap()),
        };
        Fixture { db, ctx }
    }

    impl Fixture {
        fn ids(&self, q: &str) -> Vec<i64> {
            self.ids_mode(q, SearchMode::Cards)
        }
        fn ids_mode(&self, q: &str, mode: SearchMode) -> Vec<i64> {
            let mut v = run_search(&self.db, q, &self.ctx, mode).unwrap();
            v.sort_unstable();
            v
        }
        fn compile(&self, q: &str) -> CompiledSearch {
            compile_search(q, &self.ctx).unwrap()
        }
    }

    const ALL: [i64; 7] = [11, 12, 21, 31, 41, 42, C51];
    fn all_but(x: &[i64]) -> Vec<i64> {
        ALL.iter().copied().filter(|i| !x.contains(i)).collect()
    }
    fn texts(v: &[&str]) -> Vec<Value> {
        v.iter().map(|s| text(*s)).collect()
    }

    #[test]
    fn fts_for_3_plus_chars_like_for_shorter_or_wildcard_terms() {
        let f = fixture();
        let cs = f.compile("dog");
        assert_eq!(cs.sql, "n.id IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?)");
        assert_eq!(cs.params, texts(&["plain : \"dog\""]));
        let cs = f.compile("do");
        assert_eq!(cs.sql, "n.plain LIKE ? ESCAPE '\\'");
        assert_eq!(cs.params, texts(&["%do%"]));
        assert_eq!(f.compile("d*g").params, texts(&["%d%g%"]));
        assert_eq!(f.ids("dog"), [11, 12]);
        assert_eq!(f.ids("DOG"), [11, 12]);
        assert_eq!(f.ids("do"), [11, 12]);
        assert_eq!(f.ids("d*g"), [11, 12]);
        assert_eq!(f.ids("c_at"), [21]);
        assert_eq!(f.ids("ber"), [31]);
    }

    #[test]
    fn quoted_phrases_implicit_and_or_negation_parentheses() {
        let f = fixture();
        assert_eq!(f.ids("\"is in\""), [41, 42]);
        // phrases match across cloze markup (notes.plain holds the cloze text, not {{c1::…}})
        assert_eq!(f.ids("\"Paris is in France\""), [41, 42]);
        assert_eq!(f.ids("c1"), Vec::<i64>::new());
        assert_eq!(f.ids("dog chien"), [11, 12]);
        assert_eq!(f.ids("dog chat"), Vec::<i64>::new());
        assert_eq!(f.ids("dog or chat"), [11, 12, 21]);
        assert_eq!(f.ids("dog OR chat"), [11, 12, 21]);
        assert_eq!(f.ids("-dog"), all_but(&[11, 12]));
        assert_eq!(f.ids("(dog or chat) -card:2"), [11, 21]);
        assert_eq!(f.ids("dog and card:1"), [11]);
        assert_eq!(f.ids(""), ALL);
    }

    #[test]
    fn escapes_literal_wildcards() {
        let f = fixture();
        assert_eq!(f.ids("d\\*g"), Vec::<i64>::new());
    }

    #[test]
    fn exact_field_match_wildcards_empty_and_non_empty() {
        let f = fixture();
        assert_eq!(f.ids("front:dog"), [11, 12]);
        assert_eq!(f.ids("front:do"), Vec::<i64>::new());
        assert_eq!(f.ids("Front:DOG"), [11, 12]);
        assert_eq!(f.ids("front:cat*"), [21]);
        assert_eq!(f.ids("back:_*"), [11, 12, 21, 31, C51]);
        assert_eq!(f.ids("front:"), Vec::<i64>::new());
        assert_eq!(f.ids("text:*Paris*"), [41, 42]);
        assert_eq!(f.ids("fr*:über"), [31]);
        assert_eq!(f.ids("nosuchfield:x"), Vec::<i64>::new());
    }

    #[test]
    fn field_regex_and_nc() {
        let f = fixture();
        assert_eq!(f.ids("front:re:^c"), [21]);
        assert_eq!(f.ids("front:nc:uber"), [31]);
    }

    #[test]
    fn emits_a_superset_in_sql_plus_a_post_filter() {
        let f = fixture();
        let cs = f.compile("front:dog");
        assert_eq!(cs.sql, "((n.ntid IN (?) AND n.flds LIKE ? ESCAPE '\\'))");
        assert_eq!(cs.params, vec![int(100), text("%dog%")]);
        assert!(cs.post_filter.is_some());
    }

    #[test]
    fn deck_with_children_wildcards_exclusion_current() {
        let f = fixture();
        assert_eq!(f.ids("deck:French"), [11, 12, 41, 42]);
        assert_eq!(f.ids("deck:french::verbs"), [12]);
        assert_eq!(f.ids("deck:French -deck:French::*"), [11, 41, 42]);
        assert_eq!(f.ids("deck:Fr*"), [11, 12, 41, 42]);
        assert_eq!(f.ids("deck:current"), [11, 12, 41, 42]);
        assert_eq!(f.ids("deck:*"), ALL);
        assert_eq!(f.compile("deck:French").params, texts(&["French", "French::%", "French", "French::%"]));
    }

    #[test]
    fn note_type_card_template_name_or_number_preset() {
        let f = fixture();
        assert_eq!(f.ids("note:Cloze"), [41, 42]);
        assert_eq!(f.ids("note:basic*"), [11, 12, 21, 31, C51]);
        assert_eq!(f.ids("card:2"), [12, 42]);
        assert_eq!(f.ids("\"card:Card 2\""), [12]);
        assert_eq!(f.ids("card:\"Card 1\""), [11, 21, 31, C51]);
        assert_eq!(f.ids("card:cloze"), [41, 42]);
        assert_eq!(f.ids("preset:Intense"), [21]);
    }

    #[test]
    fn tags_hierarchical_match_wildcards_none_regex() {
        let f = fixture();
        assert_eq!(f.ids("tag:animal"), [11, 12, 21]);
        assert_eq!(f.ids("tag:animal::mammal"), [11, 12]);
        assert_eq!(f.ids("tag:anim"), Vec::<i64>::new());
        assert_eq!(f.ids("tag:ani*"), [11, 12, 21, C51]);
        assert_eq!(f.ids("tag:marked"), [41, 42]);
        assert_eq!(f.ids("tag:none"), [31]);
        assert_eq!(f.ids("tag:re:^geo$"), [41, 42]);
        assert_eq!(f.compile("tag:animal").params, texts(&["% animal %", "% animal::%"]));
    }

    #[test]
    fn is_states() {
        let f = fixture();
        assert_eq!(f.ids("is:due"), [12, 31]);
        assert_eq!(f.ids("is:new"), [11, C51]);
        assert_eq!(f.ids("is:learn"), [31]);
        assert_eq!(f.ids("is:review"), [12, 21, 41, 42]);
        assert_eq!(f.ids("is:suspended"), [41]);
        assert_eq!(f.ids("is:buried"), [42, C51]);
        assert_eq!(f.ids("is:buried-sibling"), [42]);
        assert_eq!(f.ids("is:buried-manually"), [C51]);
    }

    #[test]
    fn flags() {
        let f = fixture();
        assert_eq!(f.ids("flag:1"), [12]);
        assert_eq!(f.ids("flag:0"), all_but(&[12]));
        assert_eq!(f.compile("flag:3").sql, "(c.flags & 7) = ?");
    }

    #[test]
    fn prop_scheduling_properties() {
        let f = fixture();
        assert_eq!(f.ids("prop:ivl>=10"), [12, 21]);
        assert_eq!(f.ids("prop:ivl!=3"), [11, 12, 21, 31, C51]);
        assert_eq!(f.ids("prop:due=-1"), [12]);
        assert_eq!(f.ids("prop:due>=5"), [21]);
        assert_eq!(f.ids("prop:due<=0"), [12, 31]);
        assert_eq!(f.ids("prop:reps>3"), [12]);
        assert_eq!(f.ids("prop:lapses>0"), [12]);
        assert_eq!(f.ids("prop:ease=2.3"), [21]);
        assert_eq!(f.ids("prop:pos<=1"), [11]);
    }

    fn r_of(f: &Fixture, id: i64) -> f64 {
        register_sql_functions(f.db.conn()).unwrap();
        let sql = f.compile("prop:r<1").sql;
        let expr = sql.strip_suffix(" < ?").unwrap();
        f.db.scalar::<f64, _>(&format!("SELECT {expr} AS r FROM cards c WHERE c.id = ?"), [id]).unwrap().unwrap()
    }
    fn retrievability(t: f64, s: f64) -> f64 {
        let decay = 0.1542;
        let factor = 0.9f64.powf(-1.0 / decay) - 1.0;
        (1.0 + factor * t / s).powf(-decay)
    }

    #[test]
    fn prop_fsrs_memory_state_r_from_the_forgetting_curve() {
        let f = fixture();
        assert_eq!(f.ids("prop:s>20"), [12]);
        assert_eq!(f.ids("prop:d>0.5"), [21]);
        // card 21: t = S = 5 days → R = 0.9 exactly (default decay); card 12: t=31, S=30 → R ≈ 0.8977
        assert_eq!(f.ids("prop:r>0.8999 prop:r<0.9001"), [21]);
        assert_eq!(f.ids("prop:r<0.9"), [12]);
        assert!((r_of(&f, 12) - retrievability(31.0, 30.0)).abs() < 1e-10);
    }

    #[test]
    fn prop_r_covers_cards_with_a_memory_state_in_any_queue() {
        let f = fixture();
        f.db.run("INSERT INTO cards (id, nid, did, ord, mtime, type, queue, due, ivl, data) VALUES (61, 1000, 10, 5, 0, 3, 1, ?, 2, '{\"s\":1,\"d\":6}')", [NOW_S + 600]).unwrap();
        f.db.run(
            "INSERT INTO revlog (id, cid, ease, ivl, last_ivl, factor, time, type) VALUES (?, 61, 1, -600, 2, 0, 1000, 1)",
            [NOW_MS - 4 * DAY_MS],
        )
        .unwrap();
        f.db.run(
            "INSERT INTO cards (id, nid, did, ord, mtime, type, queue, due, ivl, data) VALUES (62, 1000, 10, 6, 0, 1, 1, ?, 0, ?)",
            rusqlite::params![NOW_S + 60, format!("{{\"s\":2,\"d\":5,\"lrt\":{}}}", NOW_S - 2 * 86400)],
        )
        .unwrap();
        f.db.run("INSERT INTO cards (id, nid, did, ord, mtime, type, queue, due, ivl, data) VALUES (63, 1000, 10, 7, 0, 2, -1, ?, 10, '{\"s\":4,\"d\":5}')", [TODAY - 10]).unwrap();
        // relearning (no lrt): 4 days since the lapse in the revlog; learning: 2 days from lrt; suspended review: 20 days
        assert!((r_of(&f, 61) - retrievability(4.0, 1.0)).abs() < 1e-6); // ≈ 0.78
        assert!((r_of(&f, 62) - retrievability(2.0, 2.0)).abs() < 1e-6); // = 0.90
        assert!((r_of(&f, 63) - retrievability(20.0, 4.0)).abs() < 1e-6); // ≈ 0.76
        let over60 = |q: &str| f.ids(q).into_iter().filter(|x| *x > 60).collect::<Vec<_>>();
        assert_eq!(over60("prop:r<0.8"), [61, 63]);
        assert_eq!(over60("prop:r>0.85"), [62]);
        assert_eq!(over60("prop:r>0.77 prop:r<0.8"), [61]);
    }

    #[test]
    fn prop_custom_data() {
        let f = fixture();
        assert_eq!(f.ids("has-cd:v"), [41]);
        assert_eq!(f.ids("has-cd:zz"), Vec::<i64>::new());
        assert_eq!(f.ids("prop:cdn:n>5"), [41]);
        assert_eq!(f.ids("prop:cds:v=reschedule"), [41]);
        assert_eq!(f.ids("prop:cds:v!=reschedule"), all_but(&[41]));
    }

    #[test]
    fn rated_resched_introduced_prop_rated() {
        let f = fixture();
        assert_eq!(f.ids("rated:1"), [12]);
        assert_eq!(f.ids("rated:1:3"), [12]);
        assert_eq!(f.ids("rated:1:1"), Vec::<i64>::new());
        assert_eq!(f.ids("rated:7:1"), [21]);
        assert_eq!(f.ids("rated:7"), [12, 21]);
        assert_eq!(f.ids("rated:30"), [12, 21]);
        assert_eq!(f.ids("resched:1"), [11]);
        assert_eq!(f.ids("prop:rated=0"), [12]);
        assert_eq!(f.ids("prop:rated=-3"), [21]);
        assert_eq!(f.ids("prop:resched=0"), [11]);
        assert_eq!(f.ids("introduced:5"), [12]);
        assert_eq!(f.ids("introduced:30"), [12, 21]);
    }

    #[test]
    fn added_and_edited() {
        let f = fixture();
        assert_eq!(f.ids("added:1"), [C51]);
        assert_eq!(f.ids("edited:1"), [C51]);
    }

    #[test]
    fn nid_cid_mid_did() {
        let f = fixture();
        assert_eq!(f.ids("nid:1000,2000"), [11, 12, 21]);
        assert_eq!(f.ids("cid:11,41"), [11, 41]);
        assert_eq!(f.ids("mid:200"), [41, 42]);
        assert_eq!(f.ids("did:11"), [12]);
    }

    #[test]
    fn dupe() {
        let f = fixture();
        assert_eq!(f.ids("dupe:100,dog"), [11, 12]);
        assert_eq!(f.ids("dupe:200,dog"), Vec::<i64>::new());
    }

    #[test]
    fn post_filtered_operators_re_nc_w_sc() {
        let f = fixture();
        assert_eq!(f.ids("re:c.t"), [21]);
        assert_eq!(f.ids("re:(?-i)PARIS"), Vec::<i64>::new());
        assert_eq!(f.ids("re:PARIS"), [41, 42]);
        assert_eq!(f.ids("nc:uber"), [31]);
        assert_eq!(f.ids("w:is"), [41, 42]);
        assert_eq!(f.ids("w:do"), Vec::<i64>::new());
        assert_eq!(f.ids("w:do*"), [11, 12]);
        assert_eq!(f.ids("sc:\"Paris is\""), [41, 42]);
    }

    #[test]
    fn mixed_with_or_and_negation_stays_exact() {
        let f = fixture();
        assert_eq!(f.ids("dog or re:c.t"), [11, 12, 21]);
        assert_eq!(f.ids("-re:c.t"), all_but(&[21]));
        assert_eq!(f.ids("(tag:animal -front:dog) or is:suspended"), [21, 41]);
        assert_eq!(f.ids("-(front:dog or is:new)"), [21, 31, 41, 42]);
        let cs = f.compile("deck:French (front:dog or is:suspended)");
        assert!(cs.extra_columns.as_deref().unwrap().contains("AS l0"));
        let (_, params) = build_search_select(&cs, "c.id");
        assert_eq!(params[..cs.extra_params.len()], cs.extra_params[..]);
    }

    #[test]
    fn notes_mode_returns_distinct_note_ids() {
        let f = fixture();
        assert_eq!(f.ids_mode("deck:French", SearchMode::Notes), [1000, 4000]);
        assert_eq!(f.ids_mode("re:c.t or note:cloze", SearchMode::Notes), [2000, 4000]);
    }

    #[test]
    fn invalid_regex_and_missing_field_index_are_handled() {
        let f = fixture();
        assert!(compile_search("re:(", &f.ctx).is_err());
        let no_fields = SearchContext { fields: None, ..f.ctx.clone() };
        let mut v = run_search(&f.db, "front:dog", &no_fields, SearchMode::Cards).unwrap();
        v.sort_unstable();
        assert_eq!(v, [11, 12]);
        assert_eq!(strip_cloze_markers("{{c1::a::h}} {{c2::b {{c3::c}}}}"), "a b c");
    }
}
