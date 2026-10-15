//! Study sessions (internals §6): queue building, answering,
//! sibling burying, leeches, deck "today" counters, undo and the session summary.
//!
//! A session keeps Anki v3's three queues in memory:
//!   - `main`: interday learning, reviews and new cards, mixed per the deck options
//!     (built once by `scheduler::build_queue`, which applies daily limits, parent limits,
//!     display orders and gather-time sibling burying);
//!   - `learning`: intraday learning cards (id → due, unix seconds);
//!
//! and picks: learning due now → main → learning within the learn-ahead limit. Answers update
//! the queues incrementally. Undo and a day rollover rebuild the queues from the database.

use crate::collection::cards::update_card;
use crate::collection::internals::{CollectionCore, IdTable};
use crate::collection::notes::{get_card, get_note};
use crate::collection::render::render_stored_card;
use crate::collection::reschedule::insert_revlog;
use crate::collection::rows::{card_queue_from_i64, deck_today_to_json, parse_deck_today};
use crate::collection::sched_context::{
    marks, queue_decks, sched_clock, scheduling_input, search_context, self_and_ancestors, subtree_deck_ids, SQL_CHUNK,
};
use crate::collection::service::Collection;
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::scheduler::{
    apply_answer, build_queue, deck_filter_sql, load_queue_cards, preview_intervals, siblings_to_bury, QueueInput, SchedClock,
};
use crate::search::{run_search, SearchMode};
use crate::stats::insights::session_insight;
use crate::text::join_tags;
use crate::types::*;
use crate::undo::{self, RowSnapshot, Table, UndoSnapshot};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Preview sessions show at most this many new cards (Anki's custom-study default is 100 days' worth; we cap the list).
const PREVIEW_LIMIT: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainKind {
    New,
    Review,
    Interday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueKind {
    Main(MainKind),
    Learning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MainEntry {
    id: CardId,
    kind: MainKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRecord {
    pub card_id: CardId,
    pub revlog_id: Option<RevlogId>,
    pub rating: Rating,
    pub ms: i64,
}

/// In-memory state of one study session.
#[derive(Debug, Default)]
pub struct StudySession {
    pub deck_id: Option<DeckId>,
    pub custom: Option<CustomStudyKind>,
    main: Vec<MainEntry>,
    /// Intraday learning cards: id → due (unix seconds).
    learning: BTreeMap<CardId, i64>,
    pub answers: Vec<AnswerRecord>,
    /// Cards answered in a custom session (excluded when it is rebuilt).
    done: HashSet<CardId>,
    last_answered: Option<CardId>,
    /// A card to show first (the card whose answer was just undone).
    priority: Option<CardId>,
    day: CollectionDay,
    pub started_ms: Millis,
}

impl StudySession {
    pub fn new(deck_id: Option<DeckId>, custom: Option<CustomStudyKind>, now_ms: Millis) -> StudySession {
        StudySession { deck_id, custom, day: -1, started_ms: now_ms, ..Default::default() }
    }

    pub fn is_preview(&self) -> bool {
        matches!(self.custom, Some(CustomStudyKind::Preview))
    }

    /// Number of cards left in the queues (tests).
    pub fn queued(&self) -> usize {
        self.main.len() + self.learning.len()
    }
}

// ───────────────────────── building ─────────────────────────

fn kind_of_queue(queue: CardQueue) -> Option<QueueKind> {
    match queue {
        CardQueue::New => Some(QueueKind::Main(MainKind::New)),
        CardQueue::Review => Some(QueueKind::Main(MainKind::Review)),
        CardQueue::DayLearn => Some(QueueKind::Main(MainKind::Interday)),
        CardQueue::Learn | CardQueue::Preview => Some(QueueKind::Learning),
        _ => None,
    }
}

fn push(s: &mut StudySession, id: CardId, queue: CardQueue, due: i64) {
    match kind_of_queue(queue) {
        Some(QueueKind::Learning) => {
            s.learning.insert(id, due);
        }
        Some(QueueKind::Main(kind)) => s.main.push(MainEntry { id, kind }),
        None => {}
    }
}

/// (Re)fills the session's queues from the database.
pub fn fill_session(core: &CollectionCore, s: &mut StudySession) -> Result<()> {
    let clock = sched_clock(core)?;
    s.day = clock.today;
    s.main.clear();
    s.learning.clear();
    if s.custom.is_some() {
        fill_custom(core, s, &clock)
    } else {
        fill_normal(core, s, &clock)
    }
}

fn fill_normal(core: &CollectionCore, s: &mut StudySession, clock: &SchedClock) -> Result<()> {
    let deck_ids = subtree_deck_ids(core, s.deck_id)?;
    let cards = load_queue_cards(core.db(), deck_ids.as_deref(), clock)?;
    let by_id: HashMap<CardId, &Card> = cards.iter().map(|c| (c.id, c)).collect();
    let decks = queue_decks(core, &core.all_decks()?)?;
    let res = build_queue(&QueueInput::new(&cards, &decks, s.deck_id, clock));
    for id in res.cards {
        if let Some(c) = by_id.get(&id) {
            push(s, id, c.queue, c.due);
        }
    }
    for l in res.later_learning {
        s.learning.insert(l.id, l.due);
    }
    Ok(())
}

/// Sort rank for custom search sessions: learning and reviews before new cards.
fn queue_rank(q: i64) -> i64 {
    if q == CardQueue::New as i64 {
        2
    } else if q == CardQueue::Learn as i64 || q == CardQueue::DayLearn as i64 {
        0
    } else {
        1
    }
}

fn fill_custom(core: &CollectionCore, s: &mut StudySession, clock: &SchedClock) -> Result<()> {
    let Some(custom) = s.custom.clone() else { return Ok(()) };
    let deck_ids = subtree_deck_ids(core, s.deck_id)?;
    let (f, fparams) = deck_filter_sql(deck_ids.as_deref(), "did");
    let row3 = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, i64, i64)> { Ok((r.get(0)?, r.get(1)?, r.get(2)?)) };
    let rows: Vec<(i64, i64, i64)> = match custom {
        CustomStudyKind::Ahead { days } => {
            let days = days.max(1);
            let mut params = fparams;
            params.extend([clock.today, clock.today + days]);
            core.db().all(
                &format!(
                    "SELECT id, queue, due FROM cards WHERE {f} AND queue = {} AND due > ? AND due <= ? ORDER BY due, id",
                    CardQueue::Review as i64
                ),
                rusqlite::params_from_iter(params),
                row3,
            )?
        }
        CustomStudyKind::Forgotten { days } => {
            let days = days.max(1);
            let cutoff = (clock.next_day_at - days * 86_400) * 1000;
            let mut params = fparams;
            params.push(cutoff);
            core.db().all(
                &format!("SELECT id, queue, due FROM cards WHERE {f} AND queue >= 0 AND id IN (SELECT cid FROM revlog WHERE id > ? AND ease = 1) ORDER BY ord, id"),
                rusqlite::params_from_iter(params),
                row3,
            )?
        }
        CustomStudyKind::Preview => core.db().all(
            &format!(
                "SELECT id, queue, due FROM cards WHERE {f} AND queue = {} ORDER BY due, ord LIMIT {PREVIEW_LIMIT}",
                CardQueue::New as i64
            ),
            rusqlite::params_from_iter(fparams),
            row3,
        )?,
        CustomStudyKind::Search { query, limit } => {
            let ids = run_search(core.db(), &query, &search_context(core, s.deck_id)?, SearchMode::Cards)?;
            let in_scope: Option<HashSet<DeckId>> = deck_ids.as_ref().map(|d| d.iter().copied().collect());
            let limit = limit.max(1) as usize;
            let mut rows: Vec<(i64, i64, i64)> = Vec::new();
            for part in ids.chunks(SQL_CHUNK) {
                let found: Vec<(i64, i64, i64, i64)> = core.db().all(
                    &format!("SELECT id, queue, due, did FROM cards WHERE id IN ({}) AND queue >= 0", marks(part.len())),
                    rusqlite::params_from_iter(part.iter()),
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
                for (id, queue, due, did) in found {
                    if in_scope.as_ref().is_none_or(|scope| scope.contains(&did)) {
                        rows.push((id, queue, due));
                    }
                }
            }
            rows.sort_by(|a, b| queue_rank(a.1).cmp(&queue_rank(b.1)).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
            rows.truncate(limit);
            rows
        }
    };
    for (id, queue, due) in rows {
        if s.done.contains(&id) {
            continue;
        }
        push(s, id, card_queue_from_i64(queue), due);
    }
    Ok(())
}

// ───────────────────────── picking ─────────────────────────

/// Counts shown in the study top bar: remaining cards, including the one on screen.
pub fn session_counts(s: &StudySession, clock: &SchedClock) -> StudyCounts {
    let ahead = clock.now_ms.div_euclid(1000) + clock.learn_ahead_secs;
    let mut c = StudyCounts { new: 0, learn: s.learning.values().filter(|due| **due <= ahead).count() as i64, review: 0 };
    for e in &s.main {
        match e.kind {
            MainKind::New => c.new += 1,
            MainKind::Review => c.review += 1,
            MainKind::Interday => c.learn += 1,
        }
    }
    c
}

fn expected_queue(k: MainKind) -> CardQueue {
    match k {
        MainKind::New => CardQueue::New,
        MainKind::Review => CardQueue::Review,
        MainKind::Interday => CardQueue::DayLearn,
    }
}

/// Drops a card from both queues.
fn forget(s: &mut StudySession, id: CardId) {
    s.learning.remove(&id);
    if let Some(i) = s.main.iter().position(|e| e.id == id) {
        s.main.remove(i);
    }
}

/// Chooses the next card: the priority card, else learning due now, else the main queue, else
/// learning within the learn-ahead limit. The card just answered is skipped while anything else
/// is available (Anki shows another card first). Stale entries (suspended, buried, deleted or
/// changed meanwhile) are dropped.
fn pick_next(core: &CollectionCore, s: &mut StudySession, clock: &SchedClock) -> Result<Option<Card>> {
    for _guard in 0..100_000 {
        let now = clock.now_ms.div_euclid(1000);
        let mut learning_sorted: Vec<(CardId, i64)> = s.learning.iter().map(|(k, v)| (*k, *v)).collect();
        learning_sorted.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        let due_now = learning_sorted.iter().filter(|(_, due)| *due <= now).map(|(id, _)| *id);
        let ahead =
            learning_sorted.iter().filter(|(_, due)| *due > now && *due <= now + clock.learn_ahead_secs).map(|(id, _)| *id);
        let mut ordered: Vec<CardId> = Vec::with_capacity(s.learning.len() + s.main.len() + 1);
        if let Some(p) = s.priority {
            if s.learning.contains_key(&p) || s.main.iter().any(|e| e.id == p) {
                ordered.push(p);
            }
        }
        ordered.extend(due_now);
        ordered.extend(s.main.iter().map(|e| e.id));
        ordered.extend(ahead);
        let Some(&first) = ordered.first() else { return Ok(None) };
        let mut id = first;
        if Some(id) == s.last_answered && Some(id) != s.priority {
            if let Some(other) = ordered.iter().find(|x| Some(**x) != s.last_answered) {
                id = *other;
            }
        }
        // A vanished card is dropped from the queue.
        if let Ok(card) = get_card(core, id) {
            if is_still_queued(s, &card) {
                return Ok(Some(card));
            }
        }
        forget(s, id);
        if s.priority == Some(id) {
            s.priority = None;
        }
    }
    Ok(None)
}

fn is_still_queued(s: &mut StudySession, card: &Card) -> bool {
    if (card.queue as i64) < 0 {
        return false;
    }
    if let std::collections::btree_map::Entry::Occupied(mut e) = s.learning.entry(card.id) {
        if !matches!(card.queue, CardQueue::Learn | CardQueue::Preview) {
            return false;
        }
        e.insert(card.due);
        return true;
    }
    let Some(entry) = s.main.iter().find(|e| e.id == card.id) else { return false };
    if s.custom.is_some() {
        return true;
    }
    card.queue == expected_queue(entry.kind)
}

fn is_leech_tag(t: &str) -> bool {
    t.to_lowercase() == "leech"
}

/// The study card shown for `card`: rendering, button preview, counts.
fn to_study_card(core: &CollectionCore, s: &StudySession, card: Card, clock: &SchedClock) -> Result<StudyCard> {
    let note = get_note(core, card.nid)?;
    let config = core.deck_config(card.did)?;
    // rendered without deck when it vanished
    let deck_name = core.deck(card.did).map(|d| d.name).unwrap_or_default();
    let rendered = render_stored_card(core, &card, Some(&note))?;
    let preview = preview_intervals(&scheduling_input(core, &card, &config, clock)?)?;
    let counts = session_counts(s, clock);
    let is_leech = note.tags.iter().any(|t| is_leech_tag(t));
    Ok(StudyCard { card, note, rendered, preview, counts, deck_name, is_leech })
}

/// Next card of the session, or None when nothing is left for now. Rebuilds after a day rollover.
pub fn next_study_card(core: &CollectionCore, s: &mut StudySession) -> Result<Option<StudyCard>> {
    let mut clock = sched_clock(core)?;
    if clock.today != s.day {
        fill_session(core, s)?;
        clock = sched_clock(core)?;
    }
    match pick_next(core, s, &clock)? {
        Some(card) => Ok(Some(to_study_card(core, s, card, &clock)?)),
        None => Ok(None),
    }
}

// ───────────────────────── answering ─────────────────────────

struct CounterAdd {
    new: i64,
    rev: i64,
    lrn: i64,
    ms: i64,
}

/// Adds an answer to the "today" counters of the card's deck and its ancestors (counters of an
/// older day are reset first). Returns the deck rows as they were (for undo).
fn bump_deck_counters(core: &CollectionCore, did: DeckId, today: CollectionDay, add: CounterAdd) -> Result<Vec<RowSnapshot>> {
    let ids: Vec<i64> = self_and_ancestors(core, did)?.iter().map(|d| d.id).collect();
    let before = undo::snapshot_rows(core.db(), Table::Decks, &ids)?;
    for row in &before {
        let t = parse_deck_today(row.get("today").and_then(|v| v.as_str()).unwrap_or(""));
        let base = if t.day == today { t } else { DeckToday { day: today, ..Default::default() } };
        let next = DeckToday {
            day: today,
            new: base.new + add.new,
            rev: base.rev + add.rev,
            lrn: base.lrn + add.lrn,
            time_ms: base.time_ms + add.ms,
        };
        let id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        core.db().run("UPDATE decks SET today = ? WHERE id = ?", rusqlite::params![deck_today_to_json(&next), id])?;
    }
    Ok(before)
}

/// Adds the `leech` tag to a note; returns the note row before the change, or None when already tagged.
fn tag_leech(core: &CollectionCore, nid: NoteId) -> Result<Option<RowSnapshot>> {
    let Some(before) = undo::snapshot_rows(core.db(), Table::Notes, &[nid])?.into_iter().next() else { return Ok(None) };
    let note = get_note(core, nid)?;
    if note.tags.iter().any(|t| is_leech_tag(t)) {
        return Ok(None);
    }
    let mut tags = note.tags.clone();
    tags.push("leech".to_string());
    let tags = core.canonical_tags(&tags)?;
    core.db().run(
        "UPDATE notes SET tags = ?, mtime = ?, usn = -1 WHERE id = ?",
        rusqlite::params![join_tags(&tags), core.now_seconds(), nid],
    )?;
    core.register_tags(&tags)?;
    Ok(Some(before))
}

fn rating_name(r: Rating) -> &'static str {
    match r {
        Rating::Again => "Again",
        Rating::Hard => "Hard",
        Rating::Good => "Good",
        Rating::Easy => "Easy",
    }
}

struct Answered {
    card: Card,
    revlog_id: RevlogId,
    became_leech: bool,
    bury_ids: Vec<CardId>,
    label: String,
}

/// Answers a card: scheduler state change, revlog row, leech handling, sibling burying, deck
/// counters, one undo entry. Updates the session queues when a session is active.
pub fn answer_card(
    core: &CollectionCore,
    mut s: Option<&mut StudySession>,
    card_id: CardId,
    rating: Rating,
    elapsed_ms: i64,
) -> Result<AnswerResult> {
    let ms = elapsed_ms.max(0);
    let mut became_leech = false;
    let mut undo_label = String::new();
    let mut revlog_id: Option<RevlogId> = None;
    let is_preview = s.as_ref().is_some_and(|s| s.is_preview());

    if is_preview {
        get_card(core, card_id)?;
    } else {
        let result = core.db().transaction(|| {
            let card = get_card(core, card_id)?;
            if (card.queue as i64) < 0 {
                return Err(LociError::invalid("This card is suspended or buried"));
            }
            let clock = sched_clock(core)?;
            let config = core.deck_config(card.did)?;
            let out = apply_answer(&scheduling_input(core, &card, &config, &clock)?, rating, ms)?;
            let mut cards_before = undo::snapshot_rows(core.db(), Table::Cards, &[card.id])?;
            let revlog = RevlogEntry { id: core.new_id(IdTable::Revlog)?, ..out.revlog };
            update_card(core, &out.card)?;
            insert_revlog(core.db(), &revlog)?;

            let mut notes_before = Vec::new();
            if out.became_leech {
                if let Some(n) = tag_leech(core, card.nid)? {
                    notes_before.push(n);
                }
            }
            let bury_ids = siblings_to_bury(core.db(), card.nid, card.id, &config, clock.today)?;
            cards_before.extend(undo::snapshot_rows(core.db(), Table::Cards, &bury_ids)?);
            if !bury_ids.is_empty() {
                let mut params: Vec<i64> = vec![CardQueue::SchedBuried as i64, core.now_seconds()];
                params.extend(bury_ids.iter());
                core.db().run(
                    &format!("UPDATE cards SET queue = ?, mtime = ?, usn = -1 WHERE id IN ({})", marks(bury_ids.len())),
                    rusqlite::params_from_iter(params),
                )?;
            }
            let was_intraday = matches!(card.queue, CardQueue::Learn | CardQueue::Preview);
            let decks_before = bump_deck_counters(
                core,
                card.did,
                clock.today,
                CounterAdd {
                    new: (card.ctype == CardType::New) as i64,
                    rev: matches!(card.queue, CardQueue::Review | CardQueue::DayLearn) as i64,
                    lrn: was_intraday as i64,
                    ms: revlog.time,
                },
            )?;
            let label = msg("study.answerUndoLabel", "Answer {rating}", &[("rating", rating_name(rating).into())]);
            undo::record(
                core.db(),
                core.now(),
                &label,
                &UndoSnapshot {
                    cards: cards_before,
                    notes: notes_before,
                    decks: decks_before,
                    inserted_revlog: vec![revlog.id],
                    ..Default::default()
                },
            )?;
            core.mark_changed()?;
            Ok(Answered { card: out.card, revlog_id: revlog.id, became_leech: out.became_leech, bury_ids, label })
        })?;
        became_leech = result.became_leech;
        undo_label = result.label;
        revlog_id = Some(result.revlog_id);
        if let Some(s) = s.as_deref_mut() {
            for id in result.bury_ids {
                forget(s, id);
            }
            forget(s, card_id);
            if result.card.queue == CardQueue::Learn {
                s.learning.insert(card_id, result.card.due);
            }
        }
    }

    let leech_flag = if became_leech { Some(true) } else { None };
    let Some(s) = s else {
        return Ok(AnswerResult {
            counts: StudyCounts::default(),
            next: None,
            undo_label,
            became_leech: leech_flag,
            finished: true,
        });
    };
    if s.is_preview() {
        forget(s, card_id);
    }
    if s.custom.is_some() {
        s.done.insert(card_id);
    }
    s.answers.push(AnswerRecord { card_id, revlog_id, rating, ms });
    s.last_answered = Some(card_id);
    if s.priority == Some(card_id) {
        s.priority = None;
    }
    let next = next_study_card(core, s)?;
    let counts = match &next {
        Some(n) => n.counts,
        None => session_counts(s, &sched_clock(core)?),
    };
    Ok(AnswerResult { counts, finished: next.is_none(), next, undo_label, became_leech: leech_flag })
}

/// Re-synchronises a session after an undo: answers whose revlog row is gone are dropped, the
/// undone card is shown next, and the queues are rebuilt from the database.
pub fn after_undo(core: &CollectionCore, s: &mut StudySession) -> Result<()> {
    let mut gone: Vec<AnswerRecord> = Vec::new();
    let mut kept: Vec<AnswerRecord> = Vec::new();
    for a in s.answers.drain(..) {
        match a.revlog_id {
            None => kept.push(a),
            Some(rid) => {
                if core.db().scalar::<i64, _>("SELECT 1 FROM revlog WHERE id = ?", [rid])?.is_some() {
                    kept.push(a);
                } else {
                    gone.push(a);
                }
            }
        }
    }
    s.answers = kept;
    let undone = gone.last().cloned();
    for a in &gone {
        s.done.remove(&a.card_id);
    }
    fill_session(core, s)?;
    if let Some(u) = undone {
        s.priority = Some(u.card_id);
        s.last_answered = None;
        if !s.learning.contains_key(&u.card_id) && !s.main.iter().any(|e| e.id == u.card_id) {
            // The card's restored state may fall outside today's gathered queue (e.g. limits); show it anyway.
            let c: Option<(i64, i64)> =
                core.db().get("SELECT queue, due FROM cards WHERE id = ?", [u.card_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if let Some((queue, due)) = c {
                match kind_of_queue(card_queue_from_i64(queue)) {
                    Some(QueueKind::Learning) => {
                        s.learning.insert(u.card_id, due);
                    }
                    Some(QueueKind::Main(kind)) => s.main.insert(0, MainEntry { id: u.card_id, kind }),
                    None => {}
                }
            }
        }
    }
    Ok(())
}

/// Runs `after_undo` when an answer of this session was undone since the last call (its revlog
/// row is gone), whichever path performed the undo.
pub fn resync_if_undone(core: &CollectionCore, s: &mut StudySession) -> Result<()> {
    let ids: Vec<i64> = s.answers.iter().filter_map(|a| a.revlog_id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let mut present = 0i64;
    for part in ids.chunks(SQL_CHUNK) {
        present += core.db().count(
            &format!("SELECT count(*) FROM revlog WHERE id IN ({})", marks(part.len())),
            rusqlite::params_from_iter(part.iter()),
        )?;
    }
    if present < ids.len() as i64 {
        after_undo(core, s)?;
    }
    Ok(())
}

// ───────────────────────── summary ─────────────────────────

/// Summary of a finished session (internals §6): cards, time, accuracy, buttons, tomorrow's load.
pub fn summarise(core: &CollectionCore, s: Option<&StudySession>) -> Result<SessionSummary> {
    let answers: &[AnswerRecord] = s.map(|s| s.answers.as_slice()).unwrap_or(&[]);
    let by = |r: Rating| answers.iter().filter(|a| a.rating == r).count() as i64;
    let again = by(Rating::Again);
    let clock = sched_clock(core)?;
    let deck_id = s.and_then(|s| s.deck_id);
    let (f, mut params) = deck_filter_sql(subtree_deck_ids(core, deck_id)?.as_deref(), "did");
    params.extend([clock.today + 1, clock.next_day_at + 86_400]);
    let tomorrow_due = core.db().count(
        &format!(
            "SELECT count(*) FROM cards WHERE {f} AND ((queue IN ({}, {}) AND due <= ?) OR (queue IN ({}, {}) AND due < ?))",
            CardQueue::Review as i64,
            CardQueue::DayLearn as i64,
            CardQueue::Learn as i64,
            CardQueue::Preview as i64,
        ),
        rusqlite::params_from_iter(params),
    )?;
    let cards = answers.len() as i64;
    let total_ms: i64 = answers.iter().map(|a| a.ms).sum();
    Ok(SessionSummary {
        cards,
        seconds: (total_ms as f64 / 1000.0).round() as i64,
        accuracy: if cards > 0 { (cards - again) as f64 / cards as f64 } else { 0.0 },
        again,
        hard: by(Rating::Hard),
        good: by(Rating::Good),
        easy: by(Rating::Easy),
        tomorrow_due,
        insight: session_insight(core, deck_id),
    })
}

// ───────────────────────── Collection API ─────────────────────────

impl Collection {
    /// Starts a session for a deck (None = all decks); continuing the same deck keeps the session's answers.
    pub fn start_study(&mut self, deck_id: Option<DeckId>) -> Result<Option<StudyCard>> {
        if let Some(d) = deck_id {
            self.core.deck(d)?;
        }
        self.rollover()?;
        let reuse = self.session.as_ref().is_some_and(|s| s.custom.is_none() && s.deck_id == deck_id);
        if !reuse {
            self.session = Some(StudySession::new(deck_id, None, self.core.now()));
        }
        let core = &self.core;
        let Some(s) = self.session.as_mut() else { return Ok(None) };
        fill_session(core, s)?;
        next_study_card(core, s)
    }

    pub fn next_card(&mut self) -> Result<Option<StudyCard>> {
        let core = &self.core;
        match self.session.as_mut() {
            Some(s) => {
                resync_if_undone(core, s)?;
                next_study_card(core, s)
            }
            None => Ok(None),
        }
    }

    pub fn answer_card(&mut self, card_id: CardId, rating: Rating, elapsed_ms: i64) -> Result<AnswerResult> {
        let core = &self.core;
        if let Some(s) = self.session.as_mut() {
            resync_if_undone(core, s)?;
        }
        answer_card(core, self.session.as_mut(), card_id, rating, elapsed_ms)
    }

    pub fn start_custom_study(&mut self, deck_id: Option<DeckId>, kind: CustomStudyKind) -> Result<Option<StudyCard>> {
        if let Some(d) = deck_id {
            self.core.deck(d)?;
        }
        self.rollover()?;
        self.session = Some(StudySession::new(deck_id, Some(kind), self.core.now()));
        let core = &self.core;
        let Some(s) = self.session.as_mut() else { return Ok(None) };
        fill_session(core, s)?;
        next_study_card(core, s)
    }

    pub fn end_study(&mut self) -> Result<SessionSummary> {
        let summary = summarise(&self.core, self.session.as_ref())?;
        self.session = None;
        Ok(summary)
    }

    /// Refills a running non-custom session (after `extend_today_limits`).
    pub fn refill_session(&mut self) -> Result<()> {
        let core = &self.core;
        if let Some(s) = self.session.as_mut() {
            if s.custom.is_none() {
                fill_session(core, s)?;
            }
        }
        Ok(())
    }

    /// Re-synchronises a running session after an undo.
    pub fn session_after_undo(&mut self) -> Result<()> {
        let core = &self.core;
        if let Some(s) = self.session.as_mut() {
            after_undo(core, s)?;
        }
        Ok(())
    }
}
