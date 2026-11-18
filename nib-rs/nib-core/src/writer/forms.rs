//! Field updates (M4): writes `/V`, `/AS` and regenerated `/AP` streams for
//! `target: "formField"` Update ops, in both incremental and full-rewrite
//! saves. Create/Delete are rejected (authoring is unsupported).

use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId};

use super::{add_form_xobject, ApplyState, Writer, WriterError};
use crate::forms::FieldRefs;
use crate::model::{FieldType, FormField, Widget};

impl Writer {
    /// `Update` op against an incremental save.
    pub(super) fn apply_field_inc(
        &mut self,
        inc: &mut IncrementalDocument,
        name: &str,
        field: &FormField,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        let Some(refs) = self.fields.get(name).cloned() else {
            st.warnings
                .push(format!("update for unknown form field {name} skipped"));
            return Ok(());
        };
        inc.opt_clone_object_to_new_document(refs.field)?;
        for (wid, _) in &refs.widgets {
            inc.opt_clone_object_to_new_document(*wid)?;
        }
        self.prepare_acroform_inc(inc)?;
        self.apply_field_doc(&mut inc.new_document, &refs, field, st)
    }

    /// `Update` op against a full-rewrite document (objects already cloned).
    pub(super) fn apply_field_full(
        &mut self,
        doc: &mut Document,
        name: &str,
        field: &FormField,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        let Some(refs) = self.fields.get(name).cloned() else {
            st.warnings
                .push(format!("update for unknown form field {name} skipped"));
            return Ok(());
        };
        self.apply_field_doc(doc, &refs, field, st)
    }

    /// Clone the `/AcroForm` object (or the catalog when it is an inline
    /// dict) into the incremental update so it can be mutated.
    fn prepare_acroform_inc(&mut self, inc: &mut IncrementalDocument) -> Result<(), WriterError> {
        if let Some(id) = self.acroform {
            inc.opt_clone_object_to_new_document(id)?;
        } else if let Some(root) = root_id(inc.get_prev_documents()) {
            inc.opt_clone_object_to_new_document(root)?;
        }
        Ok(())
    }

    /// Write `/V`, `/AS`, `/AP`, `/DA`, `/NeedAppearances` and `/DR` for one
    /// field. `doc` is the mutable target (new doc for incremental, the
    /// rewrite doc otherwise).
    fn apply_field_doc(
        &mut self,
        doc: &mut Document,
        refs: &FieldRefs,
        field: &FormField,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        // Field dict: /DA (if missing) + /V.
        {
            let fd = doc
                .get_object_mut(refs.field)
                .and_then(|o| o.as_dict_mut())
                .map_err(|e| WriterError::Parse(format!("field {}: {e}", field.name)))?;
            if !fd.has(b"DA") {
                fd.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
            }
            set_field_value(fd, field);
        }

        // Widget dicts: /AS (Btn) + regenerated /AP.
        let mut per_page: std::collections::HashMap<u32, crate::geom::Rect> =
            std::collections::HashMap::new();
        for (i, (wid, page)) in refs.widgets.iter().enumerate() {
            let wmodel = field.widgets.get(i);
            {
                let wd = doc
                    .get_object_mut(*wid)
                    .and_then(|o| o.as_dict_mut())
                    .map_err(|e| WriterError::Parse(format!("widget of {}: {e}", field.name)))?;
                if matches!(field.field_type, FieldType::Checkbox | FieldType::Radio) {
                    let state = widget_state(field, wmodel);
                    wd.set("AS", Object::Name(state.into_bytes()));
                }
            }
            if let Some(wm) = wmodel {
                let ap = crate::ap::widget::build_widget(field, wm, st.fonts)?;
                let single = ap.states.len() == 1 && ap.states[0].0.is_empty();
                let mut ap_obj = Dictionary::new();
                if single {
                    let out = ap.states.into_iter().next().unwrap().1;
                    for w in &out.warnings {
                        st.warnings.push(w.clone());
                    }
                    let fid = add_form_xobject(doc, out)?;
                    ap_obj.set("N", Object::Reference(fid));
                } else {
                    let mut ap_n = Dictionary::new();
                    for (state, out) in ap.states {
                        for w in &out.warnings {
                            st.warnings.push(w.clone());
                        }
                        let fid = add_form_xobject(doc, out)?;
                        ap_n.set(state.as_str(), Object::Reference(fid));
                    }
                    ap_obj.set("N", Object::Dictionary(ap_n));
                }
                let wd = doc
                    .get_object_mut(*wid)
                    .and_then(|o| o.as_dict_mut())
                    .map_err(|e| WriterError::Parse(format!("widget of {}: {e}", field.name)))?;
                wd.set("AP", Object::Dictionary(ap_obj));
            }
            let r = wmodel
                .map(|w| w.rect)
                .or_else(|| {
                    doc.get_object(*wid)
                        .ok()
                        .and_then(|o| o.as_dict().ok())
                        .and_then(|d| super::dict::rect_of(d.get(b"Rect").ok()))
                })
                .unwrap_or_else(super::empty_rect);
            let e = per_page.entry(*page).or_insert(r);
            if *e != r {
                *e = e.union(&r);
            }
        }
        for (page, r) in per_page {
            st.dirty.push((page, r));
        }

        self.finish_acroform_doc(doc);
        st.fields_written += 1;
        Ok(())
    }

    /// `/NeedAppearances false` + ensure `/DR /Font` has `/Helv` and `/ZaDb`.
    fn finish_acroform_doc(&mut self, doc: &mut Document) {
        // Read the current DR/Font state first, add missing font objects,
        // then mutate — the AcroForm dict borrow cannot span `add_object`.
        let (mut dr, mut fonts) = {
            let Some(af) = acroform_dict(doc, self.acroform, self.acroform_inline) else {
                return;
            };
            let dr = match af.get(b"DR").ok().cloned() {
                Some(Object::Dictionary(d)) => d,
                _ => Dictionary::new(),
            };
            let fonts = match dr.get(b"Font").ok().cloned() {
                Some(Object::Dictionary(d)) => d,
                _ => Dictionary::new(),
            };
            (dr, fonts)
        };
        if !fonts.has(b"Helv") {
            let id = doc.add_object(Object::Dictionary(
                crate::ap::fonts::Base14::Helvetica.font_dict(),
            ));
            fonts.set("Helv", Object::Reference(id));
        }
        if !fonts.has(b"ZaDb") {
            let id = doc.add_object(Object::Dictionary(crate::ap::fonts::zapf_dingbats_dict()));
            fonts.set("ZaDb", Object::Reference(id));
        }
        dr.set("Font", Object::Dictionary(fonts));
        if let Some(af) = acroform_dict_mut(doc, self.acroform, self.acroform_inline) {
            af.set("NeedAppearances", Object::Boolean(false));
            af.set("DR", Object::Dictionary(dr));
        }
    }
}

/// Catalog `/Root` object id.
fn root_id(doc: &Document) -> Option<ObjectId> {
    doc.trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok())
}

/// Read access to the `/AcroForm` dict (indirect object or inline).
fn acroform_dict(doc: &Document, acroform: Option<ObjectId>, inline: bool) -> Option<&Dictionary> {
    if let Some(id) = acroform {
        return doc.get_object(id).ok().and_then(|o| o.as_dict().ok());
    }
    if !inline {
        return None;
    }
    let root = root_id(doc)?;
    doc.get_object(root)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"AcroForm").ok())
        .and_then(|o| o.as_dict().ok())
}

/// Mutable access to the `/AcroForm` dict (indirect object or inline in the
/// catalog).
fn acroform_dict_mut(
    doc: &mut Document,
    acroform: Option<ObjectId>,
    inline: bool,
) -> Option<&mut Dictionary> {
    if let Some(id) = acroform {
        return doc
            .get_object_mut(id)
            .ok()
            .and_then(|o| o.as_dict_mut().ok());
    }
    if !inline {
        return None;
    }
    let root = root_id(doc)?;
    doc.get_object_mut(root)
        .ok()
        .and_then(|o| o.as_dict_mut().ok())
        .and_then(|d| d.get_mut(b"AcroForm").ok())
        .and_then(|o| o.as_dict_mut().ok())
}

/// `/V` for the field dict per type.
fn set_field_value(fd: &mut Dictionary, field: &FormField) {
    match field.field_type {
        FieldType::Checkbox | FieldType::Radio => {
            let on = field
                .value
                .as_ref()
                .and_then(|v| v.as_str())
                .filter(|s| *s != "Off");
            fd.set("V", Object::Name(on.unwrap_or("Off").as_bytes().to_vec()));
        }
        FieldType::List => {
            let sel: Vec<String> = match &field.value {
                Some(serde_json::Value::String(s)) => vec![s.clone()],
                Some(serde_json::Value::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => Vec::new(),
            };
            let exports: Vec<&str> = field
                .options
                .as_ref()
                .map(|o| o.iter().map(|o| o.export.as_str()).collect())
                .unwrap_or_default();
            if sel.len() <= 1 {
                match sel.first() {
                    Some(s) => {
                        fd.set("V", lopdf::text_string(s));
                    }
                    None => {
                        fd.remove(b"V");
                    }
                }
            } else {
                fd.set(
                    "V",
                    Object::Array(sel.iter().map(|s| lopdf::text_string(s)).collect()),
                );
            }
            let indices: Vec<Object> = sel
                .iter()
                .filter_map(|s| exports.iter().position(|e| e == s))
                .map(|i| Object::Integer(i as i64))
                .collect();
            if indices.is_empty() {
                fd.remove(b"I");
            } else {
                fd.set("I", Object::Array(indices));
            }
        }
        FieldType::PushButton | FieldType::Signature => {}
        _ => {
            // Tx and Ch/combo: /V is a text string (PDFDoc or UTF-16BE).
            let s = field
                .value
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            fd.set("V", lopdf::text_string(&s));
        }
    }
}

/// `/AS` for a Btn widget: the model `appearance_state`, or derived from the
/// field value vs the kid's `export_value`.
fn widget_state(field: &FormField, w: Option<&Widget>) -> String {
    if let Some(s) = w.and_then(|w| w.appearance_state.clone()) {
        return s;
    }
    let value = field.value.as_ref().and_then(|v| v.as_str());
    match (value, w.and_then(|w| w.export_value.as_deref())) {
        (Some(v), Some(e)) if v == e => e.to_string(),
        (Some(v), None) if v != "Off" => v.to_string(),
        _ => "Off".to_string(),
    }
}
