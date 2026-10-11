//! Cloze deletions (recall `src/core/template/cloze.ts`; a port of Anki's rslib/src/cloze.rs).
//!
//! Syntax: `{{c1::text}}`, `{{c1::text::hint}}`, `{{c1,3::shared}}`, nested up to 10 levels,
//! overlapping via nesting. A `}}` inside a MathJax `\( … \)` / `\[ … \]` that started inside
//! a cloze does not close the cloze. Stray `}}` are literal text; clozes left open at the end
//! of the text are restored as literal text (Anki drops nothing silently either way).

use crate::text::strip_html;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct ClozeNode {
    /// sorted, deduplicated 1-based ordinals (0 allowed but never generates a card)
    pub ordinals: Vec<i64>,
    pub nodes: Vec<ClozeTextOrCloze>,
    pub hint: Option<String>,
    /// raw opening marker, for restoring unclosed clozes
    pub marker: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClozeTextOrCloze {
    Text(String),
    Cloze(ClozeNode),
}

enum Tok {
    Open { ords: Vec<i64>, raw: String },
    Close,
    Text(String),
}

static OPEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{\{c(\d+(?:,\d+)*)::").expect("valid regex"));
const MAX_NESTING: usize = 10;

/// cloze.ts:36 looks at `text.slice(i, i + 64)` for the opening marker; keep that window.
fn open_at(text: &str, i: usize) -> Option<(Vec<i64>, String)> {
    let mut end = (i + 64).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let m = OPEN_RE.captures(&text[i..end])?;
    let whole = m.get(0)?.as_str().to_string();
    let ords = parse_ords(m.get(1)?.as_str());
    Some((ords, whole))
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    // Depth of clozes opened so far and whether we are in MathJax opened inside a cloze.
    let mut depth = 0usize;
    let mut math_close: Option<&str> = None;
    fn flush(buf: &mut String, out: &mut Vec<Tok>) {
        if !buf.is_empty() {
            out.push(Tok::Text(std::mem::take(buf)));
        }
    }
    while i < text.len() {
        let rest = &text[i..];
        if let Some(mc) = math_close {
            if rest.starts_with(mc) {
                buf.push_str(mc);
                i += 2;
                math_close = None;
                continue;
            }
            if rest.starts_with("{{c") {
                if let Some((ords, raw)) = open_at(text, i) {
                    flush(&mut buf, &mut out);
                    i += raw.len();
                    out.push(Tok::Open { ords, raw });
                    depth += 1;
                    continue;
                }
            }
            let ch = rest.chars().next().unwrap_or('\0');
            buf.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if rest.starts_with("{{c") {
            if let Some((ords, raw)) = open_at(text, i) {
                flush(&mut buf, &mut out);
                i += raw.len();
                out.push(Tok::Open { ords, raw });
                depth += 1;
                continue;
            }
        }
        if rest.starts_with("}}") {
            flush(&mut buf, &mut out);
            out.push(Tok::Close);
            depth = depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if depth > 0 && (rest.starts_with("\\(") || rest.starts_with("\\[")) {
            math_close = Some(if rest.starts_with("\\(") { "\\)" } else { "\\]" });
            buf.push_str(&rest[..2]);
            i += 2;
            continue;
        }
        let ch = rest.chars().next().unwrap_or('\0');
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut buf, &mut out);
    out
}

fn parse_ords(s: &str) -> Vec<i64> {
    let mut out: Vec<i64> = s.split(',').filter_map(|p| p.parse::<i64>().ok()).filter(|n| *n <= 65535).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Text that closes a parsed cloze verbatim (used to restore unclosed clozes).
fn nodes_to_source(nodes: &[ClozeTextOrCloze]) -> String {
    let mut s = String::new();
    for n in nodes {
        match n {
            ClozeTextOrCloze::Text(t) => s.push_str(t),
            ClozeTextOrCloze::Cloze(c) => {
                s.push_str(&c.marker);
                s.push_str(&nodes_to_source(&c.nodes));
                if let Some(h) = &c.hint {
                    s.push_str("::");
                    s.push_str(h);
                }
                s.push_str("}}");
            }
        }
    }
    s
}

fn push_text(arr: &mut Vec<ClozeTextOrCloze>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(ClozeTextOrCloze::Text(last)) = arr.last_mut() {
        last.push_str(s);
    } else {
        arr.push(ClozeTextOrCloze::Text(s.to_string()));
    }
}

/// Parses text into text runs and (nested) clozes, exactly like Anki's `parse_text_with_clozes`.
pub fn parse_clozes(text: &str) -> Vec<ClozeTextOrCloze> {
    let mut open: Vec<ClozeNode> = Vec::new();
    let mut output: Vec<ClozeTextOrCloze> = Vec::new();
    // Opens beyond MAX_NESTING are ignored by Anki, but their closing `}}` still pops; track them.
    let mut ignored_opens = 0usize;
    for tok in tokenize(text) {
        match tok {
            Tok::Open { ords, raw } => {
                if open.len() < MAX_NESTING {
                    open.push(ClozeNode { ordinals: ords, nodes: Vec::new(), hint: None, marker: raw });
                } else {
                    ignored_opens += 1;
                }
            }
            Tok::Text(t) => {
                if let Some(cloze) = open.last_mut() {
                    let mut s = t.as_str();
                    let is_io = cloze.nodes.is_empty() && s.starts_with("image-occlusion:");
                    let idx = if is_io { None } else { s.find("::") };
                    if let Some(idx) = idx {
                        cloze.hint = Some(s[idx + 2..].to_string());
                        s = &s[..idx];
                    }
                    push_text(&mut cloze.nodes, s);
                } else {
                    push_text(&mut output, &t);
                }
            }
            Tok::Close => {
                if ignored_opens > 0 {
                    ignored_opens -= 1;
                    continue;
                }
                match open.pop() {
                    Some(cloze) => match open.last_mut() {
                        Some(parent) => parent.nodes.push(ClozeTextOrCloze::Cloze(cloze)),
                        None => output.push(ClozeTextOrCloze::Cloze(cloze)),
                    },
                    None => push_text(&mut output, "}}"),
                }
            }
        }
    }
    // Unclosed clozes become literal text again.
    while let Some(c) = open.pop() {
        let mut restored = c.marker.clone();
        restored.push_str(&nodes_to_source(&c.nodes));
        if let Some(h) = &c.hint {
            restored.push_str("::");
            restored.push_str(h);
        }
        match open.last_mut() {
            Some(parent) => push_text(&mut parent.nodes, &restored),
            None => push_text(&mut output, &restored),
        }
    }
    output
}

/// The revealed text of a cloze: its text plus the text of nested clozes, without markers.
pub fn clozed_text(c: &ClozeNode) -> String {
    let mut s = String::new();
    for n in &c.nodes {
        match n {
            ClozeTextOrCloze::Text(t) => s.push_str(t),
            ClozeTextOrCloze::Cloze(inner) => s.push_str(&clozed_text(inner)),
        }
    }
    s
}

/// Cloze markup rendered as the text a reader sees with every cloze revealed:
/// `{{c1::Paris::capital}}` → `Paris`, nested clozes flattened, hints dropped; image-occlusion
/// shapes carry no text and vanish. Used for search text and browse cells.
pub fn cloze_to_text(text: &str) -> String {
    if !text.contains("{{c") {
        return text.to_string();
    }
    fn walk(ns: &[ClozeTextOrCloze], out: &mut String) {
        for n in ns {
            match n {
                ClozeTextOrCloze::Text(t) => out.push_str(t),
                ClozeTextOrCloze::Cloze(c) => {
                    if image_occlusion_text(c).is_none() {
                        walk(&c.nodes, out);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    walk(&parse_clozes(text), &mut out);
    out
}

pub(crate) fn encode_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Distinct cloze ordinals (>0) present in a text, nested included (Anki `cloze_numbers_in_string`).
pub fn cloze_numbers_in_string(text: &str) -> Vec<i64> {
    fn walk(ns: &[ClozeTextOrCloze], set: &mut Vec<i64>) {
        for n in ns {
            if let ClozeTextOrCloze::Cloze(c) = n {
                for o in &c.ordinals {
                    if *o > 0 {
                        set.push(*o);
                    }
                }
                walk(&c.nodes, set);
            }
        }
    }
    let mut set = Vec::new();
    walk(&parse_clozes(text), &mut set);
    set.sort_unstable();
    set.dedup();
    set
}

/// IO cloze body (`image-occlusion:…`) if this cloze is an image-occlusion shape.
pub fn image_occlusion_text(c: &ClozeNode) -> Option<&str> {
    if c.nodes.len() != 1 {
        return None;
    }
    match &c.nodes[0] {
        ClozeTextOrCloze::Text(t) => t.strip_prefix("image-occlusion:"),
        _ => None,
    }
}

/// How an image-occlusion cloze is rendered: `(body, active, ordinals, question) -> html`.
pub type RenderOcclusion<'a> = &'a dyn Fn(&str, bool, &[i64], bool) -> String;

#[derive(Default, Clone, Copy)]
pub struct RevealOptions<'a> {
    /// Adds `data-cloze-ord` (the active ordinal) and `data-cloze-index` (0-based order of the
    /// active cloze within this text) to every active span, so the UI can reveal them one by one.
    pub one_by_one: bool,
    /// Renders an image-occlusion cloze; default mimics Anki's `<div class=… data-shape=…>`.
    pub render_occlusion: Option<RenderOcclusion<'a>>,
}

static UNESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(.)").expect("valid regex"));
static NOT_ATTR_CHAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[^a-z0-9-]").expect("valid regex"));

/// Splits on `:` not preceded by a backslash (cloze.ts:151 `split(/(?<!\\):/)`).
fn split_unescaped_colon(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b':' && (i == 0 || bytes[i - 1] != b'\\') {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

fn default_occlusion_div(body: &str, active: bool, ordinals: &[i64], question: bool) -> String {
    let parts = split_unescaped_colon(body);
    let shape = parts.first().copied().unwrap_or("");
    let cls = if active {
        if question {
            "cloze"
        } else {
            "cloze-highlight"
        }
    } else {
        "cloze-inactive"
    };
    let mut attrs = String::new();
    for p in parts.iter().skip(1) {
        let Some(eq) = p.find('=') else { continue };
        if eq == 0 {
            continue;
        }
        let k = NOT_ATTR_CHAR.replace_all(&p[..eq], "");
        let v = UNESCAPE.replace_all(&p[eq + 1..], "$1");
        attrs.push_str(&format!(" data-{k}=\"{}\"", encode_attr(&v)));
    }
    let ords: Vec<String> = ordinals.iter().map(|o| o.to_string()).collect();
    format!("<div class=\"{cls}\" data-shape=\"{}\" data-ordinal=\"{}\"{attrs}></div>", encode_attr(shape), ords.join(","))
}

fn join_ords(ords: &[i64]) -> String {
    ords.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(",")
}

/// Anki `reveal_cloze_text`: renders a field for card ordinal `ord` (1-based).
/// Returns '' when no cloze with that ordinal exists in the text.
pub fn reveal_cloze_text(text: &str, ord: i64, question: bool, opts: RevealOptions<'_>) -> String {
    struct State<'a> {
        found: bool,
        active_index: usize,
        out: String,
        ord: i64,
        question: bool,
        opts: RevealOptions<'a>,
    }
    fn reveal(n: &ClozeTextOrCloze, st: &mut State<'_>) {
        let c = match n {
            ClozeTextOrCloze::Text(t) => {
                st.out.push_str(t);
                return;
            }
            ClozeTextOrCloze::Cloze(c) => c,
        };
        let active = c.ordinals.contains(&st.ord);
        if active {
            st.found = true;
        }
        if let Some(body) = image_occlusion_text(c) {
            let html = match st.opts.render_occlusion {
                Some(f) => f(body, active, &c.ordinals, st.question),
                None => default_occlusion_div(body, active, &c.ordinals, st.question),
            };
            st.out.push_str(&html);
            return;
        }
        let ords = join_ords(&c.ordinals);
        let extra = if active && st.opts.one_by_one {
            let s = format!(" data-cloze-ord=\"{}\" data-cloze-index=\"{}\"", st.ord, st.active_index);
            st.active_index += 1;
            s
        } else {
            String::new()
        };
        if active && st.question {
            st.out.push_str(&format!(
                "<span class=\"cloze\" data-cloze=\"{}\" data-ordinal=\"{ords}\"{extra}>[{}]</span>",
                encode_attr(&clozed_text(c)),
                c.hint.as_deref().unwrap_or("...")
            ));
            return;
        }
        if active {
            st.out.push_str(&format!("<span class=\"cloze\" data-ordinal=\"{ords}\"{extra}>"));
        } else {
            st.out.push_str(&format!("<span class=\"cloze-inactive\" data-ordinal=\"{ords}\">"));
        }
        for child in &c.nodes {
            reveal(child, st);
        }
        st.out.push_str("</span>");
    }
    let mut st = State { found: false, active_index: 0, out: String::new(), ord, question, opts };
    for n in &parse_clozes(text) {
        reveal(n, &mut st);
    }
    if st.found {
        st.out
    } else {
        String::new()
    }
}

static MATHJAX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)(\\[(\[])(.*?)(\\[)\]])").expect("valid regex"));

/// Anki `strip_html_inside_mathjax`: cloze spans inside `\(..\)`/`\[..\]` would break the TeX.
pub fn strip_html_inside_mathjax(text: &str) -> String {
    MATHJAX.replace_all(text, |c: &regex::Captures| format!("{}{}{}", &c[1], strip_html(&c[2]), &c[3])).into_owned()
}

/// The `cloze` template filter (Anki `cloze_filter`).
pub fn cloze_filter(text: &str, ord: i64, question: bool, opts: RevealOptions<'_>) -> String {
    strip_html_inside_mathjax(&reveal_cloze_text(text, ord, question, opts))
}

fn collect_active(nodes: &[ClozeTextOrCloze], ord: i64, question: bool, out: &mut Vec<String>) {
    for n in nodes {
        let ClozeTextOrCloze::Cloze(c) = n else { continue };
        if c.ordinals.contains(&ord) {
            out.push(if question { c.hint.clone().unwrap_or_else(|| "...".to_string()) } else { clozed_text(c) });
        }
        collect_active(&c.nodes, ord, question, out);
    }
}

/// The `cloze-only` filter (Anki `reveal_cloze_text_only`): active cloze texts (or hints) joined by ', '.
pub fn reveal_cloze_text_only(text: &str, ord: i64, question: bool) -> String {
    let mut out = Vec::new();
    collect_active(&parse_clozes(text), ord, question, &mut out);
    out.join(", ")
}

/// Expected answer for `{{type:cloze:F}}` (Anki `extract_cloze_for_typing`): the active cloze
/// texts; identical ones collapse to one, otherwise joined by ', '. Still HTML (caller strips).
pub fn extract_cloze_for_typing(text: &str, ord: i64) -> String {
    let mut out = Vec::new();
    collect_active(&parse_clozes(text), ord, false, &mut out);
    if out.is_empty() {
        return String::new();
    }
    if out.iter().all(|s| *s == out[0]) {
        out.swap_remove(0)
    } else {
        out.join(", ")
    }
}

// ───────────────────────── editor helpers ─────────────────────────

/// Next cloze number for "new cloze" (Ctrl+Shift+C): highest existing ordinal + 1, at least 1.
pub fn next_cloze_number(text: &str) -> i64 {
    cloze_numbers_in_string(text).last().map_or(1, |n| n + 1)
}

/// Highest existing ordinal (for "same cloze", Ctrl+Alt+Shift+C), or 1 when there is none.
pub fn current_cloze_number(text: &str) -> i64 {
    cloze_numbers_in_string(text).last().copied().unwrap_or(1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedCloze {
    pub text: String,
    pub selection_start: usize,
    pub selection_end: usize,
}

/// Wraps text[selection_start, selection_end) (char offsets) as `{{cN::…}}`. Returns the new text
/// and the selection to restore: the wrapped content, or the caret inside an empty cloze.
pub fn wrap_cloze(text: &str, selection_start: usize, selection_end: usize, n: i64) -> WrappedCloze {
    let len = text.chars().count();
    let a = selection_start.min(selection_end).min(len);
    let b = selection_start.max(selection_end).min(len);
    let open = format!("{{{{c{}::", n.max(1));
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..a].iter().collect();
    let inner: String = chars[a..b].iter().collect();
    let after: String = chars[b..].iter().collect();
    let out = format!("{before}{open}{inner}}}}}{after}");
    let open_len = open.chars().count();
    WrappedCloze { text: out, selection_start: a + open_len, selection_end: a + open_len + (b - a) }
}

/// Plain text of a cloze answer for typing comparison (HTML stripped, entities decoded).
pub fn cloze_answer_plain(html: &str) -> String {
    strip_html(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: RevealOptions<'static> = RevealOptions { one_by_one: false, render_occlusion: None };

    #[test]
    fn anki_vector_nested_answer() {
        assert_eq!(
            reveal_cloze_text("foo {{c1::bar {{c2::baz}}}}", 1, false, NONE),
            "foo <span class=\"cloze\" data-ordinal=\"1\">bar <span class=\"cloze-inactive\" data-ordinal=\"2\">baz</span></span>"
        );
    }

    #[test]
    fn anki_vector_nested_question_with_hint_stripped() {
        let t = "foo {{c1::bar {{c2::baz}}::qux}}";
        assert_eq!(strip_html(&reveal_cloze_text(t, 1, true, NONE)), "foo [qux]");
        assert_eq!(strip_html(&reveal_cloze_text(t, 2, true, NONE)), "foo bar [...]");
        assert_eq!(strip_html(&reveal_cloze_text(t, 1, false, NONE)), "foo bar baz");
    }

    #[test]
    fn anki_vector_cloze_numbers() {
        assert_eq!(cloze_numbers_in_string("{{c2::te}}{{c1::s}}t{{"), [1, 2]);
        assert_eq!(cloze_numbers_in_string("{{c0::x}}{{c1,3::y}}"), [1, 3]);
    }

    #[test]
    fn anki_vector_cloze_only_joins_texts() {
        assert_eq!(reveal_cloze_text_only("{{c1::a}} {{c1::b}} {{c2::c}}", 1, false), "a, b");
        assert_eq!(reveal_cloze_text_only("{{c1::a::h}} {{c1::b}}", 1, true), "h, ...");
    }

    #[test]
    fn active_question_span_carries_data_cloze_with_the_revealed_html() {
        assert_eq!(
            reveal_cloze_text("{{c1::<b>x</b> & \"y\"}} z", 1, true, NONE),
            "<span class=\"cloze\" data-cloze=\"&lt;b&gt;x&lt;/b&gt; &amp; &quot;y&quot;\" data-ordinal=\"1\">[...]</span> z"
        );
    }

    #[test]
    fn hint_on_the_question_inactive_siblings() {
        assert_eq!(
            reveal_cloze_text("{{c1::Paris::capital}} is in {{c2::France}}", 1, true, NONE),
            "<span class=\"cloze\" data-cloze=\"Paris\" data-ordinal=\"1\">[capital]</span> is in <span class=\"cloze-inactive\" data-ordinal=\"2\">France</span>"
        );
        assert_eq!(
            reveal_cloze_text("{{c1::Paris::capital}} is in {{c2::France}}", 2, false, NONE),
            "<span class=\"cloze-inactive\" data-ordinal=\"1\">Paris</span> is in <span class=\"cloze\" data-ordinal=\"2\">France</span>"
        );
    }

    #[test]
    fn returns_empty_string_when_the_ordinal_is_absent() {
        assert_eq!(reveal_cloze_text("{{c1::a}}", 3, true, NONE), "");
        assert_eq!(reveal_cloze_text("no clozes", 1, false, NONE), "");
    }

    #[test]
    fn overlapping_clozes_via_nesting() {
        let t = "{{c1::alpha {{c2::beta}}}} gamma";
        assert_eq!(
            reveal_cloze_text(t, 2, true, NONE),
            "<span class=\"cloze-inactive\" data-ordinal=\"1\">alpha <span class=\"cloze\" data-cloze=\"beta\" data-ordinal=\"2\">[...]</span></span> gamma"
        );
        assert_eq!(
            reveal_cloze_text(t, 1, true, NONE),
            "<span class=\"cloze\" data-cloze=\"alpha beta\" data-ordinal=\"1\">[...]</span> gamma"
        );
    }

    #[test]
    fn multi_ordinal_clozes_are_active_on_each_card() {
        assert_eq!(reveal_cloze_text("{{c1,2::x}}", 2, false, NONE), "<span class=\"cloze\" data-ordinal=\"1,2\">x</span>");
    }

    #[test]
    fn stray_closers_are_literal_and_unclosed_openers_restored() {
        assert_eq!(reveal_cloze_text("a}} {{c1::b}}", 1, false, NONE), "a}} <span class=\"cloze\" data-ordinal=\"1\">b</span>");
        assert_eq!(
            reveal_cloze_text("{{c1::b}} {{c2::open", 1, false, NONE),
            "<span class=\"cloze\" data-ordinal=\"1\">b</span> {{c2::open"
        );
    }

    #[test]
    fn closing_braces_inside_mathjax_started_in_a_cloze_do_not_close_it() {
        let t = "{{c1::\\(x^{a^{b}}\\)}} after";
        assert_eq!(reveal_cloze_text(t, 1, false, NONE), "<span class=\"cloze\" data-ordinal=\"1\">\\(x^{a^{b}}\\)</span> after");
    }

    #[test]
    fn cloze_filter_strips_html_inside_mathjax() {
        assert_eq!(cloze_filter("\\(a + {{c1::b}}\\)", 1, false, NONE), "\\(a + b\\)");
        assert_eq!(cloze_filter("\\(a + {{c1::b}}\\)", 1, true, NONE), "\\(a + [...]\\)");
    }

    #[test]
    fn one_by_one_markup_numbers_the_active_clozes() {
        let opts = RevealOptions { one_by_one: true, render_occlusion: None };
        assert_eq!(
            reveal_cloze_text("{{c1::a}} {{c2::b}} {{c1::c}}", 1, true, opts),
            "<span class=\"cloze\" data-cloze=\"a\" data-ordinal=\"1\" data-cloze-ord=\"1\" data-cloze-index=\"0\">[...]</span> \
             <span class=\"cloze-inactive\" data-ordinal=\"2\">b</span> \
             <span class=\"cloze\" data-cloze=\"c\" data-ordinal=\"1\" data-cloze-ord=\"1\" data-cloze-index=\"1\">[...]</span>"
        );
    }

    #[test]
    fn nesting_beyond_10_levels_ignores_deeper_openers() {
        let deep = format!("{}x{}", "{{c1::".repeat(11), "}}".repeat(11));
        let nodes = parse_clozes(&deep);
        assert_eq!(nodes.len(), 1);
        let mut depth = 0;
        let mut n = &nodes[0];
        while let ClozeTextOrCloze::Cloze(c) = n {
            depth += 1;
            n = &c.nodes[0];
        }
        assert_eq!(depth, 10);
    }

    #[test]
    fn type_cloze_expected_answer() {
        assert_eq!(extract_cloze_for_typing("{{c1::a}} {{c2::b}} {{c1::c}}", 1), "a, c");
        assert_eq!(extract_cloze_for_typing("{{c1::same}} and {{c1::same}}", 1), "same");
        assert_eq!(extract_cloze_for_typing("{{c1::x}}", 2), "");
        assert_eq!(extract_cloze_for_typing("{{c1::outer {{c2::inner}}}}", 1), "outer inner");
    }

    #[test]
    fn next_and_current_cloze_numbers() {
        assert_eq!(next_cloze_number(""), 1);
        assert_eq!(next_cloze_number("{{c1::a}} {{c3::b}}"), 4);
        assert_eq!(current_cloze_number("{{c1::a}} {{c3::b}}"), 3);
        assert_eq!(current_cloze_number("none"), 1);
    }

    #[test]
    fn wrap_cloze_wraps_the_selection_and_keeps_it_selected() {
        assert_eq!(
            wrap_cloze("The capital is Paris.", 15, 20, 1),
            WrappedCloze { text: "The capital is {{c1::Paris}}.".into(), selection_start: 21, selection_end: 26 }
        );
        assert_eq!(wrap_cloze("ab", 1, 1, 2), WrappedCloze { text: "a{{c2::}}b".into(), selection_start: 7, selection_end: 7 });
        assert_eq!(wrap_cloze("abc", 3, 0, 1).text, "{{c1::abc}}");
    }

    #[test]
    fn cloze_to_text_flattens_and_drops_occlusions() {
        assert_eq!(cloze_to_text("{{c1::Paris::capital}} is {{c2::in {{c3::France}}}}"), "Paris is in France");
        assert_eq!(cloze_to_text("{{c1::image-occlusion:rect:left=.1}}<br>x"), "<br>x");
        assert_eq!(cloze_to_text("plain"), "plain");
        assert_eq!(cloze_answer_plain("<b>a</b>&amp;"), "a&");
    }

    #[test]
    fn default_occlusion_div_markup() {
        let html = reveal_cloze_text("{{c1::image-occlusion:rect:left=.1:top=.2:text=a\\:b}}", 1, true, NONE);
        assert_eq!(html, "<div class=\"cloze\" data-shape=\"rect\" data-ordinal=\"1\" data-left=\".1\" data-top=\".2\" data-text=\"a:b\"></div>");
        let inactive = reveal_cloze_text("{{c1::image-occlusion:rect:left=.1}}{{c2::x}}", 2, false, NONE);
        assert!(
            inactive.starts_with("<div class=\"cloze-inactive\" data-shape=\"rect\" data-ordinal=\"1\" data-left=\".1\"></div>")
        );
        let highlight = reveal_cloze_text("{{c1::image-occlusion:rect:left=.1}}", 1, false, NONE);
        assert!(highlight.starts_with("<div class=\"cloze-highlight\""));
    }
}
