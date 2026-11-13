//! Undo/redo bookkeeping (JSON_SCHEMA §8).

use crate::geom::Rect;
use crate::model::Op;

/// One applied op plus everything needed to undo it (§8).
#[derive(Debug, Clone)]
pub struct Command {
    /// Exactly what was applied (opId preserved).
    pub forward: Op,
    /// Derived at apply time; new opId, same groupId.
    pub inverse: Op,
    pub dirty: Vec<(u32, Rect)>,
    /// `(id, top-level payload key)` for same-key update coalescing.
    pub coalesce_key: Option<(String, String)>,
}

#[derive(Debug)]
pub struct UndoManager {
    /// Each entry is one user-visible step (a group of commands).
    pub undo: Vec<Vec<Command>>,
    pub redo: Vec<Vec<Command>>,
    /// Append-only log of every forward op and every undo/redo inverse,
    /// for Full export. Never truncated.
    pub history: Vec<Op>,
    /// Max steps kept on the undo stack; oldest groups are dropped.
    pub limit: usize,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            history: Vec::new(),
            limit: 200,
        }
    }
}

impl UndoManager {
    /// Pushes a step, enforcing the limit (history is never dropped).
    pub fn push_step(&mut self, step: Vec<Command>) {
        if step.is_empty() {
            return;
        }
        self.undo.push(step);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }
}
