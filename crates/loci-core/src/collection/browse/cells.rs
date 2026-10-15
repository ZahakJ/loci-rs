//! Browser row cells.

use crate::anki::percent_decode;
use crate::collection::internals::CollectionCore;
use crate::collection::render::{render_stored_card, MEDIA_URL_PREFIX};
use crate::collection::rows::{row_to_card, row_to_note, select_sql, CARD_COLUMNS, NOTE_COLUMNS};
use crate::error::Result;
use crate::i18n::msg;
use crate::scheduler::{format_interval, SchedClock};
use crate::search::{register_sql_functions, retrievability_sql};
use crate::stats::iso_date;
use crate::template::cloze_to_text;
use crate::text::strip_html;
use crate::types::*;
use regex::{Captures, Regex};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

const CELL_MAX: usize = 120;
/// Longest media file name shown in a cell; longer names (and inline images) read '[image]'.
const MEDIA_NAME_MAX: usize = 32;

static IMG_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>"#).expect("valid img regex")
});
static SOUND_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:[^\]]*\]").expect("valid sound regex"));
static DATA_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bdata:[a-z]+/[\w.+-]+(?:;[\w=.+-]+)*,\S*").expect("valid data-url regex"));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));
static ANSWER_HR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)^.*<hr id=["']?answer["']?\s*/?>"#).expect("valid hr regex"));

fn image_label() -> String {
    msg("browse.cellImage", "[image]", &[])
}

/// Collapses whitespace and cuts to 120 characters.
fn clip(s: &str) -> String {
    let t = WHITESPACE.replace_all(s, " ");
    let t = t.trim();
    if t.chars().count() > CELL_MAX {
        let mut out: String = t.chars().take(CELL_MAX - 1).collect();
        out.push('…');
        out
    } else {
        t.to_string()
    }
}

/// Field or rendered HTML as one line of browser text: images become their file name when short
/// (else '[image]'; inline data: images always '[image]'), [sound:] tags and other media vanish,
/// cloze markup reads as its text, HTML is stripped, stray data: URLs dropped, whitespace
/// collapsed, cut to 120 characters.
pub fn cell_text(html: &str) -> String {
    let s = IMG_TAG.replace_all(html, |caps: &Captures| {
        let src = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3)).map(|m| m.as_str().trim()).unwrap_or("");
        if src.is_empty() || src.to_lowercase().starts_with("data:") {
            return format!(" {} ", image_label());
        }
        let name = src.strip_prefix(MEDIA_URL_PREFIX).unwrap_or(src);
        // decodeURIComponent, keeping the text when it is not decodable
        let name = percent_decode(name);
        let name = name.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
        if !name.is_empty() && name.chars().count() <= MEDIA_NAME_MAX {
            format!(" {name} ")
        } else {
            format!(" {} ", image_label())
        }
    });
    let s = SOUND_TAG.replace_all(&s, " ");
    let s = strip_html(&cloze_to_text(&s));
    let s = DATA_URL.replace_all(&s, " ");
    clip(&s)
}

/// Local date (YYYY-MM-DD) of an instant in seconds at an offset (minutes west).
fn local_date(secs: i64, offset_west: i64) -> String {
    iso_date(secs, offset_west)
}

fn local_date_time(secs: i64, offset_west: i64) -> String {
    chrono::DateTime::from_timestamp(secs - offset_west * 60, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

pub struct RowCtx<'a> {
    pub core: &'a CollectionCore,
    pub clock: SchedClock,
    /// UTC offset, minutes west.
    pub offset: i64,
    pub deck_names: HashMap<DeckId, String>,
}

fn due_cell(c: &Card, ctx: &RowCtx<'_>) -> String {
    if c.ctype == CardType::New {
        return msg("browse.duePosition", "New #{pos}", &[("pos", c.due.into())]);
    }
    let text = if c.due > 1_000_000_000 {
        // Intraday learning (due in seconds): the time when it is due before the day ends, else the date.
        let at = local_date_time(c.due, ctx.offset);
        let today = c.due >= ctx.clock.next_day_at - 86_400 && c.due < ctx.clock.next_day_at;
        if today {
            msg("browse.dueTodayAt", "Today {time}", &[("time", at.get(11..).unwrap_or("").into())])
        } else {
            at.get(..10).unwrap_or("").to_string()
        }
    } else {
        local_date(ctx.clock.day_start_secs(c.due), ctx.offset)
    };
    if (c.queue as i64) < 0 {
        format!("({text})")
    } else {
        text
    }
}

fn card_cells(c: &Card, ctx: &RowCtx<'_>) -> Result<BTreeMap<BrowseColumn, String>> {
    let mut cells = BTreeMap::new();
    cells.insert(BrowseColumn::Deck, ctx.deck_names.get(&c.did).cloned().unwrap_or_default());
    cells.insert(BrowseColumn::Due, due_cell(c, ctx));
    cells.insert(
        BrowseColumn::Interval,
        match c.ctype {
            CardType::New => msg("browse.ivlNew", "(new)", &[]),
            CardType::Learn => msg("browse.ivlLearning", "(learning)", &[]),
            _ => format_interval(c.ivl * 86_400, 0),
        },
    );
    cells.insert(
        BrowseColumn::Ease,
        if c.factor > 0 && c.ctype != CardType::New {
            format!("{}%", (c.factor as f64 / 10.0).round() as i64)
        } else {
            String::new()
        },
    );
    cells.insert(
        BrowseColumn::Difficulty,
        c.data.d.map(|d| format!("{}%", (((d - 1.0) / 9.0) * 100.0).round() as i64)).unwrap_or_default(),
    );
    cells.insert(BrowseColumn::Stability, c.data.s.map(|s| format_interval((s * 86_400.0) as i64, 0)).unwrap_or_default());
    // Same expression as `prop:r` and the column sort, so the three always agree (any card with a
    // memory state, whatever its queue).
    register_sql_functions(ctx.core.db().conn())?;
    let r: Option<f64> = ctx
        .core
        .db()
        .get(
            &format!("SELECT {} AS r FROM cards c WHERE c.id = ?", retrievability_sql(ctx.clock.today, ctx.clock.now_ms)),
            [c.id],
            |row| row.get::<_, Option<f64>>(0),
        )?
        .flatten();
    cells.insert(
        BrowseColumn::Retrievability,
        r.filter(|v| v.is_finite()).map(|v| format!("{}%", (v * 100.0).round() as i64)).unwrap_or_default(),
    );
    cells.insert(BrowseColumn::Reps, c.reps.to_string());
    cells.insert(BrowseColumn::Lapses, c.lapses.to_string());
    cells.insert(BrowseColumn::Created, local_date(c.id.div_euclid(1000), ctx.offset));
    cells.insert(BrowseColumn::Modified, local_date(c.mtime, ctx.offset));
    Ok(cells)
}

pub fn build_row(ctx: &RowCtx<'_>, mode: BrowseMode, id: i64) -> Result<Option<BrowseRow>> {
    let core = ctx.core;
    let cards: Vec<Card> = if mode == BrowseMode::Cards {
        match core.db().get(&format!("{} WHERE id = ?", select_sql("cards", &CARD_COLUMNS)), [id], row_to_card)? {
            Some(c) => vec![c],
            None => return Ok(None),
        }
    } else {
        let cards =
            core.db().all(&format!("{} WHERE nid = ? ORDER BY ord", select_sql("cards", &CARD_COLUMNS)), [id], row_to_card)?;
        if cards.is_empty() {
            return Ok(None);
        }
        cards
    };
    let card = &cards[0];
    let Some(note) = core.db().get(&format!("{} WHERE id = ?", select_sql("notes", &NOTE_COLUMNS)), [card.nid], row_to_note)?
    else {
        return Ok(None);
    };
    let nt = core.notetype(note.ntid)?;
    let mut cells = card_cells(card, ctx)?;
    // A sort field with no text (an image-occlusion note's shapes) falls back to the first field that has some.
    let sort_idx = usize::try_from(nt.sort_field).unwrap_or(0);
    let mut sort_field = cell_text(note.fields.get(sort_idx).or(note.fields.first()).map(String::as_str).unwrap_or(""));
    if sort_field.is_empty() {
        sort_field = note.fields.iter().map(|f| cell_text(f)).find(|s| !s.is_empty()).unwrap_or_default();
    }
    cells.insert(BrowseColumn::SortField, sort_field);
    cells.insert(BrowseColumn::Notetype, nt.name.clone());
    cells.insert(
        BrowseColumn::Template,
        if nt.kind == NotetypeKind::Cloze {
            format!("Cloze {}", card.ord + 1)
        } else {
            nt.templates.iter().find(|t| t.ord == card.ord).map(|t| t.name.clone()).unwrap_or_default()
        },
    );
    cells.insert(BrowseColumn::Tags, note.tags.join(" "));
    if mode == BrowseMode::Notes {
        cells.insert(BrowseColumn::CardCount, cards.len().to_string());
        cells.insert(BrowseColumn::Reps, cards.iter().map(|c| c.reps).sum::<i64>().to_string());
        cells.insert(BrowseColumn::Lapses, cards.iter().map(|c| c.lapses).sum::<i64>().to_string());
        cells.insert(BrowseColumn::Created, local_date(note.id.div_euclid(1000), ctx.offset));
        cells.insert(BrowseColumn::Modified, local_date(note.mtime, ctx.offset));
        let mut decks: Vec<String> = Vec::new();
        for c in &cards {
            let name = ctx.deck_names.get(&c.did).cloned().unwrap_or_default();
            if !decks.contains(&name) {
                decks.push(name);
            }
        }
        cells.insert(BrowseColumn::Deck, if decks.len() == 1 { decks.remove(0) } else { format!("({})", decks.len()) });
    }
    match render_stored_card(core, card, Some(&note)) {
        Ok(rendered) => {
            cells.insert(BrowseColumn::Question, cell_text(&rendered.front.html));
            cells.insert(BrowseColumn::Answer, cell_text(&ANSWER_HR.replace(&rendered.back.html, "")));
        }
        Err(_) => {
            cells.insert(BrowseColumn::Question, String::new());
            cells.insert(BrowseColumn::Answer, String::new());
        }
    }
    Ok(Some(BrowseRow {
        id,
        card_id: card.id,
        note_id: note.id,
        cells,
        flag: (card.flags & 7) as Flag,
        suspended: cards.iter().all(|c| c.queue == CardQueue::Suspended),
        buried: cards.iter().all(|c| matches!(c.queue, CardQueue::SchedBuried | CardQueue::UserBuried)),
        is_leech: note.tags.iter().any(|t| t.to_lowercase() == "leech"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // browse cells read as text
    #[test]
    fn cell_text_reads_as_plain_text() {
        assert_eq!(cell_text("{{c1::Paris::city}} is in <b>{{c2::France}}</b>"), "Paris is in France");
        assert_eq!(cell_text("<img src=\"data:image/png;base64,AAAA\"> caption"), "[image] caption");
        assert_eq!(cell_text("<img src=\"cat.jpg\"> a cat [sound:meow.mp3]"), "cat.jpg a cat");
        assert_eq!(cell_text(&format!("<img src=\"{}.png\">", "x".repeat(40))), "[image]");
        assert_eq!(cell_text("<img src=\"loci-media:caf%C3%A9.png\">"), "café.png");
        assert_eq!(cell_text("see data:image/svg+xml;charset=utf-8,%3Csvg%3E here"), "see here");
        assert_eq!(cell_text("a\n\n   b"), "a b");
        let long = cell_text(&"word ".repeat(60));
        assert_eq!(long.chars().count(), 120);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn local_date_time_formats_at_offset() {
        assert_eq!(local_date_time(3600 * 13 + 60 * 5, 0), "1970-01-01 13:05");
        assert_eq!(local_date_time(3600 * 13, 60), "1970-01-01 12:00");
    }
}
