//! Import pre-existing `/Annots` into the model. Widgets, Links, Popups and
//! unknown subtypes are left untouched in the file and skipped.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Object, ObjectId};
use serde_json::Value;
use sha2::Digest;
use uuid::Uuid;

use super::dict;
use super::NIB_NS;
use crate::geom::{Point, QuadPoints};
use crate::model::annotation::*;
use crate::model::{Annotation, Color};

/// Returns (annotations in page then /Annots order, id→annot oid,
/// id→/AP object, parent-id→popup oid).
pub type ImportMaps = (
    Vec<Annotation>,
    HashMap<Uuid, ObjectId>,
    HashMap<Uuid, Object>,
    HashMap<Uuid, ObjectId>,
);

pub fn import(doc: &Document, page_ids: &[ObjectId]) -> ImportMaps {
    let mut annots = Vec::new();
    let mut ids = HashMap::new();
    let mut foreign_ap = HashMap::new();
    let mut popups = HashMap::new();
    for &page_id in page_ids {
        let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict()) else {
            continue;
        };
        let Ok(annots_obj) = page.get(b"Annots") else {
            continue;
        };
        // /Annots may be an inline array or a reference to one.
        let entries: Vec<Object> = match annots_obj {
            Object::Array(a) => a.clone(),
            Object::Reference(r) => match doc.get_object(*r).and_then(|o| o.as_array()) {
                Ok(a) => a.clone(),
                Err(_) => continue,
            },
            _ => continue,
        };
        for entry in entries {
            let (oid, dict) = match &entry {
                Object::Reference(r) => match doc.get_object(*r).and_then(|o| o.as_dict()) {
                    Ok(d) => (Some(*r), d),
                    Err(_) => continue,
                },
                Object::Dictionary(d) => (None, d),
                _ => continue,
            };
            let Some(mut annot) = annot_from_dict(doc, dict, oid) else {
                continue;
            };
            let id = annot.base().id;
            if let Some(oid) = oid {
                ids.insert(id, oid);
            }
            if let Ok(ap) = dict.get(b"AP") {
                let hash = ap_hash(doc, ap);
                annot.base_mut().appearance = Appearance {
                    regenerate: false,
                    ap_hash: hash.map(|h| format!("sha256:{h}")),
                };
                foreign_ap.insert(id, ap.clone());
            }
            if let Ok(pop_obj) = dict.get(b"Popup") {
                if let Object::Reference(pid) = pop_obj {
                    popups.insert(id, *pid);
                }
                if let Ok(pdict) = pop_obj
                    .as_reference()
                    .and_then(|r| doc.get_object(r))
                    .and_then(|o| o.as_dict())
                    .or_else(|_| pop_obj.as_dict())
                {
                    annot.base_mut().popup = Some(Popup {
                        open: pdict
                            .get(b"Open")
                            .ok()
                            .and_then(|o| o.as_bool().ok())
                            .unwrap_or(false),
                        rect: dict::rect_of(pdict.get(b"Rect").ok()),
                    });
                }
            }
            annots.push(annot);
        }
    }
    (annots, ids, foreign_ap, popups)
}

fn annot_from_dict(doc: &Document, d: &Dictionary, oid: Option<ObjectId>) -> Option<Annotation> {
    let subtype = dict::name_of(d.get(b"Subtype").ok()?)?;
    if matches!(subtype.as_str(), "Widget" | "Link" | "Popup") {
        return None;
    }
    let mut base = base_of(doc, d, oid)?;
    match subtype.as_str() {
        "Ink" => {
            let strokes = d
                .get(b"InkList")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|l| {
                    l.iter()
                        .filter_map(|s| s.as_array().ok())
                        .map(|s| {
                            let nums: Vec<f64> = s.iter().filter_map(dict::num).collect();
                            Stroke {
                                points: nums
                                    .as_chunks::<2>()
                                    .0
                                    .iter()
                                    .map(|c| vec![c[0], c[1]])
                                    .collect(),
                                tool: None,
                                started_at: None,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let stroke_width = d
                .get(b"BS")
                .ok()
                .and_then(|o| o.as_dict().ok())
                .and_then(|bs| bs.get(b"W").ok())
                .and_then(dict::num);
            let (intent, signer, custom) = nibdata_of(d);
            let _ = custom;
            base.custom = custom.unwrap_or_default();
            Some(Annotation::Ink(InkAnnot {
                base,
                stroke_width,
                line_cap: None,
                line_join: None,
                pressure_sensitive: None,
                min_width_factor: None,
                max_width_factor: None,
                smoothing: None,
                strokes,
                intent,
                signer,
                bind_to_field: None,
                fit_mode: None,
            }))
        }
        "Highlight" | "Underline" | "StrikeOut" | "Squiggly" => {
            let flat: Vec<f64> = d
                .get(b"QuadPoints")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| a.iter().filter_map(dict::num).collect())
                .unwrap_or_default();
            let quad_points: Vec<QuadPoints> = flat
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| QuadPoints::new(*c).normalize())
                .collect();
            let (_, _, custom) = nibdata_of(d);
            let selected_text = nibdata_key(d, "selectedText")
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            base.custom = custom.unwrap_or_default();
            let m = MarkupAnnot {
                base,
                quad_points,
                blend_mode: None,
                selected_text,
                line_width_factor: None,
            };
            Some(match subtype.as_str() {
                "Highlight" => Annotation::Highlight(m),
                "Underline" => Annotation::Underline(m),
                "StrikeOut" => Annotation::StrikeOut(m),
                _ => Annotation::Squiggly(m),
            })
        }
        "FreeText" => {
            let (family, size, da_color) = parse_da(d.get(b"DA").ok());
            if base.color.is_none() {
                base.color = da_color;
            }
            let (_, _, custom) = nibdata_of(d);
            base.custom = custom.unwrap_or_default();
            let callout = d.get(b"CL").ok().and_then(|o| o.as_array().ok()).map(|a| {
                let nums: Vec<f64> = a.iter().filter_map(dict::num).collect();
                Callout {
                    points: nums
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| Point::new(c[0], c[1]))
                        .collect(),
                    line_ending: d
                        .get(b"LE")
                        .ok()
                        .and_then(|o| o.as_name().ok())
                        .and_then(|n| dict::line_ending_of(&String::from_utf8_lossy(n))),
                }
            });
            Some(Annotation::FreeText(FreeTextAnnot {
                base,
                font_family: family,
                font_size: size,
                font_weight: None,
                font_style: None,
                text_align: d.get(b"Q").ok().and_then(dict::num).map(|q| match q as i64 {
                    1 => TextAlign::Center,
                    2 => TextAlign::Right,
                    _ => TextAlign::Left,
                }),
                vertical_align: None,
                line_height: None,
                padding: None,
                fill_color: dict::color_of(d.get(b"IC").ok()),
                border_color: dict::color_of(d.get(b"C").ok()),
                border_width: d
                    .get(b"BS")
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .and_then(|bs| bs.get(b"W").ok())
                    .and_then(dict::num),
                border_style: dict::border_style_of(
                    d.get(b"BS").ok().and_then(|o| o.as_dict().ok()),
                ),
                rotation: d.get(b"Rotate").ok().and_then(dict::num),
                callout,
            }))
        }
        "Text" => {
            let icon = d
                .get(b"Name")
                .ok()
                .and_then(|o| o.as_name().ok())
                .and_then(|n| dict::icon_of(&String::from_utf8_lossy(n)));
            let (_, _, custom) = nibdata_of(d);
            base.custom = custom.unwrap_or_default();
            let state = d
                .get(b"State")
                .ok()
                .and_then(|o| o.as_name().ok().or_else(|| o.as_str().ok()))
                .and_then(|n| match String::from_utf8_lossy(n).to_lowercase().as_str() {
                    "accepted" => Some(ReviewState::Accepted),
                    "rejected" => Some(ReviewState::Rejected),
                    "cancelled" => Some(ReviewState::Cancelled),
                    "completed" => Some(ReviewState::Completed),
                    "marked" => Some(ReviewState::Marked),
                    "unmarked" => Some(ReviewState::Unmarked),
                    _ => None,
                });
            Some(Annotation::Text(TextAnnot {
                base,
                icon,
                open: d
                    .get(b"Open")
                    .ok()
                    .and_then(|o| o.as_bool().ok()),
                state,
            }))
        }
        "Stamp" => {
            let name = d.get(b"Name").ok().and_then(dict::name_of);
            let (intent, signer, custom) = nibdata_of(d);
            base.custom = custom.unwrap_or_default();
            Some(Annotation::Stamp(StampAnnot {
                base,
                kind: name.as_ref().map(|_| StampKind::Standard),
                image: None,
                vector: None,
                standard_name: name,
                rotation: None,
                preserve_aspect: true,
                intent,
                signer,
                bind_to_field: None,
                fit_mode: None,
            }))
        }
        "Square" | "Circle" => {
            let bs = d.get(b"BS").ok().and_then(|o| o.as_dict().ok());
            let s = ShapeAnnot {
                base,
                fill_color: dict::color_of(d.get(b"IC").ok()),
                stroke_width: bs.and_then(|b| b.get(b"W").ok()).and_then(dict::num),
                border_style: dict::border_style_of(bs),
                cloudy: None,
            };
            Some(if subtype == "Square" {
                Annotation::Square(s)
            } else {
                Annotation::Circle(s)
            })
        }
        "Line" => {
            let l = d.get(b"L").ok().and_then(|o| o.as_array().ok());
            let nums: Vec<f64> = l
                .map(|a| a.iter().filter_map(dict::num).collect())
                .unwrap_or_default();
            if nums.len() < 4 {
                return None;
            }
            let endings = d.get(b"LE").ok().and_then(|o| o.as_array().ok()).map(|a| {
                a.iter()
                    .filter_map(|o| o.as_name().ok())
                    .filter_map(|n| dict::line_ending_of(&String::from_utf8_lossy(n)))
                    .collect::<Vec<_>>()
            });
            let caption = d
                .get(b"Cap")
                .ok()
                .and_then(|o| o.as_bool().ok())
                .filter(|b| *b)
                .map(|_| LineCaption {
                    text: base.contents.clone().unwrap_or_default(),
                    position: match d.get(b"CP").ok().and_then(dict::name_of).as_deref() {
                        Some("Top") => CaptionPosition::Top,
                        _ => CaptionPosition::Inline,
                    },
                });
            Some(Annotation::Line(LineAnnot {
                base,
                start: Point::new(nums[0], nums[1]),
                end: Point::new(nums[2], nums[3]),
                stroke_width: d
                    .get(b"BS")
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .and_then(|b| b.get(b"W").ok())
                    .and_then(dict::num),
                line_endings: endings,
                fill_color: dict::color_of(d.get(b"IC").ok()),
                leader_length: d.get(b"LL").ok().and_then(dict::num),
                leader_extension: d.get(b"LLE").ok().and_then(dict::num),
                caption,
            }))
        }
        _ => None,
    }
}

/// Shared `/Annot` keys → `AnnotBase`. `oid` seeds the deterministic fallback
/// id when `/NM` is absent or unparsable.
fn base_of(doc: &Document, d: &Dictionary, oid: Option<ObjectId>) -> Option<AnnotBase> {
    let nm = d
        .get(b"NM")
        .ok()
        .and_then(|o| o.as_str().ok())
        .map(|s| String::from_utf8_lossy(s).into_owned());
    let id = nm
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| {
            let (o, g) = oid.unwrap_or((0, 0));
            Uuid::new_v5(&NIB_NS, format!("{o}:{g}").as_bytes())
        });
    let page = doc.get_object_page(oid?).ok().and_then(|pid| {
        doc.get_pages()
            .iter()
            .find(|(_, v)| **v == pid)
            .map(|(k, _)| *k)
    });
    let irt = d
        .get(b"IRT")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|r| {
            doc.get_object(r)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .and_then(|pd| {
                    pd.get(b"NM")
                        .ok()
                        .and_then(|o| o.as_str().ok())
                        .and_then(|s| Uuid::parse_str(&String::from_utf8_lossy(s)).ok())
                        .or_else(|| {
                            Some(Uuid::new_v5(&NIB_NS, format!("{}:{}", r.0, r.1).as_bytes()))
                        })
                })
        });
    Some(AnnotBase {
        id,
        page: page.unwrap_or(0),
        rect: dict::rect_of(d.get(b"Rect").ok()),
        color: dict::color_of(d.get(b"C").ok()),
        opacity: d.get(b"CA").ok().and_then(dict::num).unwrap_or(1.0),
        author: text_of(d.get(b"T").ok()),
        contents: text_of(d.get(b"Contents").ok()),
        subject: text_of(d.get(b"Subj").ok()),
        created_at: d
            .get(b"CreationDate")
            .ok()
            .and_then(|o| o.as_str().ok())
            .and_then(|s| dict::parse_pdf_date(&String::from_utf8_lossy(s))),
        modified_at: d
            .get(b"M")
            .ok()
            .and_then(|o| o.as_str().ok())
            .and_then(|s| dict::parse_pdf_date(&String::from_utf8_lossy(s))),
        flags: dict::flags_of(d.get(b"F").ok()),
        in_reply_to: irt,
        popup: None,
        appearance: Appearance::default(),
        custom: Default::default(),
        extra: Default::default(),
    })
}

fn text_of(o: Option<&Object>) -> Option<String> {
    match o? {
        o2 @ Object::String(..) => lopdf::decode_text_string(o2).ok(),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

fn nibdata_key(d: &Dictionary, key: &str) -> Option<Value> {
    let raw = text_of(d.get(b"NibData").ok())?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get(key).cloned()
}

/// Extract `intent`, `signer`, `custom` from `/NibData`.
fn nibdata_of(
    d: &Dictionary,
) -> (Option<Intent>, Option<VisualSigner>, Option<serde_json::Map<String, Value>>) {
    let intent = nibdata_key(d, "intent")
        .and_then(|v| serde_json::from_value(v).ok());
    let signer = nibdata_key(d, "signer")
        .and_then(|v| serde_json::from_value(v).ok());
    let custom = nibdata_key(d, "custom").and_then(|v| {
        if let Value::Object(m) = v {
            Some(m)
        } else {
            None
        }
    });
    (intent, signer, custom)
}

/// Parse `/DA`: `/Name size Tf r g b rg` → (family, size, text color).
fn parse_da(o: Option<&Object>) -> (Option<String>, Option<f64>, Option<Color>) {
    let Some(s) = text_of(o) else {
        return (None, None, None);
    };
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut family = None;
    let mut size = None;
    let mut color = None;
    for (i, t) in toks.iter().enumerate() {
        if *t == "Tf" && i >= 2 {
            let name = toks[i - 2].trim_start_matches('/');
            family = Some(
                match name {
                    n if n.starts_with("Helv") || n.starts_with("He") => "Helvetica",
                    n if n.starts_with("Ti") => "Times",
                    n if n.starts_with("Co") => "Courier",
                    n => n,
                }
                .to_string(),
            );
            size = toks[i - 1].parse::<f64>().ok();
        }
        if *t == "rg" && i >= 3 {
            let v: Option<Vec<f64>> = toks[i - 3..i]
                .iter()
                .map(|t| t.parse::<f64>().ok())
                .collect();
            if let Some(v) = v {
                let c = |x: f64| format!("{:02X}", (x.clamp(0.0, 1.0) * 255.0).round() as u8);
                color = Some(Color(format!("#{}{}{}", c(v[0]), c(v[1]), c(v[2]))));
            }
        }
    }
    (family, size, color)
}

/// sha256 hex over the decoded `/AP /N` stream bytes (all appearance
/// substates concatenated, sorted by state key for determinism).
fn ap_hash(doc: &Document, ap: &Object) -> Option<String> {
    let d = match ap {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => doc.get_object(*r).ok()?.as_dict().ok().cloned(),
        _ => None,
    }?;
    let n = d.get(b"N").ok()?;
    let mut streams: Vec<Vec<u8>> = Vec::new();
    collect_streams(doc, n, &mut streams);
    if streams.is_empty() {
        return None;
    }
    let mut h = sha2::Sha256::new();
    for s in streams {
        h.update(&s);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn collect_streams(doc: &Document, o: &Object, out: &mut Vec<Vec<u8>>) {
    match o {
        Object::Reference(r) => {
            if let Ok(Object::Stream(s)) = doc.get_object(*r) {
                out.push(
                    s.decompressed_content()
                        .unwrap_or_else(|_| s.content.clone()),
                );
            }
        }
        Object::Stream(s) => out.push(
            s.decompressed_content()
                .unwrap_or_else(|_| s.content.clone()),
        ),
        Object::Dictionary(d) => {
            let mut keys: Vec<&Vec<u8>> = d.iter().map(|(k, _)| k).collect();
            keys.sort();
            for k in keys {
                if let Ok(v) = d.get(k.as_slice()) {
                    collect_streams(doc, v, out);
                }
            }
        }
        _ => {}
    }
}
