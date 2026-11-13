use super::*;
use crate::geom::{Point, QuadPoints, Rect};
use serde_json::{json, Value};

const ID: &str = "9b3a6c1e-1d2f-4e0a-9e4f-7a2c0b5d1f10";

fn roundtrip<T>(v: &Value) -> (T, Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let a: T = serde_json::from_value(v.clone()).expect("deserialize");
    let s = serde_json::to_value(&a).expect("serialize");
    let b: T = serde_json::from_value(s.clone()).expect("re-deserialize");
    assert_eq!(serde_json::to_value(&b).unwrap(), s);
    (a, s)
}

#[test]
fn color_parse_and_reject() {
    let c: Color = serde_json::from_value(json!("#1A237E")).unwrap();
    assert_eq!(c.0, "#1A237E");
    for bad in ["red", "#12345", "#GGGGGG"] {
        assert!(
            serde_json::from_value::<Color>(json!(bad)).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn rect_serde_array() {
    let r: Rect = serde_json::from_value(json!([1.0, 2.0, 3.5, 4.5])).unwrap();
    assert_eq!(r, Rect::new(1.0, 2.0, 3.5, 4.5));
    assert_eq!(
        serde_json::to_value(r).unwrap(),
        json!([1.0, 2.0, 3.5, 4.5])
    );
}

#[test]
fn point_and_quad_serde() {
    let p: Point = serde_json::from_value(json!([1.5, -2.0])).unwrap();
    assert_eq!(p, Point::new(1.5, -2.0));
    assert_eq!(serde_json::to_value(p).unwrap(), json!([1.5, -2.0]));

    let arr = json!([100.0, 700.5, 300.2, 700.5, 100.0, 688.0, 300.2, 688.0]);
    let q: QuadPoints = serde_json::from_value(arr.clone()).unwrap();
    assert_eq!(serde_json::to_value(q).unwrap(), arr);
}

#[test]
fn annotation_tag_roundtrip_all_11_subtypes() {
    let base = |extra: Value| {
        let mut o = serde_json::Map::new();
        o.insert("id".into(), json!(ID));
        o.insert("page".into(), json!(0));
        if let Value::Object(e) = extra {
            o.extend(e);
        }
        Value::Object(o)
    };
    let cases = [
        base(json!({"subtype": "Ink", "strokes": []})),
        base(json!({"subtype": "Highlight", "quadPoints": []})),
        base(json!({"subtype": "Underline", "quadPoints": []})),
        base(json!({"subtype": "StrikeOut", "quadPoints": []})),
        base(json!({"subtype": "Squiggly", "quadPoints": []})),
        base(json!({"subtype": "FreeText"})),
        base(json!({"subtype": "Text"})),
        base(json!({"subtype": "Stamp"})),
        base(json!({"subtype": "Square"})),
        base(json!({"subtype": "Circle"})),
        base(json!({"subtype": "Line", "start": [0.0, 0.0], "end": [1.0, 1.0]})),
    ];
    for case in cases {
        let tag = case["subtype"].as_str().unwrap().to_string();
        let (_, s) = roundtrip::<Annotation>(&case);
        assert_eq!(s["subtype"], json!(tag), "{tag}");
    }
}

#[test]
fn unknown_keys_preserved_in_extra() {
    let v = json!({
        "subtype": "Square",
        "id": ID,
        "page": 0,
        "futureKey": 1,
        "nested": {"deep": [1, 2, 3]}
    });
    let (a, s) = roundtrip::<Annotation>(&v);
    let Annotation::Square(sq) = a else { panic!() };
    assert_eq!(sq.base.extra["futureKey"], json!(1));
    assert_eq!(s["nested"], json!({"deep": [1, 2, 3]}));
    assert_eq!(s["futureKey"], json!(1));
}

#[test]
fn optional_fields_omitted_when_none() {
    let v = json!({"subtype": "Ink", "id": ID, "page": 0, "strokes": []});
    let (_, s) = roundtrip::<Annotation>(&v);
    let o = s.as_object().unwrap();
    for key in [
        "author", "contents", "subject", "rect", "color", "intent", "signer",
    ] {
        assert!(!o.contains_key(key), "{key} present");
    }
}

#[test]
fn line_ending_rename_r_variants() {
    assert_eq!(
        serde_json::to_value(LineEnding::ROpenArrow).unwrap(),
        json!("rOpenArrow")
    );
    assert_eq!(
        serde_json::to_value(LineEnding::RClosedArrow).unwrap(),
        json!("rClosedArrow")
    );
    let e: LineEnding = serde_json::from_value(json!("rOpenArrow")).unwrap();
    assert_eq!(e, LineEnding::ROpenArrow);
}

#[test]
fn signature_enum_renames() {
    assert_eq!(
        serde_json::to_value(SubFilter::EtsiCadesDetached).unwrap(),
        json!("ETSI.CAdES.detached")
    );
    assert_eq!(
        serde_json::from_value::<SubFilter>(json!("adbe.pkcs7.detached")).unwrap(),
        SubFilter::AdbePkcs7Detached
    );
    assert_eq!(
        serde_json::to_value(DigestAlgorithm::Sha256).unwrap(),
        json!("SHA-256")
    );
    assert_eq!(
        serde_json::from_value::<SigLevel>(json!("B-B")).unwrap(),
        SigLevel::BB
    );
    assert_eq!(
        serde_json::to_value(SignatureAlgorithm::RsaPkcs1v15).unwrap(),
        json!("RSA-PKCS1v15")
    );
}

#[test]
fn form_field_roundtrip() {
    let v = json!({
        "name": "applicant.name",
        "fieldType": "text",
        "value": "Andrea Rosa",
        "defaultValue": "",
        "flags": {"required": true},
        "maxLen": 64,
        "defaultAppearance": {"fontFamily": "Helvetica", "fontSize": 0, "color": "#000000"},
        "textAlign": "left",
        "tooltip": "Full legal name",
        "widgets": [{
            "id": "w-7f3b",
            "page": 0,
            "rect": [120, 640, 420, 662],
            "borderColor": "#000000",
            "backgroundColor": "#FFFFFF",
            "borderWidth": 1,
            "borderStyle": {"style": "solid", "dash": null},
            "rotation": 0
        }]
    });
    let (f, s) = roundtrip::<FormField>(&v);
    assert_eq!(f.name, "applicant.name");
    assert_eq!(f.field_type, FieldType::Text);
    assert_eq!(f.widgets.len(), 1);
    assert_eq!(s["fieldType"], json!("text"));
}

#[test]
fn explicit_nulls_deserialize_to_none() {
    let v = json!({
        "subtype": "Ink",
        "id": ID,
        "page": 0,
        "strokes": [{"points": [[120.0, 140.0], [121.5, 141.2]]}],
        "intent": null,
        "signer": null,
        "bindToField": null,
        "fitMode": null
    });
    let (a, _) = roundtrip::<Annotation>(&v);
    let Annotation::Ink(ink) = a else { panic!() };
    assert_eq!(ink.strokes[0].points.len(), 2);
    assert_eq!(ink.strokes[0].points[0].len(), 2);
    assert!(ink.intent.is_none());
}
