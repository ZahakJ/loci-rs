//! FSRS optimiser wiring (DESIGN §7; recall `src/core/collection/optimise.ts`): per-preset
//! training items from the revlog, optimisation with Anki's keep-if-better rule, `optimise_log`
//! rows, memory-state refresh, evaluation, workload forecasts, and the idle-time maintenance pass
//! (temporary-retention expiry, automatic optimisation every N reviews, the 30-minute backup rule).

use crate::backup::backup::auto_backup;
use crate::collection::backlog::expire_temporary_retention;
use crate::collection::internals::{meta, CollectionCore};
use crate::collection::presets::normalise_deck_config;
use crate::collection::rows::{parse_json_object, row_to_card, row_to_revlog, select_sql, CARD_COLUMNS, REVLOG_COLUMNS};
use crate::collection::sched_context::{marks, sched_clock, subtree_deck_ids, SQL_CHUNK};
use crate::collection::service::Collection;
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::scheduler::{
    decay_from_params, deck_filter_sql, evaluate as evaluate_params, memory_state_from_revlog, normalize_parameters,
    optimise as optimise_params, seconds_per_card, simulate_workload, training_items_from_revlog, CardSched, OptimizeOptions,
    RevlogConversionInput, WorkloadInput,
};
use crate::types::*;
use crate::undo::{self, PresetRow, RowSnapshot, Table, UndoSnapshot};
use std::collections::HashMap;

/// Decks using a preset.
fn decks_of_preset(core: &CollectionCore, config_id: DeckConfigId) -> Result<Vec<DeckId>> {
    Ok(core.all_decks()?.into_iter().filter(|d| d.config_id == config_id).map(|d| d.id).collect())
}

/// Graded and manual revlog rows of the cards in the preset's decks, oldest first.
fn preset_revlog(core: &CollectionCore, config_id: DeckConfigId, since_ms: i64) -> Result<Vec<RevlogEntry>> {
    let decks = decks_of_preset(core, config_id)?;
    let (f, mut params) = deck_filter_sql(Some(&decks), "c.did");
    params.push(since_ms);
    let cols: Vec<String> = REVLOG_COLUMNS.iter().map(|c| format!("r.\"{c}\"")).collect();
    let sql =
        format!("SELECT {} FROM revlog r JOIN cards c ON c.id = r.cid WHERE {f} AND r.id > ? ORDER BY r.id", cols.join(", "));
    core.db().all(&sql, rusqlite::params_from_iter(params), row_to_revlog)
}

/// Relearning steps that fit within one day (they lift stability in FSRS's short-term model).
pub fn relearning_steps_within_day(config: &DeckConfig) -> usize {
    let mut total = 0.0;
    let mut n = 0;
    for m in &config.relearn_steps {
        total += m;
        if total >= 1440.0 {
            break;
        }
        n += 1;
    }
    n
}

fn preset_row(core: &CollectionCore, id: DeckConfigId) -> Result<PresetRow> {
    core.db()
        .get("SELECT id, name, config, mtime, usn FROM deck_configs WHERE id = ?", [id], |r| {
            Ok(PresetRow { id: r.get(0)?, name: r.get(1)?, config: r.get(2)?, mtime: r.get(3)?, usn: r.get(4)? })
        })?
        .ok_or_else(|| LociError::not_found(format!("Preset {id} not found")))
}

fn graded_count(revlog: &[RevlogEntry]) -> i64 {
    revlog.iter().filter(|r| (1..=4).contains(&r.ease)).count() as i64
}

/// Optimises a preset's FSRS parameters. The new set is adopted only when its log loss beats
/// the current one (fsrs-rs/Anki acceptance rule); either way one optimise_log row is written.
/// Adopting refreshes every card's memory state from its revlog with the new parameters and
/// (for an explicit call) records an undo entry that restores the old parameters and memory
/// states. The result carries both parameter sets and `adopted`, so the UI can refresh a draft.
pub fn optimise(
    core: &CollectionCore,
    config_id: DeckConfigId,
    progress: &mut dyn FnMut(Progress),
    undoable: bool,
) -> Result<OptimiseResult> {
    let preset = core.preset(config_id)?;
    let clock = sched_clock(core)?;
    let revlog = preset_revlog(core, config_id, 0)?;
    let items = training_items_from_revlog(&revlog, clock.next_day_at * 1000, 0);
    let current = normalize_parameters(&preset.config.fsrs_params)?;
    let graded = graded_count(&revlog);
    let out = {
        let mut opts = OptimizeOptions {
            progress: Some(progress),
            num_relearning_steps: Some(relearning_steps_within_day(&preset.config)),
            ..Default::default()
        };
        optimise_params(&items, &current, &mut opts)?
    };
    if out.cancelled {
        return Err(LociError::busy("Optimisation was cancelled"));
    }
    let result = OptimiseResult {
        config_id,
        reviews: graded,
        before: ParamsMetrics { params: current.clone(), log_loss: out.before.log_loss, rmse: out.before.rmse },
        after: ParamsMetrics { params: out.params.clone(), log_loss: out.after.log_loss, rmse: out.after.rmse },
        adopted: out.adopted,
        reason: if items.len() < 8 {
            msg("optimise.tooFew", "Not enough reviews yet; the default parameters are kept.", &[])
        } else {
            out.reason.clone()
        },
    };
    core.db().transaction(|| {
        if out.adopted {
            let row = preset_row(core, config_id)?;
            let mut raw = parse_json_object(&row.config);
            raw.insert("fsrsParams".into(), serde_json::json!(out.params));
            core.db().run(
                "UPDATE deck_configs SET config = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![serde_json::Value::Object(raw).to_string(), core.now_seconds(), config_id],
            )?;
            let cards_before = refresh_memory_states(core, config_id, &out.params)?;
            // Undo restores the previous parameters and every refreshed memory state. Automatic
            // optimisation (maintenance) is not put on the stack, so `z` during study keeps undoing answers.
            if undoable {
                undo::record(
                    core.db(),
                    core.now(),
                    &msg("optimise.undo", "Optimise parameters", &[]),
                    &UndoSnapshot { presets: vec![row], cards: cards_before, ..Default::default() },
                )?;
            }
        }
        core.db().run(
            "INSERT INTO optimise_log (config_id, at, reviews, before, after, adopted) VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                config_id,
                core.now_seconds(),
                graded,
                serde_json::to_string(&result.before)?,
                serde_json::to_string(&result.after)?,
                out.adopted as i64
            ],
        )?;
        core.set_meta(meta::LAST_OPTIMISE, Some(&core.now_seconds().to_string()))?;
        core.mark_changed()
    })?;
    Ok(result)
}

fn round_dp(x: f64, dp: i32) -> f64 {
    let f = 10f64.powi(dp);
    (x * f).round() / f
}

/// Recomputes data.s/d/decay of every studied card in the preset from its revlog with `params`
/// (Anki updates memory states when parameters change; due dates are left as they are).
fn refresh_memory_states(core: &CollectionCore, config_id: DeckConfigId, params: &[f64]) -> Result<Vec<RowSnapshot>> {
    let config = core.preset(config_id)?.config;
    if config.scheduler != SchedulerKind::Fsrs {
        return Ok(vec![]);
    }
    let clock = sched_clock(core)?;
    let decay = decay_from_params(&normalize_parameters(params)?);
    let (f, fparams) = deck_filter_sql(Some(&decks_of_preset(core, config_id)?), "did");
    let cards: Vec<Card> = core.db().all(
        &format!("{} WHERE {f} AND type != {}", select_sql("cards", &CARD_COLUMNS), CardType::New as i64),
        rusqlite::params_from_iter(fparams),
        row_to_card,
    )?;
    let mut by_card: HashMap<CardId, Vec<RevlogEntry>> = HashMap::new();
    let ids: Vec<i64> = cards.iter().map(|c| c.id).collect();
    for part in ids.chunks(SQL_CHUNK) {
        let rows = core.db().all(
            &format!("{} WHERE cid IN ({}) ORDER BY id", select_sql("revlog", &REVLOG_COLUMNS), marks(part.len())),
            rusqlite::params_from_iter(part.iter()),
            row_to_revlog,
        )?;
        for r in rows {
            by_card.entry(r.cid).or_default().push(r);
        }
    }
    let empty: Vec<RevlogEntry> = Vec::new();
    let mut updates: Vec<(CardId, CardData)> = Vec::new();
    for c in &cards {
        let m = memory_state_from_revlog(&RevlogConversionInput {
            card: CardSched { ctype: c.ctype, ivl: c.ivl, factor: c.factor },
            revlog: by_card.get(&c.id).unwrap_or(&empty),
            params,
            historical_retention: config.historical_retention,
            next_day_at_ms: clock.next_day_at * 1000,
            ignore_before_ms: 0,
        })?;
        let Some(m) = m else { continue };
        let data = CardData {
            s: Some(round_dp(m.stability, 4)),
            d: Some(round_dp(m.difficulty, 3)),
            decay: Some(round_dp(decay, 3)),
            ..c.data.clone()
        };
        if data == c.data {
            continue;
        }
        updates.push((c.id, data));
    }
    let changed_ids: Vec<i64> = updates.iter().map(|(id, _)| *id).collect();
    let before = undo::snapshot_rows(core.db(), Table::Cards, &changed_ids)?;
    for (id, data) in &updates {
        core.db().run("UPDATE cards SET data = ? WHERE id = ?", rusqlite::params![serde_json::to_string(data)?, id])?;
    }
    Ok(before)
}

/// Log loss and RMSE(bins) of the preset's current parameters on its review history.
pub fn evaluate(core: &CollectionCore, config_id: DeckConfigId) -> Result<Evaluation> {
    let preset = core.preset(config_id)?;
    let revlog = preset_revlog(core, config_id, 0)?;
    let items = training_items_from_revlog(&revlog, sched_clock(core)?.next_day_at * 1000, 0);
    let m = evaluate_params(&items, &preset.config.fsrs_params)?;
    Ok(Evaluation { log_loss: m.log_loss, rmse: m.rmse, reviews: graded_count(&revlog) })
}

/// Graded reviews of a preset since its last optimisation (all, when never optimised).
pub fn reviews_since_optimise(core: &CollectionCore, config_id: DeckConfigId) -> Result<i64> {
    let last: Option<i64> =
        core.db().scalar("SELECT at FROM optimise_log WHERE config_id = ? ORDER BY id DESC LIMIT 1", [config_id])?;
    let (f, mut params) = deck_filter_sql(Some(&decks_of_preset(core, config_id)?), "c.did");
    params.push(last.map(|at| at * 1000).unwrap_or(0));
    core.db().count(
        &format!("SELECT count(*) FROM revlog r JOIN cards c ON c.id = r.cid WHERE {f} AND r.id > ? AND r.ease BETWEEN 1 AND 4"),
        rusqlite::params_from_iter(params),
    )
}

/// Result of the idle-time maintenance pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Maintenance {
    pub optimised: Vec<OptimiseResult>,
    pub backup: Option<BackupInfo>,
}

/// Idle-time maintenance: restores expired temporary retention, optimises at most ONE FSRS preset
/// that has auto-optimisation on and at least `optimise_every_reviews` new reviews since its last
/// run (the one with the most; the next idle call takes the next, so idle work stays short), then
/// applies the 30-minute backup rule.
pub fn maintenance(core: &CollectionCore) -> Result<Maintenance> {
    expire_temporary_retention(core)?;
    let mut optimised = Vec::new();
    let mut best: Option<(DeckConfigId, i64)> = None;
    let ids: Vec<i64> = core.db().all("SELECT id FROM deck_configs ORDER BY id", [], |r| r.get(0))?;
    for id in ids {
        let preset = core.preset(id)?;
        if !preset.config.auto_optimise || preset.config.scheduler != SchedulerKind::Fsrs {
            continue;
        }
        if decks_of_preset(core, id)?.is_empty() {
            continue;
        }
        let reviews = reviews_since_optimise(core, id)?;
        if reviews < preset.config.optimise_every_reviews.max(1) {
            continue;
        }
        if best.is_none_or(|(_, r)| reviews > r) {
            best = Some((id, reviews));
        }
    }
    if let Some((id, _)) = best {
        optimised.push(optimise(core, id, &mut |_| {}, false)?);
    }
    let backup = auto_backup(core)?;
    Ok(Maintenance { optimised, backup })
}

/// 90-day workload forecast for a deck (or everything) under `config`: expected answers per day
/// from the queue simulation on the actual cards, minutes from the recent seconds per answer.
pub fn forecast_workload(core: &CollectionCore, deck_id: Option<DeckId>, config: &DeckConfig) -> Result<WorkloadForecast> {
    let clock = sched_clock(core)?;
    let deck_ids = subtree_deck_ids(core, deck_id)?;
    let (f, params) = deck_filter_sql(deck_ids.as_deref(), "did");
    let cards: Vec<Card> = core.db().all(
        &format!("{} WHERE {f} AND queue != {}", select_sql("cards", &CARD_COLUMNS), CardQueue::Suspended as i64),
        rusqlite::params_from_iter(params),
        row_to_card,
    )?;
    let sim = simulate_workload(&WorkloadInput { cards: &cards, config, days: 90, today: clock.today, first_rating_prob: None })?;
    let pace = seconds_per_card(core.db(), clock.now_ms, deck_ids.as_deref())?;
    let reviews_per_day: Vec<f64> = sim.reviews_per_day.iter().map(|v| (v * 10.0).round() / 10.0).collect();
    let minutes_per_day: Vec<f64> = sim.reviews_per_day.iter().map(|v| (((v * pace) / 60.0) * 10.0).round() / 10.0).collect();
    let avg = |from: usize, to: usize| -> f64 {
        let s = &sim.reviews_per_day[from.min(sim.reviews_per_day.len())..to.min(sim.reviews_per_day.len())];
        if s.is_empty() {
            0.0
        } else {
            s.iter().sum::<f64>() / s.len() as f64
        }
    };
    let first_month = avg(0, 30);
    let sentence = msg(
        "forecast.sentence",
        "{reviews, plural, one {About # review} other {About # reviews}} a day over the next month (about {minutes} min), settling near {later} a day after that.",
        &[
            ("reviews", (first_month.round() as i64).into()),
            ("minutes", (((first_month * pace) / 60.0).round() as i64).into()),
            ("later", (avg(30, 90).round() as i64).into()),
        ],
    );
    Ok(WorkloadForecast { reviews_per_day, minutes_per_day, sentence })
}

impl Collection {
    pub fn optimise(&self, config_id: DeckConfigId, progress: &mut dyn FnMut(Progress)) -> Result<OptimiseResult> {
        optimise(&self.core, config_id, progress, true)
    }
    pub fn evaluate(&self, config_id: DeckConfigId) -> Result<Evaluation> {
        evaluate(&self.core, config_id)
    }
    /// Restores expired temporary retention, runs due auto-optimisations, then the 30-minute backup rule.
    pub fn maintenance(&self) -> Result<MaintenanceReport> {
        self.rollover()?;
        let m = maintenance(&self.core)?;
        Ok(MaintenanceReport { optimised: m.optimised, backed_up: m.backup.is_some() })
    }
    pub fn forecast_workload(&self, deck_id: Option<DeckId>, config: &DeckConfig) -> Result<WorkloadForecast> {
        forecast_workload(&self.core, deck_id, &normalise_deck_config(&serde_json::to_value(config)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relearning_steps_within_a_day() {
        let mut cfg = crate::collection::presets::standard_deck_config();
        cfg.relearn_steps = vec![10.0];
        assert_eq!(relearning_steps_within_day(&cfg), 1);
        cfg.relearn_steps = vec![10.0, 1440.0, 10.0];
        assert_eq!(relearning_steps_within_day(&cfg), 1);
        cfg.relearn_steps = vec![];
        assert_eq!(relearning_steps_within_day(&cfg), 0);
        assert_eq!(round_dp(1.23456, 3), 1.235);
    }
}
