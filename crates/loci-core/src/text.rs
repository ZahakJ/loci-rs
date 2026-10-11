//! Text helpers shared by every module (recall `src/core/text.ts`; Anki-compatible semantics).

use crate::types::FIELD_SEPARATOR;
use regex::Regex;
use sha1::{Digest, Sha1};
use std::sync::LazyLock;

const B91: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~";

pub fn base91(mut n: u64) -> String {
    let mut out = Vec::new();
    while n > 0 {
        out.push(B91[(n % 91) as usize]);
        n /= 91;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Anki-style note GUID: base91 of a random u64.
pub fn new_guid() -> String {
    base91(rand::random::<u64>())
}

pub fn sha1(bytes: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(bytes);
    h.finalize().into()
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1(bytes))
}

/// Anki `field_checksum`: first 4 bytes of sha1(stripped text) as u32.
/// ("test" → 2840236005, "今日" → 1464653051)
pub fn field_checksum(stripped: &str) -> u32 {
    let d = sha1(stripped.as_bytes());
    u32::from_be_bytes([d[0], d[1], d[2], d[3]])
}

static MEDIA_TAGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<\b(?:img|audio|video|object|source)\b(?:[^>]|"[^"]+?"|'[^']+?')+?\b(?:src|data)\b=(?:"([^"]+?)"[^>]*>|'([^']+?)'[^>]*>|([^ >]+?)(?: [^>]*>|>))"#).unwrap()
});
static HTML_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)(<!--.*?-->)|(<style.*?>.*?</style>)|(<script.*?>.*?</script>)|(<.*?>)").unwrap());
static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)&(#x[0-9a-f]+|#\d+|[a-z][a-z0-9]*);").unwrap());
static SOUND_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:[^\]]*\]").unwrap());
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static EMPTY_FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:[ \t\n\x0b\x0c\r]|</?(?:br|div) ?/?>)*$").unwrap());
static CLOZE_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\{\{c(\d+)::").unwrap());

/// Decodes `&name;`, `&#123;` and `&#x1F;` entities; unknown names are left as written.
pub fn decode_entities(s: &str) -> String {
    ENTITY
        .replace_all(s, |caps: &regex::Captures| {
            let whole = caps.get(0).unwrap().as_str();
            let body = caps.get(1).unwrap().as_str();
            if let Some(rest) = body.strip_prefix('#') {
                let cp = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    rest.parse::<u32>().ok()
                };
                return match cp.and_then(char::from_u32) {
                    Some(c) => c.to_string(),
                    None => whole.to_string(),
                };
            }
            let with_semicolon = format!("&{body};");
            let decoded = html_escape::decode_html_entities(&with_semicolon);
            if decoded != with_semicolon {
                return decoded.into_owned();
            }
            let lower = format!("&{};", body.to_lowercase());
            let decoded = html_escape::decode_html_entities(&lower);
            if decoded != lower {
                return decoded.into_owned();
            }
            whole.to_string()
        })
        .into_owned()
}

/// Anki `strip_html`: tags, comments, style and script removed; entities decoded; NBSP → space.
pub fn strip_html(html: &str) -> String {
    let s = HTML_TAGS.replace_all(html, "");
    if s.contains('&') {
        decode_entities(&s).replace('\u{a0}', " ")
    } else {
        s.into_owned()
    }
}

/// Anki `strip_html_preserving_media_filenames`: media filenames are kept as text
/// (used for sfld, csum, duplicates).
pub fn strip_html_preserving_media(html: &str) -> String {
    let s = MEDIA_TAGS.replace_all(html, |caps: &regex::Captures| {
        let a = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let b = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let c = caps.get(3).map(|m| m.as_str()).unwrap_or("");
        format!(" {a}{b}{c} ")
    });
    strip_html(&s)
}

/// Anki `field_is_empty`: only ASCII whitespace and br/div tags.
pub fn field_is_empty(s: &str) -> bool {
    EMPTY_FIELD.is_match(s)
}

/// Text used for search/FTS: cloze markup rendered as its text, stripped, `[sound:]` removed,
/// whitespace collapsed. Bump `PLAIN_VERSION` when this changes: stored `notes.plain` rows are
/// rebuilt on open.
pub fn plain_text(html: &str) -> String {
    let no_cloze = crate::template::cloze::cloze_to_text(html);
    let stripped = strip_html_preserving_media(&no_cloze);
    let no_sound = SOUND_TAG.replace_all(&stripped, " ");
    WS.replace_all(&no_sound, " ").trim().to_string()
}

/// Version of the `plain_text` rules stored in col_meta `plain_version` (1 = cloze markup kept).
pub const PLAIN_VERSION: i64 = 2;

pub fn split_fields(flds: &str) -> Vec<String> {
    flds.split(FIELD_SEPARATOR).map(str::to_string).collect()
}

pub fn join_fields(fields: &[String]) -> String {
    fields.join("\u{1f}")
}

/// Anki tag string: ' a b ' with surrounding spaces; empty string when none.
pub fn join_tags(tags: &[String]) -> String {
    let t: Vec<&str> = tags.iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
    if t.is_empty() {
        String::new()
    } else {
        format!(" {} ", t.join(" "))
    }
}

pub fn split_tags(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Cloze ordinals (1-based) present in a string, nested included, sorted.
pub fn cloze_numbers(text: &str) -> Vec<i64> {
    let mut out: Vec<i64> = CLOZE_NUM.captures_iter(text).filter_map(|c| c[1].parse::<i64>().ok()).filter(|n| *n > 0).collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_anki() {
        assert_eq!(field_checksum("test"), 2840236005);
        assert_eq!(field_checksum("今日"), 1464653051);
    }

    #[test]
    fn strips_html_like_anki() {
        assert_eq!(strip_html("a<b>b</b> &amp; c&nbsp;d <!-- x --> <style>p{}</style>e"), "ab & c d  e");
        assert_eq!(strip_html_preserving_media(r#"x <img src="pic.png"> y"#), "x  pic.png  y");
        assert_eq!(strip_html("&#x41;&#66;&Aacute;&bogus;"), "ABÁ&bogus;");
    }

    #[test]
    fn empty_fields_and_tags() {
        assert!(field_is_empty(" <br> <div></div>\n"));
        assert!(!field_is_empty("x"));
        assert_eq!(join_tags(&["a".into(), " b ".into(), "".into()]), " a b ");
        assert_eq!(join_tags(&[]), "");
        assert_eq!(split_tags(" a  b "), vec!["a", "b"]);
    }

    #[test]
    fn cloze_numbers_sorted_unique() {
        assert_eq!(cloze_numbers("{{c3::a}} {{c1::b}} {{c1::c}} {{c0::d}}"), vec![1, 3]);
    }

    #[test]
    fn plain_text_collapses_and_strips() {
        assert_eq!(plain_text("<b>Hello</b>  {{c1::world::hint}} [sound:x.mp3]"), "Hello world");
    }

    #[test]
    fn base91_known_value() {
        assert_eq!(base91(0), "");
        assert_eq!(base91(90), "~");
        assert_eq!(base91(91), "ba");
        assert!(new_guid().len() <= 10);
    }
}
