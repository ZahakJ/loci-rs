//! Deck-option presets. Values follow internals §5 and
//! Anki's `DeckConfig::default`; FSRS-6 default weights from fsrs-rs v6.6.2.

use crate::types::*;

/// FSRS-6 default parameters w0..w20 (fsrs-rs v6.6.2 `DEFAULT_PARAMETERS`).
pub const FSRS6_DEFAULT_PARAMS: [f64; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629, 1.6483, 0.6014,
    1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/// The "Standard" preset (internals §5). Every `DeckConfig` field has its default here.
pub fn standard_deck_config() -> DeckConfig {
    DeckConfig {
        scheduler: SchedulerKind::Fsrs,
        new_per_day: 20,
        reviews_per_day: 9999,
        minutes_per_day: 20,
        learn_steps: vec![1.0, 10.0],
        relearn_steps: vec![10.0],
        graduating_interval: 1,
        easy_interval: 4,
        new_order: NewCardOrder::Due,
        new_gather: NewCardGather::Deck,
        new_sort: NewCardSort::TemplateThenOrder,
        review_order: ReviewOrder::DueThenRandom,
        new_review_mix: NewReviewMix::Mix,
        interday_learning_mix: NewReviewMix::Mix,
        initial_ease: 2500,
        easy_bonus: 1.3,
        interval_modifier: 1.0,
        hard_multiplier: 1.2,
        lapse_multiplier: 0.0,
        minimum_lapse_interval: 1,
        maximum_interval: 36500,
        fsrs_params: FSRS6_DEFAULT_PARAMS.to_vec(),
        desired_retention: 0.9,
        historical_retention: 0.9,
        auto_optimise: true,
        optimise_every_reviews: 400,
        leech_threshold: 8,
        leech_action: LeechAction::Tag,
        // One card of a note per day (internals §5): a reversed card right after its forward card is a give-away.
        bury_siblings_new: true,
        bury_siblings_review: true,
        bury_siblings_interday: false,
        autoplay_audio: true,
        replay_question_audio: true,
        max_answer_seconds: 60,
        show_timer: false,
        show_intervals_on_buttons: true,
    }
}

impl Default for DeckConfig {
    fn default() -> Self {
        standard_deck_config()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PresetTemplate {
    /// Preset id seeded into a new collection. 1 is the default every deck falls back to.
    pub id: DeckConfigId,
    pub name: &'static str,
    /// One-line explanation for the Settings picker.
    pub description: &'static str,
    pub config: DeckConfig,
}

/// The three learning presets of internals §5/§11. Relaxed and Intense differ from Standard only
/// in desired retention and new cards per day.
pub fn default_presets() -> Vec<PresetTemplate> {
    let mut relaxed = standard_deck_config();
    relaxed.desired_retention = 0.85;
    relaxed.new_per_day = 10;
    let mut intense = standard_deck_config();
    intense.desired_retention = 0.93;
    intense.new_per_day = 40;
    vec![
        PresetTemplate {
            id: 1,
            name: "Standard",
            description: "Remember about 9 in 10 cards; 20 new cards a day.",
            config: standard_deck_config(),
        },
        PresetTemplate {
            id: 2,
            name: "Relaxed",
            description: "Fewer reviews: remember about 85 in 100; 10 new cards a day.",
            config: relaxed,
        },
        PresetTemplate {
            id: 3,
            name: "Intense",
            description: "Remember about 93 in 100; 40 new cards a day. More reviews.",
            config: intense,
        },
    ]
}

/// Fills missing or wrongly-typed keys of a stored/user-supplied DeckConfig JSON with the
/// Standard defaults, so every reader sees a complete, typed config. Unknown keys are dropped.
pub fn normalise_deck_config(input: &serde_json::Value) -> DeckConfig {
    let base = standard_deck_config();
    let Some(src) = input.as_object() else { return base };
    // Start from the defaults as JSON, overlay type-compatible values, then deserialise.
    let mut out = serde_json::to_value(&base).expect("deck config serialises");
    let out_map = out.as_object_mut().expect("object");
    for (key, def) in out_map.iter_mut() {
        let Some(v) = src.get(key) else { continue };
        if v.is_null() {
            continue;
        }
        match def {
            serde_json::Value::Array(_) => {
                if v.is_array() && v.as_array().unwrap().iter().all(|x| x.as_f64().is_some_and(f64::is_finite)) {
                    *def = v.clone();
                }
            }
            serde_json::Value::Number(_) => {
                if let Some(n) = coerce_number(def, v) {
                    *def = n;
                }
            }
            serde_json::Value::Bool(_) => {
                if v.is_boolean() {
                    *def = v.clone();
                }
            }
            serde_json::Value::String(_) if v.is_string() => {
                *def = v.clone();
            }
            _ => {}
        }
    }
    let mut cfg: DeckConfig = match serde_json::from_value(out) {
        Ok(c) => c,
        Err(_) => {
            // An unknown enum literal: fall back field by field by retrying without the string keys.
            let mut retry = serde_json::to_value(&base).unwrap();
            let rm = retry.as_object_mut().unwrap();
            for (key, def) in rm.iter_mut() {
                if def.is_string() {
                    continue;
                }
                if let Some(v) = src.get(key) {
                    if def.is_number() {
                        if let Some(n) = coerce_number(def, v) {
                            *def = n;
                        }
                    } else if !v.is_null() && same_shape(def, v) {
                        *def = v.clone();
                    }
                }
            }
            serde_json::from_value(retry).unwrap_or_else(|_| standard_deck_config())
        }
    };
    let n = cfg.fsrs_params.len();
    if n != 17 && n != 19 && n != 21 {
        cfg.fsrs_params = FSRS6_DEFAULT_PARAMS.to_vec();
    }
    cfg.desired_retention = cfg.desired_retention.clamp(0.7, 0.99);
    cfg.new_per_day = cfg.new_per_day.max(0);
    cfg.reviews_per_day = cfg.reviews_per_day.max(0);
    cfg.maximum_interval = cfg.maximum_interval.max(1);
    cfg.leech_threshold = cfg.leech_threshold.max(1);
    cfg
}

/// A finite JSON number for a numeric default; integer defaults round a fractional input.
fn coerce_number(def: &serde_json::Value, v: &serde_json::Value) -> Option<serde_json::Value> {
    let f = v.as_f64().filter(|x| x.is_finite())?;
    if def.is_i64() {
        Some(serde_json::Value::from(f.round() as i64))
    } else {
        Some(serde_json::Value::from(f))
    }
}

fn same_shape(def: &serde_json::Value, v: &serde_json::Value) -> bool {
    match def {
        serde_json::Value::Array(_) => v.is_array(),
        serde_json::Value::Number(_) => v.is_number(),
        serde_json::Value::Bool(_) => v.is_boolean(),
        serde_json::Value::String(_) => v.is_string(),
        _ => false,
    }
}

/// Parses a stored `deck_configs.config` JSON text.
pub fn parse_deck_config(text: &str) -> DeckConfig {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => normalise_deck_config(&v),
        Err(_) => standard_deck_config(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn standard_matches_design() {
        let c = standard_deck_config();
        assert_eq!(c.fsrs_params.len(), 21);
        assert_eq!(c.learn_steps, vec![1.0, 10.0]);
        assert_eq!(c.desired_retention, 0.9);
        let p = default_presets();
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].config.desired_retention, 0.85);
        assert_eq!(p[2].config.new_per_day, 40);
    }

    #[test]
    fn normalise_fills_and_clamps() {
        let c = normalise_deck_config(
            &json!({"newPerDay": 7.0, "desiredRetention": 0.2, "fsrsParams": [1,2,3], "scheduler": "sm2", "learnSteps": "bad", "unknown": 1}),
        );
        assert_eq!(c.new_per_day, 7);
        assert_eq!(c.desired_retention, 0.7);
        assert_eq!(c.fsrs_params.len(), 21);
        assert_eq!(c.scheduler, SchedulerKind::Sm2);
        assert_eq!(c.learn_steps, vec![1.0, 10.0]);
        assert_eq!(normalise_deck_config(&json!(null)), standard_deck_config());
        // An unknown enum literal does not lose the other fields.
        let c = normalise_deck_config(&json!({"scheduler": "weird", "newPerDay": 3}));
        assert_eq!(c.new_per_day, 3);
        assert_eq!(c.scheduler, SchedulerKind::Fsrs);
    }

    #[test]
    fn json_round_trip_uses_camel_case() {
        let v = serde_json::to_value(standard_deck_config()).unwrap();
        assert_eq!(v["newReviewMix"], "mix");
        assert_eq!(v["newSort"], "template-then-order");
        assert_eq!(v["leechAction"], "tag");
    }
}
