//! Catch-up mode (internals §7): the backlog report and
//! the four catch-up plans.
//!
//! A backlog is "triggered" when more reviews are due than twice the recent daily average, or
//! when anything is more than 3 days overdue. Postponing uses Anki's set-due-date semantics
//! (`reschedule.rs`) with revlog type Rescheduled (5); the cards most likely to be remembered
//! (highest retrievability) wait longest, so nothing at risk is delayed.

use crate::collection::internals::{CollectionCore, DEFAULT_PRESET_ID};
use crate::collection::reschedule::set_due_date;
use crate::collection::rows::{parse_json_object, row_to_card, select_sql, CARD_COLUMNS};
use crate::collection::sched_context::{sched_clock, subtree_deck_ids};
use crate::collection::service::Collection;
use crate::collection::study::fill_session;
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::scheduler::{
    decay_from_params, deck_filter_sql, forgetting_curve, normalize_parameters, seconds_per_card, SchedClock,
};
use crate::types::*;
use crate::undo::{self, PresetRow, Table, UndoSnapshot};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// How far back the recent daily average looks, days.
const AVERAGE_WINDOW_DAYS: i64 = 30;
/// Raw preset-config key holding a temporary desired retention: {original, untilDay, retention}.
pub const TEMP_RETENTION_KEY: &str = "lociTemporaryRetention";

/// The marker stored under `TEMP_RETENTION_KEY`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempRetention {
    pub original: f64,
    pub retention: f64,
    pub until_day: CollectionDay,
}

/// None unless `original` and `untilDay` are numbers;
/// `retention` falls back to `original`.
pub fn read_temporary_retention(raw: &Map<String, Value>) -> Option<TempRetention> {
    let o = raw.get(TEMP_RETENTION_KEY)?.as_object()?;
    let original = o.get("original")?.as_f64()?;
    // JS `typeof untilDay === 'number'`; a fractional day is kept as-is there and truncated here.
    let until_day = o.get("untilDay")?.as_f64()? as i64;
    let retention = o.get("retention").and_then(|v| v.as_f64()).unwrap_or(original);
    Some(TempRetention { original, retention, until_day })
}

/// Review answers per studied day over the last 30 days (the pace at which the user clears
/// reviews). Without history: the number of cards the preset's daily time budget allows.
pub fn recent_daily_average(core: &CollectionCore, deck_ids: Option<&[DeckId]>, clock: &SchedClock) -> Result<f64> {
    let start = (clock.next_day_at - (AVERAGE_WINDOW_DAYS + 1) * 86_400) * 1000;
    let end = (clock.next_day_at - 86_400) * 1000; // up to the start of today
    let (f, fparams) = deck_filter_sql(deck_ids, "c.did");
    let base = clock.next_day_at * 1000;
    let join = if deck_ids.is_none() { "" } else { "JOIN cards c ON c.id = r.cid" };
    let scope = if deck_ids.is_none() { String::new() } else { format!("AND {f}") };
    let sql = format!(
        "SELECT count(*), count(DISTINCT CAST((? - 1 - r.id) / 86400000 AS INTEGER)) FROM revlog r {join}
          WHERE r.id >= ? AND r.id < ? AND r.ease BETWEEN 1 AND 4 AND r.type IN ({}, {}, {}) {scope}",
        RevlogType::Review as i64,
        RevlogType::Relearn as i64,
        RevlogType::Filtered as i64,
    );
    let mut params = vec![base, start, end];
    params.extend(fparams);
    let row: Option<(i64, i64)> = core.db().get(&sql, rusqlite::params_from_iter(params), |r| Ok((r.get(0)?, r.get(1)?)))?;
    let (n, days) = row.unwrap_or((0, 0));
    if n > 0 && days > 0 {
        return Ok(((n as f64 / days as f64) * 10.0).round() / 10.0);
    }
    let first_deck = deck_ids.and_then(|d| d.first().copied()).unwrap_or(1);
    let minutes = core.deck_config(first_deck)?.minutes_per_day;
    let secs = seconds_per_card(core.db(), clock.now_ms, deck_ids)?;
    Ok(((minutes as f64 * 60.0) / secs).floor().max(1.0))
}

/// "Due today", the one definition shared by the backlog report, its insight sentence and the
/// Stats forecast's day 0: review cards (queue Review) in the deck subtree whose due day is today
/// or earlier, before daily limits. Suspended and buried cards are excluded (their queue differs);
/// learning cards (intraday and interday) are not reviews and are counted by the learning pill.
pub fn due_review_rows(core: &CollectionCore, deck_ids: Option<&[DeckId]>, today: CollectionDay) -> Result<Vec<Card>> {
    let (f, mut params) = deck_filter_sql(deck_ids, "did");
    params.push(today);
    let sql = format!("{} WHERE {f} AND queue = {} AND due <= ?", select_sql("cards", &CARD_COLUMNS), CardQueue::Review as i64);
    core.db().all(&sql, rusqlite::params_from_iter(params), row_to_card)
}

/// Interday learning cards due today in scope (they stay today in every catch-up plan).
fn due_interday_learning(core: &CollectionCore, deck_ids: Option<&[DeckId]>, today: CollectionDay) -> Result<i64> {
    let (f, mut params) = deck_filter_sql(deck_ids, "did");
    params.push(today);
    let sql = format!("SELECT count(*) FROM cards WHERE {f} AND queue = {} AND due <= ?", CardQueue::DayLearn as i64);
    core.db().count(&sql, rusqlite::params_from_iter(params))
}

pub fn backlog_report(core: &CollectionCore, deck_id: Option<DeckId>) -> Result<BacklogReport> {
    let clock = sched_clock(core)?;
    let deck_ids = subtree_deck_ids(core, deck_id)?;
    let rows = due_review_rows(core, deck_ids.as_deref(), clock.today)?;
    let due = rows.len() as i64;
    let overdue = rows.iter().filter(|r| r.due < clock.today).count() as i64;
    let max_overdue_days = rows.iter().map(|r| r.due).min().map(|m| (clock.today - m).max(0)).unwrap_or(0);
    let daily_average = recent_daily_average(core, deck_ids.as_deref(), &clock)?;
    let secs = seconds_per_card(core.db(), clock.now_ms, deck_ids.as_deref())?;
    Ok(BacklogReport {
        due,
        overdue,
        max_overdue_days,
        daily_average,
        estimated_minutes: if due > 0 { ((due as f64 * secs) / 60.0).ceil() as i64 } else { 0 },
        days_at_current_pace: if due > 0 { (due as f64 / daily_average.max(1.0)).ceil() as i64 } else { 0 },
        triggered: due as f64 > 2.0 * daily_average || max_overdue_days > 3,
    })
}

/// Retrievability now. FSRS cards use their stored state and last review time; cards without a
/// memory state use the SM-2 interval as the stability (an interval is the time to 90% recall,
/// which is what FSRS stability means) with the default decay.
pub fn card_retrievability(card: &Card, clock: &SchedClock, params: &[f64]) -> Result<f64> {
    let decay = match card.data.decay {
        Some(d) => d,
        None => decay_from_params(&normalize_parameters(params)?),
    };
    let s = match card.data.s {
        Some(s) if s > 0.0 => s,
        _ => card.ivl.max(1) as f64,
    };
    let elapsed = match card.data.lrt {
        Some(lrt) if lrt > 0 => ((clock.now_ms as f64 / 1000.0 - lrt as f64) / 86_400.0).max(0.0),
        _ => (clock.today - (card.due - card.ivl)).max(0) as f64,
    };
    Ok(forgetting_curve(elapsed, s, decay))
}

/// Presets used by the decks in scope (preset 1 for "all" when no deck uses another).
fn presets_in_scope(core: &CollectionCore, deck_ids: Option<&[DeckId]>) -> Result<Vec<DeckConfigPreset>> {
    let mut ids: Vec<DeckConfigId> = Vec::new();
    for d in core.all_decks()? {
        if deck_ids.is_none_or(|ids| ids.contains(&d.id)) && !ids.contains(&d.config_id) {
            ids.push(d.config_id);
        }
    }
    if ids.is_empty() {
        ids.push(DEFAULT_PRESET_ID);
    }
    Ok(ids.into_iter().filter_map(|id| core.preset(id).ok()).collect())
}

fn preset_row(core: &CollectionCore, id: DeckConfigId) -> Result<Option<PresetRow>> {
    core.db().get("SELECT id, name, config, mtime, usn FROM deck_configs WHERE id = ?", [id], |r| {
        Ok(PresetRow { id: r.get(0)?, name: r.get(1)?, config: r.get(2)?, mtime: r.get(3)?, usn: r.get(4)? })
    })
}

pub fn apply_catch_up(core: &CollectionCore, deck_id: Option<DeckId>, plan: &CatchUpPlan) -> Result<CatchUpResult> {
    let clock = sched_clock(core)?;
    let deck_ids = subtree_deck_ids(core, deck_id)?;
    let scope = deck_ids.as_deref();
    match plan {
        CatchUpPlan::StudyAll => {
            let due = due_review_rows(core, scope, clock.today)?.len() as i64;
            return Ok(CatchUpResult { postponed: 0, kept_today: due, undo_label: String::new() });
        }
        CatchUpPlan::LowerRetention { retention, days } => return lower_retention(core, scope, *retention, *days, &clock),
        _ => {}
    }
    let label = msg("catchUp.undoLabel", "Catch-up plan", &[]);
    core.db().transaction(|| {
        let cards = due_review_rows(core, scope, clock.today)?;
        let interday = due_interday_learning(core, scope, clock.today)?;
        let mut params: HashMap<DeckId, Vec<f64>> = HashMap::new();
        let mut scored: Vec<(Card, f64)> = Vec::with_capacity(cards.len());
        for c in cards {
            let p = match params.get(&c.did) {
                Some(p) => p.clone(),
                None => {
                    let p = core.deck_config(c.did)?.fsrs_params;
                    params.insert(c.did, p.clone());
                    p
                }
            };
            let r = card_retrievability(&c, &clock, &p)?;
            scored.push((c, r));
        }
        // lowest R first: they stay today
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.id.cmp(&b.0.id)));
        let mut offsets: HashMap<CardId, i64> = HashMap::new();
        match plan {
            CatchUpPlan::Spread { days } => {
                let days = *days;
                if days < 1 {
                    return Err(LociError::invalid("Spread over at least one day"));
                }
                let per = (scored.len() as f64 / days as f64).ceil() as i64;
                for (i, (c, _)) in scored.iter().enumerate() {
                    offsets.insert(c.id, i as i64 / per.max(1));
                }
            }
            _ => {
                // today-share
                let keep = (recent_daily_average(core, scope, &clock)?.round() as i64 - interday).max(0);
                let rest = scored.len() as i64 - keep;
                let per = (rest as f64 / 7.0).ceil() as i64;
                for (i, (c, _)) in scored.iter().enumerate() {
                    let i = i as i64;
                    offsets.insert(c.id, if i < keep { 0 } else { 1 + (i - keep) / per.max(1) });
                }
            }
        }
        let moving: Vec<&Card> =
            scored.iter().filter(|(c, _)| offsets.get(&c.id).copied().unwrap_or(0) > 0).map(|(c, _)| c).collect();
        let moving_ids: Vec<i64> = moving.iter().map(|c| c.id).collect();
        let before = undo::snapshot_rows(core.db(), Table::Cards, &moving_ids)?;
        let mut inserted_revlog = Vec::with_capacity(moving.len());
        for c in &moving {
            // Postponed by `offset` days from today; the card's new interval grows by the delay.
            let offset = offsets.get(&c.id).copied().unwrap_or(0);
            let (_, revlog) = set_due_date(core, c, offset, &clock, false, RevlogType::Rescheduled)?;
            inserted_revlog.push(revlog.id);
        }
        if !moving.is_empty() {
            undo::record(core.db(), core.now(), &label, &UndoSnapshot { cards: before, inserted_revlog, ..Default::default() })?;
            core.mark_changed()?;
        }
        Ok(CatchUpResult {
            postponed: moving.len() as i64,
            kept_today: (scored.len() - moving.len()) as i64,
            undo_label: if moving.is_empty() { String::new() } else { label.clone() },
        })
    })
}

/// Lowers the desired retention of every preset used in scope to `retention` for `days` days.
/// The original value and the expiry day are stored in the preset's raw config under
/// TEMP_RETENTION_KEY; maintenance() restores the original when the day comes. Undoable.
fn lower_retention(
    core: &CollectionCore,
    deck_ids: Option<&[DeckId]>,
    retention: f64,
    days: i64,
    clock: &SchedClock,
) -> Result<CatchUpResult> {
    if !(0.7..=0.99).contains(&retention) {
        return Err(LociError::invalid("Retention must be between 0.70 and 0.99"));
    }
    if days < 1 {
        return Err(LociError::invalid("Lower retention for at least one day"));
    }
    let label = msg("catchUp.undoLowerRetention", "Lower retention", &[]);
    core.db().transaction(|| {
        let presets = presets_in_scope(core, deck_ids)?;
        let mut before = Vec::with_capacity(presets.len());
        for p in &presets {
            if let Some(row) = preset_row(core, p.id)? {
                before.push(row);
            }
        }
        for row in &before {
            let mut raw = parse_json_object(&row.config);
            let prior = read_temporary_retention(&raw);
            let original = match prior {
                Some(p) => p.original,
                None => raw.get("desiredRetention").and_then(|v| v.as_f64()).unwrap_or(0.9),
            };
            raw.insert("desiredRetention".into(), Value::from(retention));
            raw.insert(
                TEMP_RETENTION_KEY.into(),
                serde_json::json!({ "original": original, "retention": retention, "untilDay": clock.today + days }),
            );
            core.db().run(
                "UPDATE deck_configs SET config = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![Value::Object(raw).to_string(), core.now_seconds(), row.id],
            )?;
        }
        undo::record(core.db(), core.now(), &label, &UndoSnapshot { presets: before, ..Default::default() })?;
        core.mark_changed()?;
        let due = due_review_rows(core, deck_ids, clock.today)?.len() as i64;
        Ok(CatchUpResult { postponed: 0, kept_today: due, undo_label: label.clone() })
    })
}

/// Restores presets whose temporary retention has expired. Returns the preset ids restored.
pub fn expire_temporary_retention(core: &CollectionCore) -> Result<Vec<DeckConfigId>> {
    let today = core.today()?;
    core.db().transaction(|| {
        let rows: Vec<(i64, String)> =
            core.db().all("SELECT id, config FROM deck_configs", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut restored = Vec::new();
        for (id, config) in rows {
            let mut raw = parse_json_object(&config);
            let Some(m) = read_temporary_retention(&raw) else { continue };
            if today < m.until_day {
                continue;
            }
            // Only restore when the user has not changed the value meanwhile.
            if raw.get("desiredRetention").and_then(|v| v.as_f64()) == Some(m.retention) {
                raw.insert("desiredRetention".into(), Value::from(m.original));
            }
            raw.remove(TEMP_RETENTION_KEY);
            core.db().run(
                "UPDATE deck_configs SET config = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![Value::Object(raw).to_string(), core.now_seconds(), id],
            )?;
            restored.push(id);
        }
        if !restored.is_empty() {
            core.mark_changed()?;
        }
        Ok(restored)
    })
}

impl Collection {
    pub fn backlog_report(&self, deck_id: Option<DeckId>) -> Result<BacklogReport> {
        self.rollover()?;
        backlog_report(&self.core, deck_id)
    }

    /// Applies a catch-up plan; a running session is refilled when cards moved.
    pub fn apply_catch_up(&mut self, deck_id: Option<DeckId>, plan: &CatchUpPlan) -> Result<CatchUpResult> {
        let r = apply_catch_up(&self.core, deck_id, plan)?;
        if r.postponed > 0 {
            if let Some(s) = self.session.as_mut() {
                fill_session(&self.core, s)?;
            }
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_temporary_retention_marker() {
        let raw = parse_json_object(&json!({ TEMP_RETENTION_KEY: { "original": 0.9, "untilDay": 12 } }).to_string());
        assert_eq!(read_temporary_retention(&raw), Some(TempRetention { original: 0.9, retention: 0.9, until_day: 12 }));
        let raw = parse_json_object(&json!({ TEMP_RETENTION_KEY: { "original": "x", "untilDay": 12 } }).to_string());
        assert_eq!(read_temporary_retention(&raw), None);
        assert_eq!(read_temporary_retention(&Map::new()), None);
    }

    #[test]
    fn retrievability_uses_memory_state_or_interval() {
        let clock = SchedClock {
            now_ms: 86_400_000 * 10,
            today: 10,
            crt: 0,
            rollover_hour: 4,
            next_day_at: 86_400 * 11,
            learn_ahead_secs: 0,
        };
        let mut card = Card {
            id: 1,
            nid: 1,
            did: 1,
            ord: 0,
            mtime: 0,
            usn: -1,
            ctype: CardType::Review,
            queue: CardQueue::Review,
            due: 10,
            ivl: 10,
            factor: 2500,
            reps: 1,
            lapses: 0,
            left: 0,
            odue: 0,
            odid: 0,
            flags: 0,
            data: CardData::default(),
        };
        let params = crate::collection::presets::FSRS6_DEFAULT_PARAMS;
        // Due today after a 10-day interval: elapsed = 10 days with S = 10 → 90% by definition.
        let r = card_retrievability(&card, &clock, &params).unwrap();
        assert!((r - 0.9).abs() < 1e-9, "{r}");
        card.data.s = Some(100.0);
        card.data.lrt = Some(86_400 * 9);
        assert!(card_retrievability(&card, &clock, &params).unwrap() > 0.95);
    }
}
