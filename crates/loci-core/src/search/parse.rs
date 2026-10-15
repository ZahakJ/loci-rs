//! Anki search syntax → AST (https://docs.ankiweb.net/searching.html,
//! rslib/src/search/parser.rs).
//!
//! Terms are separated by whitespace (implicit AND); `and`/`or` (any case) are operators with
//! AND binding tighter than OR, as in Anki; `-` negates a term or group; parentheses group;
//! `"…"` quotes a term (also `key:"…"` and `"key:…"`). Text values keep Anki's escapes
//! (`\*`, `\_`, `\\`, `\"`, `\:`, `\(`, `\)`, `\-`) so wildcards can be told from literals later:
//! use `glob_to_like`, `glob_to_regex_source` and `unescape_text` to consume them.
//! Error positions are character offsets into the query.

pub use super::ast::{CompareOp, FieldMode, IdTarget, PropKind, PropValue, SearchError, SearchNode, StateKind};
use regex::Regex;
use std::sync::LazyLock;

// ───────────────────────── escapes ─────────────────────────

/// Removes Anki escapes, giving the literal text (wildcards become literal `*`/`_`).
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(n) => out.push(n),
                None => out.push('\\'), // a trailing backslash has nothing to escape (JS: left as is)
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Unescapes only `\"` (raw contexts such as `re:`).
pub fn unescape_quotes(s: &str) -> String {
    s.replace("\\\"", "\"")
}

/// True when an escaped glob contains an unescaped `*` or `_`.
pub fn has_wildcards(s: &str) -> bool {
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if c == '*' || c == '_' {
            return true;
        }
    }
    false
}

/// Escaped glob → SQL LIKE pattern (use with `ESCAPE '\'`).
pub fn glob_to_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some() {
            let n = chars.next().unwrap_or('\\');
            if n == '%' || n == '_' || n == '\\' {
                out.push('\\');
            }
            out.push(n);
            continue;
        }
        match c {
            '*' => out.push('%'),
            '_' => out.push('_'),
            '%' => out.push_str("\\%"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_regex_char(c: char, out: &mut String) {
    // escapes `.*+?^${}()|[]`; `/` and `-` need no escape in Rust regex syntax
    // (and `\/` is rejected by it).
    if ".*+?^${}()|[]\\".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// Escaped glob → regex source; `*` → `any*`, `_` → `any` (default `.`/`.*`).
pub fn glob_to_regex_source(s: &str, any: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some() {
            let n = chars.next().unwrap_or('\\');
            escape_regex_char(n, &mut out);
            continue;
        }
        match c {
            '*' => {
                out.push_str(any);
                out.push('*');
            }
            '_' => out.push_str(any),
            _ => escape_regex_char(c, &mut out),
        }
    }
    out
}

// ───────────────────────── lexer ─────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum TokKind {
    Open,
    Close,
    And,
    Or,
    Neg,
    Term(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Tok {
    kind: TokKind,
    start: usize,
    end: usize,
}

fn is_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{3000}'
}

fn lex(q: &[char]) -> Result<Vec<Tok>, SearchError> {
    let mut toks = Vec::new();
    let mut i = 0;
    while i < q.len() {
        let ch = q[i];
        if is_ws(ch) {
            i += 1;
            continue;
        }
        if ch == '(' {
            toks.push(Tok { kind: TokKind::Open, start: i, end: i + 1 });
            i += 1;
            continue;
        }
        if ch == ')' {
            toks.push(Tok { kind: TokKind::Close, start: i, end: i + 1 });
            i += 1;
            continue;
        }
        if ch == '-' && i + 1 < q.len() && !is_ws(q[i + 1]) {
            toks.push(Tok { kind: TokKind::Neg, start: i, end: i + 1 });
            i += 1;
            continue;
        }
        let start = i;
        let mut text = String::new();
        let mut quoted_whole = false;
        if ch == '"' {
            // fully quoted term
            i += 1;
            while i < q.len() && q[i] != '"' {
                if q[i] == '\\' && i + 1 < q.len() {
                    text.push(q[i]);
                    text.push(q[i + 1]);
                    i += 2;
                    continue;
                }
                text.push(q[i]);
                i += 1;
            }
            if i >= q.len() {
                return Err(SearchError::new("A quote (\") is not closed.", start, q.len()));
            }
            i += 1;
            quoted_whole = true;
        } else {
            while i < q.len() && !is_ws(q[i]) && q[i] != '(' && q[i] != ')' {
                if q[i] == '\\' && i + 1 < q.len() {
                    text.push(q[i]);
                    text.push(q[i + 1]);
                    i += 2;
                    continue;
                }
                if q[i] == '"' {
                    // key:"quoted value"
                    if !text.ends_with(':') {
                        return Err(SearchError::new(
                            "Unexpected quote (\") inside a term; quote the whole term instead.",
                            i,
                            i + 1,
                        ));
                    }
                    let qs = i;
                    i += 1;
                    while i < q.len() && q[i] != '"' {
                        if q[i] == '\\' && i + 1 < q.len() {
                            text.push(q[i]);
                            text.push(q[i + 1]);
                            i += 2;
                            continue;
                        }
                        text.push(q[i]);
                        i += 1;
                    }
                    if i >= q.len() {
                        return Err(SearchError::new("A quote (\") is not closed.", qs, q.len()));
                    }
                    i += 1;
                    break;
                }
                text.push(q[i]);
                i += 1;
            }
        }
        let lower = text.to_lowercase();
        if !quoted_whole && lower == "and" {
            toks.push(Tok { kind: TokKind::And, start, end: i });
        } else if !quoted_whole && lower == "or" {
            toks.push(Tok { kind: TokKind::Or, start, end: i });
        } else {
            toks.push(Tok { kind: TokKind::Term(text), start, end: i });
        }
    }
    Ok(toks)
}

// ───────────────────────── parser ─────────────────────────

struct Parser {
    toks: Vec<Tok>,
    p: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.p)
    }
    fn peek_kind(&self) -> Option<&TokKind> {
        self.peek().map(|t| &t.kind)
    }
    fn at_boundary(&self) -> bool {
        matches!(self.peek_kind(), None | Some(TokKind::Close) | Some(TokKind::Or) | Some(TokKind::And))
    }

    fn parse_or(&mut self) -> Result<SearchNode, SearchError> {
        let mut parts = vec![self.parse_and()?];
        while self.peek_kind() == Some(&TokKind::Or) {
            let (s, e) = self.peek().map(|t| (t.start, t.end)).unwrap_or((0, 0));
            self.p += 1;
            if self.at_boundary() {
                return Err(SearchError::new("\"or\" must be followed by a search term.", s, e));
            }
            parts.push(self.parse_and()?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { SearchNode::Or { children: parts } })
    }

    fn parse_and(&mut self) -> Result<SearchNode, SearchError> {
        let mut parts: Vec<SearchNode> = Vec::new();
        while let Some(tk) = self.peek() {
            if matches!(tk.kind, TokKind::Close | TokKind::Or) {
                break;
            }
            if tk.kind == TokKind::And {
                let (s, e) = (tk.start, tk.end);
                self.p += 1;
                if parts.is_empty() {
                    return Err(SearchError::new("\"and\" must come between two search terms.", s, e));
                }
                if self.at_boundary() {
                    return Err(SearchError::new("\"and\" must be followed by a search term.", s, e));
                }
                continue;
            }
            parts.push(self.parse_unary()?);
        }
        if parts.is_empty() {
            return Err(match self.peek() {
                Some(tk) => SearchError::new("Expected a search term here.", tk.start, tk.end),
                None => SearchError::new("The search ends unexpectedly.", self.len, self.len),
            });
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { SearchNode::And { children: parts } })
    }

    fn parse_unary(&mut self) -> Result<SearchNode, SearchError> {
        let tk = self.toks[self.p].clone();
        self.p += 1;
        match tk.kind {
            TokKind::Neg => {
                if self.at_boundary() {
                    return Err(SearchError::new("\"-\" must be followed by a search term.", tk.start, tk.end));
                }
                Ok(SearchNode::Not { child: Box::new(self.parse_unary()?) })
            }
            TokKind::Open => {
                if let Some(next) = self.peek() {
                    if next.kind == TokKind::Close {
                        return Err(SearchError::new("Empty parentheses.", tk.start, next.end));
                    }
                }
                let inner = self.parse_or()?;
                match self.peek() {
                    Some(cl) if cl.kind == TokKind::Close => {
                        self.p += 1;
                        Ok(inner)
                    }
                    _ => Err(SearchError::new("A parenthesis is not closed.", tk.start, self.len)),
                }
            }
            TokKind::Term(text) => parse_term(&text, tk.start, tk.end),
            _ => Err(SearchError::new("Unexpected \")\".", tk.start, tk.end)),
        }
    }
}

/// Parses a query. Empty input → `SearchNode::All`. Fails with `SearchError` and positions.
pub fn parse_search(query: &str) -> Result<SearchNode, SearchError> {
    let chars: Vec<char> = query.chars().collect();
    let toks = lex(&chars)?;
    if toks.is_empty() {
        return Ok(SearchNode::All);
    }
    let mut parser = Parser { toks, p: 0, len: chars.len() };
    let node = parser.parse_or()?;
    if let Some(tk) = parser.peek() {
        let msg = if tk.kind == TokKind::Close { "Unexpected \")\" without a matching \"(\"." } else { "Unexpected input." };
        return Err(SearchError::new(msg, tk.start, tk.end));
    }
    Ok(node)
}

/// Index of the first unescaped ':' or None.
fn first_colon(s: &str) -> Option<usize> {
    let mut skip = false;
    for (i, c) in s.char_indices() {
        if skip {
            skip = false;
            continue;
        }
        if c == '\\' {
            skip = true;
            continue;
        }
        if c == ':' {
            return Some(i);
        }
    }
    None
}

/// JavaScript `Number(s)` for the forms a search can contain: None for NaN.
pub(crate) fn js_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    match t {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|n| n as f64);
    }
    let lower = t.to_ascii_lowercase();
    if lower.contains("inf") || lower.contains("nan") {
        return None;
    }
    t.parse::<f64>().ok()
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}

static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").expect("valid regex"));
static ID_LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(,\d+)*$").expect("valid regex"));
static WORD_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_]+$").expect("valid regex"));

fn parse_term(text: &str, start: usize, end: usize) -> Result<SearchNode, SearchError> {
    let err = |msg: String| SearchError::new(msg, start, end);
    if text.is_empty() {
        return Ok(SearchNode::Text { text: String::new() });
    }
    let Some(colon) = first_colon(text) else {
        return Ok(SearchNode::Text { text: text.to_string() });
    };
    let raw_key = &text[..colon];
    let key = unescape_text(raw_key).to_lowercase();
    let val = &text[colon + 1..];
    let pos_int = |what: &str| -> Result<i64, SearchError> {
        let lit = unescape_text(val);
        match js_number(&lit) {
            Some(n) if is_integer(n) && n >= 1.0 => Ok(n as i64),
            _ => Err(err(format!("{what} needs a whole number of days, 1 or more (got \"{lit}\")."))),
        }
    };
    let id_list = || -> Result<Vec<i64>, SearchError> {
        let v = unescape_text(val);
        if !ID_LIST.is_match(&v) {
            return Err(err(format!("Expected comma-separated ids, got \"{v}\".")));
        }
        v.split(',').map(|p| p.parse::<i64>().map_err(|_| err(format!("Expected comma-separated ids, got \"{v}\".")))).collect()
    };
    match key.as_str() {
        "tag" => {
            if val.eq_ignore_ascii_case("none") {
                return Ok(SearchNode::TagNone);
            }
            if let Some(rest) = val.strip_prefix("re:") {
                return Ok(SearchNode::TagRegex { pattern: unescape_quotes(rest) });
            }
            if val.is_empty() {
                return Err(err("tag: needs a tag name.".into()));
            }
            Ok(SearchNode::Tag { tag: val.to_string() })
        }
        "deck" => {
            if val.is_empty() {
                return Err(err("deck: needs a deck name.".into()));
            }
            if unescape_text(val).to_lowercase() == "current" {
                return Ok(SearchNode::DeckCurrent);
            }
            Ok(SearchNode::Deck { name: val.to_string() })
        }
        "note" => {
            if val.is_empty() {
                return Err(err("note: needs a note type name.".into()));
            }
            Ok(SearchNode::Notetype { name: val.to_string() })
        }
        "card" => {
            let v = unescape_text(val);
            if DIGITS.is_match(&v) {
                let n = v.parse::<i64>().unwrap_or(i64::MAX);
                if n < 1 {
                    return Err(err("card: numbers start at 1.".into()));
                }
                return Ok(SearchNode::CardOrd { ord: n - 1 });
            }
            if val.is_empty() {
                return Err(err("card: needs a card type name or number.".into()));
            }
            Ok(SearchNode::CardName { name: val.to_string() })
        }
        "is" => {
            let v = unescape_text(val);
            match StateKind::parse(&v.to_lowercase()) {
                Some(state) => Ok(SearchNode::State { state }),
                None => Err(err(format!(
                    "Unknown state \"is:{v}\". Use due, new, learn, review, suspended, buried, buried-sibling or buried-manually."
                ))),
            }
        }
        "flag" => {
            let v = unescape_text(val);
            match v.as_str() {
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" => Ok(SearchNode::Flag { flag: v.parse().unwrap_or(0) }),
                _ => Err(err(format!("flag: takes a number from 0 to 7 (got \"{v}\")."))),
            }
        }
        "prop" => parse_prop(&unescape_text(val), start, end),
        "rated" => {
            let lit = unescape_text(val);
            let mut parts = lit.split(':');
            let d = parts.next().unwrap_or("");
            let e = parts.next();
            let rest = parts.count();
            let days = js_number(d);
            match days {
                Some(n) if is_integer(n) && n >= 1.0 && rest == 0 => {}
                _ => return Err(err(format!("rated: expects rated:days or rated:days:answer (got \"{lit}\")."))),
            }
            let days = days.unwrap_or(1.0) as i64;
            match e {
                Some(e) => match e {
                    "1" | "2" | "3" | "4" => Ok(SearchNode::Rated { days, ease: e.parse().ok() }),
                    _ => Err(err("The answer in rated:days:answer must be 1 (Again) to 4 (Easy).".into())),
                },
                None => Ok(SearchNode::Rated { days, ease: None }),
            }
        }
        "resched" => Ok(SearchNode::Resched { days: pos_int("resched:")? }),
        "introduced" => Ok(SearchNode::Introduced { days: pos_int("introduced:")? }),
        "added" => Ok(SearchNode::Added { days: pos_int("added:")? }),
        "edited" => Ok(SearchNode::Edited { days: pos_int("edited:")? }),
        "nid" => Ok(SearchNode::Ids { target: IdTarget::Nid, ids: id_list()? }),
        "cid" => Ok(SearchNode::Ids { target: IdTarget::Cid, ids: id_list()? }),
        "mid" => Ok(SearchNode::Ids { target: IdTarget::Mid, ids: id_list()? }),
        "did" => Ok(SearchNode::Ids { target: IdTarget::Did, ids: id_list()? }),
        "dupe" => {
            let Some(comma) = val.find(',') else { return Err(err("dupe: expects dupe:notetypeId,text.".into())) };
            match js_number(&val[..comma]) {
                Some(n) if is_integer(n) => Ok(SearchNode::Dupe { ntid: n as i64, text: unescape_text(&val[comma + 1..]) }),
                _ => Err(err("dupe: expects dupe:notetypeId,text.".into())),
            }
        }
        "has-cd" => {
            let k = unescape_text(val);
            if !WORD_KEY.is_match(&k) {
                return Err(err("has-cd: needs a custom data key (letters, digits, _).".into()));
            }
            Ok(SearchNode::HasCd { key: k })
        }
        "preset" => {
            if val.is_empty() {
                return Err(err("preset: needs a preset name.".into()));
            }
            Ok(SearchNode::Preset { name: val.to_string() })
        }
        "re" => Ok(SearchNode::Regex { pattern: unescape_quotes(val) }),
        "nc" => Ok(SearchNode::Nc { text: val.to_string() }),
        "w" => Ok(SearchNode::Word { text: val.to_string() }),
        "sc" => Ok(SearchNode::Sc { text: val.to_string() }),
        _ => {
            if raw_key.is_empty() {
                return Err(err("A search term cannot start with \":\".".into()));
            }
            if let Some(rest) = val.strip_prefix("re:") {
                return Ok(SearchNode::Field {
                    field: raw_key.to_string(),
                    value: unescape_quotes(rest),
                    mode: FieldMode::Regex,
                });
            }
            if let Some(rest) = val.strip_prefix("nc:") {
                return Ok(SearchNode::Field { field: raw_key.to_string(), value: rest.to_string(), mode: FieldMode::Nc });
            }
            Ok(SearchNode::Field { field: raw_key.to_string(), value: val.to_string(), mode: FieldMode::Glob })
        }
    }
}

static PROP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)^(ivl|due|reps|lapses|ease|pos|r|s|d|rated|resched|cdn:[A-Za-z0-9_]+|cds:[A-Za-z0-9_]+)(<=|>=|!=|=|<|>)(.*)$",
    )
    .expect("valid regex")
});

fn parse_prop(v: &str, start: usize, end: usize) -> Result<SearchNode, SearchError> {
    let err = |msg: String| SearchError::new(msg, start, end);
    let Some(m) = PROP_RE.captures(v) else {
        return Err(err(format!(
            "Unknown property \"prop:{v}\". Use ivl, due, reps, lapses, ease, pos, r, s, d, rated, resched, cdn:key or cds:key with < > <= >= = !=."
        )));
    };
    let name = m[1].to_lowercase();
    let Some(op) = CompareOp::parse(&m[2]) else { return Err(err(format!("Unknown property \"prop:{v}\"."))) };
    let raw = &m[3];
    if name.starts_with("cds:") {
        if op != CompareOp::Eq && op != CompareOp::Ne {
            return Err(err("prop:cds: compares text, so only = and != work.".into()));
        }
        return Ok(SearchNode::Prop {
            prop: PropKind::Cds,
            key: Some(m[1][4..].to_string()),
            op,
            value: PropValue::Text(raw.to_string()),
        });
    }
    let n = match js_number(raw) {
        Some(n) if !raw.trim().is_empty() && n.is_finite() => n,
        _ => return Err(err(format!("prop:{name} needs a number (got \"{raw}\")."))),
    };
    let key = |k: &str| Some(k.to_string());
    if name.starts_with("cdn:") {
        return Ok(SearchNode::Prop { prop: PropKind::Cdn, key: key(&m[1][4..]), op, value: PropValue::Num(n) });
    }
    let prop = match name.as_str() {
        "ivl" => PropKind::Ivl,
        "due" => PropKind::Due,
        "reps" => PropKind::Reps,
        "lapses" => PropKind::Lapses,
        "ease" => PropKind::Ease,
        "pos" => PropKind::Pos,
        "r" => PropKind::R,
        "s" => PropKind::S,
        "d" => PropKind::D,
        "rated" => PropKind::Rated,
        _ => PropKind::Resched,
    };
    let int_props =
        [PropKind::Ivl, PropKind::Due, PropKind::Reps, PropKind::Lapses, PropKind::Pos, PropKind::Rated, PropKind::Resched];
    if int_props.contains(&prop) && !is_integer(n) {
        return Err(err(format!("prop:{name} needs a whole number.")));
    }
    if (prop == PropKind::Rated || prop == PropKind::Resched) && n > 0.0 {
        return Err(err(format!("prop:{name} counts days back from today, so it must be 0 or negative.")));
    }
    if (prop == PropKind::R || prop == PropKind::D) && !(0.0..=1.0).contains(&n) {
        return Err(err(format!("prop:{name} is between 0 and 1.")));
    }
    Ok(SearchNode::Prop { prop, op, value: PropValue::Num(n), key: None })
}
