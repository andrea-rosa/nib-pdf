//! `/AP` for Ink (`AnnotationDrawing.drawInk` + `InkSmoothing.catmullRomPath`).

use super::content::ContentBuilder;
use super::{color_or, ApError, ApOutput};
use crate::delta::dirty;
use crate::geom::Rect;
use crate::model::{InkAnnot, SmoothingAlgorithm};

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Uniform Catmull-Rom → cubic Bézier, endpoints duplicated.
/// `tension ∈ [0,1]` scales the control arm (0.5 default, 0 = polyline).
fn catmull_rom(cb: &mut ContentBuilder, pts: &[(f64, f64)], tension: f64) {
    match pts.len() {
        0 => return,
        1 => {
            // Zero-length stroke: m + l to the same point; round cap dots it.
            cb.move_to(pts[0].0, pts[0].1);
            cb.line_to(pts[0].0, pts[0].1);
            return;
        }
        2 => {
            cb.move_to(pts[0].0, pts[0].1);
            cb.line_to(pts[1].0, pts[1].1);
            return;
        }
        _ => {}
    }
    let t = tension / 3.0;
    cb.move_to(pts[0].0, pts[0].1);
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        let c1x = p1.0 + (p2.0 - p0.0) * t;
        let c1y = p1.1 + (p2.1 - p0.1) * t;
        let c2x = p2.0 - (p3.0 - p1.0) * t;
        let c2y = p2.1 - (p3.1 - p1.1) * t;
        cb.curve_to(c1x, c1y, c2x, c2y, p2.0, p2.1);
    }
}

pub(crate) fn gen(i: &InkAnnot) -> Result<ApOutput, ApError> {
    let color = color_or(&i.base.color, "#1A237E");
    let base_w = i.stroke_width.unwrap_or(2.0);
    let min_f = i.min_width_factor.unwrap_or(0.4);
    let max_f = i.max_width_factor.unwrap_or(1.6);
    let pressure = i.pressure_sensitive.unwrap_or(false);
    let tension = i.smoothing.as_ref().and_then(|s| s.tension).unwrap_or(0.5);
    let catmull = matches!(
        i.smoothing.as_ref().map(|s| s.algorithm),
        Some(SmoothingAlgorithm::CatmullRom)
    );

    let mut cb = ContentBuilder::new();
    cb.stroke_rgb(&color);
    let mut bbox: Option<Rect> = None;
    let mut any = false;

    for stroke in &i.strokes {
        if stroke.points.is_empty() {
            continue;
        }
        any = true;
        let pts: Vec<(f64, f64)> = stroke
            .points
            .iter()
            .filter(|p| p.len() >= 2)
            .map(|p| (p[0], p[1]))
            .collect();
        for p in &pts {
            let r = Rect::new(p.0, p.1, p.0, p.1);
            bbox = Some(match bbox {
                None => r,
                Some(acc) => acc.union(&r),
            });
        }
        if pressure && stroke.points.iter().all(|p| p.len() >= 3) {
            // Per-segment strokes, width from avg endpoint pressure.
            cb.line_cap(1);
            for seg in stroke.points.windows(2) {
                let p0 = &seg[0];
                let p1 = &seg[1];
                let w = base_w * lerp(min_f, max_f, (p0[2] + p1[2]) / 2.0);
                cb.line_width(w);
                cb.move_to(p0[0], p0[1]);
                cb.line_to(p1[0], p1[1]);
                cb.stroke();
            }
        } else {
            cb.line_cap(1);
            cb.line_join(1);
            cb.line_width(base_w);
            if catmull {
                catmull_rom(&mut cb, &pts, tension);
            } else {
                cb.move_to(pts[0].0, pts[0].1);
                for p in &pts[1..] {
                    cb.line_to(p.0, p.1);
                }
            }
            cb.stroke();
        }
    }

    if !any {
        return Err(ApError::MissingGeometry);
    }
    let grow = base_w * i.max_width_factor.unwrap_or(1.0) / 2.0 + 1.0;
    Ok(ApOutput {
        bbox: dirty::inflate(&bbox.ok_or(ApError::MissingGeometry)?, grow),
        content: cb.finish(),
        ext_gstate: vec![],
        fonts: vec![],
        images: vec![],
        warnings: vec![],
    })
}
