//! English sentences the core produces.
//!
//! The UI keeps its own full dictionary; the core only embeds the keys it needs
//! (`en-core.json`: template errors, search descriptions, insights, undo labels, …). Messages
//! are ICU-lite: `{name}` interpolates a variable, `{n, plural, =0 {none} one {# card} other
//! {# cards}}` picks a branch (exact `=n` first, then the English category, then `other`; `#` is
//! the formatted number) and `{kind, select, a {…} other {…}}` switches on a string. Branches
//! nest. Legacy `key_one` / `key_other` suffix plurals are honoured when `vars` has a numeric
//! `count`. Unknown keys return the key itself, so a sentence is never lost.

use std::collections::HashMap;
use std::sync::LazyLock;

/// The embedded English dictionary (flat dotted keys).
const EN_CORE_JSON: &str = include_str!("en-core.json");

/// A message variable: text or a number (numbers are formatted with thousands separators).
#[derive(Debug, Clone, PartialEq)]
pub enum Var {
    Str(String),
    Num(f64),
}

impl From<&str> for Var {
    fn from(s: &str) -> Self {
        Var::Str(s.to_string())
    }
}
impl From<String> for Var {
    fn from(s: String) -> Self {
        Var::Str(s)
    }
}
impl From<&String> for Var {
    fn from(s: &String) -> Self {
        Var::Str(s.clone())
    }
}
impl From<i64> for Var {
    fn from(n: i64) -> Self {
        Var::Num(n as f64)
    }
}
impl From<i32> for Var {
    fn from(n: i32) -> Self {
        Var::Num(n as f64)
    }
}
impl From<usize> for Var {
    fn from(n: usize) -> Self {
        Var::Num(n as f64)
    }
}
impl From<f64> for Var {
    fn from(n: f64) -> Self {
        Var::Num(n)
    }
}

/// Message variables: `&[("count", 3.into()), ("deck", "French".into())]`.
pub type Vars<'a> = [(&'a str, Var)];

/// A flat dictionary of dotted keys → ICU-lite messages.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    entries: HashMap<String, String>,
}

impl Dictionary {
    /// Builds a dictionary from JSON, nested (`{"study": {"x": "…"}}`) or flat (`{"study.x": "…"}`).
    pub fn from_json(json: &str) -> Result<Dictionary, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        let mut entries = HashMap::new();
        flatten(&value, "", &mut entries);
        Ok(Dictionary { entries })
    }

    pub fn from_entries<I: IntoIterator<Item = (String, String)>>(it: I) -> Dictionary {
        Dictionary { entries: it.into_iter().collect() }
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `t()` against this dictionary.
    pub fn t(&self, key: &str, vars: &Vars) -> String {
        let mut s: Option<&str> = None;
        if let Some(Var::Num(count)) = var(vars, "count") {
            // Legacy suffix plurals: `key_one` / `key_other` chosen by vars.count.
            s = self.get(&format!("{key}_{}", plural_category(*count)));
            if s.is_none() && self.get(&format!("{key}_one")).is_some() {
                s = self.get(&format!("{key}_other"));
            }
        }
        let s = match s.or_else(|| self.get(key)) {
            Some(s) => s,
            None => return key.to_string(),
        };
        if !vars.is_empty() || s.contains('{') {
            format_message(s, vars)
        } else {
            s.to_string()
        }
    }
}

fn flatten(v: &serde_json::Value, prefix: &str, out: &mut HashMap<String, String>) {
    if let serde_json::Value::Object(map) = v {
        for (k, child) in map {
            let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            match child {
                serde_json::Value::String(s) => {
                    out.insert(key, s.clone());
                }
                serde_json::Value::Object(_) => flatten(child, &key, out),
                _ => {}
            }
        }
    }
}

static EN: LazyLock<Dictionary> = LazyLock::new(|| {
    // The embedded file is generated from the frontend's en.json; a parse failure is a build defect,
    // and an empty dictionary (keys echoed back) is the safest behaviour at runtime.
    Dictionary::from_json(EN_CORE_JSON).unwrap_or_default()
});

/// The core's English dictionary.
pub fn dictionary() -> &'static Dictionary {
    &EN
}

/// Translates `key` with `vars`. Unknown keys return the key.
pub fn t(key: &str, vars: &Vars) -> String {
    EN.t(key, vars)
}

/// True when the core dictionary has `key`.
pub fn has_key(key: &str) -> bool {
    EN.has_key(key)
}

/// The dictionary entry when `key` exists, else `fallback` formatted
/// with `vars`, so a new sentence works before it is added to the dictionary.
pub fn msg(key: &str, fallback: &str, vars: &Vars) -> String {
    if has_key(key) {
        t(key, vars)
    } else {
        format_message(fallback, vars)
    }
}

/// 0.873 → "87%".
pub fn pct(fraction: f64) -> String {
    format!("{}%", (fraction * 100.0).round() as i64)
}

fn var<'a>(vars: &'a Vars, name: &str) -> Option<&'a Var> {
    vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
}

/// English CLDR cardinal category: `one` for exactly 1, else `other`.
fn plural_category(n: f64) -> &'static str {
    if n == 1.0 {
        "one"
    } else {
        "other"
    }
}

/// English CLDR ordinal category (1st, 2nd, 3rd, 4th; 11th–13th are `other`).
fn ordinal_category(n: f64) -> &'static str {
    if n.fract() != 0.0 || n < 0.0 {
        return "other";
    }
    let n = n as i64;
    match (n % 10, n % 100) {
        (1, r) if r != 11 => "one",
        (2, r) if r != 12 => "two",
        (3, r) if r != 13 => "few",
        _ => "other",
    }
}

/// Formats a number like `Intl.NumberFormat('en')`: thousands separators, at most three
/// fraction digits, no trailing zeros.
pub fn format_number(n: f64) -> String {
    if !n.is_finite() {
        return if n.is_nan() {
            "NaN".to_string()
        } else if n > 0.0 {
            "∞".to_string()
        } else {
            "-∞".to_string()
        };
    }
    let rounded = (n * 1000.0).round() / 1000.0;
    let negative = rounded < 0.0;
    let abs = rounded.abs();
    let int_part = abs.trunc();
    let mut frac = format!("{:.3}", abs - int_part);
    // "0.123" → "123", trimmed of trailing zeros.
    let frac_digits = frac.split_off(2);
    let frac_digits = frac_digits.trim_end_matches('0');
    let int_str = format!("{}", int_part as u64);
    let mut grouped = String::new();
    for (i, ch) in int_str.chars().enumerate() {
        if i > 0 && (int_str.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let mut out = String::new();
    if negative && (int_part != 0.0 || !frac_digits.is_empty()) {
        out.push('-');
    }
    out.push_str(&grouped);
    if !frac_digits.is_empty() {
        out.push('.');
        out.push_str(frac_digits);
    }
    out
}

/// Index of the `}` matching the `{` at byte `open`.
fn matching(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parses `sel {msg} sel2 {msg2}` into pairs.
fn parse_branches(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && s[i..].starts_with(|c: char| c.is_whitespace()) {
            i += s[i..].chars().next().map_or(1, char::len_utf8);
        }
        let Some(b_rel) = s[i..].find('{') else { break };
        let b = i + b_rel;
        let sel = s[i..b].trim().to_string();
        let Some(e) = matching(s, b) else { break };
        out.push((sel, s[b + 1..e].to_string()));
        i = e + 1;
    }
    out
}

fn var_display(v: &Var) -> String {
    match v {
        Var::Str(s) => s.clone(),
        Var::Num(n) => format_number(*n),
    }
}

fn var_number(v: Option<&Var>) -> Option<f64> {
    match v {
        Some(Var::Num(n)) => Some(*n),
        Some(Var::Str(s)) => s.trim().parse::<f64>().ok(),
        None => None,
    }
}

/// Formats an ICU-lite message with `vars`.
pub fn format_message(msg: &str, vars: &Vars) -> String {
    format_message_hash(msg, vars, None)
}

/// `hash` is the value substituted for `#` inside plural branches.
fn format_message_hash(msg: &str, vars: &Vars, hash: Option<&str>) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < msg.len() {
        let ch = msg[i..].chars().next().unwrap_or('\0');
        if ch == '#' {
            if let Some(h) = hash {
                out.push_str(h);
                i += 1;
                continue;
            }
        }
        if ch != '{' {
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let Some(end) = matching(msg, i) else {
            out.push_str(&msg[i..]);
            break;
        };
        let body = &msg[i + 1..end];
        match body.find(',') {
            None => {
                let name = body.trim();
                match var(vars, name) {
                    Some(v) => out.push_str(&var_display(v)),
                    None => {
                        out.push('{');
                        out.push_str(body);
                        out.push('}');
                    }
                }
            }
            Some(c1) => {
                let name = body[..c1].trim();
                let rest = &body[c1 + 1..];
                let (kind, branch_src) = match rest.find(',') {
                    Some(c2) => (rest[..c2].trim(), &rest[c2 + 1..]),
                    None => (rest.trim(), ""),
                };
                let branches = parse_branches(branch_src);
                let pick = |sel: &str| branches.iter().find(|(s, _)| s == sel).map(|(_, m)| m.as_str());
                let v = var(vars, name);
                match kind {
                    "plural" | "selectordinal" => {
                        let n = var_number(v);
                        let chosen = match n {
                            Some(n) => pick(&format!("={}", js_number(n)))
                                .or_else(|| pick(if kind == "selectordinal" { ordinal_category(n) } else { plural_category(n) }))
                                .or_else(|| pick("other"))
                                .unwrap_or(""),
                            None => pick("other").unwrap_or(""),
                        };
                        let hash_value = match n {
                            Some(n) => format_number(n),
                            None => v.map(var_display).unwrap_or_default(),
                        };
                        out.push_str(&format_message_hash(chosen, vars, Some(&hash_value)));
                    }
                    "select" => {
                        let key = v.map(var_display).unwrap_or_default();
                        let chosen = pick(&key).or_else(|| pick("other")).unwrap_or("");
                        out.push_str(&format_message_hash(chosen, vars, hash));
                    }
                    "number" => match v {
                        Some(Var::Num(n)) => out.push_str(&format_number(*n)),
                        Some(Var::Str(s)) => out.push_str(s),
                        None => {}
                    },
                    _ => out.push_str(&v.map(var_display).unwrap_or_default()),
                }
            }
        }
        i = end + 1;
    }
    out
}

/// A number as JavaScript's `String(n)` prints it for the common cases (`=5` branch names).
fn js_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_keys_and_interpolates() {
        assert_eq!(t("study.playAudio", &[]), "Play audio");
        assert_eq!(t("template.missingTemplate", &[("ord", 3.into())]), "Card type 3 no longer exists in this note type.");
        assert_eq!(t("no.such.key", &[]), "no.such.key");
        assert!(has_key("browse.describe.all"));
        assert!(!has_key("study.showAnswer"));
    }

    #[test]
    fn icu_plurals_with_exact_matches_and_hash() {
        let m = "{count, plural, =0 {Nothing due today} one {# card due today} other {# cards due today}}";
        assert_eq!(format_message(m, &[("count", 0.into())]), "Nothing due today");
        assert_eq!(format_message(m, &[("count", 1.into())]), "1 card due today");
        assert_eq!(format_message(m, &[("count", 1234.into())]), "1,234 cards due today");
        assert_eq!(t("browse.describe.rated", &[("days", 1.into())]), "answered today");
        assert_eq!(t("browse.describe.rated", &[("days", 7.into())]), "answered in the last 7 days");
    }

    #[test]
    fn select_and_nesting() {
        assert_eq!(
            format_message(
                "{g, select, a {A{n, plural, one {# x} other {# xs}}} other {O}}",
                &[("g", "a".into()), ("n", 2.into())]
            ),
            "A2 xs"
        );
        assert_eq!(format_message("{g, select, a {A} other {O}}", &[("g", "z".into())]), "O");
    }

    #[test]
    fn unknown_variables_are_left_as_written() {
        assert_eq!(format_message("x {missing} y", &[]), "x {missing} y");
        assert_eq!(format_message("unclosed {a", &[("a", 1.into())]), "unclosed {a");
    }

    #[test]
    fn legacy_suffix_plurals() {
        let d = Dictionary::from_entries([
            ("legacy_one".to_string(), "{count} item".to_string()),
            ("legacy_other".to_string(), "{count} items".to_string()),
        ]);
        assert_eq!(d.t("legacy", &[("count", 1.into())]), "1 item");
        assert_eq!(d.t("legacy", &[("count", 3.into())]), "3 items");
    }

    #[test]
    fn msg_falls_back_to_the_given_sentence() {
        assert_eq!(msg("insights.optimiseAction", "nope", &[]), "Optimise");
        assert_eq!(msg("insights.notAKey", "{n, plural, one {# card} other {# cards}}", &[("n", 2.into())]), "2 cards");
        assert_eq!(pct(0.873), "87%");
    }

    #[test]
    fn formats_numbers_like_intl_en() {
        assert_eq!(format_number(1234.5), "1,234.5");
        assert_eq!(format_number(1234567.0), "1,234,567");
        assert_eq!(format_number(0.12345), "0.123");
        assert_eq!(format_number(-42.0), "-42");
        assert_eq!(format_number(999.0), "999");
    }

    #[test]
    fn nested_json_dictionaries_flatten() {
        let d = Dictionary::from_json(r#"{"a": {"b": "x", "c": {"d": "y"}}, "e.f": "z"}"#).unwrap();
        assert_eq!(d.get("a.b"), Some("x"));
        assert_eq!(d.get("a.c.d"), Some("y"));
        assert_eq!(d.get("e.f"), Some("z"));
        assert_eq!(dictionary().len(), 104);
    }
}
