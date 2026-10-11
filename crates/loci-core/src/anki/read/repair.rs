//! Post-processing after the rows are read: v1-scheduler upgrade, dangling-reference repair and
//! emptying filtered decks (read.ts:454–577).

use super::schema18::set;
use crate::anki::deck_config::dconf_to_deck_config;
use crate::anki::image::{AnkiDeck, AnkiDeckConfig, AnkiImage, Json};
use crate::error::{LociError, Result};
use crate::types::*;
use chrono::{Local, TimeZone, Timelike};
use std::collections::{HashMap, HashSet};

/// v1-scheduler collections (schedVer missing or 1), converted like Anki's own v1→v2 upgrade
/// (rslib/src/scheduler/upgrade.rs, rslib/src/storage/revlog/v2_upgrade.sql), which Anki runs on an
/// imported package when any card is not new:
///  1. revlog: learning/relearning answers used buttons 1–3 (3 = easy) → `ease + 1` for eases 2 and 3
///     on rows of type 0 (learn) and 2 (relearn).
///  2. cards: v1 relearning cards are `type = 2` in queue 1/3 → `type = 3` (relearning).
///  3. config: schedVer 2; rollover = local hour of `crt` (v1 days are counted from `crt` itself, so this
///     keeps every review `due` on the same calendar day); creationOffset = the current UTC offset.
///
/// Buried cards (v1 queue −2) keep −2, which v2 also reads as buried. Filtered decks are emptied
/// afterwards by `return_filtered_cards` like for any other collection.
pub(super) fn upgrade_v1_scheduler(image: &mut AnkiImage, warnings: &mut Vec<String>) {
    let needed = image.cards.iter().any(|c| c.queue != CardQueue::New);
    if needed {
        let (mut revlog_fixed, mut relearn_fixed) = (0, 0);
        for r in &mut image.revlog {
            if (r.ease == 2 || r.ease == 3) && (r.rtype == RevlogType::Learn || r.rtype == RevlogType::Relearn) {
                r.ease += 1;
                revlog_fixed += 1;
            }
        }
        for c in &mut image.cards {
            if c.ctype == CardType::Review && (c.queue == CardQueue::Learn || c.queue == CardQueue::DayLearn) {
                c.ctype = CardType::Relearn;
                relearn_fixed += 1;
            }
        }
        warnings.push(format!(
            "Converted from Anki's old v1 scheduler: {revlog_fixed} learning answers renumbered, {relearn_fixed} relearning cards updated."
        ));
    }
    let rollover = Local.timestamp_opt(image.crt, 0).single().map(|d| d.hour() as i64).unwrap_or(4);
    let creation_offset = -(Local::now().offset().local_minus_utc() as i64) / 60;
    image.config.sched_ver = 2;
    image.config.rollover = rollover;
    image.config.creation_offset = Some(creation_offset);
    set(&mut image.config.raw, "schedVer", 2);
    set(&mut image.config.raw, "rollover", rollover);
    set(&mut image.config.raw, "creationOffset", creation_offset);
}

/// Fixes dangling references the way Anki's check-database would, warning for each kind.
pub(super) fn repair_references(image: &mut AnkiImage, warnings: &mut Vec<String>) -> Result<()> {
    // deck options: preset 1 must exist
    if !image.deck_configs.iter().any(|c| c.id == 1) {
        let mut ignored = Vec::new();
        image.deck_configs.insert(
            0,
            AnkiDeckConfig {
                id: 1,
                name: "Default".into(),
                config: dconf_to_deck_config(&Json::new(), image.config.fsrs, image.config.est_times, &mut ignored),
                mtime: 0,
                usn: 0,
                original_json: None,
            },
        );
    }
    let config_ids: HashSet<i64> = image.deck_configs.iter().map(|c| c.id).collect();
    // decks: Default must exist; presets must exist
    if !image.decks.iter().any(|d| d.id == 1) {
        let name = unique_deck_name(&image.decks, "Default");
        image.decks.insert(0, AnkiDeck::blank(1, name));
    }
    let mut preset_fixed = 0;
    for d in &mut image.decks {
        if !d.filtered && !config_ids.contains(&d.config_id) {
            d.config_id = 1;
            preset_fixed += 1;
        }
    }
    if preset_fixed > 0 {
        warnings.push(format!("{preset_fixed} deck(s) referred to missing deck options and now use \"Default\"."));
    }

    // notes: note type must exist; field count must match
    let field_counts: HashMap<i64, usize> = image.notetypes.iter().map(|nt| (nt.id, nt.fields.len())).collect();
    let (mut padded, mut merged) = (0, 0);
    for note in &mut image.notes {
        let Some(&want) = field_counts.get(&note.ntid) else {
            return Err(LociError::corrupt(format!(
                "Note {} uses a note type ({}) that is not in the package.",
                note.id, note.ntid
            )));
        };
        if note.fields.len() < want {
            note.fields.resize(want, String::new());
            padded += 1;
        } else if note.fields.len() > want {
            let extra: Vec<String> = note.fields.split_off(want - 1);
            note.fields.push(extra.join("; "));
            merged += 1;
        }
    }
    if padded > 0 {
        warnings.push(format!("{padded} note(s) had fewer fields than their note type; empty fields were added."));
    }
    if merged > 0 {
        warnings.push(format!(
            "{merged} note(s) had more fields than their note type; the extra fields were appended to the last field."
        ));
    }

    // cards: note must exist; deck must exist
    let note_ids: HashSet<i64> = image.notes.iter().map(|n| n.id).collect();
    let deck_ids: HashSet<i64> = image.decks.iter().map(|d| d.id).collect();
    let before = image.cards.len();
    image.cards.retain(|c| note_ids.contains(&c.nid));
    if image.cards.len() < before {
        warnings.push(format!("{} card(s) without a note were skipped.", before - image.cards.len()));
    }
    let mut homeless = 0;
    for c in &mut image.cards {
        // a card in a missing filtered deck is sent home by return_filtered_cards; others go to Default
        if !deck_ids.contains(&c.did) && c.odid == 0 {
            c.did = 1;
            homeless += 1;
        }
    }
    if homeless > 0 {
        warnings.push(format!("{homeless} card(s) were in a deck that is not in the package and were moved to \"Default\"."));
    }
    Ok(())
}

fn unique_deck_name(decks: &[AnkiDeck], base: &str) -> String {
    let taken: HashSet<String> = decks.iter().map(|d| d.name.to_lowercase()).collect();
    let mut name = base.to_string();
    let mut i = 1;
    while taken.contains(&name.to_lowercase()) {
        name = format!("{base} ({i})");
        i += 1;
    }
    name
}

/// Filtered decks are not persistent in Loci: every card in one is returned to its home deck with its
/// original due restored (Anki `remove_from_filtered_deck_restoring_queue`), and the filtered decks are
/// dropped with a warning.
pub(super) fn return_filtered_cards(image: &mut AnkiImage, warnings: &mut Vec<String>) {
    let filtered: Vec<(i64, String)> = image.decks.iter().filter(|d| d.filtered).map(|d| (d.id, d.name.clone())).collect();
    let filtered_ids: HashSet<i64> = filtered.iter().map(|(id, _)| *id).collect();
    let normal_ids: HashSet<i64> = image.decks.iter().filter(|d| !d.filtered).map(|d| d.id).collect();
    let (mut returned, mut orphaned) = (0, 0);
    for c in &mut image.cards {
        if c.odid != 0 {
            c.did = if normal_ids.contains(&c.odid) { c.odid } else { 1 };
            if c.did == 1 && c.odid != 1 {
                orphaned += 1;
            }
            c.odid = 0;
            if c.odue != 0 {
                c.due = c.odue;
            }
            if c.queue >= CardQueue::New {
                c.queue = match c.ctype {
                    CardType::Learn | CardType::Relearn => {
                        if c.due > 1_000_000_000 {
                            CardQueue::Learn
                        } else {
                            CardQueue::DayLearn
                        }
                    }
                    CardType::New => CardQueue::New,
                    CardType::Review => CardQueue::Review,
                };
            }
            c.odue = 0;
            returned += 1;
        } else if filtered_ids.contains(&c.did) {
            c.did = 1;
            orphaned += 1;
        }
    }
    if !filtered.is_empty() {
        image.decks.retain(|d| !d.filtered);
        let names = filtered.iter().map(|(_, n)| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        let plural = filtered.len() > 1;
        warnings.push(format!(
            "Filtered deck{} {names} {} not imported; {returned} card(s) went back to their home decks.",
            if plural { "s" } else { "" },
            if plural { "were" } else { "was" }
        ));
    } else if returned > 0 {
        warnings.push(format!("{returned} card(s) from filtered decks went back to their home decks."));
    }
    if orphaned > 0 {
        warnings.push(format!("{orphaned} card(s) had no home deck and were moved to \"Default\"."));
    }
}
