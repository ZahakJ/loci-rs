//! Anki template engine (recall `src/core/template/`): parser, renderer, cloze, image occlusion,
//! typed answers, maths, CSS scoping, card generation and template checking.
//!
//! `sanitize-config.ts` (the DOMPurify configuration) is deliberately not ported: the UI
//! sanitises rendered HTML itself.

pub mod check;
pub mod cloze;
pub mod css;
pub mod generate;
pub mod math;
pub mod normalize;
pub mod occlusion;
pub mod parse;
pub mod render;
pub mod typed;

pub use check::{check_template, line_column, template_issue_messages, Side, TemplateIssue};
pub use cloze::{
    cloze_answer_plain, cloze_filter, cloze_numbers_in_string, cloze_to_text, clozed_text, current_cloze_number,
    extract_cloze_for_typing, image_occlusion_text, next_cloze_number, parse_clozes, reveal_cloze_text, reveal_cloze_text_only,
    strip_html_inside_mathjax, wrap_cloze, ClozeNode, ClozeTextOrCloze, RevealOptions, WrappedCloze,
};
pub use css::{scope_card_css, scope_selector, CardCssOptions, LOCI_CARD_BASE_CSS};
pub use generate::{
    card_ords_for_note, cloze_card_ords, compute_req, non_empty_field_names, ord_is_valid, pad_fields, question_is_blank,
    template_is_empty, template_requirements, CardOrdsOptions, FieldRef, MAX_CLOZE_ORD,
};
pub use math::{escape_attr, escape_html, normalize_math, replace_sound_tags, rewrite_media_urls, SoundReplacement};
pub use occlusion::{
    first_image_name, format_io_number, occlusion_mode, occlusion_payload, occlusion_units, parse_occlusion_body,
    parse_occlusion_field, serialize_occlusion_field, serialize_occlusion_shape, IoShape, OcclusionUnits,
};
pub use parse::{
    check_field_references, has_cloze_replacement, is_cloze_conditional_key, parse_template, parse_template_cached,
    referenced_keys, ParsedTemplate, Severity, TemplateError, TemplateErrorCode, TemplateNode, KNOWN_FILTERS, SPECIAL_FIELDS,
};
pub use render::{detect_dir, render_card, RenderCardInput};
pub use typed::{
    compare_typed_answer, expected_text_from_html, normalize_typed_text, TypedCompareOptions, TypedDiffOp, TypedResult,
    TypedSegment,
};
