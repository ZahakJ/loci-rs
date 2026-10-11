//! Maths and media markup normalisation for rendered card HTML (recall `src/core/template/math.ts`).
//!
//! Anki maths delimiters become one element the UI renders with KaTeX:
//!   `<span class="loci-math" data-display="0|1">TeX source, HTML-escaped</span>`
//!   `\( … \)` and `[$] … [/$]` → inline (0); `\[ … \]` and `[$$] … [/$$]` → display (1);
//!   `[latex] … [/latex]` (a LaTeX *document body*, rendered by Anki to an image) is unwrapped when
//!   it is a single `$…$`, `$$…$$`, `\(…\)`, `\[…\]` or displaymath/equation environment;
//!   otherwise it becomes display maths with `data-source="latex"` so the UI can fall back to
//!   showing the source if KaTeX rejects it.
//! HTML inside maths is removed (`<br>`/`<div>` become newlines for [latex]-style blocks, spaces
//! for MathJax) and entities decoded before escaping, matching what MathJax/LaTeX would see.
//!
//! All rewriting happens outside HTML tags, so attribute values are never corrupted.

use crate::text::{decode_entities, strip_html};
use regex::Regex;
use std::sync::LazyLock;

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->|</?[A-Za-z!][^>]*>").expect("valid regex"));

/// Replaces every byte inside HTML tags/comments with U+0001 so regexes only see text
/// (byte-for-byte, so offsets into the masked string index the original).
pub fn mask_tags(html: &str) -> String {
    TAG.replace_all(html, |m: &regex::Captures| "\u{1}".repeat(m[0].len())).into_owned()
}

/// Runs a regex over the text outside tags (matches may span tags, but never start or end
/// inside one). `f` receives the original substring and capture groups taken from the original.
pub fn replace_outside_tags(html: &str, re: &Regex, mut f: impl FnMut(&str, &[Option<&str>]) -> String) -> String {
    let masked = mask_tags(html);
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    for m in re.captures_iter(&masked) {
        let whole = match m.get(0) {
            Some(w) => w,
            None => continue,
        };
        let groups: Vec<Option<&str>> = (1..m.len()).map(|k| m.get(k).map(|g| &html[g.start()..g.end()])).collect();
        out.push_str(&html[last..whole.start()]);
        out.push_str(&f(&html[whole.start()..whole.end()], &groups));
        last = whole.end();
    }
    out.push_str(&html[last..]);
    out
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

static LATEX_BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>|<div>").expect("valid regex"));
static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid regex"));

/// Anki's LaTeX pre-processing: <br>/<div> → newline, then strip_html (entities decoded).
fn latex_source(html: &str) -> String {
    strip_html(&LATEX_BREAKS.replace_all(html, "\n")).trim().to_string()
}

fn mathjax_source(html: &str) -> String {
    strip_html(&BR.replace_all(html, " ")).trim().to_string()
}

fn math_span(tex: &str, display: bool, extra: &str) -> String {
    format!("<span class=\"loci-math\" data-display=\"{}\"{extra}>{}</span>", if display { 1 } else { 0 }, escape_html(tex))
}

static LATEX_DD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\$\$(.+)\$\$$").expect("valid regex"));
static LATEX_D: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\$([^$]+)\$$").expect("valid regex"));
static LATEX_BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\\\[(.+)\\\]$").expect("valid regex"));
static LATEX_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\\\((.+)\\\)$").expect("valid regex"));
static LATEX_ENV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^\\begin\{(displaymath|equation\*?)\}(.+)\\end\{(displaymath|equation\*?)\}$").expect("valid regex")
});

/// Converts the body of `[latex]…[/latex]` into a maths span.
fn latex_block(src: &str) -> String {
    let s = src.trim();
    if let Some(m) = LATEX_DD.captures(s) {
        return math_span(m[1].trim(), true, "");
    }
    if let Some(m) = LATEX_D.captures(s) {
        return math_span(m[1].trim(), false, "");
    }
    if let Some(m) = LATEX_BRACKET.captures(s) {
        return math_span(m[1].trim(), true, "");
    }
    if let Some(m) = LATEX_PAREN.captures(s) {
        return math_span(m[1].trim(), false, "");
    }
    // math.ts:71 uses a backreference `\\end\{\1\}`; the begin/end names must agree.
    if let Some(m) = LATEX_ENV.captures(s) {
        if m[1] == m[3] {
            return math_span(m[2].trim(), true, "");
        }
    }
    math_span(s, true, " data-source=\"latex\"")
}

static MATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)\[latex\](.*?)\[/latex\]|\[\$\$\](.*?)\[/\$\$\]|\[\$\](.*?)\[/\$\]|\\\((.*?)\\\)|\\\[(.*?)\\\]")
        .expect("valid regex")
});
static MATH_HINT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[(?:latex|\$\$?)\]|\\[(\[]").expect("valid regex"));

/// Normalises all maths delimiters in card HTML into `.loci-math` spans.
pub fn normalize_math(html: &str) -> String {
    if !MATH_HINT.is_match(html) {
        return html.to_string();
    }
    replace_outside_tags(html, &MATH, |_m, g| {
        if let Some(latex) = g[0] {
            return latex_block(&latex_source(latex));
        }
        if let Some(dd) = g[1] {
            return math_span(&latex_source(dd), true, "");
        }
        if let Some(d) = g[2] {
            return math_span(&latex_source(d), false, "");
        }
        if let Some(paren) = g[3] {
            return math_span(&mathjax_source(paren), false, "");
        }
        math_span(&mathjax_source(g[4].unwrap_or("")), true, "")
    })
}

static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:[a-z][a-z0-9+.-]*:|//|#)").expect("valid regex"));

/// True when a reference is a remote/absolute URL rather than a collection media filename.
pub fn is_external_ref(r: &str) -> bool {
    EXTERNAL.is_match(r.trim())
}

/// `decodeURIComponent`: percent-escapes decoded when they form valid UTF-8; None otherwise.
pub(crate) fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Filename from an HTML attribute value: entities decoded, then %-escapes decoded when valid.
pub fn media_name_from_attr(raw: &str) -> String {
    let s = decode_entities(raw);
    if !s.contains('%') {
        return s;
    }
    decode_uri_component(&s).unwrap_or(s)
}

static MEDIA_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<(?:img|audio|video|source|track|object|input)\b[^>]*>").expect("valid regex"));
static MEDIA_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\s(?:src|data|poster)\s*=\s*)(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#).expect("valid regex")
});

/// Rewrites src/data/poster of media elements through `media_url` (remote URLs and data: untouched).
pub fn rewrite_media_urls(html: &str, media_url: &dyn Fn(&str) -> String) -> String {
    MEDIA_TAG
        .replace_all(html, |tag: &regex::Captures| {
            MEDIA_ATTR
                .replace_all(&tag[0], |m: &regex::Captures| {
                    let raw = m.get(2).or_else(|| m.get(3)).or_else(|| m.get(4)).map_or("", |x| x.as_str());
                    if raw.trim().is_empty() || is_external_ref(raw) {
                        return m[0].to_string();
                    }
                    format!("{}\"{}\"", &m[1], escape_attr(&media_url(&media_name_from_attr(raw))))
                })
                .into_owned()
        })
        .into_owned()
}

static SOUND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:(.+?)\]").expect("valid regex"));

pub struct SoundReplacement {
    pub html: String,
    pub media: Vec<String>,
}

/// Replaces `[sound:x]` with audio buttons and returns the filenames in play order.
/// `index` numbering continues from `start_index` (so FrontSide and back share one sequence).
pub fn replace_sound_tags(html: &str, media_url: &dyn Fn(&str) -> String, label: &str, start_index: usize) -> SoundReplacement {
    let mut media: Vec<String> = Vec::new();
    if !html.contains("[sound:") {
        return SoundReplacement { html: html.to_string(), media };
    }
    let out = replace_outside_tags(html, &SOUND, |_m, g| {
        let name = decode_entities(&strip_html(g[0].unwrap_or(""))).trim().to_string();
        if name.is_empty() {
            return String::new();
        }
        let i = start_index + media.len();
        let button = format!(
            "<button type=\"button\" class=\"loci-audio\" data-src=\"{}\" data-name=\"{}\" data-index=\"{i}\" aria-label=\"{}\"></button>",
            escape_attr(&media_url(&name)),
            escape_attr(&name),
            escape_attr(label)
        );
        media.push(name);
        button
    });
    SoundReplacement { html: out, media }
}

/// `encodeURIComponent`, for the test media-url helper (recall tests/unit/template/helpers.ts).
#[cfg(test)]
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_url(name: &str) -> String {
        format!("/media/{}", percent_encode(name))
    }

    #[test]
    fn rewrites_media_sources_and_leaves_remote_and_data_urls() {
        let html = "<img src=\"pic&amp;1.jpg\"><img src='b.png' alt=x><img src=c%20d.png><img src=\"https://x.org/a.png\"><img src=\"data:image/png;base64,AA\"><video poster=\"p.jpg\"><source src=\"v.mp4\"></video><audio src=\"s.mp3\"></audio>";
        assert_eq!(
            rewrite_media_urls(html, &media_url),
            "<img src=\"/media/pic%261.jpg\"><img src=\"/media/b.png\" alt=x><img src=\"/media/c%20d.png\"><img src=\"https://x.org/a.png\"><img src=\"data:image/png;base64,AA\"><video poster=\"/media/p.jpg\"><source src=\"/media/v.mp4\"></video><audio src=\"/media/s.mp3\"></audio>"
        );
    }

    #[test]
    fn normalises_every_delimiter_family() {
        assert_eq!(normalize_math("a \\(x^2 < 1\\) b"), "a <span class=\"loci-math\" data-display=\"0\">x^2 &lt; 1</span> b");
        assert_eq!(normalize_math("\\[\\int_0^1 f\\]"), "<span class=\"loci-math\" data-display=\"1\">\\int_0^1 f</span>");
        assert_eq!(
            normalize_math("[$]a&amp;b[/$] [$$]c<br>d[/$$]"),
            "<span class=\"loci-math\" data-display=\"0\">a&amp;b</span> <span class=\"loci-math\" data-display=\"1\">c\nd</span>"
        );
        assert_eq!(normalize_math("[latex]$e^{i\\pi}$[/latex]"), "<span class=\"loci-math\" data-display=\"0\">e^{i\\pi}</span>");
        assert_eq!(
            normalize_math("[LATEX]\\begin{displaymath}x\\end{displaymath}[/latex]"),
            "<span class=\"loci-math\" data-display=\"1\">x</span>"
        );
        assert_eq!(
            normalize_math("[latex]\\begin{align}a&=b\\end{align}[/latex]"),
            "<span class=\"loci-math\" data-display=\"1\" data-source=\"latex\">\\begin{align}a&amp;=b\\end{align}</span>"
        );
    }

    #[test]
    fn strips_html_inside_maths_and_never_rewrites_inside_attributes() {
        assert_eq!(normalize_math("\\(a<b>b</b>\\)"), "<span class=\"loci-math\" data-display=\"0\">ab</span>");
        assert_eq!(normalize_math("<span title=\"\\(x\\)\">t</span>"), "<span title=\"\\(x\\)\">t</span>");
        assert_eq!(normalize_math("plain"), "plain");
    }

    #[test]
    fn sound_tags_inside_attributes_are_not_collected() {
        let out = replace_sound_tags("<span data-cloze=\"[sound:x.mp3]\">[...]</span>", &media_url, "Play", 0);
        assert!(out.media.is_empty());
        let out = replace_sound_tags("Hi [sound:a b.mp3]", &media_url, "Play audio", 0);
        assert_eq!(out.media, ["a b.mp3"]);
        assert_eq!(
            out.html,
            "Hi <button type=\"button\" class=\"loci-audio\" data-src=\"/media/a%20b.mp3\" data-name=\"a b.mp3\" data-index=\"0\" aria-label=\"Play audio\"></button>"
        );
    }

    #[test]
    fn mask_keeps_byte_offsets_and_decodes_percent_escapes() {
        let html = "<img alt=\"é\">x";
        assert_eq!(mask_tags(html).len(), html.len());
        assert_eq!(media_name_from_attr("a%20b&amp;c"), "a b&c");
        assert_eq!(media_name_from_attr("100%25"), "100%");
        assert_eq!(media_name_from_attr("bad%zz"), "bad%zz");
        assert!(is_external_ref("https://x") && is_external_ref("//cdn/x") && is_external_ref("#a") && !is_external_ref("b.png"));
    }
}
