//! The bundled sample deck "Loci: the art of memory": 30 general-interest cards about memory and
//! learning (the method of loci, Ebbinghaus, spacing, testing, interleaving, sleep, chunking) plus a
//! few maths, chemistry and Arabic showcase cards; no scheduler jargon. Plain data; the collection's
//! `install_sample_deck` creates the notes with Loci's built-in note types.
//!
//! Field order per kind: basic / basic-reversed / basic-typed = [Front, Back]; cloze = [Text, Back Extra].

use crate::text::cloze_numbers;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleNoteKind {
    Basic,
    BasicReversed,
    BasicTyped,
    Cloze,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleNote {
    pub kind: SampleNoteKind,
    pub fields: [String; 2],
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleDeck {
    pub name: String,
    pub description: String,
    pub notes: Vec<SampleNote>,
}

const PALACE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 200" width="320" height="200">
<rect width="320" height="200" fill="#f6f1e7"/>
<g fill="none" stroke="#5b5346" stroke-width="3">
<rect x="20" y="20" width="280" height="160"/><path d="M120 20v70M200 20v70M20 110h280M160 110v70"/>
<path d="M150 180h20" stroke="#f6f1e7" stroke-width="5"/></g>
<g fill="#b08a3e"><circle cx="160" cy="165" r="7"/><circle cx="90" cy="145" r="7"/><circle cx="70" cy="60" r="7"/><circle cx="160" cy="55" r="7"/><circle cx="250" cy="60" r="7"/><circle cx="235" cy="145" r="7"/></g>
<path d="M160 165L90 145L70 60L160 55L250 60L235 145" fill="none" stroke="#b08a3e" stroke-width="2" stroke-dasharray="5 4"/>
<text x="160" y="196" font-family="sans-serif" font-size="11" text-anchor="middle" fill="#5b5346">door</text>
</svg>"##;

/// JavaScript `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` is percent-encoded.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

static PALACE_IMAGE: LazyLock<String> =
    LazyLock::new(|| format!("data:image/svg+xml;charset=utf-8,{}", encode_uri_component(PALACE_SVG)));

/// Floor plan of a six-station memory palace route, as an inline SVG data URL.
pub fn sample_palace_image() -> &'static str {
    &PALACE_IMAGE
}

fn note(kind: SampleNoteKind, tags: &[&str], front: &str, back: &str) -> SampleNote {
    SampleNote { kind, fields: [front.to_string(), back.to_string()], tags: tags.iter().map(|t| t.to_string()).collect() }
}

static SAMPLE_DECK: LazyLock<SampleDeck> = LazyLock::new(build_sample_deck);

/// The sample deck (built once).
pub fn sample_deck() -> &'static SampleDeck {
    &SAMPLE_DECK
}

fn build_sample_deck() -> SampleDeck {
    use SampleNoteKind::*;
    let img = sample_palace_image();
    SampleDeck {
        name: "Loci: the art of memory".into(),
        description: "A short tour of how memory works and how to train it, from the memory palace to sleep. Thirty cards; about ten minutes a day for a week.".into(),
        notes: vec![
            // ── the method of loci ──
            note(Basic, &["loci::method"], "What is the <b>method of loci</b>?", "Placing vivid images of what you want to remember at points along a route through a place you know well, then walking the route in your mind to recall them in order."),
            note(Basic, &["loci::history"], "How, according to the story, did Simonides of Ceos discover the method of loci?", "After a banquet hall collapsed, he identified the crushed guests by remembering <i>where each one had been sitting</i>."),
            note(Basic, &["loci::method"], "Why does a memory palace work so well?", "It recruits spatial memory and vivid imagery, which we retain far better than abstract lists."),
            note(Basic, &["loci::method"], &format!("<img src=\"{img}\" alt=\"Floor plan of a palace with six stations on a dotted route\">\n<p>On a memory-palace route, where should every walk begin?</p>"), "At the same fixed starting point, usually the front door, so the order of the stations never changes."),
            note(Cloze, &["loci::history"], "{{c1::Cicero}} and {{c2::Quintilian}} described the memory palace as a tool for orators.", "Cicero, <i>De Oratore</i> II; Quintilian, <i>Institutio Oratoria</i> XI."),
            note(BasicTyped, &["loci::history"], "The Latin word for “places” that gives the method its name", "loci"),
            note(Basic, &["loci::method"], "How does the <b>pegword method</b> work?", "You learn a rhyming “peg” for each number (one–bun, two–shoe, three–tree…) and picture each item to remember hooked onto its peg."),
            note(Basic, &["loci::languages"], "<span lang=\"ar\" dir=\"rtl\">ذَاكِرَة</span>", "memory <i>(dhākira)</i>, from the root <span lang=\"ar\" dir=\"rtl\" style=\"white-space:nowrap\">ذ ك ر</span>, “to remember, to mention”."),
            // ── spacing and forgetting ──
            note(Basic, &["memory::spacing"], "What is the <b>spacing effect</b>?", "Reviews spread out over time build more durable memories than the same number of reviews crammed together."),
            note(Basic, &["memory::forgetting"], "Who first measured the forgetting curve, and how?", "Hermann Ebbinghaus, by learning lists of nonsense syllables and timing how fast he forgot them."),
            note(BasicTyped, &["memory::forgetting"], "Year Ebbinghaus published <i>Über das Gedächtnis</i>", "1885"),
            note(Cloze, &["memory::spacing"], "Reviewing something just as you are about to forget it strengthens the memory {{c1::the most}}.", "Reviewing what you know cold adds little; that is why the gaps between reviews grow."),
            note(BasicReversed, &["memory::techniques"], "Interleaving", "Mixing different topics or problem types within one study session, instead of practising one kind at a time."),
            note(Basic, &["memory::sleep"], "What does a night’s sleep do for what you learned during the day?", "It consolidates it: the sleeping brain replays and stabilises new memories, so studying before sleep (and sleeping well) pays off."),
            note(Basic, &["memory::forgetting"], "What is the <b>serial position effect</b>?", "The first and last items of a list are remembered best (primacy and recency); the middle is forgotten most often."),
            // ── retrieval practice ──
            note(Basic, &["memory::retrieval"], "What is <b>retrieval practice</b> (the testing effect)?", "Recalling something from memory strengthens it more than re-reading or re-watching it."),
            note(Cloze, &["memory::retrieval"], "Testing yourself is {{c1::more effective}} than re-reading, even though {{c2::it feels harder}}.", "The effort is the point: a “desirable difficulty”."),
            note(Cloze, &["memory::retrieval"], "Retrieval practice works best with {{c1::feedback::what should follow each attempt?}} after each attempt.", "Seeing the answer corrects errors before they set."),
            note(Basic, &["memory::cards"], "What makes a good flashcard?", "One idea, a precise question, and an answer you can check: the <i>minimum information principle</i>."),
            note(BasicReversed, &["memory::techniques"], "Mnemonic", "A device (image, rhyme, acronym, place) that makes information easier to remember."),
            // ── working memory ──
            note(Basic, &["memory::working"], "What is <b>chunking</b>?", "Grouping items into meaningful units, like reading a phone number as three groups instead of ten digits, so working memory can hold more."),
            note(Cloze, &["memory::working"], "George Miller’s 1956 paper suggested we can hold about {{c1::seven, plus or minus two}} items in mind at once.", "Later research puts the limit nearer four chunks."),
            // ── a little maths and chemistry (formulas render as maths) ──
            note(Basic, &["showcase", "maths"], "What is the sum of the first \\(n\\) whole numbers?", "\\[1 + 2 + \\dots + n = \\frac{n(n+1)}{2}\\]\nPair the first and last terms: each pair adds up to \\(n+1\\)."),
            note(Basic, &["showcase", "chemistry"], "Methane burns in oxygen. What is the balanced equation?", "\\(\\ce{CH4 + 2 O2 -> CO2 + 2 H2O}\\)"),
            note(BasicTyped, &["showcase", "chemistry"], "The chemical symbol for gold", "Au"),
            note(Basic, &["habits"], "You missed a few days of reviews. Should you worry?", "No. Loci schedules from what you actually remember: well-known cards barely notice the gap, and catch-up mode spreads the rest over a few days."),
        ],
    }
}

/// Cards a sample note produces: basic/typed 1, reversed 2, cloze = distinct cloze numbers.
pub fn sample_card_count(note: &SampleNote) -> usize {
    match note.kind {
        SampleNoteKind::BasicReversed => 2,
        SampleNoteKind::Cloze => cloze_numbers(&note.fields[0]).len(),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_text() -> String {
        sample_deck().notes.iter().flat_map(|n| n.fields.iter().cloned()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn has_exactly_30_cards() {
        assert_eq!(sample_deck().notes.iter().map(sample_card_count).sum::<usize>(), 30);
    }

    #[test]
    fn covers_cloze_basic_reversed_typed_maths_arabic_and_an_image() {
        let kinds: std::collections::HashSet<SampleNoteKind> = sample_deck().notes.iter().map(|n| n.kind).collect();
        assert_eq!(kinds.len(), 4);
        let all = all_text();
        assert!(all.contains("\\[") || all.contains("\\("));
        assert!(all.chars().any(|c| ('\u{600}'..='\u{6ff}').contains(&c)));
        assert!(all.contains(sample_palace_image()));
        assert!(sample_palace_image().starts_with("data:image/svg+xml"));
        assert!(all.contains("\\ce{")); // chemistry through mhchem
        let root = regex::Regex::new(r#"<span lang="ar" dir="rtl" style="white-space:nowrap">[؀-ۿ ]+</span>"#).unwrap();
        assert!(root.is_match(&all)); // an Arabic root kept on one line
                                      // encodeURIComponent parity on a known prefix
        assert!(sample_palace_image().contains("%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22"));
    }

    #[test]
    fn speaks_to_a_newcomer_no_scheduler_jargon() {
        let all = format!("{}{}", all_text(), sample_deck().description).to_lowercase();
        for word in ["fsrs", "stability", "retrievability", "desired retention", "w_20", "w_{20", "leech"] {
            assert!(!all.contains(word), "{word}");
        }
        assert!(!sample_deck().notes.iter().any(|n| n.tags.iter().any(|t| t == "fsrs")));
    }

    #[test]
    fn cloze_notes_use_markers_the_shared_helper_counts_the_same_way() {
        for n in sample_deck().notes.iter().filter(|n| n.kind == SampleNoteKind::Cloze) {
            assert_eq!(cloze_numbers(&n.fields[0]).len(), sample_card_count(n));
        }
    }

    #[test]
    fn every_note_has_two_fields_a_non_empty_front_and_tags() {
        for n in &sample_deck().notes {
            assert!(!n.fields[0].trim().is_empty());
            assert!(!n.tags.is_empty());
        }
        // Arabic strings use Western digits only
        let all = all_text();
        assert!(!all.chars().any(|c| ('\u{660}'..='\u{669}').contains(&c)));
    }
}
