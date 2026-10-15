//! Insights (internals §10): rules that turn the collection's
//! numbers into one sentence and, when useful, one action. Each rule is independent and cheap
//! enough to run when Home opens.
//!
//! Rules (in display order):
//!   backlog         — the catch-up trigger fired (due > 2× daily average or > 3 days overdue);
//!   retention-gap   — 30-day true retention ≥ 3 points under a preset's desired retention
//!                     (≥ 100 review answers): suggest optimising;
//!   never-optimised — FSRS presets with ≥ 400 reviews and no optimisation run yet (one insight);
//!   workload        — the 30-day forecast at the preset's new cards/day needs > 125% of its
//!                     daily minutes budget;
//!   struggling      — cards matching `tag:leech or prop:lapses>=8` (not suspended);
//!   optimised       — an optimisation was adopted in the last 7 days (log loss before → after);
//!   new-available   — a deck has nothing due but new cards ready;
//!   welcome-back    — nothing studied today or yesterday although there is history (gentle);
//!   long-sessions   — the median study session of the last 14 days ran over an hour.

use crate::collection::backlog::backlog_report;
use crate::collection::counter::scheduler_deck_counts;
use crate::collection::decks::deck_tree;
use crate::collection::internals::CollectionCore;
use crate::collection::render::encode_uri_component;
use crate::collection::rows::{row_to_card, select_sql, CARD_COLUMNS};
use crate::collection::sched_context::{sched_clock, search_context, subtree_deck_ids};
use crate::error::Result;
use crate::i18n::{msg, pct};
use crate::scheduler::{deck_filter_sql, seconds_per_card, simulate_workload, SchedClock, WorkloadInput};
use crate::search::{run_search, SearchMode};
use crate::types::*;
use std::collections::{BTreeMap, VecDeque};

/// Browse query for struggling cards.
pub const STRUGGLING_QUERY: &str = "tag:leech or prop:lapses>=8";
const SESSION_GAP_MS: i64 = 30 * 60_000;

fn preset_route(id: i64) -> String {
    format!("#/settings/presets/{id}")
}
fn browse_route(q: &str) -> String {
    format!("#/browse?q={}", encode_uri_component(q))
}
/// `"deck:<name>"` with `\ " * _` escaped.
fn quote_deck(name: &str) -> String {
    let mut out = String::from("\"deck:");
    for c in name.chars() {
        if matches!(c, '\\' | '"' | '*' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

type Rule<'a> = Box<dyn Fn() -> Result<Vec<Insight>> + 'a>;

/// Every insight that applies, most actionable first. One failing rule never hides the others.
pub fn compute_insights(core: &CollectionCore, deck_id: Option<DeckId>) -> Vec<Insight> {
    let has_cards = core.db().scalar::<i64, _>("SELECT 1 FROM cards LIMIT 1", []).ok().flatten().is_some();
    if !has_cards {
        return vec![];
    }
    let Ok(clock) = sched_clock(core) else { return vec![] };
    let one = |r: Result<Option<Insight>>| r.map(|o| o.into_iter().collect::<Vec<_>>());
    let rules: Vec<Rule<'_>> = vec![
        Box::new(|| one(backlog_insight(core, deck_id))),
        Box::new(|| retention_gap(core, &clock)),
        Box::new(|| one(never_optimised(core))),
        Box::new(|| workload(core, &clock)),
        Box::new(|| one(struggling(core, deck_id))),
        Box::new(|| one(optimised(core, &clock))),
        Box::new(|| one(new_available(core, deck_id))),
        Box::new(|| one(welcome_back(core, &clock))),
        Box::new(|| one(long_sessions(core, &clock))),
    ];
    let mut out = Vec::new();
    for rule in rules {
        if let Ok(list) = rule() {
            out.extend(list);
        }
    }
    out
}

/// The single insight shown on the session summary: cheap rules only.
pub fn session_insight(core: &CollectionCore, deck_id: Option<DeckId>) -> Option<Insight> {
    if let Ok(Some(i)) = struggling(core, deck_id) {
        return Some(i);
    }
    if let Ok(Some(i)) = backlog_insight(core, deck_id) {
        return Some(i);
    }
    if let Ok(clock) = sched_clock(core) {
        if let Ok(Some(i)) = optimised(core, &clock) {
            return Some(i);
        }
    }
    None
}

fn backlog_insight(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<Option<Insight>> {
    let b = backlog_report(core, deck_id)?;
    if !b.triggered || b.due == 0 {
        return Ok(None);
    }
    Ok(Some(Insight {
        id: "backlog".into(),
        severity: Severity::Suggest,
        text: msg(
            "insights.backlog",
            "{due, plural, one {# review is} other {# reviews are}} waiting (about {minutes} min). At your pace they clear in {days, plural, one {# day} other {# days}}; a catch-up plan spreads them without risking what you might forget.",
            &[("due", b.due.into()), ("minutes", b.estimated_minutes.into()), ("days", b.days_at_current_pace.into())],
        ),
        action: Some(InsightAction {
            label: msg("insights.backlogAction", "Plan a catch-up", &[]),
            route: format!("#/backlog/{}", deck_id.map(|d| d.to_string()).unwrap_or_else(|| "all".into())),
        }),
    }))
}

fn retention_gap(core: &CollectionCore, clock: &SchedClock) -> Result<Vec<Insight>> {
    let since = (clock.next_day_at - 30 * 86_400) * 1000;
    let rows: Vec<(i64, i64, Option<i64>)> = core.db().all(
        &format!(
            "SELECT d.config_id, count(*), sum(r.ease > 1) FROM revlog r JOIN cards c ON c.id = r.cid JOIN decks d ON d.id = c.did
              WHERE r.id >= ? AND r.type = {} AND r.ease BETWEEN 1 AND 4 GROUP BY d.config_id",
            RevlogType::Review as i64
        ),
        [since],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut out = Vec::new();
    for (p, n, ok) in rows {
        if n < 100 {
            continue;
        }
        let preset = core.preset(p)?;
        let actual = ok.unwrap_or(0) as f64 / n as f64;
        if actual >= preset.config.desired_retention - 0.03 {
            continue;
        }
        out.push(Insight {
            id: format!("retention-gap-{}", preset.id),
            severity: Severity::Suggest,
            text: msg(
                "insights.retentionLow",
                "True retention {actual} vs target {target}: optimise parameters",
                &[("actual", pct(actual).into()), ("target", pct(preset.config.desired_retention).into())],
            ),
            action: Some(InsightAction {
                label: msg("insights.optimiseAction", "Optimise", &[]),
                route: preset_route(preset.id),
            }),
        });
    }
    Ok(out)
}

/// Graded reviews per preset (all time).
fn reviews_per_preset(core: &CollectionCore) -> Result<BTreeMap<i64, i64>> {
    let rows: Vec<(i64, i64)> = core.db().all(
        "SELECT d.config_id, count(*) FROM revlog r JOIN cards c ON c.id = r.cid JOIN decks d ON d.id = c.did
          WHERE r.ease BETWEEN 1 AND 4 GROUP BY d.config_id",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(rows.into_iter().collect())
}

fn never_optimised(core: &CollectionCore) -> Result<Option<Insight>> {
    let mut ready: Vec<(i64, String, i64)> = Vec::new();
    for (p, n) in reviews_per_preset(core)? {
        if n < 400 {
            continue;
        }
        if core.db().scalar::<i64, _>("SELECT 1 FROM optimise_log WHERE config_id = ? LIMIT 1", [p])?.is_some() {
            continue;
        }
        let Ok(preset) = core.preset(p) else { continue };
        if preset.config.scheduler != SchedulerKind::Fsrs {
            continue;
        }
        ready.push((p, preset.name, n));
    }
    if ready.is_empty() {
        return Ok(None);
    }
    ready.sort_by_key(|a| std::cmp::Reverse(a.2));
    let (id, name, reviews) = &ready[0];
    let text = if ready.len() == 1 {
        msg(
            "insights.neverOptimised",
            "“{preset}” has {count} reviews but its parameters were never optimised; optimising fits the schedule to your memory.",
            &[("preset", name.into()), ("count", (*reviews).into())],
        )
    } else {
        msg(
            "insights.neverOptimisedMany",
            "{count} presets have enough reviews to optimise their parameters, starting with “{preset}”; optimising fits the schedule to your memory.",
            &[("count", ready.len().into()), ("preset", name.into())],
        )
    };
    Ok(Some(Insight {
        id: "never-optimised".into(),
        severity: Severity::Suggest,
        text,
        action: Some(InsightAction { label: msg("insights.optimiseAction", "Optimise", &[]), route: preset_route(*id) }),
    }))
}

fn workload(core: &CollectionCore, clock: &SchedClock) -> Result<Vec<Insight>> {
    let mut out = Vec::new();
    // insertion order of `Map` in JS: first deck seen per preset
    let mut decks_by_preset: Vec<(DeckConfigId, Vec<DeckId>)> = Vec::new();
    for d in core.all_decks()? {
        match decks_by_preset.iter_mut().find(|(p, _)| *p == d.config_id) {
            Some((_, ids)) => ids.push(d.id),
            None => decks_by_preset.push((d.config_id, vec![d.id])),
        }
    }
    let pace = seconds_per_card(core.db(), clock.now_ms, None)?;
    for (p, deck_ids) in decks_by_preset {
        let Ok(preset) = core.preset(p) else { continue };
        let cfg = preset.config;
        if cfg.new_per_day <= 0 {
            continue;
        }
        let (f, params) = deck_filter_sql(Some(&deck_ids), "did");
        let cards: Vec<Card> = core.db().all(
            &format!("{} WHERE {f} AND queue != {}", select_sql("cards", &CARD_COLUMNS), CardQueue::Suspended as i64),
            rusqlite::params_from_iter(params),
            row_to_card,
        )?;
        if !cards.iter().any(|c| c.queue == CardQueue::New) {
            continue;
        }
        let sim = simulate_workload(&WorkloadInput {
            cards: &cards,
            config: &cfg,
            days: 30,
            today: clock.today,
            first_rating_prob: None,
        })?;
        let minutes = (sim.reviews_per_day.iter().sum::<f64>() / 30.0) * pace / 60.0;
        if minutes <= cfg.minutes_per_day as f64 * 1.25 {
            continue;
        }
        out.push(Insight {
            id: format!("workload-{p}"),
            severity: Severity::Suggest,
            text: msg(
                "insights.workload",
                "At {count, plural, one {# new card} other {# new cards}} a day, “{preset}” needs about {minutes} min a day, above your {budget}-minute budget. Fewer new cards keep reviews manageable.",
                &[
                    ("count", cfg.new_per_day.into()),
                    ("preset", preset.name.into()),
                    ("minutes", (minutes.round() as i64).into()),
                    ("budget", cfg.minutes_per_day.into()),
                ],
            ),
            action: Some(InsightAction { label: msg("insights.workloadAction", "Adjust new cards", &[]), route: preset_route(p) }),
        });
    }
    Ok(out)
}

fn struggling(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<Option<Insight>> {
    let scope: Option<String> = match deck_id {
        None => None,
        Some(d) => Some(core.deck(d)?.name),
    };
    let search = match &scope {
        None => format!("({STRUGGLING_QUERY}) -is:suspended"),
        Some(name) => format!("({STRUGGLING_QUERY}) -is:suspended {}", quote_deck(name)),
    };
    let ids = run_search(core.db(), &search, &search_context(core, None)?, SearchMode::Cards)?;
    if ids.is_empty() {
        return Ok(None);
    }
    let query = match &scope {
        None => STRUGGLING_QUERY.to_string(),
        Some(name) => format!("{} ({STRUGGLING_QUERY})", quote_deck(name)),
    };
    let count = ids.len() as i64;
    let text = match &scope {
        None => msg(
            "insights.strugglingAll",
            "{count, plural, one {# card keeps} other {# cards keep}} slipping. Rewording them usually helps more than repeating them.",
            &[("count", count.into())],
        ),
        Some(name) => msg(
            "insights.struggling",
            "{deck} has {count, plural, one {# struggling card} other {# struggling cards}}",
            &[("deck", name.into()), ("count", count.into())],
        ),
    };
    Ok(Some(Insight {
        id: "struggling".into(),
        severity: Severity::Warn,
        text,
        action: Some(InsightAction {
            label: msg("insights.strugglingAction", "See the cards", &[]),
            route: browse_route(&query),
        }),
    }))
}

fn optimised(core: &CollectionCore, clock: &SchedClock) -> Result<Option<Insight>> {
    let row: Option<(i64, String, String)> = core.db().get(
        "SELECT config_id, before, after FROM optimise_log WHERE adopted = 1 AND at >= ? ORDER BY id DESC LIMIT 1",
        [clock.now_ms.div_euclid(1000) - 7 * 86_400],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let Some((config_id, before, after)) = row else { return Ok(None) };
    let log_loss = |s: &str| -> f64 {
        serde_json::from_str::<serde_json::Value>(s)
            .ok()
            .and_then(|v| v.get("logLoss").and_then(|x| x.as_f64()))
            .unwrap_or(f64::NAN)
    };
    // JS `toFixed(2)`; Rust `{:.2}` differs only on exact ties of the binary value.
    Ok(Some(Insight {
        id: "optimised".into(),
        severity: Severity::Info,
        text: msg(
            "insights.optimised",
            "Parameters updated: log loss {before} → {after}",
            &[("before", format!("{:.2}", log_loss(&before)).into()), ("after", format!("{:.2}", log_loss(&after)).into())],
        ),
        action: Some(InsightAction {
            label: msg("insights.optimisedAction", "See the preset", &[]),
            route: preset_route(config_id),
        }),
    }))
}

fn new_available(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<Option<Insight>> {
    let tree = deck_tree(core, &scheduler_deck_counts)?;
    let scope_ids = subtree_deck_ids(core, deck_id)?;
    let mut queue: VecDeque<DeckTreeNode> = tree.children.into_iter().collect();
    while let Some(n) = queue.pop_front() {
        if scope_ids.as_ref().is_some_and(|ids| !ids.contains(&n.deck.id)) {
            queue.extend(n.children);
            continue;
        }
        if n.review_count == 0 && n.learn_count == 0 && n.new_count > 0 {
            return Ok(Some(Insight {
                id: "new-available".into(),
                severity: Severity::Info,
                text: msg(
                    "insights.newAvailable",
                    "{deck} has nothing due; {count, plural, one {# new card is} other {# new cards are}} ready.",
                    &[("deck", n.deck.name.as_str().into()), ("count", n.new_count.into())],
                ),
                action: Some(InsightAction {
                    label: msg("insights.newAvailableAction", "Learn new cards", &[]),
                    route: format!("#/study/{}", n.deck.id),
                }),
            }));
        }
        queue.extend(n.children);
    }
    Ok(None)
}

fn welcome_back(core: &CollectionCore, clock: &SchedClock) -> Result<Option<Insight>> {
    let yesterday_start = (clock.next_day_at - 2 * 86_400) * 1000;
    let recent =
        core.db().scalar::<i64, _>("SELECT 1 FROM revlog WHERE id >= ? AND ease BETWEEN 1 AND 4 LIMIT 1", [yesterday_start])?;
    if recent.is_some() {
        return Ok(None);
    }
    let any = core.db().scalar::<i64, _>("SELECT 1 FROM revlog WHERE ease BETWEEN 1 AND 4 LIMIT 1", [])?;
    if any.is_none() {
        return Ok(None);
    }
    Ok(Some(Insight {
        id: "welcome-back".into(),
        severity: Severity::Info,
        text: msg(
            "insights.welcomeBack",
            "Welcome back. Nothing is lost: today’s cards are a good place to start, and catch-up can pace the rest.",
            &[],
        ),
        action: None,
    }))
}

fn long_sessions(core: &CollectionCore, clock: &SchedClock) -> Result<Option<Insight>> {
    let ids: Vec<i64> = core.db().all(
        "SELECT id FROM revlog WHERE id >= ? AND ease BETWEEN 1 AND 4 ORDER BY id",
        [(clock.next_day_at - 14 * 86_400) * 1000],
        |r| r.get(0),
    )?;
    if ids.len() < 20 {
        return Ok(None);
    }
    let mut lengths: Vec<i64> = Vec::new();
    let mut start = ids[0];
    let mut prev = ids[0];
    for &id in &ids[1..] {
        if id - prev > SESSION_GAP_MS {
            lengths.push(prev - start);
            start = id;
        }
        prev = id;
    }
    lengths.push(prev - start);
    lengths.sort_unstable();
    let median = lengths[lengths.len() / 2];
    if median <= 60 * 60_000 {
        return Ok(None);
    }
    Ok(Some(Insight {
        id: "long-sessions".into(),
        severity: Severity::Info,
        text: msg(
            "insights.longSessions",
            "Your sessions usually run past an hour. Two shorter sessions tend to keep accuracy higher.",
            &[],
        ),
        action: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_deck_quoting() {
        assert_eq!(quote_deck("A::B_c*"), "\"deck:A::B\\_c\\*\"");
        assert_eq!(browse_route(STRUGGLING_QUERY), "#/browse?q=tag%3Aleech%20or%20prop%3Alapses%3E%3D8");
        assert_eq!(preset_route(3), "#/settings/presets/3");
    }
}
