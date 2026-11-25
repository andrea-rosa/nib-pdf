//! `/AP` for Line (`AnnotationDrawing.drawLine` + `endingAt`), plus the
//! shared `ending_ops` used by FreeText callouts.

use std::f64::consts::PI;

use super::content::ContentBuilder;
use super::{color_or, ApError, ApOutput};
use crate::delta::dirty;
use crate::geom::{Point, Rect};
use crate::model::{Color, LineAnnot, LineEnding};

/// headLen = 4·w + 4; open/closed arrows use a 30° half-angle.
pub(crate) fn head_len(stroke_width: f64) -> f64 {
    4.0 * stroke_width + 4.0
}

/// Emit the paint ops for a line ending at `tip`, pointing along `dir`
/// (radians, direction the tip faces — the arrow tip points opposite to the
/// line segment for a normal ending).
#[allow(clippy::too_many_arguments)]
pub(crate) fn ending_ops(
    cb: &mut ContentBuilder,
    tip: Point,
    dir: f64,
    kind: LineEnding,
    hl: f64,
    w: f64,
    color: &Color,
    fill: &Color,
) {
    let reversed = matches!(kind, LineEnding::ROpenArrow | LineEnding::RClosedArrow);
    let d = if reversed { dir + PI } else { dir };
    let half = 30.0 * PI / 180.0;
    match kind {
        LineEnding::OpenArrow | LineEnding::ROpenArrow => {
            let p1 = Point::new(tip.x + hl * (d - half).cos(), tip.y + hl * (d - half).sin());
            let p2 = Point::new(tip.x + hl * (d + half).cos(), tip.y + hl * (d + half).sin());
            cb.stroke_rgb(color);
            cb.line_width(w);
            cb.move_to(tip.x, tip.y);
            cb.line_to(p1.x, p1.y);
            cb.move_to(tip.x, tip.y);
            cb.line_to(p2.x, p2.y);
            cb.stroke();
        }
        LineEnding::ClosedArrow | LineEnding::RClosedArrow => {
            let p1 = Point::new(tip.x + hl * (d - half).cos(), tip.y + hl * (d - half).sin());
            let p2 = Point::new(tip.x + hl * (d + half).cos(), tip.y + hl * (d + half).sin());
            cb.fill_rgb(fill);
            cb.move_to(tip.x, tip.y);
            cb.line_to(p1.x, p1.y);
            cb.line_to(p2.x, p2.y);
            cb.close();
            cb.fill();
        }
        LineEnding::Circle => {
            cb.fill_rgb(color);
            let r = hl / 2.0;
            circle(cb, tip.x, tip.y, r);
            cb.fill();
        }
        LineEnding::Square => {
            cb.fill_rgb(color);
            cb.rect(tip.x - hl / 2.0, tip.y - hl / 2.0, hl, hl);
            cb.fill();
        }
        LineEnding::Diamond => {
            // Rhombus with half-diagonal hl/2 aligned to `dir`.
            let hx = hl / 2.0;
            let px = -d.sin() * hx;
            let py = d.cos() * hx;
            let fx = d.cos() * hx;
            let fy = d.sin() * hx;
            cb.fill_rgb(color);
            cb.move_to(tip.x + fx, tip.y + fy);
            cb.line_to(tip.x + px, tip.y + py);
            cb.line_to(tip.x - fx, tip.y - fy);
            cb.line_to(tip.x - px, tip.y - py);
            cb.close();
            cb.fill();
        }
        LineEnding::Butt => {
            // Perpendicular segment of length hl through the tip.
            cb.stroke_rgb(color);
            cb.line_width(w);
            let px = -d.sin() * hl / 2.0;
            let py = d.cos() * hl / 2.0;
            cb.move_to(tip.x - px, tip.y - py);
            cb.line_to(tip.x + px, tip.y + py);
            cb.stroke();
        }
        LineEnding::Slash => {
            // Segment of length hl tilted 30° off the perpendicular.
            cb.stroke_rgb(color);
            cb.line_width(w);
            let a = d + PI / 2.0 - 30.0 * PI / 180.0;
            let px = a.cos() * hl / 2.0;
            let py = a.sin() * hl / 2.0;
            cb.move_to(tip.x - px, tip.y - py);
            cb.line_to(tip.x + px, tip.y + py);
            cb.stroke();
        }
        LineEnding::None => {}
    }
}

fn circle(cb: &mut ContentBuilder, cx: f64, cy: f64, r: f64) {
    const KAPPA: f64 = 0.5523;
    let k = r * KAPPA;
    cb.move_to(cx + r, cy);
    cb.curve_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    cb.curve_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    cb.curve_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    cb.curve_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    cb.close();
}

pub(crate) fn gen(l: &LineAnnot) -> Result<ApOutput, ApError> {
    let color = color_or(&l.base.color, "#000000");
    let w = l.stroke_width.unwrap_or(1.5);
    let hl = head_len(w);
    let s = l.start;
    let e = l.end;
    let angle = (e.y - s.y).atan2(e.x - s.x);

    let mut cb = ContentBuilder::new();
    cb.stroke_rgb(&color);
    cb.line_width(w);
    cb.line_cap(1);
    cb.move_to(s.x, s.y);
    cb.line_to(e.x, e.y);
    cb.stroke();

    let fill = l.fill_color.as_ref().unwrap_or(&color).clone();
    if let Some(endings) = &l.line_endings {
        if let Some(kind) = endings.first() {
            ending_ops(&mut cb, s, angle + PI, *kind, hl, w, &color, &fill);
        }
        if let Some(kind) = endings.get(1) {
            ending_ops(&mut cb, e, angle, *kind, hl, w, &color, &fill);
        }
    }

    let bbox = dirty::inflate(
        &Rect::new(s.x.min(e.x), s.y.min(e.y), s.x.max(e.x), s.y.max(e.y)),
        hl.max(w / 2.0) + 1.0,
    );
    Ok(ApOutput {
        bbox,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        embedded_fonts: vec![],
        warnings: vec![],
    })
}
