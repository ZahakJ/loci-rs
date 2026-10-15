//! Export (internals §13): builds an `AnkiImage` from the
//! collection (a deck with its children, or everything) and writes a LEGACY_2 .apkg with
//! `anki::write_legacy_package`, which every Anki from 2.1 to 26.x and AnkiDroid import. Media:
//! every stored file for a whole-collection export, the files referenced by the exported notes and
//! their note types for a deck export. (`installSampleDeck` lives in `notes.rs`.)

use crate::anki::{
    extract_media_references, extract_template_media_references, percent_decode, write_legacy_package, AnkiCollectionConfig,
    AnkiDeck, AnkiDeckConfig, AnkiImage, AnkiMediaEntry, WriteLegacyOptions,
};
use crate::collection::import::source_today;
use crate::collection::internals::{meta, CollectionCore};
use crate::collection::media::MEDIA_PREFIX;
use crate::collection::rows::{
    parse_deck_today, parse_json_object, row_to_card, row_to_note, row_to_revlog, select_sql, CARD_COLUMNS, NOTE_COLUMNS,
    REVLOG_COLUMNS,
};
use crate::collection::sched_context::{marks, subtree_deck_ids, SQL_CHUNK};
use crate::collection::service::Collection;
use crate::error::{LociError, Result};
use crate::types::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const PHASE_COLLECT: &str = "Collecting notes";
pub const PHASE_WRITE: &str = "Writing media";
pub const PHASE_DONE: &str = "Done";

/// `(id, name, description, config_id, collapsed, today, config, mtime, usn)` of a deck row.
type DeckRowTuple = (i64, String, String, i64, i64, String, String, i64, i64);

fn decks_for_export(core: &CollectionCore, deck_ids: Option<&[DeckId]>) -> Result<Vec<AnkiDeck>> {
    let rows: Vec<DeckRowTuple> =
        core.db().all("SELECT id, name, description, config_id, collapsed, today, config, mtime, usn FROM decks", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        })?;
    let mut out = Vec::new();
    for (id, name, description, config_id, collapsed, today, config, mtime, usn) in rows {
        if deck_ids.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let extra = parse_json_object(&config);
        let mut deck = AnkiDeck::blank(id, name);
        deck.description = description;
        deck.config_id = config_id;
        deck.collapsed = collapsed != 0;
        deck.today = parse_deck_today(&today);
        deck.extend_new = extra.get("extendNew").and_then(|v| v.as_i64()).unwrap_or(0);
        deck.extend_rev = extra.get("extendRev").and_then(|v| v.as_i64()).unwrap_or(0);
        deck.mtime = mtime;
        deck.usn = usn;
        deck.original_json = extra.get("originalAnkiJson").and_then(|v| v.as_object()).cloned();
        out.push(deck);
    }
    Ok(out)
}

fn rows_by_ids<T>(
    core: &CollectionCore,
    table: &str,
    columns: &[&str],
    column: &str,
    ids: &[i64],
    f: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for part in ids.chunks(SQL_CHUNK) {
        out.extend(core.db().all(
            &format!("{} WHERE {column} IN ({})", select_sql(table, columns), marks(part.len())),
            rusqlite::params_from_iter(part.iter()),
            f,
        )?);
    }
    Ok(out)
}

/// The Anki crt recorded by the first import into an empty collection, if any.
fn source_crt(core: &CollectionCore) -> Result<Option<(Seconds, Option<i64>)>> {
    let Some(raw) = core.get_meta(meta::SOURCE_CRT)? else { return Ok(None) };
    let o = parse_json_object(&raw);
    let crt = o.get("crt").and_then(|v| v.as_f64()).filter(|c| c.is_finite() && *c > 0.0);
    Ok(crt.map(|c| (c as i64, o.get("creationOffset").and_then(|v| v.as_i64()))))
}

/// Builds the export image (no I/O beyond the database; media are read lazily).
pub fn build_export_image(core: &CollectionCore, options: &ExportOptions) -> Result<AnkiImage> {
    let deck_ids = subtree_deck_ids(core, options.deck_id)?;
    let mut cards: Vec<Card> = match &deck_ids {
        None => core.db().all(&format!("{} ORDER BY id", select_sql("cards", &CARD_COLUMNS)), [], row_to_card)?,
        Some(ids) => rows_by_ids(core, "cards", &CARD_COLUMNS, "did", ids, row_to_card)?,
    };
    let mut nids: Vec<NoteId> = Vec::new();
    let mut seen: HashSet<NoteId> = HashSet::new();
    for c in &cards {
        if seen.insert(c.nid) {
            nids.push(c.nid);
        }
    }
    let notes: Vec<Note> = match &deck_ids {
        None => core.db().all(&format!("{} ORDER BY id", select_sql("notes", &NOTE_COLUMNS)), [], row_to_note)?,
        Some(_) => rows_by_ids(core, "notes", &NOTE_COLUMNS, "id", &nids, row_to_note)?,
    };
    let nt_ids: HashSet<NotetypeId> = notes.iter().map(|n| n.ntid).collect();
    let notetypes: Vec<Notetype> = match &deck_ids {
        None => core.all_notetypes()?,
        Some(_) => core.all_notetypes()?.into_iter().filter(|nt| nt_ids.contains(&nt.id)).collect(),
    };
    let mut decks = decks_for_export(core, deck_ids.as_deref())?;
    let mut preset_ids: Vec<DeckConfigId> = vec![1];
    for d in &decks {
        if !preset_ids.contains(&d.config_id) {
            preset_ids.push(d.config_id);
        }
    }
    let deck_configs: Vec<AnkiDeckConfig> = preset_ids
        .into_iter()
        .filter_map(|id| core.preset(id).ok())
        .map(|p| AnkiDeckConfig { id: p.id, name: p.name, config: p.config, mtime: p.mtime, usn: p.usn, original_json: None })
        .collect();
    let revlog: Vec<RevlogEntry> = if options.include_scheduling {
        match &deck_ids {
            None => core.db().all(&format!("{} ORDER BY id", select_sql("revlog", &REVLOG_COLUMNS)), [], row_to_revlog)?,
            Some(_) => {
                let cids: Vec<CardId> = cards.iter().map(|c| c.id).collect();
                rows_by_ids(core, "revlog", &REVLOG_COLUMNS, "cid", &cids, row_to_revlog)?
            }
        }
    } else {
        vec![]
    };

    let mut media: BTreeMap<String, AnkiMediaEntry> = BTreeMap::new();
    if options.include_media {
        let stored: HashMap<String, (i64, String)> = core
            .db()
            .all("SELECT name, size, sha1 FROM media", [], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?
            .into_iter()
            .collect();
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        if deck_ids.is_none() {
            wanted.extend(stored.keys().cloned());
        } else {
            for n in &notes {
                for f in &n.fields {
                    for r in extract_media_references(f) {
                        wanted.insert(percent_decode(&r));
                    }
                }
            }
            for nt in &notetypes {
                for t in &nt.templates {
                    for r in extract_template_media_references(&format!("{}{}", t.qfmt, t.afmt)) {
                        wanted.insert(percent_decode(&r));
                    }
                }
                for r in extract_template_media_references(&nt.css) {
                    wanted.insert(percent_decode(&r));
                }
            }
        }
        for name in wanted {
            if let Some((size, sha1)) = stored.get(&name) {
                media.insert(
                    name,
                    AnkiMediaEntry { zip_entry: String::new(), size: (*size).max(0) as u64, sha1: Some(sha1.clone()) },
                );
            }
        }
    }
    let blobs = core.blobs.clone();
    let media_reader = Box::new(move |name: &str| -> Result<Vec<u8>> {
        blobs
            .get(&format!("{MEDIA_PREFIX}{name}"))?
            .ok_or_else(|| LociError::media(format!("Media file \"{name}\" is missing from storage")))
    });

    let settings = core.settings()?;
    let now = core.now();
    let creation = core.get_meta(meta::CREATION_OFFSET)?.and_then(|v| v.parse::<i64>().ok());
    let source = source_crt(core)?;
    let config = AnkiCollectionConfig {
        sched_ver: 2,
        sched2021: true,
        rollover: settings.rollover_hour,
        creation_offset: Some(
            source.and_then(|(_, o)| o).or(creation).unwrap_or_else(|| core.tz_offset(core.crt().unwrap_or(0) * 1000)),
        ),
        local_offset: Some(core.tz_offset(now)),
        next_pos: core.get_meta(meta::NEXT_POS)?.and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0) as i64,
        fsrs: deck_configs.iter().any(|p| p.config.scheduler == SchedulerKind::Fsrs),
        collapse_time: ((settings.learn_ahead_minutes as f64) * 60.0).round() as i64,
        est_times: true,
        cur_deck: options.deck_id.unwrap_or(1),
        last_unburied: None,
        raw: serde_json::Map::new(),
    };
    let crt = match source {
        Some((crt, _)) => crt,
        None => core.crt()?,
    };
    let mut image = AnkiImage {
        layout: PackageLayout::Legacy2,
        crt,
        mod_ms: now,
        schema_version: 11,
        config,
        notetypes,
        notetype_originals: HashMap::new(),
        decks: vec![],
        deck_configs,
        notes,
        cards: vec![],
        revlog,
        media,
        media_reader,
        warnings: vec![],
    };
    if source.is_some() {
        // Day numbers are relative to crt: move them onto the source crt's numbering (the inverse of
        // the import shift; importers, Anki's and ours, shift them back by the same amount).
        let today = core.today()?;
        let delta = source_today(&image, now, &|ms| core.tz_offset(ms)) - today;
        if delta != 0 {
            for c in &mut cards {
                if matches!(c.queue, CardQueue::Review | CardQueue::DayLearn) || c.ctype == CardType::Review {
                    c.due += delta;
                }
                if c.ctype == CardType::Review && c.odue != 0 {
                    c.odue += delta;
                }
            }
            for d in &mut decks {
                if d.today.day == today {
                    d.today.day = today + delta;
                }
            }
        }
    }
    image.cards = cards;
    image.decks = decks;
    Ok(image)
}

pub fn export_package(core: &CollectionCore, options: &ExportOptions, progress: &mut dyn FnMut(Progress)) -> Result<Vec<u8>> {
    progress(Progress { phase: PHASE_COLLECT.into(), done: 0, total: 2 });
    let image = build_export_image(core, options)?;
    progress(Progress { phase: PHASE_WRITE.into(), done: 1, total: 2 });
    let bytes = write_legacy_package(
        &image,
        WriteLegacyOptions {
            include_scheduling: options.include_scheduling,
            include_media: options.include_media,
            now: Some(core.now()),
        },
    )?;
    progress(Progress { phase: PHASE_DONE.into(), done: 2, total: 2 });
    Ok(bytes)
}

impl Collection {
    pub fn export_package(&self, options: &ExportOptions, progress: &mut dyn FnMut(Progress)) -> Result<Vec<u8>> {
        export_package(&self.core, options, progress)
    }
}
