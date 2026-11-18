//! Flatten (M4): burns widget `/AP` streams into page contents and removes
//! the AcroForm, for both incremental and full-rewrite saves.

use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream};

use super::{ApplyState, Writer, WriterError};

impl Writer {
    /// Flatten all widget annots into page contents (incremental save).
    pub(super) fn flatten_inc(
        &mut self,
        inc: &mut IncrementalDocument,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        st.flattened = true;
        for (pi, page_id) in self.page_ids.clone().into_iter().enumerate() {
            flatten_page_inc(inc, page_id, pi as u32, st)?;
        }
        // Remove /AcroForm from the catalog.
        if let Some(root) = inc
            .get_prev_documents()
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok())
        {
            inc.opt_clone_object_to_new_document(root)?;
            if let Ok(cat) = inc
                .new_document
                .get_object_mut(root)
                .and_then(|o| o.as_dict_mut())
            {
                cat.remove(b"AcroForm");
            }
        }
        Ok(())
    }

    /// Flatten all widget annots into page contents (full rewrite).
    pub(super) fn flatten_full(
        &mut self,
        doc: &mut Document,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        st.flattened = true;
        for (pi, page_id) in self.page_ids.clone().into_iter().enumerate() {
            flatten_page_full(doc, page_id, pi as u32, st)?;
        }
        let root = doc
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok());
        if let Some(root) = root {
            if let Ok(cat) = doc.get_object_mut(root).and_then(|o| o.as_dict_mut()) {
                cat.remove(b"AcroForm");
            }
        }
        Ok(())
    }
}

/// One flattened widget: its `/Rect` and the `/AP /N` stream to draw.
struct WidgetDraw {
    rect: [f64; 4],
    stream: ObjectId,
    matrix: [f64; 6],
    bbox: [f64; 4],
}

fn inc_get(inc: &IncrementalDocument, id: ObjectId) -> Option<&Object> {
    inc.new_document
        .get_object(id)
        .ok()
        .or_else(|| inc.get_prev_documents().get_object(id).ok())
}

/// `/Annots` entries of `page_id` as (annot-ref, dict) pairs (read view).
fn page_annots<F>(get: F, page_id: ObjectId) -> Vec<(ObjectId, Dictionary)>
where
    F: Fn(ObjectId) -> Option<Object>,
{
    let mut out = Vec::new();
    let Some(page_obj) = get(page_id) else {
        return out;
    };
    let Ok(page) = page_obj.as_dict() else {
        return out;
    };
    let arr: Vec<Object> = match page.get(b"Annots").ok() {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => match get(*r) {
            Some(Object::Array(a)) => a,
            _ => return out,
        },
        _ => return out,
    };
    for o in arr {
        let Some(r) = o.as_reference().ok() else {
            continue;
        };
        if let Some(d) = get(r).and_then(|o| o.as_dict().ok().cloned()) {
            out.push((r, d));
        }
    }
    out
}

/// Resolve a widget's drawable state: `/AP /N` stream id + `/Matrix` +
/// `/BBox`. `get` resolves any object id to its current Object.
fn widget_draw(get: &dyn Fn(ObjectId) -> Option<Object>, annot: &Dictionary) -> Option<WidgetDraw> {
    // /F Hidden (bit 2) widgets are not flattened.
    if annot
        .get(b"F")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0)
        & 2
        != 0
    {
        return None;
    }
    let rect = annot.get(b"Rect").ok()?.as_array().ok().and_then(|a| {
        let v: Vec<f64> = a.iter().filter_map(f64_of).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    })?;
    let ap = annot.get(b"AP").ok().and_then(|o| match o {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(r) => get(*r).and_then(|o| o.as_dict().ok().cloned()),
        _ => None,
    })?;
    let n = ap.get(b"N").ok()?;
    let stream_ref = match n {
        Object::Reference(r) => Some(*r),
        Object::Dictionary(nd) => {
            // Pick the /AS state; fall back to the only entry.
            let as_name = annot
                .get(b"AS")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|s| s.to_vec());
            let pick = |key: &[u8]| -> Option<ObjectId> {
                match nd.get(key).ok() {
                    Some(Object::Reference(r)) => Some(*r),
                    _ => None,
                }
            };
            as_name.and_then(|k| pick(&k)).or_else(|| {
                (nd.len() == 1)
                    .then(|| nd.iter().next())
                    .flatten()
                    .and_then(|(_, v)| v.as_reference().ok())
            })
        }
        _ => None,
    }?;
    let sd = get(stream_ref).and_then(|o| o.as_stream().ok().map(|s| s.dict.clone()))?;
    let matrix = array6(sd.get(b"Matrix").ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let bbox = array4(sd.get(b"BBox").ok())?;
    Some(WidgetDraw {
        rect,
        stream: stream_ref,
        matrix,
        bbox,
    })
}

fn array4(o: Option<&Object>) -> Option<[f64; 4]> {
    let v: Vec<f64> = o?.as_array().ok()?.iter().filter_map(f64_of).collect();
    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
}

fn array6(o: Option<&Object>) -> Option<[f64; 6]> {
    let v: Vec<f64> = o?.as_array().ok()?.iter().filter_map(f64_of).collect();
    (v.len() == 6).then(|| [v[0], v[1], v[2], v[3], v[4], v[5]])
}

/// ISO 32000-1 §12.5.5: `/Matrix` maps form space onto the annotation rect;
/// the /BBox (transformed by /Matrix) is scaled+translated to fit /Rect.
/// Returns the `cm` operands to emit before `Do`.
fn appearance_cm(w: &WidgetDraw) -> [f64; 6] {
    // Transform /BBox corners by /Matrix, take the bounding box.
    let [a, b, c, d, e, f] = w.matrix;
    let pts = [
        (w.bbox[0], w.bbox[1]),
        (w.bbox[2], w.bbox[1]),
        (w.bbox[0], w.bbox[3]),
        (w.bbox[2], w.bbox[3]),
    ];
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in pts {
        let px = a * x + c * y + e;
        let py = b * x + d * y + f;
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
    }
    let bw = (max_x - min_x).max(f64::EPSILON);
    let bh = (max_y - min_y).max(f64::EPSILON);
    let rw = (w.rect[2] - w.rect[0]).max(0.0);
    let rh = (w.rect[3] - w.rect[1]).max(0.0);
    let sx = if bw > 0.0 { rw / bw } else { 1.0 };
    let sy = if bh > 0.0 { rh / bh } else { 1.0 };
    // cm = T · S — the form's own /Matrix is applied by the Do machinery.
    [
        sx,
        0.0,
        0.0,
        sy,
        w.rect[0] - min_x * sx,
        w.rect[1] - min_y * sy,
    ]
}

fn num(x: f64) -> String {
    crate::ap::content::fmt_num(x)
}

fn f64_of(o: &Object) -> Option<f64> {
    o.as_float()
        .ok()
        .map(|x| x as f64)
        .or_else(|| o.as_i64().ok().map(|x| x as f64))
}

fn flatten_page_inc(
    inc: &mut IncrementalDocument,
    page_id: ObjectId,
    _page: u32,
    st: &mut ApplyState,
) -> Result<(), WriterError> {
    // Read view: new doc if cloned, else prev docs.
    let get = |id: ObjectId| inc_get(inc, id).cloned();
    let annots = page_annots(get, page_id);
    let get2 = |id: ObjectId| inc_get(inc, id).cloned();
    // Every /Widget annot is removed from /Annots; only those with a
    // resolvable /AP /N stream are drawn.
    let widget_refs: Vec<(ObjectId, &Dictionary)> = annots
        .iter()
        .filter(|(_, d)| {
            d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Widget".as_slice())
        })
        .map(|(r, d)| (*r, d))
        .collect();
    let widgets: Vec<(ObjectId, WidgetDraw)> = widget_refs
        .iter()
        .filter_map(|(r, d)| widget_draw(&get2, d).map(|w| (*r, w)))
        .collect();
    if widget_refs.is_empty() {
        return Ok(());
    }
    inc.opt_clone_object_to_new_document(page_id)?;

    // Streams: leading `q` + trailing `Q` + per-widget `q cm Do Q`.
    let q_id = inc.new_document.add_object(Object::Stream(Stream::new(
        Dictionary::new(),
        b"q\n".to_vec(),
    )));
    let mut tail = String::from("Q\n");
    for (i, (_, w)) in widgets.iter().enumerate() {
        let m = appearance_cm(w);
        tail.push_str(&format!(
            "q\n{} 0 0 {} {} {} cm\n/NibW{} Do\nQ\n",
            num(m[0]),
            num(m[3]),
            num(m[4]),
            num(m[5]),
            i
        ));
    }
    let tail_id = inc.new_document.add_object(Object::Stream(Stream::new(
        Dictionary::new(),
        tail.into_bytes(),
    )));

    // Register the XObjects in page /Resources.
    for (i, (_, w)) in widgets.iter().enumerate() {
        inc.add_xobject(page_id, format!("NibW{i}").into_bytes(), w.stream)?;
    }

    // Rewrite /Contents and /Annots on the cloned page.
    let page = inc
        .new_document
        .get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())?;
    let mut contents = vec![Object::Reference(q_id)];
    match page.get(b"Contents").ok().cloned() {
        Some(Object::Array(a)) => contents.extend(a),
        Some(o @ Object::Reference(_)) => contents.push(o),
        Some(other) => {
            st.warnings.push(
                "flatten: page /Contents is not a stream reference; keeping it inline".into(),
            );
            contents.push(other);
        }
        None => {}
    }
    contents.push(Object::Reference(tail_id));
    page.set("Contents", Object::Array(contents));
    let drop: Vec<Object> = widget_refs
        .iter()
        .map(|(r, _)| Object::Reference(*r))
        .collect();
    match page.get_mut(b"Annots") {
        Ok(Object::Array(a)) => a.retain(|o| !drop.contains(o)),
        Ok(Object::Reference(_)) | Err(_) => {
            // Re-set the /Annots array (referenced arrays were read above).
            let kept: Vec<Object> = annots
                .iter()
                .map(|(r, _)| Object::Reference(*r))
                .filter(|o| !drop.contains(o))
                .collect();
            page.set("Annots", Object::Array(kept));
        }
        _ => {}
    }
    Ok(())
}

/// Inherited `/Resources` of a page, walking the `/Parent` chain.
fn inherited_resources_full(doc: &Document, page: &Dictionary) -> Option<Dictionary> {
    let mut node = Some(page.clone());
    for _ in 0..32 {
        let n = node?;
        if let Ok(res) = n.get(b"Resources") {
            return match res {
                Object::Dictionary(d) => Some(d.clone()),
                Object::Reference(r) => doc
                    .get_object(*r)
                    .ok()
                    .and_then(|o| o.as_dict().ok().cloned()),
                _ => None,
            };
        }
        node = n
            .get(b"Parent")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .and_then(|r| doc.get_object(r).ok())
            .and_then(|o| o.as_dict().ok().cloned());
    }
    None
}

/// Register `xid` as `/Resources /XObject /<name>` on `page_id`, cloning the
/// resources and XObject sub-dicts as direct dicts on the page.
fn add_xobject_full(
    doc: &mut Document,
    page_id: ObjectId,
    name: &[u8],
    xid: ObjectId,
) -> Result<(), WriterError> {
    let (res, xo) = {
        let page = doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .map_err(|e| WriterError::Parse(e.to_string()))?;
        let res: Dictionary = match page.get(b"Resources").ok() {
            Some(Object::Dictionary(d)) => d.clone(),
            Some(Object::Reference(r)) => doc
                .get_object(*r)
                .and_then(|o| o.as_dict())
                .map_err(|e| WriterError::Parse(e.to_string()))?
                .clone(),
            _ => inherited_resources_full(doc, page).unwrap_or_default(),
        };
        let xo: Dictionary = match res.get(b"XObject").ok() {
            Some(Object::Dictionary(d)) => d.clone(),
            Some(Object::Reference(r)) => doc
                .get_object(*r)
                .and_then(|o| o.as_dict())
                .map_err(|e| WriterError::Parse(e.to_string()))?
                .clone(),
            _ => Dictionary::new(),
        };
        (res, xo)
    };
    let mut res = res;
    let mut xo = xo;
    xo.set(name.to_vec(), Object::Reference(xid));
    res.set("XObject", Object::Dictionary(xo));
    doc.get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(|e| WriterError::Parse(e.to_string()))?
        .set("Resources", Object::Dictionary(res));
    Ok(())
}

fn flatten_page_full(
    doc: &mut Document,
    page_id: ObjectId,
    _page: u32,
    st: &mut ApplyState,
) -> Result<(), WriterError> {
    let get = |id: ObjectId| doc.get_object(id).ok().cloned();
    let annots = page_annots(get, page_id);
    let get2 = |id: ObjectId| doc.get_object(id).ok().cloned();
    // Every /Widget annot is removed from /Annots; only those with a
    // resolvable /AP /N stream are drawn.
    let widget_refs: Vec<(ObjectId, &Dictionary)> = annots
        .iter()
        .filter(|(_, d)| {
            d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Widget".as_slice())
        })
        .map(|(r, d)| (*r, d))
        .collect();
    let widgets: Vec<(ObjectId, WidgetDraw)> = widget_refs
        .iter()
        .filter_map(|(r, d)| widget_draw(&get2, d).map(|w| (*r, w)))
        .collect();
    if widget_refs.is_empty() {
        return Ok(());
    }

    let q_id = doc.add_object(Object::Stream(Stream::new(
        Dictionary::new(),
        b"q\n".to_vec(),
    )));
    let mut tail = String::from("Q\n");
    for (i, (_, w)) in widgets.iter().enumerate() {
        let m = appearance_cm(w);
        tail.push_str(&format!(
            "q\n{} 0 0 {} {} {} cm\n/NibW{} Do\nQ\n",
            num(m[0]),
            num(m[3]),
            num(m[4]),
            num(m[5]),
            i
        ));
    }
    let tail_id = doc.add_object(Object::Stream(Stream::new(
        Dictionary::new(),
        tail.into_bytes(),
    )));
    for (i, (_, w)) in widgets.iter().enumerate() {
        add_xobject_full(doc, page_id, format!("NibW{i}").as_bytes(), w.stream)?;
    }
    let page = doc
        .get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(|e| WriterError::Parse(e.to_string()))?;
    let mut contents = vec![Object::Reference(q_id)];
    match page.get(b"Contents").ok().cloned() {
        Some(Object::Array(a)) => contents.extend(a),
        Some(o @ Object::Reference(_)) => contents.push(o),
        Some(other) => {
            st.warnings.push(
                "flatten: page /Contents is not a stream reference; keeping it inline".into(),
            );
            contents.push(other);
        }
        None => {}
    }
    contents.push(Object::Reference(tail_id));
    page.set("Contents", Object::Array(contents));
    let drop: Vec<Object> = widget_refs
        .iter()
        .map(|(r, _)| Object::Reference(*r))
        .collect();
    match page.get_mut(b"Annots") {
        Ok(Object::Array(a)) => a.retain(|o| !drop.contains(o)),
        _ => {
            let kept: Vec<Object> = annots
                .iter()
                .map(|(r, _)| Object::Reference(*r))
                .filter(|o| !drop.contains(o))
                .collect();
            page.set("Annots", Object::Array(kept));
        }
    }
    Ok(())
}
