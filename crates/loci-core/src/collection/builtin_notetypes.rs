//! Built-in note types, with Anki's stock field names and templates (rslib/src/notetype/stock.rs
//! and rslib/src/image_occlusion/notetype.rs; research/anki-format.md §4.1, §5.4), so notes
//! round-trip through Anki unchanged. CSS is
//! deliberately minimal: the card renderer applies the app's designed stylesheet; this only
//! carries Anki-meaningful basics.

use crate::template::compute_req;
use crate::types::*;

/// Anki's default LaTeX header (rslib/src/notetype/header.tex).
pub const DEFAULT_LATEX_PRE: &str = "\\documentclass[12pt]{article}\n\\special{papersize=3in,5in}\n\\usepackage[utf8]{inputenc}\n\\usepackage{amssymb,amsmath}\n\\pagestyle{empty}\n\\setlength{\\parindent}{0in}\n\\begin{document}\n";
/// Anki's default LaTeX footer.
pub const DEFAULT_LATEX_POST: &str = "\\end{document}";

/// Minimal card CSS: alignment only; fonts and colours come from the app's card stylesheet.
pub const BASIC_CSS: &str = ".card {\n  text-align: center;\n}\n";
/// Cloze CSS adds Anki's `.cloze` emphasis (colour comes from the app stylesheet).
pub const CLOZE_CSS: &str = ".card {\n  text-align: center;\n}\n\n.cloze {\n  font-weight: bold;\n}\n";

const ANSWER_SEP: &str = "\n\n<hr id=answer>\n\n";

/// Anki's stock image-occlusion question template (English).
pub const IO_QFMT: &str = "{{#Header}}<div>{{Header}}</div>{{/Header}}\n<div style=\"display: none\">{{cloze:Occlusion}}</div>\n<div id=\"err\"></div>\n<div id=\"image-occlusion-container\">\n    {{Image}}\n    <canvas id=\"image-occlusion-canvas\"></canvas>\n</div>\n<script>\ntry {\n    anki.imageOcclusion.setup();\n} catch (exc) {\n    document.getElementById(\"err\").innerHTML = `Error loading image occlusion. Is your Anki version up to date?<br><br>${exc}`;\n}\n</script>\n";

/// Image-occlusion field names in Anki's order (ImageOcclusionField enum).
pub const IO_FIELDS: [&str; 5] = ["Occlusion", "Image", "Header", "Back Extra", "Comments"];

/// All built-in kinds in the order they are created in a new collection.
pub const BUILTIN_KINDS: [BuiltinKind; 5] =
    [BuiltinKind::Basic, BuiltinKind::BasicReversed, BuiltinKind::BasicTyped, BuiltinKind::Cloze, BuiltinKind::ImageOcclusion];

/// Anki's stock image-occlusion answer template: question + toggle button + Back Extra.
pub fn io_afmt() -> String {
    format!("{IO_QFMT}\n<div><button id=\"toggle\">Toggle Masks</button></div>\n{{{{#Back Extra}}}}<div>{{{{Back Extra}}}}</div>{{{{/Back Extra}}}}\n")
}

fn fields(names: &[&str]) -> Vec<Field> {
    names
        .iter()
        .enumerate()
        .map(|(ord, name)| Field { ord: ord as i64, name: name.to_string(), config: FieldConfig::default() })
        .collect()
}

fn template(ord: i64, name: &str, qfmt: &str, afmt: &str) -> Template {
    Template {
        ord,
        name: name.to_string(),
        qfmt: qfmt.to_string(),
        afmt: afmt.to_string(),
        bqfmt: String::new(),
        bafmt: String::new(),
        config: TemplateConfig::default(),
    }
}

/// Builds the draft for one built-in kind (id/mtime/usn are assigned when saved), with `config.req` computed.
pub fn builtin_notetype(kind: BuiltinKind) -> NotetypeInput {
    let base = |name: &str, k: NotetypeKind, css: &str, f: Vec<Field>, t: Vec<Template>, config: NotetypeConfig| NotetypeInput {
        id: None,
        name: name.to_string(),
        kind: k,
        css: css.to_string(),
        latex_pre: DEFAULT_LATEX_PRE.to_string(),
        latex_post: DEFAULT_LATEX_POST.to_string(),
        sort_field: 0,
        fields: f,
        templates: t,
        config,
    };
    let cfg = NotetypeConfig { builtin: Some(kind), ..Default::default() };
    let mut nt = match kind {
        BuiltinKind::Basic => base(
            "Basic",
            NotetypeKind::Standard,
            BASIC_CSS,
            fields(&["Front", "Back"]),
            vec![template(0, "Card 1", "{{Front}}", &format!("{{{{FrontSide}}}}{ANSWER_SEP}{{{{Back}}}}"))],
            cfg,
        ),
        BuiltinKind::BasicReversed => base(
            "Basic (and reversed card)",
            NotetypeKind::Standard,
            BASIC_CSS,
            fields(&["Front", "Back"]),
            vec![
                template(0, "Card 1", "{{Front}}", &format!("{{{{FrontSide}}}}{ANSWER_SEP}{{{{Back}}}}")),
                template(1, "Card 2", "{{Back}}", &format!("{{{{FrontSide}}}}{ANSWER_SEP}{{{{Front}}}}")),
            ],
            cfg,
        ),
        BuiltinKind::BasicTyped => base(
            "Basic (type in the answer)",
            NotetypeKind::Standard,
            BASIC_CSS,
            fields(&["Front", "Back"]),
            vec![template(0, "Card 1", "{{Front}}\n\n{{type:Back}}", &format!("{{{{Front}}}}{ANSWER_SEP}{{{{type:Back}}}}"))],
            cfg,
        ),
        BuiltinKind::Cloze => base(
            "Cloze",
            NotetypeKind::Cloze,
            CLOZE_CSS,
            fields(&["Text", "Back Extra"]),
            vec![template(0, "Cloze", "{{cloze:Text}}", "{{cloze:Text}}<br>\n{{Back Extra}}")],
            NotetypeConfig { cloze_one_by_one: Some(false), ..cfg },
        ),
        BuiltinKind::ImageOcclusion => base(
            "Image Occlusion",
            NotetypeKind::Cloze,
            CLOZE_CSS,
            fields(&IO_FIELDS),
            vec![template(0, "Image Occlusion", IO_QFMT, &io_afmt())],
            NotetypeConfig { occlusion_mode: Some(OcclusionMode::HideAll), ..cfg },
        ),
    };
    nt.config.req = compute_req(&input_as_notetype(&nt));
    nt
}

/// A `Notetype` view of an input (id 0, no mtime) for the pure template helpers.
pub fn input_as_notetype(nt: &NotetypeInput) -> Notetype {
    Notetype {
        id: nt.id.unwrap_or(0),
        name: nt.name.clone(),
        kind: nt.kind,
        css: nt.css.clone(),
        latex_pre: nt.latex_pre.clone(),
        latex_post: nt.latex_post.clone(),
        sort_field: nt.sort_field,
        fields: nt.fields.clone(),
        templates: nt.templates.clone(),
        config: nt.config.clone(),
        mtime: 0,
        usn: -1,
    }
}

/// `NotetypeInput` from a stored note type (for `saveNotetype({...nt})` round trips).
pub fn notetype_to_input(nt: &Notetype) -> NotetypeInput {
    NotetypeInput {
        id: Some(nt.id),
        name: nt.name.clone(),
        kind: nt.kind,
        css: nt.css.clone(),
        latex_pre: nt.latex_pre.clone(),
        latex_post: nt.latex_post.clone(),
        sort_field: nt.sort_field,
        fields: nt.fields.clone(),
        templates: nt.templates.clone(),
        config: nt.config.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_carry_anki_stock_shapes_and_req() {
        let basic = builtin_notetype(BuiltinKind::Basic);
        assert_eq!(basic.templates[0].afmt, "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}");
        assert_eq!(basic.config.req, vec![Requirement(0, ReqKind::Any, vec![0])]);
        let rev = builtin_notetype(BuiltinKind::BasicReversed);
        assert_eq!(rev.config.req, vec![Requirement(0, ReqKind::Any, vec![0]), Requirement(1, ReqKind::Any, vec![1])]);
        let io = builtin_notetype(BuiltinKind::ImageOcclusion);
        assert!(io.templates[0].afmt.starts_with(IO_QFMT));
        assert!(io.templates[0].afmt.contains("{{#Back Extra}}<div>{{Back Extra}}</div>{{/Back Extra}}"));
        assert_eq!(io.fields.len(), 5);
        assert_eq!(builtin_notetype(BuiltinKind::Cloze).config.cloze_one_by_one, Some(false));
        assert_eq!(CLOZE_CSS, format!("{BASIC_CSS}\n.cloze {{\n  font-weight: bold;\n}}\n"));
        assert_eq!(BUILTIN_KINDS.len(), 5);
    }
}
