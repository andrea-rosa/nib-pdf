//! Writer tests: CRUD round-trips, foreign /AP preservation, popup
//! handling, full-rewrite behavior, atomic save, and the incremental
//! matrix across the committed fixture set.

use std::collections::{HashMap, HashSet};
use std::process::Command;

use chrono::{TimeZone, Utc};
use lopdf::{Document, Object, ObjectId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{save_atomic, FlushMode, Writer, WriterError};
use crate::ap::{NoAssets, NoFonts};
use crate::delta::{Engine, Model};
use crate::geom::{Point, QuadPoints, Rect};
use crate::model::annotation::*;
use crate::model::{AnnotBase, Annotation, Color, Op, OpKind, Target};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/").to_string() + name)
        .unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap()
}

fn base(rect: Option<Rect>, id: u128) -> AnnotBase {
    AnnotBase {
        id: Uuid::from_u128(id),
        page: 0,
        rect,
        color: None,
        opacity: 1.0,
        author: None,
        contents: None,
        subject: None,
        created_at: Some(ts()),
        modified_at: Some(ts()),
        flags: AnnotFlags::default(),
        in_reply_to: None,
        popup: None,
        appearance: Appearance::default(),
        custom: Default::default(),
        extra: Default::default(),
    }
}

fn annots_11() -> Vec<Annotation> {
    let rect = Rect::new(40.0, 40.0, 140.0, 80.0);
    let quads = vec![QuadPoints::new([
        40.0, 50.0, 140.0, 50.0, 40.0, 40.0, 140.0, 40.0,
    ])];
    vec![
        Annotation::Ink(InkAnnot {
            base: base(None, 1),
            stroke_width: None,
            line_cap: None,
            line_join: None,
            pressure_sensitive: None,
            min_width_factor: None,
            max_width_factor: None,
            smoothing: None,
            strokes: vec![Stroke {
                points: vec![vec![40.0, 40.0], vec![60.0, 60.0], vec![90.0, 45.0]],
                tool: None,
                started_at: None,
            }],
            intent: None,
            signer: None,
            bind_to_field: None,
            fit_mode: None,
        }),
        Annotation::Highlight(MarkupAnnot {
            base: base(None, 2),
            quad_points: quads.clone(),
            blend_mode: None,
            selected_text: None,
            line_width_factor: None,
        }),
        Annotation::Underline(MarkupAnnot {
            base: base(None, 3),
            quad_points: quads.clone(),
            blend_mode: None,
            selected_text: None,
            line_width_factor: None,
        }),
        Annotation::StrikeOut(MarkupAnnot {
            base: base(None, 4),
            quad_points: quads.clone(),
            blend_mode: None,
            selected_text: None,
            line_width_factor: None,
        }),
        Annotation::Squiggly(MarkupAnnot {
            base: base(None, 5),
            quad_points: quads.clone(),
            blend_mode: None,
            selected_text: None,
            line_width_factor: None,
        }),
        Annotation::FreeText(FreeTextAnnot {
            base: base(Some(rect), 6),
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
        }),
        Annotation::Text(TextAnnot {
            base: base(Some(rect), 7),
            icon: None,
            open: None,
            state: None,
        }),
        Annotation::Stamp(StampAnnot {
            base: base(Some(rect), 8),
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
        }),
        Annotation::Square(ShapeAnnot {
            base: base(Some(rect), 9),
            fill_color: None,
            stroke_width: None,
            border_style: None,
            cloudy: None,
        }),
        Annotation::Circle(ShapeAnnot {
            base: base(Some(rect), 10),
            fill_color: None,
            stroke_width: None,
            border_style: None,
            cloudy: None,
        }),
        Annotation::Line(LineAnnot {
            base: base(None, 11),
            start: Point::new(40.0, 40.0),
            end: Point::new(140.0, 80.0),
            stroke_width: None,
            line_endings: Some(vec![LineEnding::ClosedArrow]),
            fill_color: None,
            leader_length: None,
            leader_extension: None,
            caption: None,
        }),
    ]
}

fn model(annots: Vec<Annotation>, page_count: u32) -> Model {
    let mut m = Model {
        document_id: "test-doc".into(),
        page_count,
        revision: 0,
        order: Vec::new(),
        annotations: HashMap::new(),
        form_fields: HashMap::new(),
        signatures: HashMap::new(),
        signed_barrier: HashSet::new(),
    };
    for a in annots {
        let id = a.base().id.to_string();
        m.order.push(id.clone());
        m.annotations.insert(id, a);
    }
    m
}

fn op(kind: OpKind, id: Uuid, payload: Value) -> Op {
    Op {
        op_id: Uuid::now_v7(),
        ts: ts(),
        author: None,
        group_id: None,
        kind,
        target: Target::Annotation,
        id: id.to_string(),
        page: Some(0),
        payload,
    }
}

fn create_op(a: &Annotation) -> Op {
    op(
        OpKind::Create,
        a.base().id,
        serde_json::to_value(a).unwrap(),
    )
}

/// (ObjectId, dict) pairs of every annot on page 0 of `doc`.
fn annots_of(doc: &Document, page_id: ObjectId) -> Vec<(ObjectId, lopdf::Dictionary)> {
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let arr_obj = page.get(b"Annots").unwrap();
    let arr = match arr_obj {
        Object::Array(a) => a.clone(),
        Object::Reference(r) => doc.get_object(*r).unwrap().as_array().unwrap().clone(),
        _ => panic!("unexpected /Annots"),
    };
    arr.iter()
        .map(|o| {
            let r = o.as_reference().unwrap();
            let d = doc.get_object(r).unwrap().as_dict().unwrap().clone();
            (r, d)
        })
        .collect()
}

fn ap_stream_bytes(doc: &Document, dict: &lopdf::Dictionary) -> Vec<u8> {
    let ap = dict.get(b"AP").unwrap().as_dict().unwrap();
    let n = ap.get(b"N").unwrap().as_reference().unwrap();
    doc.get_object(n)
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap()
}

fn qpdf_check(bytes: &[u8], tag: &str) {
    let have_qpdf = Command::new("qpdf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !have_qpdf {
        eprintln!("qpdf not found, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.pdf");
    std::fs::write(&p, bytes).unwrap();
    let out = Command::new("qpdf")
        .arg("--check")
        .arg(&p)
        .output()
        .unwrap();
    let code = out.status.code().unwrap_or(-1);
    // 0 = clean, 3 = warnings; 1/2 = errors.
    assert!(
        code == 0 || code == 3,
        "{tag}: qpdf --check exited {code}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn annots_crud() {
    let bytes = fixture("text.pdf");
    let (mut w, existing) = Writer::open(bytes.clone()).unwrap();
    assert!(existing.is_empty());

    let annots = annots_11();
    let ops: Vec<Op> = annots.iter().map(create_op).collect();
    let m = model(annots.clone(), w.page_count());
    let out = w
        .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();
    assert_eq!(out.report.revision, 1);
    assert_eq!(out.report.annots_written, 11);

    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_id = doc.get_pages()[&1];
    let found = annots_of(&doc, page_id);
    assert_eq!(found.len(), 11);
    let mut seen_nm = HashSet::new();
    for (_, d) in &found {
        let ap = d.get(b"AP").unwrap().as_dict().unwrap();
        let n = ap.get(b"N").unwrap().as_reference().unwrap();
        doc.get_object(n).unwrap().as_stream().unwrap();
        let nm = String::from_utf8_lossy(d.get(b"NM").unwrap().as_str().unwrap()).to_string();
        seen_nm.insert(nm);
    }
    assert_eq!(seen_nm.len(), 11);

    // Update: recolor the Square to green.
    let mut m2 = m.clone();
    let sq_id = annots[8].base().id;
    let mut sq = annots[8].clone();
    sq.base_mut().color = Some(Color("#00FF00".into()));
    sq.base_mut().modified_at = Some(ts());
    m2.annotations.insert(sq_id.to_string(), sq);
    let ops = vec![op(OpKind::Update, sq_id, json!({"color": "#00FF00"}))];
    let out = w
        .flush(&ops, &m2, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();
    assert_eq!(out.report.revision, 2);
    let doc = Document::load_mem(&out.bytes).unwrap();
    let found = annots_of(&doc, page_id);
    assert_eq!(found.len(), 11);
    let (_, d) = found
        .iter()
        .find(|(_, d)| {
            String::from_utf8_lossy(d.get(b"NM").unwrap().as_str().unwrap()) == sq_id.to_string()
        })
        .unwrap();
    let c = d.get(b"C").unwrap().as_array().unwrap();
    assert_eq!(c[1].as_float().unwrap(), 1.0);

    // Delete: remove the Square.
    let m3 = model(
        m2.annotations
            .values()
            .filter(|a| a.base().id != sq_id)
            .cloned()
            .collect(),
        w.page_count(),
    );
    let ops = vec![op(OpKind::Delete, sq_id, json!({}))];
    let out = w
        .flush(&ops, &m3, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();
    let doc = Document::load_mem(&out.bytes).unwrap();
    let found = annots_of(&doc, page_id);
    assert_eq!(found.len(), 10);
    assert!(!found.iter().any(|(_, d)| {
        String::from_utf8_lossy(d.get(b"NM").unwrap().as_str().unwrap()) == sq_id.to_string()
    }));
}

#[test]
fn nm_roundtrip() {
    let bytes = fixture("annots-existing.pdf");
    let (_, first) = Writer::open(bytes.clone()).unwrap();
    let (_, second) = Writer::open(bytes).unwrap();
    let ids1: Vec<Uuid> = first.iter().map(|a| a.base().id).collect();
    let ids2: Vec<Uuid> = second.iter().map(|a| a.base().id).collect();
    assert_eq!(ids1, ids2, "import must be deterministic");
    assert_eq!(
        first[1].base().id,
        Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap()
    );
    assert_eq!(
        first[10].base().id,
        Uuid::parse_str("22222222-3333-4444-8555-666666666666").unwrap()
    );
    assert_eq!(first.len(), 11);
}

#[test]
fn import_existing_zero_ops() {
    let bytes = fixture("annots-existing.pdf");
    let (_, annots) = Writer::open(bytes).unwrap();
    let e = Engine::with_base("doc".into(), 1, annots);
    assert!(e.flush_ops().is_empty());
    assert!(e.uncommitted_ids().is_empty());
}

#[test]
fn foreign_ap_preserved() {
    let bytes = fixture("annots-existing.pdf");
    let (mut w, annots) = Writer::open(bytes).unwrap();
    let target = annots
        .iter()
        .find(|a| matches!(a, Annotation::Highlight(_)))
        .unwrap()
        .clone();
    let ap_hash = target.base().appearance.ap_hash.clone().unwrap();

    let mut m = model(annots, 1);
    let mut updated = target.clone();
    updated.base_mut().contents = Some("edited".into());
    m.annotations.insert(updated.base().id.to_string(), updated);
    let ops = vec![op(
        OpKind::Update,
        target.base().id,
        json!({"contents": "edited"}),
    )];
    let out = w
        .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();

    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_id = doc.get_pages().values().next().copied().unwrap();
    let annots = annots_of(&doc, page_id);
    let (_, d) = annots
        .iter()
        .find(|(_, d)| {
            d.get(b"NM")
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|s| s == target.base().id.to_string().as_bytes())
                .unwrap_or(false)
        })
        .unwrap();
    let raw = ap_stream_bytes(&doc, d);
    let hex = Sha256::digest(&raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(format!("sha256:{hex}"), ap_hash);
}

#[test]
fn delete_removes_popup_and_recreate_restores() {
    let bytes = fixture("annots-existing.pdf");
    let (mut w, annots) = Writer::open(bytes).unwrap();
    let text = annots
        .iter()
        .find(|a| matches!(a, Annotation::Text(_)))
        .unwrap()
        .clone();
    assert!(text.base().popup.is_some());

    let m = model(annots.clone(), 1);
    let ops = vec![op(OpKind::Delete, text.base().id, json!({}))];
    let out = w
        .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();
    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_id = doc.get_pages().values().next().copied().unwrap();
    let annots_now = annots_of(&doc, page_id);
    assert_eq!(annots_now.len(), 10);
    assert!(!annots_now.iter().any(|(_, d)| {
        d.get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| n == b"Popup")
            .unwrap_or(false)
    }));

    // Re-create the same annotation: popup is restored.
    let ops = vec![create_op(&text)];
    let out = w
        .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
        .unwrap();
    let doc = Document::load_mem(&out.bytes).unwrap();
    let annots_now = annots_of(&doc, page_id);
    // 10 remaining + re-created Text + its re-created Popup.
    assert_eq!(annots_now.len(), 12);
    assert!(annots_now.iter().any(|(_, d)| {
        d.get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| n == b"Popup")
            .unwrap_or(false)
    }));
    let (_, parent) = annots_now
        .iter()
        .find(|(_, d)| {
            d.get(b"NM")
                .ok()
                .and_then(|o| o.as_str().ok())
                .map(|s| s == text.base().id.to_string().as_bytes())
                .unwrap_or(false)
        })
        .unwrap();
    parent.get(b"Popup").unwrap().as_reference().unwrap();
}

#[test]
fn full_rewrite_refused_when_signed() {
    let bytes = fixture("incr-presigned.pdf");
    let (mut w, _) = Writer::open(bytes).unwrap();
    assert!(w.has_signatures());
    let m = model(vec![], 2);
    let err = w
        .flush(&[], &m, FlushMode::FullRewrite, &NoAssets, &NoFonts)
        .unwrap_err();
    assert!(matches!(err, WriterError::SignedDocument));
}

#[test]
fn full_rewrite_compacts() {
    let bytes = fixture("incr-classic.pdf");
    let (mut w, _) = Writer::open(bytes).unwrap();
    let a = Annotation::Square(ShapeAnnot {
        base: base(Some(Rect::new(10.0, 10.0, 50.0, 50.0)), 99),
        fill_color: None,
        stroke_width: None,
        border_style: None,
        cloudy: None,
    });
    let mut last = Vec::new();
    for _ in 0..3 {
        let m = model(vec![a.clone()], 2);
        let ops = vec![create_op(&a)];
        let out = w
            .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
            .unwrap();
        last = out.bytes.to_vec();
    }
    let incr_len = last.len();
    let m = model(vec![a.clone()], 2);
    let out = w
        .flush(&[], &m, FlushMode::FullRewrite, &NoAssets, &NoFonts)
        .unwrap();
    assert!(
        out.bytes.len() <= incr_len,
        "{} vs {incr_len}",
        out.bytes.len()
    );
    Document::load_mem(&out.bytes).unwrap();
    qpdf_check(&out.bytes, "full_rewrite_compacts");
}

#[test]
fn atomic_save() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.pdf");
    save_atomic(&p, b"%PDF-1.4 test").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"%PDF-1.4 test");
    assert!(!dir.path().join("out.pdf.tmp").exists());
}

#[test]
fn incremental_matrix() {
    for name in [
        "incr-classic.pdf",
        "incr-xrefstream.pdf",
        "incr-linearized.pdf",
        "incr-presigned.pdf",
        "incr-hybrid.pdf",
    ] {
        let bytes = fixture(name);
        let (mut w, _) = Writer::open(bytes.clone()).unwrap();
        let a = Annotation::Square(ShapeAnnot {
            base: base(Some(Rect::new(10.0, 10.0, 50.0, 50.0)), 7),
            fill_color: None,
            stroke_width: None,
            border_style: None,
            cloudy: None,
        });
        let m = model(vec![a.clone()], w.page_count());
        let ops = vec![create_op(&a)];
        let out = w
            .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
            .unwrap_or_else(|e| panic!("{name}: create flush failed: {e}"));
        assert_eq!(
            &out.bytes[..bytes.len()],
            bytes.as_slice(),
            "{name}: prefix"
        );
        Document::load_mem(&out.bytes).unwrap();
        qpdf_check(&out.bytes, name);

        // Update + delete also preserve the prefix.
        let ops = vec![
            op(OpKind::Update, a.base().id, json!({"opacity": 0.5})),
            op(OpKind::Delete, a.base().id, json!({})),
        ];
        let out2 = w
            .flush(&ops, &m, FlushMode::Incremental, &NoAssets, &NoFonts)
            .unwrap_or_else(|e| panic!("{name}: update/delete flush failed: {e}"));
        assert_eq!(
            &out2.bytes[..out.bytes.len()],
            &out.bytes[..],
            "{name}: prefix2"
        );
        Document::load_mem(&out2.bytes).unwrap();
        qpdf_check(&out2.bytes, name);

        if name == "incr-presigned.pdf" {
            let doc = Document::load_mem(&out.bytes).unwrap();
            let mut found = false;
            for obj in doc.objects.values() {
                if let Ok(d) = obj.as_dict() {
                    if d.get(b"ByteRange").is_ok() {
                        found = true;
                        let br = d.get(b"ByteRange").unwrap().as_array().unwrap();
                        assert_eq!(
                            br.iter()
                                .filter_map(|o| o.as_i64().ok())
                                .collect::<Vec<_>>(),
                            vec![0, 1000, 2000, 3000]
                        );
                        let c = d.get(b"Contents").unwrap().as_str().unwrap();
                        assert_eq!(c, &[0u8; 64][..]);
                    }
                }
            }
            assert!(found, "signature dict lost after incremental save");
        }
    }
}
