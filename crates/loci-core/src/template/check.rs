//! Template validation for the note-type editor: errors
//! shown inline with positions. Mirrors the checks Anki runs when saving a note type: syntax,
//! unknown fields, a front that can never render anything, and cloze note types without a cloze
//! filter.

use crate::types::NotetypeKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::generate::template_is_empty;
use super::parse::{
    check_field_references, has_cloze_replacement, parse_template, Severity, TemplateError, TemplateErrorCode, SPECIAL_FIELDS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Front,
    Back,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateIssue {
    #[serde(flatten)]
    pub error: TemplateError,
    pub side: Side,
    /// 1-based line and column of `error.start`
    pub line: usize,
    pub column: usize,
}

/// 1-based line/column of a character offset.
pub fn line_column(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for ch in src.chars().take(offset) {
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Validates a template pair against a note type's fields. Errors first, then warnings.
pub fn check_template(field_names: &[String], kind: NotetypeKind, qfmt: &str, afmt: &str) -> Vec<TemplateIssue> {
    let mut issues: Vec<TemplateIssue> = Vec::new();
    let add = |issues: &mut Vec<TemplateIssue>, side: Side, src: &str, list: Vec<TemplateError>| {
        for e in list {
            let (line, column) = line_column(src, e.start);
            issues.push(TemplateIssue { error: e, side, line, column });
        }
    };
    let q = parse_template(qfmt);
    let a = parse_template(afmt);
    add(&mut issues, Side::Front, qfmt, q.errors.clone());
    add(&mut issues, Side::Front, qfmt, check_field_references(&q, field_names));
    add(&mut issues, Side::Back, afmt, a.errors.clone());
    add(&mut issues, Side::Back, afmt, check_field_references(&a, field_names));

    if q.errors.is_empty() {
        if kind == NotetypeKind::Cloze {
            if !has_cloze_replacement(&q.nodes) {
                add(
                    &mut issues,
                    Side::Front,
                    qfmt,
                    vec![TemplateError {
                        code: TemplateErrorCode::NoClozeFilter,
                        severity: Severity::Error,
                        start: 0,
                        end: qfmt.chars().count(),
                        key: None,
                        message: "Cloze note types need a {{cloze:Field}} on the front.".to_string(),
                    }],
                );
            }
        } else {
            let mut all: HashSet<String> = field_names.iter().cloned().collect();
            all.extend(SPECIAL_FIELDS.iter().filter(|s| **s != "FrontSide").map(|s| s.to_string()));
            if template_is_empty(&all, &q.nodes, false) {
                add(
                    &mut issues,
                    Side::Front,
                    qfmt,
                    vec![TemplateError {
                        code: TemplateErrorCode::NoFrontField,
                        severity: Severity::Error,
                        start: 0,
                        end: qfmt.chars().count(),
                        key: None,
                        message: "The front of this card type never shows a field, so no card can be made from it.".to_string(),
                    }],
                );
            }
        }
    }
    issues.sort_by_key(|i| i.error.severity); // stable: errors first, then warnings
    issues
}

/// Human messages for `CollectionApi.checkTemplate`: "Front template, line 2, column 5: …".
pub fn template_issue_messages(issues: &[TemplateIssue]) -> Vec<String> {
    issues
        .iter()
        .map(|i| {
            format!(
                "{} template, line {}, column {}: {}",
                match i.side {
                    Side::Front => "Front",
                    Side::Back => "Back",
                },
                i.line,
                i.column,
                i.error.message
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["Front".to_string(), "Back".to_string()]
    }

    #[test]
    fn accepts_the_stock_basic_template() {
        assert!(check_template(&names(), NotetypeKind::Standard, "{{Front}}", "{{FrontSide}}<hr id=answer>{{Back}}").is_empty());
    }

    #[test]
    fn reports_positions_as_line_column() {
        let issues = check_template(&names(), NotetypeKind::Standard, "{{Front}}\n  {{Bakc}}", "{{FrontSide}}");
        assert_eq!(issues[0].side, Side::Front);
        assert_eq!((issues[0].line, issues[0].column), (2, 3));
        assert_eq!(issues[0].error.code, TemplateErrorCode::UnknownField);
        assert_eq!(
            template_issue_messages(&issues)[0],
            "Front template, line 2, column 3: Found '{{Bakc}}', but there is no field called 'Bakc'."
        );
    }

    #[test]
    fn rejects_a_front_without_any_field() {
        let codes: Vec<_> = check_template(&names(), NotetypeKind::Standard, "just text {{FrontSide}}", "{{Back}}")
            .iter()
            .map(|i| i.error.code)
            .collect();
        assert!(codes.contains(&TemplateErrorCode::NoFrontField));
    }

    #[test]
    fn requires_a_cloze_filter_on_cloze_note_types() {
        let codes: Vec<_> =
            check_template(&names(), NotetypeKind::Cloze, "{{Front}}", "{{Back}}").iter().map(|i| i.error.code).collect();
        assert_eq!(codes, [TemplateErrorCode::NoClozeFilter]);
        assert!(check_template(&names(), NotetypeKind::Cloze, "{{cloze:Front}}", "{{cloze:Front}}").is_empty());
    }

    #[test]
    fn reports_structural_errors_on_the_back() {
        let issues = check_template(&names(), NotetypeKind::Standard, "{{Front}}", "{{#Back}}x");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].side, Side::Back);
        assert_eq!(issues[0].error.code, TemplateErrorCode::UnclosedSection);
    }

    #[test]
    fn warnings_sort_after_errors() {
        let issues = check_template(&names(), NotetypeKind::Standard, "{{tts en_US:Front}}{{Nope}}", "");
        assert_eq!(issues.iter().map(|i| i.error.severity).collect::<Vec<_>>(), [Severity::Error, Severity::Warning]);
        assert_eq!(line_column("ab\ncd", 4), (2, 2));
    }
}
