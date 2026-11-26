use nib_core::model::{Annotation, DeltaDocument};
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::path::Path;

const ID: &str = "00000000-0000-0000-0000-000000000001";

fn write(target: &str, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    if target == "open_bytes" && bytes.len() >= 200_000 {
        return Err(format!("{name}: PDF seed exceeds size budget").into());
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(target);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(name), bytes)?;
    Ok(())
}

fn annotation(subtype: &str, fields: Value) -> Value {
    let mut value = json!({
        "id": ID, "page": 0, "subtype": subtype,
        "rect": [10, 20, 110, 60],
        "color": "#123456", "contents": "fuzz"
    });
    if let (Some(base), Value::Object(fields)) = (value.as_object_mut(), fields) {
        base.extend(fields);
    }
    value
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut annotations = vec![
        (
            "ink",
            annotation("Ink", json!({"strokes": [{"points": [[10,20],[30,40]]}]})),
        ),
        (
            "ink-pressure",
            annotation(
                "Ink",
                json!({"pressureSensitive": true, "strokes": [{"points": [[10,20,0.2],[30,40,0.9]]}]}),
            ),
        ),
        (
            "ink-catmull",
            annotation(
                "Ink",
                json!({"smoothing": {"algorithm": "catmullRom"}, "strokes": [{"points": [[10,20],[20,35],[30,40]]}]}),
            ),
        ),
        (
            "ink-short-point",
            annotation("Ink", json!({"strokes": [{"points": [[]]}]})),
        ),
        ("ink-empty", annotation("Ink", json!({"strokes": []}))),
        ("freetext", annotation("FreeText", json!({"fontSize": 12}))),
        ("note", annotation("Text", json!({}))),
        (
            "stamp",
            annotation(
                "Stamp",
                json!({"kind": "standard", "standardName": "Approved"}),
            ),
        ),
        (
            "stamp-vector",
            annotation(
                "Stamp",
                json!({"kind": "vector", "vector": {"viewBox": [0,0,100,50], "paths": [{"d": "M 0 0 L 100 50 Z", "stroke": "#123456"}]}}),
            ),
        ),
        (
            "stamp-broken-image",
            annotation(
                "Stamp",
                json!({"kind": "image", "image": {"dataBase64": "AA=="}}),
            ),
        ),
        ("square", annotation("Square", json!({}))),
        ("circle", annotation("Circle", json!({}))),
        (
            "line",
            annotation("Line", json!({"start": [10,20], "end": [100,50]})),
        ),
    ];
    for subtype in ["Highlight", "Underline", "StrikeOut", "Squiggly"] {
        annotations.push((
            subtype,
            annotation(
                subtype,
                json!({"quadPoints": [[10,40,100,40,10,20,100,20]]}),
            ),
        ));
    }
    for (name, value) in annotations {
        serde_json::from_value::<Annotation>(value.clone())?;
        write(
            "ap_from_annotation",
            &format!("{name}.json"),
            &serde_json::to_vec(&value)?,
        )?;
        let delta = json!({
            "schemaVersion": 1, "documentId": "fuzz", "baseRevision": 0,
            "ops": [{
                "opId": ID, "ts": "2026-01-01T00:00:00Z",
                "type": "create", "target": "annotation", "id": ID,
                "page": 0, "payload": value
            }]
        });
        serde_json::from_value::<DeltaDocument>(delta.clone())?;
        write(
            "import_delta_json",
            &format!("{name}.json"),
            &serde_json::to_vec(&delta)?,
        )?;
    }
    write(
        "import_delta_json",
        "empty.json",
        br#"{"schemaVersion":1,"documentId":"fuzz","baseRevision":0,"ops":[]}"#,
    )?;
    for target in ["import_delta_json", "ap_from_annotation"] {
        write(target, "truncated.json", br#"{"subtype":"Ink","strokes":["#)?;
        write(target, "invalid-utf8", &[0xff, 0x00, b'{'])?;
    }
    let pdfs: &[(&str, &[u8])] = &[
        (
            "classic.pdf",
            include_bytes!("../../nib-core/tests/fixtures/incr-classic.pdf"),
        ),
        (
            "xrefstream.pdf",
            include_bytes!("../../nib-core/tests/fixtures/incr-xrefstream.pdf"),
        ),
        (
            "hybrid.pdf",
            include_bytes!("../../nib-core/tests/fixtures/incr-hybrid.pdf"),
        ),
        (
            "linearized.pdf",
            include_bytes!("../../nib-core/tests/fixtures/incr-linearized.pdf"),
        ),
        (
            "annotations.pdf",
            include_bytes!("../../nib-core/tests/fixtures/annots-existing.pdf"),
        ),
        (
            "form.pdf",
            include_bytes!("../../nib-core/tests/fixtures/form.pdf"),
        ),
        (
            "signature.pdf",
            include_bytes!("../../nib-core/tests/fixtures/sig-field.pdf"),
        ),
    ];
    for (name, bytes) in pdfs {
        write("open_bytes", name, bytes)?;
    }
    let classic = pdfs[0].1;
    if let Some(offset) = classic.windows(10).position(|w| w == b"startxref\n") {
        write("open_bytes", "truncated-xref.pdf", &classic[..offset])?;
    } else {
        return Err("classic fixture lacks startxref".into());
    }
    write(
        "open_bytes",
        "broken-header.pdf",
        b"%PDF-1.7\n1 0 obj\n<< /Annots [99 0 R] /AP 1 >>\nendobj\n",
    )?;
    Ok(())
}
