//! `/AP` for Stamp (`AnnotationDrawing.drawStamp`): image (contain/stretch,
//! centred), vector (viewBox-mapped `M L C Z` paths), or standard (red
//! border + centred name).

use base64::engine::general_purpose;
use base64::Engine;

use super::content::ContentBuilder;
use super::fonts::{to_winansi, Base14};
use super::image::embed_image;
use super::{ApError, ApOutput, AssetSource, ImageXObject};
use crate::geom::Rect;
use crate::model::{Color, StampAnnot, StampKind, VectorPath};

fn standard(
    cb: &mut ContentBuilder,
    r: &Rect,
    name: &str,
    fonts: &mut Vec<(String, lopdf::Dictionary)>,
) {
    let red = Color("#FF0000".to_string());
    cb.stroke_rgb(&red);
    cb.line_width(2.0);
    cb.rect(r.x0, r.y0, r.width(), r.height());
    cb.stroke();

    let font = Base14::HelveticaBold;
    let name = name.to_uppercase();
    let (enc, _) = to_winansi(&name);
    let mut size = r.height() * 0.5;
    while size > 1.0 && font.text_width(&enc, size) > r.width() - 4.0 {
        size -= 0.5;
    }
    let size = size.max(1.0);
    let tw = font.text_width(&enc, size);
    cb.fill_rgb(&red);
    cb.begin_text();
    cb.font(font.resource_name(), size);
    cb.text_pos(
        r.x0 + (r.width() - tw) / 2.0,
        r.y0 + r.height() / 2.0 - 0.25 * size,
    );
    cb.show_text(&enc);
    cb.end_text();
    fonts.push((font.resource_name().to_string(), font.font_dict()));
}

fn parse_vector_path(
    d: &str,
    cb: &mut ContentBuilder,
    map: impl Fn(f64, f64) -> (f64, f64),
) -> bool {
    let toks: Vec<&str> = d
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    let mut i = 0;
    let mut started = false;
    macro_rules! num {
        () => {
            match toks.get(i).and_then(|t| t.parse::<f64>().ok()) {
                Some(v) => {
                    i += 1;
                    v
                }
                None => return false,
            }
        };
    }
    while i < toks.len() {
        match toks[i] {
            "M" => {
                i += 1;
                let (x, y) = (num!(), num!());
                let (x, y) = map(x, y);
                cb.move_to(x, y);
                started = true;
            }
            "L" => {
                i += 1;
                let (x, y) = map(num!(), num!());
                cb.line_to(x, y);
            }
            "C" => {
                i += 1;
                let v = [num!(), num!(), num!(), num!(), num!(), num!()];
                let (x1, y1) = map(v[0], v[1]);
                let (x2, y2) = map(v[2], v[3]);
                let (x3, y3) = map(v[4], v[5]);
                cb.curve_to(x1, y1, x2, y2, x3, y3);
            }
            "Z" => {
                i += 1;
                cb.close();
            }
            _ => return false,
        }
    }
    started
}

fn vector(cb: &mut ContentBuilder, r: &Rect, s: &StampAnnot) {
    let Some(v) = &s.vector else { return };
    let vb = v.view_box;
    if vb[2] <= 0.0 || vb[3] <= 0.0 {
        return;
    }
    let sx = r.width() / vb[2];
    let sy = r.height() / vb[3];
    for p in &v.paths {
        let VectorPath {
            d,
            fill,
            stroke,
            stroke_width,
        } = p;
        let map = |x: f64, y: f64| (r.x0 + (x - vb[0]) * sx, r.y0 + (y - vb[1]) * sy);
        let mut tmp = ContentBuilder::new();
        if !parse_vector_path(d, &mut tmp, map) {
            continue;
        }
        if let Some(f) = fill {
            cb.fill_rgb(f);
        }
        if let Some(st) = stroke {
            cb.stroke_rgb(st);
            cb.line_width(stroke_width.unwrap_or(1.0) * sx);
        }
        cb.raw(std::str::from_utf8(&tmp.finish()).unwrap_or("").trim_end());
        match (fill.is_some(), stroke.is_some()) {
            (true, true) => cb.fill_stroke(),
            (true, false) => cb.fill(),
            (false, true) => cb.stroke(),
            _ => cb.end_path(),
        }
    }
}

fn decode_image_bytes(
    s: &StampAnnot,
    assets: &dyn AssetSource,
) -> Result<Option<Vec<u8>>, ApError> {
    let img = match &s.image {
        Some(i) => i,
        None => return Ok(None),
    };
    if let Some(b64) = &img.data_base64 {
        for engine in [
            &general_purpose::STANDARD,
            &general_purpose::STANDARD_NO_PAD,
            &general_purpose::URL_SAFE,
            &general_purpose::URL_SAFE_NO_PAD,
        ] {
            if let Ok(bytes) = engine.decode(b64.trim()) {
                return Ok(Some(bytes));
            }
        }
        return Err(ApError::Asset("dataBase64 does not decode".into()));
    }
    if let Some(id) = &img.asset_id {
        return Ok(assets.asset(id));
    }
    Ok(None)
}

pub(crate) fn gen(s: &StampAnnot, assets: &dyn AssetSource) -> Result<ApOutput, ApError> {
    let r = s.base.rect.ok_or(ApError::MissingGeometry)?.normalize();
    let mut cb = ContentBuilder::new();
    let mut images: Vec<(String, ImageXObject)> = Vec::new();
    let mut fonts = Vec::new();
    let mut warnings = Vec::new();

    let mut kind = s.kind.unwrap_or(StampKind::Standard);
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut fallback = false;
    if kind == StampKind::Image {
        match decode_image_bytes(s, assets) {
            Ok(Some(b)) => image_bytes = Some(b),
            Ok(None) => {
                warnings.push("Stamp: image data missing, drawing standard stamp".into());
                kind = StampKind::Standard;
                fallback = true;
            }
            Err(e) => {
                warnings.push(format!("Stamp: {e}, drawing standard stamp"));
                kind = StampKind::Standard;
                fallback = true;
            }
        }
    }

    match kind {
        StampKind::Image => {
            let bytes = image_bytes.expect("checked above");
            match embed_image(&bytes) {
                Ok((xo, iw, ih)) => {
                    let (w, h) = if s.preserve_aspect {
                        let scale = (r.width() / iw as f64).min(r.height() / ih as f64);
                        (iw as f64 * scale, ih as f64 * scale)
                    } else {
                        (r.width(), r.height())
                    };
                    cb.save();
                    cb.cm(
                        w,
                        0.0,
                        0.0,
                        h,
                        r.x0 + (r.width() - w) / 2.0,
                        r.y0 + (r.height() - h) / 2.0,
                    );
                    cb.do_xobject("Im0");
                    cb.restore();
                    images.push(("Im0".to_string(), xo));
                }
                Err(e) => {
                    warnings.push(format!("Stamp: {e}, drawing standard stamp"));
                    standard(&mut cb, &r, "IMAGE", &mut fonts);
                }
            }
        }
        StampKind::Vector => vector(&mut cb, &r, s),
        StampKind::Standard => standard(
            &mut cb,
            &r,
            if fallback {
                "IMAGE"
            } else {
                s.standard_name.as_deref().unwrap_or("STAMP")
            },
            &mut fonts,
        ),
    }

    Ok(ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts,
        images,
        warnings,
    })
}
