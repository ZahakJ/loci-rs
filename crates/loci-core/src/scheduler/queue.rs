//! Study-queue gathering and counts (Anki v3 queue builder; research/fsrs-spec.md §9.2–§9.3;
//! recall `src/core/scheduler/queue.ts` plus the card/revlog loaders of
//! `collection/sched-context.ts`). The gathering itself is pure: it operates on card rows and
//! deck descriptions and is deterministic for a seed. The loaders at the bottom query SQLite.
//!
//! Order of the produced queue: intraday learning due now (by due) → the main queue (interday
//! learning, reviews and new cards mixed per the selected deck's options) → intraday learning
//! due within the learn-ahead limit (shown when nothing else is left).

use super::clock::{SchedClock, DEFAULT_LEARN_AHEAD_SECS};
use super::convert::day_start_ms;
use super::fsrs::{decay_from_params, normalize_parameters};
use super::fuzz::seed32;
use crate::collection::rows::{row_to_card, row_to_revlog, select_sql, CARD_COLUMNS, REVLOG_COLUMNS};
use crate::db::Db;
use crate::error::Result;
use crate::types::{
    Card, CardId, CardQueue, CollectionDay, DeckConfig, DeckId, DeckToday, NewCardGather, NewCardSort, NewReviewMix, NoteId,
    ReviewOrder, RevlogEntry, SchedulerKind, Seconds, StudyCounts,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct QueueDeck {
    pub id: DeckId,
    /// Full name with '::' hierarchy.
    pub name: String,
    pub config: DeckConfig,
    /// Today's counters; ignored unless `today.day` equals the input's `today`.
    pub today: Option<DeckToday>,
}

#[derive(Debug, Clone)]
pub struct QueueInput<'a> {
    /// Cards of (at least) the selected subtree; cards in other decks are ignored.
    pub cards: &'a [Card],
    /// All decks (parents included, so limits can cascade).
    pub decks: &'a [QueueDeck],
    /// Selected deck; `None` = all decks.
    pub deck_id: Option<DeckId>,
    pub now_ms: i64,
    pub today: CollectionDay,
    pub crt: Seconds,
    pub rollover_hour: i64,
    /// Learn-ahead limit, seconds (Anki default 1200).
    pub learn_ahead_secs: Option<i64>,
    /// Seed for random orders and tiebreaks (default: today).
    pub seed: Option<i64>,
    /// Also apply limits of the selected deck's ancestors (Anki "apply all parent limits"; default false).
    pub apply_all_parent_limits: bool,
    /// Ordering options to use when deck_id is `None` (default: first top-level deck's config).
    pub order_config: Option<&'a DeckConfig>,
}

impl<'a> QueueInput<'a> {
    /// The usual input from a clock: seed = today, learn-ahead from the clock.
    pub fn new(cards: &'a [Card], decks: &'a [QueueDeck], deck_id: Option<DeckId>, clock: &SchedClock) -> Self {
        QueueInput {
            cards,
            decks,
            deck_id,
            now_ms: clock.now_ms,
            today: clock.today,
            crt: clock.crt,
            rollover_hour: clock.rollover_hour,
            learn_ahead_secs: Some(clock.learn_ahead_secs),
            seed: Some(clock.today),
            apply_all_parent_limits: false,
            order_config: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaterLearning {
    pub id: CardId,
    pub due: Seconds,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueResult {
    /// Card ids in study order.
    pub cards: Vec<CardId>,
    pub counts: StudyCounts,
    /// Intraday learning cards due later today (beyond learn-ahead), by due: rebuild when they come due.
    pub later_learning: Vec<LaterLearning>,
}

// ───────────────────────── deck tree & limits ─────────────────────────
fn parent_name(name: &str) -> Option<&str> {
    name.rfind("::").map(|i| &name[..i])
}

/// Anki deck order: component-wise, case-insensitive.
pub fn compare_deck_names(a: &str, b: &str) -> Ordering {
    let pa: Vec<String> = a.split("::").map(|s| s.to_lowercase()).collect();
    let pb: Vec<String> = b.split("::").map(|s| s.to_lowercase()).collect();
    for (x, y) in pa.iter().zip(pb.iter()) {
        if x != y {
            return x.cmp(y);
        }
    }
    pa.len().cmp(&pb.len())
}

#[derive(Debug, Clone)]
struct DeckNode {
    deck_idx: usize,
    parent: Option<usize>,
    order: usize,
    new_left: i64,
    rev_left: i64,
}

struct Nodes<'a> {
    decks: &'a [QueueDeck],
    nodes: Vec<DeckNode>,
    by_id: HashMap<DeckId, usize>,
}

impl Nodes<'_> {
    fn get(&self, id: DeckId) -> Option<usize> {
        self.by_id.get(&id).copied()
    }
    fn config_of(&self, idx: usize) -> &DeckConfig {
        &self.decks[self.nodes[idx].deck_idx].config
    }
    fn is_in_subtree(&self, node: usize, root: Option<usize>) -> bool {
        let Some(root) = root else { return true };
        let mut n = Some(node);
        while let Some(i) = n {
            if i == root {
                return true;
            }
            n = self.nodes[i].parent;
        }
        false
    }
    /// Limit chain for a deck: itself and ancestors up to the selected deck (or all, if requested).
    fn limit_chain(&self, node: usize, root: Option<usize>, all_parents: bool) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut n = Some(node);
        while let Some(i) = n {
            chain.push(i);
            if Some(i) == root && !all_parents {
                break;
            }
            n = self.nodes[i].parent;
        }
        chain
    }
}

fn build_nodes<'a>(decks: &'a [QueueDeck], today: CollectionDay) -> Nodes<'a> {
    let mut sorted: Vec<usize> = (0..decks.len()).collect();
    sorted.sort_by(|&a, &b| compare_deck_names(&decks[a].name, &decks[b].name));
    let mut nodes: Vec<DeckNode> = Vec::with_capacity(decks.len());
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut by_id: HashMap<DeckId, usize> = HashMap::new();
    for (order, &deck_idx) in sorted.iter().enumerate() {
        let deck = &decks[deck_idx];
        let t = deck.today.as_ref().filter(|t| t.day == today);
        nodes.push(DeckNode {
            deck_idx,
            parent: None,
            order,
            new_left: (deck.config.new_per_day - t.map(|t| t.new).unwrap_or(0)).max(0),
            rev_left: (deck.config.reviews_per_day - t.map(|t| t.rev).unwrap_or(0)).max(0),
        });
        by_name.insert(deck.name.to_lowercase(), order);
        by_id.insert(deck.id, order);
    }
    for i in 0..nodes.len() {
        let name = &decks[nodes[i].deck_idx].name;
        let mut p = parent_name(name);
        while let Some(pn) = p {
            if by_name.contains_key(&pn.to_lowercase()) {
                break;
            }
            p = parent_name(pn);
        }
        nodes[i].parent = p.and_then(|pn| by_name.get(&pn.to_lowercase()).copied());
    }
    Nodes { decks, nodes, by_id }
}

// ───────────────────────── ordering helpers ─────────────────────────
/// Deterministic 32-bit tiebreak hash of an id under a seed (TS `tieHash`).
pub fn tie_hash(id: i64, seed: i64) -> f64 {
    seed32(id.wrapping_add((seed as u32 as i64) * 1_000_003)) as f64
}

/// Anki intersperser: evenly spreads `two` through `one`.
pub fn intersperse<T: Clone>(one: &[T], two: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(one.len() + two.len());
    let ratio = (one.len() + 1) as f64 / (two.len() + 1) as f64;
    let (mut i, mut j) = (0, 0);
    while i < one.len() || j < two.len() {
        if i < one.len() && j < two.len() {
            if ((j + 1) as f64) * ratio < (i + 1) as f64 {
                out.push(two[j].clone());
                j += 1;
            } else {
                out.push(one[i].clone());
                i += 1;
            }
        } else if i < one.len() {
            out.push(one[i].clone());
            i += 1;
        } else {
            out.push(two[j].clone());
            j += 1;
        }
    }
    out
}

fn mix<T: Clone>(primary: &[T], secondary: &[T], how: NewReviewMix) -> Vec<T> {
    match how {
        NewReviewMix::ReviewFirst => primary.iter().chain(secondary.iter()).cloned().collect(),
        NewReviewMix::NewFirst => secondary.iter().chain(primary.iter()).cloned().collect(),
        NewReviewMix::Mix => intersperse(primary, secondary),
    }
}

/// Relative overdueness key (smaller = more overdue), Anki RelativeOverdueness.
fn relative_overdue_key(card: &Card, cfg: &DeckConfig, now_ms: i64, today: CollectionDay) -> f64 {
    let data = &card.data;
    if cfg.scheduler == SchedulerKind::Fsrs {
        if let Some(s) = data.s.filter(|s| *s > 0.0) {
            let decay = data.decay.unwrap_or_else(|| {
                if cfg.fsrs_params.is_empty() {
                    decay_from_params(&[])
                } else {
                    normalize_parameters(&cfg.fsrs_params)
                        .map(|w| decay_from_params(&w))
                        .unwrap_or_else(|_| decay_from_params(&[]))
                }
            });
            let elapsed = match data.lrt {
                Some(lrt) if lrt != 0 => ((now_ms as f64 / 1000.0 - lrt as f64) / 86400.0).max(0.0),
                _ => (today - (card.due - card.ivl)).max(0) as f64,
            };
            let factor = 0.9f64.powf(-1.0 / decay) - 1.0;
            let r = (1.0 + factor * elapsed / s).powf(-decay);
            return -(r.powf(-1.0 / decay) - 1.0) / (cfg.desired_retention.powf(-1.0 / decay) - 1.0);
        }
    }
    // SM-2: −(1 + (today − due + 0.001)/max(ivl, 1)).
    -(1.0 + ((today - card.due) as f64 + 0.001) / card.ivl.max(1) as f64)
}

struct GatherCtx<'a> {
    nodes: Nodes<'a>,
    seed: i64,
    now_ms: i64,
    today: CollectionDay,
    order_config: &'a DeckConfig,
}

impl GatherCtx<'_> {
    fn config_of(&self, c: &Card) -> &DeckConfig {
        match self.nodes.get(c.did) {
            Some(i) => self.nodes.config_of(i),
            None => self.order_config,
        }
    }
    fn deck_order(&self, c: &Card) -> f64 {
        self.nodes.get(c.did).map(|i| self.nodes.nodes[i].order as f64).unwrap_or(0.0)
    }
    fn rand(&self, c: &Card) -> f64 {
        tie_hash(c.id, self.seed)
    }
    fn review_key(&self, order: ReviewOrder, c: &Card) -> Vec<f64> {
        let fsrs = self.order_config.scheduler == SchedulerKind::Fsrs;
        let difficulty = c.data.d.unwrap_or(5.0);
        match order {
            ReviewOrder::DueThenRandom => vec![c.due as f64, self.rand(c)],
            ReviewOrder::DueThenDeck => vec![c.due as f64, self.deck_order(c), self.rand(c)],
            ReviewOrder::DeckThenDue => vec![self.deck_order(c), c.due as f64, self.rand(c)],
            ReviewOrder::AscendingIntervals => vec![c.ivl as f64, self.rand(c)],
            ReviewOrder::DescendingIntervals => vec![-(c.ivl as f64), self.rand(c)],
            // Under FSRS, Anki's ease orders become difficulty orders (ascending ease = descending difficulty).
            ReviewOrder::AscendingEase => vec![if fsrs { -difficulty } else { c.factor as f64 }, self.rand(c)],
            ReviewOrder::DescendingEase => vec![if fsrs { difficulty } else { -(c.factor as f64) }, self.rand(c)],
            ReviewOrder::RelativeOverdue => {
                vec![relative_overdue_key(c, self.config_of(c), self.now_ms, self.today), self.rand(c)]
            }
            ReviewOrder::Random => vec![self.rand(c)],
            ReviewOrder::Added => vec![c.nid as f64, c.ord as f64, c.id as f64],
            ReviewOrder::ReverseAdded => vec![-(c.nid as f64), c.ord as f64, -(c.id as f64)],
        }
    }
}

fn sort_by(cards: Vec<&Card>, key: impl Fn(&Card) -> Vec<f64>) -> Vec<&Card> {
    let mut decorated: Vec<(Vec<f64>, &Card)> = cards.into_iter().map(|c| (key(c), c)).collect();
    decorated.sort_by(|a, b| {
        for (x, y) in a.0.iter().zip(b.0.iter()) {
            match x.total_cmp(y) {
                Ordering::Equal => continue,
                o => return o,
            }
        }
        a.1.id.cmp(&b.1.id)
    });
    decorated.into_iter().map(|d| d.1).collect()
}

// ───────────────────────── gathering ─────────────────────────
#[derive(Default)]
struct Gathered<'c> {
    learn_now: Vec<&'c Card>,
    learn_ahead: Vec<&'c Card>,
    learn_later: Vec<&'c Card>,
    interday: Vec<&'c Card>,
    reviews: Vec<&'c Card>,
    new_cards: Vec<&'c Card>,
}

fn counts_of(g: &Gathered<'_>) -> StudyCounts {
    StudyCounts {
        new: g.new_cards.len() as i64,
        learn: (g.learn_now.len() + g.learn_ahead.len() + g.interday.len()) as i64,
        review: g.reviews.len() as i64,
    }
}

fn gather<'a, 'c>(
    input: &QueueInput<'a>,
    mut nodes: Nodes<'a>,
    root: Option<usize>,
    cards: &'c [Card],
) -> Option<(Gathered<'c>, GatherCtx<'a>)> {
    let seed = input.seed.unwrap_or(input.today);
    let first_top = nodes.nodes.iter().filter(|n| n.parent.is_none()).min_by_key(|n| n.order).map(|n| n.deck_idx);
    let order_config: &DeckConfig = match root {
        Some(r) => &input.decks[nodes.nodes[r].deck_idx].config,
        None => match input.order_config {
            Some(c) => c,
            None => &input.decks[first_top?].config,
        },
    };
    let now_secs = input.now_ms.div_euclid(1000);
    let ahead_secs = now_secs + input.learn_ahead_secs.unwrap_or(DEFAULT_LEARN_AHEAD_SECS);
    let next_day_at = day_start_ms(input.today + 1, input.crt, input.rollover_hour).div_euclid(1000);

    let mut intraday = Vec::new();
    let mut day_learn = Vec::new();
    let mut review = Vec::new();
    let mut fresh = Vec::new();
    for c in cards {
        let Some(n) = nodes.get(c.did) else { continue };
        if !nodes.is_in_subtree(n, root) {
            continue;
        }
        match c.queue {
            CardQueue::Learn | CardQueue::Preview => {
                if c.due < next_day_at {
                    intraday.push(c);
                }
            }
            CardQueue::DayLearn => {
                if c.due <= input.today {
                    day_learn.push(c);
                }
            }
            CardQueue::Review => {
                if c.due <= input.today {
                    review.push(c);
                }
            }
            CardQueue::New => fresh.push(c),
            _ => {} // suspended / buried
        }
    }
    let mut seen: HashSet<NoteId> = HashSet::new();
    let mut g = Gathered::default();
    for c in sort_by(intraday, |c| vec![c.due as f64]) {
        if c.due <= now_secs {
            g.learn_now.push(c);
        } else if c.due <= ahead_secs {
            g.learn_ahead.push(c);
        } else {
            g.learn_later.push(c);
        }
        seen.insert(c.nid);
    }
    let all_parents = input.apply_all_parent_limits;
    // Takes a card if its note is not buried and every deck in its limit chain has room.
    let mut take = |nodes: &mut Nodes<'a>, c: &Card, is_new: bool, bury: bool| -> bool {
        if bury && seen.contains(&c.nid) {
            return false;
        }
        let Some(idx) = nodes.get(c.did) else { return false };
        let chain = nodes.limit_chain(idx, root, all_parents);
        let blocked = chain.iter().any(|&n| {
            let node = &nodes.nodes[n];
            if is_new {
                node.new_left <= 0 || node.rev_left <= 0
            } else {
                node.rev_left <= 0
            }
        });
        if blocked {
            return false;
        }
        for &n in &chain {
            nodes.nodes[n].rev_left -= 1;
            if is_new {
                nodes.nodes[n].new_left -= 1;
            }
        }
        seen.insert(c.nid);
        true
    };
    let ctx_keys = GatherCtx {
        nodes: Nodes { decks: nodes.decks, nodes: nodes.nodes.clone(), by_id: nodes.by_id.clone() },
        seed,
        now_ms: input.now_ms,
        today: input.today,
        order_config,
    };
    for c in sort_by(day_learn, |c| vec![c.due as f64, tie_hash(c.id, seed)]) {
        if take(&mut nodes, c, false, ctx_keys.config_of(c).bury_siblings_interday) {
            g.interday.push(c);
        }
    }
    let order = order_config.review_order;
    for c in sort_by(review, |c| ctx_keys.review_key(order, c)) {
        if take(&mut nodes, c, false, ctx_keys.config_of(c).bury_siblings_review) {
            g.reviews.push(c);
        }
    }
    for c in gather_new_order(fresh, order_config, &ctx_keys) {
        if take(&mut nodes, c, true, ctx_keys.config_of(c).bury_siblings_new) {
            g.new_cards.push(c);
        }
    }
    g.new_cards = sort_new(std::mem::take(&mut g.new_cards), order_config, seed);
    Some((g, ctx_keys))
}

fn gather_new_order<'c>(cards: Vec<&'c Card>, cfg: &DeckConfig, ctx: &GatherCtx<'_>) -> Vec<&'c Card> {
    let s = ctx.seed;
    match cfg.new_gather {
        NewCardGather::Deck => sort_by(cards, |c| vec![ctx.deck_order(c), c.due as f64, c.ord as f64]),
        NewCardGather::DeckRandomNotes => sort_by(cards, |c| vec![ctx.deck_order(c), tie_hash(c.nid, s), c.ord as f64]),
        NewCardGather::Ascending => sort_by(cards, |c| vec![c.due as f64, c.ord as f64]),
        NewCardGather::Descending => sort_by(cards, |c| vec![-(c.due as f64), c.ord as f64]),
        NewCardGather::RandomNotes => sort_by(cards, |c| vec![tie_hash(c.nid, s), c.ord as f64]),
        NewCardGather::RandomCards => sort_by(cards, |c| vec![tie_hash(c.id, s)]),
    }
}

fn sort_new<'c>(cards: Vec<&'c Card>, cfg: &DeckConfig, seed: i64) -> Vec<&'c Card> {
    let gathered: HashMap<CardId, usize> = cards.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let pos = |c: &Card| gathered.get(&c.id).copied().unwrap_or(0) as f64;
    match cfg.new_sort {
        NewCardSort::TemplateThenOrder => sort_by(cards, |c| vec![c.ord as f64, pos(c)]),
        NewCardSort::Order => cards,
        NewCardSort::TemplateThenRandom => sort_by(cards, |c| vec![c.ord as f64, tie_hash(c.id, seed + 1)]),
        NewCardSort::RandomNote => sort_by(cards, |c| vec![tie_hash(c.nid, seed + 1), c.ord as f64]),
        NewCardSort::RandomCard => sort_by(cards, |c| vec![tie_hash(c.id, seed + 1)]),
    }
}

/// Builds the ordered study queue and its counts for `input.deck_id`.
pub fn build_queue(input: &QueueInput<'_>) -> QueueResult {
    let nodes = build_nodes(input.decks, input.today);
    let root = match input.deck_id {
        None => None,
        Some(id) => match nodes.get(id) {
            Some(r) => Some(r),
            None => return QueueResult::default(),
        },
    };
    let Some((g, ctx)) = gather(input, nodes, root, input.cards) else { return QueueResult::default() };
    let cfg = ctx.order_config;
    let reviews_and_interday = mix(&g.reviews, &g.interday, cfg.interday_learning_mix);
    let main = mix(&reviews_and_interday, &g.new_cards, cfg.new_review_mix);
    let cards: Vec<CardId> = g.learn_now.iter().chain(main.iter()).chain(g.learn_ahead.iter()).map(|c| c.id).collect();
    QueueResult {
        cards,
        counts: counts_of(&g),
        later_learning: g.learn_later.iter().map(|c| LaterLearning { id: c.id, due: c.due }).collect(),
    }
}

/// Per-deck counts for the deck tree: for each deck, the counts you would see when selecting
/// it (its own limits and its children's; parents' limits ignored as in Anki).
pub fn due_counts(input: &QueueInput<'_>) -> BTreeMap<DeckId, StudyCounts> {
    let mut out = BTreeMap::new();
    let nodes0 = build_nodes(input.decks, input.today);
    let mut by_deck: HashMap<DeckId, Vec<Card>> = HashMap::new();
    for c in input.cards {
        by_deck.entry(c.did).or_default().push(c.clone());
    }
    for deck in input.decks {
        let nodes = build_nodes(input.decks, input.today); // fresh limit counters per selection
        let Some(root) = nodes.get(deck.id) else { continue };
        let mut sub: Vec<Card> = Vec::new();
        for n in &nodes0.nodes {
            let id = input.decks[n.deck_idx].id;
            if let Some(idx) = nodes.get(id) {
                if nodes.is_in_subtree(idx, Some(root)) {
                    if let Some(cs) = by_deck.get(&id) {
                        sub.extend(cs.iter().cloned());
                    }
                }
            }
        }
        let sel = QueueInput { deck_id: Some(deck.id), ..input.clone() };
        let counts = gather(&sel, nodes, Some(root), &sub).map(|(g, _)| counts_of(&g)).unwrap_or_default();
        out.insert(deck.id, counts);
    }
    out
}

// ───────────────────────── SQLite loaders (sched-context.ts) ─────────────────────────
/// Default answer time when the revlog has no recent reviews, seconds per card.
pub const DEFAULT_SECONDS_PER_CARD: f64 = 8.0;
/// Answers older than this many days do not count towards the recent pace.
const PACE_WINDOW_DAYS: i64 = 30;

/// `did IN (…)` clause for a deck subset (`None` = all decks → '1'; empty → '0').
pub fn deck_filter_sql(deck_ids: Option<&[DeckId]>, column: &str) -> (String, Vec<i64>) {
    match deck_ids {
        None => ("1".to_string(), vec![]),
        Some([]) => ("0".to_string(), vec![]),
        Some(ids) => {
            let marks: Vec<&str> = ids.iter().map(|_| "?").collect();
            (format!("{column} IN ({})", marks.join(",")), ids.to_vec())
        }
    }
}

/// Cards that can appear in today's queues: all new cards, reviews and interday learning due
/// today or earlier, intraday learning due before the next day starts.
pub fn load_queue_cards(db: &Db, deck_ids: Option<&[DeckId]>, clock: &SchedClock) -> Result<Vec<Card>> {
    let (f, mut params) = deck_filter_sql(deck_ids, "did");
    let sql = format!(
        "{} WHERE {f} AND (queue = {} OR (queue IN ({}, {}) AND due <= ?) OR (queue IN ({}, {}) AND due < ?))",
        select_sql("cards", &CARD_COLUMNS),
        CardQueue::New as i64,
        CardQueue::Review as i64,
        CardQueue::DayLearn as i64,
        CardQueue::Learn as i64,
        CardQueue::Preview as i64,
    );
    params.push(clock.today);
    params.push(clock.next_day_at);
    db.all(&sql, rusqlite::params_from_iter(params), row_to_card)
}

/// Recent seconds per answer: mean revlog time over the last 30 days (answers capped at 60 s
/// each, as Anki caps answer time), or 8 s without at least ten recent reviews.
pub fn seconds_per_card(db: &Db, now_ms: i64, deck_ids: Option<&[DeckId]>) -> Result<f64> {
    let since = now_ms - PACE_WINDOW_DAYS * 86_400_000;
    let (f, fparams) = deck_filter_sql(deck_ids, "c.did");
    let sql = if deck_ids.is_none() {
        "SELECT count(*), sum(min(time, 60000)) FROM revlog WHERE id > ? AND ease BETWEEN 1 AND 4 AND time > 0".to_string()
    } else {
        format!("SELECT count(*), sum(min(r.time, 60000)) FROM revlog r JOIN cards c ON c.id = r.cid WHERE r.id > ? AND r.ease BETWEEN 1 AND 4 AND r.time > 0 AND {f}")
    };
    let mut params = vec![since];
    params.extend(fparams);
    let row: Option<(i64, Option<i64>)> = db.get(&sql, rusqlite::params_from_iter(params), |r| Ok((r.get(0)?, r.get(1)?)))?;
    let (n, t) = row.unwrap_or((0, None));
    if n < 10 {
        return Ok(DEFAULT_SECONDS_PER_CARD);
    }
    Ok(t.unwrap_or(0) as f64 / 1000.0 / n as f64)
}

/// A card's revlog, oldest first.
pub fn card_revlog(db: &Db, card_id: CardId) -> Result<Vec<RevlogEntry>> {
    let sql = format!("{} WHERE cid = ? ORDER BY id", select_sql("revlog", &REVLOG_COLUMNS));
    db.all(&sql, [card_id], row_to_revlog)
}

/// Siblings to bury after answering a card (Anki `bury_siblings`, per the answered card's preset).
pub fn siblings_to_bury(db: &Db, nid: NoteId, card_id: CardId, config: &DeckConfig, today: CollectionDay) -> Result<Vec<CardId>> {
    let mut parts: Vec<String> = Vec::new();
    if config.bury_siblings_new {
        parts.push(format!("queue = {}", CardQueue::New as i64));
    }
    if config.bury_siblings_review {
        parts.push(format!("(queue = {} AND due <= {today})", CardQueue::Review as i64));
    }
    if config.bury_siblings_interday {
        parts.push(format!("(queue = {} AND due <= {today})", CardQueue::DayLearn as i64));
    }
    if parts.is_empty() {
        return Ok(vec![]);
    }
    let sql = format!("SELECT id FROM cards WHERE nid = ? AND id != ? AND ({})", parts.join(" OR "));
    db.all(&sql, [nid, card_id], |r| r.get(0))
}

/// Expected answers to finish today's cards: each review or learning card once, each new card
/// once per learning step (at least once).
pub fn expected_answers(counts: &StudyCounts, learn_steps: usize) -> i64 {
    counts.review + counts.learn + counts.new * (learn_steps as i64).max(1)
}

/// Minutes for `answers` at `secs_per_card`, rounded up (0 when there is nothing to do).
pub fn minutes_for(answers: i64, secs_per_card: f64) -> i64 {
    if answers > 0 {
        (answers as f64 * secs_per_card / 60.0).ceil() as i64
    } else {
        0
    }
}
