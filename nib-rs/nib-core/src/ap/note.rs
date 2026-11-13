//! `/AP` for Text (sticky note, `AnnotationDrawing.drawStickyNote`):
//! a 20×20 pt rounded rect anchored with its top-left at the user point
//! `(x0, y1)`, i.e. occupying `[x0, y1-20, x0+20, y1]`.

use super::content::ContentBuilder;
use super::{color_or, ApError, ApOutput};
use crate::geom::Rect;
use crate::model::{Color, TextAnnot};

const SIZE: f64 = 20.0;
const RADIUS: f64 = 3.0;

fn round_rect(cb: &mut ContentBuilder, r: &Rect, rad: f64) {
    const KAPPA: f64 = 0.5523;
    let k = rad * KAPPA;
    cb.move_to(r.x0 + rad, r.y0);
    cb.line_to(r.x1 - rad, r.y0);
    cb.curve_to(r.x1 - rad + k, r.y0, r.x1, r.y0 + rad - k, r.x1, r.y0 + rad);
    cb.line_to(r.x1, r.y1 - rad);
    cb.curve_to(r.x1, r.y1 - rad + k, r.x1 - rad + k, r.y1, r.x1 - rad, r.y1);
    cb.line_to(r.x0 + rad, r.y1);
    cb.curve_to(r.x0 + rad - k, r.y1, r.x0, r.y1 - rad + k, r.x0, r.y1 - rad);
    cb.line_to(r.x0, r.y0 + rad);
    cb.curve_to(r.x0, r.y0 + rad - k, r.x0 + rad - k, r.y0, r.x0 + rad, r.y0);
    cb.close();
}

pub(crate) fn gen(t: &TextAnnot) -> Result<ApOutput, ApError> {
    let anchor = t.base.rect.ok_or(ApError::MissingGeometry)?.normalize();
    // Icon box: top-left at the annotation's top-left corner.
    let r = Rect::new(anchor.x0, anchor.y1 - SIZE, anchor.x0 + SIZE, anchor.y1);
    let color = color_or(&t.base.color, "#FFC107");

    let mut cb = ContentBuilder::new();
    cb.fill_rgb(&color);
    round_rect(&mut cb, &r, RADIUS);
    cb.fill();
    cb.stroke_rgb(&Color("#FFFFFF".to_string()));
    cb.line_width(1.0);
    round_rect(&mut cb, &r, RADIUS);
    cb.stroke();

    Ok(ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        warnings: vec![],
    })
}
