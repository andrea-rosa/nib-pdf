//! RFC 7386 JSON Merge Patch plus inverse derivation and diffing.

use serde_json::{Map, Value};

/// Applies `patch` to `target` per RFC 7386 §2.
pub fn apply(target: &mut Value, patch: &Value) {
    if let Value::Object(p) = patch {
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let t = target.as_object_mut().expect("just ensured object");
        for (k, v) in p {
            if v.is_null() {
                t.remove(k);
            } else {
                apply(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *target = patch.clone();
    }
}

/// Derives a patch that undoes `patch` given the document state `before`
/// application. Every key path touched by `patch` is set to its previous
/// value, or `null` when it did not exist; arrays restore wholesale.
pub fn inverse(before: &Value, patch: &Value) -> Value {
    let Value::Object(p) = patch else {
        return before.clone();
    };
    let mut inv = Map::new();
    for (k, v) in p {
        let bv = before.get(k).cloned().unwrap_or(Value::Null);
        if v.is_object() && bv.is_object() {
            inv.insert(k.clone(), inverse(&bv, v));
        } else {
            inv.insert(k.clone(), bv);
        }
    }
    Value::Object(inv)
}

/// Computes a merge patch turning `before` into `after`: changed keys get the
/// new value (recursing when both sides are objects), keys present only in
/// `before` get `null`, and differing arrays replace wholesale.
pub fn diff(before: &Value, after: &Value) -> Value {
    match (before, after) {
        (Value::Object(b), Value::Object(a)) => {
            let mut p = Map::new();
            for (k, av) in a {
                match b.get(k) {
                    Some(bv) if bv == av => {}
                    Some(bv) => {
                        let d = diff(bv, av);
                        if !matches!(&d, Value::Object(o) if o.is_empty()) {
                            p.insert(k.clone(), d);
                        }
                    }
                    None => {
                        p.insert(k.clone(), av.clone());
                    }
                }
            }
            for k in b.keys() {
                if !a.contains_key(k) {
                    p.insert(k.clone(), Value::Null);
                }
            }
            Value::Object(p)
        }
        _ => after.clone(),
    }
}
