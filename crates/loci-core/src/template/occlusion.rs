//! Image Occlusion in Anki's built-in format (recall `src/core/template/occlusion.ts`).
//!
//! The `Occlusion` field holds one cloze per shape, each followed by `<br>`:
//!   `{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}<br>`
//!   `{{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.1}}<br>`
//!   `{{c3::image-occlusion:polygon:points=.1,.1 .2,.2 .3,.1}}<br>`
//!   `{{c0::image-occlusion:text:text=Label\:x:left=.4:top=.5:scale=1:fs=.05}}<br>`
//! Values escape `\` and `:` as `\\` and `\:`. Coordinates are normalised to 0..1 of the image
//! (current Anki); very old notes may hold pixels (see `occlusion_units`). Ellipses are given by
//! their bounding box's top-left plus radii; we expose width = 2·rx, height = 2·ry.
//! `oi=1` (occlude inactive) means "hide all, guess one"; otherwise "hide one, guess one".

use crate::text::decode_entities;
use crate::types::{OcclusionMode, OcclusionPayload, OcclusionShape, ShapeKind};
use regex::Regex;
use std::sync::LazyLock;

use super::cloze::{image_occlusion_text, parse_clozes, ClozeTextOrCloze};

/// A parsed shape plus what is needed to write it back unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct IoShape {
    pub shape: OcclusionShape,
    /// `oi=1`: all shapes are masked while this one is asked ("hide all, guess one").
    pub occlude_inactive: bool,
    /// every key=value pair in source order (unescaped), including ones Loci does not interpret (angle, fill, scale, fs…)
    pub props: Vec<(String, String)>,
}

impl IoShape {
    /// A brand-new shape without source props.
    pub fn new(shape: OcclusionShape, occlude_inactive: bool) -> IoShape {
        IoShape { shape, occlude_inactive, props: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcclusionUnits {
    Normalized,
    Pixels,
}

fn shape_kind(s: &str) -> Option<ShapeKind> {
    match s {
        "rect" => Some(ShapeKind::Rect),
        "ellipse" => Some(ShapeKind::Ellipse),
        "polygon" => Some(ShapeKind::Polygon),
        "text" => Some(ShapeKind::Text),
        _ => None,
    }
}

fn kind_name(k: ShapeKind) -> &'static str {
    match k {
        ShapeKind::Rect => "rect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Polygon => "polygon",
        ShapeKind::Text => "text",
    }
}

/// Splits on unescaped ':' and unescapes `\:` and `\\`.
fn split_escaped(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                cur.push(next);
                continue;
            }
        }
        if ch == ':' {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(ch);
    }
    out.push(cur);
    out
}

fn escape_value(v: &str) -> String {
    v.replace('\\', "\\\\").replace(':', "\\:")
}

/// occlusion.ts:41 `Number(v)`: blank → none, non-finite → none. (JavaScript also accepted
/// hex such as `0x10`; a coordinate in that form never occurs in Anki's writer.)
fn num(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    if v.is_empty() {
        return None;
    }
    v.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Parses the body after `image-occlusion:` into a shape (None if the shape kind is unknown).
pub fn parse_occlusion_body(body: &str, ord: i64) -> Option<IoShape> {
    let parts = split_escaped(body);
    let kind = shape_kind(parts.first().map(String::as_str).unwrap_or(""))?;
    let mut props: Vec<(String, String)> = Vec::new();
    for p in parts.iter().skip(1) {
        let Some(eq) = p.find('=') else { continue };
        if eq == 0 {
            continue;
        }
        props.push((p[..eq].to_string(), p[eq + 1..].to_string()));
    }
    let get = |k: &str| props.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let mut shape = OcclusionShape { kind, ord, left: None, top: None, width: None, height: None, points: None, text: None };
    shape.left = num(get("left"));
    shape.top = num(get("top"));
    let mut width = num(get("width"));
    let mut height = num(get("height"));
    if kind == ShapeKind::Ellipse {
        if width.is_none() {
            width = num(get("rx")).map(|rx| rx * 2.0);
        }
        if height.is_none() {
            height = num(get("ry")).map(|ry| ry * 2.0);
        }
    }
    shape.width = width;
    shape.height = height;
    if kind == ShapeKind::Polygon {
        let pts: Vec<[f64; 2]> = get("points")
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|xy| {
                let mut it = xy.split(',');
                let x = it.next()?.parse::<f64>().ok()?;
                let y = it.next()?.parse::<f64>().ok()?;
                (x.is_finite() && y.is_finite()).then_some([x, y])
            })
            .collect();
        if !pts.is_empty() && shape.left.is_none() {
            let min_x = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            let min_y = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
            let max_x = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
            let max_y = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
            shape.left = Some(min_x);
            shape.top = Some(min_y);
            shape.width = Some(max_x - min_x);
            shape.height = Some(max_y - min_y);
        }
        shape.points = Some(pts);
    }
    if kind == ShapeKind::Text {
        shape.text = Some(get("text").unwrap_or("").to_string());
    }
    let occlude_inactive = get("oi") == Some("1");
    Some(IoShape { shape, occlude_inactive, props })
}

/// All IO shapes in an Occlusion field, in source order (grouped shapes share an ordinal).
pub fn parse_occlusion_field(field: &str) -> Vec<IoShape> {
    fn walk(nodes: &[ClozeTextOrCloze], out: &mut Vec<IoShape>) {
        for n in nodes {
            let ClozeTextOrCloze::Cloze(c) = n else { continue };
            match image_occlusion_text(c) {
                Some(body) => {
                    if let Some(s) = parse_occlusion_body(&decode_entities(body), c.ordinals.first().copied().unwrap_or(0)) {
                        out.push(s);
                    }
                }
                None => walk(&c.nodes, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&parse_clozes(field), &mut out);
    out
}

/// Anki's number display for IO coordinates: up to 4 decimals, no leading/trailing zeros (`.2325`).
pub fn format_io_number(n: f64) -> String {
    if !n.is_finite() || n == 0.0 {
        return "0".to_string();
    }
    let mut s = format!("{n:.4}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if let Some(rest) = s.strip_prefix("0.") {
        return format!(".{rest}");
    }
    if let Some(rest) = s.strip_prefix("-0.") {
        return format!("-.{rest}");
    }
    s
}

/// Geometry props for a shape, derived from its typed fields (used when writing).
fn geometry_props(s: &OcclusionShape) -> Vec<(String, String)> {
    let f = |n: Option<f64>| format_io_number(n.unwrap_or(0.0));
    let p = |k: &str, v: String| (k.to_string(), v);
    match s.kind {
        ShapeKind::Rect => vec![p("left", f(s.left)), p("top", f(s.top)), p("width", f(s.width)), p("height", f(s.height))],
        ShapeKind::Ellipse => vec![
            p("left", f(s.left)),
            p("top", f(s.top)),
            p("rx", f(Some(s.width.unwrap_or(0.0) / 2.0))),
            p("ry", f(Some(s.height.unwrap_or(0.0) / 2.0))),
        ],
        ShapeKind::Polygon => vec![p(
            "points",
            s.points
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|[x, y]| format!("{},{}", format_io_number(*x), format_io_number(*y)))
                .collect::<Vec<_>>()
                .join(" "),
        )],
        ShapeKind::Text => vec![p("text", s.text.clone().unwrap_or_default()), p("left", f(s.left)), p("top", f(s.top))],
    }
}

fn same_geometry(a: &OcclusionShape, b: &OcclusionShape) -> bool {
    let eq = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 5e-5,
        _ => false,
    };
    if a.kind == ShapeKind::Polygon {
        return a.points.as_deref().unwrap_or(&[]) == b.points.as_deref().unwrap_or(&[]);
    }
    eq(a.left, b.left)
        && eq(a.top, b.top)
        && eq(a.width, b.width)
        && eq(a.height, b.height)
        && a.text.as_deref().unwrap_or("") == b.text.as_deref().unwrap_or("")
}

/// Serialises one shape as Anki writes it: `{{cN::image-occlusion:kind:k=v…}}`.
pub fn serialize_occlusion_shape(s: &IoShape) -> String {
    let geo = geometry_props(&s.shape);
    let kind = kind_name(s.shape.kind);
    // Keep the original key order and extra keys; overwrite geometry with current values.
    let source_body =
        format!("{kind}:{}", s.props.iter().map(|(k, v)| format!("{k}={}", escape_value(v))).collect::<Vec<_>>().join(":"));
    let unchanged = parse_occlusion_body(&source_body, s.shape.ord).is_some_and(|parsed| same_geometry(&parsed.shape, &s.shape));
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (k, v) in &s.props {
        if k == "oi" {
            continue;
        }
        if let Some((_, g)) = geo.iter().find(|(gk, _)| gk == k) {
            pairs.push((k.clone(), if unchanged { v.clone() } else { g.clone() }));
        } else if !(s.shape.kind == ShapeKind::Ellipse && (k == "width" || k == "height")) {
            pairs.push((k.clone(), v.clone()));
        }
        seen.push(k);
    }
    for (k, v) in &geo {
        if !seen.contains(&k.as_str()) {
            pairs.push((k.clone(), v.clone()));
        }
    }
    if s.occlude_inactive && s.shape.kind != ShapeKind::Text {
        pairs.push(("oi".to_string(), "1".to_string()));
    }
    let mut body = vec![kind.to_string()];
    body.extend(pairs.iter().map(|(k, v)| format!("{k}={}", escape_value(v))));
    format!("{{{{c{}::image-occlusion:{}}}}}", s.shape.ord, body.join(":"))
}

/// Writes an Occlusion field: one cloze per shape, each followed by `<br>` (Anki's layout).
pub fn serialize_occlusion_field(shapes: &[IoShape]) -> String {
    shapes.iter().map(|s| format!("{}<br>", serialize_occlusion_shape(s))).collect()
}

/// `Pixels` when any coordinate exceeds 1 (legacy notes); the UI then scales by the image's natural size.
pub fn occlusion_units(shapes: &[OcclusionShape]) -> OcclusionUnits {
    let big = |n: Option<f64>| n.is_some_and(|n| n > 1.0001);
    let pixels = shapes.iter().any(|s| {
        big(s.left)
            || big(s.top)
            || big(s.width)
            || big(s.height)
            || s.points.as_deref().unwrap_or(&[]).iter().any(|[x, y]| big(Some(*x)) || big(Some(*y)))
    });
    if pixels {
        OcclusionUnits::Pixels
    } else {
        OcclusionUnits::Normalized
    }
}

/// Mode for a card: `HideAll` when the active shapes carry `oi=1`, else `HideOne`.
pub fn occlusion_mode(shapes: &[IoShape], active_ord: i64, fallback: OcclusionMode) -> OcclusionMode {
    let active: Vec<&IoShape> = shapes.iter().filter(|s| s.shape.ord == active_ord && s.shape.kind != ShapeKind::Text).collect();
    if active.is_empty() {
        return fallback;
    }
    if active.iter().any(|s| s.occlude_inactive) {
        OcclusionMode::HideAll
    } else {
        OcclusionMode::HideOne
    }
}

static IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).expect("valid regex"));

/// First `<img src>` filename in a field (entities decoded), or ''.
pub fn first_image_name(html: &str) -> String {
    match IMG_SRC.captures(html) {
        Some(c) => decode_entities(c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map_or("", |m| m.as_str())),
        None => String::new(),
    }
}

/// Payload for one card. `card_ord` is 0-based (active_ord = card_ord + 1). None when the field
/// holds no shapes. `image` is the filename as stored; callers resolve it to a URL.
pub fn occlusion_payload(
    occlusion_field: &str,
    image_field: &str,
    card_ord: i64,
    fallback: OcclusionMode,
) -> Option<OcclusionPayload> {
    let shapes = parse_occlusion_field(occlusion_field);
    if shapes.is_empty() {
        return None;
    }
    let active_ord = card_ord + 1;
    Some(OcclusionPayload {
        image: first_image_name(image_field),
        mode: occlusion_mode(&shapes, active_ord, fallback),
        shapes: shapes.into_iter().map(|s| s.shape).collect(),
        active_ord,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: &str = "{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}<br>\
{{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.1}}<br>\
{{c3::image-occlusion:polygon:points=.1,.1 .2,.2 .3,.1}}<br>\
{{c0::image-occlusion:text:text=Label\\:x:left=.4:top=.5:scale=1:fs=.05}}<br>";

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    #[test]
    fn parses_every_shape_kind_with_coordinates() {
        let s = parse_occlusion_field(FIELD);
        assert_eq!(
            s.iter().map(|x| (x.shape.kind, x.shape.ord)).collect::<Vec<_>>(),
            [(ShapeKind::Rect, 1), (ShapeKind::Ellipse, 2), (ShapeKind::Polygon, 3), (ShapeKind::Text, 0)]
        );
        assert!(
            close(s[0].shape.left, 0.2325)
                && close(s[0].shape.top, 0.3261)
                && close(s[0].shape.width, 0.202)
                && close(s[0].shape.height, 0.0975)
        );
        assert!(s[0].occlude_inactive);
        assert!(
            close(s[1].shape.left, 0.1)
                && close(s[1].shape.top, 0.2)
                && close(s[1].shape.width, 0.1)
                && close(s[1].shape.height, 0.2)
        );
        assert!(!s[1].occlude_inactive);
        assert_eq!(s[2].shape.points.as_deref(), Some(&[[0.1, 0.1], [0.2, 0.2], [0.3, 0.1]][..]));
        assert!(close(s[2].shape.left, 0.1) && close(s[2].shape.top, 0.1));
        assert!(close(s[2].shape.width, 0.2));
        assert_eq!(s[3].shape.text.as_deref(), Some("Label:x"));
        assert!(close(s[3].shape.left, 0.4) && close(s[3].shape.top, 0.5));
    }

    #[test]
    fn round_trips_the_field_byte_for_byte() {
        assert_eq!(serialize_occlusion_field(&parse_occlusion_field(FIELD)), FIELD);
    }

    #[test]
    fn writes_edited_geometry_in_anki_number_format_and_keeps_extra_keys() {
        let mut shapes =
            parse_occlusion_field("{{c1::image-occlusion:rect:left=.1:top=.1:width=.2:height=.2:angle=15:fill=#ffeba2:oi=1}}");
        shapes[0].shape.left = Some(0.5);
        shapes[0].shape.width = Some(0.123456);
        assert_eq!(
            serialize_occlusion_field(&shapes),
            "{{c1::image-occlusion:rect:left=.5:top=.1:width=.1235:height=.2:angle=15:fill=#ffeba2:oi=1}}<br>"
        );
    }

    #[test]
    fn serialises_brand_new_shapes() {
        let shape =
            |kind, ord| OcclusionShape { kind, ord, left: None, top: None, width: None, height: None, points: None, text: None };
        let rect = IoShape::new(
            OcclusionShape { left: Some(0.25), top: Some(0.0), width: Some(1.0), height: Some(0.5), ..shape(ShapeKind::Rect, 1) },
            true,
        );
        let ellipse = IoShape::new(
            OcclusionShape {
                left: Some(0.1),
                top: Some(0.1),
                width: Some(0.2),
                height: Some(0.4),
                ..shape(ShapeKind::Ellipse, 2)
            },
            false,
        );
        let polygon =
            IoShape::new(OcclusionShape { points: Some(vec![[0.0, 0.0], [0.5, 1.0]]), ..shape(ShapeKind::Polygon, 2) }, false);
        assert_eq!(
            serialize_occlusion_field(&[rect, ellipse, polygon]),
            "{{c1::image-occlusion:rect:left=.25:top=0:width=1:height=.5:oi=1}}<br>\
             {{c2::image-occlusion:ellipse:left=.1:top=.1:rx=.1:ry=.2}}<br>\
             {{c2::image-occlusion:polygon:points=0,0 .5,1}}<br>"
        );
    }

    #[test]
    fn formats_numbers_like_anki() {
        assert_eq!(format_io_number(0.23251), ".2325");
        assert_eq!(format_io_number(1.0), "1");
        assert_eq!(format_io_number(0.0), "0");
        assert_eq!(format_io_number(12.5), "12.5");
        assert_eq!(format_io_number(-0.5), "-.5");
    }

    #[test]
    fn determines_the_mode_from_oi_of_the_active_shapes() {
        let s = parse_occlusion_field(FIELD);
        assert_eq!(occlusion_mode(&s, 1, OcclusionMode::HideOne), OcclusionMode::HideAll);
        assert_eq!(occlusion_mode(&s, 2, OcclusionMode::HideOne), OcclusionMode::HideOne);
        assert_eq!(occlusion_mode(&s, 9, OcclusionMode::HideAll), OcclusionMode::HideAll);
    }

    #[test]
    fn builds_a_payload_for_a_card_ordinal() {
        let p = occlusion_payload(FIELD, "<img src=\"brain&amp;skull.png\">", 0, OcclusionMode::HideOne).unwrap();
        assert_eq!(p.image, "brain&skull.png");
        assert_eq!(p.mode, OcclusionMode::HideAll);
        assert_eq!(p.active_ord, 1);
        assert_eq!(p.shapes.len(), 4);
        assert!(occlusion_payload("no shapes", "", 0, OcclusionMode::HideOne).is_none());
        assert_eq!(first_image_name("<img alt=x src='a b.png'>"), "a b.png");
        assert_eq!(first_image_name("<img src=c.png>"), "c.png");
        assert_eq!(first_image_name("none"), "");
    }

    #[test]
    fn detects_legacy_pixel_coordinates() {
        let shapes = |f: &str| parse_occlusion_field(f).into_iter().map(|s| s.shape).collect::<Vec<_>>();
        assert_eq!(occlusion_units(&shapes(FIELD)), OcclusionUnits::Normalized);
        assert_eq!(
            occlusion_units(&shapes("{{c1::image-occlusion:rect:left=120:top=40:width=30:height=20}}")),
            OcclusionUnits::Pixels
        );
    }
}
