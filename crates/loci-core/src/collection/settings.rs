//! Collection-wide settings stored in `col_meta`, one key per setting (snake_case), as plain
//! strings for enums and JSON for numbers/booleans.

use crate::error::{LociError, Result};
use crate::types::*;

pub fn default_settings() -> CollectionSettings {
    CollectionSettings {
        rollover_hour: 4,
        learn_ahead_minutes: 20,
        locale: "en".to_string(),
        theme: ThemePref::System,
        card_font: CardFont::Serif,
        card_font_size: 22,
        card_align: CardAlign::Center,
        show_keyboard_hints: true,
        reduced_motion: ReducedMotion::System,
        typed_answer_ignore_case: true,
        typed_answer_ignore_diacritics: true,
    }
}

impl Default for CollectionSettings {
    fn default() -> Self {
        default_settings()
    }
}

/// col_meta keys, in the order of the struct fields.
pub const KEY_ROLLOVER_HOUR: &str = "rollover_hour";
pub const KEY_LEARN_AHEAD_MINUTES: &str = "learn_ahead_minutes";
pub const KEY_LOCALE: &str = "locale";
pub const KEY_THEME: &str = "theme";
pub const KEY_CARD_FONT: &str = "card_font";
pub const KEY_CARD_FONT_SIZE: &str = "card_font_size";
pub const KEY_CARD_ALIGN: &str = "card_align";
pub const KEY_SHOW_KEYBOARD_HINTS: &str = "show_keyboard_hints";
pub const KEY_REDUCED_MOTION: &str = "reduced_motion";
pub const KEY_TYPED_ANSWER_IGNORE_CASE: &str = "typed_answer_ignore_case";
pub const KEY_TYPED_ANSWER_IGNORE_DIACRITICS: &str = "typed_answer_ignore_diacritics";

pub const SETTING_KEYS: [&str; 11] = [
    KEY_ROLLOVER_HOUR,
    KEY_LEARN_AHEAD_MINUTES,
    KEY_LOCALE,
    KEY_THEME,
    KEY_CARD_FONT,
    KEY_CARD_FONT_SIZE,
    KEY_CARD_ALIGN,
    KEY_SHOW_KEYBOARD_HINTS,
    KEY_REDUCED_MOTION,
    KEY_TYPED_ANSWER_IGNORE_CASE,
    KEY_TYPED_ANSWER_IGNORE_DIACRITICS,
];

fn enum_from<T: serde::de::DeserializeOwned>(raw: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(raw.to_string())).ok()
}

fn int_in(raw: &str, lo: i64, hi: i64) -> Option<i64> {
    let v: f64 = serde_json::from_str(raw).ok()?;
    if !v.is_finite() || v < lo as f64 || v > hi as f64 {
        return None;
    }
    Some(v.round() as i64)
}

fn bool_from(raw: &str) -> Option<bool> {
    serde_json::from_str::<bool>(raw).ok()
}

/// Decodes stored values into settings, falling back to the default for missing or malformed ones.
pub fn decode_settings(get: impl Fn(&str) -> Option<String>) -> CollectionSettings {
    let d = default_settings();
    let g = |k: &str| get(k);
    CollectionSettings {
        rollover_hour: g(KEY_ROLLOVER_HOUR).and_then(|r| int_in(&r, 0, 23)).unwrap_or(d.rollover_hour),
        learn_ahead_minutes: g(KEY_LEARN_AHEAD_MINUTES).and_then(|r| int_in(&r, 0, 1440)).unwrap_or(d.learn_ahead_minutes),
        locale: g(KEY_LOCALE).unwrap_or(d.locale),
        theme: g(KEY_THEME).and_then(|r| enum_from(&r)).unwrap_or(d.theme),
        card_font: g(KEY_CARD_FONT).and_then(|r| enum_from(&r)).unwrap_or(d.card_font),
        card_font_size: g(KEY_CARD_FONT_SIZE).and_then(|r| int_in(&r, 10, 64)).unwrap_or(d.card_font_size),
        card_align: g(KEY_CARD_ALIGN).and_then(|r| enum_from(&r)).unwrap_or(d.card_align),
        show_keyboard_hints: g(KEY_SHOW_KEYBOARD_HINTS).and_then(|r| bool_from(&r)).unwrap_or(d.show_keyboard_hints),
        reduced_motion: g(KEY_REDUCED_MOTION).and_then(|r| enum_from(&r)).unwrap_or(d.reduced_motion),
        typed_answer_ignore_case: g(KEY_TYPED_ANSWER_IGNORE_CASE)
            .and_then(|r| bool_from(&r))
            .unwrap_or(d.typed_answer_ignore_case),
        typed_answer_ignore_diacritics: g(KEY_TYPED_ANSWER_IGNORE_DIACRITICS)
            .and_then(|r| bool_from(&r))
            .unwrap_or(d.typed_answer_ignore_diacritics),
    }
}

fn enum_str<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// Validates a patch; `LociError::invalid` names the first bad key. Returns the (key, encoded
/// value) pairs to store.
pub fn encode_patch(patch: &CollectionSettingsPatch) -> Result<Vec<(&'static str, String)>> {
    let mut out = Vec::new();
    if let Some(v) = patch.rollover_hour {
        if !(0..=23).contains(&v) {
            return Err(LociError::invalid("Setting rolloverHour must be an integer between 0 and 23"));
        }
        out.push((KEY_ROLLOVER_HOUR, v.to_string()));
    }
    if let Some(v) = patch.learn_ahead_minutes {
        if !(0..=1440).contains(&v) {
            return Err(LociError::invalid("Setting learnAheadMinutes must be between 0 and 1440"));
        }
        out.push((KEY_LEARN_AHEAD_MINUTES, v.to_string()));
    }
    if let Some(v) = &patch.locale {
        out.push((KEY_LOCALE, v.clone()));
    }
    if let Some(v) = &patch.theme {
        out.push((KEY_THEME, enum_str(v)));
    }
    if let Some(v) = &patch.card_font {
        out.push((KEY_CARD_FONT, enum_str(v)));
    }
    if let Some(v) = patch.card_font_size {
        if !(10..=64).contains(&v) {
            return Err(LociError::invalid("Setting cardFontSize must be between 10 and 64"));
        }
        out.push((KEY_CARD_FONT_SIZE, v.to_string()));
    }
    if let Some(v) = &patch.card_align {
        out.push((KEY_CARD_ALIGN, enum_str(v)));
    }
    if let Some(v) = patch.show_keyboard_hints {
        out.push((KEY_SHOW_KEYBOARD_HINTS, v.to_string()));
    }
    if let Some(v) = &patch.reduced_motion {
        out.push((KEY_REDUCED_MOTION, enum_str(v)));
    }
    if let Some(v) = patch.typed_answer_ignore_case {
        out.push((KEY_TYPED_ANSWER_IGNORE_CASE, v.to_string()));
    }
    if let Some(v) = patch.typed_answer_ignore_diacritics {
        out.push((KEY_TYPED_ANSWER_IGNORE_DIACRITICS, v.to_string()));
    }
    Ok(out)
}

/// Parses a JSON patch as the UI sends it (`Partial<CollectionSettings>`), rejecting unknown keys,
/// wrong types and bad enum literals with `LociError::invalid`.
pub fn parse_patch(v: &serde_json::Value) -> Result<CollectionSettingsPatch> {
    let obj = v.as_object().ok_or_else(|| LociError::invalid("Settings patch must be an object"))?;
    const KNOWN: [&str; 11] = [
        "rolloverHour",
        "learnAheadMinutes",
        "locale",
        "theme",
        "cardFont",
        "cardFontSize",
        "cardAlign",
        "showKeyboardHints",
        "reducedMotion",
        "typedAnswerIgnoreCase",
        "typedAnswerIgnoreDiacritics",
    ];
    for k in obj.keys() {
        if !KNOWN.contains(&k.as_str()) {
            return Err(LociError::invalid(format!("Unknown setting: {k}")));
        }
    }
    // Integers must be whole numbers (2.5 is rejected).
    for (k, val) in obj {
        if matches!(k.as_str(), "rolloverHour" | "learnAheadMinutes" | "cardFontSize") {
            if let Some(f) = val.as_f64() {
                if k == "rolloverHour" && f.fract() != 0.0 {
                    return Err(LociError::invalid("Setting rolloverHour must be an integer between 0 and 23"));
                }
            } else if !val.is_null() {
                return Err(LociError::invalid(format!("Setting {k} must be a number")));
            }
        }
    }
    let mut v = v.clone();
    // Fractional learnAheadMinutes / cardFontSize are allowed; round them for i64.
    if let Some(o) = v.as_object_mut() {
        for k in ["learnAheadMinutes", "cardFontSize"] {
            if let Some(f) = o.get(k).and_then(|x| x.as_f64()) {
                o.insert(k.to_string(), serde_json::Value::from(f.round() as i64));
            }
        }
    }
    serde_json::from_value::<CollectionSettingsPatch>(v).map_err(|e| LociError::invalid(format!("Invalid settings patch: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn defaults_match_design() {
        let d = default_settings();
        assert_eq!(d.rollover_hour, 4);
        assert_eq!(d.learn_ahead_minutes, 20);
        assert_eq!(d.card_font_size, 22);
    }

    #[test]
    fn malformed_stored_values_fall_back() {
        let stored: HashMap<&str, &str> =
            [(KEY_ROLLOVER_HOUR, "garbage"), (KEY_THEME, "purple"), (KEY_CARD_FONT_SIZE, "24")].into();
        let s = decode_settings(|k| stored.get(k).map(|v| v.to_string()));
        assert_eq!(s.rollover_hour, 4);
        assert_eq!(s.theme, ThemePref::System);
        assert_eq!(s.card_font_size, 24);
    }

    #[test]
    fn patches_are_validated() {
        let p =
            parse_patch(&json!({"rolloverHour": 6, "theme": "dark", "cardFontSize": 24, "showKeyboardHints": false})).unwrap();
        let enc = encode_patch(&p).unwrap();
        assert!(enc.contains(&(KEY_ROLLOVER_HOUR, "6".to_string())));
        assert!(enc.contains(&(KEY_THEME, "dark".to_string())));
        assert!(parse_patch(&json!({"rolloverHour": 2.5})).is_err());
        assert!(encode_patch(&parse_patch(&json!({"rolloverHour": 24})).unwrap()).is_err());
        assert!(parse_patch(&json!({"theme": "neon"})).is_err());
        assert!(parse_patch(&json!({"bogus": 1})).is_err());
    }
}
