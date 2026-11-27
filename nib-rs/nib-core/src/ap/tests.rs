//! Tests for the ap module: builder formatting, generator smoke tests for
//! every subtype, and image embedding.

use super::content::{fmt_num, ContentBuilder};
use super::fonts::{to_winansi, Base14};
use super::image::embed_image;
use std::collections::HashMap;

use super::{build, FontSource, NoAssets, NoFonts};
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

const CJK_FONT: &[u8] = include_bytes!("../../tests/fixtures/fonts/noto-sans-sc-subset.ttf");
const SYMBOLS_FONT: &[u8] =
    include_bytes!("../../tests/fixtures/fonts/noto-sans-symbols2-subset.ttf");

struct FixedFont(Vec<u8>);

impl FontSource for FixedFont {
    fn font(&self, _family: &str, _bold: bool, _italic: bool) -> Option<Vec<u8>> {
        Some(self.0.clone())
    }
}

fn custom_freetext(text: &str, family: &str) -> Annotation {
    let mut b = base(Some(rect()));
    b.contents = Some(text.into());
    Annotation::FreeText(FreeTextAnnot {
        base: b,
        font_family: Some(family.into()),
        font_size: Some(12.0),
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
    })
}

fn cmap_cid(font: &super::embedded_font::EmbeddedFont, ch: char) -> u16 {
    let utf16: String = ch
        .encode_utf16(&mut [0; 2])
        .iter()
        .map(|unit| format!("{unit:04X}"))
        .collect();
    let text = String::from_utf8_lossy(&font.to_unicode);
    let line = text
        .lines()
        .find(|line| line.ends_with(&format!("<{utf16}>")))
        .unwrap_or_else(|| panic!("no /ToUnicode mapping for {ch:?}"));
    u16::from_str_radix(
        line.split_whitespace()
            .next()
            .unwrap()
            .trim_start_matches('<')
            .trim_end_matches('>'),
        16,
    )
    .unwrap()
}

fn cid_gid(font: &super::embedded_font::EmbeddedFont, cid: u16) -> u16 {
    let offset = cid as usize * 2;
    u16::from_be_bytes([font.cid_to_gid[offset], font.cid_to_gid[offset + 1]])
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
fn ink_rejects_points_without_coordinates() {
    for points in [
        serde_json::json!([[]]),
        serde_json::json!([[10]]),
        serde_json::json!([[10, 20], []]),
    ] {
        for (pressure, smoothing) in [(false, "none"), (false, "catmullRom"), (true, "none")] {
            let annotation: Annotation = serde_json::from_value(serde_json::json!({
                "subtype": "Ink", "id": Uuid::nil(), "page": 0,
                "pressureSensitive": pressure,
                "smoothing": {"algorithm": smoothing},
                "strokes": [{"points": points}]
            }))
            .unwrap();
            assert!(matches!(
                build(&annotation, &NoAssets, &NoFonts),
                Err(super::ApError::MissingGeometry)
            ));
        }
    }
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
        bind_to_widget: None,
        fit_mode: None,
    });
    check_ok(&a);
}

#[test]
fn bound_single_point_ink_draws_a_dot_with_widget_bbox() {
    let annotation: Annotation = serde_json::from_value(serde_json::json!({
        "id": Uuid::nil(),
        "subtype": "Ink",
        "page": 0,
        "rect": [10, 20, 110, 60],
        "bindToField": "Signature1",
        "strokes": [{"points": [[50, 40]]}]
    }))
    .unwrap();
    let out = check_ok(&annotation);
    assert_eq!(out.bbox, rect());
    let content = String::from_utf8_lossy(&out.content);
    assert!(content.contains("50 40 m\n50 40 l"), "{content}");
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
        bind_to_widget: None,
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
fn squiggly_with_zero_height_quad_terminates() {
    let a = Annotation::Squiggly(MarkupAnnot {
        base: base(Some(rect())),
        quad_points: vec![QuadPoints::new([
            10.0, 20.0, 100.0, 40.0, 10.0, 20.0, 100.0, 20.0,
        ])],
        blend_mode: None,
        selected_text: None,
        line_width_factor: None,
    });

    let out = check_ok(&a);
    let content = String::from_utf8(out.content).unwrap();
    assert!(content.contains("100 20 l"), "{content}");
    assert!(content.len() < 512, "unexpectedly large AP content");
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
fn freetext_embeds_subset_for_mixed_winansi_cyrillic_and_cjk() {
    use skrifa::prelude::MetadataProvider;

    let text = "Latin café — Привет мир — 中文";
    let annotation = custom_freetext(text, "Noto Sans SC");
    let out = build(&annotation, &NoAssets, &FixedFont(CJK_FONT.to_vec())).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(out.fonts.is_empty());
    assert_eq!(out.embedded_fonts.len(), 1);
    assert!(out.content.windows(5).any(|window| window == b"/NIBF"));

    let font = &out.embedded_fonts[0].1;
    let cmap = String::from_utf8_lossy(&font.to_unicode);
    assert!(cmap.contains("<041F>"), "{cmap}");
    assert!(cmap.contains("<4E2D>"), "{cmap}");
    assert!(font.program.len() < 200_000);
    let subset = skrifa::FontRef::new(&font.program).expect("subset must be valid TrueType");
    assert!(
        subset
            .glyph_metrics(
                skrifa::instance::Size::unscaled(),
                skrifa::instance::LocationRef::default()
            )
            .glyph_count()
            > 0
    );

    let prepared = super::embedded_font::prepare(CJK_FONT, text).unwrap();
    let (_, measured) = prepared.encode("L", 12.0);
    assert_ne!(measured, Base14::Helvetica.text_width(b"L", 12.0));
    let wrapping_font = super::embedded_font::prepare(CJK_FONT, "Noto Sans café").unwrap();
    let lines = wrapping_font.wrap_lines("Noto Sans café", 12.0, 30.0);
    assert!(lines.len() > 1);
    assert!(lines.iter().all(|(_, width)| *width <= 30.001));
}

#[test]
fn embedded_font_keeps_distinct_unicode_codes_for_shared_glyphs() {
    use skrifa::{prelude::MetadataProvider, FontRef};

    let font = FontRef::new(CJK_FONT).unwrap();
    let mut first_by_gid = HashMap::new();
    let duplicate = font.charmap().mappings().find_map(|(codepoint, gid)| {
        let ch = char::from_u32(codepoint)?;
        first_by_gid
            .insert(gid.to_u32(), ch)
            .filter(|first| *first != ch)
            .map(|first| (first, ch))
    });
    let (first, second) = duplicate.expect("fixture should have aliased Unicode mappings");
    let text = format!("{first}{second}");
    let prepared = super::embedded_font::prepare(CJK_FONT, &text).unwrap();
    let first_cid = cmap_cid(&prepared.resource, first);
    let second_cid = cmap_cid(&prepared.resource, second);
    assert_ne!(first_cid, second_cid);
    assert_eq!(
        cid_gid(&prepared.resource, first_cid),
        cid_gid(&prepared.resource, second_cid)
    );

    let repeated = super::embedded_font::prepare(CJK_FONT, "A").unwrap();
    let (codes, _) = repeated.encode("AA", 12.0);
    assert_eq!(&codes[..2], &codes[2..]);
}

#[test]
fn embedded_font_maps_supplementary_plane_unicode() {
    let annotation = custom_freetext("📚", "Noto Sans Symbols 2");
    let out = build(&annotation, &NoAssets, &FixedFont(SYMBOLS_FONT.to_vec())).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let font = &out.embedded_fonts[0].1;
    let cid = cmap_cid(font, '📚');
    assert!(cid_gid(font, cid) > 0);
    assert!(String::from_utf8_lossy(&font.to_unicode).contains("<D83DDCDA>"));
}

#[test]
fn freetext_font_failures_and_missing_glyphs_are_reported() {
    let missing_provider = build(
        &custom_freetext("Привет", "Noto Sans SC"),
        &NoAssets,
        &NoFonts,
    )
    .unwrap();
    assert!(missing_provider
        .warnings
        .iter()
        .any(|warning| warning.contains("FontProvider did not supply")));
    assert!(missing_provider.embedded_fonts.is_empty());
    assert_eq!(missing_provider.fonts[0].0, "Helv");

    let invalid = build(
        &custom_freetext("Привет", "Invalid"),
        &NoAssets,
        &FixedFont(vec![0, 1, 2]),
    )
    .unwrap();
    assert!(invalid
        .warnings
        .iter()
        .any(|warning| warning.contains("invalid TrueType font")));
    assert!(invalid.embedded_fonts.is_empty());

    let missing_glyph = build(
        &custom_freetext("中文\u{10ffff}", "Noto Sans SC"),
        &NoAssets,
        &FixedFont(CJK_FONT.to_vec()),
    )
    .unwrap();
    assert!(missing_glyph
        .warnings
        .iter()
        .any(|warning| warning.contains("missing from font")));
    assert!(
        String::from_utf8_lossy(&missing_glyph.embedded_fonts[0].1.to_unicode)
            .contains("<DBFFDFFF>")
    );
}

#[test]
fn freetext_rejects_unsupported_and_restricted_fonts() {
    let mut collection = CJK_FONT.to_vec();
    collection[..4].copy_from_slice(b"ttcf");
    let collection_result = build(
        &custom_freetext("Привет", "Collection"),
        &NoAssets,
        &FixedFont(collection),
    )
    .unwrap();
    assert!(collection_result
        .warnings
        .iter()
        .any(|warning| warning.contains("TrueType collections are not supported")));

    let mut unsupported = CJK_FONT.to_vec();
    let count = u16::from_be_bytes([unsupported[4], unsupported[5]]) as usize;
    let glyf_tag = b"glyf";
    let mut glyf_offset = None;
    for index in 0..count {
        let record = 12 + index * 16;
        if &unsupported[record..record + 4] == glyf_tag {
            glyf_offset = Some(record);
            break;
        }
    }
    let record = glyf_offset.expect("fixture has a glyf table");
    unsupported[record..record + 4].copy_from_slice(b"CFF ");
    let unsupported_result = build(
        &custom_freetext("Привет", "Unsupported"),
        &NoAssets,
        &FixedFont(unsupported),
    )
    .unwrap();
    assert!(unsupported_result
        .warnings
        .iter()
        .any(|warning| warning.contains("not a supported TrueType outline font")));

    for fs_type in [0x0002u16, 0x0100, 0x0200] {
        let mut restricted = CJK_FONT.to_vec();
        let count = u16::from_be_bytes([restricted[4], restricted[5]]) as usize;
        let record = (0..count)
            .map(|index| 12 + index * 16)
            .find(|record| &restricted[*record..*record + 4] == b"OS/2")
            .expect("fixture has an OS/2 table");
        let offset =
            u32::from_be_bytes(restricted[record + 8..record + 12].try_into().unwrap()) as usize;
        restricted[offset + 8..offset + 10].copy_from_slice(&fs_type.to_be_bytes());
        let result = build(
            &custom_freetext("Привет", "Restricted"),
            &NoAssets,
            &FixedFont(restricted),
        )
        .unwrap();
        assert!(result.embedded_fonts.is_empty());
        let expected = match fs_type {
            0x0002 => "embedding is restricted",
            0x0100 => "prohibits subsetting",
            _ => "bitmap embedding only",
        };
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains(expected)),
            "fsType {fs_type:#06x}: {:?}",
            result.warnings
        );
    }
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
        bind_to_widget: None,
        fit_mode: None,
    });
    let out = check_ok(&a);
    assert!(String::from_utf8_lossy(&out.content).contains("APPROVED"));
}

#[test]
fn standard_stamp_with_huge_rect_uses_bounded_font_size() {
    let a = Annotation::Stamp(StampAnnot {
        base: base(Some(Rect::new(10.0, 20.0, 110.0, 1e100))),
        kind: None,
        image: None,
        vector: None,
        standard_name: None,
        rotation: None,
        preserve_aspect: true,
        intent: None,
        signer: None,
        bind_to_field: None,
        bind_to_widget: None,
        fit_mode: None,
    });

    let out = check_ok(&a);
    let content = String::from_utf8(out.content).unwrap();
    let tokens: Vec<_> = content.split_ascii_whitespace().collect();
    let tf = tokens.iter().position(|token| *token == "Tf").unwrap();
    let size = tokens[tf - 1].parse::<f64>().unwrap();
    assert!(size.is_finite() && (1.0..=30.0).contains(&size));
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
        bind_to_widget: None,
        fit_mode: None,
    });
    check_ok(&a);
}

#[test]
fn bound_stamp_contain_preserves_vector_and_standard_aspect() {
    let mut stamp: StampAnnot = match serde_json::from_value::<Annotation>(serde_json::json!({
        "id": Uuid::nil(),
        "subtype": "Stamp",
        "page": 0,
        "rect": [10, 20, 110, 60],
        "kind": "vector",
        "vector": {
            "viewBox": [0, 0, 10, 10],
            "paths": [{"d": "M 0 0 L 10 10", "stroke": "#000000"}]
        },
        "intent": "signature",
        "bindToField": "Signature1"
    }))
    .unwrap()
    {
        Annotation::Stamp(stamp) => stamp,
        _ => unreachable!(),
    };
    let contained = check_ok(&Annotation::Stamp(stamp.clone()));
    let content = String::from_utf8_lossy(&contained.content);
    assert!(content.contains("40 20 m"), "{content}");
    assert!(content.contains("80 60 l"), "{content}");
    stamp.preserve_aspect = false;
    let stretched = check_ok(&Annotation::Stamp(stamp.clone()));
    let content = String::from_utf8_lossy(&stretched.content);
    assert!(content.contains("10 20 m"), "{content}");
    assert!(content.contains("110 60 l"), "{content}");
    stamp.preserve_aspect = true;
    stamp.kind = Some(StampKind::Standard);
    stamp.standard_name = Some("SIGNED".into());
    let standard = check_ok(&Annotation::Stamp(stamp));
    let content = String::from_utf8_lossy(&standard.content);
    assert!(content.contains("10 23.333 100 33.333 re"), "{content}");
}

#[test]
fn bound_vector_stamp_rejects_malformed_and_empty_paths() {
    for path in ["M 0 0", "L 10 10 M 0 0"] {
        let annotation: Annotation = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(),
            "subtype": "Stamp",
            "page": 0,
            "rect": [10, 20, 110, 60],
            "kind": "vector",
            "vector": {
                "viewBox": [0, 0, 10, 10],
                "paths": [{"d": path, "stroke": "#000000"}]
            },
            "intent": "signature",
            "bindToField": "Signature1"
        }))
        .unwrap();
        assert!(matches!(
            build(&annotation, &NoAssets, &NoFonts),
            Err(super::ApError::InvalidVector(_))
        ));
    }
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
        bind_to_widget: None,
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
        bind_to_widget: None,
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
        bind_to_widget: None,
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

// ---- widget /AP (M4) ----

use super::widget::build_widget;
use crate::model::{
    DefaultAppearance, FieldFlags, FieldOption, FieldType, FormField, TextAlign, Widget,
};

fn wfield(ty: FieldType) -> FormField {
    FormField {
        name: "f".into(),
        field_type: ty,
        signature_state: None,
        value: None,
        default_value: None,
        flags: FieldFlags::default(),
        max_len: None,
        default_appearance: Some(DefaultAppearance {
            font_family: Some("Helvetica".into()),
            font_size: Some(0.0),
            color: Some(Color("#000000".into())),
        }),
        text_align: None,
        tooltip: None,
        export_values: None,
        options: None,
        top_index: None,
        caption: None,
        widgets: Vec::new(),
        extra: Default::default(),
    }
}

fn wwidget(rect: Rect) -> Widget {
    Widget {
        id: Some("w".into()),
        page: 0,
        rect,
        appearance_state: None,
        export_value: None,
        border_color: Some(Color("#000000".into())),
        background_color: Some(Color("#FFFFFF".into())),
        border_width: Some(1.0),
        border_style: None,
        rotation: None,
    }
}

/// Stable dump of every state of a widget /AP: sorted fonts + content.
fn widget_dump(field: &FormField, widget: &Widget) -> String {
    let ap = build_widget(field, widget, &NoFonts).unwrap();
    let mut s = String::new();
    for (state, out) in &ap.states {
        s.push_str(&format!("== state {state:?} /BBox {:?}\n", out.bbox));
        let mut fonts: Vec<_> = out.fonts.iter().collect();
        fonts.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, dict) in fonts {
            let base = dict
                .get(b"BaseFont")
                .map(|o| format!("{o:?}"))
                .unwrap_or_default();
            s.push_str(&format!("font {name} {base}\n"));
        }
        s.push_str(&String::from_utf8_lossy(&out.content));
    }
    s
}

macro_rules! wsnap {
    ($name:ident, $field:expr, $widget:expr) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(widget_dump(&$field, &$widget));
        }
    };
}

wsnap!(
    snapshot_widget_text,
    {
        let mut f = wfield(FieldType::Text);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.value = Some("hello world".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 120.0, 20.0))
);

wsnap!(
    snapshot_widget_text_multiline,
    {
        let mut f = wfield(FieldType::Text);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.flags.multiline = true;
        f.value = Some("line one\nline two is a bit longer and wraps around the box".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 120.0, 40.0))
);

wsnap!(
    snapshot_widget_text_comb,
    {
        let mut f = wfield(FieldType::Text);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.flags.comb = true;
        f.max_len = Some(6);
        f.value = Some("123456".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 60.0, 20.0))
);

wsnap!(
    snapshot_widget_text_password,
    {
        let mut f = wfield(FieldType::Text);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.flags.password = true;
        f.value = Some("hunter2".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 120.0, 20.0))
);

wsnap!(
    snapshot_widget_text_autosize,
    {
        let mut f = wfield(FieldType::Text);
        f.value = Some("quite a long value that must shrink to fit".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 100.0, 20.0))
);

wsnap!(
    snapshot_widget_checkbox_on,
    {
        let mut f = wfield(FieldType::Checkbox);
        f.export_values = Some(vec!["Yes".into()]);
        f.value = Some("Yes".into());
        f
    },
    {
        let mut w = wwidget(Rect::new(0.0, 0.0, 16.0, 16.0));
        w.export_value = Some("Yes".into());
        w
    }
);

wsnap!(
    snapshot_widget_checkbox_off,
    {
        let mut f = wfield(FieldType::Checkbox);
        f.export_values = Some(vec!["Yes".into()]);
        f.value = Some("Off".into());
        f
    },
    {
        let mut w = wwidget(Rect::new(0.0, 0.0, 16.0, 16.0));
        w.export_value = Some("Yes".into());
        w
    }
);

wsnap!(
    snapshot_widget_radio_on,
    {
        let mut f = wfield(FieldType::Radio);
        f.value = Some("Green".into());
        f
    },
    {
        let mut w = wwidget(Rect::new(0.0, 0.0, 16.0, 16.0));
        w.export_value = Some("Green".into());
        w
    }
);

wsnap!(
    snapshot_widget_radio_off,
    {
        let mut f = wfield(FieldType::Radio);
        f.value = Some("Red".into());
        f
    },
    {
        let mut w = wwidget(Rect::new(0.0, 0.0, 16.0, 16.0));
        w.export_value = Some("Green".into());
        w
    }
);

wsnap!(
    snapshot_widget_combo,
    {
        let mut f = wfield(FieldType::Combo);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.options = Some(vec![
            FieldOption {
                export: "IT".into(),
                label: Some("Italy".into()),
            },
            FieldOption {
                export: "FR".into(),
                label: Some("France".into()),
            },
        ]);
        f.value = Some("IT".into());
        f
    },
    wwidget(Rect::new(0.0, 0.0, 120.0, 20.0))
);

wsnap!(
    snapshot_widget_list_multi,
    {
        let mut f = wfield(FieldType::List);
        f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
        f.flags.multi_select = true;
        f.options = Some(vec![
            FieldOption {
                export: "Basic".into(),
                label: None,
            },
            FieldOption {
                export: "Pro".into(),
                label: None,
            },
            FieldOption {
                export: "Enterprise".into(),
                label: None,
            },
            FieldOption {
                export: "Ultimate".into(),
                label: None,
            },
        ]);
        f.top_index = Some(1);
        f.value = Some(serde_json::json!(["Pro", "Enterprise"]));
        f
    },
    wwidget(Rect::new(0.0, 0.0, 120.0, 60.0))
);

#[test]
fn widget_autosize_shrinks_until_fits() {
    let mut f = wfield(FieldType::Text);
    f.value = Some("quite a long value that must shrink to fit".into());
    let w = wwidget(Rect::new(0.0, 0.0, 100.0, 20.0));
    let ap = build_widget(&f, &w, &NoFonts).unwrap();
    let content = String::from_utf8_lossy(&ap.states[0].1.content);
    // auto-size: font size must be < 12 and the text must fit inside.
    let tf = content
        .split_whitespace()
        .position(|t| t == "Tf")
        .map(|i| {
            content
                .split_whitespace()
                .nth(i - 1)
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
        .unwrap();
    assert!((4.0..12.0).contains(&tf), "size {tf}");
    let (enc, _) = to_winansi("quite a long value that must shrink to fit");
    assert!(Base14::Helvetica.text_width(&enc, tf) <= 96.0 + 0.001);
}

#[test]
fn widget_comb_cells_equal() {
    let mut f = wfield(FieldType::Text);
    f.default_appearance.as_mut().unwrap().font_size = Some(10.0);
    f.flags.comb = true;
    f.max_len = Some(4);
    f.value = Some("ABCD".into());
    let w = wwidget(Rect::new(0.0, 0.0, 80.0, 20.0));
    let ap = build_widget(&f, &w, &NoFonts).unwrap();
    let content = String::from_utf8_lossy(&ap.states[0].1.content);
    // Equal-width cells: successive Td x-offsets differ only by glyph widths,
    // and there are exactly 4 glyphs.
    let glyphs = content.matches("Tj").count();
    assert_eq!(glyphs, 4, "content:\n{content}");
}

#[test]
fn widget_states_q_balanced() {
    let mut f = wfield(FieldType::Checkbox);
    f.export_values = Some(vec!["Yes".into()]);
    f.value = Some("Yes".into());
    let mut w = wwidget(Rect::new(0.0, 0.0, 16.0, 16.0));
    w.export_value = Some("Yes".into());
    let ap = build_widget(&f, &w, &NoFonts).unwrap();
    assert_eq!(ap.states.len(), 2);
    for (_, out) in &ap.states {
        let (q, qq) = q_count(&out.content);
        assert_eq!(q, qq);
        assert!(out.ext_gstate.is_empty());
    }
}
