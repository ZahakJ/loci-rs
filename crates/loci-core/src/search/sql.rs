//! Search AST → SQL over Loci's schema (recall `src/core/search/sql.ts`; aliases: `c` cards,
//! `n` notes; subqueries use decks, notetypes, templates, deck_configs, revlog, notes_fts).
//!
//! Most operators compile to exact SQL. Some cannot be expressed in plain SQLite without custom
//! functions (we register none, so the SQL matches the TypeScript build byte for byte):
//!   `field:value` (needs the field at a per-notetype index), `re:`, `nc:`, `w:`, `sc:`,
//!   `tag:` with wildcards, `tag:re:`, `dupe:`.
//! For these the compiler emits a SQL *superset* in `sql` plus `post_filter(row)`, which gives the
//! exact answer for each candidate row. When such a term sits inside OR/NOT, the truth value of
//! every exact SQL term in that subtree is selected as an extra column (`extra_columns`, aliases
//! `l0`, `l1`, …) so the post-filter can evaluate the whole boolean expression exactly.
//! `build_search_select` assembles the full statement; `run_search` executes it and applies the filter.
//!
//! Text semantics: bare text and `nc:`/`w:` match the note's HTML-stripped text (`notes.plain`);
//! terms of 3+ characters without wildcards use the FTS5 trigram index (Unicode case-insensitive),
//! shorter or wildcard terms use LIKE (ASCII case-insensitive, like Anki). `field:` and `re:` match
//! the raw field HTML, case-insensitively, per field (`^`/`$` anchor at field boundaries).

use crate::db::Db;
use crate::error::Result;
use crate::template::normalize::strip_combining;
use crate::text::{field_checksum, split_fields, split_tags, strip_html, strip_html_preserving_media};
use crate::types::{CardQueue, CardType};
use rusqlite::types::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::parse::{
    glob_to_like, glob_to_regex_source, has_wildcards, parse_search, unescape_text, CompareOp, FieldMode, IdTarget, PropKind,
    PropValue, SearchError, SearchNode, StateKind,
};

/// FSRS-6 default decay (w20) used when a card's data has no `decay`.
pub const FSRS6_DEFAULT_DECAY: f64 = 0.1542;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub ntid: i64,
    pub ord: i64,
    pub name: String,
}

/// What the compiler needs from the collection (TypeScript `SearchContext`; phase 2's
/// `CollectionInternals` fills it from `col_meta` and the scheduler's timing).
#[derive(Debug, Clone, Default)]
pub struct SearchContext {
    /// collection day number (days since crt, rollover applied)
    pub today: i64,
    /// now, ms since epoch
    pub now_ms: i64,
    /// collection creation, seconds since epoch (local day start at rollover hour)
    pub crt: i64,
    /// hour (0–23) at which a new day starts
    pub rollover_hour: i64,
    /// deck for `deck:current`
    pub current_deck_id: Option<i64>,
    /// start of the next collection day, seconds; default crt + (today+1)·86400
    pub next_day_at: Option<i64>,
    /// field names of every note type (for `field:value`); `run_search` loads them when None
    pub fields: Option<Vec<FieldInfo>>,
}

impl SearchContext {
    fn next_day_at(&self) -> i64 {
        self.next_day_at.unwrap_or(self.crt + (self.today + 1) * 86400)
    }
}

/// A candidate row handed to the post-filter.
#[derive(Debug, Clone, PartialEq)]
pub struct PostFilterRow {
    pub cid: i64,
    pub nid: i64,
    pub ntid: i64,
    pub flds: String,
    pub tags: String,
    pub plain: String,
    /// the `l0`, `l1`, … columns of `extra_columns`, in order (1 = true)
    pub extra: Vec<i64>,
}

pub type PostFilter = Box<dyn Fn(&PostFilterRow) -> bool>;

pub struct CompiledSearch {
    /// boolean SQL expression over `c` and `n`; a superset of the result when `post_filter` is set
    pub sql: String,
    pub params: Vec<Value>,
    /// extra select expressions (`expr AS lN, …`) the post-filter needs; bind `extra_params` before `params`
    pub extra_columns: Option<String>,
    pub extra_params: Vec<Value>,
    /// exact test for candidate rows (selected with n.flds, n.tags, n.plain, n.ntid and extra_columns)
    pub post_filter: Option<PostFilter>,
}

impl std::fmt::Debug for CompiledSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledSearch")
            .field("sql", &self.sql)
            .field("params", &self.params)
            .field("extra_columns", &self.extra_columns)
            .field("post_filter", &self.post_filter.is_some())
            .finish()
    }
}

type Test = Box<dyn Fn(&PostFilterRow) -> bool>;

struct Leaf {
    sql: String,
    params: Vec<Value>,
    /// present when `sql` is only a superset and the exact answer needs code
    test: Option<Test>,
}

enum Compiled {
    Leaf(Leaf),
    And(Vec<Compiled>),
    Or(Vec<Compiled>),
    Not(Box<Compiled>),
}

const LIKE_ESC: &str = " ESCAPE '\\'";

fn exact(sql: impl Into<String>, params: Vec<Value>) -> Compiled {
    Compiled::Leaf(Leaf { sql: sql.into(), params, test: None })
}

fn inexact(sql: impl Into<String>, params: Vec<Value>, test: Test) -> Compiled {
    Compiled::Leaf(Leaf { sql: sql.into(), params, test: Some(test) })
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// A JavaScript number as a SQL parameter: INTEGER when integral, REAL otherwise.
fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        Value::Integer(n as i64)
    } else {
        Value::Real(n)
    }
}

pub(crate) fn int(n: i64) -> Value {
    Value::Integer(n)
}

pub(crate) fn text(s: impl Into<String>) -> Value {
    Value::Text(s.into())
}

/// A number interpolated into SQL text the way JavaScript's template literal prints it.
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Case-insensitive, dot-all regex; honours a leading `(?-i)` like Anki's Rust regex.
pub fn user_regex(pattern: &str) -> std::result::Result<fancy_regex::Regex, SearchError> {
    let (flags, src) = if let Some(rest) = pattern.strip_prefix("(?-i)") {
        ("(?s)", rest)
    } else if let Some(rest) = pattern.strip_prefix("(?i)") {
        ("(?is)", rest)
    } else {
        ("(?is)", pattern)
    };
    fancy_regex::Regex::new(&format!("{flags}{src}"))
        .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))
}

fn glob_regex(glob: &str, any: &str) -> std::result::Result<regex::Regex, SearchError> {
    regex::Regex::new(&format!("(?is)^{}$", glob_to_regex_source(glob, any)))
        .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))
}

static CLOZE_MARKER: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(r"\{\{c\d+(?:,\d+)*::((?:(?!\{\{c\d)[\s\S])*?)(?:::[^{}]*?)?\}\}").expect("valid regex")
});

/// Strips cloze markers but keeps their text (`{{c1::a::hint}}` → `a`).
pub(crate) fn strip_cloze_markers(s: &str) -> String {
    let mut cur = s.to_string();
    loop {
        let next = CLOZE_MARKER.replace_all(&cur, "$1").into_owned();
        if next == cur {
            return cur;
        }
        cur = next;
    }
}

fn lookup_fields<'a>(ctx: &'a SearchContext, field: &str) -> std::result::Result<Vec<&'a FieldInfo>, SearchError> {
    let re = glob_regex(field, ".")?;
    Ok(ctx.fields.as_deref().unwrap_or(&[]).iter().filter(|f| re.is_match(&f.name)).collect())
}

static WS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\s+").expect("valid regex"));

/// Text term → SQL on notes.plain (FTS trigram for literal terms of 3+ chars, else LIKE).
fn text_leaf(glob: &str) -> Compiled {
    let literal = WS.replace_all(&unescape_text(glob), " ").into_owned();
    if !has_wildcards(glob) && literal.trim().chars().count() >= 3 && literal == literal.trim() {
        return exact(
            "n.id IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?)",
            vec![text(format!("plain : \"{}\"", literal.replace('"', "\"\"")))],
        );
    }
    exact(format!("n.plain LIKE ?{LIKE_ESC}"), vec![text(format!("%{}%", glob_to_like(&WS.replace_all(glob, " "))))])
}

/// Exact `deck:` SQL for a LIKE pattern: the deck itself or any child, matched on did or odid.
fn deck_sql(like_pattern: &str) -> Compiled {
    let sub = format!("SELECT id FROM decks WHERE name LIKE ?{LIKE_ESC} OR name LIKE ?{LIKE_ESC}");
    exact(
        format!("(c.did IN ({sub}) OR (c.odid != 0 AND c.odid IN ({sub})))"),
        vec![text(like_pattern), text(format!("{like_pattern}::%")), text(like_pattern), text(format!("{like_pattern}::%"))],
    )
}

fn compare_sql(expr: &str, op: CompareOp) -> String {
    format!("{expr} {} ?", op.as_str())
}

const CD_JSON: &str = "json_extract(c.data, '$.cd')";

fn cd_value(key: &str) -> String {
    format!("(CASE WHEN json_valid({CD_JSON}) THEN json_extract({CD_JSON}, '$.{key}') END)")
}

/// FSRS retrievability of card `c` now, as SQL: every card with a memory state (data.s > 0 and
/// data.d present) whatever its queue, NULL otherwise. Days since the last review come from
/// data.lrt, else from the due day of a review card (due − ivl), else from the card's latest
/// graded revlog row (learning and relearning cards without lrt), else 0. Shared by `prop:r`,
/// the browser's retrievability column and its sort.
pub fn retrievability_sql(today: i64, now_ms: i64) -> String {
    let s = "json_extract(c.data, '$.s')";
    let lrt = "json_extract(c.data, '$.lrt')";
    let decay = format!("COALESCE(json_extract(c.data, '$.decay'), {})", js_num(FSRS6_DEFAULT_DECAY));
    let last_revlog = "(SELECT max(rl.id) FROM revlog rl WHERE rl.cid = c.id AND rl.ease BETWEEN 1 AND 4)";
    let now_secs = js_num(now_ms as f64 / 1000.0);
    let elapsed = format!(
        "(CASE\n    WHEN {lrt} > 0 THEN MAX(0, ({now_secs} - {lrt}) / 86400.0)\n    WHEN c.due < 1000000000 AND c.type IN ({}, {}) THEN MAX(0, {today} - (c.due - c.ivl))\n    ELSE COALESCE(MAX(0, ({now_ms} - {last_revlog}) / 86400000.0), 0) END)",
        CardType::Review as i32,
        CardType::Relearn as i32
    );
    format!("(CASE WHEN {s} > 0 AND json_extract(c.data, '$.d') IS NOT NULL THEN power(1.0 + (power(0.9, -1.0 / {decay}) - 1.0) * {elapsed} / {s}, -{decay}) END)")
}

fn day_range_sql(column: &str, op: CompareOp, day: i64, ctx: &SearchContext, scale: i64) -> (String, Vec<Value>) {
    // Day `day` (0 = today, -1 = yesterday) spans [start, end) in `column` units (scale 1000 = ms).
    let start = (ctx.next_day_at() + (day - 1) * 86400) * scale;
    let end = start + 86400 * scale;
    match op {
        CompareOp::Eq => (format!("{column} >= ? AND {column} < ?"), vec![int(start), int(end)]),
        CompareOp::Ne => (format!("({column} < ? OR {column} >= ?)"), vec![int(start), int(end)]),
        CompareOp::Lt => (format!("{column} < ?"), vec![int(start)]),
        CompareOp::Le => (format!("{column} < ?"), vec![int(end)]),
        CompareOp::Gt => (format!("{column} >= ?"), vec![int(end)]),
        CompareOp::Ge => (format!("{column} >= ?"), vec![int(start)]),
    }
}

fn compile_node(node: &SearchNode, ctx: &SearchContext) -> std::result::Result<Compiled, SearchError> {
    let q = |queue: CardQueue| queue as i32;
    let ty = |t: CardType| t as i32;
    Ok(match node {
        SearchNode::All => exact("1", vec![]),
        SearchNode::And { children } => Compiled::And(children.iter().map(|c| compile_node(c, ctx)).collect::<std::result::Result<_, _>>()?),
        SearchNode::Or { children } => Compiled::Or(children.iter().map(|c| compile_node(c, ctx)).collect::<std::result::Result<_, _>>()?),
        SearchNode::Not { child } => Compiled::Not(Box::new(compile_node(child, ctx)?)),
        SearchNode::Text { text: t } => {
            if t.is_empty() {
                exact("1", vec![])
            } else {
                text_leaf(t)
            }
        }

        SearchNode::Field { field, value, mode } => {
            let matches = lookup_fields(ctx, field)?;
            if matches.is_empty() {
                return Ok(exact("0", vec![]));
            }
            let mut by_nt: HashMap<i64, Vec<usize>> = HashMap::new();
            let mut ntids: Vec<i64> = Vec::new();
            for f in &matches {
                let ords = by_nt.entry(f.ntid).or_default();
                if ords.is_empty() {
                    ntids.push(f.ntid);
                }
                if let Ok(o) = usize::try_from(f.ord) {
                    ords.push(o);
                }
            }
            let test: Box<dyn Fn(&str) -> bool> = match mode {
                FieldMode::Regex => {
                    let re = user_regex(value)?;
                    Box::new(move |v| re.is_match(v).unwrap_or(false))
                }
                FieldMode::Nc => {
                    let re = glob_regex(&strip_combining(value), ".")?;
                    Box::new(move |v| re.is_match(&strip_combining(v)))
                }
                FieldMode::Glob => {
                    let re = glob_regex(value, ".")?;
                    Box::new(move |v| re.is_match(v))
                }
            };
            let mut superset = String::new();
            let mut params: Vec<Value> = ntids.iter().map(|n| int(*n)).collect();
            if *mode == FieldMode::Glob {
                let lit = unescape_text(value);
                if !has_wildcards(value) && !lit.is_empty() && lit.is_ascii() {
                    superset = format!(" AND n.flds LIKE ?{LIKE_ESC}");
                    params.push(text(format!("%{}%", glob_to_like(value))));
                }
            }
            inexact(
                format!("(n.ntid IN ({}){superset})", placeholders(ntids.len())),
                params,
                Box::new(move |row| {
                    let Some(ords) = by_nt.get(&row.ntid) else { return false };
                    let fl = split_fields(&row.flds);
                    ords.iter().any(|o| test(fl.get(*o).map(String::as_str).unwrap_or("")))
                }),
            )
        }

        SearchNode::Deck { name } => {
            let lit = unescape_text(name);
            if lit == "*" {
                exact("1", vec![])
            } else if lit.to_lowercase() == "filtered" {
                exact("c.odid != 0", vec![])
            } else {
                deck_sql(&glob_to_like(name))
            }
        }
        SearchNode::DeckCurrent => match ctx.current_deck_id {
            None => exact("0", vec![]),
            Some(id) => exact(
                "c.did IN (SELECT d.id FROM decks d, (SELECT name AS p FROM decks WHERE id = ?) cur WHERE d.name = cur.p OR substr(d.name, 1, length(cur.p) + 2) = (cur.p || '::') COLLATE NOCASE)",
                vec![int(id)],
            ),
        },
        SearchNode::Notetype { name } => exact(format!("n.ntid IN (SELECT id FROM notetypes WHERE name LIKE ?{LIKE_ESC})"), vec![text(glob_to_like(name))]),
        SearchNode::CardOrd { ord } => exact("c.ord = ?", vec![int(*ord)]),
        SearchNode::CardName { name } => exact(
            format!("EXISTS (SELECT 1 FROM templates t JOIN notetypes nt ON nt.id = t.ntid WHERE t.ntid = n.ntid AND t.name LIKE ?{LIKE_ESC} AND (nt.kind = 1 OR t.ord = c.ord))"),
            vec![text(glob_to_like(name))],
        ),

        SearchNode::Tag { tag } => {
            if !has_wildcards(tag) {
                let lit = glob_to_like(tag);
                return Ok(exact(format!("(n.tags LIKE ?{LIKE_ESC} OR n.tags LIKE ?{LIKE_ESC})"), vec![text(format!("% {lit} %")), text(format!("% {lit}::%"))]));
            }
            let re = regex::Regex::new(&format!("(?is)^{}(?:::.*)?$", glob_to_regex_source(tag, "\\S")))
                .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))?;
            inexact(
                format!("n.tags LIKE ?{LIKE_ESC}"),
                vec![text(format!("% {}%", glob_to_like(tag)))],
                Box::new(move |row| split_tags(&row.tags).iter().any(|t| re.is_match(t))),
            )
        }
        SearchNode::TagNone => exact("trim(n.tags) = ''", vec![]),
        SearchNode::TagRegex { pattern } => {
            let re = user_regex(pattern)?;
            inexact("trim(n.tags) != ''", vec![], Box::new(move |row| split_tags(&row.tags).iter().any(|t| re.is_match(t).unwrap_or(false))))
        }

        SearchNode::State { state } => {
            let now_secs = ctx.now_ms.div_euclid(1000);
            match state {
                StateKind::Due => exact(
                    format!(
                        "((c.queue IN ({}, {}) AND c.due <= ?) OR (c.queue IN ({}, {}) AND c.due <= ?))",
                        q(CardQueue::Review),
                        q(CardQueue::DayLearn),
                        q(CardQueue::Learn),
                        q(CardQueue::Preview)
                    ),
                    vec![int(ctx.today), int(now_secs)],
                ),
                StateKind::New => exact(format!("c.type = {}", ty(CardType::New)), vec![]),
                StateKind::Learn => exact(format!("c.queue IN ({}, {})", q(CardQueue::Learn), q(CardQueue::DayLearn)), vec![]),
                StateKind::Review => exact(format!("c.type IN ({}, {})", ty(CardType::Review), ty(CardType::Relearn)), vec![]),
                StateKind::Suspended => exact(format!("c.queue = {}", q(CardQueue::Suspended)), vec![]),
                StateKind::Buried => exact(format!("c.queue IN ({}, {})", q(CardQueue::SchedBuried), q(CardQueue::UserBuried)), vec![]),
                StateKind::BuriedSibling => exact(format!("c.queue = {}", q(CardQueue::SchedBuried)), vec![]),
                StateKind::BuriedManually => exact(format!("c.queue = {}", q(CardQueue::UserBuried)), vec![]),
            }
        }
        SearchNode::Flag { flag } => exact("(c.flags & 7) = ?", vec![int(*flag)]),

        SearchNode::Prop { prop, op, value, key } => {
            let v = match value {
                PropValue::Num(n) => *n,
                PropValue::Text(_) => 0.0,
            };
            let key = key.clone().unwrap_or_default();
            match prop {
                PropKind::Ivl => exact(compare_sql("c.ivl", *op), vec![num(v)]),
                PropKind::Reps => exact(compare_sql("c.reps", *op), vec![num(v)]),
                PropKind::Lapses => exact(compare_sql("c.lapses", *op), vec![num(v)]),
                PropKind::Ease => exact(compare_sql("c.factor", *op), vec![num((v * 1000.0).round())]),
                PropKind::Pos => exact(format!("(c.type = {} AND {})", ty(CardType::New), compare_sql("c.due", *op)), vec![num(v)]),
                PropKind::Due => {
                    let days = v as i64;
                    exact(
                        format!(
                            "((c.queue IN ({}, {}) AND c.due {o} ?) OR (c.queue IN ({}, {}) AND ((c.due - ?) / 86400) {o} ?))",
                            q(CardQueue::Review),
                            q(CardQueue::DayLearn),
                            q(CardQueue::Learn),
                            q(CardQueue::Preview),
                            o = op.as_str()
                        ),
                        vec![int(ctx.today + days), int(ctx.next_day_at()), int(days)],
                    )
                }
                PropKind::S => exact(compare_sql("json_extract(c.data, '$.s')", *op), vec![num(v)]),
                PropKind::D => exact(compare_sql("json_extract(c.data, '$.d')", *op), vec![num(v * 9.0 + 1.0)]),
                PropKind::R => exact(compare_sql(&retrievability_sql(ctx.today, ctx.now_ms), *op), vec![num(v)]),
                PropKind::Rated => {
                    let (sql, params) = day_range_sql("r.id", *op, v as i64, ctx, 1000);
                    exact(format!("EXISTS (SELECT 1 FROM revlog r WHERE r.cid = c.id AND r.ease BETWEEN 1 AND 4 AND {sql})"), params)
                }
                PropKind::Resched => {
                    let (sql, params) = day_range_sql("r.id", *op, v as i64, ctx, 1000);
                    exact(format!("EXISTS (SELECT 1 FROM revlog r WHERE r.cid = c.id AND r.ease = 0 AND r.type IN (4, 5) AND {sql})"), params)
                }
                PropKind::Cdn => exact(compare_sql(&format!("CAST({} AS REAL)", cd_value(&key)), *op), vec![num(v)]),
                PropKind::Cds => {
                    let s = match value {
                        PropValue::Text(s) => s.clone(),
                        PropValue::Num(n) => js_num(*n),
                    };
                    let sql = if *op == CompareOp::Eq { format!("{} = ?", cd_value(&key)) } else { format!("COALESCE({} != ?, 1)", cd_value(&key)) };
                    exact(sql, vec![text(s)])
                }
            }
        }

        SearchNode::Rated { days, ease } => {
            let cutoff = (ctx.next_day_at() - 86400 * days) * 1000;
            match ease {
                Some(e) => exact("c.id IN (SELECT cid FROM revlog WHERE id > ? AND ease = ?)", vec![int(cutoff), int(*e)]),
                None => exact("c.id IN (SELECT cid FROM revlog WHERE id > ? AND ease BETWEEN 1 AND 4)", vec![int(cutoff)]),
            }
        }
        SearchNode::Resched { days } => {
            exact("c.id IN (SELECT cid FROM revlog WHERE id > ? AND ease = 0 AND type IN (4, 5))", vec![int((ctx.next_day_at() - 86400 * days) * 1000)])
        }
        SearchNode::Introduced { days } => exact(
            "c.id IN (SELECT cid FROM revlog WHERE ease BETWEEN 1 AND 4 AND type IN (0, 1, 2, 3) GROUP BY cid HAVING MIN(id) > ?)",
            vec![int((ctx.next_day_at() - 86400 * days) * 1000)],
        ),
        SearchNode::Added { days } => exact("c.id > ?", vec![int((ctx.next_day_at() - 86400 * days) * 1000)]),
        SearchNode::Edited { days } => exact("n.mtime > ?", vec![int(ctx.next_day_at() - 86400 * days)]),
        SearchNode::Ids { target, ids } => {
            let col = match target {
                IdTarget::Nid => "n.id",
                IdTarget::Cid => "c.id",
                IdTarget::Mid => "n.ntid",
                IdTarget::Did => "c.did",
            };
            exact(format!("{col} IN ({})", placeholders(ids.len())), ids.iter().map(|i| int(*i)).collect())
        }
        SearchNode::Dupe { ntid, text: t } => {
            let ntid = *ntid;
            let t = t.clone();
            inexact(
                "(n.ntid = ? AND n.csum = ?)",
                vec![int(ntid), int(i64::from(field_checksum(&t)))],
                Box::new(move |row| row.ntid == ntid && strip_html_preserving_media(split_fields(&row.flds).first().map(String::as_str).unwrap_or("")) == t),
            )
        }
        SearchNode::HasCd { key } => exact(format!("{} IS NOT NULL", cd_value(key)), vec![]),
        SearchNode::Preset { name } => exact(
            format!("c.did IN (SELECT d.id FROM decks d JOIN deck_configs p ON p.id = d.config_id WHERE p.name LIKE ?{LIKE_ESC})"),
            vec![text(glob_to_like(name))],
        ),
        SearchNode::Regex { pattern } => {
            let re = user_regex(pattern)?;
            inexact("1", vec![], Box::new(move |row| split_fields(&row.flds).iter().any(|f| re.is_match(f).unwrap_or(false))))
        }
        SearchNode::Nc { text: t } => {
            let re = regex::Regex::new(&format!("(?is){}", glob_to_regex_source(&strip_combining(t), ".")))
                .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))?;
            inexact("1", vec![], Box::new(move |row| re.is_match(&strip_combining(&row.plain))))
        }
        SearchNode::Word { text: t } => {
            const W: &str = "[\\p{L}\\p{N}\\p{M}_]";
            let re = fancy_regex::Regex::new(&format!("(?i)(?<!{W}){}(?!{W})", glob_to_regex_source(t, W)))
                .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))?;
            let lit: String = unescape_text(t).chars().filter(|c| *c != '*' && *c != '_').collect();
            let superset = !has_wildcards(t) && !lit.is_empty() && lit.is_ascii();
            inexact(
                if superset { format!("n.plain LIKE ?{LIKE_ESC}") } else { "1".to_string() },
                if superset { vec![text(format!("%{}%", glob_to_like(t)))] } else { vec![] },
                Box::new(move |row| re.is_match(&row.plain).unwrap_or(false)),
            )
        }
        SearchNode::Sc { text: t } => {
            let re = regex::Regex::new(&format!("(?is){}", glob_to_regex_source(t, ".")))
                .map_err(|e| SearchError::new(format!("Invalid regular expression: {e}"), 0, 0))?;
            inexact("1", vec![], Box::new(move |row| split_fields(&row.flds).iter().any(|f| re.is_match(&strip_html(&strip_cloze_markers(f))))))
        }
    })
}

fn has_code(c: &Compiled) -> bool {
    match c {
        Compiled::Leaf(l) => l.test.is_some(),
        Compiled::Not(child) => has_code(child),
        Compiled::And(ch) | Compiled::Or(ch) => ch.iter().any(has_code),
    }
}

/// SQL for a compiled tree: exact if pure; otherwise a superset (positive) or subset (negative).
fn to_sql(c: &Compiled, positive: bool, params: &mut Vec<Value>) -> String {
    match c {
        Compiled::Leaf(l) => {
            if l.test.is_none() || positive {
                params.extend(l.params.iter().cloned());
                l.sql.clone()
            } else {
                "0".to_string()
            }
        }
        Compiled::Not(child) => format!("NOT ({})", to_sql(child, !positive, params)),
        Compiled::And(ch) => ch.iter().map(|x| format!("({})", to_sql(x, positive, params))).collect::<Vec<_>>().join(" AND "),
        Compiled::Or(ch) => ch.iter().map(|x| format!("({})", to_sql(x, positive, params))).collect::<Vec<_>>().join(" OR "),
    }
}

/// Builds the exact evaluator for a mixed subtree, registering SQL-only leaves as extra columns.
fn build_eval(c: Compiled, cols: &mut Vec<String>, extra_params: &mut Vec<Value>) -> Test {
    match c {
        Compiled::Leaf(l) => match l.test {
            Some(t) => t,
            None => {
                let idx = cols.len();
                cols.push(format!("(CASE WHEN {} THEN 1 ELSE 0 END) AS l{idx}", l.sql));
                extra_params.extend(l.params);
                Box::new(move |row| row.extra.get(idx) == Some(&1))
            }
        },
        Compiled::Not(child) => {
            let f = build_eval(*child, cols, extra_params);
            Box::new(move |row| !f(row))
        }
        Compiled::And(ch) => {
            let fs: Vec<Test> = ch.into_iter().map(|x| build_eval(x, cols, extra_params)).collect();
            Box::new(move |row| fs.iter().all(|f| f(row)))
        }
        Compiled::Or(ch) => {
            let fs: Vec<Test> = ch.into_iter().map(|x| build_eval(x, cols, extra_params)).collect();
            Box::new(move |row| fs.iter().any(|f| f(row)))
        }
    }
}

/// Compiles an AST with a context.
pub fn search_to_sql(ast: &SearchNode, ctx: &SearchContext) -> std::result::Result<CompiledSearch, SearchError> {
    let root = compile_node(ast, ctx)?;
    if !has_code(&root) {
        let mut params = Vec::new();
        let sql = to_sql(&root, true, &mut params);
        return Ok(CompiledSearch { sql, params, extra_columns: None, extra_params: Vec::new(), post_filter: None });
    }
    // Pure top-level conjuncts go straight to SQL; mixed ones need columns + code.
    let conjuncts = match root {
        Compiled::And(ch) => ch,
        other => vec![other],
    };
    let mut params: Vec<Value> = Vec::new();
    let mut where_parts: Vec<String> = Vec::new();
    let mut mixed: Vec<Compiled> = Vec::new();
    for cj in conjuncts {
        where_parts.push(format!("({})", to_sql(&cj, true, &mut params)));
        if has_code(&cj) {
            mixed.push(cj);
        }
    }
    let mut cols: Vec<String> = Vec::new();
    let mut extra_params: Vec<Value> = Vec::new();
    let evals: Vec<Test> = mixed.into_iter().map(|c| build_eval(c, &mut cols, &mut extra_params)).collect();
    Ok(CompiledSearch {
        sql: where_parts.join(" AND "),
        params,
        extra_columns: if cols.is_empty() { None } else { Some(cols.join(", ")) },
        extra_params,
        post_filter: Some(Box::new(move |row| evals.iter().all(|f| f(row)))),
    })
}

/// Parses and compiles a query string.
pub fn compile_search(query: &str, ctx: &SearchContext) -> std::result::Result<CompiledSearch, SearchError> {
    search_to_sql(&parse_search(query)?, ctx)
}

/// Full SELECT for a compiled search: rows of matching cards (`cid`, `nid`, `ntid`, plus the
/// columns the post-filter needs). Bind the returned params in order. `order_by` is inserted verbatim.
pub fn build_search_select(cs: &CompiledSearch, order_by: &str) -> (String, Vec<Value>) {
    let extra = if cs.post_filter.is_some() {
        format!(
            ", n.flds AS flds, n.tags AS tags, n.plain AS plain{}",
            cs.extra_columns.as_ref().map(|c| format!(", {c}")).unwrap_or_default()
        )
    } else {
        String::new()
    };
    let sql = format!("SELECT c.id AS cid, n.id AS nid, n.ntid AS ntid{extra} FROM cards c JOIN notes n ON n.id = c.nid WHERE {} ORDER BY {order_by}", cs.sql);
    let mut params = cs.extra_params.clone();
    params.extend(cs.params.iter().cloned());
    (sql, params)
}

/// Registers the SQL functions the compiled search needs that the bundled SQLite lacks
/// (`power`, used by `retrievability_sql`; node:sqlite shipped it). Idempotent; `run_search`
/// calls it, and `Db::configure` may call it once per connection instead.
pub fn register_sql_functions(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function("power", 2, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let base: Option<f64> = ctx.get(0)?;
        let exp: Option<f64> = ctx.get(1)?;
        Ok(match (base, exp) {
            (Some(b), Some(e)) => Some(b.powf(e)),
            _ => None,
        })
    })
}

/// Builds the field index (`SearchContext.fields`) from the `fields` table.
pub fn load_field_index(db: &Db) -> Result<Vec<FieldInfo>> {
    db.all("SELECT ntid, ord, name FROM fields ORDER BY ntid, ord", [], |r| {
        Ok(FieldInfo { ntid: r.get(0)?, ord: r.get(1)?, name: r.get(2)? })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Cards,
    Notes,
}

/// Executes a search. Returns card ids (`Cards`, ascending) or distinct note ids (`Notes`,
/// ascending). Loads the field index when the context has none.
pub fn run_search(db: &Db, query: &str, ctx: &SearchContext, mode: SearchMode) -> Result<Vec<i64>> {
    let ast = parse_search(query)?;
    run_search_ast(db, &ast, ctx, mode)
}

/// `run_search` for an already parsed query.
pub fn run_search_ast(db: &Db, ast: &SearchNode, ctx: &SearchContext, mode: SearchMode) -> Result<Vec<i64>> {
    let loaded;
    let ctx = if ctx.fields.is_some() {
        ctx
    } else {
        loaded = SearchContext { fields: Some(load_field_index(db)?), ..ctx.clone() };
        &loaded
    };
    let cs = search_to_sql(ast, ctx)?;
    register_sql_functions(db.conn())?;
    let (sql, params) = build_search_select(&cs, if mode == SearchMode::Notes { "n.id, c.id" } else { "c.id" });
    let filtered = cs.post_filter.is_some();
    let rows: Vec<PostFilterRow> = db.all(&sql, rusqlite::params_from_iter(params), |r| {
        let mut row = PostFilterRow {
            cid: r.get(0)?,
            nid: r.get(1)?,
            ntid: r.get(2)?,
            flds: String::new(),
            tags: String::new(),
            plain: String::new(),
            extra: Vec::new(),
        };
        if filtered {
            row.flds = r.get(3)?;
            row.tags = r.get(4)?;
            row.plain = r.get(5)?;
            let mut i = 6;
            while let Ok(v) = r.get::<_, i64>(i) {
                row.extra.push(v);
                i += 1;
            }
        }
        Ok(row)
    })?;
    let rows: Vec<PostFilterRow> = match &cs.post_filter {
        Some(f) => rows.into_iter().filter(|r| f(r)).collect(),
        None => rows,
    };
    if mode == SearchMode::Cards {
        return Ok(rows.iter().map(|r| r.cid).collect());
    }
    let mut seen: Vec<i64> = Vec::new();
    for r in rows {
        if !seen.contains(&r.nid) {
            seen.push(r.nid);
        }
    }
    Ok(seen)
}
