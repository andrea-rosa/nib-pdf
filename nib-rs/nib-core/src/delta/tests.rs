use super::merge_patch;
use super::*;
use crate::geom::{Point, Rect};
use crate::model::DeltaDocument;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

fn ts(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(1_700_000_000_000 + ms).unwrap()
}

fn uid(n: u128) -> Uuid {
    Uuid::from_u128(0x9b3a6c1e_0000_4000_8000_000000000000 + n)
}

fn sid(n: u128) -> String {
    uid(n).to_string()
}

fn op(kind: OpKind, target: Target, id: &str, page: Option<u32>, payload: Value, ms: i64) -> Op {
    Op {
        op_id: uid(10_000 + ms as u128),
        ts: ts(ms),
        author: None,
        group_id: None,
        kind,
        target,
        id: id.into(),
        page,
        payload,
    }
}

fn square(id: u128, page: u32, ms: i64) -> Op {
    op(
        OpKind::Create,
        Target::Annotation,
        &sid(id),
        Some(page),
        json!({"subtype": "Square", "page": page, "rect": [10.0, 10.0, 50.0, 50.0], "color": "#FF0000"}),
        ms,
    )
}

fn engine() -> Engine {
    Engine::new("sha256:test".into(), 3)
}

fn invalid(res: Result<ApplyReport, DeltaError>) -> Vec<ValidationError> {
    match res {
        Err(DeltaError::Invalid(e)) => e,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn code(res: Result<ApplyReport, DeltaError>) -> ErrorCode {
    let e = invalid(res);
    assert_eq!(e.len(), 1, "{e:?}");
    e[0].code
}

fn visual_signature_engine() -> Engine {
    let field: FormField = serde_json::from_value(json!({
        "name": "Signature1",
        "fieldType": "signature",
        "signatureState": "empty",
        "widgets": [
            {"id": "widget-1", "page": 0, "rect": [40, 70, 260, 150]}
        ]
    }))
    .unwrap();
    Engine::with_base("sha256:test".into(), 3, Vec::new(), vec![field])
}

fn visual_ink(id: u128, fit: &str) -> Op {
    op(
        OpKind::Create,
        Target::Annotation,
        &sid(id),
        Some(0),
        json!({
            "subtype": "Ink",
            "page": 0,
            "intent": "signature",
            "signer": {"name": "Signer", "signedAt": "2026-11-19T12:00:00Z"},
            "bindToField": "Signature1",
            "bindToWidget": "widget-1",
            "fitMode": fit,
            "strokeWidth": 2.0,
            "strokes": [{"points": [[0.0, 0.0], [100.0, 20.0]]}]
        }),
        0,
    )
}

#[test]
fn visual_signature_binding_fits_and_replays() {
    for mode in ["contain", "stretch"] {
        let mut engine = visual_signature_engine();
        engine.apply(vec![visual_ink(1, mode)]).unwrap();
        let annotation = engine.model().annotation(&sid(1)).unwrap();
        let Annotation::Ink(ink) = annotation else {
            panic!("expected ink");
        };
        assert_eq!(ink.base.rect, Some(Rect::new(40.0, 70.0, 260.0, 150.0)));
        assert!(ink.base.flags.locked && ink.base.flags.print);
        assert_eq!(ink.base.author.as_deref(), Some("Signer"));
        assert!(ink.base.contents.as_deref().unwrap().contains("Signer"));
        for point in ink.strokes.iter().flat_map(|s| &s.points) {
            assert!((40.0..=260.0).contains(&point[0]));
            assert!((70.0..=150.0).contains(&point[1]));
        }
        let expected = engine.model().canonical();
        let compacted = engine.export(ExportMode::Compacted, None);
        let mut replay = visual_signature_engine();
        replay.import(compacted, ImportOptions::default()).unwrap();
        assert_eq!(replay.model().canonical(), expected, "{mode}");

        assert!(engine.undo().is_some());
        assert!(engine.model().annotation(&sid(1)).is_none());
        assert!(engine.redo().is_some());
        assert_eq!(engine.model().canonical(), expected, "{mode} redo");
    }
}

#[test]
fn visual_signature_fits_small_art_and_replays_fractional_widget_rect() {
    for fit in ["contain", "stretch"] {
        let mut engine = visual_signature_engine();
        let rect = Rect::new(40.1234, 70.4567, 260.2345, 150.6789);
        engine
            .model
            .form_fields
            .get_mut("Signature1")
            .unwrap()
            .widgets[0]
            .rect = rect;
        engine.base = engine.model.clone();
        let mut create = visual_ink(1, fit);
        create.payload["rect"] = json!([rect.x0, rect.y0, rect.x1, rect.y1]);
        create.payload["strokes"] = json!([{"points": [[50.0, 90.0], [60.0, 95.0]]}]);
        engine.apply(vec![create]).unwrap();
        let Annotation::Ink(ink) = engine.model().annotation(&sid(1)).unwrap() else {
            panic!("expected ink");
        };
        assert!(ink.strokes[0].points[1][0] - ink.strokes[0].points[0][0] > 100.0);
        let expected = ink.strokes[0].points.clone();
        let json = engine.export_json(ExportMode::Compacted, None);
        let delta: DeltaDocument = serde_json::from_str(&json).unwrap();
        let mut replay = visual_signature_engine();
        replay
            .model
            .form_fields
            .get_mut("Signature1")
            .unwrap()
            .widgets[0]
            .rect = rect;
        replay.base = replay.model.clone();
        replay.import(delta, ImportOptions::default()).unwrap();
        let Annotation::Ink(actual) = replay.model().annotation(&sid(1)).unwrap() else {
            panic!("expected ink");
        };
        assert_eq!(actual.base.rect, Some(rect));
        for (actual, expected) in actual.strokes[0].points.iter().zip(expected) {
            for index in 0..2 {
                assert!((actual[index] - expected[index]).abs() <= 0.001, "{fit}");
            }
        }
    }
}

#[test]
fn visual_signature_json_replay_preserves_tiny_source_geometry() {
    let mut engine = visual_signature_engine();
    let mut create = visual_ink(1, "contain");
    create.payload["strokeWidth"] = json!(0.000001);
    create.payload["strokes"] = json!([{"points": [[0.00001, 0.00002], [0.00004, 0.00003]]}]);
    engine.apply(vec![create]).unwrap();
    let Annotation::Ink(expected) = engine.model().annotation(&sid(1)).unwrap() else {
        panic!("expected ink");
    };

    for mode in [ExportMode::Full, ExportMode::Compacted] {
        let json = engine.export_json(mode, None);
        let delta: DeltaDocument = serde_json::from_str(&json).unwrap();
        let mut replay = visual_signature_engine();
        replay.import(delta, ImportOptions::default()).unwrap();
        let Annotation::Ink(actual) = replay.model().annotation(&sid(1)).unwrap() else {
            panic!("expected ink");
        };
        assert_eq!(actual.base.rect, expected.base.rect);
        assert!((actual.stroke_width.unwrap() - expected.stroke_width.unwrap()).abs() < 1e-9);
        for (actual, expected) in actual.strokes[0]
            .points
            .iter()
            .zip(&expected.strokes[0].points)
        {
            assert!((actual[0] - expected[0]).abs() < 1e-9);
            assert!((actual[1] - expected[1]).abs() < 1e-9);
        }
    }
}

#[test]
fn visual_signature_rejects_unrepresentable_widget_geometry() {
    for rect in [
        Rect::new(0.0, 0.0, 0.000_000_01, 0.000_000_01),
        Rect::new(1e12, 0.0, 1e12 + 100.0, 100.0),
        Rect::new(-f64::MAX, 0.0, f64::MAX, 100.0),
    ] {
        let mut engine = visual_signature_engine();
        engine
            .model
            .form_fields
            .get_mut("Signature1")
            .unwrap()
            .widgets[0]
            .rect = rect;
        assert_eq!(
            code(engine.apply(vec![visual_ink(1, "contain")])),
            ErrorCode::RectInvalid,
            "{rect:?}"
        );
    }
}

#[test]
fn visual_signature_binding_rejects_unsafe_fields_and_ambiguity() {
    let mut engine = visual_signature_engine();
    let field = engine.model.form_fields.get_mut("Signature1").unwrap();
    field.signature_state = Some(crate::model::SignatureFieldState::Occupied);
    assert_eq!(
        code(engine.apply(vec![visual_ink(1, "contain")])),
        ErrorCode::SignatureLocked
    );
    let field = engine.model.form_fields.get_mut("Signature1").unwrap();
    field.signature_state = Some(crate::model::SignatureFieldState::Unknown);
    assert_eq!(
        code(engine.apply(vec![visual_ink(1, "contain")])),
        ErrorCode::SignatureLocked
    );
    let field = engine.model.form_fields.get_mut("Signature1").unwrap();
    field.signature_state = Some(crate::model::SignatureFieldState::Empty);
    field.flags.read_only = true;
    assert_eq!(
        code(engine.apply(vec![visual_ink(1, "contain")])),
        ErrorCode::FieldReadonly
    );
    let field = engine.model.form_fields.get_mut("Signature1").unwrap();
    field.flags.read_only = false;
    let mut second = field.widgets[0].clone();
    second.id = Some("widget-2".into());
    field.widgets.push(second);
    let mut ambiguous = visual_ink(1, "contain");
    ambiguous
        .payload
        .as_object_mut()
        .unwrap()
        .remove("bindToWidget");
    assert_eq!(code(engine.apply(vec![ambiguous])), ErrorCode::Schema);
    engine.apply(vec![visual_ink(1, "contain")]).unwrap();
}

#[test]
fn signature_field_state_cannot_be_forged_by_form_ops() {
    let mut engine = visual_signature_engine();
    let update = op(
        OpKind::Update,
        Target::FormField,
        "Signature1",
        Some(0),
        json!({"signatureState": "empty"}),
        1,
    );
    assert_eq!(code(engine.apply(vec![update])), ErrorCode::SignatureLocked);
    let delete = op(
        OpKind::Delete,
        Target::FormField,
        "Signature1",
        Some(0),
        json!({}),
        2,
    );
    assert_eq!(code(engine.apply(vec![delete])), ErrorCode::SignatureLocked);
}

#[test]
fn visual_binding_cannot_be_bypassed_by_updates() {
    let mut engine = visual_signature_engine();
    engine.apply(vec![visual_ink(1, "contain")]).unwrap();
    let update = op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"rect": [0, 0, 10, 10], "flags": {"locked": false}}),
        1,
    );
    assert_eq!(code(engine.apply(vec![update])), ErrorCode::SignatureLocked);
    engine.apply(vec![square(2, 0, 2)]).unwrap();
    let bind = op(
        OpKind::Update,
        Target::Annotation,
        &sid(2),
        Some(0),
        json!({"bindToField": "Signature1"}),
        3,
    );
    assert_eq!(code(engine.apply(vec![bind])), ErrorCode::Schema);
}

// ---------------------------------------------------------- merge_patch

#[test]
fn rfc7386_appendix_a_cases() {
    let cases: [(Value, Value, Value); 15] = [
        (json!({"a":"b"}), json!({"a":"c"}), json!({"a":"c"})),
        (json!({"a":"b"}), json!({"b":"c"}), json!({"a":"b","b":"c"})),
        (json!({"a":"b"}), json!({"a":null}), json!({})),
        (
            json!({"a":"b","b":"c"}),
            json!({"a":null}),
            json!({"b":"c"}),
        ),
        (json!({"a":["b"]}), json!({"a":"c"}), json!({"a":"c"})),
        (json!({"a":"c"}), json!({"a":["b"]}), json!({"a":["b"]})),
        (
            json!({"a":{"b":"c"}}),
            json!({"a":{"b":"d","c":null}}),
            json!({"a":{"b":"d"}}),
        ),
        (json!({"a":[{"b":"c"}]}), json!({"a":[1]}), json!({"a":[1]})),
        (json!(["a", "b"]), json!(["c", "d"]), json!(["c", "d"])),
        (json!({"a":"b"}), json!(["c"]), json!(["c"])),
        (json!({"a":"foo"}), json!(null), json!(null)),
        (json!({"a":"foo"}), json!("bar"), json!("bar")),
        (json!({"e":null}), json!({"a":1}), json!({"e":null,"a":1})),
        (json!([1, 2]), json!({"a":"b","c":null}), json!({"a":"b"})),
        (
            json!({}),
            json!({"a":{"bb":{"ccc":null}}}),
            json!({"a":{"bb":{}}}),
        ),
    ];
    for (i, (b, p, want)) in cases.into_iter().enumerate() {
        let mut t = b.clone();
        merge_patch::apply(&mut t, &p);
        assert_eq!(t, want, "case {}", i + 1);
    }
}

#[test]
fn inverse_restores_nested() {
    let b = json!({"a": {"x": 1, "y": 2}, "keep": true});
    let p = json!({"a": {"x": 9, "z": null}, "new": 1});
    let mut after = b.clone();
    merge_patch::apply(&mut after, &p);
    let inv = merge_patch::inverse(&b, &p);
    let mut restored = after.clone();
    merge_patch::apply(&mut restored, &inv);
    assert_eq!(restored, b);
}

#[test]
fn inverse_array_whole() {
    let b = json!({"a": [1, 2, 3]});
    let p = json!({"a": [9]});
    let inv = merge_patch::inverse(&b, &p);
    assert_eq!(inv, json!({"a": [1, 2, 3]}));
}

// ------------------------------------------------------------- validate

#[test]
fn validate_unknown_id() {
    let mut e = engine();
    let c = code(e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        None,
        json!({"color": "#000000"}),
        0,
    )]));
    assert_eq!(c, ErrorCode::UnknownId);
}

#[test]
fn validate_duplicate_id() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    let mut different = square(1, 0, 1);
    different.payload["rect"] = json!([0.0, 0.0, 9.0, 9.0]);
    assert_eq!(code(e.apply(vec![different])), ErrorCode::DuplicateId);
}

#[test]
fn validate_schema_error() {
    let mut e = engine();
    let bad = op(
        OpKind::Create,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"subtype": "Nope", "page": 0}),
        0,
    );
    assert_eq!(code(e.apply(vec![bad])), ErrorCode::Schema);
}

#[test]
fn validate_page_out_of_range() {
    let mut e = engine();
    assert_eq!(
        code(e.apply(vec![square(1, 9, 0)])),
        ErrorCode::PageOutOfRange
    );
}

#[test]
fn validate_rect_invalid() {
    let mut e = engine();
    let mut bad = square(1, 0, 0);
    bad.payload["rect"] = json!([50.0, 10.0, 10.0, 50.0]);
    assert_eq!(code(e.apply(vec![bad])), ErrorCode::RectInvalid);
}

#[test]
fn validate_signature_locked() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    let sig = op(
        OpKind::Create,
        Target::Signature,
        "S1",
        Some(0),
        json!({"fieldName": "S1", "widget": {"page": 0, "rect": [0.0, 0.0, 10.0, 10.0]}, "status": "signed"}),
        1,
    );
    e.apply(vec![sig]).unwrap();
    let upd = op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        None,
        json!({"color": "#000000"}),
        2,
    );
    assert_eq!(code(e.apply(vec![upd])), ErrorCode::SignatureLocked);
}

#[test]
fn validate_field_readonly() {
    let mut e = engine();
    let f = op(
        OpKind::Create,
        Target::FormField,
        "f1",
        None,
        json!({"name": "f1", "fieldType": "text", "flags": {"readOnly": true}}),
        0,
    );
    e.apply(vec![f]).unwrap();
    let upd = op(
        OpKind::Update,
        Target::FormField,
        "f1",
        None,
        json!({"value": "x"}),
        1,
    );
    assert_eq!(code(e.apply(vec![upd])), ErrorCode::FieldReadonly);
}

#[test]
fn validate_value_not_in_options() {
    let mut e = engine();
    let f = op(
        OpKind::Create,
        Target::FormField,
        "c1",
        None,
        json!({"name": "c1", "fieldType": "checkbox", "exportValues": ["Yes"]}),
        0,
    );
    e.apply(vec![f]).unwrap();
    let upd = op(
        OpKind::Update,
        Target::FormField,
        "c1",
        None,
        json!({"value": "Maybe"}),
        1,
    );
    assert_eq!(code(e.apply(vec![upd])), ErrorCode::ValueNotInOptions);
    let ok = op(
        OpKind::Update,
        Target::FormField,
        "c1",
        None,
        json!({"value": "Yes"}),
        2,
    );
    e.apply(vec![ok]).unwrap();
}

#[test]
fn validate_maxlen_exceeded() {
    let mut e = engine();
    let f = op(
        OpKind::Create,
        Target::FormField,
        "t1",
        None,
        json!({"name": "t1", "fieldType": "text", "maxLen": 3}),
        0,
    );
    e.apply(vec![f]).unwrap();
    let upd = op(
        OpKind::Update,
        Target::FormField,
        "t1",
        None,
        json!({"value": "abcd"}),
        1,
    );
    assert_eq!(code(e.apply(vec![upd])), ErrorCode::MaxlenExceeded);
}

#[test]
fn validate_revision_mismatch() {
    let mut e = engine();
    let doc = DeltaDocument {
        schema_version: 1,
        document_id: "sha256:test".into(),
        pdf_id: None,
        base_revision: 5,
        generator: None,
        created_at: None,
        ops: vec![],
        extra: Default::default(),
    };
    match e.import(doc, ImportOptions::default()) {
        Err(DeltaError::RevisionMismatch { expected, got }) => {
            assert_eq!((expected, got), (0, 5));
        }
        other => panic!("expected RevisionMismatch, got {other:?}"),
    }
}

#[test]
fn group_is_atomic() {
    let mut e = engine();
    let res = e.apply(vec![square(1, 0, 0), square(2, 9, 1)]);
    assert!(invalid(res)
        .iter()
        .any(|v| v.code == ErrorCode::PageOutOfRange));
    assert_eq!(e.model().len(), 0, "failed group must not apply");
}

#[test]
fn create_identical_is_idempotent_skip() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    // Same entity, fresh opId: injected id/page match, timestamps differ but
    // are stripped for the comparison... createdAt differs → not identical.
    let mut replay = square(1, 0, 0);
    replay.op_id = uid(99);
    let r = e.apply(vec![replay]).unwrap();
    // created_at is set from the op ts; identical ts → identical entity.
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(e.model().len(), 1);
}

// ----------------------------------------------------------------- undo

#[test]
fn inverse_create_is_delete() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    let step = e.undo.undo.last().unwrap();
    assert_eq!(step[0].inverse.kind, OpKind::Delete);
    assert_eq!(step[0].inverse.id, sid(1));
}

#[test]
fn inverse_delete_is_full_create() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![op(
        OpKind::Delete,
        Target::Annotation,
        &sid(1),
        None,
        json!({}),
        1,
    )])
    .unwrap();
    let step = e.undo.undo.last().unwrap();
    assert_eq!(step[0].inverse.kind, OpKind::Create);
    let p = &step[0].inverse.payload;
    assert_eq!(p["subtype"], json!("Square"));
    assert_eq!(p["rect"], json!([10.0, 10.0, 50.0, 50.0]));
}

#[test]
fn inverse_update_restores_prev() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        None,
        json!({"color": "#00FF00", "contents": "hi"}),
        1,
    )])
    .unwrap();
    let inv = &e.undo.undo.last().unwrap()[0].inverse;
    assert_eq!(inv.payload["color"], json!("#FF0000"));
    assert_eq!(inv.payload["contents"], json!(null));
}

#[test]
fn coalesce_typing() {
    let mut e = engine();
    let f = op(
        OpKind::Create,
        Target::FormField,
        "t",
        None,
        json!({"name": "t", "fieldType": "text"}),
        0,
    );
    e.apply(vec![f]).unwrap();
    let u1 = op(
        OpKind::Update,
        Target::FormField,
        "t",
        None,
        json!({"value": "a"}),
        100,
    );
    let u2 = op(
        OpKind::Update,
        Target::FormField,
        "t",
        None,
        json!({"value": "ab"}),
        200,
    );
    e.apply(vec![u1]).unwrap();
    e.apply(vec![u2]).unwrap();
    assert_eq!(
        e.undo_depth(),
        2,
        "two same-key updates coalesce into one step"
    );
    e.undo().unwrap();
    assert_eq!(e.model().form_field("t").unwrap().value, None);
}

#[test]
fn coalesce_gap_over_500ms_not_merged() {
    let mut e = engine();
    let f = op(
        OpKind::Create,
        Target::FormField,
        "t",
        None,
        json!({"name": "t", "fieldType": "text"}),
        0,
    );
    e.apply(vec![f]).unwrap();
    e.apply(vec![op(
        OpKind::Update,
        Target::FormField,
        "t",
        None,
        json!({"value": "a"}),
        100,
    )])
    .unwrap();
    e.apply(vec![op(
        OpKind::Update,
        Target::FormField,
        "t",
        None,
        json!({"value": "ab"}),
        700,
    )])
    .unwrap();
    assert_eq!(e.undo_depth(), 3);
}

#[test]
fn explicit_group_undoes_together() {
    let mut e = engine();
    e.begin_group();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![square(2, 0, 1)]).unwrap();
    e.end_group();
    assert_eq!(e.undo_depth(), 1);
    e.undo().unwrap();
    assert_eq!(e.model().len(), 0);
}

#[test]
fn limit_drops_oldest_but_keeps_history() {
    let mut e = engine();
    e.undo.limit = 3;
    for n in 0..5 {
        e.apply(vec![square(n, 0, n as i64 * 1000)]).unwrap();
    }
    assert_eq!(e.undo_depth(), 3);
    assert_eq!(e.undo.history.len(), 5);
    // Oldest two were dropped; only the three newest can be undone.
    e.undo().unwrap();
    e.undo().unwrap();
    e.undo().unwrap();
    assert!(!e.can_undo());
    assert_eq!(e.model().len(), 2);
}

#[test]
fn redo_cleared_on_apply() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.undo().unwrap();
    assert!(e.can_redo());
    e.apply(vec![square(2, 0, 1)]).unwrap();
    assert!(!e.can_redo());
}

#[test]
fn undo_redo_roundtrip() {
    let mut e = engine();
    let before = e.model().canonical();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    let after = e.model().canonical();
    e.undo().unwrap();
    assert_eq!(e.model().canonical(), before);
    e.redo().unwrap();
    assert_eq!(e.model().canonical(), after);
}

#[test]
fn full_export_replays_undo_redo() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        None,
        json!({"color": "#00FF00"}),
        1,
    )])
    .unwrap();
    e.undo().unwrap();
    e.redo().unwrap();
    e.undo().unwrap();
    e.redo().unwrap();
    let doc = e.export(ExportMode::Full, None);
    let mut fresh = engine();
    fresh
        .import(
            doc,
            ImportOptions {
                force: false,
                rebase: false,
                undoable: false,
            },
        )
        .unwrap();
    assert_eq!(fresh.model().canonical(), e.model().canonical());
}

// --------------------------------------------------------------- export

fn scenario() -> Engine {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        None,
        json!({"color": "#00FF00"}),
        1000,
    )])
    .unwrap();
    e.apply(vec![square(2, 1, 2000)]).unwrap();
    e
}

#[test]
fn compacted_create_then_update_is_single_create() {
    let e = scenario();
    let doc = e.export(ExportMode::Compacted, None);
    let creates: Vec<_> = doc.ops.iter().filter(|o| o.id == sid(1)).collect();
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].kind, OpKind::Create);
    assert_eq!(creates[0].payload["color"], json!("#00FF00"));
}

#[test]
fn compacted_create_then_delete_is_nothing() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![op(
        OpKind::Delete,
        Target::Annotation,
        &sid(1),
        None,
        json!({}),
        1,
    )])
    .unwrap();
    let doc = e.export(ExportMode::Compacted, None);
    assert!(doc.ops.iter().all(|o| o.id != sid(1)));
}

#[test]
fn compacted_signature_never_dropped() {
    let mut e = engine();
    let sig = op(
        OpKind::Create,
        Target::Signature,
        "S1",
        Some(0),
        json!({"fieldName": "S1", "widget": {"page": 0, "rect": [0.0, 0.0, 10.0, 10.0]}, "status": "prepared"}),
        0,
    );
    let sig_id = sig.op_id;
    e.apply(vec![sig]).unwrap();
    e.apply(vec![op(
        OpKind::Delete,
        Target::Signature,
        "S1",
        None,
        json!({}),
        1,
    )])
    .unwrap();
    let doc = e.export(ExportMode::Compacted, None);
    assert!(
        doc.ops.iter().any(|o| o.op_id == sig_id),
        "signature ops stay verbatim"
    );
}

// --------------------------------------------------------------- import

#[test]
fn import_skips_seen_op_ids() {
    let mut e = scenario();
    let doc = e.export(ExportMode::Full, None);
    let r = e.import(doc, ImportOptions::default()).unwrap();
    assert_eq!(r.applied.len(), 0);
    assert_eq!(r.skipped.len(), 3);
}

#[test]
fn import_rebase_skips_unknown_ids() {
    let e = scenario();
    let doc = DeltaDocument {
        base_revision: 99,
        ..e.export(ExportMode::Full, None)
    };
    let mut fresh = engine();
    let r = fresh
        .import(
            doc,
            ImportOptions {
                force: true,
                rebase: true,
                undoable: true,
            },
        )
        .unwrap();
    assert_eq!(r.applied.len(), 3);
    // An update to an entity that does not exist under rebase is rejected.
    let doc2 = DeltaDocument {
        base_revision: 42,
        ops: vec![op(
            OpKind::Update,
            Target::Annotation,
            &sid(55),
            None,
            json!({"color": "#000000"}),
            9,
        )],
        ..e.export(ExportMode::Full, None)
    };
    let r2 = fresh
        .import(
            doc2,
            ImportOptions {
                force: true,
                rebase: true,
                undoable: true,
            },
        )
        .unwrap();
    assert_eq!(r2.rejected.len(), 1);
    assert_eq!(r2.rejected[0].code, ErrorCode::UnknownId);
}

#[test]
fn import_undoable_single_group() {
    let e = scenario();
    let doc = e.export(ExportMode::Full, None);
    let mut fresh = engine();
    fresh.import(doc, ImportOptions::default()).unwrap();
    assert_eq!(fresh.undo_depth(), 1);
    fresh.undo().unwrap();
    assert_eq!(fresh.model().len(), 0);
}

#[test]
fn import_document_id_mismatch() {
    let mut e = engine();
    let mut doc = e.export(ExportMode::Full, None);
    doc.document_id = "sha256:other".into();
    assert!(matches!(
        e.import(doc.clone(), ImportOptions::default()),
        Err(DeltaError::Invalid(_))
    ));
    let mut e2 = engine();
    e2.import(
        doc,
        ImportOptions {
            force: true,
            rebase: false,
            undoable: true,
        },
    )
    .unwrap();
}

// ---------------------------------------------------------------- dirty

#[test]
fn ink_bbox_includes_stroke_width() {
    let ink = op(
        OpKind::Create,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"subtype": "Ink", "page": 0, "strokeWidth": 4.0,
               "strokes": [{"points": [[10.0, 10.0], [20.0, 20.0]]}]}),
        0,
    );
    let mut e = engine();
    e.apply(vec![ink]).unwrap();
    let a = e.model().annotation(&sid(1)).unwrap();
    let r = a.base().rect.unwrap();
    assert_eq!(
        r,
        Rect::new(8.0, 8.0, 22.0, 22.0),
        "inflated by strokeWidth/2"
    );
}

#[test]
fn move_dirty_is_union() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    let r = e
        .apply(vec![op(
            OpKind::Update,
            Target::Annotation,
            &sid(1),
            None,
            json!({"rect": [100.0, 100.0, 150.0, 150.0]}),
            1,
        )])
        .unwrap();
    let u = r.dirty[0].1;
    assert_eq!(r.dirty.len(), 1);
    assert_eq!(u, Rect::new(10.0, 10.0, 150.0, 150.0));
}

// ----------------------------------------------------------------- hit

#[test]
fn hit_test_ink_polyline() {
    let ink = op(
        OpKind::Create,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"subtype": "Ink", "page": 0, "strokeWidth": 2.0,
               "strokes": [{"points": [[0.0, 0.0], [100.0, 0.0]]}]}),
        0,
    );
    let mut e = engine();
    e.apply(vec![ink]).unwrap();
    assert_eq!(e.hit_test(0, Point::new(50.0, 1.5), 1.0), Some(sid(1)));
    assert_eq!(e.hit_test(0, Point::new(50.0, 10.0), 1.0), None);
}

#[test]
fn hit_test_highlight_quad() {
    let h = op(
        OpKind::Create,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"subtype": "Highlight", "page": 0,
               "quadPoints": [[10.0, 20.0, 60.0, 20.0, 10.0, 10.0, 60.0, 10.0]]}),
        0,
    );
    let mut e = engine();
    e.apply(vec![h]).unwrap();
    assert_eq!(e.hit_test(0, Point::new(30.0, 15.0), 0.0), Some(sid(1)));
    assert_eq!(e.hit_test(0, Point::new(30.0, 25.0), 0.0), None);
}

#[test]
fn hit_test_prefers_topmost() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    e.apply(vec![square(2, 0, 1)]).unwrap();
    // Both squares occupy [10,50]²; id 2 is on top (later in z-order).
    assert_eq!(e.hit_test(0, Point::new(30.0, 30.0), 0.0), Some(sid(2)));
    e.undo().unwrap();
    assert_eq!(e.hit_test(0, Point::new(30.0, 30.0), 0.0), Some(sid(1)));
}

// ------------------------------------------------------------ snapshots

#[test]
fn export_full_snapshot() {
    let e = scenario();
    let now = Utc.timestamp_millis_opt(1_800_000_000_000).unwrap();
    let mut counter = 0u128;
    let doc = e.export_with_clock(ExportMode::Full, Some("test".into()), now, &mut || {
        counter += 1;
        uid(5000 + counter)
    });
    insta::assert_json_snapshot!(serde_json::to_value(&doc).unwrap());
}

#[test]
fn export_compacted_snapshot() {
    let e = scenario();
    let now = Utc.timestamp_millis_opt(1_800_000_000_000).unwrap();
    let mut counter = 0u128;
    let doc = e.export_with_clock(ExportMode::Compacted, Some("test".into()), now, &mut || {
        counter += 1;
        uid(5000 + counter)
    });
    insta::assert_json_snapshot!(serde_json::to_value(&doc).unwrap());
}

// ---------------------------------------------------------- M3 writer hooks

#[test]
fn with_base_then_compacted_is_empty() {
    let a: crate::model::Annotation = serde_json::from_value(json!({
        "subtype": "Square", "id": sid(1), "page": 0,
        "rect": [10.0, 10.0, 50.0, 50.0], "color": "#FF0000"
    }))
    .unwrap();
    let e = Engine::with_base("sha256:test".into(), 3, vec![a], vec![]);
    assert!(e.flush_ops().is_empty());
    assert!(e.uncommitted_ids().is_empty());
    assert_eq!(e.model().annotations.len(), 1);
}

#[test]
fn uncommitted_after_update() {
    let a: crate::model::Annotation = serde_json::from_value(json!({
        "subtype": "Square", "id": sid(1), "page": 0,
        "rect": [10.0, 10.0, 50.0, 50.0], "color": "#FF0000"
    }))
    .unwrap();
    let mut e = Engine::with_base("sha256:test".into(), 3, vec![a], vec![]);
    e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"color": "#00FF00"}),
        1,
    )])
    .unwrap();
    assert_eq!(e.uncommitted_ids(), vec![sid(1)]);
    assert_eq!(e.flush_ops().len(), 1);
    assert_eq!(e.flush_ops()[0].kind, OpKind::Update);
}

#[test]
fn mark_flushed_clears_uncommitted() {
    let mut e = engine();
    e.apply(vec![square(1, 0, 0)]).unwrap();
    assert_eq!(e.uncommitted_ids(), vec![sid(1)]);
    assert_eq!(e.flush_ops().len(), 1);
    e.mark_flushed(7);
    assert!(e.uncommitted_ids().is_empty());
    assert!(e.flush_ops().is_empty());
    assert_eq!(e.revision(), 7);
    // New edits after the flush are uncommitted again.
    e.apply(vec![op(
        OpKind::Update,
        Target::Annotation,
        &sid(1),
        Some(0),
        json!({"color": "#00FF00"}),
        2,
    )])
    .unwrap();
    assert_eq!(e.uncommitted_ids(), vec![sid(1)]);
}

// ------------------------------------------------------------- form fields

fn field_op(kind: OpKind, name: &str, payload: Value, ms: i64) -> Op {
    op(kind, Target::FormField, name, Some(0), payload, ms)
}

fn checkbox_field(name: &str, export: &str) -> Value {
    json!({
        "name": name,
        "fieldType": "checkbox",
        "value": "Off",
        "exportValues": [export],
        "widgets": [{
            "page": 0,
            "rect": [10.0, 10.0, 30.0, 30.0],
            "exportValue": export,
            "appearanceState": "Off"
        }]
    })
}

fn radio_field(name: &str) -> Value {
    json!({
        "name": name,
        "fieldType": "radio",
        "value": "Off",
        "exportValues": ["A", "B"],
        "widgets": [
            {"page": 0, "rect": [10.0, 10.0, 30.0, 30.0], "exportValue": "A", "appearanceState": "Off"},
            {"page": 0, "rect": [40.0, 10.0, 60.0, 30.0], "exportValue": "B", "appearanceState": "Off"}
        ]
    })
}

fn text_field(name: &str) -> Value {
    json!({
        "name": name,
        "fieldType": "text",
        "value": "",
        "widgets": [{"page": 0, "rect": [10.0, 10.0, 200.0, 30.0]}]
    })
}

#[test]
fn form_update_sets_appearance_states() {
    let mut e = engine();
    e.apply(vec![field_op(
        OpKind::Create,
        "cb",
        checkbox_field("cb", "Yes"),
        0,
    )])
    .unwrap();
    e.apply(vec![field_op(
        OpKind::Update,
        "cb",
        json!({"value": "Yes"}),
        1,
    )])
    .unwrap();
    let f = e.model().form_field("cb").unwrap();
    assert_eq!(f.value, Some(json!("Yes")));
    assert_eq!(f.widgets[0].appearance_state.as_deref(), Some("Yes"));

    e.apply(vec![field_op(OpKind::Create, "r", radio_field("r"), 2)])
        .unwrap();
    e.apply(vec![field_op(
        OpKind::Update,
        "r",
        json!({"value": "B"}),
        3,
    )])
    .unwrap();
    let f = e.model().form_field("r").unwrap();
    assert_eq!(f.widgets[0].appearance_state.as_deref(), Some("Off"));
    assert_eq!(f.widgets[1].appearance_state.as_deref(), Some("B"));
}

#[test]
fn form_undo_restores_appearance_states() {
    let mut e = engine();
    e.apply(vec![field_op(OpKind::Create, "r", radio_field("r"), 0)])
        .unwrap();
    e.apply(vec![field_op(
        OpKind::Update,
        "r",
        json!({"value": "A"}),
        1,
    )])
    .unwrap();
    e.undo().unwrap();
    let f = e.model().form_field("r").unwrap();
    assert_eq!(f.value, Some(json!("Off")));
    assert_eq!(f.widgets[0].appearance_state.as_deref(), Some("Off"));
    assert_eq!(f.widgets[1].appearance_state.as_deref(), Some("Off"));
    e.redo().unwrap();
    let f = e.model().form_field("r").unwrap();
    assert_eq!(f.widgets[0].appearance_state.as_deref(), Some("A"));
}

#[test]
fn uncommitted_includes_form_fields() {
    let f: FormField = serde_json::from_value(checkbox_field("cb", "Yes")).unwrap();
    let mut e = Engine::with_base("sha256:test".into(), 3, vec![], vec![f]);
    assert!(e.uncommitted_ids().is_empty());
    e.apply(vec![field_op(
        OpKind::Update,
        "cb",
        json!({"value": "Yes"}),
        1,
    )])
    .unwrap();
    assert_eq!(e.uncommitted_ids(), vec!["cb".to_string()]);
}

#[test]
fn adopt_base_fields_idempotent() {
    let f: FormField = serde_json::from_value(checkbox_field("cb", "Yes")).unwrap();
    let mut e = engine();
    e.adopt_base(vec![], vec![f.clone()]);
    e.adopt_base(vec![], vec![f]);
    assert_eq!(e.model().form_fields.len(), 1);
    assert_eq!(e.model().order.iter().filter(|id| *id == "cb").count(), 1);
    assert!(e.uncommitted_ids().is_empty());
    assert_eq!(e.model().form_fields_ordered().len(), 1);
    assert_eq!(e.model().form_fields_on_page(0).len(), 1);
    assert!(e.model().form_fields_on_page(1).is_empty());
}

#[test]
fn form_value_typing_coalesces() {
    let mut e = engine();
    e.apply(vec![field_op(OpKind::Create, "t", text_field("t"), 0)])
        .unwrap();
    e.apply(vec![field_op(
        OpKind::Update,
        "t",
        json!({"value": "a"}),
        100,
    )])
    .unwrap();
    e.apply(vec![field_op(
        OpKind::Update,
        "t",
        json!({"value": "ab"}),
        200,
    )])
    .unwrap();
    // create (1) + one coalesced value step.
    assert_eq!(e.undo_depth(), 2);
    e.undo().unwrap();
    assert_eq!(e.model().form_field("t").unwrap().value, Some(json!("")));
    e.redo().unwrap();
    assert_eq!(e.model().form_field("t").unwrap().value, Some(json!("ab")));
}

#[test]
fn drop_form_fields_clears_everything() {
    let f: FormField = serde_json::from_value(checkbox_field("cb", "Yes")).unwrap();
    let mut e = Engine::with_base("sha256:test".into(), 3, vec![], vec![f]);
    e.drop_form_fields();
    assert!(e.model().form_fields.is_empty());
    assert!(e.model().order.is_empty());
    assert!(e.uncommitted_ids().is_empty());
}
