//! Anki card-template lexer and parser (a port of
//! rslib/src/template.rs).
//!
//! Grammar: text, `<!-- comments -->` (kept verbatim, handlebars inside are inert),
//! `{{Field}}`, `{{filter2:filter1:Field}}`, `{{#Field}}…{{/Field}}`, `{{^Field}}…{{/Field}}`.
//! The legacy `{{=<% %>=}}` delimiter switch at the start of a template is honoured.
//!
//! The parser is lenient: it always returns a tree (so previews keep working) plus a list of
//! structural errors with offsets into the source. Offsets are *character* indices.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Built-in special fields (rslib/src/notetype/mod.rs SPECIAL_FIELDS).
pub const SPECIAL_FIELDS: [&str; 8] = ["FrontSide", "Card", "CardFlag", "Deck", "Subdeck", "Tags", "Type", "CardID"];

/// Filters Loci implements natively; anything else passes text through unchanged.
pub const KNOWN_FILTERS: [&str; 12] =
    ["text", "cloze", "cloze-only", "hint", "type", "type-cloze", "type-nc", "furigana", "kana", "kanji", "tts", ""];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TemplateNode {
    Text {
        text: String,
        start: usize,
        end: usize,
    },
    Comment {
        text: String,
        start: usize,
        end: usize,
    },
    /// `filters` are in application order: innermost (closest to the field name) first.
    Replacement {
        key: String,
        filters: Vec<String>,
        start: usize,
        end: usize,
    },
    Conditional {
        key: String,
        negated: bool,
        children: Vec<TemplateNode>,
        start: usize,
        end: usize,
    },
}

impl TemplateNode {
    pub fn start(&self) -> usize {
        match self {
            TemplateNode::Text { start, .. }
            | TemplateNode::Comment { start, .. }
            | TemplateNode::Replacement { start, .. }
            | TemplateNode::Conditional { start, .. } => *start,
        }
    }
    pub fn end(&self) -> usize {
        match self {
            TemplateNode::Text { end, .. }
            | TemplateNode::Comment { end, .. }
            | TemplateNode::Replacement { end, .. }
            | TemplateNode::Conditional { end, .. } => *end,
        }
    }
    /// Field or section key of a replacement or conditional.
    pub fn key(&self) -> Option<&str> {
        match self {
            TemplateNode::Replacement { key, .. } | TemplateNode::Conditional { key, .. } => Some(key),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateErrorCode {
    /// `{{` without `}}`
    NoClosingBrackets,
    /// `{{#F}}` never closed
    UnclosedSection,
    /// `{{/X}}` while `{{#Y}}` is open
    MismatchedClose,
    /// `{{/X}}` with nothing open
    UnexpectedClose,
    /// `{{X}}` or `{{#X}}` where X is not a field
    UnknownField,
    /// the front renders nothing even with every field filled
    NoFrontField,
    /// cloze note type whose front has no {{cloze:…}}
    NoClozeFilter,
    /// `{{tts …:F}}` renders nothing in Loci
    TtsUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// blocks rendering
    Error,
    /// advisory
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateError {
    pub code: TemplateErrorCode,
    pub message: String,
    /// field or section name involved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub severity: Severity,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ParsedTemplate {
    pub nodes: Vec<TemplateNode>,
    pub errors: Vec<TemplateError>,
}

impl ParsedTemplate {
    pub fn has_errors(&self) -> bool {
        self.errors.iter().any(|e| e.severity == Severity::Error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokKind {
    Text,
    Comment,
    Replacement,
    Open,
    OpenNegated,
    Close,
}

/// Byte offsets during tokenising; converted to char offsets when building nodes.
struct Token {
    kind: TokKind,
    text: String,
    key: String,
    filters: Vec<String>,
    start: usize,
    end: usize,
}

/// Splits a handlebar body like Anki's `classify_handle`.
fn classify_handle(body: &str) -> (TokKind, String, Vec<String>) {
    let s = body.trim_start_matches('{').trim();
    if s.chars().count() < 2 {
        return (TokKind::Replacement, s.to_string(), Vec::new());
    }
    if let Some(rest) = s.strip_prefix('#') {
        return (TokKind::Open, rest.trim().to_string(), Vec::new());
    }
    if let Some(rest) = s.strip_prefix('/') {
        return (TokKind::Close, rest.trim().to_string(), Vec::new());
    }
    if let Some(rest) = s.strip_prefix('^') {
        return (TokKind::OpenNegated, rest.trim().to_string(), Vec::new());
    }
    // rsplit(':'): key = last segment; filters right-to-left = innermost first.
    let mut parts: Vec<String> = s.split(':').map(|p| p.trim().to_string()).collect();
    let key = parts.pop().unwrap_or_default();
    parts.reverse();
    (TokKind::Replacement, key, parts)
}

fn char_offset(src: &str, byte: usize) -> usize {
    src[..byte.min(src.len())].chars().count()
}

/// The legacy `{{=<% %>=}}` switch after optional leading whitespace: returns (whitespace, total length).
fn legacy_delims(src: &str) -> Option<(usize, usize)> {
    let ws_len = src.len() - src.trim_start().len();
    let rest = &src[ws_len..];
    rest.starts_with("{{=<% %>=}}").then_some((ws_len, ws_len + "{{=<% %>=}}".len()))
}

fn tokenize(src: &str, errors: &mut Vec<TemplateError>) -> Vec<Token> {
    let mut open = "{{";
    let mut close = "}}";
    let mut i = 0;
    let mut tokens: Vec<Token> = Vec::new();
    if let Some((ws, total)) = legacy_delims(src) {
        open = "<%";
        close = "%>";
        if ws > 0 {
            tokens.push(Token {
                kind: TokKind::Text,
                text: src[..ws].to_string(),
                key: String::new(),
                filters: Vec::new(),
                start: 0,
                end: ws,
            });
        }
        i = total;
    }
    fn push_text(tokens: &mut Vec<Token>, text: &str, start: usize) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = tokens.last_mut() {
            if last.kind == TokKind::Text && last.end == start {
                last.text.push_str(text);
                last.end = start + text.len();
                return;
            }
        }
        tokens.push(Token {
            kind: TokKind::Text,
            text: text.to_string(),
            key: String::new(),
            filters: Vec::new(),
            start,
            end: start + text.len(),
        });
    }
    while i < src.len() {
        let nh = src[i..].find(open).map_or(usize::MAX, |p| p + i);
        let nc = src[i..].find("<!--").map_or(usize::MAX, |p| p + i);
        if nh == usize::MAX && nc == usize::MAX {
            push_text(&mut tokens, &src[i..], i);
            break;
        }
        if nc < nh {
            push_text(&mut tokens, &src[i..nc], i);
            let stop = src[nc + 4..].find("-->").map(|p| nc + 4 + p + 3).unwrap_or(src.len());
            tokens.push(Token {
                kind: TokKind::Comment,
                text: src[nc..stop].to_string(),
                key: String::new(),
                filters: Vec::new(),
                start: nc,
                end: stop,
            });
            i = stop;
            continue;
        }
        push_text(&mut tokens, &src[i..nh], i);
        let Some(end_h) = src[nh + open.len()..].find(close).map(|p| p + nh + open.len()) else {
            errors.push(TemplateError {
                code: TemplateErrorCode::NoClosingBrackets,
                severity: Severity::Error,
                start: nh,
                end: src.len(),
                key: None,
                message: format!("Found '{open}' without a matching '{close}'."),
            });
            push_text(&mut tokens, &src[nh..], nh);
            break;
        };
        let body = &src[nh + open.len()..end_h];
        let stop = end_h + close.len();
        let (kind, key, filters) = classify_handle(body);
        tokens.push(Token { kind, text: src[nh..stop].to_string(), key, filters, start: nh, end: stop });
        i = stop;
    }
    tokens
}

/// Parses a template. Never fails; structural problems are reported in `errors`.
pub fn parse_template(src: &str) -> ParsedTemplate {
    let mut byte_errors: Vec<TemplateError> = Vec::new();
    let tokens = tokenize(src, &mut byte_errors);
    // Build the tree over a flat arena of frames: each open frame owns its children until closed.
    struct Frame {
        key: String,
        negated: bool,
        start: usize,
        children: Vec<TemplateNode>,
    }
    let mut root: Vec<TemplateNode> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    fn out<'a>(stack: &'a mut [Frame], root: &'a mut Vec<TemplateNode>) -> &'a mut Vec<TemplateNode> {
        match stack.last_mut() {
            Some(f) => &mut f.children,
            None => root,
        }
    }
    for tok in tokens {
        match tok.kind {
            TokKind::Text => {
                out(&mut stack, &mut root).push(TemplateNode::Text { text: tok.text, start: tok.start, end: tok.end })
            }
            TokKind::Comment => {
                out(&mut stack, &mut root).push(TemplateNode::Comment { text: tok.text, start: tok.start, end: tok.end })
            }
            TokKind::Replacement => out(&mut stack, &mut root).push(TemplateNode::Replacement {
                key: tok.key,
                filters: tok.filters,
                start: tok.start,
                end: tok.end,
            }),
            TokKind::Open | TokKind::OpenNegated => {
                stack.push(Frame {
                    key: tok.key,
                    negated: tok.kind == TokKind::OpenNegated,
                    start: tok.start,
                    children: Vec::new(),
                });
            }
            TokKind::Close => {
                let Some(top) = stack.last() else {
                    byte_errors.push(TemplateError {
                        code: TemplateErrorCode::UnexpectedClose,
                        severity: Severity::Error,
                        start: tok.start,
                        end: tok.end,
                        key: Some(tok.key.clone()),
                        message: format!("Found {{{{/{0}}}}}, but there is no matching {{{{#{0}}}}}.", tok.key),
                    });
                    out(&mut stack, &mut root).push(TemplateNode::Text { text: tok.text, start: tok.start, end: tok.end });
                    continue;
                };
                if top.key != tok.key {
                    byte_errors.push(TemplateError {
                        code: TemplateErrorCode::MismatchedClose,
                        severity: Severity::Error,
                        start: tok.start,
                        end: tok.end,
                        key: Some(tok.key.clone()),
                        message: format!("Found {{{{/{}}}}}, but expected {{{{/{}}}}}.", tok.key, top.key),
                    });
                    // Anki aborts; for a usable preview we close the innermost section anyway.
                }
                let frame =
                    stack.pop().unwrap_or_else(|| Frame { key: String::new(), negated: false, start: 0, children: Vec::new() });
                out(&mut stack, &mut root).push(TemplateNode::Conditional {
                    key: frame.key,
                    negated: frame.negated,
                    children: frame.children,
                    start: frame.start,
                    end: tok.end,
                });
            }
        }
    }
    // Unclosed sections are reported innermost first and kept as sections running to the end of
    // the source, so previews still render.
    while let Some(frame) = stack.pop() {
        byte_errors.push(TemplateError {
            code: TemplateErrorCode::UnclosedSection,
            severity: Severity::Error,
            start: frame.start,
            end: src.len(),
            key: Some(frame.key.clone()),
            message: format!(
                "Missing {{{{/{0}}}}}: the section {{{{{1}{0}}}}} is never closed.",
                frame.key,
                if frame.negated { '^' } else { '#' }
            ),
        });
        out(&mut stack, &mut root).push(TemplateNode::Conditional {
            key: frame.key,
            negated: frame.negated,
            children: frame.children,
            start: frame.start,
            end: src.len(),
        });
    }
    // Byte offsets → char offsets.
    fn convert(nodes: &mut [TemplateNode], src: &str) {
        for n in nodes {
            match n {
                TemplateNode::Text { start, end, .. }
                | TemplateNode::Comment { start, end, .. }
                | TemplateNode::Replacement { start, end, .. } => {
                    *start = char_offset(src, *start);
                    *end = char_offset(src, *end);
                }
                TemplateNode::Conditional { start, end, children, .. } => {
                    *start = char_offset(src, *start);
                    *end = char_offset(src, *end);
                    convert(children, src);
                }
            }
        }
    }
    convert(&mut root, src);
    for e in &mut byte_errors {
        e.start = char_offset(src, e.start);
        e.end = char_offset(src, e.end);
    }
    ParsedTemplate { nodes: root, errors: byte_errors }
}

/// True when a key is an always-allowed cloze conditional such as `c2`.
pub fn is_cloze_conditional_key(key: &str) -> bool {
    let rest = match key.strip_prefix('c') {
        Some(r) => r,
        None => return false,
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

/// Field-name checks Anki performs when saving a note type: unknown replacements and
/// unknown conditionals (except `c<digits>`), plus `tts` warnings.
pub fn check_field_references(parsed: &ParsedTemplate, field_names: &[String]) -> Vec<TemplateError> {
    let mut known: HashSet<&str> = field_names.iter().map(String::as_str).collect();
    known.extend(SPECIAL_FIELDS.iter().copied());
    let mut out = Vec::new();
    fn walk(nodes: &[TemplateNode], known: &HashSet<&str>, out: &mut Vec<TemplateError>) {
        for n in nodes {
            match n {
                TemplateNode::Replacement { key, filters, start, end } => {
                    if filters.iter().any(|f| f == "tts" || f.starts_with("tts ")) {
                        let mut rev = filters.clone();
                        rev.reverse();
                        out.push(TemplateError {
                            code: TemplateErrorCode::TtsUnsupported,
                            severity: Severity::Warning,
                            start: *start,
                            end: *end,
                            key: Some(key.clone()),
                            message: format!(
                                "{{{{{}:{}}}}} uses text-to-speech, which Loci does not play; it renders nothing.",
                                rev.join(":"),
                                key
                            ),
                        });
                    }
                    // `{{tts en_US:}}` (empty key with filters) is allowed by Anki.
                    if key.is_empty() && !filters.is_empty() {
                        continue;
                    }
                    if !known.contains(key.as_str()) {
                        out.push(TemplateError {
                            code: TemplateErrorCode::UnknownField,
                            severity: Severity::Error,
                            start: *start,
                            end: *end,
                            key: Some(key.clone()),
                            message: format!("Found '{{{{{0}}}}}', but there is no field called '{0}'.", key),
                        });
                    }
                }
                TemplateNode::Conditional { key, negated, children, start, end } => {
                    if !known.contains(key.as_str()) && !is_cloze_conditional_key(key) {
                        out.push(TemplateError {
                            code: TemplateErrorCode::UnknownField,
                            severity: Severity::Error,
                            start: *start,
                            end: *end,
                            key: Some(key.clone()),
                            message: format!(
                                "Found '{{{{{}{}}}}}', but there is no field called '{}'.",
                                if *negated { '^' } else { '#' },
                                key,
                                key
                            ),
                        });
                    }
                    walk(children, known, out);
                }
                _ => {}
            }
        }
    }
    walk(&parsed.nodes, &known, &mut out);
    out
}

/// Every field key referenced by replacements and conditionals (deduplicated, in order of appearance).
pub fn referenced_keys(nodes: &[TemplateNode]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    fn walk(nodes: &[TemplateNode], seen: &mut Vec<String>) {
        for n in nodes {
            if let Some(k) = n.key() {
                if !seen.iter().any(|s| s == k) {
                    seen.push(k.to_string());
                }
            }
            if let TemplateNode::Conditional { children, .. } = n {
                walk(children, seen);
            }
        }
    }
    walk(nodes, &mut seen);
    seen
}

/// True when the tree contains a `{{cloze:…}}` (or `cloze-only`/`type:cloze`) replacement.
pub fn has_cloze_replacement(nodes: &[TemplateNode]) -> bool {
    nodes.iter().any(|n| match n {
        TemplateNode::Replacement { filters, .. } => {
            filters.iter().any(|f| f == "cloze" || f == "cloze-only" || f == "type-cloze")
        }
        TemplateNode::Conditional { children, .. } => has_cloze_replacement(children),
        _ => false,
    })
}

const CACHE_LIMIT: usize = 512;
static PARSE_CACHE: Mutex<Option<HashMap<String, Arc<ParsedTemplate>>>> = Mutex::new(None);

/// Memoised `parse_template` for hot paths (card generation during import, rendering).
pub fn parse_template_cached(src: &str) -> Arc<ParsedTemplate> {
    let mut guard = match PARSE_CACHE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(hit) = cache.get(src) {
        return Arc::clone(hit);
    }
    let parsed = Arc::new(parse_template(src));
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(src.to_string(), Arc::clone(&parsed));
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(nodes: &[TemplateNode]) -> Vec<&'static str> {
        nodes
            .iter()
            .map(|n| match n {
                TemplateNode::Text { .. } => "text",
                TemplateNode::Comment { .. } => "comment",
                TemplateNode::Replacement { .. } => "replacement",
                TemplateNode::Conditional { .. } => "conditional",
            })
            .collect()
    }

    #[test]
    fn parses_text_comments_replacements_and_both_conditional_kinds() {
        let src = "a<!-- {{X}} -->{{Front}}{{#Back}}b{{/Back}}{{^Back}}c{{/Back}}";
        let ParsedTemplate { nodes, errors } = parse_template(src);
        assert!(errors.is_empty());
        assert_eq!(kinds(&nodes), ["text", "comment", "replacement", "conditional", "conditional"]);
        assert_eq!(nodes[1], TemplateNode::Comment { text: "<!-- {{X}} -->".into(), start: 1, end: 15 });
        assert_eq!(nodes[2], TemplateNode::Replacement { key: "Front".into(), filters: vec![], start: 15, end: 24 });
        match &nodes[3] {
            TemplateNode::Conditional { key, negated, children, .. } => {
                assert_eq!(key, "Back");
                assert!(!negated);
                assert_eq!(children, &[TemplateNode::Text { text: "b".into(), start: 33, end: 34 }]);
            }
            other => panic!("{other:?}"),
        }
        match &nodes[4] {
            TemplateNode::Conditional { key, negated, .. } => {
                assert_eq!(key, "Back");
                assert!(negated);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn splits_filter_chains_with_rsplit_semantics_and_trims() {
        let nodes = parse_template("{{ text : cloze : Text }}{{type:cloze:Text}}{{hint:Extra}}").nodes;
        let kf = |n: &TemplateNode| match n {
            TemplateNode::Replacement { key, filters, .. } => (key.clone(), filters.clone()),
            _ => panic!(),
        };
        assert_eq!(kf(&nodes[0]), ("Text".into(), vec!["cloze".into(), "text".into()]));
        assert_eq!(kf(&nodes[1]), ("Text".into(), vec!["cloze".into(), "type".into()]));
        assert_eq!(kf(&nodes[2]), ("Extra".into(), vec!["hint".into()]));
    }

    #[test]
    fn strips_leading_braces_and_treats_short_handles_as_replacements() {
        let nodes = parse_template("{{{Front}}}{{#}}").nodes;
        assert_eq!(nodes[0].key(), Some("Front"));
        assert!(matches!(&nodes[0], TemplateNode::Replacement { .. }));
        assert_eq!(nodes[1], TemplateNode::Text { text: "}".into(), start: 10, end: 11 });
        assert!(matches!(&nodes[2], TemplateNode::Replacement { key, .. } if key == "#"));
    }

    #[test]
    fn nests_conditionals() {
        let ParsedTemplate { nodes, errors } = parse_template("{{#A}}{{#B}}x{{/B}}{{/A}}");
        assert!(errors.is_empty());
        match &nodes[0] {
            TemplateNode::Conditional { key, children, .. } => {
                assert_eq!(key, "A");
                assert!(matches!(&children[0], TemplateNode::Conditional { key, .. } if key == "B"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn supports_the_legacy_delimiters() {
        let ParsedTemplate { nodes, errors } = parse_template("{{=<% %>=}}<%Front%> {{literal}}");
        assert!(errors.is_empty());
        assert_eq!(nodes.len(), 2);
        assert!(matches!(&nodes[0], TemplateNode::Replacement { key, .. } if key == "Front"));
        assert!(matches!(&nodes[1], TemplateNode::Text { text, .. } if text == " {{literal}}"));
    }

    #[test]
    fn reports_unclosed_sections_with_the_opening_position() {
        let src = "x{{#Back}}y";
        let errors = parse_template(src).errors;
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, TemplateErrorCode::UnclosedSection);
        assert_eq!(errors[0].key.as_deref(), Some("Back"));
        assert_eq!((errors[0].start, errors[0].end), (1, src.len()));
        assert_eq!(errors[0].message, "Missing {{/Back}}: the section {{#Back}} is never closed.");
    }

    #[test]
    fn reports_mismatched_and_unexpected_closes() {
        let e = &parse_template("{{#A}}x{{/B}}").errors[0];
        assert_eq!(e.code, TemplateErrorCode::MismatchedClose);
        assert_eq!((e.start, e.end), (7, 13));
        assert_eq!(e.message, "Found {{/B}}, but expected {{/A}}.");
        let e = &parse_template("x{{/B}}").errors[0];
        assert_eq!(e.code, TemplateErrorCode::UnexpectedClose);
        assert_eq!((e.start, e.end), (1, 7));
        assert_eq!(e.message, "Found {{/B}}, but there is no matching {{#B}}.");
    }

    #[test]
    fn reports_a_handlebar_without_closing_brackets() {
        let e = &parse_template("ab{{Front").errors[0];
        assert_eq!(e.code, TemplateErrorCode::NoClosingBrackets);
        assert_eq!(e.start, 2);
        assert_eq!(e.message, "Found '{{' without a matching '}}'.");
    }

    #[test]
    fn flags_unknown_fields_but_allows_special_fields_and_cloze_conditionals() {
        let parsed = parse_template("{{Front}}{{Nope}}{{Tags}}{{Deck}}{{#c2}}x{{/c2}}{{#Missing}}y{{/Missing}}{{tts en_US:}}");
        let all = check_field_references(&parsed, &["Front".to_string()]);
        let errs: Vec<&TemplateError> = all.iter().filter(|e| e.severity == Severity::Error).collect();
        assert_eq!(errs.iter().map(|e| e.key.as_deref().unwrap_or("")).collect::<Vec<_>>(), ["Nope", "Missing"]);
        assert_eq!(
            all.iter().filter(|e| e.severity == Severity::Warning).map(|e| e.code).collect::<Vec<_>>(),
            [TemplateErrorCode::TtsUnsupported]
        );
        assert_eq!(errs[0].code, TemplateErrorCode::UnknownField);
        assert_eq!((errs[0].start, errs[0].end), (9, 17));
        assert_eq!(errs[0].message, "Found '{{Nope}}', but there is no field called 'Nope'.");
        assert_eq!(errs[1].message, "Found '{{#Missing}}', but there is no field called 'Missing'.");
    }

    #[test]
    fn warns_about_tts() {
        let errs = check_field_references(&parse_template("{{tts en_US voices=Apple:Front}}"), &["Front".to_string()]);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, TemplateErrorCode::TtsUnsupported);
        assert_eq!(errs[0].severity, Severity::Warning);
        assert_eq!(
            errs[0].message,
            "{{tts en_US voices=Apple:Front}} uses text-to-speech, which Loci does not play; it renders nothing."
        );
    }

    #[test]
    fn lists_referenced_keys() {
        assert_eq!(referenced_keys(&parse_template("{{#A}}{{B}}{{/A}}{{text:C}}").nodes), ["A", "B", "C"]);
    }

    #[test]
    fn char_offsets_for_non_ascii_sources() {
        let parsed = parse_template("é{{Front}}");
        assert_eq!(parsed.nodes[1], TemplateNode::Replacement { key: "Front".into(), filters: vec![], start: 1, end: 10 });
        assert!(has_cloze_replacement(&parse_template("{{#A}}{{cloze:T}}{{/A}}").nodes));
        assert!(!has_cloze_replacement(&parse_template("{{T}}").nodes));
        assert!(is_cloze_conditional_key("c12") && !is_cloze_conditional_key("c") && !is_cloze_conditional_key("cx"));
        let a = parse_template_cached("{{X}}");
        let b = parse_template_cached("{{X}}");
        assert!(Arc::ptr_eq(&a, &b));
    }
}
