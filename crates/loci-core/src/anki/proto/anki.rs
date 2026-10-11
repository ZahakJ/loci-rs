//! Typed decoders for the Anki messages Loci reads, by field number (see the module doc of
//! `anki::proto` for the AGPL note and the sources of the numbers).

use super::{round_f32, ProtoDecodeError, ProtoMessage};
use crate::anki::image::Json;

type PResult<T> = std::result::Result<T, ProtoDecodeError>;

/// Parses the `other` (field 255) bytes: JSON map of legacy keys Anki keeps for round-trips.
fn parse_other(bytes: &[u8]) -> Json {
    if bytes.is_empty() {
        return Json::new();
    }
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => Json::new(),
    }
}

// ───────────────────────── anki.import_export ─────────────────────────

/// PackageMetadata { Version version = 1 }   Version: 0 UNKNOWN, 1 LEGACY_1, 2 LEGACY_2, 3 LATEST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMetadata {
    pub version: i64,
}
pub fn decode_package_metadata(bytes: &[u8]) -> PResult<PackageMetadata> {
    Ok(PackageMetadata { version: ProtoMessage::new(bytes)?.enum_value(1) })
}

/// MediaEntries { repeated MediaEntry entries = 1 }
/// MediaEntry   { string name = 1; uint32 size = 2; bytes sha1 = 3; optional uint32 legacy_zip_filename = 255 }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEntry {
    pub name: String,
    pub size: u64,
    pub sha1: Vec<u8>,
    pub legacy_zip_filename: Option<u64>,
}
pub fn decode_media_entries(bytes: &[u8]) -> PResult<Vec<MediaEntry>> {
    ProtoMessage::new(bytes)?
        .repeated_messages(1)?
        .iter()
        .map(|m| {
            Ok(MediaEntry {
                name: m.string(1),
                size: m.uint(2),
                sha1: m.bytes(3).to_vec(),
                legacy_zip_filename: m.optional_uint(255),
            })
        })
        .collect()
}

// ───────────────────────── anki.notetypes ─────────────────────────

/// Notetype.Config:
///   1 kind (enum: 0 NORMAL, 1 CLOZE)      2 sort_field_idx (uint32)   3 css (string)
///   4 target_deck_id_unused (int64)        5 latex_pre (string)        6 latex_post (string)
///   7 latex_svg (bool)                     8 reqs (repeated CardRequirement)
///   9 original_stock_kind (enum)           10 original_id (optional int64)   255 other (bytes, JSON)
/// CardRequirement: 1 card_ord (uint32), 2 kind (enum: 0 NONE, 1 ANY, 2 ALL), 3 field_ords (repeated uint32)
/// OriginalStockKind: 0 UNKNOWN, 1 BASIC, 2 BASIC_AND_REVERSED, 3 BASIC_OPTIONAL_REVERSED,
///                    4 BASIC_TYPING, 5 CLOZE, 6 IMAGE_OCCLUSION
#[derive(Debug, Clone, PartialEq)]
pub struct CardRequirementProto {
    pub card_ord: u64,
    pub kind: i64,
    pub field_ords: Vec<u64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct NotetypeConfigProto {
    pub kind: i64,
    pub sort_field_idx: u64,
    pub css: String,
    pub target_deck_id_unused: i64,
    pub latex_pre: String,
    pub latex_post: String,
    pub latex_svg: bool,
    pub reqs: Vec<CardRequirementProto>,
    pub original_stock_kind: i64,
    pub original_id: Option<i64>,
    pub other: Json,
}
pub fn decode_notetype_config(bytes: &[u8]) -> PResult<NotetypeConfigProto> {
    let m = ProtoMessage::new(bytes)?;
    let mut reqs = Vec::new();
    for r in m.repeated_messages(8)? {
        reqs.push(CardRequirementProto { card_ord: r.uint(1), kind: r.enum_value(2), field_ords: r.repeated_uint(3)? });
    }
    Ok(NotetypeConfigProto {
        kind: m.enum_value(1),
        sort_field_idx: m.uint(2),
        css: m.string(3),
        target_deck_id_unused: m.int(4),
        latex_pre: m.string(5),
        latex_post: m.string(6),
        latex_svg: m.bool(7),
        reqs,
        original_stock_kind: m.enum_value(9),
        original_id: m.optional_int(10),
        other: parse_other(m.bytes(255)),
    })
}

/// Notetype.Field.Config:
///   1 sticky (bool)  2 rtl (bool)  3 font_name (string)  4 font_size (uint32)  5 description (string)
///   6 plain_text (bool)  7 collapsed (bool)  8 exclude_from_search (bool)  9 id (optional int64)
///   10 tag (optional uint32)  11 prevent_deletion (bool)  255 other (bytes, JSON)
#[derive(Debug, Clone, PartialEq)]
pub struct FieldConfigProto {
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u64,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
    pub id: Option<i64>,
    pub tag: Option<u64>,
    pub prevent_deletion: bool,
    pub other: Json,
}
pub fn decode_field_config(bytes: &[u8]) -> PResult<FieldConfigProto> {
    let m = ProtoMessage::new(bytes)?;
    Ok(FieldConfigProto {
        sticky: m.bool(1),
        rtl: m.bool(2),
        font_name: m.string(3),
        font_size: m.uint(4),
        description: m.string(5),
        plain_text: m.bool(6),
        collapsed: m.bool(7),
        exclude_from_search: m.bool(8),
        id: m.optional_int(9),
        tag: m.optional_uint(10),
        prevent_deletion: m.bool(11),
        other: parse_other(m.bytes(255)),
    })
}

/// Notetype.Template.Config:
///   1 q_format  2 a_format  3 q_format_browser  4 a_format_browser (strings)
///   5 target_deck_id (int64)  6 browser_font_name (string)  7 browser_font_size (uint32)
///   8 id (optional int64)  255 other (bytes, JSON)
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateConfigProto {
    pub q_format: String,
    pub a_format: String,
    pub q_format_browser: String,
    pub a_format_browser: String,
    pub target_deck_id: i64,
    pub browser_font_name: String,
    pub browser_font_size: u64,
    pub id: Option<i64>,
    pub other: Json,
}
pub fn decode_template_config(bytes: &[u8]) -> PResult<TemplateConfigProto> {
    let m = ProtoMessage::new(bytes)?;
    Ok(TemplateConfigProto {
        q_format: m.string(1),
        a_format: m.string(2),
        q_format_browser: m.string(3),
        a_format_browser: m.string(4),
        target_deck_id: m.int(5),
        browser_font_name: m.string(6),
        browser_font_size: m.uint(7),
        id: m.optional_int(8),
        other: parse_other(m.bytes(255)),
    })
}

// ───────────────────────── anki.decks ─────────────────────────

/// Deck.Common:
///   1 study_collapsed (bool)  2 browser_collapsed (bool)  3 last_day_studied (uint32)
///   4 new_studied (int32)  5 review_studied (int32)  6 learning_studied (int32)
///   7 milliseconds_studied (int32)  255 other (bytes, JSON)
#[derive(Debug, Clone, PartialEq)]
pub struct DeckCommonProto {
    pub study_collapsed: bool,
    pub browser_collapsed: bool,
    pub last_day_studied: u64,
    pub new_studied: i64,
    pub review_studied: i64,
    pub learning_studied: i64,
    pub milliseconds_studied: i64,
    pub other: Json,
}
pub fn decode_deck_common(bytes: &[u8]) -> PResult<DeckCommonProto> {
    let m = ProtoMessage::new(bytes)?;
    Ok(DeckCommonProto {
        study_collapsed: m.bool(1),
        browser_collapsed: m.bool(2),
        last_day_studied: m.uint(3),
        new_studied: m.int(4),
        review_studied: m.int(5),
        learning_studied: m.int(6),
        milliseconds_studied: m.int(7),
        other: parse_other(m.bytes(255)),
    })
}

/// Deck.KindContainer { oneof kind { Normal normal = 1; Filtered filtered = 2 } }
/// Deck.Normal:
///   1 config_id (int64)  2 extend_new (uint32)  3 extend_review (uint32)  4 description (string)
///   5 markdown_description (bool)  6 review_limit (optional uint32)  7 new_limit (optional uint32)
///   8 review_limit_today (DayLimit)  9 new_limit_today (DayLimit)  10 desired_retention (optional float)
///   DayLimit: 1 limit (uint32), 2 today (uint32)
/// Deck.Filtered:
///   1 reschedule (bool)  2 search_terms (repeated SearchTerm{1 search, 2 limit, 3 order})
///   3 delays (repeated float)  4 preview_delay (uint32)  5 preview_hard_secs  6 preview_good_secs
///   7 preview_again_secs (uint32)
#[derive(Debug, Clone, PartialEq)]
pub struct DayLimitProto {
    pub limit: u64,
    pub today: u64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeckNormalProto {
    pub config_id: i64,
    pub extend_new: u64,
    pub extend_review: u64,
    pub description: String,
    pub markdown_description: bool,
    pub review_limit: Option<u64>,
    pub new_limit: Option<u64>,
    pub review_limit_today: Option<DayLimitProto>,
    pub new_limit_today: Option<DayLimitProto>,
    pub desired_retention: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SearchTermProto {
    pub search: String,
    pub limit: u64,
    pub order: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeckFilteredProto {
    pub reschedule: bool,
    pub search_terms: Vec<SearchTermProto>,
    pub delays: Vec<f64>,
    pub preview_delay: u64,
    pub preview_again_secs: u64,
    pub preview_hard_secs: u64,
    pub preview_good_secs: u64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum DeckKindProto {
    Normal(DeckNormalProto),
    Filtered(DeckFilteredProto),
}

pub fn decode_deck_kind(bytes: &[u8]) -> PResult<DeckKindProto> {
    let m = ProtoMessage::new(bytes)?;
    if let Some(filtered) = m.message(2)? {
        if !m.has(1) {
            let mut search_terms = Vec::new();
            for t in filtered.repeated_messages(2)? {
                search_terms.push(SearchTermProto { search: t.string(1), limit: t.uint(2), order: t.enum_value(3) });
            }
            return Ok(DeckKindProto::Filtered(DeckFilteredProto {
                reschedule: filtered.bool(1),
                search_terms,
                delays: filtered.repeated_float(3)?.into_iter().map(round_f32).collect(),
                preview_delay: filtered.uint(4),
                preview_again_secs: filtered.uint(7),
                preview_hard_secs: filtered.uint(5),
                preview_good_secs: filtered.uint(6),
            }));
        }
    }
    let n = m.message(1)?.unwrap_or(ProtoMessage { fields: Vec::new() });
    let day_limit = |f: u32| -> PResult<Option<DayLimitProto>> {
        Ok(n.message(f)?.map(|d| DayLimitProto { limit: d.uint(1), today: d.uint(2) }))
    };
    Ok(DeckKindProto::Normal(DeckNormalProto {
        config_id: n.int(1),
        extend_new: n.uint(2),
        extend_review: n.uint(3),
        description: n.string(4),
        markdown_description: n.bool(5),
        review_limit: n.optional_uint(6),
        new_limit: n.optional_uint(7),
        review_limit_today: day_limit(8)?,
        new_limit_today: day_limit(9)?,
        desired_retention: n.optional_float(10).map(round_f32),
    }))
}

// ───────────────────────── anki.deck_config ─────────────────────────

/// DeckConfig.Config (all numbers are field numbers):
///   1 learn_steps (repeated float, minutes)       2 relearn_steps (repeated float, minutes)
///   3 fsrs_params_4 (repeated float)              5 fsrs_params_5 (repeated float)
///   6 fsrs_params_6 (repeated float)              4 easy_days_percentages (repeated float)
///   9 new_per_day (uint32)                        10 reviews_per_day (uint32)
///   35 new_per_day_minimum (uint32)               11 initial_ease (float, 2.5 = 250%)
///   12 easy_multiplier (float)                    13 hard_multiplier (float)
///   14 lapse_multiplier (float)                   15 interval_multiplier (float)
///   16 maximum_review_interval (uint32, days)     17 minimum_lapse_interval (uint32, days)
///   18 graduating_interval_good (uint32, days)    19 graduating_interval_easy (uint32, days)
///   20 new_card_insert_order (enum: 0 DUE, 1 RANDOM)
///   34 new_card_gather_priority (enum: 0 DECK, 5 DECK_THEN_RANDOM_NOTES, 1 LOWEST_POSITION,
///                                2 HIGHEST_POSITION, 3 RANDOM_NOTES, 4 RANDOM_CARDS)
///   32 new_card_sort_order (enum: 0 TEMPLATE, 1 NO_SORT, 2 TEMPLATE_THEN_RANDOM,
///                           3 RANDOM_NOTE_THEN_TEMPLATE, 4 RANDOM_CARD)
///   30 new_mix, 31 interday_learning_mix (enum: 0 MIX_WITH_REVIEWS, 1 AFTER_REVIEWS, 2 BEFORE_REVIEWS)
///   33 review_order (enum: 0 DAY, 1 DAY_THEN_DECK, 2 DECK_THEN_DAY, 3 INTERVALS_ASCENDING,
///                    4 INTERVALS_DESCENDING, 5 EASE_ASCENDING, 6 EASE_DESCENDING,
///                    7 RETRIEVABILITY_ASCENDING, 11 RETRIEVABILITY_DESCENDING,
///                    12 RELATIVE_OVERDUENESS, 8 RANDOM, 9 ADDED, 10 REVERSE_ADDED)
///   21 leech_action (enum: 0 SUSPEND, 1 TAG_ONLY)  22 leech_threshold (uint32)
///   23 disable_autoplay (bool)  24 cap_answer_time_to_secs (uint32)  25 show_timer (bool)
///   38 stop_timer_on_answer (bool)  41 seconds_to_show_question (float)  42 seconds_to_show_answer (float)
///   36 question_action (enum)  43 answer_action (enum)  44 wait_for_audio (bool)
///   26 skip_question_when_replaying_answer (bool)
///   27 bury_new  28 bury_reviews  29 bury_interday_learning (bool)
///   37 desired_retention (float)  46 ignore_revlogs_before_date (string)  40 historical_retention (float)
///   45 param_search (string)  255 other (bytes, JSON)
#[derive(Debug, Clone, PartialEq)]
pub struct DeckConfigProto {
    pub learn_steps: Vec<f64>,
    pub relearn_steps: Vec<f64>,
    pub fsrs_params_4: Vec<f64>,
    pub fsrs_params_5: Vec<f64>,
    pub fsrs_params_6: Vec<f64>,
    pub easy_days_percentages: Vec<f64>,
    pub new_per_day: u64,
    pub reviews_per_day: u64,
    pub new_per_day_minimum: u64,
    pub initial_ease: f64,
    pub easy_multiplier: f64,
    pub hard_multiplier: f64,
    pub lapse_multiplier: f64,
    pub interval_multiplier: f64,
    pub maximum_review_interval: u64,
    pub minimum_lapse_interval: u64,
    pub graduating_interval_good: u64,
    pub graduating_interval_easy: u64,
    pub new_card_insert_order: i64,
    pub new_card_gather_priority: i64,
    pub new_card_sort_order: i64,
    pub new_mix: i64,
    pub interday_learning_mix: i64,
    pub review_order: i64,
    pub leech_action: i64,
    pub leech_threshold: u64,
    pub disable_autoplay: bool,
    pub cap_answer_time_to_secs: u64,
    pub show_timer: bool,
    pub stop_timer_on_answer: bool,
    pub seconds_to_show_question: f64,
    pub seconds_to_show_answer: f64,
    pub question_action: i64,
    pub answer_action: i64,
    pub wait_for_audio: bool,
    pub skip_question_when_replaying_answer: bool,
    pub bury_new: bool,
    pub bury_reviews: bool,
    pub bury_interday_learning: bool,
    pub desired_retention: f64,
    pub ignore_revlogs_before_date: String,
    pub historical_retention: f64,
    pub param_search: String,
    pub other: Json,
}
pub fn decode_deck_config_config(bytes: &[u8]) -> PResult<DeckConfigProto> {
    let m = ProtoMessage::new(bytes)?;
    let fl = |n: u32| round_f32(m.float(n));
    let rf = |n: u32| -> PResult<Vec<f64>> { Ok(m.repeated_float(n)?.into_iter().map(round_f32).collect()) };
    Ok(DeckConfigProto {
        learn_steps: rf(1)?,
        relearn_steps: rf(2)?,
        fsrs_params_4: rf(3)?,
        fsrs_params_5: rf(5)?,
        fsrs_params_6: rf(6)?,
        easy_days_percentages: rf(4)?,
        new_per_day: m.uint(9),
        reviews_per_day: m.uint(10),
        new_per_day_minimum: m.uint(35),
        initial_ease: fl(11),
        easy_multiplier: fl(12),
        hard_multiplier: fl(13),
        lapse_multiplier: fl(14),
        interval_multiplier: fl(15),
        maximum_review_interval: m.uint(16),
        minimum_lapse_interval: m.uint(17),
        graduating_interval_good: m.uint(18),
        graduating_interval_easy: m.uint(19),
        new_card_insert_order: m.enum_value(20),
        new_card_gather_priority: m.enum_value(34),
        new_card_sort_order: m.enum_value(32),
        new_mix: m.enum_value(30),
        interday_learning_mix: m.enum_value(31),
        review_order: m.enum_value(33),
        leech_action: m.enum_value(21),
        leech_threshold: m.uint(22),
        disable_autoplay: m.bool(23),
        cap_answer_time_to_secs: m.uint(24),
        show_timer: m.bool(25),
        stop_timer_on_answer: m.bool(38),
        seconds_to_show_question: fl(41),
        seconds_to_show_answer: fl(42),
        question_action: m.enum_value(36),
        answer_action: m.enum_value(43),
        wait_for_audio: m.bool(44),
        skip_question_when_replaying_answer: m.bool(26),
        bury_new: m.bool(27),
        bury_reviews: m.bool(28),
        bury_interday_learning: m.bool(29),
        desired_retention: fl(37),
        ignore_revlogs_before_date: m.string(46),
        historical_retention: fl(40),
        param_search: m.string(45),
        other: parse_other(m.bytes(255)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32le(x: f32) -> Vec<u8> {
        x.to_le_bytes().to_vec()
    }
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn package_metadata() {
        assert_eq!(decode_package_metadata(&[0x08, 0x03]).unwrap().version, 3);
        assert_eq!(decode_package_metadata(&[0x08, 0x02]).unwrap().version, 2);
        assert_eq!(decode_package_metadata(&[]).unwrap().version, 0);
    }

    #[test]
    fn media_entries_with_sha1_and_legacy_zip_filename() {
        let sha: Vec<u8> = (0..20).collect();
        let entry = cat(&[&[0x0a, 0x05], b"a.png", &[0x10, 0x03, 0x1a, 0x14], &sha, &[0xf8, 0x0f, 0x07]]);
        let entry2 = cat(&[&[0x0a, 0x05], b"b.mp3", &[0x10, 0x80, 0x01]]);
        let bytes = cat(&[&[0x0a, entry.len() as u8], &entry, &[0x0a, entry2.len() as u8], &entry2]);
        let out = decode_media_entries(&bytes).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "a.png");
        assert_eq!(out[0].size, 3);
        assert_eq!(out[0].legacy_zip_filename, Some(7));
        assert_eq!(out[0].sha1, sha);
        assert_eq!(out[1].name, "b.mp3");
        assert_eq!(out[1].size, 128);
        assert_eq!(out[1].legacy_zip_filename, None);
    }

    #[test]
    fn notetype_config_with_requirements_and_stock_kind() {
        let req = [0x08, 0x01, 0x10, 0x02, 0x1a, 0x02, 0x00, 0x01];
        let bytes = cat(&[
            &[0x08, 0x01, 0x10, 0x02, 0x1a, 0x04],
            b".c{}",
            &[0x2a, 0x01],
            b"P",
            &[0x32, 0x01],
            b"Q",
            &[0x38, 0x01, 0x42, req.len() as u8],
            &req,
            &[0x48, 0x06, 0x50, 0x2a],
        ]);
        let c = decode_notetype_config(&bytes).unwrap();
        assert_eq!(
            c,
            NotetypeConfigProto {
                kind: 1,
                sort_field_idx: 2,
                css: ".c{}".into(),
                target_deck_id_unused: 0,
                latex_pre: "P".into(),
                latex_post: "Q".into(),
                latex_svg: true,
                reqs: vec![CardRequirementProto { card_ord: 1, kind: 2, field_ords: vec![0, 1] }],
                original_stock_kind: 6,
                original_id: Some(42),
                other: Json::new(),
            }
        );
    }

    #[test]
    fn field_and_template_config_including_other_json() {
        let other = br#"{"media":[]}"#;
        let f = decode_field_config(&cat(&[
            &[0x08, 0x01, 0x10, 0x01, 0x1a, 0x05],
            b"Arial",
            &[0x20, 0x14, 0x2a, 0x02],
            b"hi",
            &[0x30, 0x01, 0x38, 0x01, 0x50, 0x03, 0x58, 0x01, 0xfa, 0x0f, other.len() as u8],
            other,
        ]))
        .unwrap();
        assert!(f.sticky && f.rtl && f.plain_text && f.collapsed && f.prevent_deletion);
        assert_eq!((f.font_name.as_str(), f.font_size, f.description.as_str(), f.tag), ("Arial", 20, "hi", Some(3)));
        assert_eq!(f.other.get("media"), Some(&serde_json::json!([])));
        let t = decode_template_config(&cat(&[
            &[0x0a, 0x03],
            b"{{F",
            &[0x12, 0x01],
            b"A",
            &[0x28, 0x05, 0x32, 0x01],
            b"X",
            &[0x38, 0x0c],
        ]))
        .unwrap();
        assert_eq!(t.q_format, "{{F");
        assert_eq!(t.a_format, "A");
        assert_eq!(t.q_format_browser, "");
        assert_eq!((t.target_deck_id, t.browser_font_name.as_str(), t.browser_font_size), (5, "X", 12));
    }

    #[test]
    fn deck_common_and_kind_container() {
        let c = decode_deck_common(&[0x08, 0x01, 0x18, 0xc2, 0x09, 0x20, 0x03, 0x28, 0x0a, 0x38, 0xe8, 0x07]).unwrap();
        assert!(c.study_collapsed && !c.browser_collapsed);
        assert_eq!((c.last_day_studied, c.new_studied, c.review_studied, c.milliseconds_studied), (1218, 3, 10, 1000));
        let normal = cat(&[&[0x08, 0x07, 0x10, 0x02, 0x22, 0x01], b"d", &[0x55], &f32le(0.85)]);
        let k = decode_deck_kind(&cat(&[&[0x0a, normal.len() as u8], &normal])).unwrap();
        assert_eq!(
            k,
            DeckKindProto::Normal(DeckNormalProto {
                config_id: 7,
                extend_new: 2,
                extend_review: 0,
                description: "d".into(),
                markdown_description: false,
                review_limit: None,
                new_limit: None,
                review_limit_today: None,
                new_limit_today: None,
                desired_retention: Some(0.85),
            })
        );
        let term = cat(&[&[0x0a, 0x06], b"is:due", &[0x10, 0x0a, 0x18, 0x06]]);
        let filtered = cat(&[&[0x08, 0x01, 0x12, term.len() as u8], &term, &[0x20, 0x0a]]);
        match decode_deck_kind(&cat(&[&[0x12, filtered.len() as u8], &filtered])).unwrap() {
            DeckKindProto::Filtered(f) => {
                assert!(f.reschedule);
                assert_eq!(f.search_terms, vec![SearchTermProto { search: "is:due".into(), limit: 10, order: 6 }]);
                assert_eq!(f.preview_delay, 10);
            }
            other => panic!("expected filtered, got {other:?}"),
        }
    }

    #[test]
    fn deck_config_config_by_field_number() {
        let params = [0.212f32, 1.2931, 2.3065];
        let p: Vec<u8> = params.iter().flat_map(|x| f32le(*x)).collect();
        let bytes = cat(&[
            &[0x0a, 0x08],
            &f32le(1.0),
            &f32le(10.0), // 1 learn_steps
            &[0x12, 0x04],
            &f32le(10.0), // 2 relearn_steps
            &[0x32, 0x0c],
            &p,                              // 6 fsrs_params_6
            &[0x48, 0x14, 0x50, 0xc8, 0x01], // 9 new/day 20, 10 reviews/day 200
            &[0x5d],
            &f32le(2.5),
            &[0x65],
            &f32le(1.3),
            &[0x6d],
            &f32le(1.2),                           // 11, 12, 13
            &[0x80, 0x01, 0x94, 0x9d, 0x02],       // 16 maximum_review_interval 36500
            &[0x90, 0x01, 0x01, 0x98, 0x01, 0x04], // 18, 19
            &[0xa0, 0x01, 0x01],                   // 20 insert order RANDOM
            &[0x88, 0x02, 0x03],                   // 33 review order
            &[0xa8, 0x01, 0x01, 0xb0, 0x01, 0x08], // 21 leech action TAG_ONLY, 22 threshold 8
            &[0xd8, 0x01, 0x01],                   // 27 bury_new
            &[0xad, 0x02],
            &f32le(0.9), // 37 desired retention
            &[0xc5, 0x02],
            &f32le(0.87), // 40 historical retention
        ]);
        let c = decode_deck_config_config(&bytes).unwrap();
        assert_eq!(c.learn_steps, vec![1.0, 10.0]);
        assert_eq!(c.relearn_steps, vec![10.0]);
        assert_eq!(c.fsrs_params_6, vec![0.212, 1.2931, 2.3065]);
        assert!(c.fsrs_params_5.is_empty());
        assert_eq!((c.new_per_day, c.reviews_per_day), (20, 200));
        assert_eq!((c.initial_ease, c.easy_multiplier, c.hard_multiplier), (2.5, 1.3, 1.2));
        assert_eq!(c.maximum_review_interval, 36500);
        assert_eq!((c.graduating_interval_good, c.graduating_interval_easy), (1, 4));
        assert_eq!((c.new_card_insert_order, c.review_order, c.leech_action, c.leech_threshold), (1, 3, 1, 8));
        assert!(c.bury_new && !c.bury_reviews);
        assert_eq!((c.desired_retention, c.historical_retention), (0.9, 0.87));
    }
}
