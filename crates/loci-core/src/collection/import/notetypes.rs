//! Note-type merging for the Anki importer:
//! matching by id, by identical shape, adoption of empty built-ins, suffixing of conflicting
//! names and the `originalId` copy for a different note type under a known id.

use crate::anki::AnkiImage;
use crate::collection::internals::{CollectionCore, IdTable};
use crate::collection::rows::to_json;
use crate::error::Result;
use crate::template::compute_req;
use crate::types::*;
use std::collections::HashMap;

fn schema_matches(a: &Notetype, b: &Notetype) -> bool {
    a.kind == b.kind
        && a.fields.len() == b.fields.len()
        && a.templates.len() == b.templates.len()
        && a.fields.iter().zip(&b.fields).all(|(x, y)| x.name.to_lowercase() == y.name.to_lowercase())
        && a.templates.iter().zip(&b.templates).all(|(x, y)| x.name.to_lowercase() == y.name.to_lowercase())
}

fn unique_name(core: &CollectionCore, table: &str, name: &str, self_id: i64) -> Result<String> {
    let mut n = name.to_string();
    while core
        .db()
        .scalar::<i64, _>(&format!("SELECT 1 FROM {table} WHERE name = ? AND id != ?"), rusqlite::params![n, self_id])?
        .is_some()
    {
        n.push('+');
    }
    Ok(n)
}

fn insert_notetype(core: &CollectionCore, nt: &Notetype) -> Result<()> {
    core.db().run(
        "INSERT INTO notetypes (id, name, kind, css, latex_pre, latex_post, sort_field, config, mtime, usn) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![nt.id, nt.name, nt.kind as i64, nt.css, nt.latex_pre, nt.latex_post, nt.sort_field, to_json(&nt.config), nt.mtime, nt.usn],
    )?;
    for f in &nt.fields {
        core.db().run(
            "INSERT INTO fields (ntid, ord, name, config) VALUES (?, ?, ?, ?)",
            rusqlite::params![nt.id, f.ord, f.name, to_json(&f.config)],
        )?;
    }
    for t in &nt.templates {
        core.db().run(
            "INSERT INTO templates (ntid, ord, name, qfmt, afmt, bqfmt, bafmt, config) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![nt.id, t.ord, t.name, t.qfmt, t.afmt, t.bqfmt, t.bafmt, to_json(&t.config)],
        )?;
    }
    core.invalidate_caches();
    Ok(())
}

fn delete_notetype_rows(core: &CollectionCore, id: NotetypeId) -> Result<()> {
    core.db().run("DELETE FROM notetypes WHERE id = ?", [id])?;
    core.db().run("DELETE FROM fields WHERE ntid = ?", [id])?;
    core.db().run("DELETE FROM templates WHERE ntid = ?", [id])?;
    core.invalidate_caches();
    Ok(())
}

fn replace_notetype(core: &CollectionCore, nt: &Notetype) -> Result<()> {
    delete_notetype_rows(core, nt.id)?;
    insert_notetype(core, nt)
}

/// Note types here whose name equals `name` (case-insensitive, the column collates NOCASE).
fn notetypes_named(core: &CollectionCore, name: &str) -> Result<Vec<Notetype>> {
    let ids: Vec<i64> = core.db().all("SELECT id FROM notetypes WHERE name = ? ORDER BY id", [name], |r| r.get(0))?;
    ids.into_iter().map(|id| core.notetype(id)).collect()
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Same fields, same templates (formats compared with whitespace collapsed) and same CSS: one note type under two ids.
fn identical_notetype(a: &Notetype, b: &Notetype) -> bool {
    schema_matches(a, b)
        && squash(&a.css) == squash(&b.css)
        && a.templates
            .iter()
            .zip(&b.templates)
            .all(|(t, u)| squash(&t.qfmt) == squash(&u.qfmt) && squash(&t.afmt) == squash(&u.afmt))
}

/// An existing note type the incoming one may replace: no notes use it, same kind, and for Loci's
/// built-ins the same number of fields and templates (so an Anki "Basic" replaces Loci's empty
/// "Basic", but a 3-field note type called "Basic" does not).
fn can_adopt(core: &CollectionCore, local: &Notetype, incoming: &Notetype) -> Result<bool> {
    if local.kind != incoming.kind {
        return Ok(false);
    }
    if core.db().scalar::<i64, _>("SELECT 1 FROM notes WHERE ntid = ? LIMIT 1", [local.id])?.is_some() {
        return Ok(false);
    }
    if local.config.builtin.is_some()
        && (local.fields.len() != incoming.fields.len() || local.templates.len() != incoming.templates.len())
    {
        return Ok(false);
    }
    Ok(true)
}

/// JS `{ ...existing.config, ...nt.config }`: the incoming value wins wherever it is set.
fn merge_config(existing: &NotetypeConfig, incoming: &NotetypeConfig) -> NotetypeConfig {
    NotetypeConfig {
        req: incoming.req.clone(),
        builtin: incoming.builtin.or(existing.builtin),
        cloze_one_by_one: incoming.cloze_one_by_one.or(existing.cloze_one_by_one),
        occlusion_mode: incoming.occlusion_mode.or(existing.occlusion_mode),
        original_id: incoming.original_id.or(existing.original_id),
        original_stock_kind: incoming.original_stock_kind.or(existing.original_stock_kind),
        use_own_styling: incoming.use_own_styling.or(existing.use_own_styling),
    }
}

/// Merges note types; returns source id → target id.
pub(super) fn import_notetypes(
    core: &CollectionCore,
    image: &AnkiImage,
    conflict: ConflictPolicy,
    report: &mut ImportReport,
) -> Result<HashMap<NotetypeId, NotetypeId>> {
    let mut map = HashMap::new();
    for incoming in &image.notetypes {
        let mut nt = incoming.clone();
        nt.usn = -1;
        if nt.config.req.is_empty() {
            nt.config.req = compute_req(&nt);
        }
        let existing = if core.db().scalar::<i64, _>("SELECT 1 FROM notetypes WHERE id = ?", [nt.id])?.is_some() {
            Some(core.notetype(nt.id)?)
        } else {
            None
        };
        match existing {
            None => {
                let same_name = notetypes_named(core, &nt.name)?;
                if let Some(identical) = same_name.iter().find(|x| identical_notetype(x, &nt)) {
                    // A used (or unused) note type here is the very same note type under another id: share it.
                    map.insert(incoming.id, identical.id);
                    continue;
                }
                let mut adoptable = None;
                for x in &same_name {
                    if x.id != nt.id && can_adopt(core, x, &nt)? {
                        adoptable = Some(x);
                        break;
                    }
                }
                if let Some(adoptable) = adoptable {
                    // An empty note type of the same name and shape (typically one of Loci's built-ins in a
                    // fresh collection): the imported one takes its place under its Anki id and plain name,
                    // keeping Loci's markers (built-in kind, cloze/occlusion options) when the shape is identical.
                    if schema_matches(adoptable, &nt) {
                        let keep = &adoptable.config;
                        if keep.builtin.is_some() && nt.config.builtin.is_none() {
                            nt.config.builtin = keep.builtin;
                        }
                        if keep.cloze_one_by_one.is_some() && nt.config.cloze_one_by_one.is_none() {
                            nt.config.cloze_one_by_one = keep.cloze_one_by_one;
                        }
                        if keep.occlusion_mode.is_some() && nt.config.occlusion_mode.is_none() {
                            nt.config.occlusion_mode = keep.occlusion_mode;
                        }
                    }
                    delete_notetype_rows(core, adoptable.id)?;
                }
                nt.name = unique_name(core, "notetypes", &nt.name, nt.id)?;
                insert_notetype(core, &nt)?;
                report.notetypes_added += 1;
                map.insert(incoming.id, nt.id);
            }
            Some(existing) if schema_matches(&existing, &nt) => {
                let update = match conflict {
                    ConflictPolicy::Theirs => existing.mtime != nt.mtime,
                    ConflictPolicy::Newer => existing.mtime < nt.mtime,
                    ConflictPolicy::Keep => false,
                };
                if update {
                    nt.name = unique_name(core, "notetypes", &nt.name, nt.id)?;
                    nt.config = merge_config(&existing.config, &nt.config);
                    replace_notetype(core, &nt)?;
                }
                map.insert(incoming.id, existing.id);
            }
            Some(_) => {
                let reuse = core
                    .all_notetypes()?
                    .into_iter()
                    .find(|x| x.config.original_id == Some(incoming.id) && schema_matches(x, &nt));
                if let Some(reuse) = reuse {
                    map.insert(incoming.id, reuse.id);
                } else {
                    let mut copy = nt.clone();
                    copy.id = core.new_id(IdTable::Notetypes)?;
                    copy.config.original_id = Some(incoming.id);
                    copy.name = unique_name(core, "notetypes", &copy.name, copy.id)?;
                    insert_notetype(core, &copy)?;
                    report.notetypes_added += 1;
                    report.warnings.push(format!(
                        "Note type \"{}\" differs from the one with the same id here; it was added as \"{}\".",
                        incoming.name, copy.name
                    ));
                    map.insert(incoming.id, copy.id);
                }
            }
        }
    }
    core.invalidate_caches();
    Ok(map)
}
