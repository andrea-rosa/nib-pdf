//! Tests for the ap module: builder formatting, generator smoke tests for
//! every subtype, and image embedding.

use super::content::{fmt_num, ContentBuilder};
use super::fonts::{to_winansi, Base14};
use super::image::embed_image;
use super::{build, NoAssets, NoFonts};
use crate::geom::{Point, QuadPoints, Rect};
use crate::model::annotation::*;
use crate::model::{AnnotBase, Annotation, Color};
use image::ImageEncoder;
use uuid::Uuid;

fn base(rect: Option<Rect>) -> AnnotBase {
    AnnotBase {
        id: Uuid::nil(),
        page: 0,
        rect,
        color: None,
        opacity: 1.0,
        author: None,
        contents: None,
        subject: None,
        created_at: None,
        modified_at: None,
        flags: AnnotFlags::default(),
        in_reply_to: None,
        popup: None,
        appearance: Appearance::default(),
        custom: Default::default(),
        extra: Default::default(),
    }
}

fn rect() -> Rect {
    Rect::new(10.0, 20.0, 110.0, 60.0)
}

fn q_count(content: &[u8]) -> (usize, usize) {
    let s = String::from_utf8_lossy(content);
    let mut q = 0;
    let mut qq = 0;
    for t in s.split_whitespace() {
        if t == "q" {
            q += 1;
        } else if t == "Q" {
            qq += 1;
        }
    }
    (q, qq)
}

fn check_ok(a: &Annotation) -> super::ApOutput {
    let out = build(a, &NoAssets, &NoFonts).expect("build must succeed");
    let (q, qq) = q_count(&out.content);
    assert_eq!(
        q,
        qq,
        "q/Q unbalanced in {}",
        String::from_utf8_lossy(&out.content)
    );
    assert!(
        out.content.starts_with(b"/GS0 gs"),
        "missing /GS0 gs prefix"
    );
    let b = out.bbox.normalize();
    assert!(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite());
    assert!(b.x1 > b.x0 && b.y1 > b.y0, "degenerate bbox {:?}", b);
    out
}

#[test]
fn fmt_num_cases() {
    assert_eq!(fmt_num(0.0), "0");
    assert_eq!(fmt_num(-0.0), "0");
    assert_eq!(fmt_num(1.5), "1.5");
    assert_eq!(fmt_num(2.0), "2");
    assert_eq!(fmt_num(0.1255), "0.126");
    assert_eq!(fmt_num(-3.25), "-3.25");
    assert_eq!(fmt_num(100.0004), "100");
    assert_eq!(fmt_num(0.0004), "0");
}

#[test]
fn literal_escaping() {
    let mut cb = ContentBuilder::new();
    cb.begin_text();
    cb.show_text(b"a(b)\\c\x01\xff\r");
    cb.end_text();
    let bytes = cb.finish();
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(a\\(b\\)\\\\c\\001\\377\\r)"), "{s}");
}

#[test]
fn ink_builds() {
    let a = Annotation::Ink(InkAnnot {
        base: base(None),
        stroke_width: None,
        line_cap: None,
        line_join: None,
        pressure_sensitive: None,
        min_width_factor: None,
        max_width_factor: None,
        smoothing: Some(Smoothing {
            algorithm: SmoothingAlgorithm::CatmullRom,
            tension: None,
            resample_spacing: None,
            simplify_tolerance: None,
            max_error: None,
        }),
        strokes: vec![Stroke {
            points: vec![
                vec![10.0, 10.0],
                vec![20.0, 30.0],
                vec![40.0, 15.0],
                vec![60.0, 40.0],
            ],
            tool: None,
            started_at: None,
        }],
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    });
    check_ok(&a);
}

#[test]
fn ink_pressure_builds() {
    let a = Annotation::Ink(InkAnnot {
        base: base(None),
        stroke_width: Some(3.0),
        line_cap: None,
        line_join: None,
        pressure_sensitive: Some(true),
        min_width_factor: None,
        max_width_factor: None,
        smoothing: None,
        strokes: vec![Stroke {
            points: vec![vec![10.0, 10.0, 0.2], vec![20.0, 30.0, 0.9]],
            tool: None,
            started_at: None,
        }],
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    });
    check_ok(&a);
}

#[test]
fn markup_builds() {
    let quads = vec![QuadPoints::new([
        10.0, 30.0, 60.0, 30.0, 10.0, 20.0, 60.0, 20.0,
    ])];
    for mk in [
        |m| Annotation::Highlight(m),
        |m| Annotation::Underline(m),
        |m| Annotation::StrikeOut(m),
        |m| Annotation::Squiggly(m),
    ] {
        let a = mk(MarkupAnnot {
            base: base(None),
            quad_points: quads.clone(),
            blend_mode: None,
            selected_text: None,
            line_width_factor: None,
        });
        check_ok(&a);
    }
}

#[test]
fn freetext_builds() {
    let mut b = base(Some(rect()));
    b.contents = Some("Hello world, this wraps nicely.".into());
    let a = Annotation::FreeText(FreeTextAnnot {
        base: b,
        font_family: None,
        font_size: None,
        font_weight: None,
        font_style: None,
        text_align: None,
        vertical_align: None,
        line_height: None,
        padding: None,
        fill_color: Some(Color("#FFFFFF".into())),
        border_color: Some(Color("#FF0000".into())),
        border_width: None,
        border_style: None,
        rotation: None,
        callout: None,
    });
    let out = check_ok(&a);
    assert_eq!(out.fonts.len(), 1);
    assert_eq!(out.fonts[0].0, "Helv");
}

#[test]
fn freetext_text_pos_is_absolute() {
    let mut b = base(Some(Rect::new(100.0, 100.0, 300.0, 160.0)));
    b.contents = Some("Hi".into());
    let a = Annotation::FreeText(FreeTextAnnot {
        base: b,
        font_family: None,
        font_size: None,
        font_weight: None,
        font_style: None,
        text_align: None,
        vertical_align: None,
        line_height: None,
        padding: None,
        fill_color: None,
        border_color: None,
        border_width: None,
        border_style: None,
        rotation: None,
        callout: None,
    });
    let out = check_ok(&a);
    let s = String::from_utf8_lossy(&out.content);
    // baseline = y1 - pad - 0.75*12 = 160 - 2 - 9 = 149
    assert!(s.contains("102 149 Td"), "{s}");
}

#[test]
fn note_builds() {
    let a = Annotation::Text(TextAnnot {
        base: base(Some(rect())),
        icon: None,
        open: None,
        state: None,
    });
    let out = check_ok(&a);
    assert_eq!(out.bbox.width(), 20.0);
    assert_eq!(out.bbox.height(), 20.0);
}

#[test]
fn stamp_standard_builds() {
    let a = Annotation::Stamp(StampAnnot {
        base: base(Some(rect())),
        kind: None,
        image: None,
        vector: None,
        standard_name: Some("approved".into()),
        rotation: None,
        preserve_aspect: true,
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    });
    let out = check_ok(&a);
    assert!(String::from_utf8_lossy(&out.content).contains("APPROVED"));
}

#[test]
fn stamp_vector_builds() {
    let a = Annotation::Stamp(StampAnnot {
        base: base(Some(rect())),
        kind: Some(StampKind::Vector),
        image: None,
        vector: Some(VectorStamp {
            view_box: [0.0, 0.0, 10.0, 10.0],
            paths: vec![VectorPath {
                d: "M 0 0 L 10 0 L 10 10 C 8 8 2 8 0 10 Z".into(),
                fill: Some(Color("#00FF00".into())),
                stroke: Some(Color("#000000".into())),
                stroke_width: Some(0.5),
            }],
        }),
        standard_name: None,
        rotation: None,
        preserve_aspect: true,
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    });
    check_ok(&a);
}

#[test]
fn stamp_image_builds() {
    // 4x4 RGB PNG generated in-test.
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([255, 0, 0, 255]),
    ));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_bytes(), 4, 4, image::ExtendedColorType::Rgba8)
        .unwrap();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let a = Annotation::Stamp(StampAnnot {
        base: base(Some(rect())),
        kind: Some(StampKind::Image),
        image: Some(StampImage {
            asset_id: None,
            mime: None,
            sha256: None,
            width_px: None,
            height_px: None,
            data_base64: Some(b64),
        }),
        vector: None,
        standard_name: None,
        rotation: None,
        preserve_aspect: true,
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    });
    let out = check_ok(&a);
    assert_eq!(out.images.len(), 1);
}

#[test]
fn shapes_build() {
    for mk in [|s| Annotation::Square(s), |s| Annotation::Circle(s)] {
        let a = mk(ShapeAnnot {
            base: base(Some(rect())),
            fill_color: None,
            stroke_width: None,
            border_style: Some(BorderStyle {
                style: BorderStyleKind::Dashed,
                dash: None,
            }),
            cloudy: None,
        });
        check_ok(&a);
    }
}

#[test]
fn line_builds() {
    let a = Annotation::Line(LineAnnot {
        base: base(None),
        start: Point::new(10.0, 10.0),
        end: Point::new(100.0, 50.0),
        stroke_width: None,
        line_endings: Some(vec![LineEnding::ClosedArrow, LineEnding::OpenArrow]),
        fill_color: None,
        leader_length: None,
        leader_extension: None,
        caption: None,
    });
    check_ok(&a);
}

#[test]
fn embed_png_with_alpha_has_smask() {
    let mut px = image::RgbaImage::new(4, 4);
    for p in px.pixels_mut() {
        *p = image::Rgba([10, 20, 30, 128]);
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(px.as_raw(), 4, 4, image::ExtendedColorType::Rgba8)
        .unwrap();
    let (xo, w, h) = embed_image(&png).unwrap();
    assert_eq!((w, h), (4, 4));
    assert!(xo.smask.is_some());
    let d = &xo.stream.dict;
    assert!(d.get(b"Filter").is_ok(), "RGB stream should be Flate");
}

#[test]
fn embed_jpeg_passthrough() {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        4,
        4,
        image::Rgb([200, 100, 50]),
    ));
    let jpeg = super::image::encode_jpeg(&img);
    let (xo, w, h) = embed_image(&jpeg).unwrap();
    assert_eq!((w, h), (4, 4));
    assert!(xo.smask.is_none());
    let f = xo.stream.dict.get(b"Filter").unwrap().as_name().unwrap();
    assert_eq!(f, b"DCTDecode");
    assert_eq!(xo.stream.content, jpeg);
}

#[test]
fn to_winansi_maps() {
    let (b, bad) = to_winansi("héllo “€” \u{6F22}");
    assert!(bad);
    assert_eq!(b[0], b'h');
    assert_eq!(b[b.len() - 1], b'?');
    assert!(b.contains(&0x93));
    assert!(b.contains(&0x80));
}

#[test]
fn base14_metrics() {
    assert_eq!(Base14::Courier.text_width(b"mm", 10.0), 12.0);
    let h = Base14::Helvetica;
    assert!((h.text_width(b"Hello", 12.0) - 27.336).abs() < 0.01);
    assert_eq!(
        h.font_dict().get(b"BaseFont").unwrap().as_name().unwrap(),
        b"Helvetica"
    );
}

// ---------------------------------------------------------------- snapshots

fn dump_obj(o: &lopdf::Object, out: &mut String) {
    use std::fmt::Write;
    match o {
        lopdf::Object::Dictionary(d) => {
            out.push_str("<<");
            let mut keys: Vec<_> = d
                .iter()
                .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
                .collect();
            keys.sort();
            for k in keys {
                let _ = write!(out, " /{k} ");
                dump_obj(d.get(k.as_bytes()).unwrap(), out);
            }
            out.push_str(" >>");
        }
        lopdf::Object::Array(a) => {
            out.push('[');
            for it in a {
                out.push(' ');
                dump_obj(it, out);
            }
            out.push_str(" ]");
        }
        lopdf::Object::String(s, _) => {
            let _ = write!(out, "({})", String::from_utf8_lossy(s));
        }
        other => {
            let _ = write!(out, "{other:?}");
        }
    }
}

/// Stable textual dump of the writer-produced annot dict + /AP content.
fn snapshot_dump(a: &Annotation) -> String {
    let out = build(a, &NoAssets, &NoFonts).unwrap();
    let mut d = lopdf::Dictionary::new();
    let ids = Default::default();
    let now = chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    crate::writer::dict::fill_annot_dict(&mut d, a, out.bbox, &ids, now);
    let mut s = String::new();
    dump_obj(&lopdf::Object::Dictionary(d), &mut s);
    s.push_str(&format!("\n/BBox {:?}\n", out.bbox.normalize()));
    s.push_str(&String::from_utf8_lossy(&out.content));
    s
}

fn snap_base() -> AnnotBase {
    let mut b = base(Some(Rect::new(100.0, 100.0, 300.0, 160.0)));
    b.id = Uuid::from_u128(42);
    b.author = Some("t".into());
    b.created_at = Some(
        chrono::DateTime::parse_from_rfc3339("2024-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    b.modified_at = b.created_at;
    b
}

macro_rules! snap {
    ($name:ident, $a:expr) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(snapshot_dump(&$a));
        }
    };
}

snap!(snapshot_ink, {
    Annotation::Ink(InkAnnot {
        base: {
            let mut b = snap_base();
            b.color = Some(Color("#1A237E".into()));
            b
        },
        stroke_width: Some(3.0),
        line_cap: None,
        line_join: None,
        pressure_sensitive: None,
        min_width_factor: None,
        max_width_factor: None,
        smoothing: Some(Smoothing {
            algorithm: SmoothingAlgorithm::CatmullRom,
            tension: None,
            resample_spacing: None,
            simplify_tolerance: None,
            max_error: None,
        }),
        strokes: vec![Stroke {
            points: vec![
                vec![110.0, 110.0],
                vec![150.0, 140.0],
                vec![200.0, 120.0],
                vec![260.0, 145.0],
            ],
            tool: None,
            started_at: None,
        }],
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    })
});

snap!(snapshot_highlight, {
    Annotation::Highlight(MarkupAnnot {
        base: snap_base(),
        quad_points: vec![QuadPoints::new([
            110.0, 140.0, 290.0, 140.0, 110.0, 125.0, 290.0, 125.0,
        ])],
        blend_mode: None,
        selected_text: None,
        line_width_factor: None,
    })
});

snap!(snapshot_underline, {
    Annotation::Underline(MarkupAnnot {
        base: snap_base(),
        quad_points: vec![QuadPoints::new([
            110.0, 140.0, 290.0, 140.0, 110.0, 125.0, 290.0, 125.0,
        ])],
        blend_mode: None,
        selected_text: None,
        line_width_factor: None,
    })
});

snap!(snapshot_strikeout, {
    Annotation::StrikeOut(MarkupAnnot {
        base: snap_base(),
        quad_points: vec![QuadPoints::new([
            110.0, 140.0, 290.0, 140.0, 110.0, 125.0, 290.0, 125.0,
        ])],
        blend_mode: None,
        selected_text: None,
        line_width_factor: None,
    })
});

snap!(snapshot_squiggly, {
    Annotation::Squiggly(MarkupAnnot {
        base: snap_base(),
        quad_points: vec![QuadPoints::new([
            110.0, 140.0, 290.0, 140.0, 110.0, 125.0, 290.0, 125.0,
        ])],
        blend_mode: None,
        selected_text: None,
        line_width_factor: None,
    })
});

snap!(snapshot_freetext, {
    let mut b = snap_base();
    b.contents = Some("Hello wrapped world of text".into());
    Annotation::FreeText(FreeTextAnnot {
        base: b,
        font_family: None,
        font_size: None,
        font_weight: None,
        font_style: None,
        text_align: Some(TextAlign::Center),
        vertical_align: None,
        line_height: None,
        padding: None,
        fill_color: Some(Color("#FFFF00".into())),
        border_color: Some(Color("#FF0000".into())),
        border_width: Some(1.5),
        border_style: Some(BorderStyle {
            style: BorderStyleKind::Dashed,
            dash: None,
        }),
        rotation: None,
        callout: None,
    })
});

snap!(snapshot_text, {
    let mut b = snap_base();
    b.contents = Some("note".into());
    b.popup = Some(Popup {
        open: true,
        rect: None,
    });
    Annotation::Text(TextAnnot {
        base: b,
        icon: Some(TextIcon::Comment),
        open: Some(true),
        state: None,
    })
});

snap!(snapshot_stamp, {
    Annotation::Stamp(StampAnnot {
        base: snap_base(),
        kind: None,
        image: None,
        vector: None,
        standard_name: Some("approved".into()),
        rotation: None,
        preserve_aspect: true,
        intent: None,
        signer: None,
        bind_to_field: None,
        fit_mode: None,
    })
});

snap!(snapshot_square, {
    Annotation::Square(ShapeAnnot {
        base: snap_base(),
        fill_color: Some(Color("#00FF00".into())),
        stroke_width: Some(2.5),
        border_style: Some(BorderStyle {
            style: BorderStyleKind::Dashed,
            dash: None,
        }),
        cloudy: None,
    })
});

snap!(snapshot_circle, {
    Annotation::Circle(ShapeAnnot {
        base: snap_base(),
        fill_color: None,
        stroke_width: None,
        border_style: None,
        cloudy: None,
    })
});

snap!(snapshot_line, {
    Annotation::Line(LineAnnot {
        base: snap_base(),
        start: Point::new(110.0, 110.0),
        end: Point::new(290.0, 150.0),
        stroke_width: Some(2.0),
        line_endings: Some(vec![LineEnding::ClosedArrow, LineEnding::OpenArrow]),
        fill_color: Some(Color("#0000FF".into())),
        leader_length: None,
        leader_extension: None,
        caption: None,
    })
});
