//! Unicode canonical composition (NFC) for media file names, as Anki normalises them.
//! Uses the `unicode-normalization` crate, which also performs canonical reordering of combining marks.

use unicode_normalization::UnicodeNormalization;

/// The NFC form of `s`.
pub fn nfc(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    s.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::nfc;

    #[test]
    fn composes_decomposed_sequences() {
        assert_eq!(nfc("e\u{301}"), "\u{e9}");
        assert_eq!(nfc("\u{1100}\u{1161}\u{11a8}"), "\u{ac01}");
        assert_eq!(nfc("plain.png"), "plain.png");
        // Canonical reordering: a dot below (ccc 220) must precede an acute (ccc 230) before composing.
        assert_eq!(nfc("a\u{301}\u{323}"), "\u{1ea1}\u{301}");
    }
}
