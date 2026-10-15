//! Anki "Set due date" (rslib `scheduler/reviews.rs` `set_due_date`), shared by the browser's bulk action and the catch-up
//! planner.
//!
//! Semantics per card, for `days` from today:
//!   - due = today + days; type and queue become Review; `left` = 0; a new card remembers its
//!     position in data.pos; ease 0 becomes the preset's initial ease;
//!   - interval: with `force` ('!' suffix) or for a card that was not in review: max(1, days);
//!     otherwise the time since the last review plus `days` (FSRS, from data.lrt), or the old
//!     interval adjusted by how far the due date moved (SM-2): max(1, ivl + newDue − oldDue);
//!   - a revlog row with ease 0, ivl = new interval, lastIvl = old interval, factor = ease.

use crate::collection::cards::update_card;
use crate::collection::internals::{CollectionCore, IdTable};
use crate::collection::rows::{insert_sql, revlog_params, REVLOG_COLUMNS};
use crate::db::Db;
use crate::error::{LociError, Result};
use crate::scheduler::SchedClock;
use crate::types::*;
use std::sync::LazyLock;

/// Parsed Anki due-date string: '0' today, '1' tomorrow, '3-7' random in range, '!' = also reset interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueDateSpec {
    pub min: i64,
    pub max: i64,
    pub force: bool,
}

static SPEC: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\s*(\d+)\s*(?:-\s*(\d+))?\s*(!)?\s*$").expect("valid due-date regex"));

/// Parses a due-date spec (`0`, `3-7`, `5!`).
pub fn parse_due_date_spec(text: &str) -> Result<DueDateSpec> {
    let invalid = || LociError::invalid(format!("Invalid due date \"{text}\": use a number of days like 0, 1, 3-7 or 5!"));
    let m = SPEC.captures(text).ok_or_else(invalid)?;
    let a: i64 = m.get(1).and_then(|g| g.as_str().parse().ok()).ok_or_else(invalid)?;
    let b: i64 = match m.get(2) {
        Some(g) => g.as_str().parse().map_err(|_| invalid())?,
        None => a,
    };
    Ok(DueDateSpec { min: a.min(b), max: a.max(b), force: m.get(3).is_some() })
}

/// Inserts one revlog row.
pub(crate) fn insert_revlog(db: &Db, e: &RevlogEntry) -> Result<()> {
    db.run(&insert_sql("revlog", &REVLOG_COLUMNS), rusqlite::params_from_iter(revlog_params(e)))?;
    Ok(())
}

/// Reschedules one card to `days` from today (see module doc). Writes the card and its revlog
/// row; the caller owns the transaction and the undo snapshot. Returns the new card and revlog.
pub fn set_due_date(
    core: &CollectionCore,
    card: &Card,
    days: i64,
    clock: &SchedClock,
    force: bool,
    revlog_type: RevlogType,
) -> Result<(Card, RevlogEntry)> {
    let today = clock.today;
    let new_due = today + days;
    let was_review = matches!(card.ctype, CardType::Review | CardType::Relearn);
    let ivl = if force || !was_review {
        days.max(1)
    } else if let (Some(lrt), Some(_)) = (card.data.lrt.filter(|l| *l > 0), card.data.s) {
        let elapsed = (today - clock.day_of_ms(lrt * 1000)).max(0);
        (elapsed + days).max(1)
    } else {
        let old_due_day = if card.due > 1_000_000_000 { today } else { card.due };
        (card.ivl + new_due - old_due_day).max(1)
    };
    let mut data = card.data.clone();
    if card.ctype == CardType::New && data.pos.is_none() {
        data.pos = Some(card.due);
    }
    let factor = if card.factor > 0 { card.factor } else { core.deck_config(card.did)?.initial_ease };
    let next = Card {
        ctype: CardType::Review,
        queue: CardQueue::Review,
        due: new_due,
        ivl,
        factor,
        left: 0,
        odue: 0,
        odid: 0,
        mtime: core.now_seconds(),
        usn: -1,
        data,
        ..card.clone()
    };
    update_card(core, &next)?;
    let revlog = RevlogEntry {
        id: core.new_id(IdTable::Revlog)?,
        cid: card.id,
        usn: -1,
        ease: 0,
        ivl,
        last_ivl: card.ivl,
        factor,
        time: 0,
        rtype: revlog_type,
    };
    insert_revlog(core.db(), &revlog)?;
    Ok((next, revlog))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_anki_due_date_strings() {
        assert_eq!(parse_due_date_spec("0").unwrap(), DueDateSpec { min: 0, max: 0, force: false });
        assert_eq!(parse_due_date_spec(" 3-7 ").unwrap(), DueDateSpec { min: 3, max: 7, force: false });
        assert_eq!(parse_due_date_spec("7-3!").unwrap(), DueDateSpec { min: 3, max: 7, force: true });
        assert_eq!(parse_due_date_spec("5!").unwrap(), DueDateSpec { min: 5, max: 5, force: true });
        assert_eq!(parse_due_date_spec("x").unwrap_err().code, crate::error::ErrorCode::Invalid);
        assert_eq!(parse_due_date_spec("-1").unwrap_err().code, crate::error::ErrorCode::Invalid);
    }
}
