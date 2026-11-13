use nib_core::geom::{PageTransform, Point, QuadPoints, Rect, Rotation};
use proptest::prelude::*;

fn rotation_strategy() -> impl Strategy<Value = Rotation> {
    prop_oneof![
        Just(Rotation::R0),
        Just(Rotation::R90),
        Just(Rotation::R180),
        Just(Rotation::R270),
    ]
}

// Crop boxes possibly away from the origin, always with positive extent.
fn crop_box_strategy() -> impl Strategy<Value = Rect> {
    (
        -2000.0f64..2000.0,
        -2000.0..2000.0,
        1.0..2000.0,
        1.0..2000.0,
    )
        .prop_map(|(x0, y0, w, h)| Rect::new(x0, y0, x0 + w, y0 + h))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 10_000, ..ProptestConfig::default() })]

    #[test]
    fn transform_roundtrip(
        crop in crop_box_strategy(),
        rotate in rotation_strategy(),
        scale in 0.1f64..16.0,
        x in -4000.0f64..4000.0,
        y in -4000.0f64..4000.0,
    ) {
        let t = PageTransform::new(crop, rotate, scale);
        let p = Point::new(x, y);
        let back = t.to_user(t.to_device(p));
        prop_assert!((back.x - p.x).abs() < 1e-3, "x: {back:?} vs {p:?}");
        prop_assert!((back.y - p.y).abs() < 1e-3, "y: {back:?} vs {p:?}");

        let d = Point::new(x, y);
        let back2 = t.to_device(t.to_user(d));
        prop_assert!((back2.x - d.x).abs() < 1e-3);
        prop_assert!((back2.y - d.y).abs() < 1e-3);
    }
}

/// Corners of a `w×h` crop box must land where the §5.3 rotation table says.
#[test]
fn rotation_table() {
    let (w, h) = (200.0, 100.0);
    let cases = [
        // (rotation, device size, (0,0) user pt → device, (w,h) user pt → device)
        (Rotation::R0, (w, h), (0.0, h), (w, 0.0)),
        (Rotation::R90, (h, w), (0.0, 0.0), (h, w)),
        (Rotation::R180, (w, h), (w, 0.0), (0.0, h)),
        (Rotation::R270, (h, w), (h, w), (0.0, 0.0)),
    ];
    for (rot, (dw, dh), lo_expect, hi_expect) in cases {
        let t = PageTransform::new(Rect::new(0.0, 0.0, w, h), rot, 1.0);
        assert_eq!(t.device_size(), (dw, dh), "{rot:?} device size");
        let lo = t.to_device(Point::new(0.0, 0.0));
        assert!(
            (lo.x - lo_expect.0).abs() < 1e-9 && (lo.y - lo_expect.1).abs() < 1e-9,
            "{rot:?} lower-left: {lo:?} expected {lo_expect:?}"
        );
        let hi = t.to_device(Point::new(w, h));
        assert!(
            (hi.x - hi_expect.0).abs() < 1e-9 && (hi.y - hi_expect.1).abs() < 1e-9,
            "{rot:?} upper-right: {hi:?} expected {hi_expect:?}"
        );
    }
}

#[test]
fn rect_normalisation() {
    let r = Rect::new(10.0, 20.0, -5.0, 8.0).normalize();
    assert_eq!(r, Rect::new(-5.0, 8.0, 10.0, 20.0));

    // A rect under R90 flips ordering; rect_to_device re-normalizes.
    let t = PageTransform::new(Rect::new(0.0, 0.0, 200.0, 100.0), Rotation::R90, 1.0);
    let d = t.rect_to_device(Rect::new(10.0, 20.0, 30.0, 40.0));
    assert_eq!(d, d.normalize());
    // Under R90: dx = y - y0, dy = x - x0 → rect (10,20)-(30,40) → (20,10)-(40,30).
    assert_eq!(d, Rect::new(20.0, 10.0, 40.0, 30.0));
}

#[test]
fn quad_normalisation() {
    // Shuffled corner order normalizes back to spec order.
    let q = QuadPoints::new([
        30.0, 40.0, // lower-right
        10.0, 40.0, // upper-left
        30.0, 20.0, // upper-right? (x1,y1)=(30,20)... construct carefully
        10.0, 20.0,
    ]);
    let n = q.normalize();
    let b = n.bbox();
    assert_eq!(b, Rect::new(10.0, 20.0, 30.0, 40.0));
    // Spec order: UL, UR, LL, LR.
    assert_eq!(&n.0[..2], &[10.0, 40.0]);
    assert_eq!(&n.0[2..4], &[30.0, 40.0]);
    assert_eq!(&n.0[4..6], &[10.0, 20.0]);
    assert_eq!(&n.0[6..8], &[30.0, 20.0]);
}
