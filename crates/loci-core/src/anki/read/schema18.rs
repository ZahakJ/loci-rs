//! Schema 15–18 packages: rebuilds the schema-11 JSON objects (note types, decks) from the
//! normalised tables and their protobuf blobs.

use super::{col_bytes, col_i64, col_str, dbe};
use crate::anki::image::Json;
use crate::anki::proto::*;
use crate::db::Db;
use crate::error::Result;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn set(j: &mut Json, key: &str, v: impl Into<Value>) {
    j.insert(key.to_string(), v.into());
}

/// Schema 15+: rebuilds the schema-11 JSON object for each note type from its tables and blobs.
pub(super) fn notetypes_from_tables(db: &Db) -> Result<Vec<Json>> {
    let mut fields: HashMap<i64, Vec<Json>> = HashMap::new();
    let field_rows = db
        .all("SELECT ntid, ord, name, config FROM fields ORDER BY ntid, ord", [], |r| {
            Ok((col_i64(r, 0), col_i64(r, 1), col_str(r, 2), col_bytes(r, 3)))
        })
        .map_err(dbe)?;
    for (ntid, ord, name, config) in field_rows {
        let c = decode_field_config(&config)?;
        let mut f = c.other.clone();
        set(&mut f, "name", name);
        set(&mut f, "ord", ord);
        set(&mut f, "sticky", c.sticky);
        set(&mut f, "rtl", c.rtl);
        set(&mut f, "font", c.font_name);
        set(&mut f, "size", c.font_size);
        set(&mut f, "description", c.description);
        set(&mut f, "plainText", c.plain_text);
        set(&mut f, "collapsed", c.collapsed);
        set(&mut f, "excludeFromSearch", c.exclude_from_search);
        set(&mut f, "preventDeletion", c.prevent_deletion);
        if let Some(id) = c.id {
            set(&mut f, "id", id);
        }
        if let Some(tag) = c.tag {
            set(&mut f, "tag", tag);
        }
        fields.entry(ntid).or_default().push(f);
    }
    let mut templates: HashMap<i64, Vec<Json>> = HashMap::new();
    let template_rows = db
        .all("SELECT ntid, ord, name, mtime_secs, usn, config FROM templates ORDER BY ntid, ord", [], |r| {
            Ok((col_i64(r, 0), col_i64(r, 1), col_str(r, 2), col_bytes(r, 5)))
        })
        .map_err(dbe)?;
    for (ntid, ord, name, config) in template_rows {
        let c = decode_template_config(&config)?;
        let mut t = c.other.clone();
        set(&mut t, "name", name);
        set(&mut t, "ord", ord);
        set(&mut t, "qfmt", c.q_format);
        set(&mut t, "afmt", c.a_format);
        set(&mut t, "bqfmt", c.q_format_browser);
        set(&mut t, "bafmt", c.a_format_browser);
        set(&mut t, "did", if c.target_deck_id > 0 { Value::from(c.target_deck_id) } else { Value::Null });
        set(&mut t, "bfont", c.browser_font_name);
        set(&mut t, "bsize", c.browser_font_size);
        if let Some(id) = c.id {
            set(&mut t, "id", id);
        }
        templates.entry(ntid).or_default().push(t);
    }
    let nt_rows = db
        .all("SELECT id, name, mtime_secs, usn, config FROM notetypes ORDER BY id", [], |r| {
            Ok((col_i64(r, 0), col_str(r, 1), col_i64(r, 2), col_i64(r, 3), col_bytes(r, 4)))
        })
        .map_err(dbe)?;
    let mut out = Vec::new();
    for (id, name, mtime, usn, config) in nt_rows {
        let c = decode_notetype_config(&config)?;
        let mut j = c.other.clone();
        set(&mut j, "id", id);
        set(&mut j, "name", name);
        set(&mut j, "type", c.kind);
        set(&mut j, "mod", mtime);
        set(&mut j, "usn", usn);
        set(&mut j, "sortf", c.sort_field_idx);
        set(&mut j, "did", if c.target_deck_id_unused != 0 { Value::from(c.target_deck_id_unused) } else { Value::Null });
        set(&mut j, "tmpls", Value::Array(templates.remove(&id).unwrap_or_default().into_iter().map(Value::Object).collect()));
        set(&mut j, "flds", Value::Array(fields.remove(&id).unwrap_or_default().into_iter().map(Value::Object).collect()));
        set(&mut j, "css", c.css);
        set(&mut j, "latexPre", c.latex_pre);
        set(&mut j, "latexPost", c.latex_post);
        set(&mut j, "latexsvg", c.latex_svg);
        let req: Vec<Value> = c
            .reqs
            .iter()
            .map(|q| {
                let kind = match q.kind {
                    2 => "all",
                    1 => "any",
                    _ => "none",
                };
                Value::Array(vec![
                    q.card_ord.into(),
                    kind.into(),
                    Value::Array(q.field_ords.iter().map(|x| Value::from(*x)).collect()),
                ])
            })
            .collect();
        set(&mut j, "req", Value::Array(req));
        set(&mut j, "originalStockKind", c.original_stock_kind);
        if let Some(oid) = c.original_id {
            set(&mut j, "originalId", oid);
        }
        out.push(j);
    }
    Ok(out)
}

/// Schema 15+: rebuilds the schema-11 deck JSON from the `decks` table (name, common and kind blobs).
pub(super) fn decks_from_tables(db: &Db) -> Result<Vec<Json>> {
    let rows = db
        .all("SELECT id, name, mtime_secs, usn, common, kind FROM decks ORDER BY id", [], |r| {
            Ok((col_i64(r, 0), col_str(r, 1), col_i64(r, 2), col_i64(r, 3), col_bytes(r, 4), col_bytes(r, 5)))
        })
        .map_err(dbe)?;
    let mut out = Vec::new();
    for (id, name, mtime, usn, common, kind) in rows {
        let c = decode_deck_common(&common)?;
        let k = decode_deck_kind(&kind)?;
        let day = c.last_day_studied;
        let pair = |v: i64| Value::Array(vec![day.into(), v.into()]);
        let mut j = c.other.clone();
        set(&mut j, "id", id);
        set(&mut j, "name", name.split('\u{1f}').collect::<Vec<_>>().join("::"));
        set(&mut j, "mod", mtime);
        set(&mut j, "usn", usn);
        set(&mut j, "collapsed", c.study_collapsed);
        set(&mut j, "browserCollapsed", c.browser_collapsed);
        set(&mut j, "newToday", pair(c.new_studied));
        set(&mut j, "revToday", pair(c.review_studied));
        set(&mut j, "lrnToday", pair(c.learning_studied));
        set(&mut j, "timeToday", pair(c.milliseconds_studied));
        match k {
            DeckKindProto::Normal(x) => {
                set(&mut j, "dyn", 0);
                set(&mut j, "conf", x.config_id);
                set(&mut j, "desc", x.description);
                set(&mut j, "md", x.markdown_description);
                set(&mut j, "extendNew", x.extend_new);
                set(&mut j, "extendRev", x.extend_review);
                if let Some(v) = x.review_limit {
                    set(&mut j, "reviewLimit", v);
                }
                if let Some(v) = x.new_limit {
                    set(&mut j, "newLimit", v);
                }
                let limit = |d: &DayLimitProto| serde_json::json!({ "limit": d.limit, "today": d.today });
                if let Some(d) = &x.review_limit_today {
                    set(&mut j, "reviewLimitToday", limit(d));
                }
                if let Some(d) = &x.new_limit_today {
                    set(&mut j, "newLimitToday", limit(d));
                }
                if let Some(dr) = x.desired_retention {
                    set(&mut j, "desiredRetention", (dr * 100.0).round() as i64);
                }
            }
            DeckKindProto::Filtered(f) => {
                set(&mut j, "dyn", 1);
                set(&mut j, "desc", "");
                set(&mut j, "resched", f.reschedule);
                let terms: Vec<Value> = f
                    .search_terms
                    .iter()
                    .map(|t| Value::Array(vec![t.search.as_str().into(), t.limit.into(), t.order.into()]))
                    .collect();
                set(&mut j, "terms", Value::Array(terms));
                set(&mut j, "separate", true);
                set(
                    &mut j,
                    "delays",
                    if f.delays.is_empty() {
                        Value::Null
                    } else {
                        Value::Array(f.delays.iter().map(|d| crate::anki::deck_config::json_num(*d)).collect())
                    },
                );
                set(&mut j, "previewDelay", f.preview_delay);
                set(&mut j, "previewAgainSecs", f.preview_again_secs);
                set(&mut j, "previewHardSecs", f.preview_hard_secs);
                set(&mut j, "previewGoodSecs", f.preview_good_secs);
            }
        }
        out.push(j);
    }
    Ok(out)
}
