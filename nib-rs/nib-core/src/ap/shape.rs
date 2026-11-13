//! `/AP` for Square and Circle (`AnnotationDrawing.drawRectShape`).

use super::content::ContentBuilder;
use super::{color_or, ApError, ApOutput};
use crate::geom::Rect;
use crate::model::{Annotation, BorderStyleKind, ShapeAnnot};

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

pub(crate) fn gen(a: &Annotation, s: &ShapeAnnot) -> Result<ApOutput, ApError> {
    let r = s.base.rect.ok_or(ApError::MissingGeometry)?.normalize();
    let oval = matches!(a, Annotation::Circle(_));
    let w = s.stroke_width.unwrap_or(2.0);
    // Inset by w/2 so the stroke stays inside the rect.
    let i = r.inset(w / 2.0).unwrap_or(r);
    let color = color_or(&s.base.color, "#D32F2F");

    let mut cb = ContentBuilder::new();
    if let Some(dashed) = s
        .border_style
        .as_ref()
        .filter(|b| b.style == BorderStyleKind::Dashed)
    {
        let pat = dashed.dash.clone().unwrap_or_else(|| vec![8.0, 6.0]);
        cb.dash(&pat, 0.0);
    }
    cb.line_width(w);

    let paint = |cb: &mut ContentBuilder| {
        if oval {
            ellipse(
                cb,
                (i.x0 + i.x1) / 2.0,
                (i.y0 + i.y1) / 2.0,
                i.width() / 2.0,
                i.height() / 2.0,
            );
        } else {
            cb.rect(i.x0, i.y0, i.width(), i.height());
        }
    };

    match s.fill_color.as_ref() {
        Some(fill) => {
            cb.fill_rgb(fill);
            cb.stroke_rgb(&color);
            paint(&mut cb);
            cb.fill_stroke();
        }
        None => {
            cb.stroke_rgb(&color);
            paint(&mut cb);
            cb.stroke();
        }
    }

    Ok(ApOutput {
        bbox: r,
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        warnings: vec![],
    })
}

trait Inset {
    /// `rect - d` on all sides; `None` when the inset would collapse the rect.
    fn inset(&self, d: f64) -> Option<Rect>;
}

impl Inset for Rect {
    fn inset(&self, d: f64) -> Option<Rect> {
        let out = Rect::new(self.x0 + d, self.y0 + d, self.x1 - d, self.y1 - d);
        (out.x0 < out.x1 && out.y0 < out.y1).then_some(out)
    }
}
