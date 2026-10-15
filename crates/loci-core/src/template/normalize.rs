//! Unicode canonical normalisation (NFD / NFC) and combining-mark stripping for typed
//! answers and search. Backed by the `unicode-normalization` crate; `is_combining` keeps the
//! explicit mark ranges (`\p{M}`) for the scripts cards meet in practice.

/// True for a combining mark (general category M) — the ranges that matter for typed answers.
pub fn is_combining(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8
        | 0x06EA..=0x06ED | 0x0711 | 0x0730..=0x074A | 0x07A6..=0x07B0 | 0x0816..=0x082D | 0x08D3..=0x08E1
        | 0x08E3..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x0E31 | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20F0 | 0x3099..=0x309A
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
}

/// Canonical decomposition (NFD), via `unicode-normalization` (full Unicode coverage).
pub fn nfd(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    if s.is_ascii() {
        return s.to_string();
    }
    s.nfd().collect()
}

/// Canonical composition (NFC), via `unicode-normalization`.
pub fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    if s.is_ascii() {
        return s.to_string();
    }
    s.nfc().collect()
}

/// Removes combining marks after decomposing (search `nc:` and the typed-answer `nc` modifier).
pub fn strip_combining(s: &str) -> String {
    use unicode_normalization::char::is_combining_mark;
    nfc(&nfd(s).chars().filter(|c| !is_combining_mark(*c) && !is_combining(*c)).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposes_and_recomposes_latin_letters() {
        assert_eq!(nfd("é"), "e\u{301}");
        assert_eq!(nfc("e\u{301}"), "é");
        assert_eq!(nfc("été"), "été");
        assert_eq!(nfd("Ǟ"), "A\u{308}\u{304}");
        assert_eq!(nfc("A\u{308}\u{304}"), "Ǟ");
        assert_eq!(nfc("ệ"), "ệ");
        assert_eq!(strip_combining("Ünïcödé"), "Unicode");
        assert_eq!(strip_combining("كَتَبَ"), "كتب");
        assert_eq!(strip_combining("über"), "uber");
    }
}
