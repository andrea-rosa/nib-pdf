//! Property tests for the undo/redo laws of JSON_SCHEMA §8.
//!
//! Laws 1–2 are checked on an engine that undoes/redoes every applied step.
//! Because coalescing merges a step into the previous undo unit, the expected
//! "before" snapshot is tracked per undo unit (a stack parallel to the undo
//! stack): when an apply starts a new step the pre-apply canonical model is
//! pushed; when it coalesces the earlier snapshot stands.
//!
//! Laws 3–4 run on a second engine that applies the same steps forward-only.
//! Law 4's strict undo-depth equality therefore compares engines that contain
//! only forward ops — replaying a Full export containing undo/redo inverses
//! cannot reproduce depth, because inverse ops become new steps on replay.

use chrono::{TimeZone, Utc};
use nib_core::delta::{Engine, ExportMode, ImportOptions};
use nib_core::model::{DeltaDocument, Op, OpKind, Target};
use proptest::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

const PAGE_COUNT: u32 = 3;
const POOL: usize = 6;

fn entity_id(n: usize) -> String {
    Uuid::from_u128(0x1000 + n as u128).to_string()
}

#[derive(Debug, Clone)]
enum GenOp {
    Create {
        subtype: u8,
        page: u32,
        rect: [f64; 4],
    },
    Update {
        pool: usize,
        patch: u8,
        rect: [f64; 4],
        num: f64,
        text: u32,
    },
    Delete {
        pool: usize,
    },
}

/// Ops applied in a single `apply` call; `grouped` gives them a shared
/// explicit `groupId`.
#[derive(Debug, Clone)]
struct Step {
    ops: Vec<GenOp>,
    grouped: bool,
    gap_ms: i64,
}

fn arb_rect() -> impl Strategy<Value = [f64; 4]> {
    (0.0f64..400.0, 0.0f64..400.0).prop_flat_map(|(x0, y0)| {
        (Just(x0), Just(y0), x0 + 1.0..500.0, y0 + 1.0..500.0).prop_map(|(a, b, c, d)| [a, b, c, d])
    })
}

fn arb_genop() -> impl Strategy<Value = GenOp> {
    prop_oneof![
        (0u8..6, 0u32..PAGE_COUNT, arb_rect()).prop_map(|(subtype, page, rect)| GenOp::Create {
            subtype,
            page,
            rect
        }),
        (0usize..POOL, 0u8..4, arb_rect(), 0.0f64..1.0, 0u32..1000).prop_map(
            |(pool, patch, rect, num, text)| GenOp::Update {
                pool,
                patch,
                rect,
                num,
                text
            }
        ),
        (0usize..POOL).prop_map(|pool| GenOp::Delete { pool }),
    ]
}

fn arb_steps() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        (
            prop::collection::vec(arb_genop(), 1..=3),
            prop::bool::weighted(0.15),
            0i64..2000,
        )
            .prop_map(|(ops, grouped, gap_ms)| Step {
                ops,
                grouped,
                gap_ms,
            }),
        1..20,
    )
}

fn annot_payload(subtype: u8, page: u32, rect: [f64; 4]) -> Value {
    match subtype {
        0 => json!({"subtype": "Ink", "page": page, "strokeWidth": 2.0,
                    "strokes": [{"points": [[rect[0], rect[1]], [rect[2], rect[3]]]}]}),
        1 => json!({"subtype": "Highlight", "page": page,
                    "quadPoints": [[rect[0], rect[3], rect[2], rect[3], rect[0], rect[1], rect[2], rect[1]]]}),
        2 => json!({"subtype": "Square", "page": page, "rect": rect, "color": "#FF0000"}),
        3 => {
            json!({"subtype": "Line", "page": page, "start": [rect[0], rect[1]], "end": [rect[2], rect[3]]})
        }
        4 => json!({"subtype": "FreeText", "page": page, "rect": rect, "contents": "hi"}),
        _ => json!({"subtype": "Text", "page": page, "rect": rect}),
    }
}

struct Ctx {
    op_counter: u128,
    ts_ms: i64,
    created: usize,
    live: Vec<String>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            op_counter: 0,
            ts_ms: 1_700_000_000_000,
            created: 0,
            live: Vec::new(),
        }
    }

    fn mk_op(
        &mut self,
        kind: OpKind,
        id: String,
        page: Option<u32>,
        payload: Value,
        group: Option<Uuid>,
    ) -> Op {
        self.op_counter += 1;
        Op {
            op_id: Uuid::from_u128(self.op_counter),
            ts: Utc.timestamp_millis_opt(self.ts_ms).unwrap(),
            author: None,
            group_id: group,
            kind,
            target: Target::Annotation,
            id,
            page,
            payload,
        }
    }

    /// Materialises a `GenOp`, or `None` when it would be invalid against the
    /// tracked live set.
    fn realise(&mut self, g: &GenOp, group: Option<Uuid>) -> Option<Op> {
        match *g {
            GenOp::Create {
                subtype,
                page,
                rect,
            } => {
                if self.created >= POOL {
                    return None;
                }
                let id = entity_id(self.created);
                self.created += 1;
                self.live.push(id.clone());
                Some(self.mk_op(
                    OpKind::Create,
                    id,
                    Some(page),
                    annot_payload(subtype, page, rect),
                    group,
                ))
            }
            GenOp::Update {
                pool,
                patch,
                rect,
                num,
                text,
            } => {
                let id = entity_id(pool);
                if !self.live.contains(&id) {
                    return None;
                }
                let payload = match patch {
                    0 => json!({"color": format!("#{:06X}", text & 0xFF_FFFF)}),
                    1 => json!({"opacity": num}),
                    2 => json!({"rect": rect}),
                    _ => json!({"contents": format!("text-{text}")}),
                };
                Some(self.mk_op(OpKind::Update, id, None, payload, group))
            }
            GenOp::Delete { pool } => {
                let id = entity_id(pool);
                if !self.live.contains(&id) {
                    return None;
                }
                self.live.retain(|x| x != &id);
                Some(self.mk_op(OpKind::Delete, id, None, json!({}), group))
            }
        }
    }
}

/// Applies one batch of ops and, with `check_undo`, asserts laws 1–2.
/// `unit_starts` mirrors the undo stack: each entry is the canonical model
/// from before its undo unit began (coalesced ops extend the previous unit).
fn apply_and_check(
    e: &mut Engine,
    ops: Vec<Op>,
    check_undo: bool,
    unit_starts: &mut Vec<Value>,
) -> Result<(), TestCaseError> {
    if ops.is_empty() {
        return Ok(());
    }
    if !check_undo {
        prop_assert!(e.apply(ops).is_ok(), "generated ops must apply");
        return Ok(());
    }
    let before = e.model().canonical();
    let depth = e.undo_depth();
    prop_assert!(e.apply(ops).is_ok(), "generated ops must apply");
    if e.undo_depth() > depth {
        unit_starts.push(before);
    }
    let after = e.model().canonical();
    let expect = unit_starts.pop().expect("non-empty undo stack after apply");
    // Law 1: apply then undo restores the model before the undo unit.
    e.undo();
    prop_assert_eq!(e.model().canonical(), expect.clone(), "law 1");
    // Law 2: redo restores the post-apply model.
    e.redo();
    prop_assert_eq!(e.model().canonical(), after, "law 2");
    unit_starts.push(expect);
    Ok(())
}

/// Materialises and applies every step. A grouped step is one apply call
/// (one undo unit); un-grouped ops are applied individually — this matches
/// how `replay_full` reconstructs steps from a Full export, where apply-call
/// boundaries are only recoverable through `groupId`.
fn apply_steps(e: &mut Engine, steps: &[Step], check_undo: bool) -> Result<(), TestCaseError> {
    let mut ctx = Ctx::new();
    let mut unit_starts: Vec<Value> = Vec::new();
    for step in steps {
        ctx.ts_ms += step.gap_ms;
        let group = step.grouped.then(Uuid::now_v7);
        let ops: Vec<Op> = step
            .ops
            .iter()
            .filter_map(|g| ctx.realise(g, group))
            .collect();
        if step.grouped {
            apply_and_check(e, ops, check_undo, &mut unit_starts)?;
        } else {
            for op in ops {
                apply_and_check(e, vec![op], check_undo, &mut unit_starts)?;
            }
        }
    }
    Ok(())
}

/// Replays a Full export: consecutive ops sharing a `groupId` apply as one
/// step, matching how they were recorded.
fn replay_full(e: &mut Engine, doc: &DeltaDocument) -> Result<(), TestCaseError> {
    let mut i = 0;
    while i < doc.ops.len() {
        let g = doc.ops[i].group_id;
        let mut j = i + 1;
        if g.is_some() {
            while j < doc.ops.len() && doc.ops[j].group_id == g {
                j += 1;
            }
        }
        prop_assert!(e.apply(doc.ops[i..j].to_vec()).is_ok(), "replay failed");
        i = j;
    }
    Ok(())
}

fn laws(steps: Vec<Step>) -> Result<(), TestCaseError> {
    let mut e_a = Engine::new("sha256:laws".into(), PAGE_COUNT);
    apply_steps(&mut e_a, &steps, true)?;

    let mut e_b = Engine::new("sha256:laws".into(), PAGE_COUNT);
    apply_steps(&mut e_b, &steps, false)?;

    // Law 3: Compacted export replays to the same model.
    let compacted = e_b.export(ExportMode::Compacted, Some("proptest".into()));
    let mut e_c = Engine::new("sha256:laws".into(), PAGE_COUNT);
    e_c.import(compacted, ImportOptions::default())
        .map_err(|e| TestCaseError::fail(format!("compacted import: {e:?}")))?;
    prop_assert_eq!(e_c.model().canonical(), e_b.model().canonical(), "law 3");

    // Law 4: Full export replayed op-by-op → same model, same undo depth.
    let full = e_b.export(ExportMode::Full, Some("proptest".into()));
    let mut e_d = Engine::new("sha256:laws".into(), PAGE_COUNT);
    replay_full(&mut e_d, &full)?;
    prop_assert_eq!(
        e_d.model().canonical(),
        e_b.model().canonical(),
        "law 4 model"
    );
    prop_assert_eq!(e_d.undo_depth(), e_b.undo_depth(), "law 4 undo depth");

    // e_a's interleaved undo/redo inverses net to zero.
    prop_assert_eq!(
        e_a.model().canonical(),
        e_b.model().canonical(),
        "undo/redo net-zero"
    );

    // e_a's Full export (which contains undo/redo inverses and fresh redo
    // opIds) replays to the same model — depth equality is not checked here
    // since inverse ops become new steps on replay.
    let full_a = e_a.export(ExportMode::Full, Some("proptest".into()));
    let mut e_e = Engine::new("sha256:laws".into(), PAGE_COUNT);
    e_e.import(
        full_a,
        ImportOptions {
            force: false,
            rebase: false,
            undoable: false,
        },
    )
    .map_err(|e| TestCaseError::fail(format!("full import (undo/redo): {e:?}")))?;
    prop_assert_eq!(
        e_e.model().canonical(),
        e_a.model().canonical(),
        "law 4 (undo/redo history)"
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn undo_redo_laws(steps in arb_steps()) {
        laws(steps)?;
    }
}
