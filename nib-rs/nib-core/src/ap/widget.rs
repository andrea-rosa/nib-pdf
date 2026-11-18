//! `/AP` for AcroForm widget annotations (M4).
//!
//! Widgets are always fully opaque: `ApOutput.ext_gstate` is empty (no
//! `/GS0` is emitted). Form space == user space (`/BBox` = widget rect,
//! identity `/Matrix`). The value text of text/choice fields is wrapped in
//! `/Tx BMC q … Q EMC`; Btn widgets draw only the frame plus their state
//! glyph (no `/D` down-appearance is emitted).

use super::content::ContentBuilder;
use super::fonts::{to_winansi, Base14};
use super::{color_or, ApError, ApOutput, FontSource};
use crate::geom::Rect;
use crate::model::{BorderStyleKind, FieldType, FormField, TextAlign, Widget};

pub use super::fonts::{zapf_dingbats_dict, ZADB_CHECK, ZADB_DOT};

/// Appearance streams of one widget: `(state, output)` pairs. `""` is the
/// key for single-state `/N`; Btn widgets produce `export` + `Off`.
pub struct WidgetAp {
    pub states: Vec<(String, ApOutput)>,
}

const PAD: f64 = 2.0;

/// Build the appearance of `widget` of `field`.
pub fn build_widget(
    field: &FormField,
    widget: &Widget,
    fonts: &dyn FontSource,
) -> Result<WidgetAp, ApError> {
    let r = widget.rect.normalize();
    if !(r.x1 > r.x0 && r.y1 > r.y0) {
        return Err(ApError::MissingGeometry);
    }
    let mut warnings = Vec::new();
    let (font, family) = da_font(field);
    if !Base14::is_base14_family(&family)
        && !family.is_empty()
        && fonts.font(&family, false, false).is_some()
    {
        warnings.push(format!(
            "widget {}: embedded fonts not supported yet (family {family}), falling back to Helvetica",
            field.name
        ));
    }

    match field.field_type {
        FieldType::Checkbox => {
            let export = widget
                .export_value
                .clone()
                .or_else(|| {
                    field
                        .export_values
                        .as_ref()
                        .and_then(|v| v.first().cloned())
                })
                .unwrap_or_else(|| "Yes".into());
            Ok(WidgetAp {
                states: vec![
                    (export, btn_state(field, widget, r, ZADB_CHECK, 0.8, false)),
                    ("Off".into(), btn_off(field, widget, r, false)),
                ],
            })
        }
        FieldType::Radio => {
            let export = widget.export_value.clone().unwrap_or_else(|| "On".into());
            Ok(WidgetAp {
                states: vec![
                    (export, btn_state(field, widget, r, ZADB_DOT, 0.5, true)),
                    ("Off".into(), btn_off(field, widget, r, true)),
                ],
            })
        }
        _ => {
            let mut cb = ContentBuilder::new();
            cb.raw("/Tx BMC");
            cb.save();
            frame(&mut cb, field, widget, r, false);
            let mut ctx = DrawCtx {
                field,
                font,
                font_res: Vec::new(),
                warnings,
            };
            match field.field_type {
                FieldType::Combo => {
                    let label = combo_label(field);
                    draw_text_line(&mut cb, &mut ctx, r, &label);
                }
                FieldType::List => draw_list(&mut cb, &mut ctx, r),
                FieldType::PushButton => {
                    if let Some(caption) = &field.caption {
                        let caption = caption.clone();
                        draw_text_line(&mut cb, &mut ctx, r, &caption);
                    }
                }
                _ => draw_text_field(&mut cb, &mut ctx, r),
            }
            warnings = ctx.warnings;
            let font_res = ctx.font_res;
            cb.restore();
            cb.raw("EMC");
            Ok(WidgetAp {
                states: vec![(
                    String::new(),
                    ApOutput {
                        bbox: r,
                        content: cb.finish(),
                        ext_gstate: vec![],
                        fonts: font_res,
                        images: vec![],
                        warnings,
                    },
                )],
            })
        }
    }
}

/// `/DA` → (Base14, family). Unknown families fall back to Helvetica.
fn da_font(field: &FormField) -> (Base14, String) {
    let family = field
        .default_appearance
        .as_ref()
        .and_then(|da| da.font_family.clone())
        .unwrap_or_default();
    (Base14::from_family(&family, false, false), family)
}

fn da_size(field: &FormField) -> Option<f64> {
    field
        .default_appearance
        .as_ref()
        .and_then(|da| da.font_size)
        .filter(|s| *s > 0.0)
}

fn da_color(field: &FormField) -> crate::model::Color {
    color_or(
        &field
            .default_appearance
            .as_ref()
            .and_then(|da| da.color.clone()),
        "#000000",
    )
}

/// `/MK /BG` fill + `/MK /BC` stroke (inset w/2, dashed when the border
/// style is Dashed). `circle` draws the ellipse frame used by radio kids.
fn frame(cb: &mut ContentBuilder, _field: &FormField, widget: &Widget, r: Rect, circle: bool) {
    if let Some(bg) = &widget.background_color {
        cb.fill_rgb(bg);
        path(cb, r, circle);
        cb.fill();
    }
    if let Some(bc) = &widget.border_color {
        let w = widget.border_width.unwrap_or(1.0);
        let half = w / 2.0;
        let inset = Rect::new(r.x0 + half, r.y0 + half, r.x1 - half, r.y1 - half);
        let i = if inset.x0 < inset.x1 && inset.y0 < inset.y1 {
            inset
        } else {
            r
        };
        cb.stroke_rgb(bc);
        cb.line_width(w);
        if let Some(dashed) = widget
            .border_style
            .as_ref()
            .filter(|b| b.style == BorderStyleKind::Dashed)
        {
            let pat = dashed.dash.clone().unwrap_or_else(|| vec![3.0, 2.0]);
            cb.dash(&pat, 0.0);
        }
        path(cb, i, circle);
        cb.stroke();
        cb.dash(&[], 0.0);
    }
}

fn path(cb: &mut ContentBuilder, r: Rect, circle: bool) {
    if circle {
        ellipse(
            cb,
            (r.x0 + r.x1) / 2.0,
            (r.y0 + r.y1) / 2.0,
            r.width() / 2.0,
            r.height() / 2.0,
        );
    } else {
        cb.rect(r.x0, r.y0, r.width(), r.height());
    }
}

/// κ = 0.5523 circle approximation (same math as `shape.rs`).
const KAPPA: f64 = 0.5523;

fn ellipse(cb: &mut ContentBuilder, cx: f64, cy: f64, rx: f64, ry: f64) {
    let kx = rx * KAPPA;
    let ky = ry * KAPPA;
    cb.move_to(cx + rx, cy);
    cb.curve_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    cb.curve_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    cb.curve_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    cb.curve_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    cb.close();
}

/// Btn on-state: frame + centred ZapfDingbats glyph.
fn btn_state(
    field: &FormField,
    widget: &Widget,
    r: Rect,
    glyph: (u8, f64),
    size_factor: f64,
    circle: bool,
) -> ApOutput {
    let mut cb = ContentBuilder::new();
    frame(&mut cb, field, widget, r, circle);
    let size = da_size(field)
        .filter(|s| *s > 0.0)
        .unwrap_or_else(|| r.width().min(r.height()) * size_factor);
    let (byte, advance) = glyph;

    let x = (r.x0 + r.x1) / 2.0 - advance * size / 2.0;
    let y = (r.y0 + r.y1) / 2.0 - 0.35 * size;
    cb.begin_text();
    cb.font("ZaDb", size);
    cb.text_pos(x, y);
    cb.show_text(&[byte]);
    cb.end_text();
    ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![("ZaDb".to_string(), zapf_dingbats_dict())],
        images: vec![],
        warnings: vec![],
    }
}

/// Btn off-state: frame only.
fn btn_off(field: &FormField, widget: &Widget, r: Rect, circle: bool) -> ApOutput {
    let mut cb = ContentBuilder::new();
    frame(&mut cb, field, widget, r, circle);
    ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        warnings: vec![],
    }
}

fn value_string(field: &FormField) -> String {
    match &field.value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// Shrink `size` (from 12) by 0.5 until the text fits `inner_w`×`inner_h`,
/// floor 4. Used when the `/DA` font size is 0/absent.
fn autosize(font: Base14, text: &[u8], inner_w: f64, inner_h: f64) -> f64 {
    let mut size = 12.0;
    while size > 4.0 {
        if font.text_width(text, size) <= inner_w && 1.2 * size <= inner_h {
            break;
        }
        size -= 0.5;
    }
    size.max(4.0)
}

fn text_x(field: &FormField, r: Rect, line_w: f64) -> f64 {
    match field.text_align.unwrap_or(TextAlign::Left) {
        TextAlign::Left => r.x0 + PAD,
        TextAlign::Center => r.x0 + (r.width() - line_w) / 2.0,
        TextAlign::Right => r.x1 - PAD - line_w,
    }
}

/// Shared draw context: field model, resolved font, output collections.
struct DrawCtx<'a> {
    field: &'a FormField,
    font: Base14,
    font_res: Vec<(String, lopdf::Dictionary)>,
    warnings: Vec<String>,
}

impl DrawCtx<'_> {
    fn warn_unencodable(&mut self, bad: bool) {
        if bad {
            self.warnings.push(format!(
                "field {}: characters not encodable in WinAnsi, replaced by '?'",
                self.field.name
            ));
        }
    }
}

/// One-line text (`Tx` non-multiline, combo, pushButton caption): pad 2,
/// clipped, single centred baseline.
fn draw_text_line(cb: &mut ContentBuilder, ctx: &mut DrawCtx<'_>, r: Rect, text: &str) {
    let field = ctx.field;
    let font = ctx.font;
    let (enc, bad) = to_winansi(text);
    ctx.warn_unencodable(bad);
    let inner_w = (r.width() - 2.0 * PAD).max(1.0);
    let inner_h = (r.height() - 2.0 * PAD).max(1.0);
    let size = da_size(field).unwrap_or_else(|| autosize(font, &enc, inner_w, inner_h));
    cb.rect(r.x0, r.y0, r.width(), r.height());
    cb.clip();
    cb.fill_rgb(&da_color(field));
    cb.begin_text();
    cb.font(font.resource_name(), size);
    let baseline = (r.y0 + r.y1) / 2.0 - 0.35 * size;
    let x = text_x(field, r, font.text_width(&enc, size));
    cb.text_pos(x, baseline);
    cb.show_text(&enc);
    cb.end_text();
    ctx.font_res
        .push((font.resource_name().to_string(), font.font_dict()));
}

/// `/Tx` body: comb cells, password mask, multiline wrap or single line.
fn draw_text_field(cb: &mut ContentBuilder, ctx: &mut DrawCtx<'_>, r: Rect) {
    let field = ctx.field;
    let mut text = value_string(field);
    if field.flags.password {
        text = "*".repeat(text.chars().count());
    }
    if field.flags.comb {
        if let Some(max_len) = field.max_len {
            draw_comb(cb, ctx, r, &text, max_len);
            return;
        }
    }
    if field.flags.multiline {
        draw_multiline(cb, ctx, r, &text);
    } else {
        draw_text_line(cb, ctx, r, &text);
    }
}

/// Multiline text: wrap at `inner_w`, first baseline `top − pad − 0.75·size`,
/// line height `1.2·size`.
fn draw_multiline(cb: &mut ContentBuilder, ctx: &mut DrawCtx<'_>, r: Rect, text: &str) {
    let field = ctx.field;
    let font = ctx.font;
    let inner_w = (r.width() - 2.0 * PAD).max(1.0);
    let size = da_size(field).unwrap_or(12.0);
    let lh = 1.2 * size;
    let lines = super::freetext::wrap_lines(font, text, size, inner_w);
    let mut bad = 0usize;
    for c in text.chars() {
        let mut buf = [0u8; 4];
        let (_, b) = to_winansi(c.encode_utf8(&mut buf));
        bad += b as usize;
    }
    if bad > 0 {
        ctx.warnings.push(format!(
            "field {}: {bad} characters not encodable in WinAnsi, replaced by '?'",
            field.name
        ));
    }
    cb.rect(r.x0, r.y0, r.width(), r.height());
    cb.clip();
    cb.fill_rgb(&da_color(field));
    cb.begin_text();
    cb.font(font.resource_name(), size);
    let mut baseline = r.y1 - PAD - 0.75 * size;
    let mut prev_x: Option<f64> = None;
    for line in &lines {
        let lw = font.text_width(line, size);
        let x = text_x(field, r, lw);
        match prev_x {
            None => cb.text_pos(x, baseline),
            Some(px) => cb.text_pos(x - px, -lh),
        }
        cb.show_text(line);
        prev_x = Some(x);
        baseline -= lh;
    }
    cb.end_text();
    ctx.font_res
        .push((font.resource_name().to_string(), font.font_dict()));
}

/// Comb text: `inner_w / max_len` cells, each glyph centred in its cell.
fn draw_comb(cb: &mut ContentBuilder, ctx: &mut DrawCtx<'_>, r: Rect, text: &str, max_len: u32) {
    let field = ctx.field;
    let font = ctx.font;
    let (enc, bad) = to_winansi(text);
    ctx.warn_unencodable(bad);
    let inner_w = (r.width() - 2.0 * PAD).max(1.0);
    let inner_h = (r.height() - 2.0 * PAD).max(1.0);
    let cell_w = inner_w / max_len.max(1) as f64;
    let size = da_size(field).unwrap_or_else(|| autosize(font, &enc, cell_w, inner_h));
    cb.rect(r.x0, r.y0, r.width(), r.height());
    cb.clip();
    cb.fill_rgb(&da_color(field));
    cb.begin_text();
    cb.font(font.resource_name(), size);
    let baseline = (r.y0 + r.y1) / 2.0 - 0.35 * size;
    let mut prev_x: Option<f64> = None;
    for (i, &byte) in enc.iter().enumerate() {
        if i as u32 >= max_len {
            break;
        }
        let gw = font.text_width(&[byte], size);
        let x = r.x0 + PAD + (i as f64 + 0.5) * cell_w - gw / 2.0;
        match prev_x {
            None => cb.text_pos(x, baseline),
            Some(px) => cb.text_pos(x - px, 0.0),
        }
        cb.show_text(&[byte]);
        prev_x = Some(x);
    }
    cb.end_text();
    ctx.font_res
        .push((font.resource_name().to_string(), font.font_dict()));
}

/// Selected combo option's label (fallback: export value / free text).
fn combo_label(field: &FormField) -> String {
    let v = value_string(field);
    if let Some(opts) = &field.options {
        if let Some(o) = opts.iter().find(|o| o.export == v) {
            return o.label.clone().unwrap_or_else(|| o.export.clone());
        }
    }
    v
}

/// List box: rows from `top_index`, `1.2·size` row height (size 0 → 12),
/// selected rows filled `#99C1DA` before the text, all clipped.
fn draw_list(cb: &mut ContentBuilder, ctx: &mut DrawCtx<'_>, r: Rect) {
    let field = ctx.field;
    let font = ctx.font;
    let size = da_size(field).unwrap_or(12.0);
    let row_h = 1.2 * size;
    let inner_w = (r.width() - 2.0 * PAD).max(1.0);
    let selected: Vec<String> = match &field.value {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    };
    let options: Vec<String> = field
        .options
        .as_ref()
        .map(|o| {
            o.iter()
                .map(|o| o.label.clone().unwrap_or_else(|| o.export.clone()))
                .collect()
        })
        .unwrap_or_default();
    let exports: Vec<&str> = field
        .options
        .as_ref()
        .map(|o| o.iter().map(|o| o.export.as_str()).collect())
        .unwrap_or_default();
    let top = field.top_index.unwrap_or(0) as usize;
    let hi = crate::model::Color("#99C1DA".into());

    cb.rect(r.x0, r.y0, r.width(), r.height());
    cb.clip();
    let mut any_font = false;
    let mut row_top = r.y1 - PAD;
    let mut baseline = r.y1 - PAD - 0.75 * size;
    for (i, option) in options.iter().enumerate().skip(top) {
        if row_top - row_h < r.y0 + PAD - 0.001 {
            break;
        }
        if exports
            .get(i)
            .is_some_and(|e| selected.iter().any(|s| s == e))
        {
            cb.fill_rgb(&hi);
            cb.rect(r.x0 + 1.0, row_top - row_h, r.width() - 2.0, row_h);
            cb.fill();
        }
        let (enc, bad) = to_winansi(option);
        ctx.warn_unencodable(bad);
        let _ = inner_w; // overflow is handled by the clip
        cb.fill_rgb(&da_color(field));
        cb.begin_text();
        cb.font(font.resource_name(), size);
        cb.text_pos(r.x0 + PAD, baseline);
        cb.show_text(&enc);
        cb.end_text();
        any_font = true;
        row_top -= row_h;
        baseline -= row_h;
    }
    if any_font {
        ctx.font_res
            .push((font.resource_name().to_string(), font.font_dict()));
    }
}
