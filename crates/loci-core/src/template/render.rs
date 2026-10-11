//! Card renderer: Anki template + note → `RenderedCard` (recall `src/core/template/render.ts`).
//!
//! Output HTML is NOT sanitised; the UI passes each side through DOMPurify with its own config
//! (`sanitize-config.ts`, kept on the UI side) before inserting it into the card's shadow root.
//!
//! Post-processing applied to each rendered side, in order:
//!  1. `<style>` blocks are moved out of the HTML into `RenderedCard.css` (scoped like note-type CSS),
//!  2. `[sound:x]` → `<button class="loci-audio" data-src=… data-name=x data-index=i>`; names → `media[]`,
//!  3. `src`/`data`/`poster` of img/audio/video/source/track/object rewritten through `media_url`,
//!  4. maths delimiters → `<span class="loci-math" data-display="0|1">` (see math.rs),
//!  5. only the first `{{type:}}` placeholder of a side is kept (Anki replaces the first and drops the rest).
//!
//! `data-cloze` attributes (revealed text of a hidden cloze) get steps 2–4 too, so the UI can
//! swap them in for one-by-one reveal.

use crate::i18n::t;
use crate::text::{decode_entities, field_is_empty, strip_html};
use crate::types::{CardId, Dir, NoteId, Notetype, NotetypeKind, OcclusionMode, OcclusionPayload, RenderedCard, RenderedSide};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::cloze::{cloze_filter, cloze_numbers_in_string, extract_cloze_for_typing, reveal_cloze_text_only, RevealOptions};
use super::css::{scope_card_css, CardCssOptions};
use super::generate::{non_empty_field_names, template_is_empty};
use super::math::{escape_attr, escape_html, normalize_math, replace_sound_tags, rewrite_media_urls};
use super::occlusion::occlusion_payload;
use super::parse::{check_field_references, parse_template_cached, ParsedTemplate, Severity, TemplateNode};
use super::typed::expected_text_from_html;

pub struct RenderCardInput<'a> {
    pub notetype: &'a Notetype,
    pub note_id: Option<NoteId>,
    pub fields: &'a [String],
    pub tags: &'a [String],
    pub card_id: Option<CardId>,
    /// template ordinal (normal) or cloze number − 1 (cloze)
    pub card_ord: i64,
    /// low 3 bits = flag
    pub card_flags: i64,
    /// full deck name with `::`
    pub deck_name: &'a str,
    /// resolves a collection media filename to a URL the card can load
    pub media_url: &'a dyn Fn(&str) -> String,
    /// cloze one-by-one markup; defaults to notetype.config.cloze_one_by_one
    pub one_by_one: Option<bool>,
}

struct Ctx<'a, 'f> {
    fields: &'a HashMap<String, String>,
    non_empty: &'a HashSet<String>,
    question: bool,
    /// 1-based cloze ordinal of this card
    cloze_ord: i64,
    front_side: Option<&'f str>,
    one_by_one: bool,
    cloze_ords: HashSet<i64>,
    warnings: Vec<String>,
    /// set when a type placeholder has been emitted on this side
    type_answer: Option<String>,
}

const TYPE_MODIFIERS: [&str; 3] = ["cloze", "nc", "ci"];

/// Anki's `type` handling: returns the modifiers when `type` is the outermost filter.
fn type_filter(filters: &[String]) -> Option<HashSet<&str>> {
    let outer = filters.last()?;
    match outer.as_str() {
        "type-cloze" => return Some(HashSet::from(["cloze"])),
        "type-nc" => return Some(HashSet::from(["nc"])),
        "type" => {}
        _ => return None,
    }
    let mods = &filters[..filters.len() - 1];
    if !mods.iter().all(|m| TYPE_MODIFIERS.contains(&m.as_str())) {
        return None;
    }
    Some(mods.iter().map(String::as_str).collect())
}

/// Short stable hash (FNV-1a, 32 bit, over UTF-16 units like the TypeScript) for hint element ids.
fn hash_id(s: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for unit in s.encode_utf16() {
        h ^= unit as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{h:08x}")
}

static FURIGANA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" ?([^ >]+?)\[(.+?)\]").expect("valid regex"));

fn furigana(text: &str, mode: &str) -> String {
    let text = text.replace("&nbsp;", " ");
    FURIGANA
        .replace_all(&text, |c: &regex::Captures| {
            let base = &c[1];
            let reading = &c[2];
            if reading.starts_with("sound:") {
                return c[0].to_string();
            }
            match mode {
                "kana" => reading.to_string(),
                "kanji" => base.to_string(),
                _ => format!("<ruby><rb>{base}</rb><rt>{reading}</rt></ruby>"),
            }
        })
        .into_owned()
}

/// Applies the filter chain (innermost first). Unknown filters stop processing, as in Anki without add-ons.
fn apply_filters(mut text: String, filters: &[String], key: &str, ctx: &mut Ctx<'_, '_>) -> String {
    for f in filters {
        match f.as_str() {
            "" => {}
            "text" => text = strip_html(&text),
            "cloze" => {
                ctx.cloze_ords.extend(cloze_numbers_in_string(&text));
                text = cloze_filter(
                    &text,
                    ctx.cloze_ord,
                    ctx.question,
                    RevealOptions { one_by_one: ctx.one_by_one, render_occlusion: None },
                );
            }
            "cloze-only" => text = reveal_cloze_text_only(&text, ctx.cloze_ord, ctx.question),
            "hint" => {
                if strip_html(&text).trim().is_empty() {
                    text = String::new();
                    continue;
                }
                let id = format!("hint{}", hash_id(&format!("{text}{key}")));
                text = format!(
                    "<a class=\"hint\" href=\"#\" data-hint=\"{id}\" draggable=\"false\">{}</a><div id=\"{id}\" class=\"hint\" style=\"display: none\">{text}</div>",
                    escape_html(key)
                );
            }
            "furigana" | "kana" | "kanji" => text = furigana(&text, f),
            _ => {
                if f == "tts" || f.starts_with("tts ") {
                    ctx.warnings.push(format!("{{{{tts}}}} on '{key}' is not supported and renders nothing."));
                    return String::new();
                }
                return text; // unknown filter: pass through
            }
        }
    }
    text
}

static TYPE_ANSWER_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<span class="loci-type" data-answer="([^"]*)""#).expect("valid regex"));

fn render_nodes(nodes: &[TemplateNode], ctx: &mut Ctx<'_, '_>) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            TemplateNode::Text { text, .. } | TemplateNode::Comment { text, .. } => out.push_str(text),
            TemplateNode::Conditional { key, negated, children, .. } => {
                let non_empty = ctx.non_empty.contains(key);
                if non_empty != *negated {
                    out.push_str(&render_nodes(children, ctx));
                }
            }
            TemplateNode::Replacement { key, filters, .. } => {
                if key == "FrontSide" && !ctx.fields.contains_key("FrontSide") {
                    if let Some(front) = ctx.front_side {
                        out.push_str(front);
                        if ctx.type_answer.is_none() {
                            if let Some(m) = TYPE_ANSWER_ATTR.captures(front) {
                                ctx.type_answer = Some(decode_entities(&m[1]));
                            }
                        }
                    }
                    continue;
                }
                let value = if key.is_empty() { String::new() } else { ctx.fields.get(key).cloned().unwrap_or_default() };
                if let Some(mods) = type_filter(filters) {
                    out.push_str(&type_placeholder(&value, key, &mods, ctx));
                    continue;
                }
                out.push_str(&apply_filters(value, filters, key, ctx));
            }
        }
    }
    out
}

fn type_placeholder(value: &str, key: &str, mods: &HashSet<&str>, ctx: &mut Ctx<'_, '_>) -> String {
    let expected_html = if mods.contains("cloze") { extract_cloze_for_typing(value, ctx.cloze_ord) } else { value.to_string() };
    let expected = expected_text_from_html(&expected_html);
    if ctx.type_answer.is_some() {
        return String::new(); // only the first one counts
    }
    let html = format!(
        "<span class=\"loci-type\" data-answer=\"{}\" data-field=\"{}\"{}{}{}></span>",
        escape_attr(&expected),
        escape_attr(key),
        if mods.contains("cloze") { " data-type-cloze=\"1\"" } else { "" },
        if mods.contains("nc") { " data-nc=\"1\"" } else { "" },
        if mods.contains("ci") { " data-ci=\"1\"" } else { "" }
    );
    ctx.type_answer = Some(expected);
    html
}

fn error_html(title: &str, message: &str) -> String {
    format!(
        "<div class=\"loci-template-error\" role=\"note\"><strong>{}</strong><br>{}</div>",
        escape_html(title),
        escape_html(message)
    )
}

/// Strong RTL scripts: Hebrew, Arabic, Syriac, Thaana, NKo, Samaritan, Mandaic, Arabic ext., presentation forms, RTL SMP blocks.
fn is_rtl_char(ch: char) -> bool {
    matches!(ch as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

static MATH_SPANS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<span class="loci-math"[^>]*>.*?</span>"#).expect("valid regex"));
static STYLE_SCRIPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style\b.*?</style>|<script\b.*?</script>").expect("valid regex"));
static SOUND_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[sound:[^\]]*\]").expect("valid regex"));
static STRONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}").expect("valid regex"));

/// `Rtl` when the first strong character of the text is right-to-left, else `Auto`.
pub fn detect_dir(html: &str) -> Dir {
    let s = MATH_SPANS.replace_all(html, " ");
    let s = STYLE_SCRIPT.replace_all(&s, " ");
    let s = SOUND_TAG.replace_all(&s, " ");
    let text = strip_html(&s);
    for ch in text.chars() {
        if is_rtl_char(ch) {
            return Dir::Rtl;
        }
        if STRONG.is_match(ch.encode_utf8(&mut [0u8; 4])) {
            return Dir::Auto;
        }
    }
    Dir::Auto
}

static STYLE_BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<style\b[^>]*>(.*?)</style>").expect("valid regex"));
static DATA_CLOZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\sdata-cloze=")([^"]*)""#).expect("valid regex"));

struct Post {
    html: String,
    media: Vec<String>,
    styles: Vec<String>,
}

fn post_process(html: &str, media_url: &dyn Fn(&str) -> String, label: &str) -> Post {
    let mut styles: Vec<String> = Vec::new();
    let html = STYLE_BLOCK
        .replace_all(html, |c: &regex::Captures| {
            styles.push(c[1].to_string());
            String::new()
        })
        .into_owned();
    let sound = replace_sound_tags(&html, media_url, label, 0);
    let html = rewrite_media_urls(&sound.html, media_url);
    let html = normalize_math(&html);
    // Revealed content of hidden clozes (attribute) gets the same treatment, so one-by-one can swap it in.
    let html = DATA_CLOZE
        .replace_all(&html, |c: &regex::Captures| {
            let inner = decode_entities(&c[2]);
            let inner = replace_sound_tags(&inner, media_url, label, 0).html;
            let inner = normalize_math(&rewrite_media_urls(&inner, media_url));
            format!("{}{}\"", &c[1], escape_attr(&inner))
        })
        .into_owned();
    Post { html, media: sound.media, styles }
}

static TYPE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<span class="loci-type"[^>]*></span>"#).expect("valid regex"));

/// Removes every `.loci-type` placeholder after the first (Anki keeps only one type box per side).
fn keep_first_type(html: &str) -> String {
    let mut seen = false;
    TYPE_SPAN
        .replace_all(html, |c: &regex::Captures| {
            if seen {
                return String::new();
            }
            seen = true;
            c[0].to_string()
        })
        .into_owned()
}

fn first_error(p: &ParsedTemplate, field_names: &[String]) -> Option<String> {
    p.errors
        .iter()
        .cloned()
        .chain(check_field_references(p, field_names))
        .find(|e| e.severity == Severity::Error)
        .map(|e| e.message)
}

/// Renders one card. Never fails for template problems; they are shown inside the card.
pub fn render_card(input: &RenderCardInput<'_>) -> RenderedCard {
    let nt = input.notetype;
    let media_url = input.media_url;
    let is_cloze = nt.kind == NotetypeKind::Cloze;
    let template =
        if is_cloze { nt.templates.iter().min_by_key(|t| t.ord) } else { nt.templates.iter().find(|x| x.ord == input.card_ord) };
    let label = t("study.playAudio", &[]);
    let css_opts = CardCssOptions { own_styling: nt.config.use_own_styling == Some(true) };
    let empty_side =
        |html: String| RenderedSide { html, media: Vec::new(), has_type_input: false, type_answer: None, cloze_ords: None };
    let base = |front: RenderedSide, back: RenderedSide, css: String, dir: Dir| RenderedCard {
        card_id: input.card_id.unwrap_or(0),
        note_id: input.note_id.unwrap_or(0),
        notetype_id: nt.id,
        template_name: template.map(|t| t.name.clone()).unwrap_or_default(),
        front,
        back,
        css,
        dir,
        occlusion: None,
    };
    let Some(template) = template else {
        let msg =
            error_html(&t("template.errorTitle", &[]), &t("template.missingTemplate", &[("ord", (input.card_ord + 1).into())]));
        return base(empty_side(msg.clone()), empty_side(msg), scope_card_css(&nt.css, Some(media_url), css_opts), Dir::Auto);
    };

    // Field map: real fields first, then special fields that are not shadowed.
    let mut fields: HashMap<String, String> = HashMap::new();
    for f in &nt.fields {
        let value = usize::try_from(f.ord).ok().and_then(|i| input.fields.get(i)).cloned().unwrap_or_default();
        fields.insert(f.name.clone(), value);
    }
    let specials: [(&str, String); 7] = [
        ("Tags", input.tags.join(" ").trim().to_string()),
        ("Type", nt.name.clone()),
        ("Deck", input.deck_name.to_string()),
        ("Subdeck", input.deck_name.rsplit("::").next().unwrap_or(input.deck_name).to_string()),
        ("CardFlag", format!("flag{}", input.card_flags & 7)),
        ("Card", template.name.clone()),
        ("CardID", input.card_id.map(|id| id.to_string()).unwrap_or_default()),
    ];
    for (k, v) in specials {
        fields.entry(k.to_string()).or_insert(v);
    }
    let cloze_key = format!("c{}", input.card_ord + 1);
    fields.entry(cloze_key).or_insert_with(|| "1".to_string());
    let non_empty: HashSet<String> = fields.iter().filter(|(_, v)| !field_is_empty(v)).map(|(k, _)| k.clone()).collect();

    let field_names: Vec<String> = nt.fields.iter().map(|f| f.name.clone()).collect();
    let q_parsed = parse_template_cached(&template.qfmt);
    let a_parsed = parse_template_cached(&template.afmt);
    let one_by_one = input.one_by_one.or(nt.config.cloze_one_by_one).unwrap_or(false);
    fn ctx_for<'a, 'f>(
        fields: &'a HashMap<String, String>,
        non_empty: &'a HashSet<String>,
        cloze_ord: i64,
        one_by_one: bool,
        question: bool,
        front_side: Option<&'f str>,
    ) -> Ctx<'a, 'f> {
        Ctx {
            fields,
            non_empty,
            question,
            cloze_ord,
            front_side,
            one_by_one,
            cloze_ords: HashSet::new(),
            warnings: Vec::new(),
            type_answer: None,
        }
    }

    // Question
    let mut q_ctx = ctx_for(&fields, &non_empty, input.card_ord + 1, one_by_one, true, None);
    let q_html = if let Some(err) = first_error(&q_parsed, &field_names) {
        error_html(&t("template.errorTitle", &[]), &err)
    } else {
        let blank = if is_cloze {
            !nt.fields.iter().any(|f| {
                let value = usize::try_from(f.ord).ok().and_then(|i| input.fields.get(i)).map(String::as_str).unwrap_or("");
                cloze_numbers_in_string(value).contains(&(input.card_ord + 1))
            })
        } else {
            template_is_empty(&non_empty_field_names(nt, input.fields, input.tags), &q_parsed.nodes, true)
        };
        if blank && is_cloze {
            error_html(&t("template.noClozeTitle", &[("n", (input.card_ord + 1).into())]), &t("template.noClozeHelp", &[]))
        } else if blank {
            error_html(&t("template.blankFrontTitle", &[]), &t("template.blankFrontHelp", &[]))
        } else {
            render_nodes(&q_parsed.nodes, &mut q_ctx)
        }
    };

    // Answer
    let mut a_ctx = ctx_for(&fields, &non_empty, input.card_ord + 1, one_by_one, false, Some(&q_html));
    let a_html = match first_error(&a_parsed, &field_names) {
        Some(err) => error_html(&t("template.errorTitle", &[]), &err),
        None => render_nodes(&a_parsed.nodes, &mut a_ctx),
    };

    let q = post_process(&keep_first_type(&q_html), media_url, &label);
    let a = post_process(&keep_first_type(&a_html), media_url, &label);

    let side = |p: &Post, ctx: &Ctx<'_, '_>| {
        // has_type_input reflects what is actually present after "first one wins".
        let has_type_input = p.html.contains("class=\"loci-type\"");
        let mut cloze_ords: Vec<i64> = ctx.cloze_ords.iter().copied().collect();
        cloze_ords.sort_unstable();
        RenderedSide {
            html: p.html.clone(),
            media: p.media.clone(),
            has_type_input,
            type_answer: if has_type_input { ctx.type_answer.clone() } else { None },
            cloze_ords: if cloze_ords.is_empty() { None } else { Some(cloze_ords) },
        }
    };
    let front = side(&q, &q_ctx);
    let back = side(&a, &a_ctx);

    let mut styles: Vec<String> = Vec::new();
    for s in q.styles.iter().chain(a.styles.iter()) {
        if !styles.contains(s) {
            styles.push(s.clone());
        }
    }
    let mut all_css = vec![nt.css.clone()];
    all_css.extend(styles);
    let css = scope_card_css(&all_css.join("\n"), Some(media_url), css_opts);
    let dir = detect_dir(&q.html);
    let mut rendered = base(front, back, css, dir);

    if let Some(occ) = occlusion_for(nt, input.fields, input.card_ord) {
        let image = if occ.image.is_empty() { String::new() } else { media_url(&occ.image) };
        rendered.occlusion = Some(OcclusionPayload { image, ..occ });
    }
    rendered
}

/// Image-occlusion payload when the note type is IO (built-in flag, or cloze with Occlusion+Image fields holding IO clozes).
fn occlusion_for(nt: &Notetype, fields: &[String], ord: i64) -> Option<OcclusionPayload> {
    let is_builtin_io = nt.config.builtin == Some(crate::types::BuiltinKind::ImageOcclusion);
    let occ_field =
        nt.fields.iter().find(|f| f.name == "Occlusion").or_else(|| if is_builtin_io { nt.fields.first() } else { None })?;
    let img_field =
        nt.fields.iter().find(|f| f.name == "Image").or_else(|| if is_builtin_io { nt.fields.get(1) } else { None })?;
    let value = |ord: i64| usize::try_from(ord).ok().and_then(|i| fields.get(i)).map(String::as_str).unwrap_or("");
    let occ = value(occ_field.ord);
    if !is_builtin_io && !(nt.kind == NotetypeKind::Cloze && occ.contains("image-occlusion:")) {
        return None;
    }
    occlusion_payload(occ, value(img_field.ord), ord, nt.config.occlusion_mode.unwrap_or(OcclusionMode::HideOne))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::generate::test_helpers::*;
    use crate::types::BuiltinKind;

    struct R {
        note_id: Option<i64>,
        card_id: Option<i64>,
        ord: i64,
        flags: i64,
        tags: Vec<String>,
        deck: &'static str,
        one_by_one: Option<bool>,
    }

    fn defaults() -> R {
        R {
            note_id: Some(5),
            card_id: Some(9),
            ord: 0,
            flags: 3,
            tags: strs(&["t1", "p::c"]),
            deck: "Lang::French",
            one_by_one: None,
        }
    }

    fn render_with(nt: &Notetype, fields: &[&str], r: R) -> RenderedCard {
        let fields = strs(fields);
        render_card(&RenderCardInput {
            notetype: nt,
            note_id: r.note_id,
            fields: &fields,
            tags: &r.tags,
            card_id: r.card_id,
            card_ord: r.ord,
            card_flags: r.flags,
            deck_name: r.deck,
            media_url: &media_url,
            one_by_one: r.one_by_one,
        })
    }

    fn render(nt: &Notetype, fields: &[&str]) -> RenderedCard {
        render_with(nt, fields, defaults())
    }

    /// The cloze-style calls of render.test.ts: no ids, no tags, deck "D".
    fn render_plain(nt: &Notetype, fields: &[&str], ord: i64) -> RenderedCard {
        render_with(nt, fields, R { note_id: None, card_id: None, ord, flags: 0, tags: vec![], deck: "D", one_by_one: None })
    }

    #[test]
    fn renders_basic_with_frontside() {
        let r = render(&basic(), &["Q", "A"]);
        assert_eq!(r.front.html, "Q");
        assert_eq!(r.back.html, "Q\n\n<hr id=answer>\n\nA");
        assert_eq!(
            (r.card_id, r.note_id, r.notetype_id, r.template_name.as_str(), r.dir),
            (9, 5, 1_700_000_000_000, "Card 1", Dir::Auto)
        );
        assert!(!r.front.has_type_input);
    }

    #[test]
    fn special_fields() {
        let nt = make_notetype(
            "My Type",
            NotetypeKind::Standard,
            &["F"],
            &[tpl("Fwd", "{{F}}|{{Tags}}|{{Type}}|{{Deck}}|{{Subdeck}}|{{Card}}|{{CardFlag}}|{{CardID}}", "")],
        );
        assert_eq!(render(&nt, &["x"]).front.html, "x|t1 p::c|My Type|Lang::French|French|Fwd|flag3|9");
    }

    #[test]
    fn conditionals_negation_and_the_cn_pseudo_field() {
        let nt =
            standard(&["F", "G"], &[tpl("c", "{{F}}{{#G}}[g]{{/G}}{{^G}}[no g]{{/G}}{{#c1}}<c1>{{/c1}}{{#c2}}<c2>{{/c2}}", "")]);
        assert_eq!(render(&nt, &["x", ""]).front.html, "x[no g]<c1>");
        assert_eq!(render(&nt, &["x", "y"]).front.html, "x[g]<c1>");
    }

    #[test]
    fn text_hint_furigana_kana_kanji_filters_and_unknown_filters_pass_through() {
        let nt = standard(
            &["F", "H", "J"],
            &[tpl("c", "{{text:F}}|{{hint:H}}|{{furigana:J}}|{{kana:J}}|{{kanji:J}}|{{myaddon:F}}|{{text:myaddon:F}}", "")],
        );
        let html = render(&nt, &["<b>bold</b> &amp; x", "secret", "日本[にほん]語[ご]"]).front.html;
        let parts: Vec<&str> = html.split('|').collect();
        assert_eq!(parts[0], "bold & x");
        let hint = Regex::new(r##"^<a class="hint" href="#" data-hint="(hint[0-9a-f]{8})" draggable="false">H</a><div id="(hint[0-9a-f]{8})" class="hint" style="display: none">secret</div>$"##).unwrap();
        let caps = hint.captures(parts[1]).expect("hint markup");
        assert_eq!(&caps[1], &caps[2]);
        assert_eq!(parts[2], "<ruby><rb>日本</rb><rt>にほん</rt></ruby><ruby><rb>語</rb><rt>ご</rt></ruby>");
        assert_eq!(parts[3], "にほんご");
        assert_eq!(parts[4], "日本語");
        assert_eq!(parts[5], "<b>bold</b> &amp; x");
        assert_eq!(parts[6], "<b>bold</b> &amp; x");
    }

    #[test]
    fn hint_on_an_empty_field_renders_nothing_tts_renders_nothing() {
        let nt = standard(&["F", "H"], &[tpl("c", "{{F}}{{hint:H}}{{tts en_US:F}}", "")]);
        assert_eq!(render(&nt, &["x", " "]).front.html, "x");
    }

    #[test]
    fn blank_front_and_template_errors_render_a_message() {
        assert!(render(&basic(), &["", "A"]).front.html.contains("The front of this card is blank"));
        let broken = standard(&["F"], &[tpl("c", "{{#F}}x", "{{Nope}}")]);
        let r = render(&broken, &["y"]);
        assert!(r.front.html.contains("loci-template-error"));
        assert!(r.front.html.contains("Missing {{/F}}"));
        assert!(r.back.html.contains("there is no field called 'Nope'"));
    }

    #[test]
    fn missing_template_renders_a_message_on_both_sides() {
        let r = render_with(&basic(), &["Q", "A"], R { ord: 4, ..defaults() });
        assert!(r.front.html.contains("Card type 5 no longer exists"));
        assert_eq!(r.front.html, r.back.html);
        assert_eq!(r.template_name, "");
    }

    #[test]
    fn cloze_question_answer_back_extra_cloze_ords() {
        let r = render_plain(&cloze(), &["{{c1::Paris::city}} is in {{c2::France}}", "extra"], 0);
        assert_eq!(
            r.front.html,
            "<span class=\"cloze\" data-cloze=\"Paris\" data-ordinal=\"1\">[city]</span> is in <span class=\"cloze-inactive\" data-ordinal=\"2\">France</span>"
        );
        assert_eq!(
            r.back.html,
            "<span class=\"cloze\" data-ordinal=\"1\">Paris</span> is in <span class=\"cloze-inactive\" data-ordinal=\"2\">France</span><br>\nextra"
        );
        assert_eq!(r.front.cloze_ords, Some(vec![1, 2]));
        assert_eq!(r.template_name, "Cloze");
    }

    #[test]
    fn missing_cloze_number_shows_the_anki_message() {
        let r = render_plain(&cloze(), &["{{c1::a}}", ""], 4);
        assert!(r.front.html.contains("No cloze 5 found"));
    }

    #[test]
    fn one_by_one_from_the_note_type_config() {
        let mut nt = cloze();
        nt.config.cloze_one_by_one = Some(true);
        let r = render_plain(&nt, &["{{c1::a}} {{c1::b}}", ""], 0);
        assert!(r.back.html.contains("data-cloze-ord=\"1\" data-cloze-index=\"1\">b</span>"));
    }

    #[test]
    fn cloze_only_and_type_cloze() {
        let nt = make_notetype(
            "Cloze",
            NotetypeKind::Cloze,
            &["Text"],
            &[tpl("C", "{{cloze:Text}}{{type:cloze:Text}}", "{{cloze-only:Text}}")],
        );
        let r = render_plain(&nt, &["{{c1::<b>Rome</b>}} and {{c1::Milan}} {{c2::x}}"], 0);
        assert!(r.front.has_type_input);
        assert_eq!(r.front.type_answer.as_deref(), Some("Rome, Milan"));
        assert!(r
            .front
            .html
            .contains("<span class=\"loci-type\" data-answer=\"Rome, Milan\" data-field=\"Text\" data-type-cloze=\"1\"></span>"));
        assert_eq!(r.back.html, "<b>Rome</b>, Milan");
    }

    fn typed() -> Notetype {
        standard(&["Front", "Back"], &[tpl("c", "{{Front}}\n\n{{type:Back}}", "{{Front}}<hr id=answer>{{type:Back}}")])
    }

    #[test]
    fn type_placeholder_on_both_sides_with_plain_expected_text() {
        let r = render(&typed(), &["Q", "caf&#233; <i>au</i> lait<br>noir"]);
        assert!(r.front.has_type_input);
        assert_eq!(r.front.type_answer.as_deref(), Some("café au lait noir"));
        assert_eq!(r.front.html, "Q\n\n<span class=\"loci-type\" data-answer=\"café au lait noir\" data-field=\"Back\"></span>");
        assert!(r.back.has_type_input);
    }

    #[test]
    fn only_the_first_type_box_per_side_is_kept() {
        let nt = standard(&["F", "B"], &[tpl("c", "{{F}}{{type:B}}", "{{FrontSide}}<hr id=answer>{{type:B}}{{type:nc:F}}")]);
        let r = render(&nt, &["f", "b"]);
        assert_eq!(r.back.html.matches("loci-type").count(), 1);
        assert_eq!(r.back.type_answer.as_deref(), Some("b"));
    }

    #[test]
    fn nc_modifier() {
        let nt = standard(&["F"], &[tpl("c", "{{type:nc:F}}", "")]);
        assert_eq!(
            render(&nt, &["x"]).front.html,
            "<span class=\"loci-type\" data-answer=\"x\" data-field=\"F\" data-nc=\"1\"></span>"
        );
    }

    #[test]
    fn collects_sound_in_order_and_replaces_them_with_buttons() {
        let r = render(&basic(), &["Hi [sound:a b.mp3]", "[sound:c&amp;d.ogg]"]);
        assert_eq!(r.front.media, ["a b.mp3"]);
        assert_eq!(
            r.front.html,
            "Hi <button type=\"button\" class=\"loci-audio\" data-src=\"/media/a%20b.mp3\" data-name=\"a b.mp3\" data-index=\"0\" aria-label=\"Play audio\"></button>"
        );
        assert_eq!(r.back.media, ["a b.mp3", "c&d.ogg"]);
        assert!(r.back.html.contains("data-index=\"1\""));
    }

    #[test]
    fn maths_inside_a_cloze_answer_and_inside_data_cloze_attributes() {
        let r = render_plain(&cloze(), &["{{c1::\\(x^{2}\\)}} and {{c2::y}}", ""], 0);
        assert_eq!(
            r.back.html,
            "<span class=\"cloze\" data-ordinal=\"1\"><span class=\"loci-math\" data-display=\"0\">x^{2}</span></span> and <span class=\"cloze-inactive\" data-ordinal=\"2\">y</span><br>\n"
        );
        assert!(r
            .front
            .html
            .contains("data-cloze=\"&lt;span class=&quot;loci-math&quot; data-display=&quot;0&quot;&gt;x^{2}&lt;/span&gt;\""));
    }

    #[test]
    fn moves_style_blocks_into_the_scoped_stylesheet() {
        let nt = standard(&["F"], &[tpl("c", "<style>body { color: red; border: 1px solid }</style>{{F}}", "{{FrontSide}}")]);
        let r = render(&nt, &["x"]);
        assert_eq!(r.front.html, "x");
        // the card root's colour belongs to Loci unless the note type keeps its own styling
        assert!(r.css.contains(".card { border: 1px solid; }"));
        let mut own = nt.clone();
        own.config.use_own_styling = Some(true);
        assert!(render(&own, &["x"]).css.contains(".card { color: red; border: 1px solid; }"));
    }

    #[test]
    fn direction_detection_arabic_hebrew_latin_mixed() {
        assert_eq!(detect_dir("<b>العربية</b> text"), Dir::Rtl);
        assert_eq!(detect_dir("  123 – שלום"), Dir::Rtl);
        assert_eq!(detect_dir("Hello مرحبا"), Dir::Auto);
        assert_eq!(detect_dir("<span class=\"loci-math\" data-display=\"0\">x</span> مرحبا"), Dir::Rtl);
        assert_eq!(detect_dir("ދިވެހި"), Dir::Rtl); // Thaana
        assert_eq!(detect_dir("ܐܪܡܝܐ"), Dir::Rtl); // Syriac
        assert_eq!(detect_dir("ߒߞߏ"), Dir::Rtl); // NKo
        assert_eq!(detect_dir("42 !"), Dir::Auto);
    }

    #[test]
    fn render_card_sets_dir_from_the_question() {
        assert_eq!(render(&basic(), &["ما هذا؟", "x"]).dir, Dir::Rtl);
    }

    #[test]
    fn image_occlusion_payload_is_attached_for_io_note_types() {
        let nt = make_notetype(
            "IO",
            NotetypeKind::Cloze,
            &["Occlusion", "Image", "Header", "Back Extra", "Comments"],
            &[tpl(
                "IO",
                "{{#Header}}<div>{{Header}}</div>{{/Header}}<div style=\"display: none\">{{cloze:Occlusion}}</div><div id=\"image-occlusion-container\">{{Image}}</div><script>anki.imageOcclusion.setup()</script>",
                "{{cloze:Occlusion}}",
            )],
        );
        let r = render_plain(
            &nt,
            &[
                "{{c1::image-occlusion:rect:left=.1:top=.1:width=.2:height=.2:oi=1}}<br>{{c2::image-occlusion:rect:left=.5:top=.5:width=.2:height=.2}}<br>",
                "<img src=\"m.png\">",
                "",
                "",
                "",
            ],
            1,
        );
        let occ = r.occlusion.expect("occlusion payload");
        assert_eq!((occ.image.as_str(), occ.mode, occ.active_ord), ("/media/m.png", OcclusionMode::HideOne, 2));
        assert_eq!(occ.shapes.len(), 2);
        assert!(r.front.html.contains("<div class=\"cloze\" data-shape=\"rect\" data-ordinal=\"2\" data-left=\".5\""));
        assert!(r.front.html.contains("<img src=\"/media/m.png\">"));
    }

    #[test]
    fn builtin_io_uses_the_first_two_fields_and_plain_cloze_has_no_payload() {
        let mut nt = make_notetype("IO", NotetypeKind::Cloze, &["Occ", "Img"], &[tpl("IO", "{{cloze:Occ}}", "{{cloze:Occ}}")]);
        nt.config.builtin = Some(BuiltinKind::ImageOcclusion);
        nt.config.occlusion_mode = Some(OcclusionMode::HideAll);
        let r = render_plain(&nt, &["{{c1::image-occlusion:rect:left=.1}}", "<img src=\"a.png\">"], 0);
        assert_eq!(r.occlusion.as_ref().map(|o| o.mode), Some(OcclusionMode::HideOne));
        assert!(render_plain(&cloze(), &["{{c1::a}}", ""], 0).occlusion.is_none());
    }
}
