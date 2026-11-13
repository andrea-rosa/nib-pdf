//! Delta engine: validate / apply / undo / redo over the JSON_SCHEMA model
//! (§3, §8).

pub mod dirty;
pub mod export;
mod hit;
pub mod merge_patch;
mod undo;
pub mod validate;

pub use export::{ExportMode, ImportOptions};
pub use undo::{Command, UndoManager};
pub use validate::{ErrorCode, ValidationError};

use crate::geom::Rect;
use crate::model::{
    Annotation, FieldType, FormField, Op, OpKind, SigStatus, SignatureEntity, Target,
};
use chrono::{Duration, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[cfg(test)]
mod tests;

const COALESCE_GAP_MS: i64 = 500;
const COALESCE_KEYS: &[&str] = &["rect", "strokes", "value", "quadPoints", "start", "end"];

#[derive(Debug, thiserror::Error)]
pub enum DeltaError {
    #[error("invalid delta: {0:?}")]
    Invalid(Vec<ValidationError>),
    #[error("revision mismatch: expected {expected}, got {got}")]
    RevisionMismatch { expected: u32, got: u32 },
}

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub applied: Vec<Uuid>,
    pub skipped: Vec<Uuid>,
    pub dirty: Vec<(u32, Rect)>,
    pub rejected: Vec<ValidationError>,
}

/// The entity store. `order` is z-order / first-touch order over every
/// entity id; `signed_barrier` snapshots the ids present when a signature
/// reached `signed` status (§6.2 lock rule).
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub document_id: String,
    pub page_count: u32,
    pub revision: u32,
    pub order: Vec<String>,
    pub annotations: HashMap<String, Annotation>,
    pub form_fields: HashMap<String, FormField>,
    pub signatures: HashMap<String, SignatureEntity>,
    pub signed_barrier: HashSet<String>,
}

impl Model {
    pub fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.get(id)
    }

    /// Annotations on `page` in z-order.
    pub fn annotations_on_page(&self, page: u32) -> Vec<&Annotation> {
        self.order
            .iter()
            .filter_map(|id| self.annotations.get(id))
            .filter(|a| a.base().page == page)
            .collect()
    }

    pub fn form_field(&self, name: &str) -> Option<&FormField> {
        self.form_fields.get(name)
    }

    pub fn signature(&self, id: &str) -> Option<&SignatureEntity> {
        self.signatures.get(id)
    }

    pub fn len(&self) -> usize {
        self.annotations.len() + self.form_fields.len() + self.signatures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Deterministic JSON of all entities ordered by `order`, with
    /// `modifiedAt` stripped — for structural-equality checks.
    pub fn canonical(&self) -> Value {
        let mut entities = Map::new();
        for id in &self.order {
            let v = if let Some(a) = self.annotations.get(id) {
                Some(serde_json::to_value(a).expect("annotation serialises"))
            } else if let Some(f) = self.form_fields.get(id) {
                Some(serde_json::to_value(f).expect("form field serialises"))
            } else {
                self.signatures
                    .get(id)
                    .map(|s| serde_json::to_value(s).expect("signature serialises"))
            };
            if let Some(mut v) = v {
                strip_engine_keys(&mut v);
                entities.insert(id.clone(), v);
            }
        }
        Value::Object(entities)
    }
}

/// Removes `createdAt`/`modifiedAt` from an entity JSON object.
pub(crate) fn strip_engine_keys(v: &mut Value) {
    if let Value::Object(o) = v {
        o.remove("modifiedAt");
        o.remove("createdAt");
    }
}

fn err(
    op_index: usize,
    path: impl Into<String>,
    code: ErrorCode,
    msg: impl Into<String>,
) -> Vec<ValidationError> {
    vec![ValidationError::new(op_index, path, code, msg)]
}

fn one_err(
    op_index: usize,
    path: impl Into<String>,
    code: ErrorCode,
    msg: impl Into<String>,
) -> ValidationError {
    ValidationError::new(op_index, path, code, msg)
}

fn inverse_op(
    kind: OpKind,
    target: Target,
    id: String,
    page: Option<u32>,
    payload: Value,
    fwd: &Op,
) -> Op {
    Op {
        op_id: Uuid::now_v7(),
        ts: fwd.ts,
        author: fwd.author.clone(),
        group_id: fwd.group_id,
        kind,
        target,
        id,
        page,
        payload,
    }
}

/// Top-level payload key of a single-key update, if it is a coalescible one.
fn coalesce_key(op: &Op) -> Option<(String, String)> {
    let obj = op.payload.as_object()?;
    if obj.len() == 1 {
        let (k, _) = obj.iter().next().expect("one key");
        if COALESCE_KEYS.contains(&k.as_str()) {
            return Some((op.id.clone(), k.clone()));
        }
    }
    None
}

fn valid_rect(r: &Rect) -> bool {
    r.x0.is_finite()
        && r.y0.is_finite()
        && r.x1.is_finite()
        && r.y1.is_finite()
        && r.x0 < r.x1
        && r.y0 < r.y1
}

/// Per-annotation geometric checks shared by create and update.
fn check_annotation(a: &Annotation, i: usize, page_count: u32) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let base = a.base();
    if base.page >= page_count {
        errors.push(one_err(
            i,
            "payload.page",
            ErrorCode::PageOutOfRange,
            format!("page {} out of range (pageCount {page_count})", base.page),
        ));
    }
    if let Some(r) = base.rect {
        if !valid_rect(&r) {
            errors.push(one_err(
                i,
                "payload.rect",
                ErrorCode::RectInvalid,
                "rect is degenerate or non-finite",
            ));
        }
    }
    if let Some(p) = &base.popup {
        if let Some(r) = p.rect {
            if !valid_rect(&r) {
                errors.push(one_err(
                    i,
                    "payload.popup.rect",
                    ErrorCode::RectInvalid,
                    "popup rect is degenerate or non-finite",
                ));
            }
        }
    }
    match a {
        Annotation::Ink(ink) => {
            for (si, s) in ink.strokes.iter().enumerate() {
                if s.points.is_empty() {
                    errors.push(one_err(
                        i,
                        format!("payload.strokes[{si}]"),
                        ErrorCode::Schema,
                        "stroke has no points",
                    ));
                    continue;
                }
                let arity = s.points[0].len();
                if !(2..=4).contains(&arity)
                    || s.points.iter().any(|p| p.len() != arity)
                    || (arity >= 3 && s.points.iter().any(|p| !(0.0..=1.0).contains(&p[2])))
                {
                    errors.push(one_err(
                        i,
                        format!("payload.strokes[{si}]"),
                        ErrorCode::Schema,
                        "stroke points must share arity 2..=4 with pressure in [0,1]",
                    ));
                }
            }
        }
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => {
            for (qi, q) in m.quad_points.iter().enumerate() {
                let b = q.bbox();
                if !(b.x0 < b.x1 && b.y0 < b.y1) {
                    errors.push(one_err(
                        i,
                        format!("payload.quadPoints[{qi}]"),
                        ErrorCode::RectInvalid,
                        "degenerate quad",
                    ));
                }
            }
        }
        _ => {}
    }
    errors
}

fn signature_locked(model: &Model, id: &str) -> bool {
    model
        .signatures
        .values()
        .any(|s| s.status == SigStatus::Signed)
        && model.signed_barrier.contains(id)
}

fn snapshot_barrier(model: &mut Model) {
    model.signed_barrier = model.order.iter().cloned().collect();
}

/// Validates and applies one op to `model`. Returns `Ok(None)` for an
/// idempotent-skip create, `Ok(Some(command))` on application.
pub(crate) fn apply_op(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    match (op.kind, op.target) {
        (OpKind::Create, Target::Annotation) => create_annotation(model, i, op),
        (OpKind::Update, Target::Annotation) => update_annotation(model, i, op),
        (OpKind::Delete, Target::Annotation) => delete_annotation(model, i, op),
        (OpKind::Create, Target::FormField) => create_form_field(model, i, op),
        (OpKind::Update, Target::FormField) => update_form_field(model, i, op),
        (OpKind::Delete, Target::FormField) => delete_form_field(model, i, op),
        (OpKind::Create, Target::Signature) => create_signature(model, i, op),
        (OpKind::Update, Target::Signature) => update_signature(model, i, op),
        (OpKind::Delete, Target::Signature) => delete_signature(model, i, op),
    }
}

// ------------------------------------------------------------ annotations

fn create_annotation(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    let Some(page) = op.page else {
        return Err(err(
            i,
            "page",
            ErrorCode::Schema,
            "page required for create",
        ));
    };
    let mut payload = op.payload.clone();
    let Some(obj) = payload.as_object_mut() else {
        return Err(err(
            i,
            "payload",
            ErrorCode::Schema,
            "annotation payload must be an object",
        ));
    };
    obj.entry("id")
        .or_insert_with(|| Value::String(op.id.clone()));
    obj.entry("page").or_insert_with(|| Value::from(page));
    let mut ann: Annotation = serde_json::from_value(payload)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if ann.base().page != page {
        return Err(err(
            i,
            "payload.page",
            ErrorCode::Schema,
            "payload.page must equal op.page",
        ));
    }
    let mut errors = check_annotation(&ann, i, model.page_count);
    if ann.base().rect.is_none() && errors.is_empty() {
        match dirty::bbox(&ann) {
            Some(r) => ann.base_mut().rect = Some(r),
            None => errors.push(one_err(
                i,
                "payload.rect",
                ErrorCode::Schema,
                "cannot determine rect",
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let base = ann.base_mut();
    if base.created_at.is_none() {
        base.created_at = Some(op.ts);
    }
    base.modified_at = Some(op.ts);
    if let Some(existing) = model.annotations.get(&op.id) {
        let mut a = serde_json::to_value(existing).expect("serialises");
        let mut b = serde_json::to_value(&ann).expect("serialises");
        strip_engine_keys(&mut a);
        strip_engine_keys(&mut b);
        return if a == b {
            Ok(None) // idempotent replay
        } else {
            Err(err(
                i,
                "id",
                ErrorCode::DuplicateId,
                format!("annotation {} exists", op.id),
            ))
        };
    }
    let after = dirty::bbox(&ann);
    model.order.push(op.id.clone());
    model.annotations.insert(op.id.clone(), ann);
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Delete,
            Target::Annotation,
            op.id.clone(),
            Some(page),
            serde_json::json!({}),
            op,
        ),
        dirty: after.into_iter().map(|r| (page, r)).collect(),
        coalesce_key: None,
    }))
}

fn update_annotation(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    if !model.annotations.contains_key(&op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown annotation {}", op.id),
        ));
    }
    if signature_locked(model, &op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            format!("annotation {} is signature-locked", op.id),
        ));
    }
    let existing = model.annotations.get(&op.id).expect("checked");
    let before_val = serde_json::to_value(existing).expect("serialises");
    let before_bbox = dirty::bbox(existing);
    let page = existing.base().page;
    let mut patched = before_val.clone();
    merge_patch::apply(&mut patched, &op.payload);
    let mut ann: Annotation = serde_json::from_value(patched)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if ann.base().id.to_string() != op.id {
        return Err(err(
            i,
            "payload.id",
            ErrorCode::Schema,
            "update may not change id",
        ));
    }
    let errors = check_annotation(&ann, i, model.page_count);
    if ann.base().rect.is_none() && errors.is_empty() {
        ann.base_mut().rect = dirty::bbox(&ann);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    ann.base_mut().modified_at = Some(op.ts);
    let after_page = ann.base().page;
    let after_bbox = dirty::bbox(&ann);
    let mut inverse_payload = merge_patch::inverse(&before_val, &op.payload);
    if let Value::Object(o) = &mut inverse_payload {
        o.remove("modifiedAt");
    }
    model.annotations.insert(op.id.clone(), ann);
    let dirty = match (before_bbox, after_bbox) {
        (Some(b), Some(a)) if page == after_page => vec![(page, b.union(&a))],
        (b, a) => b
            .into_iter()
            .map(|r| (page, r))
            .chain(a.into_iter().map(|r| (after_page, r)))
            .collect(),
    };
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Update,
            Target::Annotation,
            op.id.clone(),
            Some(after_page),
            inverse_payload,
            op,
        ),
        dirty,
        coalesce_key: coalesce_key(op),
    }))
}

fn delete_annotation(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    if !model.annotations.contains_key(&op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown annotation {}", op.id),
        ));
    }
    if signature_locked(model, &op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            format!("annotation {} is signature-locked", op.id),
        ));
    }
    let ann = model.annotations.remove(&op.id).expect("checked");
    model.order.retain(|x| x != &op.id);
    let before = dirty::bbox(&ann);
    let page = ann.base().page;
    let snapshot = serde_json::to_value(&ann).expect("serialises");
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Create,
            Target::Annotation,
            op.id.clone(),
            Some(page),
            snapshot,
            op,
        ),
        dirty: before.into_iter().map(|r| (page, r)).collect(),
        coalesce_key: None,
    }))
}

// ------------------------------------------------------------ form fields

fn create_form_field(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    let mut payload = op.payload.clone();
    let Some(obj) = payload.as_object_mut() else {
        return Err(err(
            i,
            "payload",
            ErrorCode::Schema,
            "formField payload must be an object",
        ));
    };
    obj.entry("name")
        .or_insert_with(|| Value::String(op.id.clone()));
    let field: FormField = serde_json::from_value(payload)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if field.name != op.id {
        return Err(err(
            i,
            "payload.name",
            ErrorCode::Schema,
            "payload.name must equal op.id",
        ));
    }
    if let Some(existing) = model.form_fields.get(&op.id) {
        let a = serde_json::to_value(existing).expect("serialises");
        let b = serde_json::to_value(&field).expect("serialises");
        return if a == b {
            Ok(None)
        } else {
            Err(err(
                i,
                "id",
                ErrorCode::DuplicateId,
                format!("form field {} exists", op.id),
            ))
        };
    }
    let dirty: Vec<(u32, Rect)> = field.widgets.iter().map(|w| (w.page, w.rect)).collect();
    model.order.push(op.id.clone());
    model.form_fields.insert(op.id.clone(), field);
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Delete,
            Target::FormField,
            op.id.clone(),
            op.page,
            serde_json::json!({}),
            op,
        ),
        dirty: dirty::merge(dirty),
        coalesce_key: None,
    }))
}

fn check_field_value(field: &FormField, i: usize) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    match field.field_type {
        FieldType::Text => {
            if let (Some(ml), Some(Value::String(s))) = (field.max_len, &field.value) {
                if s.chars().count() > ml as usize {
                    errors.push(one_err(
                        i,
                        "payload.value",
                        ErrorCode::MaxlenExceeded,
                        "value exceeds maxLen",
                    ));
                }
            }
        }
        FieldType::Checkbox | FieldType::Radio => {
            if let Some(v) = &field.value {
                let Some(s) = v.as_str() else {
                    errors.push(one_err(
                        i,
                        "payload.value",
                        ErrorCode::Schema,
                        "value must be a string",
                    ));
                    return errors;
                };
                let ok = s == "Off"
                    || field
                        .export_values
                        .as_ref()
                        .is_some_and(|ev| ev.iter().any(|e| e == s));
                if !ok {
                    errors.push(one_err(
                        i,
                        "payload.value",
                        ErrorCode::ValueNotInOptions,
                        "value is not an export value",
                    ));
                }
            }
        }
        FieldType::Combo => {
            if let Some(v) = &field.value {
                let Some(s) = v.as_str() else {
                    errors.push(one_err(
                        i,
                        "payload.value",
                        ErrorCode::Schema,
                        "value must be a string",
                    ));
                    return errors;
                };
                if !field.flags.editable {
                    let ok = field
                        .options
                        .as_ref()
                        .is_some_and(|opts| opts.iter().any(|o| o.export == s));
                    if !ok {
                        errors.push(one_err(
                            i,
                            "payload.value",
                            ErrorCode::ValueNotInOptions,
                            "value is not an option export",
                        ));
                    }
                }
            }
        }
        FieldType::List => {
            if let Some(v) = &field.value {
                let vals: Vec<&str> = match v {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(arr) => arr.iter().filter_map(|x| x.as_str()).collect(),
                    _ => {
                        errors.push(one_err(
                            i,
                            "payload.value",
                            ErrorCode::Schema,
                            "value must be a string or string array",
                        ));
                        return errors;
                    }
                };
                if let Some(opts) = &field.options {
                    if vals.iter().any(|s| !opts.iter().any(|o| o.export == *s)) {
                        errors.push(one_err(
                            i,
                            "payload.value",
                            ErrorCode::ValueNotInOptions,
                            "value is not an option export",
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    errors
}

fn update_form_field(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    if !model.form_fields.contains_key(&op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown form field {}", op.id),
        ));
    }
    if signature_locked(model, &op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            format!("form field {} is signature-locked", op.id),
        ));
    }
    let existing = model.form_fields.get(&op.id).expect("checked");
    if existing.flags.read_only
        && op
            .payload
            .as_object()
            .is_some_and(|o| o.contains_key("value"))
    {
        return Err(err(
            i,
            "payload.value",
            ErrorCode::FieldReadonly,
            "field is read-only",
        ));
    }
    let before_val = serde_json::to_value(existing).expect("serialises");
    let before_dirty: Vec<(u32, Rect)> =
        existing.widgets.iter().map(|w| (w.page, w.rect)).collect();
    let mut patched = before_val.clone();
    merge_patch::apply(&mut patched, &op.payload);
    let field: FormField = serde_json::from_value(patched)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if field.name != op.id {
        return Err(err(
            i,
            "payload.name",
            ErrorCode::Schema,
            "update may not change name",
        ));
    }
    let errors = check_field_value(&field, i);
    if !errors.is_empty() {
        return Err(errors);
    }
    let after_dirty: Vec<(u32, Rect)> = field.widgets.iter().map(|w| (w.page, w.rect)).collect();
    let inverse_payload = merge_patch::inverse(&before_val, &op.payload);
    let page = op.page.or_else(|| field.widgets.first().map(|w| w.page));
    model.form_fields.insert(op.id.clone(), field);
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Update,
            Target::FormField,
            op.id.clone(),
            page,
            inverse_payload,
            op,
        ),
        dirty: dirty::merge(before_dirty.into_iter().chain(after_dirty).collect()),
        coalesce_key: coalesce_key(op),
    }))
}

fn delete_form_field(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    if !model.form_fields.contains_key(&op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown form field {}", op.id),
        ));
    }
    if signature_locked(model, &op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            format!("form field {} is signature-locked", op.id),
        ));
    }
    let field = model.form_fields.remove(&op.id).expect("checked");
    model.order.retain(|x| x != &op.id);
    let dirty: Vec<(u32, Rect)> = field.widgets.iter().map(|w| (w.page, w.rect)).collect();
    let snapshot = serde_json::to_value(&field).expect("serialises");
    let page = op.page.or_else(|| field.widgets.first().map(|w| w.page));
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Create,
            Target::FormField,
            op.id.clone(),
            page,
            snapshot,
            op,
        ),
        dirty: dirty::merge(dirty),
        coalesce_key: None,
    }))
}

// -------------------------------------------------------------- signatures

fn create_signature(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    let mut payload = op.payload.clone();
    let Some(obj) = payload.as_object_mut() else {
        return Err(err(
            i,
            "payload",
            ErrorCode::Schema,
            "signature payload must be an object",
        ));
    };
    obj.entry("fieldName")
        .or_insert_with(|| Value::String(op.id.clone()));
    let sig: SignatureEntity = serde_json::from_value(payload)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if let Some(w) = &sig.widget {
        if w.page >= model.page_count {
            return Err(err(
                i,
                "payload.widget.page",
                ErrorCode::PageOutOfRange,
                "widget page out of range",
            ));
        }
    }
    if model.signatures.contains_key(&op.id) {
        return Err(err(
            i,
            "id",
            ErrorCode::DuplicateId,
            format!("signature {} exists", op.id),
        ));
    }
    let dirty: Vec<(u32, Rect)> = sig
        .widget
        .as_ref()
        .filter(|w| valid_rect(&w.rect))
        .map(|w| vec![(w.page, w.rect)])
        .unwrap_or_default();
    let page = sig.widget.as_ref().map(|w| w.page);
    let becomes_signed = sig.status == SigStatus::Signed;
    model.order.push(op.id.clone());
    model.signatures.insert(op.id.clone(), sig);
    if becomes_signed {
        snapshot_barrier(model);
    }
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Delete,
            Target::Signature,
            op.id.clone(),
            page.or(op.page),
            serde_json::json!({}),
            op,
        ),
        dirty: dirty::merge(dirty),
        coalesce_key: None,
    }))
}

fn update_signature(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    let Some(existing) = model.signatures.get(&op.id) else {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown signature {}", op.id),
        ));
    };
    if existing.status != SigStatus::Prepared {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            "signature is no longer prepared",
        ));
    }
    let before_val = serde_json::to_value(existing).expect("serialises");
    let mut patched = before_val.clone();
    merge_patch::apply(&mut patched, &op.payload);
    let sig: SignatureEntity = serde_json::from_value(patched)
        .map_err(|e| err(i, "payload", ErrorCode::Schema, e.to_string()))?;
    if let Some(w) = &sig.widget {
        if w.page >= model.page_count {
            return Err(err(
                i,
                "payload.widget.page",
                ErrorCode::PageOutOfRange,
                "widget page out of range",
            ));
        }
    }
    let before_dirty: Vec<(u32, Rect)> = existing
        .widget
        .as_ref()
        .filter(|w| valid_rect(&w.rect))
        .map(|w| vec![(w.page, w.rect)])
        .unwrap_or_default();
    let after_dirty: Vec<(u32, Rect)> = sig
        .widget
        .as_ref()
        .filter(|w| valid_rect(&w.rect))
        .map(|w| vec![(w.page, w.rect)])
        .unwrap_or_default();
    let inverse_payload = merge_patch::inverse(&before_val, &op.payload);
    let page = sig.widget.as_ref().map(|w| w.page);
    let becomes_signed = sig.status == SigStatus::Signed;
    model.signatures.insert(op.id.clone(), sig);
    if becomes_signed {
        snapshot_barrier(model);
    }
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Update,
            Target::Signature,
            op.id.clone(),
            page.or(op.page),
            inverse_payload,
            op,
        ),
        dirty: dirty::merge(before_dirty.into_iter().chain(after_dirty).collect()),
        coalesce_key: coalesce_key(op),
    }))
}

fn delete_signature(
    model: &mut Model,
    i: usize,
    op: &Op,
) -> Result<Option<Command>, Vec<ValidationError>> {
    let Some(existing) = model.signatures.get(&op.id) else {
        return Err(err(
            i,
            "id",
            ErrorCode::UnknownId,
            format!("unknown signature {}", op.id),
        ));
    };
    if existing.status != SigStatus::Prepared {
        return Err(err(
            i,
            "id",
            ErrorCode::SignatureLocked,
            "cannot delete a signature that is not prepared",
        ));
    }
    let sig = model.signatures.remove(&op.id).expect("checked");
    model.order.retain(|x| x != &op.id);
    let dirty: Vec<(u32, Rect)> = sig
        .widget
        .as_ref()
        .filter(|w| valid_rect(&w.rect))
        .map(|w| vec![(w.page, w.rect)])
        .unwrap_or_default();
    let page = sig.widget.as_ref().map(|w| w.page);
    let snapshot = serde_json::to_value(&sig).expect("serialises");
    Ok(Some(Command {
        forward: op.clone(),
        inverse: inverse_op(
            OpKind::Create,
            Target::Signature,
            op.id.clone(),
            page.or(op.page),
            snapshot,
            op,
        ),
        dirty: dirty::merge(dirty),
        coalesce_key: None,
    }))
}

// ------------------------------------------------------------------ engine

pub struct Engine {
    pub(crate) model: Model,
    /// The base model (as constructed); Compact export diffs against it.
    pub(crate) base: Model,
    pub(crate) undo: UndoManager,
    pub(crate) seen_op_ids: HashSet<Uuid>,
    open_group: Option<Uuid>,
    pending_group: Vec<Command>,
}

impl Engine {
    pub fn new(document_id: String, page_count: u32) -> Self {
        let model = Model {
            document_id,
            page_count,
            revision: 0,
            order: Vec::new(),
            annotations: HashMap::new(),
            form_fields: HashMap::new(),
            signatures: HashMap::new(),
            signed_barrier: HashSet::new(),
        };
        Self {
            base: model.clone(),
            model,
            undo: UndoManager::default(),
            seen_op_ids: HashSet::new(),
            open_group: None,
            pending_group: Vec::new(),
        }
    }

    /// `"sha256:" + hex(SHA-256(bytes))` document id (§2).
    pub fn document_id_for(bytes: &[u8]) -> String {
        format!(
            "sha256:{}",
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undo.redo.is_empty()
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.undo.len()
    }

    /// Opens an explicit group; ops applied until `end_group` undo as one
    /// step and get `group_id` defaulted to the returned id.
    pub fn begin_group(&mut self) -> Uuid {
        let g = Uuid::now_v7();
        self.open_group = Some(g);
        g
    }

    pub fn end_group(&mut self) {
        self.open_group = None;
        let step = std::mem::take(&mut self.pending_group);
        if !step.is_empty() {
            self.undo.push_step(step);
            self.undo.redo.clear();
        }
    }

    /// Applies a batch of ops atomically: all ops in one call are one undo
    /// step; on the first error the live model is left untouched.
    pub fn apply(&mut self, mut ops: Vec<Op>) -> Result<ApplyReport, DeltaError> {
        if let Some(g) = self.open_group {
            for op in ops.iter_mut() {
                if op.group_id.is_none() {
                    op.group_id = Some(g);
                }
            }
        }
        let mut work = self.model.clone();
        let mut cmds = Vec::new();
        let mut report = ApplyReport::default();
        let mut errors = Vec::new();
        for (i, op) in ops.iter().enumerate() {
            if self.seen_op_ids.contains(&op.op_id) {
                report.skipped.push(op.op_id);
                continue;
            }
            match apply_op(&mut work, i, op) {
                Ok(Some(cmd)) => {
                    report.applied.push(op.op_id);
                    report.dirty.extend(cmd.dirty.iter().copied());
                    cmds.push(cmd);
                }
                Ok(None) => report.skipped.push(op.op_id),
                Err(e) => errors.extend(e),
            }
        }
        if !errors.is_empty() {
            return Err(DeltaError::Invalid(errors));
        }
        self.model = work;
        for op in &ops {
            self.seen_op_ids.insert(op.op_id);
        }
        if self.open_group.is_some() {
            self.pending_group.extend(cmds);
        } else {
            self.push_step_coalescing(cmds);
        }
        self.undo.history.extend(ops);
        report.dirty = dirty::merge(report.dirty);
        Ok(report)
    }

    /// Pushes commands as one undo step, coalescing single-key updates
    /// (same id, same key, gap < 500 ms) into the previous step.
    fn push_step_coalescing(&mut self, cmds: Vec<Command>) {
        if cmds.is_empty() {
            return;
        }
        if cmds.len() == 1 {
            if let Some(key) = &cmds[0].coalesce_key {
                if let Some(prev) = self.undo.undo.last_mut() {
                    if let Some(last) = prev.last_mut() {
                        let same = last.coalesce_key.as_ref() == Some(key);
                        let gap = cmds[0].forward.ts.signed_duration_since(last.forward.ts)
                            < Duration::milliseconds(COALESCE_GAP_MS);
                        if same && gap {
                            merge_patch::apply(&mut last.forward.payload, &cmds[0].forward.payload);
                            last.forward.ts = cmds[0].forward.ts;
                            last.dirty = dirty::merge(
                                last.dirty
                                    .iter()
                                    .copied()
                                    .chain(cmds[0].dirty.iter().copied())
                                    .collect(),
                            );
                            self.undo.redo.clear();
                            return;
                        }
                    }
                }
            }
        }
        self.undo.push_step(cmds);
        self.undo.redo.clear();
    }

    /// Reverts the last undo step, returning the inverse ops applied.
    pub fn undo(&mut self) -> Option<Vec<Op>> {
        let step = self.undo.undo.pop()?;
        let mut work = self.model.clone();
        let mut applied = Vec::new();
        let mut ok = true;
        for cmd in step.iter().rev() {
            match apply_op(&mut work, usize::MAX, &cmd.inverse) {
                Ok(_) => applied.push(cmd.inverse.clone()),
                Err(e) => {
                    debug_assert!(false, "inverse failed to apply: {e:?}");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            self.model = work;
            for op in &applied {
                self.seen_op_ids.insert(op.op_id);
            }
            self.undo.redo.push(step);
            self.undo.history.extend(applied.iter().cloned());
            Some(applied)
        } else {
            self.undo.undo.push(step);
            None
        }
    }

    /// Re-applies the last undone step, returning the forward ops applied.
    pub fn redo(&mut self) -> Option<Vec<Op>> {
        let step = self.undo.redo.pop()?;
        let mut work = self.model.clone();
        let mut applied = Vec::new();
        let mut ok = true;
        for cmd in step.iter() {
            match apply_op(&mut work, usize::MAX, &cmd.forward) {
                Ok(_) => applied.push(cmd.forward.clone()),
                Err(e) => {
                    debug_assert!(false, "forward failed to re-apply: {e:?}");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            self.model = work;
            // History and the return value get clones with fresh opIds: the
            // forward opIds are already in `seen_op_ids`, so a Full-export
            // replay would otherwise skip them and lose the redo.
            let recorded: Vec<Op> = applied
                .iter()
                .map(|op| Op {
                    op_id: Uuid::now_v7(),
                    ..op.clone()
                })
                .collect();
            for op in &recorded {
                self.seen_op_ids.insert(op.op_id);
            }
            self.undo.undo.push(step);
            self.undo.history.extend(recorded.iter().cloned());
            Some(recorded)
        } else {
            self.undo.redo.push(step);
            None
        }
    }
}

// ------------------------------------------------------------- M3 writer hooks

impl Engine {
    /// Engine whose `base` AND `model` are seeded with pre-existing
    /// annotations (imported by `writer::Writer::open`). `order` = the given
    /// order; nothing lands in undo history, so `flush_ops` starts empty.
    pub fn with_base(
        document_id: String,
        page_count: u32,
        annotations: Vec<Annotation>,
    ) -> Self {
        let mut e = Self::new(document_id, page_count);
        for a in annotations {
            let id = a.base().id.to_string();
            e.model.order.push(id.clone());
            e.model.annotations.insert(id, a);
        }
        e.base = e.model.clone();
        e
    }

    /// Compacted annotation ops the writer should apply on `flush()`.
    pub fn flush_ops(&self) -> Vec<Op> {
        self.compacted_ops(Utc::now(), &mut || Uuid::now_v7())
    }

    /// Called after a successful flush: `base` adopts the flushed model.
    pub fn mark_flushed(&mut self, revision: u32) {
        self.model.revision = revision;
        self.base = self.model.clone();
    }

    /// Annotation ids whose base and model states differ (created, deleted,
    /// or content differs ignoring `modifiedAt`/`createdAt`).
    pub fn uncommitted_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for id in self.model.order.iter().chain(self.base.order.iter()) {
            if out.contains(id) {
                continue;
            }
            let live = self.model.annotations.get(id);
            let base = self.base.annotations.get(id);
            let differ = match (base, live) {
                (None, Some(_)) | (Some(_), None) => true,
                (Some(b), Some(a)) => {
                    let mut bv = serde_json::to_value(b).expect("serialises");
                    let mut av = serde_json::to_value(a).expect("serialises");
                    strip_engine_keys(&mut bv);
                    strip_engine_keys(&mut av);
                    bv != av
                }
                (None, None) => false,
            };
            if differ {
                out.push(id.clone());
            }
        }
        out
    }

    pub fn revision(&self) -> u32 {
        self.model.revision
    }
}
