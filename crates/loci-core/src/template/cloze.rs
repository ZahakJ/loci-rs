//! Cloze deletions (recall `src/core/template/cloze.ts`).
//!
//! FOUNDATION STUB: only `cloze_to_text` exists so `text::plain_text` compiles. The template
//! agent replaces this file with the full port (parser, nested/overlapping clozes, hints,
//! one-by-one, image-occlusion detection) and keeps `cloze_to_text`'s signature.

use regex::Regex;
use std::sync::LazyLock;

static CLOZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)\{\{c(\d+)::(.*?)(?:::(.*?))?\}\}").unwrap());

/// Cloze markup rendered as its text (`{{c1::a::hint}}` → `a`); image-occlusion clozes become ''.
pub fn cloze_to_text(text: &str) -> String {
    if !text.contains("{{c") {
        return text.to_string();
    }
    let mut s = text.to_string();
    for _ in 0..8 {
        if !CLOZE.is_match(&s) {
            break;
        }
        s = CLOZE
            .replace_all(&s, |c: &regex::Captures| {
                let body = c.get(2).map(|m| m.as_str()).unwrap_or("");
                if body.starts_with("image-occlusion:") {
                    String::new()
                } else {
                    body.to_string()
                }
            })
            .into_owned();
    }
    s
}
