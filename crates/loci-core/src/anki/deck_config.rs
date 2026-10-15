//! Anki deck options (`dconf`) ⇄ Loci `DeckConfig`.
//!
//! Mapping table (schema-11 JSON key → DeckConfig field; schema-18 protobuf is first converted to
//! the same JSON by `deck_config_proto_to_json`):
//!
//! | Anki dconf JSON                        | DeckConfig                    | notes |
//! |----------------------------------------|-------------------------------|-------|
//! | collection `fsrs`                      | scheduler                     | true → Fsrs, else Sm2 (Anki's switch is global) |
//! | new.perDay                             | new_per_day                   | |
//! | rev.perDay                             | reviews_per_day               | |
//! | —                                      | minutes_per_day               | Loci-only, Standard default 20 |
//! | new.delays (minutes)                   | learn_steps                   | |
//! | lapse.delays (minutes)                 | relearn_steps                 | |
//! | new.ints[0] / new.ints[1]              | graduating_interval / easy_interval | days |
//! | new.order (0 random, 1 due)            | new_order                     | |
//! | newGatherPriority 0,5,1,2,3,4          | new_gather deck, deck-random-notes, ascending, descending, random-notes, random-cards | |
//! | newSortOrder 0,1,2,3,4                 | new_sort template-then-order, order, template-then-random, random-note, random-card | |
//! | reviewOrder 0,1,2,3,4,5,6,12,8,9,10    | review_order due-then-random, due-then-deck, deck-then-due, ascending-intervals, descending-intervals, ascending-ease, descending-ease, relative-overdue, random, added, reverse-added | 7 (retrievability ascending) → relative-overdue, 11 (retrievability descending) → due-then-random, with a warning |
//! | newMix / interdayLearningMix 0,1,2     | new_review_mix / interday_learning_mix mix, review-first, new-first | |
//! | new.initialFactor (permille)           | initial_ease (permille)       | |
//! | rev.ease4                              | easy_bonus                    | |
//! | rev.ivlFct                             | interval_modifier             | |
//! | rev.hardFactor                         | hard_multiplier               | |
//! | lapse.mult                             | lapse_multiplier              | |
//! | lapse.minInt                           | minimum_lapse_interval        | days |
//! | rev.maxIvl                             | maximum_interval              | days |
//! | fsrsParams6 ‖ fsrsParams5 ‖ fsrsWeights| fsrs_params                   | first non-empty, Anki's 6→5→4 order; kept verbatim (17/19 values are upgraded by the scheduler); empty → FSRS-6 defaults |
//! | desiredRetention                       | desired_retention             | |
//! | sm2Retention                           | historical_retention          | |
//! | —                                      | auto_optimise, optimise_every_reviews | Loci-only, Standard defaults (true, 400) |
//! | lapse.leechFails                       | leech_threshold               | |
//! | lapse.leechAction (0 suspend, 1 tag)   | leech_action                  | |
//! | new.bury / rev.bury / buryInterdayLearning | bury_siblings_new / _review / _interday | |
//! | autoplay                               | autoplay_audio                | |
//! | replayq                                | replay_question_audio         | |
//! | maxTaken (s)                           | max_answer_seconds            | |
//! | timer (0/1)                            | show_timer                    | |
//! | collection `estTimes`                  | show_intervals_on_buttons     | Anki's is collection-wide |
//!
//! Missing or malformed Anki keys fall back to Anki's own defaults (what the user effectively had);
//! Loci-only fields use the Loci "Standard" preset (`collection::presets::standard_deck_config`).

use super::image::Json;
use super::proto::DeckConfigProto;
use crate::collection::presets::standard_deck_config;
use crate::types::*;
use serde_json::Value;

/// FSRS-6 default parameters (fsrs-rs v6.6.2, research/fsrs-spec.md §2.1).
pub use crate::collection::presets::FSRS6_DEFAULT_PARAMS;

// ── enum tables (Anki integer ⇄ Loci) ──
fn gather(n: i64) -> Option<NewCardGather> {
    Some(match n {
        0 => NewCardGather::Deck,
        5 => NewCardGather::DeckRandomNotes,
        1 => NewCardGather::Ascending,
        2 => NewCardGather::Descending,
        3 => NewCardGather::RandomNotes,
        4 => NewCardGather::RandomCards,
        _ => return None,
    })
}
fn gather_to_anki(g: NewCardGather) -> i64 {
    match g {
        NewCardGather::Deck => 0,
        NewCardGather::DeckRandomNotes => 5,
        NewCardGather::Ascending => 1,
        NewCardGather::Descending => 2,
        NewCardGather::RandomNotes => 3,
        NewCardGather::RandomCards => 4,
    }
}
fn sort(n: i64) -> Option<NewCardSort> {
    Some(match n {
        0 => NewCardSort::TemplateThenOrder,
        1 => NewCardSort::Order,
        2 => NewCardSort::TemplateThenRandom,
        3 => NewCardSort::RandomNote,
        4 => NewCardSort::RandomCard,
        _ => return None,
    })
}
fn sort_to_anki(s: NewCardSort) -> i64 {
    match s {
        NewCardSort::TemplateThenOrder => 0,
        NewCardSort::Order => 1,
        NewCardSort::TemplateThenRandom => 2,
        NewCardSort::RandomNote => 3,
        NewCardSort::RandomCard => 4,
    }
}
fn review_order(n: i64) -> Option<ReviewOrder> {
    Some(match n {
        0 => ReviewOrder::DueThenRandom,
        1 => ReviewOrder::DueThenDeck,
        2 => ReviewOrder::DeckThenDue,
        3 => ReviewOrder::AscendingIntervals,
        4 => ReviewOrder::DescendingIntervals,
        5 => ReviewOrder::AscendingEase,
        6 => ReviewOrder::DescendingEase,
        7 => ReviewOrder::RelativeOverdue,
        11 => ReviewOrder::DueThenRandom,
        12 => ReviewOrder::RelativeOverdue,
        8 => ReviewOrder::Random,
        9 => ReviewOrder::Added,
        10 => ReviewOrder::ReverseAdded,
        _ => return None,
    })
}
fn review_order_to_anki(r: ReviewOrder) -> i64 {
    match r {
        ReviewOrder::DueThenRandom => 0,
        ReviewOrder::DueThenDeck => 1,
        ReviewOrder::DeckThenDue => 2,
        ReviewOrder::AscendingIntervals => 3,
        ReviewOrder::DescendingIntervals => 4,
        ReviewOrder::AscendingEase => 5,
        ReviewOrder::DescendingEase => 6,
        ReviewOrder::RelativeOverdue => 12,
        ReviewOrder::Random => 8,
        ReviewOrder::Added => 9,
        ReviewOrder::ReverseAdded => 10,
    }
}
fn mix(n: i64) -> Option<NewReviewMix> {
    Some(match n {
        0 => NewReviewMix::Mix,
        1 => NewReviewMix::ReviewFirst,
        2 => NewReviewMix::NewFirst,
        _ => return None,
    })
}
fn mix_to_anki(m: NewReviewMix) -> i64 {
    match m {
        NewReviewMix::Mix => 0,
        NewReviewMix::ReviewFirst => 1,
        NewReviewMix::NewFirst => 2,
    }
}

// ── JSON accessors (tolerant: Anki uses default_on_invalid for most keys) ──

/// `v && typeof v === 'object' && !Array.isArray(v) ? v : {}`
pub(crate) fn obj(v: Option<&Value>) -> Json {
    match v {
        Some(Value::Object(m)) => m.clone(),
        _ => Json::new(),
    }
}

/// `num`: finite number, boolean (1/0) or numeric string; else the default.
pub(crate) fn num(v: Option<&Value>, d: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()).unwrap_or(d),
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).unwrap_or(d),
        _ => d,
    }
}

pub(crate) fn int(v: Option<&Value>, d: i64) -> i64 {
    let f = num(v, d as f64);
    if f.is_finite() {
        f as i64
    } else {
        d
    }
}

/// `bool`: boolean, or number ≠ 0; else the default.
pub(crate) fn bool_or(v: Option<&Value>, d: bool) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        _ => d,
    }
}

/// `nums`: an array of finite numbers, else None.
pub(crate) fn nums(v: Option<&Value>) -> Option<Vec<f64>> {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| x.as_f64().filter(|f| f.is_finite())).collect(),
        _ => None,
    }
}

/// A JSON number written the way `JSON.stringify` writes a JS number: integral values without a
/// fractional part.
pub(crate) fn json_num(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        Value::from(x)
    }
}

fn json_nums(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|x| json_num(*x)).collect())
}

/// Maps one schema-11 dconf object to a DeckConfig.
/// `fsrs_enabled`: collection-wide `fsrs` flag; `est_times`: collection-wide `estTimes` flag.
pub fn dconf_to_deck_config(dconf: &Json, fsrs_enabled: bool, est_times: bool, warnings: &mut Vec<String>) -> DeckConfig {
    let n = obj(dconf.get("new"));
    let r = obj(dconf.get("rev"));
    let l = obj(dconf.get("lapse"));
    let ints = nums(n.get("ints")).unwrap_or_else(|| vec![1.0, 4.0, 0.0]);
    let name = match dconf.get("name") {
        Some(Value::String(s)) => s.clone(),
        _ => dconf.get("id").map(|v| v.to_string()).unwrap_or_else(|| "?".to_string()),
    };
    let review_order_num = int(dconf.get("reviewOrder"), 0);
    if review_order_num == 7 || review_order_num == 11 {
        let direction = if review_order_num == 7 { "ascending" } else { "descending" };
        let used = if review_order_num == 7 { "relative-overdue" } else { "due-then-random" };
        warnings.push(format!(
            "Preset \"{name}\": review order \"retrievability {direction}\" is not available in Loci; using \"{used}\"."
        ));
    }
    let p6 = nums(dconf.get("fsrsParams6")).unwrap_or_default();
    let p5 = nums(dconf.get("fsrsParams5")).unwrap_or_default();
    let p4 = nums(dconf.get("fsrsWeights")).unwrap_or_default();
    let params = if !p6.is_empty() {
        p6
    } else if !p5.is_empty() {
        p5
    } else if !p4.is_empty() {
        p4
    } else {
        FSRS6_DEFAULT_PARAMS.to_vec()
    };
    let standard = standard_deck_config();
    DeckConfig {
        scheduler: if fsrs_enabled { SchedulerKind::Fsrs } else { SchedulerKind::Sm2 },
        new_per_day: int(n.get("perDay"), 20),
        reviews_per_day: int(r.get("perDay"), 200),
        minutes_per_day: standard.minutes_per_day,
        learn_steps: nums(n.get("delays")).unwrap_or_else(|| vec![1.0, 10.0]),
        relearn_steps: nums(l.get("delays")).unwrap_or_else(|| vec![10.0]),
        graduating_interval: ints.first().map(|x| *x as i64).unwrap_or(1),
        easy_interval: ints.get(1).map(|x| *x as i64).unwrap_or(4),
        new_order: if int(n.get("order"), 1) == 0 { NewCardOrder::Random } else { NewCardOrder::Due },
        new_gather: gather(int(dconf.get("newGatherPriority"), 0)).unwrap_or(NewCardGather::Deck),
        new_sort: sort(int(dconf.get("newSortOrder"), 0)).unwrap_or(NewCardSort::TemplateThenOrder),
        review_order: review_order(review_order_num).unwrap_or(ReviewOrder::DueThenRandom),
        new_review_mix: mix(int(dconf.get("newMix"), 0)).unwrap_or(NewReviewMix::Mix),
        interday_learning_mix: mix(int(dconf.get("interdayLearningMix"), 0)).unwrap_or(NewReviewMix::Mix),
        initial_ease: int(n.get("initialFactor"), 2500),
        easy_bonus: num(r.get("ease4"), 1.3),
        interval_modifier: num(r.get("ivlFct"), 1.0),
        hard_multiplier: num(r.get("hardFactor"), 1.2),
        lapse_multiplier: num(l.get("mult"), 0.0),
        minimum_lapse_interval: int(l.get("minInt"), 1),
        maximum_interval: int(r.get("maxIvl"), 36500),
        fsrs_params: params,
        desired_retention: num(dconf.get("desiredRetention"), 0.9),
        historical_retention: num(dconf.get("sm2Retention"), 0.9),
        auto_optimise: standard.auto_optimise,
        optimise_every_reviews: standard.optimise_every_reviews,
        leech_threshold: int(l.get("leechFails"), 8),
        leech_action: if int(l.get("leechAction"), 1) == 0 { LeechAction::Suspend } else { LeechAction::Tag },
        bury_siblings_new: bool_or(n.get("bury"), false),
        bury_siblings_review: bool_or(r.get("bury"), false),
        bury_siblings_interday: bool_or(dconf.get("buryInterdayLearning"), false),
        autoplay_audio: bool_or(dconf.get("autoplay"), true),
        replay_question_audio: bool_or(dconf.get("replayq"), true),
        max_answer_seconds: int(dconf.get("maxTaken"), 60),
        show_timer: bool_or(dconf.get("timer"), false),
        show_intervals_on_buttons: est_times,
    }
}

fn set(j: &mut Json, key: &str, v: impl Into<Value>) {
    j.insert(key.to_string(), v.into());
}

/// Rebuilds the schema-11 dconf JSON Anki would produce from a schema-18 DeckConfig.Config blob.
pub fn deck_config_proto_to_json(id: i64, name: &str, mtime: i64, usn: i64, c: &DeckConfigProto) -> Json {
    let mut j = c.other.clone();
    set(&mut j, "id", id);
    set(&mut j, "name", name);
    set(&mut j, "mod", mtime);
    set(&mut j, "usn", usn);
    set(&mut j, "maxTaken", c.cap_answer_time_to_secs);
    set(&mut j, "autoplay", !c.disable_autoplay);
    set(&mut j, "timer", if c.show_timer { 1 } else { 0 });
    set(&mut j, "replayq", !c.skip_question_when_replaying_answer);
    set(&mut j, "dyn", false);
    let mut new = obj(c.other.get("new"));
    set(&mut new, "delays", json_nums(&c.learn_steps));
    set(&mut new, "ints", Value::Array(vec![c.graduating_interval_good.into(), c.graduating_interval_easy.into(), 0.into()]));
    set(&mut new, "initialFactor", (c.initial_ease * 1000.0).round() as i64);
    set(&mut new, "order", if c.new_card_insert_order == 1 { 0 } else { 1 });
    set(&mut new, "perDay", c.new_per_day);
    set(&mut new, "bury", c.bury_new);
    set(&mut j, "new", Value::Object(new));
    let mut rev = obj(c.other.get("rev"));
    set(&mut rev, "perDay", c.reviews_per_day);
    set(&mut rev, "ease4", json_num(c.easy_multiplier));
    set(&mut rev, "ivlFct", json_num(c.interval_multiplier));
    set(&mut rev, "maxIvl", c.maximum_review_interval);
    set(&mut rev, "hardFactor", json_num(c.hard_multiplier));
    set(&mut rev, "bury", c.bury_reviews);
    set(&mut j, "rev", Value::Object(rev));
    let mut lapse = obj(c.other.get("lapse"));
    set(&mut lapse, "delays", json_nums(&c.relearn_steps));
    set(&mut lapse, "mult", json_num(c.lapse_multiplier));
    set(&mut lapse, "minInt", c.minimum_lapse_interval);
    set(&mut lapse, "leechFails", c.leech_threshold);
    set(&mut lapse, "leechAction", c.leech_action);
    set(&mut j, "lapse", Value::Object(lapse));
    set(&mut j, "newMix", c.new_mix);
    set(&mut j, "newPerDayMinimum", c.new_per_day_minimum);
    set(&mut j, "interdayLearningMix", c.interday_learning_mix);
    set(&mut j, "reviewOrder", c.review_order);
    set(&mut j, "newSortOrder", c.new_card_sort_order);
    set(&mut j, "newGatherPriority", c.new_card_gather_priority);
    set(&mut j, "buryInterdayLearning", c.bury_interday_learning);
    set(&mut j, "fsrsWeights", json_nums(&c.fsrs_params_4));
    set(&mut j, "fsrsParams5", json_nums(&c.fsrs_params_5));
    set(&mut j, "fsrsParams6", json_nums(&c.fsrs_params_6));
    set(&mut j, "desiredRetention", json_num(c.desired_retention));
    set(&mut j, "sm2Retention", json_num(c.historical_retention));
    set(&mut j, "weightSearch", c.param_search.as_str());
    set(&mut j, "ignoreRevlogsBeforeDate", c.ignore_revlogs_before_date.as_str());
    set(&mut j, "easyDaysPercentages", json_nums(&c.easy_days_percentages));
    set(&mut j, "stopTimerOnAnswer", c.stop_timer_on_answer);
    set(&mut j, "secondsToShowQuestion", json_num(c.seconds_to_show_question));
    set(&mut j, "secondsToShowAnswer", json_num(c.seconds_to_show_answer));
    set(&mut j, "questionAction", c.question_action);
    set(&mut j, "answerAction", c.answer_action);
    set(&mut j, "waitForAudio", c.wait_for_audio);
    j
}

/// Builds the schema-11 dconf JSON for a Loci preset (research/anki-format.md §6.4, §9 step 5).
/// Keys Loci does not model are taken from `original` when present, else Anki's defaults.
pub fn deck_config_to_dconf(id: i64, name: &str, mtime: i64, usn: i64, c: &DeckConfig, original: Option<&Json>) -> Json {
    let o = original.cloned().unwrap_or_default();
    let on = obj(o.get("new"));
    let or = obj(o.get("rev"));
    let ol = obj(o.get("lapse"));
    let params = &c.fsrs_params;
    // Anki reads the first non-empty of fsrsParams6 → fsrsParams5 → fsrsWeights: write ours under the
    // key matching its length and clear the keys that would take priority over it.
    let p6: Vec<f64> = match params.len() {
        21 => params.clone(),
        19 | 17 => Vec::new(),
        _ => nums(o.get("fsrsParams6")).unwrap_or_default(),
    };
    let p5: Vec<f64> = match params.len() {
        19 => params.clone(),
        17 => Vec::new(),
        _ => nums(o.get("fsrsParams5")).unwrap_or_default(),
    };
    let p4: Vec<f64> = if params.len() == 17 { params.clone() } else { nums(o.get("fsrsWeights")).unwrap_or_default() };

    let mut j = o.clone();
    set(&mut j, "id", id);
    set(&mut j, "name", name);
    set(&mut j, "mod", mtime);
    set(&mut j, "usn", usn);
    set(&mut j, "maxTaken", c.max_answer_seconds);
    set(&mut j, "autoplay", c.autoplay_audio);
    set(&mut j, "timer", if c.show_timer { 1 } else { 0 });
    set(&mut j, "replayq", c.replay_question_audio);
    set(&mut j, "dyn", false);
    let mut new = Json::new();
    set(&mut new, "separate", true);
    new.extend(on);
    set(&mut new, "delays", json_nums(&c.learn_steps));
    set(&mut new, "ints", Value::Array(vec![c.graduating_interval.into(), c.easy_interval.into(), 0.into()]));
    set(&mut new, "initialFactor", c.initial_ease);
    set(&mut new, "order", if c.new_order == NewCardOrder::Random { 0 } else { 1 });
    set(&mut new, "perDay", c.new_per_day);
    set(&mut new, "bury", c.bury_siblings_new);
    set(&mut j, "new", Value::Object(new));
    let mut rev = Json::new();
    set(&mut rev, "fuzz", 0.05);
    set(&mut rev, "minSpace", 1);
    rev.extend(or);
    set(&mut rev, "perDay", c.reviews_per_day);
    set(&mut rev, "ease4", json_num(c.easy_bonus));
    set(&mut rev, "ivlFct", json_num(c.interval_modifier));
    set(&mut rev, "maxIvl", c.maximum_interval);
    set(&mut rev, "hardFactor", json_num(c.hard_multiplier));
    set(&mut rev, "bury", c.bury_siblings_review);
    set(&mut j, "rev", Value::Object(rev));
    let mut lapse = ol;
    set(&mut lapse, "delays", json_nums(&c.relearn_steps));
    set(&mut lapse, "mult", json_num(c.lapse_multiplier));
    set(&mut lapse, "minInt", c.minimum_lapse_interval);
    set(&mut lapse, "leechFails", c.leech_threshold);
    set(&mut lapse, "leechAction", if c.leech_action == LeechAction::Suspend { 0 } else { 1 });
    set(&mut j, "lapse", Value::Object(lapse));
    set(&mut j, "newMix", mix_to_anki(c.new_review_mix));
    set(&mut j, "newPerDayMinimum", int(o.get("newPerDayMinimum"), 0));
    set(&mut j, "interdayLearningMix", mix_to_anki(c.interday_learning_mix));
    set(&mut j, "reviewOrder", review_order_to_anki(c.review_order));
    set(&mut j, "newSortOrder", sort_to_anki(c.new_sort));
    set(&mut j, "newGatherPriority", gather_to_anki(c.new_gather));
    set(&mut j, "buryInterdayLearning", c.bury_siblings_interday);
    set(&mut j, "fsrsWeights", json_nums(&p4));
    set(&mut j, "fsrsParams5", json_nums(&p5));
    set(&mut j, "fsrsParams6", json_nums(&p6));
    set(&mut j, "desiredRetention", json_num(c.desired_retention));
    set(&mut j, "sm2Retention", json_num(c.historical_retention));
    set(&mut j, "weightSearch", o.get("weightSearch").and_then(Value::as_str).unwrap_or(""));
    set(&mut j, "ignoreRevlogsBeforeDate", o.get("ignoreRevlogsBeforeDate").and_then(Value::as_str).unwrap_or(""));
    set(&mut j, "easyDaysPercentages", json_nums(&nums(o.get("easyDaysPercentages")).unwrap_or_else(|| vec![1.0; 7])));
    set(&mut j, "stopTimerOnAnswer", bool_or(o.get("stopTimerOnAnswer"), false));
    set(&mut j, "secondsToShowQuestion", json_num(num(o.get("secondsToShowQuestion"), 0.0)));
    set(&mut j, "secondsToShowAnswer", json_num(num(o.get("secondsToShowAnswer"), 0.0)));
    set(&mut j, "questionAction", int(o.get("questionAction"), 0));
    set(&mut j, "answerAction", int(o.get("answerAction"), 0));
    set(&mut j, "waitForAudio", bool_or(o.get("waitForAudio"), true));
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn as_map(v: Value) -> Json {
        match v {
            Value::Object(m) => m,
            _ => panic!("object"),
        }
    }

    #[test]
    fn fills_anki_defaults_for_missing_keys_and_loci_defaults_for_loci_only_fields() {
        let mut w = Vec::new();
        let c = dconf_to_deck_config(&Json::new(), false, false, &mut w);
        assert_eq!(c.scheduler, SchedulerKind::Sm2);
        assert_eq!((c.new_per_day, c.reviews_per_day), (20, 200));
        assert_eq!(c.learn_steps, vec![1.0, 10.0]);
        assert_eq!(c.relearn_steps, vec![10.0]);
        assert_eq!((c.graduating_interval, c.easy_interval), (1, 4));
        assert_eq!(c.initial_ease, 2500);
        assert_eq!((c.easy_bonus, c.interval_modifier, c.hard_multiplier, c.lapse_multiplier), (1.3, 1.0, 1.2, 0.0));
        assert_eq!((c.minimum_lapse_interval, c.maximum_interval), (1, 36500));
        assert_eq!(c.desired_retention, 0.9);
        assert_eq!((c.leech_threshold, c.leech_action), (8, LeechAction::Tag));
        assert_eq!(c.new_order, NewCardOrder::Due);
        assert!(!c.show_intervals_on_buttons);
        assert_eq!(c.minutes_per_day, standard_deck_config().minutes_per_day);
        assert!(c.auto_optimise);
        assert_eq!(c.optimise_every_reviews, 400);
        assert_eq!(c.fsrs_params, FSRS6_DEFAULT_PARAMS.to_vec());
        assert!(w.is_empty());
    }

    #[test]
    fn maps_legacy_blocks_enums_and_fsrs_fallbacks() {
        let w4 = vec![0.4, 0.6, 2.4, 5.8, 4.93, 0.94, 0.86, 0.01, 1.49, 0.14, 0.94, 2.18, 0.05, 0.34, 1.26, 0.29, 2.61];
        let dconf = as_map(json!({
            "name": "Old",
            "new": { "delays": [2, 20, 120], "ints": [2, 5, 0], "initialFactor": 2300, "order": 0, "perDay": 9, "bury": true },
            "rev": { "perDay": 150, "ease4": 1.4, "ivlFct": 0.9, "maxIvl": 400, "hardFactor": 1.1, "bury": true },
            "lapse": { "delays": [], "mult": 0.2, "minInt": 2, "leechFails": 5, "leechAction": 0 },
            "maxTaken": 30, "autoplay": false, "replayq": false, "timer": 1, "newGatherPriority": 4, "newSortOrder": 3,
            "reviewOrder": 7, "newMix": 2, "interdayLearningMix": 1, "buryInterdayLearning": true,
            "fsrsWeights": w4, "fsrsParams5": [], "fsrsParams6": [], "desiredRetention": 0.85, "sm2Retention": 0.8,
        }));
        let mut warnings = Vec::new();
        let c = dconf_to_deck_config(&dconf, true, true, &mut warnings);
        assert_eq!(c.scheduler, SchedulerKind::Fsrs);
        assert_eq!(c.learn_steps, vec![2.0, 20.0, 120.0]);
        assert!(c.relearn_steps.is_empty());
        assert_eq!((c.graduating_interval, c.easy_interval, c.initial_ease), (2, 5, 2300));
        assert_eq!(c.new_order, NewCardOrder::Random);
        assert_eq!(c.new_per_day, 9);
        assert!(c.bury_siblings_new && c.bury_siblings_review && c.bury_siblings_interday);
        assert_eq!(
            (c.reviews_per_day, c.easy_bonus, c.interval_modifier, c.maximum_interval, c.hard_multiplier),
            (150, 1.4, 0.9, 400, 1.1)
        );
        assert_eq!(
            (c.lapse_multiplier, c.minimum_lapse_interval, c.leech_threshold, c.leech_action),
            (0.2, 2, 5, LeechAction::Suspend)
        );
        assert_eq!(c.max_answer_seconds, 30);
        assert!(!c.autoplay_audio && !c.replay_question_audio && c.show_timer);
        assert_eq!(c.new_gather, NewCardGather::RandomCards);
        assert_eq!(c.new_sort, NewCardSort::RandomNote);
        assert_eq!(c.review_order, ReviewOrder::RelativeOverdue);
        assert_eq!(c.new_review_mix, NewReviewMix::NewFirst);
        assert_eq!(c.interday_learning_mix, NewReviewMix::ReviewFirst);
        assert_eq!(c.fsrs_params, w4);
        assert_eq!((c.desired_retention, c.historical_retention), (0.85, 0.8));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("retrievability ascending"));
    }

    #[test]
    fn dconf_round_trips_through_deck_config() {
        let mut w = Vec::new();
        let mut cfg = standard_deck_config();
        cfg.fsrs_params = vec![0.5; 19];
        cfg.review_order = ReviewOrder::RelativeOverdue;
        let j = deck_config_to_dconf(7, "P", 1, 2, &cfg, None);
        assert_eq!(j["fsrsParams5"].as_array().unwrap().len(), 19);
        assert_eq!(j["fsrsParams6"], json!([]));
        assert_eq!(j["reviewOrder"], json!(12));
        assert_eq!(j["new"]["separate"], json!(true));
        assert_eq!(j["rev"]["fuzz"], json!(0.05));
        assert_eq!(j["easyDaysPercentages"], json!([1, 1, 1, 1, 1, 1, 1]));
        let back = dconf_to_deck_config(&j, true, true, &mut w);
        assert_eq!(back, cfg);
        // keys Loci does not model survive from the original
        let orig = as_map(json!({ "weightSearch": "deck:x", "new": { "custom": 1 }, "unknownTop": [1] }));
        let j2 = deck_config_to_dconf(7, "P", 1, 2, &cfg, Some(&orig));
        assert_eq!(j2["weightSearch"], json!("deck:x"));
        assert_eq!(j2["new"]["custom"], json!(1));
        assert_eq!(j2["unknownTop"], json!([1]));
    }

    #[test]
    fn json_numbers_print_like_javascript() {
        assert_eq!(json_num(1.0).to_string(), "1");
        assert_eq!(json_num(1.3).to_string(), "1.3");
        assert_eq!(json_num(-2.0).to_string(), "-2");
    }
}
