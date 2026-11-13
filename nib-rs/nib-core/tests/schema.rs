//! Checks the checked-in JSON Schema is up to date and that every ```json
//! example block in `docs/JSON_SCHEMA.md` validates and round-trips.

use nib_core::model::schema::{delta_schema, delta_schema_string};
use nib_core::model::{Annotation, DeltaDocument, FormField, Op, SignatureEntity};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::PathBuf;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/nib-delta.schema.json")
}

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/JSON_SCHEMA.md")
        .canonicalize()
        .expect("docs/JSON_SCHEMA.md")
}

/// Every fenced ```json block in the spec document.
fn example_blocks() -> Vec<Value> {
    let doc = std::fs::read_to_string(doc_path()).unwrap();
    let mut blocks = Vec::new();
    let mut rest = doc.as_str();
    while let Some(start) = rest.find("```json") {
        rest = &rest[start + "```json".len()..];
        let end = rest.find("```").expect("unclosed fence");
        let body = &rest[..end];
        rest = &rest[end + 3..];
        blocks.push(
            serde_json::from_str(body)
                .unwrap_or_else(|e| panic!("invalid json block: {e}\n{body}")),
        );
    }
    assert!(
        blocks.len() >= 12,
        "expected >= 12 example blocks, found {}",
        blocks.len()
    );
    blocks
}

enum Kind {
    Root,
    Op,
    Ops,
    Annotation,
    FormField,
    Signature,
}

fn classify(v: &Value) -> Kind {
    if let Some(arr) = v.as_array() {
        assert!(
            arr.iter().all(|e| e.get("opId").is_some()),
            "array block whose elements lack opId: {v}"
        );
        return Kind::Ops;
    }
    let o = v.as_object().expect("example block is not an object/array");
    if o.contains_key("schemaVersion") {
        Kind::Root
    } else if o.contains_key("opId") {
        Kind::Op
    } else if o.contains_key("subtype") {
        Kind::Annotation
    } else if o.contains_key("fieldType") {
        Kind::FormField
    } else if o.contains_key("fieldName") {
        Kind::Signature
    } else {
        panic!("unclassifiable example block: {v}");
    }
}

/// A validator for the whole schema or for one `$defs` entry.
fn validator_for_def(def: Option<&str>) -> (jsonschema::Validator, Value) {
    let root = delta_schema();
    let schema = match def {
        None => root.clone(),
        Some(name) => json!({
            "$ref": format!("#/$defs/{name}"),
            "$defs": root["$defs"].clone(),
        }),
    };
    let v = jsonschema::validator_for(&schema).expect("schema compiles");
    (v, schema)
}

fn validate_value(v: &Value, schema: &jsonschema::Validator, ctx: &str) {
    if let Err(e) = schema.validate(v) {
        panic!("{ctx} failed schema validation: {e}\n{v}");
    }
}

fn roundtrip_typed<T>(v: &Value, ctx: &str)
where
    T: serde::Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let a: T = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{ctx}: {e}"));
    let s = serde_json::to_value(&a).unwrap();
    let b: T = serde_json::from_value(s).unwrap();
    assert_eq!(a, b, "{ctx} roundtrip changed value");
}

#[test]
fn schema_is_up_to_date() {
    let on_disk = std::fs::read_to_string(schema_path()).expect("read checked-in schema");
    assert_eq!(
        on_disk,
        delta_schema_string(),
        "schema drift — regenerate with `cargo test -p nib-core --test schema schema_regenerate -- --ignored`"
    );
}

#[test]
#[ignore]
fn schema_regenerate() {
    std::fs::create_dir_all(schema_path().parent().unwrap()).unwrap();
    std::fs::write(schema_path(), delta_schema_string()).unwrap();
}

#[test]
fn schema_examples_validate() {
    let (root_val, _) = validator_for_def(None);
    let (op_val, _) = validator_for_def(Some("Op"));
    let (ann_val, _) = validator_for_def(Some("Annotation"));
    let (field_val, _) = validator_for_def(Some("FormField"));
    let (sig_val, _) = validator_for_def(Some("SignatureEntity"));

    let mut counts = [0usize; 6];
    for block in example_blocks() {
        match classify(&block) {
            Kind::Root => {
                counts[0] += 1;
                validate_value(&block, &root_val, "DeltaDocument");
            }
            Kind::Op => {
                counts[1] += 1;
                validate_value(&block, &op_val, "Op");
                validate_op_payload(&block, &ann_val, &sig_val);
            }
            Kind::Ops => {
                counts[2] += 1;
                for op in block.as_array().unwrap() {
                    validate_value(op, &op_val, "Op[]");
                    validate_op_payload(op, &ann_val, &sig_val);
                }
            }
            Kind::Annotation => {
                counts[3] += 1;
                validate_value(&block, &ann_val, "Annotation");
            }
            Kind::FormField => {
                counts[4] += 1;
                validate_value(&block, &field_val, "FormField");
            }
            Kind::Signature => {
                counts[5] += 1;
                validate_value(&block, &sig_val, "SignatureEntity");
            }
        }
    }
    eprintln!("classified blocks (root, op, ops, annot, field, sig): {counts:?}");
}

fn validate_op_payload(
    op: &Value,
    ann_val: &jsonschema::Validator,
    sig_val: &jsonschema::Validator,
) {
    let payload = op_payload_for_create(op);
    let Some(payload) = payload else { return };
    match op["target"].as_str() {
        Some("annotation") => validate_value(&payload, ann_val, "Op.payload annotation"),
        Some("signature") => validate_value(&payload, sig_val, "Op.payload signature"),
        _ => {}
    }
}

/// For `create` ops, returns the payload with `id` injected from the op when
/// the payload omits it (the examples in §6.1/§10 do; the engine fills it in
/// at apply time). Returns `None` for update/delete payloads (merge patches).
fn op_payload_for_create(op: &Value) -> Option<Value> {
    if op["type"].as_str() != Some("create") {
        return None;
    }
    let mut payload = op["payload"].clone();
    if let Some(obj) = payload.as_object_mut() {
        if !obj.contains_key("id") {
            if let Some(id) = op.get("id") {
                obj.insert("id".into(), id.clone());
            }
        }
    }
    Some(payload)
}

#[test]
fn schema_examples_roundtrip() {
    for block in example_blocks() {
        match classify(&block) {
            Kind::Root => roundtrip_typed::<DeltaDocument>(&block, "DeltaDocument"),
            Kind::Op => {
                roundtrip_typed::<Op>(&block, "Op");
                roundtrip_op_payload(&block);
            }
            Kind::Ops => {
                for op in block.as_array().unwrap() {
                    roundtrip_typed::<Op>(op, "Op[]");
                    roundtrip_op_payload(op);
                }
            }
            Kind::Annotation => roundtrip_typed::<Annotation>(&block, "Annotation"),
            Kind::FormField => roundtrip_typed::<FormField>(&block, "FormField"),
            Kind::Signature => roundtrip_typed::<SignatureEntity>(&block, "SignatureEntity"),
        }
    }
}

fn roundtrip_op_payload(op: &Value) {
    let Some(payload) = op_payload_for_create(op) else {
        return;
    };
    match op["target"].as_str() {
        Some("annotation") => roundtrip_typed::<Annotation>(&payload, "payload annotation"),
        Some("formField") => roundtrip_typed::<FormField>(&payload, "payload formField"),
        Some("signature") => roundtrip_typed::<SignatureEntity>(&payload, "payload signature"),
        _ => {}
    }
}
