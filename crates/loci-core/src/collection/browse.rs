//! Browser (DESIGN §11; recall `src/core/collection/browse.ts`): search with Anki syntax
//! (`search::sql`), sorting, a page of rows, the sidebar, saved searches and bulk actions with
//! Anki semantics. Every bulk action records one undo entry.
//!
//! Sorting runs in SQL (exact search terms filter in SQL too; post-filter-only terms such as `re:`
//! are applied by the compiled post-filter while preserving the SQL order). Question/answer cells
//! are rendered for the requested page only, stripped of HTML and cut to 120 characters.
//!
//! Submodules: `cells` (row rendering), `bulk` (bulk actions).

pub mod bulk;
pub mod cells;

use crate::collection::counter::scheduler_deck_counts;
use crate::collection::decks::deck_tree;
use crate::collection::internals::CollectionCore;
use crate::collection::notes::all_tags;
use crate::collection::sched_context::{sched_clock, search_context};
use crate::collection::service::Collection;
use crate::error::{LociError, Result};
use crate::i18n::msg;
use crate::search::{
    build_search_select, compile_search, register_sql_functions, retrievability_sql, run_search, CompiledSearch, PostFilterRow,
    SearchMode,
};
use crate::types::*;
use std::collections::HashMap;

pub use bulk::{bulk, BulkResult};
pub use cells::cell_text;

// ───────────────────────── search ─────────────────────────

fn effective_query(query: &str) -> &str {
    let q = query.trim();
    if q.is_empty() {
        "deck:*"
    } else {
        q
    }
}

fn compile(core: &CollectionCore, query: &str) -> Result<CompiledSearch> {
    compile_search(effective_query(query), &search_context(core, None)?).map_err(LociError::from)
}

/// Card-level due key: new cards by position (after everything else), learning by timestamp
/// mapped to days, reviews by day (Anki's browser sorts by type, then due).
fn due_key_sql(today: i64, next_day_at: i64) -> String {
    format!(
        "(CASE WHEN c.type = {} THEN 1000000000 + c.due WHEN c.due > 1000000000 THEN {today} + (c.due - {next_day_at}) / 86400.0 + 1 ELSE c.due END)",
        CardType::New as i64
    )
}

/// ORDER BY expression for a column. Notes mode aggregates over each note's cards.
fn order_sql(column: BrowseColumn, mode: BrowseMode, today: i64, next_day_at: i64, now_ms: i64) -> String {
    let per_card: Option<(String, &str)> = match column {
        BrowseColumn::Due => Some((due_key_sql(today, next_day_at), "min")),
        BrowseColumn::Interval => Some(("c.ivl".into(), "avg")),
        BrowseColumn::Ease => Some(("c.factor".into(), "avg")),
        BrowseColumn::Difficulty => Some(("json_extract(c.data, '$.d')".into(), "avg")),
        BrowseColumn::Stability => Some(("json_extract(c.data, '$.s')".into(), "avg")),
        BrowseColumn::Retrievability => Some((retrievability_sql(today, now_ms), "avg")),
        BrowseColumn::Reps => Some(("c.reps".into(), "sum")),
        BrowseColumn::Lapses => Some(("c.lapses".into(), "sum")),
        _ => None,
    };
    if let Some((expr, agg)) = per_card {
        if mode == BrowseMode::Cards {
            return expr;
        }
        let inner = regex::Regex::new(r"\bc\.").map(|re| re.replace_all(&expr, "c2.").into_owned()).unwrap_or(expr);
        return format!("(SELECT {agg}({inner}) FROM cards c2 WHERE c2.nid = n.id)");
    }
    match column {
        BrowseColumn::SortField | BrowseColumn::Question | BrowseColumn::Answer => "n.sfld COLLATE NOCASE".into(),
        BrowseColumn::Deck => "(SELECT name FROM decks WHERE id = c.did) COLLATE NOCASE".into(),
        BrowseColumn::Notetype => "(SELECT name FROM notetypes WHERE id = n.ntid) COLLATE NOCASE".into(),
        BrowseColumn::Template => {
            if mode == BrowseMode::Cards {
                format!(
                    "(SELECT CASE WHEN nt.kind = {} THEN printf('%05d', c.ord) ELSE t.name END FROM notetypes nt LEFT JOIN templates t ON t.ntid = nt.id AND t.ord = c.ord WHERE nt.id = n.ntid) COLLATE NOCASE",
                    NotetypeKind::Cloze as i64
                )
            } else {
                "(SELECT name FROM notetypes WHERE id = n.ntid) COLLATE NOCASE".into()
            }
        }
        BrowseColumn::Tags => "n.tags COLLATE NOCASE".into(),
        BrowseColumn::Created => if mode == BrowseMode::Cards { "c.id" } else { "n.id" }.into(),
        BrowseColumn::Modified => if mode == BrowseMode::Cards { "c.mtime" } else { "n.mtime" }.into(),
        BrowseColumn::CardCount => "(SELECT count(*) FROM cards c2 WHERE c2.nid = n.id)".into(),
        _ => "c.id".into(),
    }
}

/// Matching ids in sort order (cards mode: card ids; notes mode: note ids).
fn sorted_ids(core: &CollectionCore, query: &str, mode: BrowseMode, sort: &BrowseSort) -> Result<Vec<i64>> {
    let clock = sched_clock(core)?;
    let cs = compile(core, query)?;
    let dir = if sort.descending { "DESC" } else { "ASC" };
    let key = order_sql(sort.column, mode, clock.today, clock.next_day_at, clock.now_ms);
    let tie = if mode == BrowseMode::Cards { format!("c.id {dir}") } else { format!("n.id {dir}, c.ord") };
    let (sql, params) = build_search_select(&cs, &format!("{key} {dir} NULLS LAST, {tie}"));
    register_sql_functions(core.db().conn())?;
    let filtered = cs.post_filter.is_some();
    let rows: Vec<PostFilterRow> = core.db().all(&sql, rusqlite::params_from_iter(params), |r| {
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
    if mode == BrowseMode::Cards {
        return Ok(rows.iter().map(|r| r.cid).collect());
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in rows {
        if seen.insert(r.nid) {
            out.push(r.nid);
        }
    }
    Ok(out)
}

pub fn search_ids(core: &CollectionCore, query: &str, mode: BrowseMode) -> Result<Vec<i64>> {
    let sm = if mode == BrowseMode::Notes { SearchMode::Notes } else { SearchMode::Cards };
    run_search(core.db(), effective_query(query), &search_context(core, None)?, sm)
}

// ───────────────────────── rows ─────────────────────────

pub fn browse(core: &CollectionCore, req: &BrowseRequest) -> Result<BrowseResult> {
    let mode = req.mode;
    let ids = sorted_ids(core, &req.query, mode, &req.sort)?;
    let offset = req.offset.max(0) as usize;
    let limit = req.limit.max(0) as usize;
    let clock = sched_clock(core)?;
    let ctx = cells::RowCtx {
        core,
        offset: core.tz_offset(clock.now_ms),
        clock,
        deck_names: core.all_decks()?.into_iter().map(|d| (d.id, d.name)).collect::<HashMap<_, _>>(),
    };
    let mut rows = Vec::new();
    for id in ids.iter().skip(offset).take(limit) {
        if let Some(r) = cells::build_row(&ctx, mode, *id)? {
            rows.push(r);
        }
    }
    Ok(BrowseResult { rows, total: ids.len() as i64, ids })
}

// ───────────────────────── sidebar ─────────────────────────

/// Anki search term with a quoted, escaped value: `"deck:My \*deck"`.
pub fn quoted_term(prefix: &str, value: &str) -> String {
    let mut out = format!("\"{prefix}:");
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '*' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn item(kind: SidebarKind, label: String, query: String) -> SidebarItem {
    SidebarItem { kind, label, query, count: None, children: None }
}

fn deck_item(n: &DeckTreeNode) -> SidebarItem {
    SidebarItem {
        kind: SidebarKind::Deck,
        label: n.deck.name.rsplit("::").next().unwrap_or("").to_string(),
        query: quoted_term("deck", &n.deck.name),
        count: Some(n.total_cards),
        children: Some(n.children.iter().map(deck_item).collect()),
    }
}

/// Drops empty `children` lists (the TypeScript `delete`s the key).
fn prune(items: &mut [SidebarItem]) {
    for it in items.iter_mut() {
        match it.children.as_mut() {
            Some(ch) if ch.is_empty() => it.children = None,
            Some(ch) => prune(ch),
            None => {}
        }
    }
}

pub fn sidebar(core: &CollectionCore) -> Result<Vec<SidebarItem>> {
    let ctx = search_context(core, None)?;
    let count = |q: &str| -> Result<i64> { Ok(run_search(core.db(), q, &ctx, SearchMode::Cards)?.len() as i64) };
    let saved: Vec<(String, String)> =
        core.db()
            .all("SELECT name, query FROM saved_searches ORDER BY name COLLATE NOCASE", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let tree = deck_tree(core, &scheduler_deck_counts)?;
    let flag_names = ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"];
    let flag_counts: HashMap<i64, i64> = core
        .db()
        .all("SELECT flags & 7 AS f, count(*) AS n FROM cards WHERE flags & 7 != 0 GROUP BY 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .into_iter()
        .collect();
    let tags = all_tags(core)?;
    let tag_tree = || -> Vec<SidebarItem> {
        let mut root: Vec<SidebarItem> = Vec::new();
        // path (lower-case) → index path into the tree
        for t in &tags {
            let parts: Vec<&str> = t.tag.split("::").collect();
            let mut level: &mut Vec<SidebarItem> = &mut root;
            for i in 0..parts.len() {
                let path = parts[..=i].join("::");
                let key = path.to_lowercase();
                let pos = match level.iter().position(|it| it.query.to_lowercase() == quoted_term("tag", &key).to_lowercase()) {
                    Some(p) => p,
                    None => {
                        level.push(SidebarItem {
                            kind: SidebarKind::Tag,
                            label: parts[i].to_string(),
                            query: quoted_term("tag", &path),
                            count: None,
                            children: Some(vec![]),
                        });
                        level.len() - 1
                    }
                };
                let it = &mut level[pos];
                if i == parts.len() - 1 {
                    it.count = Some(t.count);
                }
                level = it.children.get_or_insert_with(Vec::new);
            }
        }
        prune(&mut root);
        root
    };
    let today_children = vec![
        SidebarItem {
            count: Some(count("is:due")?),
            ..item(SidebarKind::Today, msg("browse.sidebar.due", "Due", &[]), "is:due".into())
        },
        SidebarItem {
            count: Some(count("added:1")?),
            ..item(SidebarKind::Today, msg("browse.sidebar.addedToday", "Added today", &[]), "added:1".into())
        },
        SidebarItem {
            count: Some(count("rated:1")?),
            ..item(SidebarKind::Today, msg("browse.sidebar.studiedToday", "Studied today", &[]), "rated:1".into())
        },
        SidebarItem {
            count: Some(count("rated:1:1")?),
            ..item(SidebarKind::Today, msg("browse.sidebar.forgottenToday", "Forgotten today", &[]), "rated:1:1".into())
        },
    ];
    let mut notetype_children = Vec::new();
    for nt in core.all_notetypes()? {
        let n = core.db().count("SELECT count(*) FROM notes WHERE ntid = ?", [nt.id])?;
        notetype_children
            .push(SidebarItem { count: Some(n), ..item(SidebarKind::Notetype, nt.name.clone(), quoted_term("note", &nt.name)) });
    }
    Ok(vec![
        SidebarItem {
            children: Some(saved.into_iter().map(|(name, query)| item(SidebarKind::Saved, name, query)).collect()),
            ..item(SidebarKind::Saved, msg("browse.sidebar.saved", "Saved searches", &[]), String::new())
        },
        SidebarItem {
            children: Some(today_children),
            ..item(SidebarKind::Today, msg("browse.sidebar.today", "Today", &[]), String::new())
        },
        SidebarItem {
            children: Some(
                flag_names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| SidebarItem {
                        count: Some(flag_counts.get(&(i as i64 + 1)).copied().unwrap_or(0)),
                        ..item(
                            SidebarKind::Flag,
                            msg(&format!("browse.flag.{}", name.to_lowercase()), name, &[]),
                            format!("flag:{}", i + 1),
                        )
                    })
                    .collect(),
            ),
            ..item(SidebarKind::Flag, msg("browse.sidebar.flags", "Flags", &[]), String::new())
        },
        SidebarItem {
            children: Some(tree.children.iter().map(deck_item).collect()),
            ..item(SidebarKind::Deck, msg("browse.sidebar.decks", "Decks", &[]), "deck:*".into())
        },
        SidebarItem {
            children: Some(notetype_children),
            ..item(SidebarKind::Notetype, msg("browse.sidebar.notetypes", "Note types", &[]), String::new())
        },
        SidebarItem {
            children: Some(tag_tree()),
            ..item(SidebarKind::Tag, msg("browse.sidebar.tags", "Tags", &[]), String::new())
        },
    ])
}

impl Collection {
    pub fn browse(&self, req: &BrowseRequest) -> Result<BrowseResult> {
        self.rollover()?;
        browse(&self.core, req)
    }
    pub fn search_ids(&self, query: &str, mode: BrowseMode) -> Result<Vec<i64>> {
        self.rollover()?;
        search_ids(&self.core, query, mode)
    }
    pub fn sidebar(&self) -> Result<Vec<SidebarItem>> {
        self.rollover()?;
        sidebar(&self.core)
    }
    pub fn bulk(&self, card_ids: &[CardId], action: &BulkAction) -> Result<BulkResult> {
        bulk(&self.core, card_ids, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_terms_escape_search_specials() {
        assert_eq!(quoted_term("deck", "My *deck_1"), "\"deck:My \\*deck\\_1\"");
        assert_eq!(effective_query("   "), "deck:*");
        assert!(order_sql(BrowseColumn::Due, BrowseMode::Notes, 3, 1000, 5).starts_with("(SELECT min((CASE WHEN c2.type"));
        assert_eq!(order_sql(BrowseColumn::Created, BrowseMode::Notes, 0, 0, 0), "n.id");
    }
}
