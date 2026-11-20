//! Annotation → `/Annot` dictionary mapping (JSON_SCHEMA §4–§5).
//! `fill_annot_dict` rewrites every managed key on a dict — used both for
//! fresh creates and for patching cloned dicts on update.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use lopdf::{Dictionary, Object, ObjectId};
use uuid::Uuid;

use crate::ap::freetext::{da_string, q_value};
use crate::geom::Rect;
use crate::model::annotation::*;
use crate::model::{Annotation, Color};

/// `/Subtype` name == model variant name.
pub fn subtype_name(a: &Annotation) -> &'static str {
    match a {
        Annotation::Ink(_) => "Ink",
        Annotation::Highlight(_) => "Highlight",
        Annotation::Underline(_) => "Underline",
        Annotation::StrikeOut(_) => "StrikeOut",
        Annotation::Squiggly(_) => "Squiggly",
        Annotation::FreeText(_) => "FreeText",
        Annotation::Text(_) => "Text",
        Annotation::Stamp(_) => "Stamp",
        Annotation::Square(_) => "Square",
        Annotation::Circle(_) => "Circle",
        Annotation::Line(_) => "Line",
    }
}

/// `/Rect` array → `Rect`.
pub fn rect_of(o: Option<&Object>) -> Option<Rect> {
    let arr = o?.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let f = |i: usize| num(&arr[i]);
    Some(Rect::new(f(0)?, f(1)?, f(2)?, f(3)?))
}

pub fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Real(f) => Some(*f as f64),
        Object::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

pub fn name_of(o: &Object) -> Option<String> {
    o.as_name()
        .ok()
        .map(|n| String::from_utf8_lossy(n).into_owned())
}

/// `#RRGGBB` → `/C`/`/IC` array of 3 reals.
pub fn color_arr(c: &Color) -> Vec<Object> {
    let h = c.0.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    vec![Object::Real(p(0)), Object::Real(p(2)), Object::Real(p(4))]
}

/// `/C`/`/IC` array → `#RRGGBB`. Handles gray (1), RGB (3), CMYK (4, naive
/// `1 - min(1, x + k)` conversion); empty → None.
pub fn color_of(o: Option<&Object>) -> Option<Color> {
    let arr = o?.as_array().ok()?;
    if arr.is_empty() {
        return None;
    }
    let v: Vec<f64> = arr.iter().filter_map(num).collect();
    let (r, g, b) = match v.len() {
        0 => return None,
        1 => (v[0], v[0], v[0]),
        3 => (v[0], v[1], v[2]),
        4 => (
            (1.0 - v[0]).max(0.0) * (1.0 - v[3]),
            (1.0 - v[1]).max(0.0) * (1.0 - v[3]),
            (1.0 - v[2]).max(0.0) * (1.0 - v[3]),
        ),
        _ => (v[0], v[1], v[2]),
    };
    let c = |x: f64| format!("{:02X}", (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    Some(Color(format!("#{}{}{}", c(r), c(g), c(b))))
}

/// `/F` bitmask from model flags; Text annots add NoZoom|NoRotate.
pub fn flags_int(a: &Annotation) -> i64 {
    let f = &a.base().flags;
    let mut bits = 0i64;
    if f.hidden {
        bits |= 2;
    }
    if f.print {
        bits |= 4;
    }
    if matches!(a, Annotation::Text(_)) {
        bits |= 8 | 16;
    }
    if f.no_view {
        bits |= 32;
    }
    if f.read_only {
        bits |= 64;
    }
    if f.locked {
        bits |= 128;
    }
    bits
}

pub fn flags_of(o: Option<&Object>) -> AnnotFlags {
    let bits = o.and_then(num).unwrap_or(4.0) as i64;
    AnnotFlags {
        hidden: bits & 2 != 0,
        print: bits & 4 != 0,
        no_view: bits & 32 != 0,
        read_only: bits & 64 != 0,
        locked: bits & 128 != 0,
    }
}

/// `DateTime<Utc>` → `D:YYYYMMDDHHmmSSZ` literal string.
pub fn pdf_date(dt: &DateTime<Utc>) -> Object {
    Object::string_literal(dt.format("D:%Y%m%d%H%M%SZ").to_string())
}

/// Parse a PDF date (`D:YYYYMMDDHHmmSS` + optional `Z`/`+HH'mm'`/`-HH'mm'`).
/// Tolerates partial dates and junk; `None` when nothing usable.
pub fn parse_pdf_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim().trim_start_matches("D:");
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    if digits.len() < 4 {
        return None;
    }
    let padded = format!("{:0<14}", digits);
    let y: i32 = padded[0..4].parse().ok()?;
    let mo: u32 = padded[4..6].parse().ok()?;
    let d: u32 = padded[6..8].parse().ok()?;
    let h: u32 = padded[8..10].parse().ok()?;
    let mi: u32 = padded[10..12].parse().ok()?;
    let sec: u32 = padded[12..14].parse().ok()?;
    let naive = chrono::NaiveDate::from_ymd_opt(y, mo, d)?.and_hms_opt(h, mi, sec)?;
    let mut off_secs = 0i64;
    let tz = &s[s.find(['Z', '+', '-']).unwrap_or(s.len())..];
    if let Some(t) = tz.strip_prefix(['+', '-']) {
        let sign = if tz.starts_with('-') { -1i64 } else { 1 };
        let td: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
        if td.len() >= 2 {
            let oh: i64 = td[0..2].parse().unwrap_or(0);
            let om: i64 = td.get(2..4).unwrap_or("0").parse().unwrap_or(0);
            off_secs = sign * (oh * 3600 + om * 60);
        }
    }
    Some(DateTime::from_naive_utc_and_offset(
        naive - chrono::Duration::seconds(off_secs),
        Utc,
    ))
}

/// `/BS` dict from stroke width + optional border style.
fn bs_dict(w: f64, style: Option<&BorderStyle>) -> Dictionary {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"Border".to_vec()));
    d.set("W", Object::Real(w as f32));
    match style {
        Some(bs) => {
            d.set(
                "S",
                Object::Name(
                    match bs.style {
                        BorderStyleKind::Solid => b"S",
                        BorderStyleKind::Dashed => b"D",
                        BorderStyleKind::Beveled => b"B",
                        BorderStyleKind::Inset => b"I",
                        BorderStyleKind::Underline => b"U",
                    }
                    .to_vec(),
                ),
            );
            if let Some(dash) = &bs.dash {
                d.set(
                    "D",
                    Object::Array(dash.iter().map(|v| Object::Real(*v as f32)).collect()),
                );
            }
        }
        None => d.set("S", Object::Name(b"S".to_vec())),
    }
    d
}

pub fn border_style_of(bs: Option<&Dictionary>) -> Option<BorderStyle> {
    let bs = bs?;
    let style = match bs.get(b"S").ok().and_then(name_of).as_deref() {
        Some("S") => BorderStyleKind::Solid,
        Some("D") => BorderStyleKind::Dashed,
        Some("B") => BorderStyleKind::Beveled,
        Some("I") => BorderStyleKind::Inset,
        Some("U") => BorderStyleKind::Underline,
        _ => return None,
    };
    let dash = bs.get(b"D").ok().and_then(|o| {
        o.as_array()
            .ok()
            .map(|a| a.iter().filter_map(num).collect())
    });
    Some(BorderStyle { style, dash })
}

pub fn line_ending_name(e: LineEnding) -> &'static str {
    match e {
        LineEnding::None => "None",
        LineEnding::Square => "Square",
        LineEnding::Circle => "Circle",
        LineEnding::Diamond => "Diamond",
        LineEnding::OpenArrow => "OpenArrow",
        LineEnding::ClosedArrow => "ClosedArrow",
        LineEnding::Butt => "Butt",
        LineEnding::ROpenArrow => "ROpenArrow",
        LineEnding::RClosedArrow => "RClosedArrow",
        LineEnding::Slash => "Slash",
    }
}

pub fn line_ending_of(name: &str) -> Option<LineEnding> {
    Some(match name {
        "None" => LineEnding::None,
        "Square" => LineEnding::Square,
        "Circle" => LineEnding::Circle,
        "Diamond" => LineEnding::Diamond,
        "OpenArrow" => LineEnding::OpenArrow,
        "ClosedArrow" => LineEnding::ClosedArrow,
        "Butt" => LineEnding::Butt,
        "ROpenArrow" => LineEnding::ROpenArrow,
        "RClosedArrow" => LineEnding::RClosedArrow,
        "Slash" => LineEnding::Slash,
        _ => return None,
    })
}

pub fn icon_name(i: TextIcon) -> &'static str {
    match i {
        TextIcon::Comment => "Comment",
        TextIcon::Key => "Key",
        TextIcon::Note => "Note",
        TextIcon::Help => "Help",
        TextIcon::NewParagraph => "NewParagraph",
        TextIcon::Paragraph => "Paragraph",
        TextIcon::Insert => "Insert",
    }
}

pub fn icon_of(name: &str) -> Option<TextIcon> {
    Some(match name {
        "Comment" => TextIcon::Comment,
        "Key" => TextIcon::Key,
        "Note" => TextIcon::Note,
        "Help" => TextIcon::Help,
        "NewParagraph" => TextIcon::NewParagraph,
        "Paragraph" => TextIcon::Paragraph,
        "Insert" => TextIcon::Insert,
        _ => return None,
    })
}

/// `/NibData` JSON: custom + intent + selectedText + signer, only non-empty.
pub fn nibdata(a: &Annotation) -> Option<String> {
    let mut m = serde_json::Map::new();
    if !a.base().custom.is_empty() {
        m.insert(
            "custom".into(),
            serde_json::Value::Object(a.base().custom.clone()),
        );
    }
    match a {
        Annotation::Ink(i) => {
            if let Some(i2) = &i.intent {
                m.insert("intent".into(), serde_json::to_value(i2).ok()?);
            }
            if let Some(s) = &i.signer {
                m.insert("signer".into(), serde_json::to_value(s).ok()?);
            }
            if let Some(field) = &i.bind_to_field {
                m.insert("bindToField".into(), field.clone().into());
            }
            if let Some(widget) = &i.bind_to_widget {
                m.insert("bindToWidget".into(), widget.clone().into());
            }
            if let Some(mode) = i.fit_mode {
                m.insert("fitMode".into(), serde_json::to_value(mode).ok()?);
            }
        }
        Annotation::Stamp(s) => {
            if let Some(i2) = &s.intent {
                m.insert("intent".into(), serde_json::to_value(i2).ok()?);
            }
            if let Some(s2) = &s.signer {
                m.insert("signer".into(), serde_json::to_value(s2).ok()?);
            }
            if let Some(field) = &s.bind_to_field {
                m.insert("bindToField".into(), field.clone().into());
            }
            if let Some(widget) = &s.bind_to_widget {
                m.insert("bindToWidget".into(), widget.clone().into());
            }
            if let Some(mode) = s.fit_mode {
                m.insert("fitMode".into(), serde_json::to_value(mode).ok()?);
            }
        }
        Annotation::Highlight(mk)
        | Annotation::Underline(mk)
        | Annotation::StrikeOut(mk)
        | Annotation::Squiggly(mk) => {
            if let Some(t) = &mk.selected_text {
                m.insert("selectedText".into(), t.clone().into());
            }
        }
        _ => {}
    }
    if m.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(m).to_string())
    }
}

/// Every key `fill_annot_dict` may write — removed first on update so stale
/// keys never survive a model change.
const MANAGED: &[&[u8]] = &[
    b"Type",
    b"Subtype",
    b"Rect",
    b"NM",
    b"F",
    b"CA",
    b"C",
    b"IC",
    b"T",
    b"Contents",
    b"Subj",
    b"CreationDate",
    b"M",
    b"NibData",
    b"IRT",
    b"RT",
    b"InkList",
    b"BS",
    b"QuadPoints",
    b"DA",
    b"Q",
    b"IT",
    b"CL",
    b"LE",
    b"Name",
    b"Open",
    b"State",
    b"StateModel",
    b"L",
    b"LL",
    b"LLE",
    b"Cap",
    b"CP",
    b"BE",
];

/// Rewrite all managed keys on `d` from the model. `/AP` and `/Popup` are
/// caller-managed and untouched here.
pub fn fill_annot_dict(
    d: &mut Dictionary,
    a: &Annotation,
    rect: Rect,
    ids: &HashMap<Uuid, ObjectId>,
    now: DateTime<Utc>,
) {
    for k in MANAGED {
        d.remove(k);
    }
    let r = rect.normalize();
    let base = a.base();
    d.set("Type", Object::Name(b"Annot".to_vec()));
    d.set("Subtype", Object::Name(subtype_name(a).as_bytes().to_vec()));
    d.set(
        "Rect",
        Object::Array(vec![
            Object::Real(r.x0 as f32),
            Object::Real(r.y0 as f32),
            Object::Real(r.x1 as f32),
            Object::Real(r.y1 as f32),
        ]),
    );
    d.set("NM", Object::string_literal(base.id.to_string()));
    d.set("F", Object::Integer(flags_int(a)));
    if base.opacity != 1.0 {
        d.set("CA", Object::Real(base.opacity as f32));
    }
    // /C: border colour for FreeText, stroke colour elsewhere.
    let c_key = match a {
        Annotation::FreeText(f) => f.border_color.as_ref().or(base.color.as_ref()),
        _ => base.color.as_ref(),
    };
    if let Some(c) = c_key {
        d.set("C", Object::Array(color_arr(c)));
    }
    if let Some(t) = &base.author {
        d.set("T", lopdf::text_string(t));
    }
    if let Some(t) = &base.contents {
        d.set("Contents", lopdf::text_string(t));
    }
    if let Some(t) = &base.subject {
        d.set("Subj", lopdf::text_string(t));
    }
    if let Some(t) = &base.created_at {
        d.set("CreationDate", pdf_date(t));
    }
    let m = base.modified_at.or(base.created_at).unwrap_or(now);
    d.set("M", pdf_date(&m));
    if let Some(j) = nibdata(a) {
        d.set("NibData", Object::string_literal(j));
    }
    if let Some(irt) = &base.in_reply_to {
        if let Some(oid) = ids.get(irt) {
            d.set("IRT", Object::Reference(*oid));
            d.set("RT", Object::Name(b"R".to_vec()));
        }
    }

    match a {
        Annotation::Ink(i) => {
            let inklist: Vec<Object> = i
                .strokes
                .iter()
                .map(|s| {
                    Object::Array(
                        s.points
                            .iter()
                            .filter(|p| p.len() >= 2)
                            .flat_map(|p| [Object::Real(p[0] as f32), Object::Real(p[1] as f32)])
                            .collect(),
                    )
                })
                .collect();
            d.set("InkList", Object::Array(inklist));
            d.set(
                "BS",
                Object::Dictionary(bs_dict(i.stroke_width.unwrap_or(2.0), None)),
            );
        }
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => {
            let qp: Vec<Object> = m
                .quad_points
                .iter()
                .flat_map(|q| {
                    q.0.iter()
                        .map(|v| Object::Real(*v as f32))
                        .collect::<Vec<_>>()
                })
                .collect();
            d.set("QuadPoints", Object::Array(qp));
        }
        Annotation::FreeText(f) => {
            d.set("DA", Object::string_literal(da_string(f)));
            d.set("Q", Object::Integer(q_value(f)));
            if let Some(ic) = &f.fill_color {
                d.set("IC", Object::Array(color_arr(ic)));
            }
            if f.border_color.is_some() || f.border_width.is_some() || f.border_style.is_some() {
                d.set(
                    "BS",
                    Object::Dictionary(bs_dict(
                        f.border_width.unwrap_or(1.0),
                        f.border_style.as_ref(),
                    )),
                );
            }
            if let Some(c) = &f.callout {
                d.set("IT", Object::Name(b"FreeTextCallout".to_vec()));
                d.set(
                    "CL",
                    Object::Array(
                        c.points
                            .iter()
                            .flat_map(|p| [Object::Real(p.x as f32), Object::Real(p.y as f32)])
                            .collect(),
                    ),
                );
                if let Some(le) = c.line_ending {
                    d.set("LE", Object::Name(line_ending_name(le).as_bytes().to_vec()));
                }
            }
        }
        Annotation::Text(t) => {
            d.set(
                "Name",
                Object::Name(
                    icon_name(t.icon.unwrap_or(TextIcon::Comment))
                        .as_bytes()
                        .to_vec(),
                ),
            );
            if let Some(open) = t.open {
                d.set("Open", Object::Boolean(open));
            }
            if let Some(state) = t.state {
                let (state_name, model) = match state {
                    ReviewState::Accepted => ("Accepted", "Review"),
                    ReviewState::Rejected => ("Rejected", "Review"),
                    ReviewState::Cancelled => ("Cancelled", "Review"),
                    ReviewState::Completed => ("Completed", "Review"),
                    ReviewState::Marked => ("Marked", "Marked"),
                    ReviewState::Unmarked => ("Unmarked", "Marked"),
                };
                d.set("State", Object::Name(state_name.as_bytes().to_vec()));
                d.set("StateModel", Object::Name(model.as_bytes().to_vec()));
            }
        }
        Annotation::Stamp(s) => {
            let name = s
                .standard_name
                .as_deref()
                .map(sanitize_name)
                .unwrap_or_else(|| "Draft".to_string());
            d.set("Name", Object::Name(name.into_bytes()));
        }
        Annotation::Square(s) | Annotation::Circle(s) => {
            if let Some(ic) = &s.fill_color {
                d.set("IC", Object::Array(color_arr(ic)));
            }
            d.set(
                "BS",
                Object::Dictionary(bs_dict(
                    s.stroke_width.unwrap_or(2.0),
                    s.border_style.as_ref(),
                )),
            );
        }
        Annotation::Line(l) => {
            d.set(
                "L",
                Object::Array(vec![
                    Object::Real(l.start.x as f32),
                    Object::Real(l.start.y as f32),
                    Object::Real(l.end.x as f32),
                    Object::Real(l.end.y as f32),
                ]),
            );
            if let Some(les) = &l.line_endings {
                d.set(
                    "LE",
                    Object::Array(
                        les.iter()
                            .map(|e| Object::Name(line_ending_name(*e).as_bytes().to_vec()))
                            .collect(),
                    ),
                );
            }
            if let Some(ic) = &l.fill_color {
                d.set("IC", Object::Array(color_arr(ic)));
            }
            if let Some(ll) = l.leader_length {
                d.set("LL", Object::Real(ll as f32));
            }
            if let Some(lle) = l.leader_extension {
                d.set("LLE", Object::Real(lle as f32));
            }
            if let Some(cap) = &l.caption {
                d.set("Cap", Object::Boolean(true));
                d.set(
                    "CP",
                    Object::Name(
                        match cap.position {
                            CaptionPosition::Inline => b"Inline".as_slice(),
                            CaptionPosition::Top => b"Top".as_slice(),
                        }
                        .to_vec(),
                    ),
                );
            }
        }
    }
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}
