//! Rendering and card explanation for the API:
//! renderCard, previewNote, checkTemplate and explainCard ("why this interval?", internals §6).
//!
//! Media in rendered HTML are referenced as `loci-media:<encoded name>`; the UI resolves them
//! through its media URL cache.

use crate::collection::internals::CollectionCore;
use crate::collection::notes::{get_card, get_note};
use crate::collection::sched_context::sched_clock;
use crate::collection::service::Collection;
use crate::error::{LociError, Result};
use crate::i18n::{msg, pct};
use crate::scheduler::{
    card_revlog, decay_from_params, forgetting_curve, memory_state_from_revlog, normalize_parameters, CardSched, MemoryState,
    RevlogConversionInput,
};
use crate::template::{
    check_template as check_template_issues, render_card as render_template, template_issue_messages, RenderCardInput,
};
use crate::types::*;

/// URL scheme for media inside rendered cards; the UI maps it to an object URL.
pub const MEDIA_URL_PREFIX: &str = "loci-media:";

/// JavaScript `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` is
/// percent-encoded as UTF-8 bytes.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Media URL for a file name.
pub fn media_url(name: &str) -> String {
    format!("{MEDIA_URL_PREFIX}{}", encode_uri_component(name))
}

fn padded(nt: &Notetype, fields: &[String]) -> Vec<String> {
    let mut out = fields.to_vec();
    while out.len() < nt.fields.len() {
        out.push(String::new());
    }
    out
}

/// Renders a stored card (note, note type and deck name loaded from the collection).
pub fn render_stored_card(core: &CollectionCore, card: &Card, note: Option<&Note>) -> Result<RenderedCard> {
    let loaded;
    let n = match note {
        Some(n) => n,
        None => {
            loaded = get_note(core, card.nid)?;
            &loaded
        }
    };
    let nt = core.notetype(n.ntid)?;
    // deck vanished; render anyway
    let deck_name = core.deck(card.did).map(|d| d.name).unwrap_or_default();
    let fields = padded(&nt, &n.fields);
    Ok(render_template(&RenderCardInput {
        notetype: &nt,
        note_id: Some(n.id),
        fields: &fields,
        tags: &n.tags,
        card_id: Some(card.id),
        card_ord: card.ord,
        card_flags: card.flags,
        deck_name: &deck_name,
        media_url: &media_url,
        one_by_one: None,
    }))
}

pub fn render_card_by_id(core: &CollectionCore, id: CardId) -> Result<RenderedCard> {
    let card = get_card(core, id)?;
    render_stored_card(core, &card, None)
}

/// Renders unsaved fields against a note type (editor live preview). `ord`: template ord or cloze number − 1.
pub fn preview_note(core: &CollectionCore, ntid: NotetypeId, fields: &[String], ord: i64) -> Result<RenderedCard> {
    let nt = core.notetype(ntid)?;
    if ord < 0 {
        return Err(LociError::invalid(format!("Invalid card ordinal {ord}")));
    }
    let fields = padded(&nt, fields);
    Ok(render_template(&RenderCardInput {
        notetype: &nt,
        note_id: None,
        fields: &fields,
        tags: &[],
        card_id: None,
        card_ord: ord,
        card_flags: 0,
        deck_name: "",
        media_url: &media_url,
        one_by_one: None,
    }))
}

/// Template validation messages ("Front template, line 2, column 5: …"); [] when fine.
pub fn check_template(core: &CollectionCore, ntid: NotetypeId, qfmt: &str, afmt: &str) -> Result<Vec<String>> {
    let nt = core.notetype(ntid)?;
    let names: Vec<String> = nt.fields.iter().map(|f| f.name.clone()).collect();
    Ok(template_issue_messages(&check_template_issues(&names, nt.kind, qfmt, afmt)))
}

/// Explains a card's schedule. Retrievability uses the card's FSRS memory state (or one replayed
/// from its revlog with the preset's parameters, also under SM-2) and the time since the last
/// review in fractional days; stability in days, difficulty on Anki's 1–10 scale.
pub fn explain_card(core: &CollectionCore, id: CardId) -> Result<CardExplanation> {
    let card = get_card(core, id)?;
    let config = core.deck_config(card.did)?;
    let clock = sched_clock(core)?;
    let history = card_revlog(core.db(), id)?;
    let graded: Vec<&RevlogEntry> = history.iter().filter(|r| (1..=4).contains(&r.ease)).collect();
    let timed: Vec<&RevlogEntry> = graded.iter().copied().filter(|r| r.time > 0).collect();
    let average_seconds =
        if timed.is_empty() { 0.0 } else { timed.iter().map(|r| r.time as f64).sum::<f64>() / timed.len() as f64 / 1000.0 };

    let mut stability = None;
    let mut difficulty = None;
    let mut retrievability = None;
    let last_ms: Option<i64> = match card.data.lrt {
        Some(lrt) if lrt > 0 => Some(lrt * 1000),
        _ => graded.last().map(|r| r.id),
    };
    if card.ctype != CardType::New {
        let mut memory = match (card.data.s, card.data.d) {
            (Some(s), Some(d)) => Some(MemoryState { stability: s, difficulty: d }),
            _ => None,
        };
        if memory.is_none() {
            memory = memory_state_from_revlog(&RevlogConversionInput {
                card: CardSched { ctype: card.ctype, ivl: card.ivl, factor: card.factor },
                revlog: &history,
                params: &config.fsrs_params,
                historical_retention: config.historical_retention,
                next_day_at_ms: clock.next_day_at * 1000,
                ignore_before_ms: 0,
            })?;
        }
        if let Some(m) = memory {
            stability = Some(m.stability);
            difficulty = Some(m.difficulty);
            let decay = match card.data.decay {
                Some(d) => d,
                None => decay_from_params(&normalize_parameters(&config.fsrs_params)?),
            };
            let elapsed = match last_ms {
                Some(l) => ((clock.now_ms - l) as f64 / 86_400_000.0).max(0.0),
                None => (card.ivl - (card.due - clock.today)).max(0) as f64,
            };
            retrievability = Some(forgetting_curve(elapsed, m.stability, decay));
        }
    }

    let due_date = if matches!(card.queue, CardQueue::Learn | CardQueue::Preview) {
        Some(card.due)
    } else if matches!(card.ctype, CardType::Review | CardType::Relearn) || card.queue == CardQueue::DayLearn {
        Some(if card.due > 1_000_000_000 { card.due } else { clock.day_start_secs(card.due) })
    } else {
        None
    };

    let sentence = match last_ms {
        None => msg("explain.newCard", "This card is new and has not been studied yet.", &[]),
        Some(_) if card.ctype == CardType::New => msg("explain.newCard", "This card is new and has not been studied yet.", &[]),
        Some(last) => {
            let days = clock.today - clock.day_of_ms(last);
            let when = if days <= 0 {
                msg("explain.today", "today", &[])
            } else if days == 1 {
                msg("explain.yesterday", "yesterday", &[])
            } else {
                msg("explain.daysAgo", "{days} days ago", &[("days", days.into())])
            };
            match retrievability {
                Some(r) => msg(
                    "explain.lastSeen",
                    "You last saw this {when}; about {percent} chance you still know it.",
                    &[("when", when.into()), ("percent", pct(r).into())],
                ),
                None => msg("explain.lastSeenNoMemory", "You last saw this {when}.", &[("when", when.into())]),
            }
        }
    };
    Ok(CardExplanation {
        retrievability,
        stability,
        difficulty,
        due_date,
        interval: if matches!(card.ctype, CardType::Review | CardType::Relearn) { card.ivl } else { 0 },
        reps: card.reps,
        lapses: card.lapses,
        average_seconds,
        history,
        sentence,
    })
}

impl Collection {
    pub fn render_card(&self, id: CardId) -> Result<RenderedCard> {
        render_card_by_id(&self.core, id)
    }
    pub fn explain_card(&self, id: CardId) -> Result<CardExplanation> {
        explain_card(&self.core, id)
    }
    pub fn preview_note(&self, ntid: NotetypeId, fields: &[String], ord: i64) -> Result<RenderedCard> {
        preview_note(&self.core, ntid, fields, ord)
    }
    pub fn check_template(&self, ntid: NotetypeId, qfmt: &str, afmt: &str) -> Result<Vec<String>> {
        check_template(&self.core, ntid, qfmt, afmt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_urls_are_percent_encoded_like_encode_uri_component() {
        assert_eq!(media_url("café.png"), "loci-media:caf%C3%A9.png");
        assert_eq!(media_url("a b(1)!.jpg"), "loci-media:a%20b(1)!.jpg");
        assert_eq!(encode_uri_component("~*'-_."), "~*'-_.");
    }
}
