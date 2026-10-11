//! Card generation with Anki's exact semantics (recall `src/core/template/generate.ts`; rslib
//! cardgen.rs, template.rs `template_is_empty`, `requirements`).
//!
//! Normal note types: a template produces a card iff its question renders something with the
//! set of non-empty fields (real fields that pass `field_is_empty`, plus special fields not
//! shadowed by a real field, except FrontSide, and Tags only when the note has tags).
//! Cloze note types: one card per distinct cloze number N > 0 in any field, ord = min(N-1, 499).
//! A brand-new note that would get no card gets ord 0 (`ensure_not_empty`).
//!
//! Also home to two helpers the TypeScript kept in `collection/notetypes.ts` (`ord_is_valid`,
//! `pad_fields`) so the phase-2 collection code can reuse them.

use crate::text::field_is_empty;
use crate::types::{Notetype, NotetypeKind, ReqKind, Requirement};
use std::collections::HashSet;

use super::parse::{parse_template_cached, TemplateNode, SPECIAL_FIELDS};

/// Highest cloze card ordinal (`collection/notetypes.ts` MAX_CLOZE_ORD).
pub const MAX_CLOZE_ORD: i64 = 499;

/// Anki `template_is_empty`. `check_negated=false` makes `{{^F}}` always resolve to its children (used for req).
pub fn template_is_empty(non_empty: &HashSet<String>, nodes: &[TemplateNode], check_negated: bool) -> bool {
    for n in nodes {
        match n {
            TemplateNode::Text { .. } | TemplateNode::Comment { .. } => {}
            TemplateNode::Replacement { key, .. } => {
                if non_empty.contains(key) {
                    return false;
                }
            }
            TemplateNode::Conditional { key, negated, children, .. } => {
                if *negated {
                    if check_negated && non_empty.contains(key) {
                        continue;
                    }
                } else if !non_empty.contains(key) {
                    continue;
                }
                if !template_is_empty(non_empty, children, check_negated) {
                    return false;
                }
            }
        }
    }
    true
}

/// Names of fields that count as non-empty for card generation, special fields included.
pub fn non_empty_field_names(nt: &Notetype, fields: &[String], tags: &[String]) -> HashSet<String> {
    let mut set = HashSet::new();
    let real: HashSet<&str> = nt.fields.iter().map(|f| f.name.as_str()).collect();
    for f in &nt.fields {
        let value = usize::try_from(f.ord).ok().and_then(|i| fields.get(i)).map(String::as_str).unwrap_or("");
        if !field_is_empty(value) {
            set.insert(f.name.clone());
        }
    }
    for s in SPECIAL_FIELDS {
        if s == "FrontSide" || real.contains(s) {
            continue;
        }
        if s == "Tags" && tags.is_empty() {
            continue;
        }
        set.insert(s.to_string());
    }
    set
}

/// Card ordinals of a cloze note: distinct cloze numbers (> 0) in any field, ord = min(N-1, 499).
pub fn cloze_card_ords(fields: &[String]) -> Vec<i64> {
    let mut ords: Vec<i64> =
        fields.iter().flat_map(|f| super::cloze::cloze_numbers_in_string(f)).map(|n| (n - 1).min(MAX_CLOZE_ORD)).collect();
    ords.sort_unstable();
    ords.dedup();
    ords
}

#[derive(Debug, Clone, Default)]
pub struct CardOrdsOptions<'a> {
    /// Note tags (only affect templates that use {{Tags}}). Default none.
    pub tags: &'a [String],
    /// Force ord 0 when nothing would be generated (Anki does this for brand-new notes). Default true.
    pub ensure_not_empty: Option<bool>,
}

/// Ordinals (0-based) of the cards this note should have, ascending. With `ensure_not_empty`
/// (default) the result is never empty. When updating an existing note, Anki only *adds* the
/// missing ords and never deletes cards; callers decide removal policy.
pub fn card_ords_for_note(nt: &Notetype, fields: &[String], opts: &CardOrdsOptions<'_>) -> Vec<i64> {
    let ensure = opts.ensure_not_empty.unwrap_or(true);
    let ords: Vec<i64> = if nt.kind == NotetypeKind::Cloze {
        cloze_card_ords(fields)
    } else {
        let non_empty = non_empty_field_names(nt, fields, opts.tags);
        let mut templates: Vec<_> = nt.templates.iter().collect();
        templates.sort_by_key(|t| t.ord);
        let mut out = Vec::new();
        for t in templates {
            let parsed = parse_template_cached(&t.qfmt);
            if parsed.has_errors() {
                continue; // Anki: unparsable template never generates
            }
            if !template_is_empty(&non_empty, &parsed.nodes, true) {
                out.push(t.ord);
            }
        }
        out
    };
    if ords.is_empty() && ensure {
        vec![0]
    } else {
        ords
    }
}

/// Whether the question side of `template_ord` renders anything for these fields (Anki "front is blank" check).
pub fn question_is_blank(nt: &Notetype, template_ord: i64, fields: &[String], tags: &[String]) -> bool {
    if nt.kind == NotetypeKind::Cloze {
        return !cloze_card_ords(fields).contains(&template_ord);
    }
    let Some(t) = nt.templates.iter().find(|x| x.ord == template_ord) else { return true };
    template_is_empty(&non_empty_field_names(nt, fields, tags), &parse_template_cached(&t.qfmt).nodes, true)
}

/// A field name with its ordinal, for `template_requirements`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldRef {
    pub name: String,
    pub ord: i64,
}

/// Anki `ParsedTemplate::requirements` for one parsed question template over the real fields.
pub fn template_requirements(nodes: &[TemplateNode], field_map: &[FieldRef]) -> (ReqKind, Vec<i64>) {
    let renders = |set: &HashSet<String>| !template_is_empty(set, nodes, false);
    let mut any: Vec<i64> = Vec::new();
    for f in field_map {
        if renders(&HashSet::from([f.name.clone()])) {
            any.push(f.ord);
        }
    }
    if !any.is_empty() {
        any.sort_unstable();
        any.dedup();
        return (ReqKind::Any, any);
    }
    let mut all: Vec<i64> = field_map.iter().map(|f| f.ord).collect();
    let mut non_empty: HashSet<String> = field_map.iter().map(|f| f.name.clone()).collect();
    for f in field_map {
        non_empty.remove(&f.name);
        if renders(&non_empty) {
            all.retain(|o| *o != f.ord);
        }
        non_empty.insert(f.name.clone());
    }
    if !all.is_empty() && renders(&non_empty) {
        all.sort_unstable();
        all.dedup();
        return (ReqKind::All, all);
    }
    (ReqKind::None, Vec::new())
}

/// Anki's stored `req` (for old clients): `[ord, 'any'|'all'|'none', fieldOrds]` per template.
pub fn compute_req(nt: &Notetype) -> Vec<Requirement> {
    let field_map: Vec<FieldRef> = nt.fields.iter().map(|f| FieldRef { name: f.name.clone(), ord: f.ord }).collect();
    let mut templates: Vec<_> = nt.templates.iter().collect();
    templates.sort_by_key(|t| t.ord);
    templates
        .into_iter()
        .map(|t| {
            let parsed = parse_template_cached(&t.qfmt);
            if parsed.has_errors() {
                return Requirement(t.ord, ReqKind::None, Vec::new());
            }
            let (kind, ords) = template_requirements(&parsed.nodes, &field_map);
            Requirement(t.ord, kind, ords)
        })
        .collect()
}

/// `collection/notetypes.ts` `ordIsValid`: a card ordinal this note type can hold.
pub fn ord_is_valid(nt: &Notetype, ord: i64) -> bool {
    if nt.kind == NotetypeKind::Cloze {
        (0..=MAX_CLOZE_ORD).contains(&ord)
    } else {
        nt.templates.iter().any(|t| t.ord == ord)
    }
}

/// `collection/notetypes.ts` `padFields`: the note's fields extended with '' up to the note type's field count.
pub fn pad_fields(nt: &Notetype, fields: &[String]) -> Vec<String> {
    let mut out = fields.to_vec();
    while out.len() < nt.fields.len() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
pub(crate) mod test_helpers {
    //! Port of recall `tests/unit/template/helpers.ts`.
    use crate::types::*;

    pub struct TemplateSpec<'a> {
        pub name: &'a str,
        pub qfmt: &'a str,
        pub afmt: &'a str,
    }

    pub fn tpl<'a>(name: &'a str, qfmt: &'a str, afmt: &'a str) -> TemplateSpec<'a> {
        TemplateSpec { name, qfmt, afmt }
    }

    pub fn make_notetype(name: &str, kind: NotetypeKind, fields: &[&str], templates: &[TemplateSpec<'_>]) -> Notetype {
        Notetype {
            id: 1_700_000_000_000,
            name: name.to_string(),
            kind,
            css: ".card { font-family: arial; }".to_string(),
            latex_pre: String::new(),
            latex_post: String::new(),
            sort_field: 0,
            fields: fields
                .iter()
                .enumerate()
                .map(|(ord, n)| Field { ord: ord as i64, name: n.to_string(), config: FieldConfig::default() })
                .collect(),
            templates: templates
                .iter()
                .enumerate()
                .map(|(ord, t)| Template {
                    ord: ord as i64,
                    name: t.name.to_string(),
                    qfmt: t.qfmt.to_string(),
                    afmt: t.afmt.to_string(),
                    bqfmt: String::new(),
                    bafmt: String::new(),
                    config: TemplateConfig::default(),
                })
                .collect(),
            config: NotetypeConfig::default(),
            mtime: 0,
            usn: -1,
        }
    }

    pub fn standard(fields: &[&str], templates: &[TemplateSpec<'_>]) -> Notetype {
        make_notetype("Basic", NotetypeKind::Standard, fields, templates)
    }

    pub fn basic() -> Notetype {
        standard(&["Front", "Back"], &[tpl("Card 1", "{{Front}}", "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}")])
    }

    pub fn cloze() -> Notetype {
        make_notetype(
            "Cloze",
            NotetypeKind::Cloze,
            &["Text", "Back Extra"],
            &[tpl("Cloze", "{{cloze:Text}}", "{{cloze:Text}}<br>\n{{Back Extra}}")],
        )
    }

    pub fn media_url(name: &str) -> String {
        format!("/media/{}", crate::template::math::percent_encode(name))
    }

    pub fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::template::parse::parse_template;

    fn reversed() -> Notetype {
        standard(&["Front", "Back"], &[tpl("Card 1", "{{Front}}", "{{Back}}"), tpl("Card 2", "{{Back}}", "{{Front}}")])
    }
    fn optional_reverse() -> Notetype {
        standard(
            &["Front", "Back", "Add Reverse"],
            &[tpl("Card 1", "{{Front}}", "{{Back}}"), tpl("Card 2", "{{#Add Reverse}}{{Back}}{{/Add Reverse}}", "{{Front}}")],
        )
    }
    fn ords(nt: &Notetype, fields: &[&str]) -> Vec<i64> {
        card_ords_for_note(nt, &strs(fields), &CardOrdsOptions::default())
    }
    fn no_ensure<'a>() -> CardOrdsOptions<'a> {
        CardOrdsOptions { tags: &[], ensure_not_empty: Some(false) }
    }
    fn req(ord: i64, kind: ReqKind, ords: &[i64]) -> Requirement {
        Requirement(ord, kind, ords.to_vec())
    }

    #[test]
    fn treats_whitespace_br_div_as_empty_but_nbsp_and_text_as_content() {
        let nt = basic();
        for v in ["", " ", "\n\t", "<br>", "<br/>", "<br />", "<div></div>", "<DIV> <BR></DIV>"] {
            assert!(!non_empty_field_names(&nt, &strs(&[v, "x"]), &[]).contains("Front"), "{v:?}");
        }
        for v in ["a", "&nbsp;", "<b></b>", "<img src=\"a.jpg\">", "\u{a0}"] {
            assert!(non_empty_field_names(&nt, &strs(&[v, "x"]), &[]).contains("Front"), "{v:?}");
        }
    }

    #[test]
    fn includes_special_fields_except_frontside_tags_only_with_tags_unless_shadowed() {
        let set = non_empty_field_names(&basic(), &strs(&["", ""]), &[]);
        let mut names: Vec<&str> = set.iter().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["Card", "CardFlag", "CardID", "Deck", "Subdeck", "Type"]);
        assert!(non_empty_field_names(&basic(), &strs(&["", ""]), &strs(&["t"])).contains("Tags"));
        let shadow = standard(&["Deck"], &[tpl("c", "{{Deck}}", "")]);
        assert!(!non_empty_field_names(&shadow, &strs(&[""]), &[]).contains("Deck"));
    }

    #[test]
    fn template_is_empty_ignores_text_and_comments() {
        assert!(template_is_empty(&HashSet::new(), &parse_template("hello <!-- {{A}} -->").nodes, true));
    }

    #[test]
    fn negated_conditionals_count_only_when_the_key_is_empty() {
        let n = parse_template("{{^A}}{{B}}{{/A}}").nodes;
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
        assert!(!template_is_empty(&set(&["B"]), &n, true));
        assert!(template_is_empty(&set(&["A", "B"]), &n, true));
        assert!(!template_is_empty(&set(&["A", "B"]), &n, false));
    }

    #[test]
    fn filters_do_not_change_the_key() {
        assert!(!template_is_empty(&HashSet::from(["F".to_string()]), &parse_template("{{text:hint:F}}").nodes, true));
    }

    #[test]
    fn basic_and_reversed() {
        assert_eq!(ords(&basic(), &["a", ""]), [0]);
        assert_eq!(ords(&reversed(), &["a", "b"]), [0, 1]);
        assert_eq!(ords(&reversed(), &["", "b"]), [1]);
        assert_eq!(ords(&reversed(), &["a", "<br>"]), [0]);
    }

    #[test]
    fn optional_reverse_uses_the_conditional() {
        assert_eq!(ords(&optional_reverse(), &["a", "b", ""]), [0]);
        assert_eq!(ords(&optional_reverse(), &["a", "b", "y"]), [0, 1]);
        // conditional set but the inner field empty → nothing rendered
        assert_eq!(ords(&optional_reverse(), &["a", "", "y"]), [0]);
    }

    #[test]
    fn frontside_on_the_question_is_never_non_empty() {
        let nt = standard(&["A"], &[tpl("c1", "{{A}}", ""), tpl("c2", "{{FrontSide}}", "")]);
        assert_eq!(ords(&nt, &["x"]), [0]);
    }

    #[test]
    fn a_template_that_only_uses_special_fields_always_generates() {
        let nt = standard(&["A"], &[tpl("c1", "{{A}}", ""), tpl("c2", "Deck: {{Deck}}", "")]);
        assert_eq!(ords(&nt, &[""]), [1]);
    }

    #[test]
    fn tags_counts_only_when_the_note_has_tags() {
        let nt = standard(&["A"], &[tpl("c1", "{{A}}", ""), tpl("c2", "{{#Tags}}{{Tags}}{{/Tags}}", "")]);
        let x = strs(&["x"]);
        assert_eq!(card_ords_for_note(&nt, &x, &CardOrdsOptions { tags: &[], ensure_not_empty: None }), [0]);
        let t = strs(&["t"]);
        assert_eq!(card_ords_for_note(&nt, &x, &CardOrdsOptions { tags: &t, ensure_not_empty: None }), [0, 1]);
    }

    #[test]
    fn negated_section_on_the_front() {
        let nt = standard(&["A", "B"], &[tpl("c", "{{^A}}{{B}}{{/A}}", "")]);
        assert_eq!(card_ords_for_note(&nt, &strs(&["", "b"]), &no_ensure()), [0]);
        assert_eq!(card_ords_for_note(&nt, &strs(&["a", "b"]), &no_ensure()), Vec::<i64>::new());
    }

    #[test]
    fn ensure_not_empty_forces_ord_0_for_a_note_with_no_card() {
        assert_eq!(ords(&reversed(), &["", ""]), [0]);
        assert_eq!(card_ords_for_note(&reversed(), &strs(&["", ""]), &no_ensure()), Vec::<i64>::new());
    }

    #[test]
    fn a_broken_template_never_generates() {
        let nt = standard(&["A"], &[tpl("c1", "{{A}}", ""), tpl("c2", "{{#A}}{{A}}", "")]);
        assert_eq!(ords(&nt, &["x"]), [0]);
    }

    #[test]
    fn cloze_one_card_per_cloze_number_from_any_field_nested_included() {
        assert_eq!(ords(&cloze(), &["{{c1::a}} {{c3::b {{c2::c}}}}", ""]), [0, 1, 2]);
        assert_eq!(ords(&cloze(), &["plain", "extra {{c4::x}}"]), [3]);
    }

    #[test]
    fn cloze_multi_ordinal_clozes_and_c0() {
        assert_eq!(ords(&cloze(), &["{{c1,3::a}} {{c0::b}}", ""]), [0, 2]);
    }

    #[test]
    fn cloze_anki_vector() {
        assert_eq!(ords(&cloze(), &["{{c2::te}}{{c1::s}}t{{", ""]), [0, 1]);
    }

    #[test]
    fn cloze_caps_ord_at_499_and_forces_card_0_when_there_is_no_cloze() {
        assert_eq!(ords(&cloze(), &["{{c900::x}}", ""]), [499]);
        assert_eq!(ords(&cloze(), &["none", ""]), [0]);
        assert_eq!(card_ords_for_note(&cloze(), &strs(&["{{c1::unclosed", ""]), &no_ensure()), Vec::<i64>::new());
    }

    #[test]
    fn question_is_blank_normal_and_cloze() {
        assert!(question_is_blank(&reversed(), 1, &strs(&["a", ""]), &[]));
        assert!(!question_is_blank(&reversed(), 0, &strs(&["a", ""]), &[]));
        assert!(question_is_blank(&cloze(), 1, &strs(&["{{c1::a}}", ""]), &[]));
        assert!(question_is_blank(&reversed(), 7, &strs(&["a", ""]), &[]));
    }

    #[test]
    fn compute_req_any_for_single_fields() {
        assert_eq!(compute_req(&basic()), [req(0, ReqKind::Any, &[0])]);
        assert_eq!(compute_req(&reversed()), [req(0, ReqKind::Any, &[0]), req(1, ReqKind::Any, &[1])]);
    }

    #[test]
    fn compute_req_all_when_every_field_is_needed() {
        let nt = standard(&["A", "B", "C"], &[tpl("c", "{{#A}}{{#B}}{{C}}{{/B}}{{/A}}", "")]);
        assert_eq!(compute_req(&nt), [req(0, ReqKind::All, &[0, 1, 2])]);
        assert_eq!(compute_req(&optional_reverse())[1], req(1, ReqKind::All, &[1, 2]));
    }

    #[test]
    fn compute_req_any_over_several_fields() {
        let nt = standard(&["A", "B", "C"], &[tpl("c", "{{A}}{{B}}", "")]);
        assert_eq!(compute_req(&nt), [req(0, ReqKind::Any, &[0, 1])]);
    }

    #[test]
    fn compute_req_negated_conditionals_resolve_to_children() {
        let nt = standard(&["A", "B"], &[tpl("c", "{{^A}}{{B}}{{/A}}", "")]);
        assert_eq!(compute_req(&nt), [req(0, ReqKind::Any, &[1])]);
    }

    #[test]
    fn compute_req_none_for_templates_with_only_special_fields_or_broken_syntax() {
        let nt = standard(&["A"], &[tpl("c", "{{Deck}}", ""), tpl("d", "{{#A}}", "")]);
        assert_eq!(compute_req(&nt), [req(0, ReqKind::None, &[]), req(1, ReqKind::None, &[])]);
    }

    #[test]
    fn ord_is_valid_and_pad_fields() {
        assert!(
            ord_is_valid(&cloze(), 0)
                && ord_is_valid(&cloze(), 499)
                && !ord_is_valid(&cloze(), 500)
                && !ord_is_valid(&cloze(), -1)
        );
        assert!(ord_is_valid(&reversed(), 1) && !ord_is_valid(&reversed(), 2));
        assert_eq!(pad_fields(&basic(), &strs(&["a"])), strs(&["a", ""]));
        assert_eq!(pad_fields(&basic(), &strs(&["a", "b", "c"])), strs(&["a", "b", "c"]));
    }
}
