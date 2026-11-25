//! `/AP` for Highlight / Underline / StrikeOut / Squiggly
//! (`AnnotationDrawing.drawMarkup`).

use super::content::ContentBuilder;
use super::{color_or, ApError, ApOutput};
use crate::delta::dirty;
use crate::model::{Annotation, MarkupAnnot};

pub(crate) fn gen(a: &Annotation, m: &MarkupAnnot) -> Result<ApOutput, ApError> {
    if m.quad_points.is_empty() {
        return Err(ApError::MissingGeometry);
    }
    let color = color_or(&m.base.color, "#FFEB3B");
    let f = m.line_width_factor.unwrap_or(0.06);
    let mut cb = ContentBuilder::new();
    let mut bbox: Option<crate::geom::Rect> = None;

    for q in &m.quad_points {
        let b = q.bbox();
        bbox = Some(match bbox {
            None => b,
            Some(acc) => acc.union(&b),
        });
        // Quad order: UL UR LL LR, y-up; quad_h mirrors the Kotlin
        // `ll.y - ul.y` computed in y-down content space.
        let pts = q.points();
        let (ul, ur, ll, lr) = (pts[0], pts[1], pts[2], pts[3]);
        let quad_h = (ul.y - ll.y).abs();
        match a {
            Annotation::Highlight(_) => {
                cb.fill_rgb(&color);
                cb.move_to(ul.x, ul.y);
                cb.line_to(ur.x, ur.y);
                cb.line_to(lr.x, lr.y);
                cb.line_to(ll.x, ll.y);
                cb.close();
                cb.fill();
            }
            Annotation::Underline(_) | Annotation::StrikeOut(_) | Annotation::Squiggly(_) => {
                let t = f * quad_h;
                let y = match a {
                    Annotation::Underline(_) => ll.y + t / 2.0,
                    Annotation::StrikeOut(_) => (ul.y + ll.y) / 2.0,
                    _ => ll.y + t / 2.0,
                };
                cb.stroke_rgb(&color);
                if matches!(a, Annotation::Squiggly(_)) {
                    // Zig-zag: period 2t, amplitude t, stroke t/2.
                    let period = 2.0 * t;
                    cb.line_width(t / 2.0);
                    let mut x = ll.x;
                    let mut up = true;
                    cb.move_to(x, y);
                    while x < lr.x {
                        x = (x + period / 2.0).min(lr.x);
                        cb.line_to(x, if up { y + t } else { y });
                        up = !up;
                    }
                    cb.stroke();
                } else {
                    cb.line_width(t);
                    cb.move_to(ll.x, y);
                    cb.line_to(lr.x, y);
                    cb.stroke();
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(ApOutput {
        bbox: dirty::inflate(&bbox.ok_or(ApError::MissingGeometry)?, 1.0),
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        embedded_fonts: vec![],
        warnings: vec![],
    })
}
