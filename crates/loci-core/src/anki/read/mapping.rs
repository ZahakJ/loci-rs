//! Schema-11 JSON objects → Loci types.

use super::{jb, js};
use crate::anki::deck_config::{int, num, obj};
use crate::anki::image::{AnkiDeck, Json};
use crate::error::{LociError, Result};
use crate::types::*;
use serde_json::Value;

pub(super) fn notetype_from_json(j: &Json, warnings: &mut Vec<String>) -> Result<Notetype> {
    let id = int(j.get("id"), 0);
    let name = js(j.get("name"), &format!("Note type {id}"));
    if id == 0 {
        return Err(LociError::corrupt(format!("Note type \"{name}\" has no id.")));
    }
    let kind = if int(j.get("type"), 0) == 1 { NotetypeKind::Cloze } else { NotetypeKind::Standard };
    let arr = |key: &str| -> Vec<Json> {
        match j.get(key) {
            Some(Value::Array(a)) => a.iter().map(|x| obj(Some(x))).collect(),
            _ => Vec::new(),
        }
    };
    let mut flds = arr("flds");
    let mut tmpls = arr("tmpls");
    // Anki sorts by `ord` and then renumbers 0..n (ord may be null in old files).
    let ord_of = |x: &Json| num(x.get("ord"), 1e9);
    flds.sort_by(|a, b| ord_of(a).total_cmp(&ord_of(b)));
    tmpls.sort_by(|a, b| ord_of(a).total_cmp(&ord_of(b)));
    let fields: Vec<Field> = flds
        .iter()
        .enumerate()
        .map(|(i, f)| Field {
            ord: i as i64,
            name: js(f.get("name"), &format!("Field {}", i + 1)),
            config: FieldConfig {
                sticky: jb(f.get("sticky")),
                rtl: jb(f.get("rtl")),
                font: js(f.get("font"), ""),
                size: int(f.get("size"), 0),
                description: js(f.get("description"), ""),
                plain_text: jb(f.get("plainText")),
                collapsed: jb(f.get("collapsed")),
            },
        })
        .collect();
    let templates: Vec<Template> = tmpls
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let did = int(t.get("did"), 0);
            Template {
                ord: i as i64,
                name: js(t.get("name"), &format!("Card {}", i + 1)),
                qfmt: js(t.get("qfmt"), ""),
                afmt: js(t.get("afmt"), ""),
                bqfmt: js(t.get("bqfmt"), ""),
                bafmt: js(t.get("bafmt"), ""),
                config: TemplateConfig { deck_override: if did > 0 { Some(did) } else { None } },
            }
        })
        .collect();
    if fields.is_empty() {
        return Err(LociError::corrupt(format!("Note type \"{name}\" has no fields.")));
    }
    if templates.is_empty() {
        return Err(LociError::corrupt(format!("Note type \"{name}\" has no card templates.")));
    }
    if kind == NotetypeKind::Cloze && templates.len() > 1 {
        warnings.push(format!(
            "Cloze note type \"{name}\" has {} templates; only the first is used by cloze cards.",
            templates.len()
        ));
    }
    let mut req = Vec::new();
    if let Some(Value::Array(rs)) = j.get("req") {
        for r in rs {
            if let Value::Array(parts) = r {
                if parts.len() >= 3 {
                    if let (Some(Value::String(kind)), Some(Value::Array(ords))) = (parts.get(1), parts.get(2)) {
                        let kind = match kind.as_str() {
                            "all" => ReqKind::All,
                            "any" => ReqKind::Any,
                            "none" => ReqKind::None,
                            _ => continue,
                        };
                        req.push(Requirement(int(parts.first(), 0), kind, ords.iter().map(|x| int(Some(x), 0)).collect()));
                    }
                }
            }
        }
    }
    let mut config = NotetypeConfig { req, ..NotetypeConfig::default() };
    let stock = int(j.get("originalStockKind"), 0);
    if stock > 0 {
        config.original_stock_kind = Some(stock);
    }
    // stock kind 6 is the built-in image-occlusion type
    if stock == 6 && kind == NotetypeKind::Cloze && fields.len() >= 4 {
        config.builtin = Some(BuiltinKind::ImageOcclusion);
        config.occlusion_mode = Some(OcclusionMode::HideAll);
    }
    let sortf = int(j.get("sortf"), 0);
    Ok(Notetype {
        id,
        name,
        kind,
        css: js(j.get("css"), ""),
        latex_pre: js(j.get("latexPre"), ""),
        latex_post: js(j.get("latexPost"), ""),
        sort_field: if sortf >= 0 && (sortf as usize) < fields.len() { sortf } else { 0 },
        fields,
        templates,
        config,
        mtime: int(j.get("mod"), 0),
        usn: int(j.get("usn"), 0),
    })
}

pub(super) fn deck_from_json(j: Json) -> AnkiDeck {
    let pair = |v: Option<&Value>| -> (i64, i64) {
        match v {
            Some(Value::Array(a)) => (int(a.first(), 0), int(a.get(1), 0)),
            _ => (0, 0),
        }
    };
    let new = pair(j.get("newToday"));
    let rev = pair(j.get("revToday"));
    let lrn = pair(j.get("lrnToday"));
    let time = pair(j.get("timeToday"));
    // Anki's upgrade keeps only the counters whose day equals the most recent day.
    let day = new.0.max(rev.0).max(lrn.0).max(time.0);
    let at = |p: (i64, i64)| if p.0 == day { p.1 } else { 0 };
    let today = DeckToday { day, new: at(new), rev: at(rev), lrn: at(lrn), time_ms: at(time) };
    let filtered = jb(j.get("dyn"));
    let id = int(j.get("id"), 0);
    let desired_retention = match j.get("desiredRetention") {
        Some(Value::Number(n)) => n.as_f64().filter(|dr| *dr > 0.0).map(|dr| if dr > 1.0 { dr / 100.0 } else { dr }),
        _ => None,
    };
    AnkiDeck {
        id,
        name: js(j.get("name"), &format!("Deck {id}")),
        description: js(j.get("desc"), ""),
        config_id: if filtered { 0 } else { int(j.get("conf"), 1) },
        filtered,
        collapsed: jb(j.get("collapsed")),
        browser_collapsed: jb(j.get("browserCollapsed")),
        today,
        extend_new: int(j.get("extendNew"), 0),
        extend_rev: int(j.get("extendRev"), 0),
        desired_retention,
        mtime: int(j.get("mod"), 0),
        usn: int(j.get("usn"), 0),
        original_json: Some(j),
    }
}
