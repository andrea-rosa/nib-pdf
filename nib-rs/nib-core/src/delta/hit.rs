//! Hit testing over annotations (reverse z-order).

use super::Engine;
use crate::geom::{Point, Rect};
use crate::model::Annotation;

fn seg_distance(pt: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((pt.x - a.x) * dx + (pt.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((pt.x - a.x - t * dx).powi(2) + (pt.y - a.y - t * dy).powi(2)).sqrt()
}

fn inflated_contains(r: &Rect, pt: Point, tol: f64) -> bool {
    let r = r.normalize();
    pt.x >= r.x0 - tol && pt.x <= r.x1 + tol && pt.y >= r.y0 - tol && pt.y <= r.y1 + tol
}

fn hits(a: &Annotation, pt: Point, tol: f64) -> bool {
    match a {
        Annotation::Ink(ink) => {
            let reach = tol + ink.stroke_width.unwrap_or(1.0) / 2.0;
            ink.strokes.iter().any(|s| {
                let pts: Vec<Point> = s
                    .points
                    .iter()
                    .filter(|p| p.len() >= 2)
                    .map(|p| Point::new(p[0], p[1]))
                    .collect();
                pts.windows(2)
                    .any(|w| seg_distance(pt, w[0], w[1]) <= reach)
                    || (pts.len() == 1 && seg_distance(pt, pts[0], pts[0]) <= reach)
            })
        }
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => m
            .quad_points
            .iter()
            .any(|q| inflated_contains(&q.bbox(), pt, tol)),
        Annotation::Line(l) => {
            seg_distance(pt, l.start, l.end) <= tol + l.stroke_width.unwrap_or(1.0) / 2.0
        }
        _ => a
            .base()
            .rect
            .is_some_and(|r| inflated_contains(&r, pt, tol)),
    }
}

impl Engine {
    /// Topmost annotation hit at `pt` on `page`, scanning reverse z-order.
    pub fn hit_test(&self, page: u32, pt: Point, tolerance: f64) -> Option<String> {
        self.model
            .annotations_on_page(page)
            .into_iter()
            .rev()
            .find(|a| hits(a, pt, tolerance))
            .map(|a| a.base().id.to_string())
    }
}
