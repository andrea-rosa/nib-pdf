//! Text line snapping: groups PDFium char boxes into line rects (used for
//! text-markup quad generation).

use crate::geom::Rect;

/// One character's bounding box in user space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CharBox {
    pub rect: Rect,
    pub ch: char,
}

/// Snaps the chars whose box centre lies inside `sel` into line rects.
/// Chars are grouped by vertical centre (a char joins the current line when
/// its cy is within half the median char height of the line's running mean
/// cy); each line rect is the union of its in-selection char rects. Lines
/// are sorted top→bottom (descending y in user space).
pub fn snap_lines(chars: &[CharBox], sel: Rect) -> Vec<Rect> {
    let sel = sel.normalize();
    let inside: Vec<CharBox> = chars
        .iter()
        .copied()
        .filter(|c| {
            let r = c.rect.normalize();
            let cx = (r.x0 + r.x1) / 2.0;
            let cy = (r.y0 + r.y1) / 2.0;
            cx >= sel.x0 && cx <= sel.x1 && cy >= sel.y0 && cy <= sel.y1
        })
        .collect();
    if inside.is_empty() {
        return Vec::new();
    }
    // Median char height (whitespace boxes still count — same font metrics).
    let mut heights: Vec<f64> = inside.iter().map(|c| c.rect.normalize().height()).collect();
    heights.sort_by(f64::total_cmp);
    let median = heights[heights.len() / 2];
    let tol = 0.5 * median;

    // Sort by descending cy so lines come out top→bottom.
    let mut sorted = inside.clone();
    sorted.sort_by(|a, b| {
        let ca = (a.rect.y0 + a.rect.y1) / 2.0;
        let cb = (b.rect.y0 + b.rect.y1) / 2.0;
        cb.total_cmp(&ca)
    });

    struct Line {
        cy_sum: f64,
        n: usize,
        /// Extent over non-whitespace chars only; whitespace joins the line
        /// without stretching it.
        rect: Option<Rect>,
    }

    let mut lines: Vec<Line> = Vec::new();
    for c in sorted {
        let r = c.rect.normalize();
        let cy = (r.y0 + r.y1) / 2.0;
        // Join the nearest line within tolerance (usually the last one).
        let join = lines
            .iter_mut()
            .filter(|l| (cy - l.cy_sum / l.n as f64).abs() <= tol)
            .min_by(|a, b| {
                let da = (cy - a.cy_sum / a.n as f64).abs();
                let db = (cy - b.cy_sum / b.n as f64).abs();
                da.total_cmp(&db)
            });
        let extend = |rect: Option<Rect>| -> Option<Rect> {
            if c.ch.is_whitespace() {
                rect
            } else {
                Some(rect.map_or(r, |acc| acc.union(&r)))
            }
        };
        match join {
            Some(l) => {
                l.cy_sum += cy;
                l.n += 1;
                l.rect = extend(l.rect);
            }
            None => lines.push(Line {
                cy_sum: cy,
                n: 1,
                rect: extend(None),
            }),
        }
    }
    lines.sort_by(|a, b| {
        let ca = a.rect.map_or(0.0, |r| (r.y0 + r.y1) / 2.0);
        let cb = b.rect.map_or(0.0, |r| (r.y0 + r.y1) / 2.0);
        cb.total_cmp(&ca)
    });
    lines
        .into_iter()
        .filter_map(|l| l.rect)
        .filter(|r| r.width() > 0.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_chars(y0: f64, y1: f64, xs: &[(f64, f64)]) -> Vec<CharBox> {
        xs.iter()
            .map(|&(x0, x1)| CharBox {
                rect: Rect::new(x0, y0, x1, y1),
                ch: 'x',
            })
            .collect()
    }

    /// Three lines of 10 chars at a 16 pt pitch, 10 pt tall, 10 pt wide.
    fn three_lines() -> Vec<CharBox> {
        let mut v = Vec::new();
        let xs: Vec<(f64, f64)> = (0..10)
            .map(|i| (40.0 + i as f64 * 10.0, 50.0 + i as f64 * 10.0))
            .collect();
        for &(y0, y1) in &[(150.0, 160.0), (134.0, 144.0), (118.0, 128.0)] {
            v.extend(line_chars(y0, y1, &xs));
        }
        v
    }

    #[test]
    fn snap_lines_groups_by_baseline() {
        let chars = three_lines();
        // Selection covering line 1 fully and line 2 partially (x ≤ 100).
        let lines = snap_lines(&chars, Rect::new(30.0, 130.0, 100.0, 165.0));
        assert_eq!(lines.len(), 2);
        // Top line (y 150–160): 6 chars inside (centres ≤ 100 → x0 < 95).
        assert_eq!(lines[0].y0, 150.0);
        assert_eq!(lines[0].y1, 160.0);
        assert_eq!(lines[1].y0, 134.0);
        assert_eq!(lines[1].y1, 144.0);
        assert_eq!(lines[0].x0, 40.0);
        assert!(lines[0].x1 <= 100.0 && lines[0].x1 > 90.0);
        // Pitch preserved between line tops.
        assert!((lines[0].y1 - lines[1].y1 - 16.0).abs() < 0.5);
    }

    #[test]
    fn snap_lines_empty_selection() {
        let chars = three_lines();
        assert!(snap_lines(&chars, Rect::new(0.0, 0.0, 1.0, 1.0)).is_empty());
        assert!(snap_lines(&[], Rect::new(0.0, 0.0, 100.0, 100.0)).is_empty());
    }

    #[test]
    fn snap_lines_ignores_outside_chars() {
        let chars = three_lines();
        // Selection over the middle line only.
        let lines = snap_lines(&chars, Rect::new(30.0, 130.0, 200.0, 148.0));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], Rect::new(40.0, 134.0, 140.0, 144.0));
    }
}
