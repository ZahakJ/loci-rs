//! Typed-answer comparison (`{{type:Field}}`; recall `src/core/template/typed.ts`, modelled on
//! Anki's rslib/src/typeanswer.rs).
//!
//! Both strings are normalised (NFC, whitespace collapsed; the expected text is HTML-stripped
//! with `[sound:]` tags removed), then compared per user-perceived character (a base code point
//! plus its combining marks) with optional case folding and diacritic stripping. The diff is a
//! longest-common-subsequence alignment; the HTML mirrors Anki's markup so existing deck CSS
//! keeps working:
//!   `<code id=typeans>typed line<br><span id=typearrow>&darr;</span><br>expected line</code>`
//! with `typeGood` (matching), `typeBad` (wrong/extra typed chars) and `typeMissed` (missing
//! chars; shown as '-' in the typed line).

use crate::text::strip_html;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::normalize::{is_combining, nfc, nfd};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TypedCompareOptions {
    /// compare case-insensitively (default false, like Anki)
    pub ignore_case: bool,
    /// strip combining marks before comparing, like Anki's `nc` modifier (default false)
    pub ignore_diacritics: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypedDiffOp {
    Equal,
    Replace,
    /// extra typed text
    Delete,
    /// expected text that was not typed
    Insert,
}

/// One aligned run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedSegment {
    pub op: TypedDiffOp,
    pub typed: String,
    pub expected: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedResult {
    pub correct: bool,
    pub segments: Vec<TypedSegment>,
    /// Anki-compatible HTML (`<code id=typeans>…`)
    pub html: String,
    /// normalised strings that were compared (display forms, not folded)
    pub expected: String,
    pub typed: String,
}

static SOUND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:[^\]]*\]").expect("valid regex"));
static BREAKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>|</?div[^>]*>|</?p[^>]*>|\n").expect("valid regex"));
static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s\u{a0}]+").expect("valid regex"));
/// Splits into user-perceived characters: base code point plus following combining marks.
static UNIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\P{M}\p{M}*|\p{M}+").expect("valid regex"));

/// Anki `html_to_text_line` + av-tag stripping: one line of plain text from field HTML.
pub fn expected_text_from_html(html: &str) -> String {
    let s = SOUND.replace_all(html, "");
    let s = BREAKS.replace_all(&s, " ");
    normalize_typed_text(&strip_html(&s))
}

/// NFC + trim + collapse runs of whitespace (incl. NBSP) to one space.
pub fn normalize_typed_text(s: &str) -> String {
    WS.replace_all(&nfc(s), " ").trim().to_string()
}

fn units(s: &str) -> Vec<String> {
    UNIT.find_iter(s).map(|m| m.as_str().to_string()).collect()
}

fn fold_key(u: &str, o: TypedCompareOptions) -> String {
    let mut k = nfd(u);
    if o.ignore_diacritics {
        k = k.chars().filter(|c| !is_combining(*c)).collect();
    }
    if o.ignore_case {
        k = k.to_lowercase();
    }
    nfc(&k)
}

/// Largest table the LCS is allowed to build (cells); beyond it the middle is one replace.
const MAX_CELLS: usize = 4_000_000;

struct Op {
    op: TypedDiffOp,
    a0: usize,
    a1: usize,
    b0: usize,
    b1: usize,
}

/// LCS alignment of key arrays → opcodes over index ranges (difflib-style).
fn align(a: &[String], b: &[String]) -> Vec<Op> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] {
        suf += 1;
    }
    let n = a.len() - pre - suf;
    let m = b.len() - pre - suf;
    // Pairs of (i in a, j in b) that match, in order.
    let mut pairs: Vec<(usize, usize)> = (0..pre).map(|i| (i, i)).collect();
    if n > 0 && m > 0 && (n + 1) * (m + 1) <= MAX_CELLS {
        let w = m + 1;
        let mut dp = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i * w + j] = if a[pre + i] == b[pre + j] {
                    dp[(i + 1) * w + j + 1] + 1
                } else {
                    dp[(i + 1) * w + j].max(dp[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a[pre + i] == b[pre + j] {
                pairs.push((pre + i, pre + j));
                i += 1;
                j += 1;
            } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    for k in 0..suf {
        pairs.push((a.len() - suf + k, b.len() - suf + k));
    }
    // Turn matches into opcodes.
    let mut ops: Vec<Op> = Vec::new();
    let (mut ai, mut bi) = (0, 0);
    fn push_op(ops: &mut Vec<Op>, op: TypedDiffOp, a0: usize, a1: usize, b0: usize, b1: usize) {
        if let Some(last) = ops.last_mut() {
            if last.op == op && last.a1 == a0 && last.b1 == b0 {
                last.a1 = a1;
                last.b1 = b1;
                return;
            }
        }
        ops.push(Op { op, a0, a1, b0, b1 });
    }
    pairs.push((a.len(), b.len()));
    for (pa, pb) in pairs {
        if pa > ai && pb > bi {
            push_op(&mut ops, TypedDiffOp::Replace, ai, pa, bi, pb);
        } else if pa > ai {
            push_op(&mut ops, TypedDiffOp::Delete, ai, pa, bi, bi);
        } else if pb > bi {
            push_op(&mut ops, TypedDiffOp::Insert, ai, ai, bi, pb);
        }
        if pa < a.len() && pb < b.len() {
            push_op(&mut ops, TypedDiffOp::Equal, pa, pa + 1, pb, pb + 1);
        }
        ai = pa + 1;
        bi = pb + 1;
    }
    ops
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Compares a typed answer with the expected one.
/// `expected`: expected answer; HTML is stripped (pass plain text if already plain).
/// `typed`: what the user typed (plain text).
pub fn compare_typed_answer(expected: &str, typed: &str, opts: TypedCompareOptions) -> TypedResult {
    let exp = expected_text_from_html(expected);
    let giv = normalize_typed_text(typed);
    let ua = units(&giv);
    let ub = units(&exp);
    let ka: Vec<String> = ua.iter().map(|u| fold_key(u, opts)).collect();
    let kb: Vec<String> = ub.iter().map(|u| fold_key(u, opts)).collect();
    let ops = align(&ka, &kb);
    let segments: Vec<TypedSegment> = ops
        .iter()
        .map(|o| TypedSegment { op: o.op, typed: ua[o.a0..o.a1].concat(), expected: ub[o.b0..o.b1].concat() })
        .collect();
    let correct = ka == kb;

    let html = if correct {
        format!("<code id=typeans><span class=typeGood>{}</span></code>", esc(&giv))
    } else if giv.is_empty() {
        format!("<code id=typeans><span class=typeMissed>{}</span></code>", esc(&exp))
    } else {
        let mut top = String::new();
        let mut bottom = String::new();
        for s in &segments {
            match s.op {
                TypedDiffOp::Equal => {
                    top.push_str(&format!("<span class=typeGood>{}</span>", esc(&s.typed)));
                    bottom.push_str(&format!("<span class=typeGood>{}</span>", esc(&s.expected)));
                }
                TypedDiffOp::Delete => top.push_str(&format!("<span class=typeBad>{}</span>", esc(&s.typed))),
                TypedDiffOp::Insert => {
                    top.push_str(&format!("<span class=typeMissed>{}</span>", "-".repeat(units(&s.expected).len())));
                    bottom.push_str(&format!("<span class=typeMissed>{}</span>", esc(&s.expected)));
                }
                TypedDiffOp::Replace => {
                    top.push_str(&format!("<span class=typeBad>{}</span>", esc(&s.typed)));
                    bottom.push_str(&format!("<span class=typeMissed>{}</span>", esc(&s.expected)));
                }
            }
        }
        format!("<code id=typeans>{top}<br><span id=typearrow>&darr;</span><br>{bottom}</code>")
    };
    TypedResult { correct, segments, html, expected: exp, typed: giv }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: TypedCompareOptions = TypedCompareOptions { ignore_case: false, ignore_diacritics: false };
    const NO_DIACRITICS: TypedCompareOptions = TypedCompareOptions { ignore_case: false, ignore_diacritics: true };
    const NO_CASE: TypedCompareOptions = TypedCompareOptions { ignore_case: true, ignore_diacritics: false };

    fn seg(op: TypedDiffOp, typed: &str, expected: &str) -> TypedSegment {
        TypedSegment { op, typed: typed.into(), expected: expected.into() }
    }

    #[test]
    fn exact_match() {
        let r = compare_typed_answer("Paris", "Paris", DEFAULT);
        assert!(r.correct);
        assert_eq!(r.segments, [seg(TypedDiffOp::Equal, "Paris", "Paris")]);
        assert_eq!(r.html, "<code id=typeans><span class=typeGood>Paris</span></code>");
    }

    #[test]
    fn typo_substitution() {
        let r = compare_typed_answer("Paris", "Parys", DEFAULT);
        assert!(!r.correct);
        assert_eq!(
            r.segments,
            [seg(TypedDiffOp::Equal, "Par", "Par"), seg(TypedDiffOp::Replace, "y", "i"), seg(TypedDiffOp::Equal, "s", "s")]
        );
        assert_eq!(
            r.html,
            "<code id=typeans><span class=typeGood>Par</span><span class=typeBad>y</span><span class=typeGood>s</span>\
             <br><span id=typearrow>&darr;</span><br>\
             <span class=typeGood>Par</span><span class=typeMissed>i</span><span class=typeGood>s</span></code>"
        );
    }

    #[test]
    fn missing_characters() {
        let r = compare_typed_answer("Berlin", "Brln", DEFAULT);
        let missing: Vec<&str> = r.segments.iter().filter(|s| s.op == TypedDiffOp::Insert).map(|s| s.expected.as_str()).collect();
        assert_eq!(missing, ["e", "i"]);
        assert!(r.html.contains("<span class=typeMissed>-</span>"));
    }

    #[test]
    fn extra_characters() {
        let r = compare_typed_answer("cat", "catss", DEFAULT);
        assert_eq!(r.segments, [seg(TypedDiffOp::Equal, "cat", "cat"), seg(TypedDiffOp::Delete, "ss", "")]);
        assert!(r.html.contains("<span class=typeBad>ss</span>"));
    }

    #[test]
    fn empty_input_shows_the_expected_answer_as_missed() {
        assert_eq!(
            compare_typed_answer("a <b>c</b>", "  ", DEFAULT).html,
            "<code id=typeans><span class=typeMissed>a c</span></code>"
        );
    }

    #[test]
    fn diacritics_matter_by_default_and_can_be_ignored() {
        assert!(!compare_typed_answer("café", "cafe", DEFAULT).correct);
        assert!(compare_typed_answer("café", "cafe", NO_DIACRITICS).correct);
        assert!(
            compare_typed_answer("Ünïcödé", "unicode", TypedCompareOptions { ignore_case: true, ignore_diacritics: true })
                .correct
        );
        // Arabic harakat are combining marks
        assert!(compare_typed_answer("كَتَبَ", "كتب", NO_DIACRITICS).correct);
        assert!(!compare_typed_answer("كَتَبَ", "كتب", DEFAULT).correct);
    }

    #[test]
    fn case_folding_is_optional() {
        assert!(!compare_typed_answer("Paris", "paris", DEFAULT).correct);
        assert!(compare_typed_answer("Paris", "paris", NO_CASE).correct);
    }

    #[test]
    fn nfc_and_nfd_forms_compare_equal_and_whitespace_collapses() {
        assert!(compare_typed_answer("été", "e\u{301}te\u{301}", DEFAULT).correct);
        assert!(compare_typed_answer("a  b", " a b ", DEFAULT).correct);
    }

    #[test]
    fn display_keeps_the_original_characters_while_comparing_folded_ones() {
        let r = compare_typed_answer("Straße", "strasse", NO_CASE);
        assert!(!r.correct);
        assert_eq!(r.segments[0], seg(TypedDiffOp::Equal, "stra", "Stra"));
    }

    #[test]
    fn escapes_html_in_the_output() {
        assert_eq!(
            compare_typed_answer("&lt;x&gt;", "<x>", DEFAULT).html,
            "<code id=typeans><span class=typeGood>&lt;x&gt;</span></code>"
        );
    }

    #[test]
    fn expected_text_from_html_strips_tags_sound_and_line_breaks() {
        assert_eq!(expected_text_from_html("one<br>two<div>three</div>[sound:a.mp3]&nbsp;four"), "one two three four");
    }
}
