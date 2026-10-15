//! Sentences produced by core (insights, explanations, summaries).
//! Each goes through the i18n dictionary when its key exists there,
//! and otherwise uses the English ICU-lite fallback given at the call site, so new sentences work
//! before a translator adds them to the dictionary. The implementation lives in `i18n`.

pub use crate::i18n::{msg, pct};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_prefers_the_dictionary_and_falls_back_to_the_literal() {
        assert_eq!(msg("optimise.undo", "fallback", &[]), "Optimise parameters");
        assert_eq!(msg("no.such.key", "Answer {rating}", &[("rating", "Good".into())]), "Answer Good");
        assert_eq!(msg("no.such.key", "{n, plural, one {# day} other {# days}}", &[("n", 1_i64.into())]), "1 day");
        assert_eq!(pct(0.873), "87%");
    }
}
