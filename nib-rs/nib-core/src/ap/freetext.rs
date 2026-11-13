//! `/AP` for FreeText (`AnnotationDrawing.drawFreeText`): optional fill and
//! border, text word-wrapped with Base-14 WinAnsi metrics inside
//! `rect - 2·padding`.

use super::content::ContentBuilder;
use super::fonts::{to_winansi, Base14};
use super::line::ending_ops;
use super::{color_or, ApError, ApOutput, FontSource};
use crate::model::{
    BorderStyleKind, FontStyle, FontWeight, FreeTextAnnot, TextAlign, VerticalAlign,
};

const ASCENT: f64 = 0.75;

/// `/DA` default-appearance string for the writer (`/Helv 12 Tf 0 0 0 rg`).
pub fn da_string(a: &FreeTextAnnot) -> String {
    let bold = matches!(a.font_weight, Some(FontWeight::Bold));
    let italic = matches!(a.font_style, Some(FontStyle::Italic));
    let f = font_for(a, bold, italic).0;
    let size = a.font_size.unwrap_or(12.0);
    let c = color_or(&a.base.color, "#000000");
    let (r, g, b) = rgb(&c);
    format!(
        "/{} {} Tf {} {} {} rg",
        f.resource_name(),
        super::content::fmt_num(size),
        super::content::fmt_num(r),
        super::content::fmt_num(g),
        super::content::fmt_num(b)
    )
}

/// `/Q` quadding value: 0 left, 1 centered, 2 right.
pub fn q_value(a: &FreeTextAnnot) -> i64 {
    match a.text_align.unwrap_or(TextAlign::Left) {
        TextAlign::Left => 0,
        TextAlign::Center => 1,
        TextAlign::Right => 2,
    }
}

fn rgb(c: &crate::model::Color) -> (f64, f64, f64) {
    let h = c.0.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
    (p(0), p(2), p(4))
}

fn font_for(a: &FreeTextAnnot, bold: bool, italic: bool) -> (Base14, String) {
    let family = a.font_family.clone().unwrap_or_default();
    (Base14::from_family(&family, bold, italic), family)
}

/// Greedy word wrap of `text` into lines that fit `max_w` at `size`.
/// Long words are broken by character. Returns WinAnsi-encoded lines and
/// the total count of unencodable characters.
fn wrap(font: Base14, text: &str, size: f64, max_w: f64) -> (Vec<Vec<u8>>, usize) {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let space = font.text_width(b" ", size);
    for para in text.split('\n') {
        let mut line: Vec<u8> = Vec::new();
        let mut line_w = 0.0;
        for word in para.split(' ') {
            let (enc, _) = to_winansi(word);
            let w = font.text_width(&enc, size);
            let need = w + if line.is_empty() { 0.0 } else { space };
            if line_w + need > max_w && !line.is_empty() {
                lines.push(line);
                line = Vec::new();
                line_w = 0.0;
            }
            if w > max_w {
                // Break a long word by character.
                for &byte in &enc {
                    let cw = font.text_width(&[byte], size);
                    if line_w + cw > max_w && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                        line_w = 0.0;
                    }
                    line.push(byte);
                    line_w += cw;
                }
            } else {
                if !line.is_empty() {
                    line.push(b' ');
                    line_w += space;
                }
                line.extend_from_slice(&enc);
                line_w += w;
            }
        }
        lines.push(line);
    }
    let mut bad = 0usize;
    for c in text.chars() {
        let mut buf = [0u8; 4];
        let (_, b) = to_winansi(c.encode_utf8(&mut buf));
        bad += b as usize;
    }
    (lines, bad)
}

pub(crate) fn gen(a: &FreeTextAnnot, fonts: &dyn FontSource) -> Result<ApOutput, ApError> {
    let r = a.base.rect.ok_or(ApError::MissingGeometry)?.normalize();
    let mut warnings = Vec::new();
    let bold = matches!(a.font_weight, Some(FontWeight::Bold));
    let italic = matches!(a.font_style, Some(FontStyle::Italic));
    let (font, family) = font_for(a, bold, italic);
    if !Base14::is_base14_family(&family) && fonts.font(&family, bold, italic).is_some() {
        warnings.push(format!(
            "FreeText: embedded fonts not supported yet (family {family}), falling back to Helvetica"
        ));
    }

    let size = a.font_size.unwrap_or(12.0);
    let pad = a.padding.unwrap_or(2.0);
    let lh = a.line_height.unwrap_or(1.2) * size;
    let mut cb = ContentBuilder::new();

    if let Some(fill) = &a.fill_color {
        cb.fill_rgb(fill);
        cb.rect(r.x0, r.y0, r.width(), r.height());
        cb.fill();
    }
    let bw = a.border_width.unwrap_or(1.0);
    if let Some(border) = &a.border_color {
        cb.stroke_rgb(border);
        cb.line_width(bw);
        if let Some(dashed) = a
            .border_style
            .as_ref()
            .filter(|b| b.style == BorderStyleKind::Dashed)
        {
            let pat = dashed.dash.clone().unwrap_or_else(|| vec![8.0, 6.0]);
            cb.dash(&pat, 0.0);
        }
        cb.rect(r.x0, r.y0, r.width(), r.height());
        cb.stroke();
        cb.dash(&[], 0.0);
    }

    if let Some(callout) = &a.callout {
        if callout.points.len() >= 2 {
            cb.stroke_rgb(&color_or(&a.border_color, "#000000"));
            cb.line_width(bw);
            cb.move_to(callout.points[0].x, callout.points[0].y);
            for p in &callout.points[1..] {
                cb.line_to(p.x, p.y);
            }
            cb.stroke();
            let dir = (callout.points[0].y - callout.points[1].y)
                .atan2(callout.points[0].x - callout.points[1].x);
            if let Some(kind) = callout.line_ending {
                let color = color_or(&a.border_color, "#000000");
                ending_ops(
                    &mut cb,
                    callout.points[0],
                    dir + std::f64::consts::PI,
                    kind,
                    super::line::head_len(bw),
                    bw,
                    &color,
                    &color,
                );
            }
        }
    }

    let mut font_res = Vec::new();
    if let Some(text) = &a.base.contents {
        let max_w = (r.width() - 2.0 * pad).max(1.0);
        let (lines, n_bad) = wrap(font, text, size, max_w);
        if n_bad > 0 {
            warnings.push(format!(
                "FreeText: {n_bad} characters not encodable in WinAnsi, replaced by '?'"
            ));
        }
        let block_h = lines.len() as f64 * lh;
        let top = match a.vertical_align.unwrap_or(VerticalAlign::Top) {
            VerticalAlign::Top => r.y1 - pad,
            VerticalAlign::Middle => (r.y0 + r.y1) / 2.0 + block_h / 2.0,
            VerticalAlign::Bottom => r.y0 + pad + block_h,
        };
        let mut baseline = top - ASCENT * size;
        let text_color = color_or(&a.base.color, "#000000");
        cb.rect(r.x0, r.y0, r.width(), r.height());
        cb.clip();
        cb.fill_rgb(&text_color);
        cb.begin_text();
        cb.font(font.resource_name(), size);
        let mut prev_x: Option<f64> = None;
        for line in &lines {
            let lw = font.text_width(line, size);
            let x = match a.text_align.unwrap_or(TextAlign::Left) {
                TextAlign::Left => r.x0 + pad,
                TextAlign::Center => r.x0 + (r.width() - lw) / 2.0,
                TextAlign::Right => r.x1 - pad - lw,
            };
            match prev_x {
                None => cb.text_pos(x, baseline),
                Some(px) => {
                    // Td is relative to the previous line's start.
                    cb.text_pos(x - px, -lh)
                }
            }
            cb.show_text(line);
            prev_x = Some(x);
            baseline -= lh;
        }
        cb.end_text();
        font_res.push((font.resource_name().to_string(), font.font_dict()));
    }

    Ok(ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: font_res,
        images: vec![],
        warnings,
    })
}
