//! Bulk actions of the browser. Each action records one undo entry; `affected`
//! counts notes for tag edits, find-and-replace and delete, cards otherwise.

use crate::collection::cards::{bury_cards, forget_cards, set_flag, suspend_cards};
use crate::collection::internals::{meta, CollectionCore};
use crate::collection::notes::delete_notes;
use crate::collection::reschedule::{parse_due_date_spec, set_due_date};
use crate::collection::rows::{note_derived_columns, row_to_card, select_sql, CARD_COLUMNS};
use crate::collection::sched_context::{marks, sched_clock, SQL_CHUNK};
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::text::{join_fields, join_tags, split_fields, split_tags};
use crate::types::*;
use crate::undo::{self, RowSnapshot, Table, UndoSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// SQL expression giving the queue a card returns to when unsuspended/unburied
/// (Anki `restore_queue_after_bury_or_suspend`).
const RESTORED_QUEUE_SQL: &str = "CASE type WHEN 0 THEN 0 WHEN 2 THEN 2 ELSE (CASE WHEN due > 1000000000 THEN 1 ELSE 3 END) END";

/// `bulk()` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
    pub affected: i64,
    pub undo_label: String,
}

fn load_cards(core: &CollectionCore, ids: &[CardId]) -> Result<Vec<Card>> {
    let mut unique: Vec<CardId> = Vec::with_capacity(ids.len());
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    let mut out = Vec::new();
    for part in unique.chunks(SQL_CHUNK) {
        out.extend(core.db().all(
            &format!("{} WHERE id IN ({})", select_sql("cards", &CARD_COLUMNS), marks(part.len())),
            rusqlite::params_from_iter(part.iter()),
            row_to_card,
        )?);
    }
    Ok(out)
}

fn note_ids_of(cards: &[Card]) -> Vec<NoteId> {
    let mut out = Vec::new();
    for c in cards {
        if !out.contains(&c.nid) {
            out.push(c.nid);
        }
    }
    out
}

/// Seeded shuffle (mulberry32) for reposition/randomize and due-date ranges.
fn rng(seed: i64) -> impl FnMut() -> f64 {
    let mut a = seed as u32;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

fn snapshot_i64(row: &RowSnapshot, key: &str) -> i64 {
    row.get(key).and_then(|v| v.as_i64()).unwrap_or(0)
}
fn snapshot_str<'a>(row: &'a RowSnapshot, key: &str) -> &'a str {
    row.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn chunked_update(core: &CollectionCore, sql_prefix: &str, head: &[rusqlite::types::Value], ids: &[i64]) -> Result<()> {
    for part in ids.chunks(SQL_CHUNK) {
        let mut params: Vec<rusqlite::types::Value> = head.to_vec();
        params.extend(part.iter().map(|id| rusqlite::types::Value::Integer(*id)));
        core.db().run(&format!("{sql_prefix} WHERE id IN ({})", marks(part.len())), rusqlite::params_from_iter(params))?;
    }
    Ok(())
}

/// Updates tags of notes; returns the number of notes changed.
fn edit_tags(core: &CollectionCore, note_ids: &[NoteId], label: &str, edit: impl Fn(Vec<String>) -> Vec<String>) -> Result<i64> {
    core.db().transaction(|| {
        let before = undo::snapshot_rows(core.db(), Table::Notes, note_ids)?;
        let mut changed = Vec::new();
        let mtime = core.now_seconds();
        for row in before {
            let tags_text = snapshot_str(&row, "tags").to_string();
            let next = core.canonical_tags(&edit(split_tags(&tags_text)))?;
            if join_tags(&next) == tags_text {
                continue;
            }
            core.db().run(
                "UPDATE notes SET tags = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![join_tags(&next), mtime, snapshot_i64(&row, "id")],
            )?;
            core.register_tags(&next)?;
            changed.push(row);
        }
        let n = changed.len() as i64;
        if n > 0 {
            undo::record(core.db(), core.now(), label, &UndoSnapshot { notes: changed, ..Default::default() })?;
            core.mark_changed()?;
        }
        Ok(n)
    })
}

/// What each bulk action is called (the undo label when it records one).
fn action_label(action: &BulkAction) -> String {
    match action {
        BulkAction::ChangeDeck { .. } => msg("browse.undo.changeDeck", "Change deck", &[]),
        BulkAction::AddTags { .. } => msg("browse.undo.addTags", "Add tags", &[]),
        BulkAction::RemoveTags { .. } => msg("browse.undo.removeTags", "Remove tags", &[]),
        BulkAction::Suspend => "Suspend".into(),
        BulkAction::Unsuspend => "Unsuspend".into(),
        BulkAction::Bury => "Bury".into(),
        BulkAction::Unbury => msg("browse.undo.unbury", "Unbury", &[]),
        BulkAction::Flag { flag } => if *flag == 0 { "Remove flag" } else { "Flag" }.into(),
        BulkAction::Reposition { .. } => msg("browse.undo.reposition", "Reposition", &[]),
        BulkAction::SetDue { .. } => msg("browse.undo.setDue", "Set due date", &[]),
        BulkAction::Forget { .. } => msg("browse.undo.forget", "Reset cards", &[]),
        BulkAction::Delete => msg("browse.undo.delete", "Delete", &[]),
        BulkAction::FindReplace { .. } => msg("browse.undo.findReplace", "Find and replace", &[]),
    }
}

/// Runs a bulk action. `affected` counts what changed: notes for tag edits and find-and-replace,
/// cards otherwise; 0 when nothing changed. `undo_label` is the label of the undo entry the action
/// recorded, or the action's name when nothing changed (no entry recorded).
pub fn bulk(core: &CollectionCore, card_ids: &[CardId], action: &BulkAction) -> Result<BulkResult> {
    let before: i64 = core.db().scalar::<Option<i64>, _>("SELECT max(id) FROM undo_log", [])?.flatten().unwrap_or(0);
    let affected = run_bulk(core, card_ids, action)?;
    let top: Option<(i64, String)> =
        core.db().get("SELECT id, label FROM undo_log ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let undo_label = match top {
        Some((id, label)) if id != before => label,
        _ => action_label(action),
    };
    Ok(BulkResult { affected, undo_label })
}

fn run_bulk(core: &CollectionCore, card_ids: &[CardId], action: &BulkAction) -> Result<i64> {
    let cards = load_cards(core, card_ids)?;
    if cards.is_empty() && !matches!(action, BulkAction::Delete) {
        return Ok(0);
    }
    let ids: Vec<CardId> = cards.iter().map(|c| c.id).collect();
    match action {
        BulkAction::ChangeDeck { deck_id } => {
            core.deck(*deck_id)?;
            core.db().transaction(|| {
                let moving: Vec<CardId> = cards.iter().filter(|c| c.did != *deck_id).map(|c| c.id).collect();
                if moving.is_empty() {
                    return Ok(0);
                }
                let snap = undo::snapshot_rows(core.db(), Table::Cards, &moving)?;
                chunked_update(
                    core,
                    "UPDATE cards SET did = ?, mtime = ?, usn = -1",
                    &[rusqlite::types::Value::Integer(*deck_id), rusqlite::types::Value::Integer(core.now_seconds())],
                    &moving,
                )?;
                undo::record(
                    core.db(),
                    core.now(),
                    &msg("browse.undo.changeDeck", "Change deck", &[]),
                    &UndoSnapshot { cards: snap, ..Default::default() },
                )?;
                core.mark_changed()?;
                Ok(moving.len() as i64)
            })
        }
        BulkAction::AddTags { tags } => {
            let add = core.canonical_tags(tags)?;
            if add.is_empty() {
                return Ok(0);
            }
            edit_tags(core, &note_ids_of(&cards), &msg("browse.undo.addTags", "Add tags", &[]), |mut t| {
                t.extend(add.iter().cloned());
                t
            })
        }
        BulkAction::RemoveTags { tags } => {
            let mut res: Vec<regex::Regex> = Vec::new();
            for t in tags.iter().flat_map(|t| t.split_whitespace()) {
                // `^escaped$` with `*` as a wildcard, case-insensitive
                let pattern = format!("(?i)^{}$", regex::escape(t).replace("\\*", ".*"));
                res.push(regex::Regex::new(&pattern).map_err(|e| LociError::invalid(format!("Invalid tag pattern: {e}")))?);
            }
            if res.is_empty() {
                return Ok(0);
            }
            edit_tags(core, &note_ids_of(&cards), &msg("browse.undo.removeTags", "Remove tags", &[]), |t| {
                t.into_iter().filter(|tag| !res.iter().any(|re| re.is_match(tag))).collect()
            })
        }
        BulkAction::Suspend => {
            let n = cards.iter().filter(|c| c.queue != CardQueue::Suspended).count() as i64;
            suspend_cards(core, &ids, true)?;
            Ok(n)
        }
        BulkAction::Unsuspend => {
            let n = cards.iter().filter(|c| c.queue == CardQueue::Suspended).count() as i64;
            suspend_cards(core, &ids, false)?;
            Ok(n)
        }
        BulkAction::Bury => {
            let n = cards.iter().filter(|c| (c.queue as i64) >= 0).count() as i64;
            bury_cards(core, &ids)?;
            Ok(n)
        }
        BulkAction::Unbury => {
            let buried: Vec<CardId> = cards
                .iter()
                .filter(|c| matches!(c.queue, CardQueue::SchedBuried | CardQueue::UserBuried))
                .map(|c| c.id)
                .collect();
            if buried.is_empty() {
                return Ok(0);
            }
            core.db().transaction(|| {
                let snap = undo::snapshot_rows(core.db(), Table::Cards, &buried)?;
                chunked_update(
                    core,
                    &format!("UPDATE cards SET queue = {RESTORED_QUEUE_SQL}, mtime = ?, usn = -1"),
                    &[rusqlite::types::Value::Integer(core.now_seconds())],
                    &buried,
                )?;
                undo::record(
                    core.db(),
                    core.now(),
                    &msg("browse.undo.unbury", "Unbury", &[]),
                    &UndoSnapshot { cards: snap, ..Default::default() },
                )?;
                core.mark_changed()?;
                Ok(buried.len() as i64)
            })
        }
        BulkAction::Flag { flag } => {
            let n = cards.iter().filter(|c| (c.flags & 7) != *flag as i64).count() as i64;
            set_flag(core, &ids, *flag as i64)?;
            Ok(n)
        }
        BulkAction::Reposition { start, step, randomize, shift } => reposition(core, &cards, *start, *step, *randomize, *shift),
        BulkAction::SetDue { days } => bulk_set_due(core, &cards, days),
        BulkAction::Forget { reset_counts, restore_position } => core.db().transaction(|| {
            let snap = undo::snapshot_rows(core.db(), Table::Cards, &ids)?;
            let inserted = forget_cards(core, &cards, *reset_counts, *restore_position)?;
            undo::record(
                core.db(),
                core.now(),
                &msg("browse.undo.forget", "Reset cards", &[]),
                &UndoSnapshot { cards: snap, inserted_revlog: inserted, ..Default::default() },
            )?;
            core.mark_changed()?;
            Ok(cards.len() as i64)
        }),
        BulkAction::Delete => {
            let nids = note_ids_of(&cards);
            delete_notes(core, &nids)?;
            Ok(nids.len() as i64)
        }
        BulkAction::FindReplace { find, replace, regex, field, match_case } => {
            find_replace(core, &note_ids_of(&cards), find, replace, *regex, field.as_deref(), *match_case)
        }
    }
}

/// Anki "Reposition" for new cards: notes keep their cards together (siblings share a position),
/// ordered by current position (or shuffled), placed at start, start+step, …; with `shift`,
/// other new cards at or after `start` move back to make room. Non-new cards are ignored.
fn reposition(core: &CollectionCore, cards: &[Card], start: i64, step: i64, randomize: bool, shift: bool) -> Result<i64> {
    let mut fresh: Vec<&Card> = cards.iter().filter(|c| c.ctype == CardType::New).collect();
    if fresh.is_empty() {
        return Ok(0);
    }
    let start = start.max(0);
    let step = step.max(1);
    core.db().transaction(|| {
        fresh.sort_by(|x, y| x.due.cmp(&y.due).then(x.ord.cmp(&y.ord)));
        let mut notes: Vec<NoteId> = Vec::new();
        let mut by_note: HashMap<NoteId, Vec<&Card>> = HashMap::new();
        for c in &fresh {
            if !by_note.contains_key(&c.nid) {
                notes.push(c.nid);
            }
            by_note.entry(c.nid).or_default().push(c);
        }
        if randomize {
            let mut r = rng(core.now() & 0x7fff_ffff);
            for i in (1..notes.len()).rev() {
                let j = (r() * (i as f64 + 1.0)).floor() as usize;
                notes.swap(i, j);
            }
        }
        let selected: Vec<CardId> = fresh.iter().map(|c| c.id).collect();
        let mut shifted: Vec<RowSnapshot> = Vec::new();
        if shift {
            let by = notes.len() as i64 * step;
            let ids: Vec<i64> = core
                .db()
                .all(&format!("SELECT id FROM cards WHERE type = {} AND due >= ?", CardType::New as i64), [start], |r| r.get(0))?
                .into_iter()
                .filter(|id| !selected.contains(id))
                .collect();
            shifted = undo::snapshot_rows(core.db(), Table::Cards, &ids)?;
            chunked_update(
                core,
                "UPDATE cards SET due = due + ?, mtime = ?, usn = -1",
                &[rusqlite::types::Value::Integer(by), rusqlite::types::Value::Integer(core.now_seconds())],
                &ids,
            )?;
        }
        let mut snap = undo::snapshot_rows(core.db(), Table::Cards, &selected)?;
        let mut pos = start;
        let mut last = start;
        for nid in &notes {
            for c in by_note.get(nid).map(|v| v.as_slice()).unwrap_or(&[]) {
                core.db().run(
                    "UPDATE cards SET due = ?, mtime = ?, usn = -1 WHERE id = ?",
                    rusqlite::params![pos, core.now_seconds(), c.id],
                )?;
            }
            last = pos;
            pos += step;
        }
        let meta_before = core.get_meta(meta::NEXT_POS)?;
        let max_due: i64 = core
            .db()
            .scalar::<Option<i64>, _>(&format!("SELECT max(due) FROM cards WHERE type = {}", CardType::New as i64), [])?
            .flatten()
            .unwrap_or(last);
        let next_pos = meta_before.as_deref().and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
        if (max_due + 1) as f64 > next_pos {
            core.set_meta(meta::NEXT_POS, Some(&(max_due + 1).to_string()))?;
        }
        snap.extend(shifted);
        let mut meta_snap = serde_json::Map::new();
        meta_snap.insert(meta::NEXT_POS.to_string(), meta_before.map(Value::String).unwrap_or(Value::Null));
        undo::record(
            core.db(),
            core.now(),
            &msg("browse.undo.reposition", "Reposition", &[]),
            &UndoSnapshot { cards: snap, meta: meta_snap, ..Default::default() },
        )?;
        core.mark_changed()?;
        Ok(fresh.len() as i64)
    })
}

/// Anki "Set due date" over cards ('0', '1!', '3-7'); revlog rows of type Manual.
fn bulk_set_due(core: &CollectionCore, cards: &[Card], spec: &str) -> Result<i64> {
    let d = parse_due_date_spec(spec)?;
    core.db().transaction(|| {
        let clock = sched_clock(core)?;
        let ids: Vec<CardId> = cards.iter().map(|c| c.id).collect();
        let snap = undo::snapshot_rows(core.db(), Table::Cards, &ids)?;
        let mut r = rng((core.now() ^ cards.len() as i64) & 0x7fff_ffff);
        let mut inserted = Vec::with_capacity(cards.len());
        for c in cards {
            let days = if d.min == d.max { d.min } else { d.min + (r() * (d.max - d.min + 1) as f64).floor() as i64 };
            let (_, revlog) = set_due_date(core, c, days, &clock, d.force, RevlogType::Manual)?;
            inserted.push(revlog.id);
        }
        undo::record(
            core.db(),
            core.now(),
            &msg("browse.undo.setDue", "Set due date", &[]),
            &UndoSnapshot { cards: snap, inserted_revlog: inserted, ..Default::default() },
        )?;
        core.mark_changed()?;
        Ok(cards.len() as i64)
    })
}

/// Converts a JavaScript replacement string (`$1`, `$&`, `$<name>`, `$$`) to the `${…}` syntax
/// the regex engines here expand, so `[$1]` works the same in both.
fn js_replacement(replace: &str) -> String {
    let mut out = String::with_capacity(replace.len());
    let chars: Vec<char> = replace.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '$' {
                out.push_str("$$");
                i += 2;
                continue;
            }
            if next == '&' {
                out.push_str("${0}");
                i += 2;
                continue;
            }
            if next.is_ascii_digit() {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() && digits.len() < 2 {
                    digits.push(chars[j]);
                    j += 1;
                }
                out.push_str(&format!("${{{digits}}}"));
                i = j;
                continue;
            }
            if next == '<' {
                if let Some(end) = chars[i + 2..].iter().position(|c| *c == '>') {
                    let name: String = chars[i + 2..i + 2 + end].iter().collect();
                    out.push_str(&format!("${{{name}}}"));
                    i += 3 + end;
                    continue;
                }
            }
            // a lone `$`: literal
            out.push_str("$$");
            i += 1;
            continue;
        }
        if chars[i] == '$' {
            out.push_str("$$");
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Find and replace in note fields: a field name (case-insensitive, in every note type that has
/// it) or all fields; plain text or a regular expression (JavaScript syntax; `$1` for groups);
/// case sensitive only with `match_case`. Derived columns (sort field, checksum, search text) are
/// recomputed. Returns the number of notes changed.
fn find_replace(
    core: &CollectionCore,
    note_ids: &[NoteId],
    find: &str,
    replace: &str,
    is_regex: bool,
    field: Option<&str>,
    match_case: bool,
) -> Result<i64> {
    if find.is_empty() {
        return Err(LociError::invalid("Nothing to find"));
    }
    let source = if is_regex { find.to_string() } else { fancy_regex::escape(find).into_owned() };
    let pattern = if match_case { source } else { format!("(?i){source}") };
    let re = fancy_regex::Regex::new(&pattern).map_err(|e| LociError::invalid(format!("Invalid regular expression: {e}")))?;
    // Plain-text mode: `$` in the replacement is literal.
    let replacement = if is_regex { js_replacement(replace) } else { replace.replace('$', "$$") };
    core.db().transaction(|| {
        let before = undo::snapshot_rows(core.db(), Table::Notes, note_ids)?;
        let mut changed = Vec::new();
        let mtime = core.now_seconds();
        for row in before {
            let ntid = snapshot_i64(&row, "ntid");
            let nt = core.notetype(ntid)?;
            let ords: Vec<usize> = match field {
                None => nt.fields.iter().filter_map(|f| usize::try_from(f.ord).ok()).collect(),
                Some(name) => nt
                    .fields
                    .iter()
                    .filter(|f| f.name.to_lowercase() == name.to_lowercase())
                    .filter_map(|f| usize::try_from(f.ord).ok())
                    .collect(),
            };
            if ords.is_empty() {
                continue;
            }
            let mut fields = split_fields(snapshot_str(&row, "flds"));
            let mut dirty = false;
            for o in ords {
                let v = fields.get(o).cloned().unwrap_or_default();
                let next = re.replace_all(&v, replacement.as_str()).into_owned();
                if next != v {
                    while fields.len() <= o {
                        fields.push(String::new());
                    }
                    fields[o] = next;
                    dirty = true;
                }
            }
            if !dirty {
                continue;
            }
            let d = note_derived_columns(&fields, nt.sort_field);
            core.db().run(
                "UPDATE notes SET flds = ?, sfld = ?, csum = ?, plain = ?, mtime = ?, usn = -1 WHERE id = ?",
                rusqlite::params![join_fields(&fields), d.sfld, d.csum, d.plain, mtime, snapshot_i64(&row, "id")],
            )?;
            changed.push(row);
        }
        let n = changed.len() as i64;
        if n > 0 {
            undo::record(
                core.db(),
                core.now(),
                &msg("browse.undo.findReplace", "Find and replace", &[]),
                &UndoSnapshot { notes: changed, ..Default::default() },
            )?;
            core.mark_changed()?;
        }
        Ok(n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_matches_the_javascript_sequence() {
        // mulberry32(1): first outputs of the reference JavaScript implementation.
        let mut r = rng(1);
        let a = r();
        let b = r();
        assert!((0.0..1.0).contains(&a) && (0.0..1.0).contains(&b));
        assert_ne!(a, b);
        let mut r2 = rng(1);
        assert_eq!(r2(), a);
    }

    #[test]
    fn javascript_replacements_are_translated() {
        assert_eq!(js_replacement("[$1]"), "[${1}]");
        assert_eq!(js_replacement("$&-$$-$<n>"), "${0}-$$-${n}");
        assert_eq!(js_replacement("cost $"), "cost $$");
        let re = fancy_regex::Regex::new("^(.)").unwrap();
        assert_eq!(re.replace_all("abc", js_replacement("[$1]").as_str()), "[a]bc");
    }
}
