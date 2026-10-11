//! Note-type CSS → stylesheet for the card's shadow root (recall `src/core/template/css.ts`).
//!
//! Anki styles target `html`, `body`, `.card` and `.nightMode`/`.night_mode` on body. Inside our
//! shadow root the card is `<div class="card [nightMode night_mode]">`, so:
//!   `html`, `:root`            → `:host`
//!   `body`, `html body`        → `.card`
//!   `.nightMode .card`, `body.nightMode`, `.nightMode.card` → `.card.nightMode` (same for night_mode)
//!   `.card` and every other selector are kept (they already live inside the shadow root).
//! `@import`, `@charset`, `@namespace` are dropped; declarations using remote `url(http…)`/`url(//…)`
//! are dropped (privacy and offline); relative `url(name)` (collection media, e.g. `_font.ttf`
//! in `@font-face`) is rewritten through `media_url`; `data:` URLs are kept.
//!
//! Loci owns the card's typography and colours, so unless the note type opts out
//! (`NotetypeConfig.use_own_styling == Some(true)`):
//!   (a) rules of Anki's stock note-type CSS are dropped (`.card { font-family: arial; font-size:
//!       20px; line-height: 1.5; text-align: center; color: black; background-color: white }` or
//!       any subset, `.cloze { font-weight: bold; color: blue }`, `.nightMode .cloze { color:
//!       lightblue }`, the image-occlusion template's `#image-occlusion-container` / `-canvas` rules);
//!   (b) in the remaining rules aimed at the card root (`.card`, `body`, `html`, `.card.nightMode`, …)
//!       the declarations background, background-color, color, font, font-family and font-size are
//!       removed; every other selector and declaration is kept.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardCssOptions {
    /// The note type's `config.useOwnStyling`: keep its colours and fonts (skip (a) and (b)).
    pub own_styling: bool,
}

type MediaUrl<'a> = Option<&'a dyn Fn(&str) -> String>;

static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("valid regex"));

fn strip_comments(css: &str) -> String {
    COMMENT.replace_all(css, "").into_owned()
}

/// Index of the `}` matching the `{` at `open`, honouring strings; css.len() if unbalanced.
fn match_brace(css: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    let mut i = open;
    while i < css.len() {
        let ch = css[i];
        if ch == b'"' || ch == b'\'' {
            let q = ch;
            i += 1;
            while i < css.len() && css[i] != q {
                if css[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if ch == b'{' {
            depth += 1;
        } else if ch == b'}' {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
        i += 1;
    }
    css.len()
}

/// Splits on `sep` at parenthesis depth 0, outside strings.
fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            cur.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            cur.push(ch);
            continue;
        }
        if ch == '(' || ch == '[' {
            depth += 1;
        } else if ch == ')' || ch == ']' {
            depth -= 1;
        }
        if ch == sep && depth == 0 {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(ch);
    }
    out.push(cur);
    out
}

static SEL_BODY_NIGHT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:html\s+)?body\.(nightMode|night_mode)(?:\s+\.card\b)?").expect("valid regex"));
static SEL_NIGHT_CARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:html\s*)?\.(nightMode|night_mode)(?:\s+body)?\s+\.card\b").expect("valid regex"));
static SEL_NIGHT_DOT_CARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.(nightMode|night_mode)\.card\b").expect("valid regex"));
static SEL_CARD_DOT_NIGHT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.card\.(nightMode|night_mode)\b").expect("valid regex"));
static SEL_BODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:html\s+)?body\b").expect("valid regex"));
static SEL_HTML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^html\b").expect("valid regex"));
static SEL_ROOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:root\b").expect("valid regex"));
static SEL_CARD_CARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\.card\s+\.card\b").expect("valid regex"));

/// Rewrites one selector for the shadow root (see the module header).
pub fn scope_selector(sel: &str) -> String {
    let mut s = sel.trim().to_string();
    if s.is_empty() {
        return s;
    }
    s = SEL_BODY_NIGHT.replace(&s, ".card.$1").into_owned();
    s = SEL_NIGHT_CARD.replace(&s, ".card.$1").into_owned();
    s = SEL_NIGHT_DOT_CARD.replace(&s, ".card.$1").into_owned();
    s = SEL_CARD_DOT_NIGHT.replace(&s, ".card.$1").into_owned();
    s = SEL_BODY.replace(&s, ".card").into_owned();
    s = SEL_HTML.replace(&s, ":host").into_owned();
    s = SEL_ROOT.replace(&s, ":host").into_owned();
    // `.card .card` can appear after `body .card` → `.card .card`; collapse.
    s = SEL_CARD_CARD.replace(&s, ".card").into_owned();
    s
}

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^)\s]*))\s*\)"#).expect("valid regex"));
static REMOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:https?:)?//").expect("valid regex"));
static SCRIPT_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:javascript|vbscript):").expect("valid regex"));
static DATA_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:data|blob):").expect("valid regex"));
static HAS_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)url\(").expect("valid regex"));
static WS_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));

/// Rewrites url() references in one value item; None when it loads something remote.
fn rewrite_urls(item: &str, media_url: MediaUrl<'_>) -> Option<String> {
    let mut remote = false;
    let out = URL_RE.replace_all(item, |m: &regex::Captures| {
        let r = m.get(1).or_else(|| m.get(2)).or_else(|| m.get(3)).map_or("", |x| x.as_str()).trim();
        if REMOTE.is_match(r) || SCRIPT_URL.is_match(r) {
            remote = true;
            return m[0].to_string();
        }
        let Some(media_url) = media_url else { return m[0].to_string() };
        if DATA_URL.is_match(r) || r.is_empty() || r.starts_with('#') {
            return m[0].to_string();
        }
        let name = super::math::decode_uri_component(r).unwrap_or_else(|| r.to_string());
        format!("url(\"{}\")", media_url(&name).replace('\\', "\\\\").replace('"', "\\\""))
    });
    if remote {
        None
    } else {
        Some(out.into_owned())
    }
}

/// Processes a declaration block: in comma-separated values (font-face `src`, layered
/// backgrounds) remote items are removed; a declaration left with nothing is dropped.
fn process_declarations(block: &str, media_url: MediaUrl<'_>) -> String {
    let mut out: Vec<String> = Vec::new();
    for decl in split_top_level(block, ';') {
        if decl.trim().is_empty() {
            continue;
        }
        if !HAS_URL.is_match(&decl) {
            out.push(decl.trim().to_string());
            continue;
        }
        let Some(colon) = decl.find(':') else { continue };
        let items: Vec<String> = split_top_level(&decl[colon + 1..], ',')
            .iter()
            .filter_map(|i| rewrite_urls(i, media_url))
            .map(|i| i.trim().to_string())
            .collect();
        if !items.is_empty() {
            out.push(format!("{}: {}", decl[..colon].trim(), items.join(", ")));
        }
    }
    if out.is_empty() {
        " ".to_string()
    } else {
        format!(" {}; ", out.join("; "))
    }
}

fn norm_decl(d: &str) -> String {
    match d.find(':') {
        None => d.trim().to_lowercase(),
        Some(colon) => {
            format!("{}:{}", d[..colon].trim().to_lowercase(), WS_RUN.replace_all(&d[colon + 1..].trim().to_lowercase(), " "))
        }
    }
}

fn decl_prop(d: &str) -> String {
    d[..d.find(':').unwrap_or(0)].trim().to_lowercase()
}

fn norm_sel(sel: &str) -> String {
    WS_RUN.replace_all(&sel.trim().to_lowercase(), " ").into_owned()
}

struct StockRule {
    sel: &'static str,
    allowed: &'static [&'static str],
    required: &'static str,
}

/// Anki's stock rules: selector → the declarations it may carry (a rule with a subset is stock).
const STOCK_RULES: [StockRule; 4] = [
    StockRule {
        sel: ".card",
        allowed: &[
            "font-family:arial",
            "font-size:20px",
            "line-height:1.5",
            "text-align:center",
            "color:black",
            "background-color:white",
        ],
        required: "font-family:arial",
    },
    StockRule { sel: ".cloze", allowed: &["font-weight:bold", "color:blue"], required: "color:blue" },
    StockRule { sel: ".nightmode .cloze", allowed: &["color:lightblue"], required: "color:lightblue" },
    StockRule { sel: ".night_mode .cloze", allowed: &["color:lightblue"], required: "color:lightblue" },
];
static IO_STOCK_SELECTOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#image-occlusion-(?:container|canvas)\b").expect("valid regex"));

/// True for a rule of Anki's stock note-type CSS (see the module header).
fn is_stock_rule(selectors: &[String], decls: &[String]) -> bool {
    let sels: Vec<String> = selectors.iter().map(|s| norm_sel(s)).filter(|s| !s.is_empty()).collect();
    if sels.is_empty() {
        return false;
    }
    if sels.iter().all(|x| IO_STOCK_SELECTOR.is_match(x)) {
        return true;
    }
    if sels.len() != 1 {
        return false;
    }
    let Some(rule) = STOCK_RULES.iter().find(|r| r.sel == sels[0]) else { return false };
    let norm: Vec<String> = decls.iter().map(|d| norm_decl(d)).filter(|d| !d.is_empty()).collect();
    !norm.is_empty() && norm.iter().any(|d| d == rule.required) && norm.iter().all(|d| rule.allowed.contains(&d.as_str()))
}

/// Selectors (after scoping) that style the card root itself, whose colours and fonts Loci owns.
static CARD_ROOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\.card(?:\.(?:nightMode|night_mode))*|:host)$").expect("valid regex"));
const OWNED_PROPS: [&str; 6] = ["background", "background-color", "color", "font", "font-family", "font-size"];

static AT_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@([\w-]+)").expect("valid regex"));

fn scope_block_list(css: &str, media_url: MediaUrl<'_>, opts: CardCssOptions) -> String {
    let bytes = css.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'}' {
            i += 1;
            continue; // stray
        }
        let brace_at = css[i..].find('{').map(|p| p + i);
        let semi_at = css[i..].find(';').map(|p| p + i);
        if bytes[i] == b'@' {
            let name = AT_NAME.captures(&css[i..]).map(|m| m[1].to_lowercase()).unwrap_or_default();
            if let Some(semi) = semi_at {
                if brace_at.is_none_or(|b| semi < b) {
                    // statement at-rule
                    let stmt = &css[i..=semi];
                    if !["import", "charset", "namespace"].contains(&name.as_str()) {
                        out.push_str(stmt);
                        out.push('\n');
                    }
                    i = semi + 1;
                    continue;
                }
            }
            let Some(brace) = brace_at else { break };
            let close = match_brace(bytes, brace);
            let prelude = css[i..brace].trim();
            let body = &css[brace + 1..close];
            if ["media", "supports", "container", "layer", "document"].contains(&name.as_str()) {
                out.push_str(&format!("{prelude} {{\n{}}}\n", scope_block_list(body, media_url, opts)));
            } else if ["font-face", "page", "property", "counter-style", "font-feature-values"].contains(&name.as_str()) {
                out.push_str(&format!("{prelude} {{{}}}\n", process_declarations(body, media_url)));
            } else {
                out.push_str(&format!("{prelude} {{{body}}}\n")); // keyframes etc.
            }
            i = close + 1;
            continue;
        }
        let Some(brace) = brace_at else { break };
        let close = match_brace(bytes, brace);
        let raw_selectors = split_top_level(&css[i..brace], ',');
        let selectors: Vec<String> = raw_selectors.iter().map(|s| scope_selector(s)).filter(|s| !s.is_empty()).collect();
        let mut block = css[brace + 1..close].to_string();
        i = close + 1;
        if !opts.own_styling {
            let decls: Vec<String> = split_top_level(&block, ';').into_iter().filter(|d| !d.trim().is_empty()).collect();
            if is_stock_rule(&raw_selectors, &decls) {
                continue;
            }
            if !selectors.is_empty() && selectors.iter().all(|x| CARD_ROOT.is_match(x.trim())) {
                let kept: Vec<String> = decls.into_iter().filter(|d| !OWNED_PROPS.contains(&decl_prop(d).as_str())).collect();
                if kept.is_empty() {
                    continue;
                }
                block = kept.join(";");
            }
        }
        let body = process_declarations(&block, media_url);
        if !selectors.is_empty() {
            out.push_str(&format!("{} {{{body}}}\n", selectors.join(", ")));
        }
    }
    out
}

/// Scopes note-type CSS for the card shadow root. `media_url` resolves relative url() references;
/// unless `opts.own_styling`, Anki's stock rules and the card root's colours and fonts are removed.
pub fn scope_card_css(css: &str, media_url: MediaUrl<'_>, opts: CardCssOptions) -> String {
    scope_block_list(&strip_comments(css), media_url, opts)
}

/// Loci's base card stylesheet, placed before the note type's CSS in the shadow root. Driven by
/// variables the host sets: --loci-card-font, --loci-card-size, --loci-ink, --loci-accent,
/// --loci-cloze, --loci-muted, --loci-line, --loci-surface (plus optional --loci-card-align,
/// --loci-mono, --loci-good/--loci-again/--loci-hard and --loci-io-fill/-stroke).
pub const LOCI_CARD_BASE_CSS: &str = include_str!("base.css");

#[cfg(test)]
mod tests {
    use super::*;

    fn media_url(name: &str) -> String {
        format!("/media/{}", super::super::math::percent_encode(name))
    }
    const OWN: CardCssOptions = CardCssOptions { own_styling: true };
    const DEFAULT: CardCssOptions = CardCssOptions { own_styling: false };

    #[test]
    fn rewrites_html_body_nightmode_selectors_and_keeps_card() {
        assert_eq!(scope_selector("body"), ".card");
        assert_eq!(scope_selector("html"), ":host");
        assert_eq!(scope_selector(":root"), ":host");
        assert_eq!(scope_selector(".card"), ".card");
        assert_eq!(scope_selector(".nightMode .card"), ".card.nightMode");
        assert_eq!(scope_selector(".night_mode .cloze"), ".night_mode .cloze");
        assert_eq!(scope_selector("body.nightMode .card"), ".card.nightMode");
        assert_eq!(scope_selector("body .card b"), ".card b");
    }

    #[test]
    fn strips_import_and_remote_urls_rewrites_media_urls_and_font_face() {
        let css = "@import url(\"https://fonts.googleapis.com/css?family=X\");\n\
@charset \"utf-8\";\n\
/* comment */\n\
body { background: url(https://evil.example/bg.png); color: red; }\n\
.card { background-image: url('_paper.png'); }\n\
@font-face { font-family: Mine; src: url(\"_mine.ttf\") format(\"truetype\"), url(//cdn.example/x.woff); }\n\
@media (max-width: 600px) { body, .x { font-size: 18px } }\n\
img { content: url(data:image/png;base64,AA) }";
        let out = scope_card_css(css, Some(&media_url), OWN);
        assert!(!out.contains("@import"));
        assert!(!out.contains("evil.example"));
        assert!(!out.contains("cdn.example"));
        assert!(Regex::new(r"\.card \{\s*color: red;?\s*\}").unwrap().is_match(&out));
        assert!(out.contains("background-image: url(\"/media/_paper.png\")"));
        assert!(out.contains("@font-face {"));
        assert!(out.contains("font-family: Mine;"));
        assert!(out.contains("@media (max-width: 600px) {\n.card, .x { font-size: 18px; }"));
        assert!(out.contains("src: url(\"/media/_mine.ttf\") format(\"truetype\");"));
        assert!(out.contains("url(data:image/png;base64,AA)"));
    }

    #[test]
    fn drops_ankis_stock_css_every_version_unless_own_styling() {
        let basic = ".card {\n    font-family: arial;\n    font-size: 20px;\n    line-height: 1.5;\n    text-align: center;\n    color: black;\n    background-color: white;\n}\n";
        let old = ".card {\n font-family: arial;\n font-size: 20px;\n text-align: center;\n color: black;\n background-color: white;\n}\n";
        let cloze = format!(
            "{basic}.cloze {{\n    font-weight: bold;\n    color: blue;\n}}\n.nightMode .cloze {{\n    color: lightblue;\n}}\n"
        );
        let io = format!("{cloze}#image-occlusion-container {{\n    position: relative;\n}}\n#image-occlusion-canvas {{ position: absolute; top: 0; left: 0; }}\n");
        let crlf = format!("/* Anki */\n{}", cloze.replace('\n', "\r\n"));
        for css in [basic, old, cloze.as_str(), io.as_str(), ".card{font-family:Arial;font-size:20px}", crlf.as_str()] {
            assert_eq!(scope_card_css(css, None, DEFAULT).trim(), "", "{css}");
        }
        let own = scope_card_css(&cloze, None, OWN);
        assert!(own.contains("background-color: white"));
        assert!(own.contains(".nightMode .cloze { color: lightblue; }"));
    }

    #[test]
    fn strips_colours_and_fonts_from_card_root_rules_of_custom_css() {
        let css = ".card {\n    font-family: arial;\n    font-size: 20px;\n    text-align: center;\n    color: black;\n    background-color: white;\n}\n\n\
.word { font-family: 'Noto Naskh Arabic', serif; font-size: 40px; color: #333; }\n\
.card { font-family: Georgia; font-size: 24px; color: #222; background: #fff url(_paper.png); text-align: left; padding: 8px; }\n\
body { background-color: ivory; margin: 0 }\n\
html { font-size: 18px; color: navy }\n\
.nightMode .card { color: white; background: black; border-color: grey }\n\
.card.nightMode .cloze { color: lightblue }\n\
.card, .extra { color: red }\n\
.card b { color: green }";
        let out = scope_card_css(css, Some(&media_url), DEFAULT);
        assert!(out.contains(".word { font-family: 'Noto Naskh Arabic', serif; font-size: 40px; color: #333; }"));
        assert!(out.contains(".card { text-align: left; padding: 8px; }"));
        assert!(out.contains(".card { margin: 0; }"));
        assert!(!out.contains(":host {"));
        assert!(out.contains(".card.nightMode { border-color: grey; }"));
        assert!(out.contains(".card.nightMode .cloze { color: lightblue; }"));
        assert!(out.contains(".card, .extra { color: red; }"));
        assert!(out.contains(".card b { color: green; }"));
        assert!(!Regex::new(r"Georgia|ivory|navy|#222|background: black").unwrap().is_match(&out));
        assert!(!out.contains("text-align: center"));
    }

    #[test]
    fn base_css_is_driven_by_the_documented_variables() {
        for v in [
            "--loci-card-font",
            "--loci-card-size",
            "--loci-ink",
            "--loci-accent",
            "--loci-cloze",
            "--loci-muted",
            "--loci-line",
            "--loci-surface",
        ] {
            assert!(LOCI_CARD_BASE_CSS.contains(v), "{v}");
        }
        for c in [
            ".cloze",
            ".cloze-inactive",
            ".typeGood",
            ".typeBad",
            ".typeMissed",
            ".loci-audio",
            ".loci-io-mask",
            ".loci-math",
            "a.hint",
            "unicode-bidi: plaintext",
        ] {
            assert!(LOCI_CARD_BASE_CSS.contains(c), "{c}");
        }
    }

    // css-dark.test.ts: stock Anki CSS paints nothing in Loci (dark theme safe).
    const ANKI_IO_CSS: &str = "#image-occlusion-canvas {\n    --inactive-shape-color: #ffeba2;\n    --active-shape-color: #ff8e8e;\n    --inactive-shape-border: 1px #212121 solid;\n    --active-shape-border: 1px #212121 solid;\n    --highlight-shape-color: #ff8e8e00;\n    --highlight-shape-border: 1px #ff8e8e solid;\n}\n\n.card {\n    font-family: arial;\n    font-size: 20px;\n    text-align: center;\n    color: black;\n    background-color: white;\n}\n";
    const STOCK_BASIC: &str = ".card {\n    font-family: arial;\n    font-size: 20px;\n    line-height: 1.5;\n    text-align: center;\n    color: black;\n    background-color: white;\n}\n";

    #[test]
    fn stock_anki_css_paints_nothing_in_dark_theme() {
        let stock_cloze = format!("{STOCK_BASIC}.cloze {{\n    font-weight: bold;\n    color: blue;\n}}\n.nightMode .cloze {{\n    color: lightblue;\n}}\n");
        let fixture_io = format!("{stock_cloze}#image-occlusion-container {{\n    position: relative;\n}}\n");
        let colours = Regex::new(r"\b(?:white|black|blue|lightblue)\b").unwrap();
        for css in [ANKI_IO_CSS, STOCK_BASIC, stock_cloze.as_str(), fixture_io.as_str()] {
            let out = scope_card_css(css, None, DEFAULT);
            assert!(!colours.is_match(&out), "{css}");
            assert!(!out.contains("background"), "{css}");
        }
    }

    #[test]
    fn customised_card_root_keeps_layout_but_not_colours() {
        let out = scope_card_css("body { background: #fff; color: #000; padding: 4px } .header { font-weight: 600 } .nightMode .header { color: #ddd }", None, DEFAULT);
        assert!(out.contains(".card { padding: 4px; }"));
        assert!(!Regex::new(r"#fff|#000").unwrap().is_match(&out));
        assert!(out.contains(".nightMode .header")); // the wrapper (Anki's <body>) carries nightMode too
    }

    #[test]
    fn keyframes_and_strings_with_braces_survive() {
        let out = scope_card_css("@keyframes spin { from { transform: rotate(0) } to { transform: rotate(360deg) } }\n.a::after { content: \"}\"; color: red }", None, DEFAULT);
        assert!(out.contains("@keyframes spin { from { transform: rotate(0) } to { transform: rotate(360deg) } }"));
        assert!(out.contains(".a::after { content: \"}\"; color: red; }"));
    }
}
