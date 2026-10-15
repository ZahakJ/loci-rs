//! `describe_query`: a short human sentence for a parsed search,
//! e.g. `deck:French tag:verbs is:due` → "Cards in deck French, tagged verbs, and due".
//! All wording comes from i18n keys under `browse.describe.*`; lists follow English
//! `Intl.ListFormat` (long style): "a and b", "a, b, and c".

use crate::i18n::{t, Var};

use super::parse::{unescape_text, PropValue, SearchNode};

const GRADES: [&str; 5] = ["", "study.again", "study.hard", "study.good", "study.easy"];

fn list(parts: &[String], conjunction: bool) -> String {
    let word = if conjunction { "and" } else { "or" };
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} {word} {}", parts[0], parts[1]),
        n => format!("{}, {word} {}", parts[..n - 1].join(", "), parts[n - 1]),
    }
}

fn d(key: &str, vars: &[(&str, Var)]) -> String {
    t(&format!("browse.describe.{key}"), vars)
}

/// `String(n)` for the numbers a query carries.
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn phrase(n: &SearchNode) -> String {
    match n {
        SearchNode::All => String::new(),
        SearchNode::And { children } => list(&children.iter().map(phrase).filter(|s| !s.is_empty()).collect::<Vec<_>>(), true),
        SearchNode::Or { children } => list(&children.iter().map(phrase).filter(|s| !s.is_empty()).collect::<Vec<_>>(), false),
        SearchNode::Not { child } => d("not", &[("inner", phrase(child).into())]),
        SearchNode::Text { text } => d("text", &[("text", unescape_text(text).into())]),
        SearchNode::Field { field, value, mode } => {
            let field = unescape_text(field);
            match mode {
                super::parse::FieldMode::Regex => d("field", &[("field", field.into()), ("text", value.as_str().into())]),
                super::parse::FieldMode::Glob if value.is_empty() => d("fieldEmpty", &[("field", field.into())]),
                super::parse::FieldMode::Glob if value == "_*" => d("fieldNonEmpty", &[("field", field.into())]),
                _ => d("field", &[("field", field.into()), ("text", unescape_text(value).into())]),
            }
        }
        SearchNode::Deck { name } => {
            let name = unescape_text(name);
            if name == "*" {
                d("allDecks", &[])
            } else if name.to_lowercase() == "filtered" {
                d("deckFiltered", &[])
            } else {
                d("deck", &[("name", name.into())])
            }
        }
        SearchNode::DeckCurrent => d("deckCurrent", &[]),
        SearchNode::Notetype { name } => d("notetype", &[("name", unescape_text(name).into())]),
        SearchNode::CardOrd { ord } => d("card", &[("name", (ord + 1).to_string().into())]),
        SearchNode::CardName { name } => d("card", &[("name", unescape_text(name).into())]),
        SearchNode::Tag { tag } => d("tag", &[("tag", unescape_text(tag).into())]),
        SearchNode::TagNone => d("tagNone", &[]),
        SearchNode::TagRegex { pattern } => d("tag", &[("tag", format!("/{pattern}/").into())]),
        SearchNode::State { state } => d(&format!("is.{}", state.as_str()), &[]),
        SearchNode::Flag { flag } => {
            if *flag == 0 {
                d("flagNone", &[])
            } else {
                d("flag", &[("colour", t(&format!("study.flags.{flag}"), &[]).to_lowercase().into())])
            }
        }
        SearchNode::Prop { prop, op, value, key } => {
            let prop = d(&format!("props.{}", prop.as_str()), &[("key", key.clone().unwrap_or_default().into())]);
            let value = match value {
                PropValue::Num(n) => js_num(*n),
                PropValue::Text(s) => s.clone(),
            };
            d("prop", &[("prop", prop.into()), ("op", d(&format!("ops.{}", op.as_str()), &[]).into()), ("value", value.into())])
        }
        SearchNode::Rated { days, ease } => match ease {
            Some(e) => d(
                "ratedEase",
                &[("days", (*days).into()), ("grade", t(GRADES.get(*e as usize).copied().unwrap_or(""), &[]).into())],
            ),
            None => d("rated", &[("days", (*days).into())]),
        },
        SearchNode::Resched { days } => d("resched", &[("days", (*days).into())]),
        SearchNode::Introduced { days } => d("introduced", &[("days", (*days).into())]),
        SearchNode::Added { days } => d("added", &[("days", (*days).into())]),
        SearchNode::Edited { days } => d("edited", &[("days", (*days).into())]),
        SearchNode::Ids { ids, .. } => {
            d("ids", &[("ids", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ").into())])
        }
        SearchNode::Dupe { text, .. } => d("dupe", &[("text", text.as_str().into())]),
        SearchNode::HasCd { key } => d("hasCd", &[("key", key.as_str().into())]),
        SearchNode::Preset { name } => d("preset", &[("name", unescape_text(name).into())]),
        SearchNode::Regex { pattern } => d("regex", &[("pattern", pattern.as_str().into())]),
        SearchNode::Nc { text } => d("noCombining", &[("text", unescape_text(text).into())]),
        SearchNode::Word { text } => d("word", &[("text", unescape_text(text).into())]),
        SearchNode::Sc { text } => d("stripClozes", &[("text", unescape_text(text).into())]),
    }
}

/// One short sentence describing what a query matches ("All cards" for an empty query).
pub fn describe_query(ast: &SearchNode) -> String {
    let p = phrase(ast);
    if p.is_empty() {
        t("browse.describe.all", &[])
    } else {
        t("browse.describe.cardsThat", &[("conditions", p.into())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::parse::parse_search;

    fn describe(q: &str) -> String {
        describe_query(&parse_search(q).unwrap())
    }

    #[test]
    fn produces_a_short_sentence() {
        assert_eq!(describe(""), "All cards");
        assert_eq!(describe("deck:French tag:verbs is:due"), "Cards in deck French, tagged verbs, and due");
        assert_eq!(describe("dog or cat"), "Cards containing “dog” or containing “cat”");
        assert_eq!(describe("-is:suspended flag:1"), "Cards not suspended and flagged red");
        assert_eq!(describe("rated:7:1"), "Cards answered Again in the last 7 days");
        assert_eq!(describe("added:1"), "Cards added today");
        assert_eq!(describe("prop:ivl>=21"), "Cards with interval (days) at least 21");
        assert_eq!(describe("front:"), "Cards with front empty");
    }

    #[test]
    fn covers_the_remaining_term_kinds() {
        assert_eq!(describe("back:_*"), "Cards with back filled in");
        assert_eq!(describe("front:re:^c"), "Cards with front matching “^c”");
        assert_eq!(
            describe("deck:* deck:filtered deck:current"),
            "Cards in any deck, in a filtered deck, and in the current deck"
        );
        assert_eq!(
            describe("card:2 card:Reverse note:Basic"),
            "Cards of card type 2, of card type Reverse, and of note type Basic"
        );
        assert_eq!(describe("tag:none tag:re:^a flag:0"), "Cards without tags, tagged /^a/, and without a flag");
        assert_eq!(describe("prop:cdn:n>5"), "Cards with custom value n above 5");
        assert_eq!(describe("prop:cds:v=x"), "Cards with custom text v equal to x");
        assert_eq!(describe("prop:r<0.9"), "Cards with retrievability below 0.9");
        assert_eq!(
            describe("rated:1 resched:7 introduced:1 edited:3"),
            "Cards answered today, rescheduled in the last 7 days, first studied today, and edited in the last 3 days"
        );
        assert_eq!(
            describe("nid:1,2 dupe:5,dog has-cd:v preset:P"),
            "Cards with ids 1, 2, duplicates of “dog”, with custom data v, and using preset P"
        );
        assert_eq!(describe("re:x nc:uber w:dog sc:cat"), "Cards matching the pattern x, containing “uber” (ignoring accents), containing the word “dog”, and containing “cat” (ignoring clozes)");
        assert_eq!(describe("is:buried-manually"), "Cards buried by hand");
    }
}
