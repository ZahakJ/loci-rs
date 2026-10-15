//! Statistics (internals §10), computed with SQL over `revlog` and `cards`.
//!
//! Scope: a deck and its descendants (revlog rows of cards currently in those decks), or the
//! whole collection (every revlog row, deleted cards included). Days are collection days
//! (rollover-aligned); "today" counts answers since the current day started.
//!
//! Definitions:
//!   - true retention: passed (ease > 1) / all graded answers of revlog type Review (1) in the
//!     period; learning, relearning, filtered and manual rows are excluded (Anki's definition);
//!   - forecast: `due` = review cards (queue Review) scheduled on each of the next 90 days, overdue
//!     ones on day 0, before daily limits; day 0 is therefore the same "due today" number as the
//!     backlog report (`collection::backlog::due_review_rows`). Learning cards are not reviews and
//!     are left out. `with_new` adds the extra load the queue simulation predicts from introducing
//!     new cards at each preset's new-cards-per-day;
//!   - today: `learned` = distinct cards whose first graded review happened today; `relearned` =
//!     distinct cards answered in relearning today; the other figures count answers;
//!   - buttons: learning = Learn and Relearn rows, young = Review rows with lastIvl < 21 days,
//!     mature = Review rows with lastIvl ≥ 21 days, over the last 365 days;
//!   - card counts: suspended and buried cards are counted only as such.

use crate::collection::internals::CollectionCore;
use crate::collection::rows::{row_to_card, select_sql, CARD_COLUMNS};
use crate::collection::sched_context::{sched_clock, subtree_deck_ids};
use crate::collection::service::Collection;
use crate::error::Result;
use crate::i18n::t;
use crate::scheduler::{deck_filter_sql, simulate_workload, SchedClock, WorkloadInput};
use crate::stats::insights::compute_insights;
use crate::types::*;
use rusqlite::types::ValueRef;
use std::collections::{BTreeMap, HashMap};

/// `(count, sum time, again, ok, relearned)` of today's answers.
type TodayRow = (i64, Option<i64>, Option<i64>, Option<i64>, i64);
/// `(day, count, sum time, learn, review, relearn)` per studied day.
type PerDayRow = (i64, i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>);
/// `(new, learning, young, mature, suspended, buried)` sums.
type CardCountRow = (Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<i64>);

pub const FORECAST_DAYS: usize = 90;
pub const HISTORY_DAYS: i64 = 365;
const MATURE_DAYS: i64 = 21;

/// Revlog source for a scope: FROM clause and WHERE fragment over alias `r`.
#[derive(Debug, Clone)]
pub struct RevlogScope {
    pub from: String,
    pub where_sql: String,
    pub params: Vec<i64>,
}

pub fn revlog_scope(deck_ids: Option<&[DeckId]>) -> RevlogScope {
    match deck_ids {
        None => RevlogScope { from: "revlog r".into(), where_sql: "1".into(), params: vec![] },
        Some(ids) => {
            let (f, params) = deck_filter_sql(Some(ids), "c.did");
            RevlogScope { from: "revlog r JOIN cards c ON c.id = r.cid".into(), where_sql: f, params }
        }
    }
}

/// SQL for the collection day of revlog row `r.id`, given `today` and `next_day_at` (ms) bound first.
const DAY_EXPR: &str = "CAST(? - CAST((? - 1 - r.id) / 86400000 AS INTEGER) AS INTEGER)";

/// Local calendar date (YYYY-MM-DD) of an instant (seconds) at a UTC offset (minutes west).
pub fn iso_date(secs: i64, offset_west: i64) -> String {
    chrono::DateTime::from_timestamp(secs - offset_west * 60, 0).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// Labelled buckets for a value list: upper bounds are inclusive.
fn bucketise(values: &[f64], buckets: &[(&str, f64)]) -> Vec<Bucket> {
    let mut out: Vec<Bucket> = buckets.iter().map(|(b, _)| Bucket { bucket: (*b).to_string(), count: 0 }).collect();
    for v in values {
        let i = buckets.iter().position(|(_, max)| *v <= *max).unwrap_or(out.len() - 1);
        out[i].count += 1;
    }
    out
}

pub const INTERVAL_BUCKETS: [(&str, f64); 9] = [
    ("1d", 1.0),
    ("2–3d", 3.0),
    ("4–7d", 7.0),
    ("1–2w", 14.0),
    ("2–4w", 30.0),
    ("1–3mo", 91.0),
    ("3–6mo", 182.0),
    ("6–12mo", 365.0),
    ("1y+", f64::INFINITY),
];
pub const STABILITY_BUCKETS: [(&str, f64); 9] = [
    ("<1d", 1.0),
    ("1–3d", 3.0),
    ("3–7d", 7.0),
    ("1–2w", 14.0),
    ("2–4w", 30.0),
    ("1–3mo", 91.0),
    ("3–6mo", 182.0),
    ("6–12mo", 365.0),
    ("1y+", f64::INFINITY),
];

/// JS `typeof v === 'number'` for a `json_extract` column: integers and reals only.
fn num(v: ValueRef<'_>) -> Option<f64> {
    match v {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f),
        _ => None,
    }
}

/// Days with at least one graded answer, as collection day numbers (ascending).
pub fn studied_days(core: &CollectionCore, deck_ids: Option<&[DeckId]>, clock: &SchedClock) -> Result<Vec<CollectionDay>> {
    let s = revlog_scope(deck_ids);
    let mut params = vec![clock.today, clock.next_day_at * 1000];
    params.extend(s.params.iter());
    core.db().all(
        &format!(
            "SELECT DISTINCT {DAY_EXPR} AS day FROM {} WHERE r.ease BETWEEN 1 AND 4 AND {} ORDER BY day",
            s.from, s.where_sql
        ),
        rusqlite::params_from_iter(params),
        |r| r.get(0),
    )
}

/// True retention over the answers since `since_ms` (None when there are none), with the count.
pub fn true_retention(core: &CollectionCore, deck_ids: Option<&[DeckId]>, since_ms: i64) -> Result<(Option<f64>, i64)> {
    let s = revlog_scope(deck_ids);
    let mut params = vec![since_ms];
    params.extend(s.params.iter());
    let row: Option<(i64, Option<i64>)> = core.db().get(
        &format!(
            "SELECT count(*), sum(r.ease > 1) FROM {} WHERE r.id >= ? AND r.type = {} AND r.ease BETWEEN 1 AND 4 AND {}",
            s.from,
            RevlogType::Review as i64,
            s.where_sql
        ),
        rusqlite::params_from_iter(params),
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (n, ok) = row.unwrap_or((0, None));
    Ok((if n > 0 { Some(ok.unwrap_or(0) as f64 / n as f64) } else { None }, n))
}

/// Desired retention for a scope: the deck's preset, or the card-weighted mean over presets.
pub fn desired_retention_for(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<f64> {
    if let Some(d) = deck_id {
        return Ok(core.deck_config(d)?.desired_retention);
    }
    let rows: Vec<(i64, i64)> =
        core.db().all("SELECT did, count(*) FROM cards GROUP BY did", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut sum = 0.0;
    let mut weight = 0.0;
    for (did, n) in rows {
        let Ok(cfg) = core.deck_config(did) else { continue };
        sum += cfg.desired_retention * n as f64;
        weight += n as f64;
    }
    if weight > 0.0 {
        Ok(sum / weight)
    } else {
        Ok(core.deck_config(1)?.desired_retention)
    }
}

/// Per-day review counts from the queue simulation, summed over the presets in scope.
pub fn simulated_load(
    core: &CollectionCore,
    cards: &[Card],
    days: usize,
    with_new: bool,
    clock: &SchedClock,
) -> Result<Vec<f64>> {
    let preset_of: HashMap<DeckId, DeckConfigId> = core.all_decks()?.into_iter().map(|d| (d.id, d.config_id)).collect();
    let mut by_preset: BTreeMap<DeckConfigId, Vec<Card>> = BTreeMap::new();
    for c in cards {
        by_preset.entry(preset_of.get(&c.did).copied().unwrap_or(1)).or_default().push(c.clone());
    }
    let mut total = vec![0.0; days];
    for (preset_id, list) in by_preset {
        let mut config = match core.preset(preset_id) {
            Ok(p) => p.config,
            Err(_) => core.deck_config(1)?,
        };
        if !with_new {
            config.new_per_day = 0;
        }
        let sim = simulate_workload(&WorkloadInput {
            cards: &list,
            config: &config,
            days,
            today: clock.today,
            first_rating_prob: None,
        })?;
        for (i, v) in sim.reviews_per_day.iter().enumerate().take(days) {
            total[i] += v;
        }
    }
    Ok(total)
}

pub fn stats(core: &CollectionCore, req: &StatsRequest) -> Result<Stats> {
    let clock = sched_clock(core)?;
    let deck_ids = subtree_deck_ids(core, req.deck_id)?;
    let scope = deck_ids.as_deref();
    let (cf, cf_params) = deck_filter_sql(scope, "did");
    let rs = revlog_scope(scope);
    let day_base = [clock.today, clock.next_day_at * 1000];
    let today_start_ms = clock.day_start_secs(clock.today) * 1000;
    let db = core.db();
    let with_scope = |first: &[i64]| -> Vec<i64> {
        let mut p = first.to_vec();
        p.extend(rs.params.iter());
        p
    };
    let with_cards = |first: &[i64], last: &[i64]| -> Vec<i64> {
        let mut p = first.to_vec();
        p.extend(cf_params.iter());
        p.extend(last);
        p
    };

    // ── today ──
    let td: Option<TodayRow> = db.get(
        &format!(
            "SELECT count(*), sum(r.time), sum(r.ease = 1), sum(r.ease > 1), count(DISTINCT CASE WHEN r.type = {} THEN r.cid END)
               FROM {} WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4 AND {}",
            RevlogType::Relearn as i64,
            rs.from,
            rs.where_sql
        ),
        rusqlite::params_from_iter(with_scope(&[today_start_ms])),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let (studied, t_ms, again, ok, relearned) = td.unwrap_or((0, None, None, None, 0));
    // New cards learned today: distinct cards with a graded answer today and none before today.
    let mut learned_params = with_scope(&[today_start_ms]);
    learned_params.push(today_start_ms);
    let learned = db.count(
        &format!(
            "SELECT count(DISTINCT r.cid) FROM {} WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4 AND {}
                AND NOT EXISTS (SELECT 1 FROM revlog p WHERE p.cid = r.cid AND p.id < ? AND p.ease BETWEEN 1 AND 4)",
            rs.from, rs.where_sql
        ),
        rusqlite::params_from_iter(learned_params),
    )?;
    let today = StatsToday {
        studied,
        seconds: (t_ms.unwrap_or(0) as f64 / 1000.0).round() as i64,
        again: again.unwrap_or(0),
        correct_pct: if studied > 0 { ok.unwrap_or(0) as f64 / studied as f64 } else { 0.0 },
        learned,
        relearned,
    };

    // ── forecast ──
    let mut scheduled = vec![0i64; FORECAST_DAYS];
    let due_rows: Vec<(i64, i64)> = db.all(
        &format!(
            "SELECT CAST(max(0, due - ?) AS INTEGER) AS d, count(*) FROM cards WHERE {cf} AND queue = {} AND due < ? GROUP BY 1",
            CardQueue::Review as i64
        ),
        rusqlite::params_from_iter(with_cards(&[clock.today], &[clock.today + FORECAST_DAYS as i64])),
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    for (d, n) in due_rows {
        if let Some(slot) = usize::try_from(d).ok().and_then(|i| scheduled.get_mut(i)) {
            *slot += n;
        }
    }
    let scope_cards: Vec<Card> = db
        .all(
            &format!("{} WHERE {cf} AND queue >= {}", select_sql("cards", &CARD_COLUMNS), CardQueue::SchedBuried as i64),
            rusqlite::params_from_iter(cf_params.iter()),
            row_to_card,
        )?
        .into_iter()
        .filter(|c| c.queue != CardQueue::Suspended)
        .collect();
    let sim_with = simulated_load(core, &scope_cards, FORECAST_DAYS, true, &clock)?;
    let sim_without = simulated_load(core, &scope_cards, FORECAST_DAYS, false, &clock)?;
    let forecast: Vec<ForecastDay> = scheduled
        .iter()
        .enumerate()
        .map(|(day, due)| ForecastDay {
            day: day as i64,
            due: *due,
            with_new: due + ((sim_with[day] - sim_without[day]).round() as i64).max(0),
        })
        .collect();

    // ── retention ──
    let desired = desired_retention_for(core, req.deck_id)?;
    let periods: [(String, Option<i64>); 5] = [
        (t("stats.periods.week", &[]), Some(7)),
        (t("stats.periods.month", &[]), Some(30)),
        (t("stats.periods.quarter", &[]), Some(90)),
        (t("stats.periods.year", &[]), Some(365)),
        (t("stats.periods.all", &[]), None),
    ];
    let mut retention = Vec::with_capacity(5);
    for (label, days) in periods {
        let since = match days {
            None => 0,
            Some(d) => clock.day_start_secs(clock.today - d + 1) * 1000,
        };
        let (r, reviews) = true_retention(core, scope, since)?;
        retention.push(RetentionRow { label, true_retention: r, desired, reviews });
    }

    // ── reviews per day / heatmap ──
    let first_day = clock.today - HISTORY_DAYS + 1;
    let mut per_day: HashMap<i64, ReviewsDay> = HashMap::new();
    let mut per_day_params = day_base.to_vec();
    per_day_params.push(clock.day_start_secs(first_day) * 1000);
    per_day_params.extend(rs.params.iter());
    let rows: Vec<PerDayRow> = db.all(
        &format!(
            "SELECT {DAY_EXPR} AS day, count(*), sum(r.time), sum(r.type = {}), sum(r.type IN ({}, {})), sum(r.type = {})
               FROM {} WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4 AND {} GROUP BY day",
            RevlogType::Learn as i64,
            RevlogType::Review as i64,
            RevlogType::Filtered as i64,
            RevlogType::Relearn as i64,
            rs.from,
            rs.where_sql
        ),
        rusqlite::params_from_iter(per_day_params),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    for (day, n, t_ms, l, v, rl) in rows {
        per_day.insert(
            day,
            ReviewsDay {
                day,
                count: n,
                seconds: (t_ms.unwrap_or(0) as f64 / 1000.0).round() as i64,
                learn: l.unwrap_or(0),
                review: v.unwrap_or(0),
                relearn: rl.unwrap_or(0),
            },
        );
    }
    let offset = core.tz_offset(clock.now_ms);
    let mut reviews_per_day = Vec::with_capacity(HISTORY_DAYS as usize);
    let mut heatmap = Vec::with_capacity(HISTORY_DAYS as usize);
    for day in first_day..=clock.today {
        let v = per_day.remove(&day).unwrap_or(ReviewsDay { day, count: 0, seconds: 0, learn: 0, review: 0, relearn: 0 });
        heatmap.push(HeatCell { date: iso_date(clock.day_start_secs(day), offset), count: v.count });
        reviews_per_day.push(v);
    }

    // ── distributions ──
    let review_ivls: Vec<f64> = db.all(
        &format!("SELECT ivl FROM cards WHERE {cf} AND type IN ({}, {})", CardType::Review as i64, CardType::Relearn as i64),
        rusqlite::params_from_iter(cf_params.iter()),
        |r| r.get::<_, i64>(0).map(|v| v as f64),
    )?;
    let intervals = bucketise(&review_ivls, &INTERVAL_BUCKETS);
    let mem_cards: Vec<(Option<f64>, Option<f64>)> = db.all(
        &format!(
            "SELECT json_extract(data, '$.s'), json_extract(data, '$.d') FROM cards WHERE {cf} AND type != {}",
            CardType::New as i64
        ),
        rusqlite::params_from_iter(cf_params.iter()),
        |r| Ok((num(r.get_ref(0)?), num(r.get_ref(1)?))),
    )?;
    let stabilities: Vec<f64> = mem_cards.iter().filter_map(|(s, _)| *s).collect();
    let stability = bucketise(&stabilities, &STABILITY_BUCKETS);
    let mut difficulty: Vec<Bucket> =
        (0..10).map(|i| Bucket { bucket: format!("{}–{}%", i * 10, i * 10 + 10), count: 0 }).collect();
    for d in mem_cards.iter().filter_map(|(_, d)| *d) {
        let frac = ((d - 1.0) / 9.0).clamp(0.0, 1.0);
        let idx = ((frac * 10.0).floor() as usize).min(9);
        difficulty[idx].count += 1;
    }

    // ── hourly ──
    let offset_secs = offset * 60;
    let mut hourly: Vec<HourRow> = (0..24).map(|hour| HourRow { hour, reviews: 0, correct_pct: 0.0 }).collect();
    let hour_rows: Vec<(i64, i64, Option<i64>)> = db.all(
        &format!(
            "SELECT CAST(((CAST(r.id / 1000 AS INTEGER) - CAST(? AS INTEGER)) % 86400 + 86400) % 86400 / 3600 AS INTEGER) AS h, count(*), sum(r.ease > 1)
               FROM {} WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4 AND {} GROUP BY h",
            rs.from, rs.where_sql
        ),
        rusqlite::params_from_iter(with_scope(&[offset_secs, clock.day_start_secs(first_day) * 1000])),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    for (h, n, ok) in hour_rows {
        if let Some(row) = usize::try_from(h).ok().and_then(|i| hourly.get_mut(i)) {
            row.reviews = n;
            row.correct_pct = if n > 0 { ok.unwrap_or(0) as f64 / n as f64 } else { 0.0 };
        }
    }

    // ── buttons ──
    let mut buttons = ButtonStats { learn: vec![0; 4], young: vec![0; 4], mature: vec![0; 4] };
    let button_rows: Vec<(String, i64, i64)> = db.all(
        &format!(
            "SELECT CASE WHEN r.type IN ({}, {}) THEN 'learn' WHEN r.last_ivl >= {MATURE_DAYS} THEN 'mature' ELSE 'young' END AS k,
                    r.ease AS ease, count(*)
               FROM {} WHERE r.id >= ? AND r.ease BETWEEN 1 AND 4 AND r.type IN (0, 1, 2, 3) AND {} GROUP BY k, ease",
            RevlogType::Learn as i64,
            RevlogType::Relearn as i64,
            rs.from,
            rs.where_sql
        ),
        rusqlite::params_from_iter(with_scope(&[clock.day_start_secs(first_day) * 1000])),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    for (k, ease, n) in button_rows {
        let target = match k.as_str() {
            "learn" => &mut buttons.learn,
            "mature" => &mut buttons.mature,
            _ => &mut buttons.young,
        };
        if let Some(slot) = usize::try_from(ease - 1).ok().and_then(|i| target.get_mut(i)) {
            *slot = n;
        }
    }

    // ── card counts ──
    let cc: Option<CardCountRow> = db.get(
        &format!(
            "SELECT sum(queue = {}), sum(queue IN ({}, {}, {})), sum(queue = {} AND ivl < {MATURE_DAYS}), sum(queue = {} AND ivl >= {MATURE_DAYS}),
                    sum(queue = {}), sum(queue IN ({}, {})) FROM cards WHERE {cf}",
            CardQueue::New as i64,
            CardQueue::Learn as i64,
            CardQueue::DayLearn as i64,
            CardQueue::Preview as i64,
            CardQueue::Review as i64,
            CardQueue::Review as i64,
            CardQueue::Suspended as i64,
            CardQueue::SchedBuried as i64,
            CardQueue::UserBuried as i64,
        ),
        rusqlite::params_from_iter(cf_params.iter()),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    let (n_new, n_learning, n_young, n_mature, n_susp, n_buried) = cc.unwrap_or_default();
    let card_counts = CardCounts {
        new: n_new.unwrap_or(0),
        learning: n_learning.unwrap_or(0),
        young: n_young.unwrap_or(0),
        mature: n_mature.unwrap_or(0),
        suspended: n_susp.unwrap_or(0),
        buried: n_buried.unwrap_or(0),
    };

    Ok(Stats {
        today,
        forecast,
        retention,
        reviews_per_day,
        heatmap,
        intervals,
        stability,
        difficulty,
        hourly,
        buttons,
        card_counts,
        momentum: momentum(&studied_days(core, scope, &clock)?, clock.today),
        insights: compute_insights(core, req.deck_id),
    })
}

/// Momentum (internals §7): days studied in the last 30, the current run of consecutive study days
/// (ending today, or yesterday when today has not been studied yet: a day is never lost before
/// it ends) and the longest run ever.
pub fn momentum(days: &[CollectionDay], today: CollectionDay) -> Momentum {
    let set: std::collections::BTreeSet<CollectionDay> = days.iter().copied().collect();
    let days_studied_last30 = ((today - 29)..=today).filter(|d| set.contains(d)).count() as i64;
    let mut current_run = 0;
    let mut d = if set.contains(&today) { today } else { today - 1 };
    while set.contains(&d) {
        current_run += 1;
        d -= 1;
    }
    let mut longest_run = 0;
    let mut run = 0;
    let mut prev: Option<CollectionDay> = None;
    for d in &set {
        run = if prev.is_some_and(|p| *d == p + 1) { run + 1 } else { 1 };
        longest_run = longest_run.max(run);
        prev = Some(*d);
    }
    Momentum { days_studied_last30, current_run, longest_run }
}

impl Collection {
    pub fn stats(&self, req: &StatsRequest) -> Result<Stats> {
        self.rollover()?;
        stats(&self.core, req)
    }
    pub fn insights(&self) -> Result<Vec<Insight>> {
        self.rollover()?;
        Ok(compute_insights(&self.core, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn momentum_counts_runs_and_recent_days() {
        let m = momentum(&[1, 2, 3, 7, 8, 10], 10);
        assert_eq!(m, Momentum { days_studied_last30: 6, current_run: 1, longest_run: 3 });
        // today not yet studied: the run ending yesterday still counts
        let m = momentum(&[8, 9], 10);
        assert_eq!(m.current_run, 2);
        assert_eq!(momentum(&[], 10), Momentum::default());
    }

    #[test]
    fn buckets_are_inclusive_upper_bounds_with_overflow_in_the_last() {
        let b = bucketise(&[1.0, 2.0, 3.0, 3.5, 1000.0, f64::INFINITY], &INTERVAL_BUCKETS);
        assert_eq!(b[0].count, 1);
        assert_eq!(b[1].count, 2);
        assert_eq!(b[2].count, 1);
        assert_eq!(b[8].count, 2);
    }

    #[test]
    fn iso_date_applies_the_offset() {
        assert_eq!(iso_date(0, 0), "1970-01-01");
        assert_eq!(iso_date(0, 60), "1969-12-31");
    }
}
