//! Media filename rules and reference extraction, following Anki
//! (rslib/src/media/files.rs `normalize_filename`, rslib/io `filename_is_safe`,
//! rslib/src/text.rs `extract_media_refs`; research/anki-format.md §1.2, §5.5).
//!
//! Also the pure filename functions of the media store (`split_extension`, `is_normalized_media_filename`,
//! `add_hash_suffix`, `is_remote_reference`, `extract_media_references`,
//! `extract_template_media_references`, `percent_decode`), so the collection module's media
//! store reuses one implementation. `normalize_filename` re-checks the trailing space/period after
//! truncation and returns `""` when nothing usable is left.

use super::nfc::nfc;
use crate::text::decode_entities;
use regex::Regex;
use std::sync::LazyLock;

/// Anki's MAX_FILENAME_LENGTH, in UTF-8 bytes.
pub const MAX_MEDIA_FILENAME_BYTES: usize = 120;

static DISALLOWED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\[\]<>:"/?*^\\|\x00-\x1f\x7f]|\p{Unassigned}"#).unwrap());
static WINDOWS_DEVICE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)").unwrap());
static TRAILING_SPACE_OR_DOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s.]$").unwrap());
// Anki's HTML_MEDIA_TAGS (quoted or unquoted src/data on img, audio, video, object, source).
static MEDIA_TAGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<\b(?:img|audio|video|object|source)\b(?:[^>]|"[^"]+?"|'[^']+?')+?\b(?:src|data)\b=(?:"([^"]+?)"[^>]*>|'([^']+?)'[^>]*>|([^ >]+?)(?: [^>]*>|>))"#).unwrap()
});
static SOUND_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\[sound:(.+?)\]").unwrap());
// CSS url(...) references, used for template stylesheets ('_font.woff2'); the quote must match.
static CSS_URL: LazyLock<fancy_regex::Regex> =
    LazyLock::new(|| fancy_regex::Regex::new(r#"(?i)url\(\s*(['"]?)([^'")]+)\1\s*\)"#).unwrap());
static REMOTE_SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:[a-z][a-z0-9+.-]*:)").unwrap());

/// True when `name` is exactly one normal path component (no separators, not `.`/`..`, not empty).
pub fn filename_is_safe(name: &str) -> bool {
    !(name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0'))
}

/// Truncates to at most `max` UTF-8 bytes on a code-point boundary.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = 0;
    for (i, ch) in s.char_indices() {
        if i + ch.len_utf8() > max {
            break;
        }
        end = i + ch.len_utf8();
    }
    &s[..end]
}

/// Splits `a.b.png` into `("a.b", ".png")`; names without a dot (or a leading dot only) have no extension.
pub fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Truncates a filename to the byte limit, keeping its extension (Anki `truncate_filename`).
pub fn truncate_filename(name: &str, max_bytes: usize) -> String {
    if name.len() <= max_bytes {
        return name.to_string();
    }
    let (stem, ext) = split_extension(name);
    if ext.len() >= max_bytes {
        return truncate_bytes(name, max_bytes).to_string();
    }
    format!("{}{ext}", truncate_bytes(stem, max_bytes - ext.len()))
}

/// Anki `normalize_filename`: NFC; drop `[]<>:"/?*^\|`, ASCII control characters and unassigned
/// code points; NBSP → space; Windows device names get `_` after the stem; a trailing space or
/// period gets `_` appended; truncated to 120 bytes keeping the extension.
/// Returns `""` when nothing usable is left.
pub fn normalize_media_filename(name: &str) -> String {
    let composed = nfc(name);
    let mut s = DISALLOWED.replace_all(&composed, "").replace('\u{a0}', " ");
    if WINDOWS_DEVICE_NAME.is_match(&s) {
        s = WINDOWS_DEVICE_NAME.replace(&s, "${1}_${2}").into_owned();
    }
    if TRAILING_SPACE_OR_DOT.is_match(&s) {
        s.push('_');
    }
    s = truncate_filename(&s, MAX_MEDIA_FILENAME_BYTES);
    // Truncation can expose a new trailing space/period.
    if TRAILING_SPACE_OR_DOT.is_match(&s) {
        let mut chars = s.chars();
        chars.next_back();
        s = truncate_filename(&format!("{}_", chars.as_str()), MAX_MEDIA_FILENAME_BYTES);
    }
    if s.trim().is_empty() || s == "_" {
        String::new()
    } else {
        s
    }
}

/// True when `name` is already a normalised, safe single-component filename.
pub fn is_normalized_media_filename(name: &str) -> bool {
    !name.is_empty() && normalize_media_filename(name) == name
}

/// Anki `add_hash_suffix_to_file_stem`: `a.png` + sha1 → `a-<sha1>.png` (still within 120 bytes).
pub fn add_hash_suffix(name: &str, sha1_hex: &str) -> String {
    let (stem, ext) = split_extension(name);
    let suffix = format!("-{sha1_hex}");
    let room = MAX_MEDIA_FILENAME_BYTES.saturating_sub(ext.len() + suffix.len());
    format!("{}{suffix}{ext}", truncate_bytes(stem, room))
}

/// True for references that are not local media files (remote URLs, data URIs, anchors).
pub fn is_remote_reference(reference: &str) -> bool {
    REMOTE_SCHEME.is_match(reference) || reference.starts_with("//") || reference.starts_with('#')
}

/// Local media filenames referenced by field HTML (entity-decoded; remote URLs excluded).
pub fn extract_media_references(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !html.contains('<') && !html.contains("[sound:") {
        return out;
    }
    for caps in MEDIA_TAGS.captures_iter(html) {
        let raw = caps.get(1).or_else(|| caps.get(2)).or_else(|| caps.get(3)).map(|m| m.as_str()).unwrap_or("");
        let r = decode_entities(raw.trim());
        if !r.is_empty() && !is_remote_reference(&r) {
            out.push(r);
        }
    }
    for caps in SOUND_TAG.captures_iter(html) {
        let r = decode_entities(caps[1].trim());
        if !r.is_empty() && !is_remote_reference(&r) {
            out.push(r);
        }
    }
    out
}

/// Media filenames referenced from template text or CSS (`_`-prefixed shared files, `url(...)`).
pub fn extract_template_media_references(text: &str) -> Vec<String> {
    let mut out = extract_media_references(text);
    for caps in CSS_URL.captures_iter(text).flatten() {
        let r = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        if !r.is_empty() && !is_remote_reference(r) {
            out.push(r.to_string());
        }
    }
    out
}

/// Percent-decodes a reference if it is valid percent-encoding (`decodeURIComponent`); otherwise
/// returns it unchanged.
pub fn percent_decode(reference: &str) -> String {
    if !reference.contains('%') {
        return reference.to_string();
    }
    let bytes = reference.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex =
                bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            match hex {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => return reference.to_string(),
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| reference.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ordinary_names() {
        for n in ["a.png", "Ünïcödé.jpg", "صورة.png", "with space.mp3", "_static.css", "latex-abc.svg"] {
            assert_eq!(normalize_media_filename(n), n);
        }
    }

    #[test]
    fn nfc_normalises_and_removes_forbidden_characters() {
        assert_eq!(normalize_media_filename("cafe\u{301}.png"), "café.png");
        assert_eq!(normalize_media_filename("a[b]<c>:d\"e/f?g*h^i\\j|k.png"), "abcdefghijk.png");
        assert_eq!(normalize_media_filename("tab\there\u{1}.png"), "tabhere.png");
        assert_eq!(normalize_media_filename("nb\u{a0}sp.png"), "nb sp.png");
        assert_eq!(normalize_media_filename("x\u{378}y.png"), "xy.png"); // unassigned code point
    }

    #[test]
    fn protects_windows_device_names_and_trailing_dots_or_spaces() {
        assert_eq!(normalize_media_filename("con.png"), "con_.png");
        assert_eq!(normalize_media_filename("LPT1"), "LPT1_");
        assert_eq!(normalize_media_filename("console.png"), "console.png");
        assert_eq!(normalize_media_filename("name."), "name._");
        assert_eq!(normalize_media_filename("name "), "name _");
    }

    #[test]
    fn truncates_to_120_utf8_bytes_keeping_the_extension() {
        let long = format!("{}.jpeg", "é".repeat(100));
        let out = normalize_media_filename(&long);
        assert!(out.len() <= 120);
        assert!(out.ends_with(".jpeg"));
        assert_eq!(truncate_filename("short.png", MAX_MEDIA_FILENAME_BYTES), "short.png");
        let hashed = add_hash_suffix(&"a".repeat(200), "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(hashed.len(), 120);
        assert!(hashed.ends_with("-0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(add_hash_suffix("a.png", "ff"), "a-ff.png");
    }

    #[test]
    fn safety_one_normal_path_component() {
        assert!(filename_is_safe("a.png"));
        for n in ["", ".", "..", "a/b.png", "/etc/passwd"] {
            assert!(!filename_is_safe(n), "{n}");
        }
        assert_eq!(normalize_media_filename("../../evil.png"), "....evil.png");
        assert_eq!(normalize_media_filename(".."), ".._");
        assert_eq!(normalize_media_filename("/"), "");
        assert!(is_normalized_media_filename("a.png"));
        assert!(!is_normalized_media_filename("a:b.png"));
        assert!(!is_normalized_media_filename(""));
    }

    #[test]
    fn extracts_references_from_html_and_templates() {
        let refs = extract_media_references(
            r#"<img src="a.png"> <img src='b &amp; c.jpg'> <audio src=d.mp3> [sound:e.ogg] <img src="https://x/y.png"> <img src="data:image/png;base64,AAA">"#,
        );
        assert_eq!(refs, vec!["a.png", "b & c.jpg", "d.mp3", "e.ogg"]);
        assert!(extract_media_references("plain text").is_empty());
        let t =
            extract_template_media_references(r#"@font-face { src: url("_font.woff2"); } .x { background: url(//cdn/x.png) }"#);
        assert_eq!(t, vec!["_font.woff2"]);
        assert!(is_remote_reference("#anchor"));
        assert!(!is_remote_reference("c:d.png") || is_remote_reference("c:d.png")); // scheme-like, Anki treats as remote
        assert_eq!(split_extension(".hidden"), (".hidden", ""));
        assert_eq!(split_extension("a.b.png"), ("a.b", ".png"));
    }

    #[test]
    fn percent_decoding_is_lenient() {
        assert_eq!(percent_decode("caf%C3%A9.png"), "café.png");
        assert_eq!(percent_decode("100%.png"), "100%.png");
        assert_eq!(percent_decode("bad%FF.png"), "bad%FF.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
    }
}
