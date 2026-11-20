//! Export (Full / Compacted) and import (JSON_SCHEMA §8).

use super::{dirty, merge_patch, validate::ErrorCode, DeltaError, Engine, ValidationError};
use crate::model::{DeltaDocument, Op, OpKind, Target};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExportMode {
    /// The full append-only history (audit trail, crash recovery).
    Full,
    /// Per entity by first touch (compact payload for servers / PDF write).
    Compacted,
}

#[derive(Debug, Copy, Clone)]
pub struct ImportOptions {
    /// Ignore `documentId` mismatch.
    pub force: bool,
    /// Ignore `baseRevision` mismatch; ops hitting `UnknownId` are skipped.
    pub rebase: bool,
    /// Push the whole import as one undo step.
    pub undoable: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            force: false,
            rebase: false,
            undoable: true,
        }
    }
}

/// Rounds numbers: 4 decimals for `opacity` and stroke pressure
/// (index 2 of a stroke point row), 3 decimals otherwise (§11).
fn round_numbers(v: &mut Value, key: Option<&str>, in_points: bool, index: usize) {
    match v {
        Value::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().expect("floating-point JSON number");
                let decimals = if key == Some("opacity") || (in_points && index == 2) {
                    4
                } else {
                    3
                };
                let m = 10f64.powi(decimals);
                *v = Value::from((f * m).round() / m);
            }
        }
        Value::Array(arr) => {
            let inner_points = key == Some("points") || in_points;
            for (i, item) in arr.iter_mut().enumerate() {
                round_numbers(item, None, inner_points, i);
            }
        }
        Value::Object(o) => {
            for (k, item) in o.iter_mut() {
                round_numbers(item, Some(k.as_str()), false, 0);
            }
        }
        _ => {}
    }
}

impl Engine {
    /// Deterministic export used by `export` and by snapshot tests: `now`
    /// supplies envelope `createdAt` and compacted op `ts`, `id_gen` supplies
    /// fresh opIds for compacted ops.
    pub(crate) fn export_with_clock(
        &self,
        mode: ExportMode,
        generator: Option<String>,
        now: DateTime<Utc>,
        id_gen: &mut dyn FnMut() -> Uuid,
    ) -> DeltaDocument {
        let ops = match mode {
            ExportMode::Full => self.undo.history.clone(),
            ExportMode::Compacted => self.compacted_ops(now, id_gen),
        };
        DeltaDocument {
            schema_version: 1,
            document_id: self.model.document_id.clone(),
            pdf_id: None,
            base_revision: self.model.revision,
            generator,
            created_at: Some(now),
            ops,
            extra: Map::new(),
        }
    }

    pub fn export(&self, mode: ExportMode, generator: Option<String>) -> DeltaDocument {
        self.export_with_clock(mode, generator, Utc::now(), &mut Uuid::now_v7)
    }

    /// Export serialised with standard rounding, preserving bound visual geometry.
    pub fn export_json(&self, mode: ExportMode, generator: Option<String>) -> String {
        let mut doc = self.export(mode, generator);
        for op in &mut doc.ops {
            let bound_visual = op.kind == OpKind::Create
                && op.target == Target::Annotation
                && op
                    .payload
                    .get("bindToField")
                    .and_then(Value::as_str)
                    .is_some()
                && matches!(
                    op.payload.get("subtype").and_then(Value::as_str),
                    Some("Ink" | "Stamp")
                );
            if bound_visual {
                if let Value::Object(payload) = &mut op.payload {
                    for (key, value) in payload {
                        if !matches!(
                            key.as_str(),
                            "rect"
                                | "strokes"
                                | "strokeWidth"
                                | "minWidthFactor"
                                | "maxWidthFactor"
                                | "smoothing"
                                | "vector"
                        ) {
                            round_numbers(value, Some(key), false, 0);
                        }
                    }
                }
            } else {
                round_numbers(&mut op.payload, None, false, 0);
            }
        }
        serde_json::to_string(&doc).expect("DeltaDocument serialises")
    }

    /// Per entity id ordered by first touch in `history`.
    fn first_touch_order(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut order = Vec::new();
        for op in &self.undo.history {
            if seen.insert(op.id.clone()) {
                order.push(op.id.clone());
            }
        }
        order
    }

    fn last_author(&self, id: &str) -> Option<String> {
        self.undo
            .history
            .iter()
            .rev()
            .find(|o| o.id == id)
            .and_then(|o| o.author.clone())
    }

    pub(crate) fn compacted_ops(
        &self,
        now: DateTime<Utc>,
        id_gen: &mut dyn FnMut() -> Uuid,
    ) -> Vec<Op> {
        let mut ops = Vec::new();
        for id in self.first_touch_order() {
            // Signature ops are never compacted away (handled below).
            if self.model.signatures.contains_key(&id) || self.base.signatures.contains_key(&id) {
                continue;
            }
            let target = if self.model.annotations.contains_key(&id)
                || self.base.annotations.contains_key(&id)
            {
                Some(Target::Annotation)
            } else if self.model.form_fields.contains_key(&id)
                || self.base.form_fields.contains_key(&id)
            {
                Some(Target::FormField)
            } else {
                None
            };
            let Some(target) = target else { continue };
            let mut make_op = |kind: OpKind, page: Option<u32>, payload: Value| Op {
                op_id: id_gen(),
                ts: now,
                author: self.last_author(&id),
                group_id: None,
                kind,
                target,
                id: id.clone(),
                page,
                payload,
            };
            match target {
                Target::Annotation => {
                    let live = self.model.annotations.get(&id);
                    let base = self.base.annotations.get(&id);
                    match (base, live) {
                        (None, Some(a)) => {
                            let page = a.base().page;
                            ops.push(make_op(
                                OpKind::Create,
                                Some(page),
                                serde_json::to_value(a).expect("serialises"),
                            ));
                        }
                        (Some(b), Some(a)) => {
                            let mut bv = serde_json::to_value(b).expect("serialises");
                            let mut av = serde_json::to_value(a).expect("serialises");
                            super::strip_engine_keys(&mut bv);
                            super::strip_engine_keys(&mut av);
                            if bv != av {
                                let patch = merge_patch::diff(&bv, &av);
                                ops.push(make_op(OpKind::Update, Some(a.base().page), patch));
                            }
                        }
                        (Some(b), None) => {
                            ops.push(make_op(
                                OpKind::Delete,
                                Some(b.base().page),
                                serde_json::json!({}),
                            ));
                        }
                        (None, None) => {} // created & deleted: nothing
                    }
                }
                Target::FormField => {
                    let live = self.model.form_fields.get(&id);
                    let base = self.base.form_fields.get(&id);
                    match (base, live) {
                        (None, Some(f)) => {
                            let page = f.widgets.first().map(|w| w.page);
                            ops.push(make_op(
                                OpKind::Create,
                                page,
                                serde_json::to_value(f).expect("serialises"),
                            ));
                        }
                        (Some(b), Some(f)) => {
                            let bv = serde_json::to_value(b).expect("serialises");
                            let av = serde_json::to_value(f).expect("serialises");
                            if bv != av {
                                let patch = merge_patch::diff(&bv, &av);
                                let page = f.widgets.first().map(|w| w.page);
                                ops.push(make_op(OpKind::Update, page, patch));
                            }
                        }
                        (Some(b), None) => {
                            let page = b.widgets.first().map(|w| w.page);
                            ops.push(make_op(OpKind::Delete, page, serde_json::json!({})));
                        }
                        (None, None) => {}
                    }
                }
                Target::Signature => unreachable!("filtered above"),
            }
        }
        // Signature ops verbatim, in history order.
        ops.extend(
            self.undo
                .history
                .iter()
                .filter(|o| o.target == Target::Signature)
                .cloned(),
        );
        ops
    }

    /// Imports a `DeltaDocument` (§8 import semantics).
    pub fn import(
        &mut self,
        doc: DeltaDocument,
        opts: ImportOptions,
    ) -> Result<super::ApplyReport, DeltaError> {
        if doc.schema_version > 1 {
            return Err(DeltaError::Invalid(vec![ValidationError::new(
                0,
                "schemaVersion",
                ErrorCode::Schema,
                format!("unsupported schemaVersion {}", doc.schema_version),
            )]));
        }
        if doc.document_id != self.model.document_id && !opts.force {
            return Err(DeltaError::Invalid(vec![ValidationError::new(
                0,
                "documentId",
                ErrorCode::Schema,
                "documentId mismatch (import with force=true to override)",
            )]));
        }
        if doc.base_revision != self.model.revision && !opts.rebase {
            return Err(DeltaError::RevisionMismatch {
                expected: self.model.revision,
                got: doc.base_revision,
            });
        }
        let mut work = self.model.clone();
        let mut cmds = Vec::new();
        let mut report = super::ApplyReport::default();
        let mut errors = Vec::new();
        for (i, op) in doc.ops.iter().enumerate() {
            if self.seen_op_ids.contains(&op.op_id) {
                report.skipped.push(op.op_id);
                continue;
            }
            match super::apply_op(&mut work, i, op) {
                Ok(Some(cmd)) => {
                    report.applied.push(op.op_id);
                    report.dirty.extend(cmd.dirty.iter().copied());
                    cmds.push(cmd);
                }
                Ok(None) => report.skipped.push(op.op_id),
                Err(es) => {
                    let (rejected, fatal): (Vec<_>, Vec<_>) = es
                        .into_iter()
                        .partition(|e| opts.rebase && e.code == ErrorCode::UnknownId);
                    report.rejected.extend(rejected);
                    errors.extend(fatal);
                }
            }
        }
        if !errors.is_empty() {
            return Err(DeltaError::Invalid(errors));
        }
        self.model = work;
        for op in &doc.ops {
            self.seen_op_ids.insert(op.op_id);
        }
        let any = !cmds.is_empty();
        if opts.undoable {
            self.undo.push_step(cmds);
        }
        if any {
            self.undo.redo.clear();
        }
        self.undo.history.extend(doc.ops.iter().cloned());
        report.dirty = dirty::merge(report.dirty);
        Ok(report)
    }
}
