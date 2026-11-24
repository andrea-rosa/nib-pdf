//! Writer tests: CRUD round-trips, foreign /AP preservation, popup
//! handling, full-rewrite behavior, atomic save, and the incremental
//! matrix across the committed fixture set.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use chrono::{TimeZone, Utc};
use lopdf::{Document, Object, ObjectId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{save_atomic, FlushOptions, Writer, WriterError};
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
            bind_to_widget: None,
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
            bind_to_widget: None,
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
        verified_signature_present: false,
        doc_mdp_permission: None,
        signature_locked_fields: HashSet::new(),
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
    // NIB_KEEP_PDFS=<dir> keeps every checked file for manual reader checks.
    if let Ok(keep) = std::env::var("NIB_KEEP_PDFS") {
        let _ = std::fs::create_dir_all(&keep);
        let _ = std::fs::write(Path::new(&keep).join(format!("{tag}.pdf")), bytes);
    }
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
    let (mut w, existing, _) = Writer::open(bytes.clone()).unwrap();
    assert!(existing.is_empty());

    let annots = annots_11();
    let ops: Vec<Op> = annots.iter().map(create_op).collect();
    let m = model(annots.clone(), w.page_count());
    let out = w
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .unwrap();
    assert_eq!(out.report.revision, 1);
    assert_eq!(out.report.annots_written, 11);
    qpdf_check(&out.bytes, "annots_crud_11_subtypes");

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
        .flush(&ops, &m2, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
        .flush(&ops, &m3, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
    let (_, first, _) = Writer::open(bytes.clone()).unwrap();
    let (_, second, _) = Writer::open(bytes).unwrap();
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
    let (_, annots, _) = Writer::open(bytes).unwrap();
    let e = Engine::with_base("doc".into(), 1, annots, vec![]);
    assert!(e.flush_ops().is_empty());
    assert!(e.uncommitted_ids().is_empty());
}

#[test]
fn foreign_ap_preserved() {
    let bytes = fixture("annots-existing.pdf");
    let (mut w, annots, _) = Writer::open(bytes).unwrap();
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
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
    let (mut w, annots, _) = Writer::open(bytes).unwrap();
    let text = annots
        .iter()
        .find(|a| matches!(a, Annotation::Text(_)))
        .unwrap()
        .clone();
    assert!(text.base().popup.is_some());

    let m = model(annots.clone(), 1);
    let ops = vec![op(OpKind::Delete, text.base().id, json!({}))];
    let out = w
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
    let (mut w, _, _) = Writer::open(bytes).unwrap();
    assert!(w.has_signatures());
    let m = model(vec![], 2);
    let err = w
        .flush(&[], &m, FlushOptions::full_rewrite(), &NoAssets, &NoFonts)
        .unwrap_err();
    assert!(matches!(err, WriterError::SignedDocument));
}

#[test]
fn full_rewrite_compacts() {
    let bytes = fixture("incr-classic.pdf");
    let (mut w, _, _) = Writer::open(bytes).unwrap();
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
            .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
            .unwrap();
        last = out.bytes.to_vec();
    }
    let incr_len = last.len();
    let m = model(vec![a.clone()], 2);
    let out = w
        .flush(&[], &m, FlushOptions::full_rewrite(), &NoAssets, &NoFonts)
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
        let (mut w, _, _) = Writer::open(bytes.clone()).unwrap();
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
            .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
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
            .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
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

#[test]
fn flush_error_leaves_writer_usable() {
    let bytes = fixture("text.pdf");
    let (mut w, _, _) = Writer::open(bytes).unwrap();
    let a = Annotation::Square(ShapeAnnot {
        base: base(Some(Rect::new(10.0, 10.0, 50.0, 50.0)), 42),
        fill_color: None,
        stroke_width: None,
        border_style: None,
        cloudy: None,
    });
    // Create op whose payload places the annot on a nonexistent page.
    let mut bad = a.clone();
    bad.base_mut().page = 99;
    let m = model(vec![bad.clone()], w.page_count());
    let ops = vec![create_op(&bad)];
    let err = w
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .unwrap_err();
    assert!(matches!(err, WriterError::PageOutOfRange(99)));

    // Writer is still usable: a valid flush succeeds afterwards.
    let m = model(vec![a.clone()], w.page_count());
    let ops = vec![create_op(&a)];
    let out = w
        .flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .expect("flush after error must succeed");
    assert_eq!(out.report.revision, 1);
    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_id = doc.get_pages()[&1];
    assert_eq!(annots_of(&doc, page_id).len(), 1);
}

#[test]
fn incremental_after_full_rewrite_writes_correct_page() {
    let bytes = fixture("incr-classic.pdf");
    let (mut w, _, _) = Writer::open(bytes).unwrap();
    let a = Annotation::Square(ShapeAnnot {
        base: base(Some(Rect::new(10.0, 10.0, 50.0, 50.0)), 7),
        fill_color: None,
        stroke_width: None,
        border_style: None,
        cloudy: None,
    });
    let m = model(vec![a.clone()], w.page_count());
    let ops = vec![create_op(&a)];
    w.flush(&ops, &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .unwrap();
    let out = w
        .flush(&[], &m, FlushOptions::full_rewrite(), &NoAssets, &NoFonts)
        .unwrap();

    // After renumbering, page object ids moved — the next incremental flush
    // must still target the right page.
    let b = Annotation::Circle(ShapeAnnot {
        base: {
            let mut bb = base(Some(Rect::new(60.0, 60.0, 90.0, 90.0)), 8);
            bb.page = 1;
            bb
        },
        fill_color: None,
        stroke_width: None,
        border_style: None,
        cloudy: None,
    });
    let m2 = model(vec![a.clone(), b.clone()], w.page_count());
    let ops = vec![create_op(&b)];
    let out2 = w
        .flush(&ops, &m2, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .unwrap();
    let doc = Document::load_mem(&out2.bytes).unwrap();
    let pages = doc.get_pages();
    assert_eq!(annots_of(&doc, pages[&1]).len(), 1);
    assert_eq!(annots_of(&doc, pages[&2]).len(), 1);
    let _ = out;
}

// ---- form field updates + flatten (M4) ----

fn field_op(name: &str, value: Value) -> Op {
    Op {
        op_id: Uuid::now_v7(),
        ts: ts(),
        author: None,
        group_id: None,
        kind: OpKind::Update,
        target: Target::FormField,
        id: name.to_string(),
        page: Some(0),
        payload: json!({ "value": value }),
    }
}

/// All writable fixture fields and their new values.
fn form_updates() -> Vec<(&'static str, Value)> {
    vec![
        ("name", json!("Andrea Rosa")),
        ("notes", json!("line one\nline two")),
        ("code", json!("123456")),
        ("secret", json!("hunter2")),
        ("agree", json!("Yes")),
        ("newsletter", json!("On")),
        ("color", json!("Green")),
        ("country", json!("IT")),
        ("city", json!("Turin")),
        ("plan", json!(["Pro", "Enterprise"])),
        ("bottom.note", json!("bottom text")),
    ]
}

fn open_form() -> (Writer, Vec<crate::model::FormField>) {
    let bytes = fixture("form.pdf");
    let (w, _annots, fields) = Writer::open(bytes).unwrap();
    (w, fields)
}

fn apply_form_updates(w: &Writer, fields: Vec<crate::model::FormField>) -> (Engine, Vec<Op>) {
    let mut e = Engine::with_base("doc".into(), w.page_count(), vec![], fields);
    let ops: Vec<Op> = form_updates()
        .into_iter()
        .map(|(n, v)| field_op(n, v))
        .collect();
    e.apply(ops).unwrap();
    let ops = e.flush_ops();
    (e, ops)
}

/// Field dict's `/V` (resolved).
fn field_v(doc: &Document, oid: ObjectId) -> Option<Object> {
    doc.get_object(oid)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"V").ok().cloned())
}

fn widget_dict(doc: &Document, oid: ObjectId) -> lopdf::Dictionary {
    doc.get_object(oid).unwrap().as_dict().unwrap().clone()
}

fn name_of(o: &Object) -> String {
    String::from_utf8_lossy(o.as_name().unwrap()).to_string()
}

fn str_of(o: &Object) -> String {
    String::from_utf8_lossy(o.as_str().unwrap()).to_string()
}

#[test]
fn form_set_values_roundtrip() {
    let orig = fixture("form.pdf");
    let (mut w, fields) = open_form();
    let (e, ops) = apply_form_updates(&w, fields);
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert_eq!(out.report.fields_written, 11);
    // Incremental save: the original bytes are a byte-identical prefix.
    assert!(out.bytes.starts_with(&orig));
    qpdf_check(&out.bytes, "form_set_values_roundtrip");

    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let imp = crate::forms::parse(&doc, &page_ids);

    let v = |name: &str| field_v(&doc, imp.refs[name].field).expect(name);
    assert_eq!(str_of(&v("name")), "Andrea Rosa");
    assert_eq!(str_of(&v("notes")), "line one\nline two");
    assert_eq!(str_of(&v("code")), "123456");
    assert_eq!(str_of(&v("secret")), "hunter2");
    assert_eq!(name_of(&v("agree")), "Yes");
    assert_eq!(name_of(&v("newsletter")), "On");
    assert_eq!(name_of(&v("color")), "Green");
    assert_eq!(str_of(&v("country")), "IT");
    assert_eq!(str_of(&v("city")), "Turin");
    assert_eq!(str_of(&v("bottom.note")), "bottom text");
    // plan: /V array + /I [1 2]
    match v("plan") {
        Object::Array(a) => {
            assert_eq!(a.len(), 2);
            assert_eq!(str_of(&a[0]), "Pro");
            assert_eq!(str_of(&a[1]), "Enterprise");
        }
        other => panic!("plan /V = {other:?}"),
    }
    let plan_d = widget_dict(&doc, imp.refs["plan"].field);
    let idx: Vec<i64> = plan_d
        .get(b"I")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_i64().unwrap())
        .collect();
    assert_eq!(idx, vec![1, 2]);

    // /AS on all 3 radio kids (Green on) and both checkboxes.
    for (i, exp) in ["Off", "Green", "Off"].iter().enumerate() {
        let (wid, _) = imp.refs["color"].widgets[i];
        assert_eq!(
            name_of(widget_dict(&doc, wid).get(b"AS").unwrap()),
            *exp,
            "color kid {i}"
        );
    }
    for (f, exp) in [("agree", "Yes"), ("newsletter", "On")] {
        let (wid, _) = imp.refs[f].widgets[0];
        assert_eq!(name_of(widget_dict(&doc, wid).get(b"AS").unwrap()), exp);
    }

    // Every written widget has /AP /N; Btn widgets have /N dicts with /Off.
    for f in imp.fields.iter().filter(|f| f.name != "readonly") {
        for (wid, _) in &imp.refs[&f.name].widgets {
            let wd = widget_dict(&doc, *wid);
            let n = wd
                .get(b"AP")
                .and_then(|o| o.as_dict())
                .and_then(|d| d.get(b"N"))
                .unwrap_or_else(|_| panic!("widget of {} has no /AP /N", f.name));
            match n {
                Object::Reference(_) => {}
                Object::Dictionary(nd) => {
                    assert!(nd.has(b"Off"), "{}: /AP /N missing /Off", f.name)
                }
                other => panic!("{}: unexpected /N {other:?}", f.name),
            }
        }
    }

    // /NeedAppearances false.
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let cat = doc.get_object(root).unwrap().as_dict().unwrap();
    let af = cat
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .map(|r| doc.get_object(r).unwrap().as_dict().unwrap().clone())
        .unwrap_or_else(|_| cat.get(b"AcroForm").unwrap().as_dict().unwrap().clone());
    assert!(!af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    // /DR /Font has Helv + ZaDb.
    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Helv") && fonts.has(b"ZaDb"));
}

#[test]
fn form_update_full_rewrite() {
    let (mut w, fields) = open_form();
    let (e, ops) = apply_form_updates(&w, fields);
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::full_rewrite(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert_eq!(out.report.fields_written, 11);
    qpdf_check(&out.bytes, "form_update_full_rewrite");
    let doc = Document::load_mem(&out.bytes).unwrap();
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let imp = crate::forms::parse(&doc, &page_ids);
    let v = |name: &str| field_v(&doc, imp.refs[name].field).expect(name);
    assert_eq!(str_of(&v("name")), "Andrea Rosa");
    assert_eq!(name_of(&v("color")), "Green");
    assert_eq!(str_of(&v("bottom.note")), "bottom text");
}

#[test]
fn form_readonly_field_untouched() {
    let orig = fixture("form.pdf");
    let (mut w, fields) = open_form();
    let ro_oid = w.form_refs()["readonly"].field;
    let before = w
        .doc
        .as_ref()
        .and_then(|d| d.get_object(ro_oid).ok())
        .cloned();
    let (e, ops) = apply_form_updates(&w, fields);
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    let doc = Document::load_mem(&out.bytes).unwrap();
    let after = doc.get_object(ro_oid).unwrap().clone();
    assert_eq!(before.unwrap(), after);
    let _ = orig;
}

#[test]
fn form_create_delete_warns() {
    let (mut w, fields) = open_form();
    let e = Engine::with_base("doc".into(), w.page_count(), vec![], fields);
    let mk = |kind: OpKind| Op {
        op_id: Uuid::now_v7(),
        ts: ts(),
        author: None,
        group_id: None,
        kind,
        target: Target::FormField,
        id: "whatever".into(),
        page: Some(0),
        payload: json!({}),
    };
    let ops = vec![mk(OpKind::Create), mk(OpKind::Delete)];
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert_eq!(out.report.fields_written, 0);
    assert!(out
        .report
        .warnings
        .iter()
        .any(|s| s.contains("authoring unsupported")));
}

#[test]
fn form_fields_imported_zero_ops() {
    let (_w, fields) = open_form();
    let e = Engine::with_base("doc".into(), 1, vec![], fields);
    assert!(e.flush_ops().is_empty());
}

#[test]
fn visual_signature_metadata_and_empty_field_roundtrip() {
    let (mut writer, annots, fields) = Writer::open(fixture("sig-field.pdf")).unwrap();
    let widget_id = fields[0].widgets[0].id.clone().unwrap();
    let mut engine = Engine::with_base("doc".into(), 1, annots, fields);
    let id = Uuid::from_u128(42);
    engine
        .apply(vec![op(
            OpKind::Create,
            id,
            json!({
                "subtype": "Ink",
                "page": 0,
                "intent": "signature",
                "signer": {"name": "Signer", "signedAt": "2026-11-19T12:00:00Z"},
                "bindToField": "Signature1",
                "bindToWidget": widget_id,
                "fitMode": "contain",
                "strokeWidth": 2.0,
                "strokes": [{"points": [[0.0, 0.0], [100.0, 20.0]]}]
            }),
        )])
        .unwrap();
    let out = writer
        .flush(
            &engine.flush_ops(),
            engine.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    let pdf = Document::load_mem(&out.bytes).unwrap();
    let refs = writer.form_refs().get("Signature1").unwrap();
    assert!(field_v(&pdf, refs.field).is_none());
    let (_reopened, annots, fields) = Writer::open(out.bytes.to_vec()).unwrap();
    assert_eq!(
        fields[0].signature_state,
        Some(crate::model::SignatureFieldState::Empty)
    );
    let Annotation::Ink(ink) = annots.iter().find(|a| a.base().id == id).unwrap() else {
        panic!("expected ink");
    };
    assert_eq!(
        ink.base.rect,
        Some(crate::geom::Rect::new(40.0, 70.0, 260.0, 150.0))
    );
    assert_eq!(ink.bind_to_field.as_deref(), Some("Signature1"));
    assert_eq!(ink.bind_to_widget.as_deref(), Some(widget_id.as_str()));
    assert_eq!(ink.fit_mode, Some(FitMode::Contain));
    assert_eq!(
        ink.signer.as_ref().and_then(|s| s.name.as_deref()),
        Some("Signer")
    );
    assert_eq!(ink.base.author.as_deref(), Some("Signer"));
    assert!(ink.base.flags.locked);
}

#[test]
fn visual_image_signature_roundtrips_and_missing_asset_fails_atomically() {
    use base64::Engine as _;

    let original = fixture("sig-field.pdf");
    let (mut writer, annots, fields) = Writer::open(original.clone()).unwrap();
    let widget = fields[0].widgets[0].id.clone().unwrap();
    let mut engine = Engine::with_base("doc".into(), 1, annots, fields);
    let id = Uuid::from_u128(43);
    engine
        .apply(vec![op(
            OpKind::Create,
            id,
            json!({
                "subtype": "Stamp", "page": 0, "intent": "signature",
                "bindToField": "Signature1", "bindToWidget": widget,
                "fitMode": "contain", "kind": "image",
                "image": {"assetId": "missing"}
            }),
        )])
        .unwrap();
    let result = writer.flush(
        &engine.flush_ops(),
        engine.model(),
        FlushOptions::incremental(),
        &NoAssets,
        &NoFonts,
    );
    assert!(matches!(
        result,
        Err(WriterError::Ap(crate::ap::ApError::Asset(_)))
    ));
    assert_eq!(writer.bytes().as_ref(), original.as_slice());

    let (mut writer, annots, fields) = Writer::open(original.clone()).unwrap();
    let widget = fields[0].widgets[0].id.clone().unwrap();
    let mut engine = Engine::with_base("doc".into(), 1, annots, fields);
    engine
        .apply(vec![op(
            OpKind::Create,
            id,
            json!({
                "subtype": "Stamp", "page": 0, "intent": "signature",
                "bindToField": "Signature1", "bindToWidget": widget,
                "fitMode": "contain", "kind": "image",
                "image": {"dataBase64": "bm90IGFuIGltYWdl"}
            }),
        )])
        .unwrap();
    let result = writer.flush(
        &engine.flush_ops(),
        engine.model(),
        FlushOptions::incremental(),
        &NoAssets,
        &NoFonts,
    );
    assert!(matches!(
        result,
        Err(WriterError::Ap(crate::ap::ApError::Image(_)))
    ));
    assert_eq!(writer.bytes().as_ref(), original.as_slice());

    let (mut writer, annots, fields) = Writer::open(original).unwrap();
    let widget = fields[0].widgets[0].id.clone().unwrap();
    let mut engine = Engine::with_base("doc".into(), 1, annots, fields);
    let data = base64::engine::general_purpose::STANDARD.encode(fixture("stamp-src.png"));
    engine
        .apply(vec![op(
            OpKind::Create,
            id,
            json!({
                "subtype": "Stamp", "page": 0, "intent": "signature",
                "signer": {"name": "Image Signer"},
                "bindToField": "Signature1", "bindToWidget": widget,
                "fitMode": "contain", "kind": "image",
                "image": {"mime": "image/png", "dataBase64": data}
            }),
        )])
        .unwrap();
    let out = writer
        .flush(
            &engine.flush_ops(),
            engine.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert!(out.report.warnings.is_empty());
    let pdf = Document::load_mem(&out.bytes).unwrap();
    let page = pdf.get_pages()[&1];
    let (_, stamp) = annots_of(&pdf, page)
        .into_iter()
        .find(|(_, dict)| {
            dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Stamp")
        })
        .unwrap();
    assert_eq!(
        super::dict::rect_of(stamp.get(b"Rect").ok()),
        Some(crate::geom::Rect::new(40.0, 70.0, 260.0, 150.0))
    );
    assert!(field_v(&pdf, writer.form_refs()["Signature1"].field).is_none());
    let (_writer, annots, fields) = Writer::open(out.bytes.to_vec()).unwrap();
    let Annotation::Stamp(stamp) = annots.iter().find(|a| a.base().id == id).unwrap() else {
        panic!("expected stamp");
    };
    assert_eq!(stamp.bind_to_field.as_deref(), Some("Signature1"));
    assert_eq!(stamp.bind_to_widget.as_deref(), Some(widget.as_str()));
    assert_eq!(stamp.fit_mode, Some(FitMode::Contain));
    assert_eq!(
        stamp.signer.as_ref().and_then(|s| s.name.as_deref()),
        Some("Image Signer")
    );
    assert_eq!(
        fields[0].signature_state,
        Some(crate::model::SignatureFieldState::Empty)
    );
}

#[test]
fn flatten_removes_acroform_and_widgets() {
    let orig = fixture("form.pdf");
    let (mut w, fields) = open_form();
    let (e, ops) = apply_form_updates(&w, fields);
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::incremental().flatten(true),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert!(out.report.flattened);
    assert!(out.bytes.starts_with(&orig));
    qpdf_check(&out.bytes, "flatten_incremental");

    let doc = Document::load_mem(&out.bytes).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let cat = doc.get_object(root).unwrap().as_dict().unwrap();
    assert!(!cat.has(b"AcroForm"));

    let page_id = doc.get_pages()[&1];
    let annots = annots_of(&doc, page_id);
    assert!(annots
        .iter()
        .all(|(_, d)| { d.get(b"Subtype").unwrap().as_name().unwrap() != b"Widget" }));

    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let contents = page.get(b"Contents").unwrap().as_array().unwrap();
    let last = contents.last().unwrap().as_reference().unwrap();
    let tail = doc
        .get_object(last)
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap();
    let tail = String::from_utf8_lossy(&tail);
    assert!(tail.contains("cm"), "tail:\n{tail}");
    assert!(tail.contains("/NibW"), "tail:\n{tail}");
    assert!(tail.contains("Do"), "tail:\n{tail}");
}

#[test]
fn flatten_full_rewrite_reloads() {
    let (mut w, fields) = open_form();
    let (e, ops) = apply_form_updates(&w, fields);
    let out = w
        .flush(
            &ops,
            e.model(),
            FlushOptions::full_rewrite().flatten(true),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert!(out.report.flattened);
    qpdf_check(&out.bytes, "flatten_full_rewrite");
    let doc = Document::load_mem(&out.bytes).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let cat = doc.get_object(root).unwrap().as_dict().unwrap();
    assert!(!cat.has(b"AcroForm"));
}

#[test]
fn flatten_incremental_prefix_identical() {
    let orig = fixture("form.pdf");
    let (mut w, fields) = open_form();
    let e = Engine::with_base("doc".into(), w.page_count(), vec![], fields);
    let out = w
        .flush(
            &[],
            e.model(),
            FlushOptions::incremental().flatten(true),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    assert!(out.report.flattened);
    assert!(out.bytes.starts_with(&orig));
}

// ---------------------------------------------------------------- signing (M5)

fn sig_spec(target: super::sign::SigTarget) -> super::sign::SigDictSpec {
    super::sign::SigDictSpec {
        target,
        sub_filter: crate::model::SubFilter::EtsiCadesDetached,
        contents_hex_len: 64,
        claimed_time: None,
        name: None,
        reason: None,
        location: None,
        contact_info: None,
        doc_mdp: None,
        lock_fields: None,
    }
}

/// Fill the staged placeholders the way `nib-sign` does (without a CMS).
fn fake_sign(staged: &super::sign::StagedSignature) -> Vec<u8> {
    let mut b = staged.bytes().to_vec();
    let tail = &b[staged.base_len()..];
    let find = |n: &[u8]| tail.windows(n.len()).position(|w| w == n).unwrap() + staged.base_len();
    let br_open = find(b"[0 9999999999");
    let c_open = find(b"/Contents<") + b"/Contents".len();
    let c_end = c_open + b[c_open..].iter().position(|&x| x == b'>').unwrap() + 1;
    let text = format!("0 {} {} {}", c_open, c_end, b.len() - c_end);
    let br_close = br_open + b[br_open..].iter().position(|&x| x == b']').unwrap();
    let slot = &mut b[br_open + 1..br_close];
    slot.fill(b' ');
    slot[..text.len()].copy_from_slice(text.as_bytes());
    b[c_open + 1] = b'A';
    b
}

#[test]
fn signature_ops_are_rejected_not_skipped() {
    let bytes = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(bytes.clone()).unwrap();
    let m = model(vec![], w.page_count());
    for kind in [OpKind::Create, OpKind::Update, OpKind::Delete] {
        let mut o = op(kind, Uuid::now_v7(), json!({}));
        o.target = Target::Signature;
        let e = w
            .flush(&[o], &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
            .unwrap_err();
        assert!(matches!(e, WriterError::UnsupportedOp(_)), "{e:?}");
    }
    assert_eq!(&*w.bytes(), &bytes[..]);
    assert_eq!(w.revision(), 0);
}

#[test]
fn flatten_refused_when_signed() {
    let (mut w, _, _) = Writer::open(fixture("incr-presigned.pdf")).unwrap();
    let m = model(vec![], 2);
    let e = w
        .flush(
            &[],
            &m,
            FlushOptions::incremental().flatten(true),
            &NoAssets,
            &NoFonts,
        )
        .unwrap_err();
    assert!(matches!(e, WriterError::SignedDocument), "{e:?}");
}

#[test]
fn staged_signature_is_private_until_adopted() {
    use super::sign::SigTarget;
    let bytes = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(bytes.clone()).unwrap();
    let staged = w
        .stage_signature(&sig_spec(SigTarget::ExistingField("Signature1".into())))
        .unwrap();
    assert!(w.signature_pending());
    assert_eq!(&*w.bytes(), &bytes[..]);
    assert_eq!(w.revision(), 0);
    assert_eq!(staged.base_len(), bytes.len());
    assert_eq!(&staged.bytes()[..bytes.len()], &bytes[..]);
    Document::load_mem(staged.bytes()).unwrap();

    // Unpatched placeholders and edits outside them are refused.
    let e = w
        .adopt_signed(&staged, staged.bytes().to_vec())
        .unwrap_err();
    assert!(matches!(e, WriterError::SignaturePlaceholder(_)), "{e:?}");
    let mut tampered = fake_sign(&staged);
    let n = tampered.len();
    tampered[n - 3] = b' ';
    let e = w.adopt_signed(&staged, tampered).unwrap_err();
    assert!(
        matches!(
            e,
            WriterError::SignaturePlaceholder(_) | WriterError::Parse(_)
        ),
        "{e:?}"
    );
    let mut longer = fake_sign(&staged);
    longer.push(b'\n');
    let e = w.adopt_signed(&staged, longer).unwrap_err();
    assert!(matches!(e, WriterError::StaleSignature(_)), "{e:?}");
    assert!(w.signature_pending());
    assert_eq!(w.revision(), 0);

    w.abort_signature(&staged);
    assert!(!w.signature_pending());
    let e = w.adopt_signed(&staged, fake_sign(&staged)).unwrap_err();
    assert!(matches!(e, WriterError::StaleSignature(_)), "{e:?}");

    let staged = w
        .stage_signature(&sig_spec(SigTarget::ExistingField("Signature1".into())))
        .unwrap();
    let out = w.adopt_signed(&staged, fake_sign(&staged)).unwrap();
    assert_eq!(out.report.revision, 1);
    assert_eq!(&out.bytes[..bytes.len()], &bytes[..]);
    qpdf_check(&out.bytes, "staged_fake_signed");
    let (_, _, fields) = Writer::open(out.bytes.to_vec()).unwrap();
    assert_eq!(
        fields[0].signature_state,
        Some(crate::model::SignatureFieldState::Occupied)
    );
}

#[test]
fn staged_signature_goes_stale_after_intermediate_flush() {
    use super::sign::SigTarget;
    let (mut w, _, _) = Writer::open(fixture("sig-field.pdf")).unwrap();
    let staged = w
        .stage_signature(&sig_spec(SigTarget::NewInvisibleField {
            name: "Late".into(),
            page: 0,
        }))
        .unwrap();
    let a = annots_11().remove(0);
    let m = model(vec![a.clone()], w.page_count());
    w.flush(
        &[create_op(&a)],
        &m,
        FlushOptions::incremental(),
        &NoAssets,
        &NoFonts,
    )
    .unwrap();
    assert_eq!(w.revision(), 1);
    let before = w.bytes();
    let e = w.adopt_signed(&staged, fake_sign(&staged)).unwrap_err();
    assert!(matches!(e, WriterError::StaleSignature(_)), "{e:?}");
    assert_eq!(&*w.bytes(), &before[..]);
    assert_eq!(w.revision(), 1);
}

#[test]
fn stage_signature_rejects_invalid_requests() {
    use super::sign::SigTarget;
    let bytes = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(bytes.clone()).unwrap();
    let mut odd = sig_spec(SigTarget::ExistingField("Signature1".into()));
    odd.contents_hex_len = 63;
    for spec in [
        odd,
        sig_spec(SigTarget::ExistingField("Nope".into())),
        sig_spec(SigTarget::NewInvisibleField {
            name: "Signature1".into(),
            page: 0,
        }),
        sig_spec(SigTarget::NewInvisibleField {
            name: "x".into(),
            page: 1,
        }),
    ] {
        assert!(w.stage_signature(&spec).is_err(), "{spec:?}");
        assert!(!w.signature_pending());
        assert_eq!(&*w.bytes(), &bytes[..]);
    }
}
