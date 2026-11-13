//! Dirty-region computation (JSON_SCHEMA §9).

use crate::geom::Rect;
use crate::model::Annotation;

/// Bounding box of an annotation in user space: its `rect` when present,
/// otherwise computed from geometry (Ink strokes, markup quads, Line
/// start/end). Returns `None` when neither is available.
pub fn bbox(a: &Annotation) -> Option<Rect> {
    if let Some(r) = a.base().rect {
        return Some(r.normalize());
    }
    match a {
        Annotation::Ink(ink) => {
            let mut r: Option<Rect> = None;
            for stroke in &ink.strokes {
                for p in &stroke.points {
                    if p.len() < 2 {
                        continue;
                    }
                    let pt = Rect::new(p[0], p[1], p[0], p[1]);
                    r = Some(match r {
                        None => pt,
                        Some(acc) => acc.union(&pt),
                    });
                }
            }
            let factor = if ink.pressure_sensitive == Some(true) {
                ink.max_width_factor.unwrap_or(1.6)
            } else {
                1.0
            };
            let grow = ink.stroke_width.unwrap_or(1.0) * factor / 2.0;
            r.map(|r| inflate(&r, grow))
        }
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => {
            let mut r: Option<Rect> = None;
            for q in &m.quad_points {
                r = Some(match r {
                    None => q.bbox(),
                    Some(acc) => acc.union(&q.bbox()),
                });
            }
            r
        }
        Annotation::Line(l) => {
            let r = Rect::new(
                l.start.x.min(l.end.x),
                l.start.y.min(l.end.y),
                l.start.x.max(l.end.x),
                l.start.y.max(l.end.y),
            );
            let grow = l.stroke_width.unwrap_or(1.0) / 2.0 + 5.0;
            Some(inflate(&r, grow))
        }
        _ => None,
    }
}

pub fn inflate(r: &Rect, d: f64) -> Rect {
    let r = r.normalize();
    Rect::new(r.x0 - d, r.y0 - d, r.x1 + d, r.y1 + d)
}

/// Merges same-page overlapping rects into unions.
pub fn merge(dirty: Vec<(u32, Rect)>) -> Vec<(u32, Rect)> {
    let mut out: Vec<(u32, Rect)> = Vec::new();
    for (page, rect) in dirty {
        let rect = rect.normalize();
        let mut merged = false;
        for (p, r) in out.iter_mut() {
            if *p == page && r.intersect(&rect).is_some() {
                *r = r.union(&rect);
                merged = true;
                break;
            }
        }
        if !merged {
            out.push((page, rect));
        }
    }
    out
}
