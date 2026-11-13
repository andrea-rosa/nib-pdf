//! PDF writer: persists the annotation model into the document via lopdf
//! incremental saves (or a full rewrite), generating `/AP` appearance streams
//! through `crate::ap` and tracking `/NM` identity + foreign `/AP`s across
//! revisions.

pub mod dict;
pub mod import;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use lopdf::{dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream};
use uuid::Uuid;

use crate::ap::{self, ApOutput, AssetSource, FontSource};
use crate::delta::dirty;
use crate::delta::Model;
use crate::geom::Rect;
use crate::model::{Annotation, Op, OpKind};

/// Fixed namespace for deterministic `/NM`-less ids:
/// `Uuid::new_v5(&NIB_NS, "{obj}:{gen}")`. Arbitrary but fixed — it only needs
/// to be stable across opens of the same file.
pub const NIB_NS: Uuid = Uuid::from_u128(0x6e69_625f_7772_6974_6572_5f6e_7331);

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("encrypted")]
    Encrypted,
    #[error("parse: {0}")]
    Parse(String),
    #[error("document has signatures; full rewrite refused")]
    SignedDocument,
    #[error("io: {0}")]
    Io(String),
    #[error("ap: {0}")]
    Ap(#[from] crate::ap::ApError),
    #[error("unknown annotation {0}")]
    UnknownId(Uuid),
    #[error("page {0} out of range")]
    PageOutOfRange(u32),
}

impl From<std::io::Error> for WriterError {
    fn from(e: std::io::Error) -> Self {
        WriterError::Io(e.to_string())
    }
}

impl From<lopdf::Error> for WriterError {
    fn from(e: lopdf::Error) -> Self {
        WriterError::Parse(e.to_string())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlushMode {
    Incremental,
    FullRewrite,
}

#[derive(Debug, Clone)]
pub struct FlushReport {
    pub revision: u32,
    pub bytes_written: u64,
    pub mode: FlushMode,
    pub annots_written: u32,
    pub warnings: Vec<String>,
    pub dirty: Vec<(u32, Rect)>,
}

#[derive(Debug)]
pub struct FlushOutput {
    pub bytes: Arc<[u8]>,
    pub report: FlushReport,
}

/// State for one opened document. `doc` is the parsed current revision;
/// `ids`/`foreign_ap`/`popups` map model ids to PDF objects.
pub struct Writer {
    bytes: Arc<[u8]>,
    doc: Option<Document>,
    page_ids: Vec<ObjectId>,
    ids: HashMap<Uuid, ObjectId>,
    foreign_ap: HashMap<Uuid, Object>,
    popups: HashMap<Uuid, ObjectId>,
    revision: u32,
}

impl Writer {
    /// Parse the document and import pre-existing annotations.
    pub fn open(bytes: Vec<u8>) -> Result<(Writer, Vec<Annotation>), WriterError> {
        let doc = Document::load_mem(&bytes).map_err(|e| WriterError::Parse(e.to_string()))?;
        if doc.is_encrypted() {
            return Err(WriterError::Encrypted);
        }
        let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        let (annots, ids, foreign_ap, popups) = import::import(&doc, &page_ids);
        Ok((
            Writer {
                bytes: Arc::from(bytes.into_boxed_slice()),
                doc: Some(doc),
                page_ids,
                ids,
                foreign_ap,
                popups,
                revision: 0,
            },
            annots,
        ))
    }

    pub fn bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn page_count(&self) -> u32 {
        self.page_ids.len() as u32
    }

    /// True when any signature object (`/Type /Sig` with `/ByteRange`, or a
    /// `/FT /Sig` field whose `/V` holds `/ByteRange`) exists.
    pub fn has_signatures(&self) -> bool {
        let Some(doc) = &self.doc else {
            return false;
        };
        Self::signatures_in(doc)
    }

    fn signatures_in(doc: &Document) -> bool {
        doc.objects.values().any(|o| {
            let Some(d) = o.as_dict().ok() else {
                return false;
            };
            let is_sig_dict = d.get(b"Type").ok().and_then(|o| o.as_name().ok())
                == Some(b"Sig".as_slice())
                && d.has(b"ByteRange");
            let is_signed_field = d.get(b"FT").ok().and_then(|o| o.as_name().ok())
                == Some(b"Sig".as_slice())
                && d.get(b"V")
                    .ok()
                    .and_then(|v| v.as_dict().ok())
                    .map(|v| v.has(b"ByteRange"))
                    .unwrap_or(false);
            is_sig_dict || is_signed_field
        })
    }

    /// Apply annotation ops and save. Returns the new document bytes plus a
    /// report; the writer reparses the output before adopting it.
    pub fn flush(
        &mut self,
        ops: &[Op],
        model: &Model,
        mode: FlushMode,
        assets: &dyn AssetSource,
        fonts: &dyn FontSource,
    ) -> Result<FlushOutput, WriterError> {
        let prev_doc = self
            .doc
            .take()
            .ok_or_else(|| WriterError::Parse("writer has no document".into()))?;
        let now = Utc::now();
        let mut st = ApplyState {
            warnings: Vec::new(),
            dirty: Vec::new(),
            annots_written: 0,
            assets,
            fonts,
            now,
            last_bbox: None,
        };

        let new_bytes: Vec<u8> = match mode {
            FlushMode::Incremental => {
                let mut inc =
                    IncrementalDocument::create_from(self.bytes.to_vec(), prev_doc.clone());
                for op in ops {
                    if op.target != crate::model::Target::Annotation {
                        continue;
                    }
                    st.last_bbox = None;
                    self.apply_op_inc(&mut inc, op, model, &mut st)?;
                }
                let mut out = Vec::new();
                inc.save_to(&mut out)?;
                out
            }
            FlushMode::FullRewrite => {
                if Self::signatures_in(&prev_doc) {
                    self.doc = Some(prev_doc);
                    return Err(WriterError::SignedDocument);
                }
                let mut doc = prev_doc.clone();
                for op in ops {
                    if op.target != crate::model::Target::Annotation {
                        continue;
                    }
                    st.last_bbox = None;
                    self.apply_op_full(&mut doc, &prev_doc, op, model, &mut st)?;
                }
                doc.prune_objects();
                doc.renumber_objects();
                let mut out = Vec::new();
                let modern = doc.save_with_options(
                    &mut out,
                    lopdf::SaveOptions::builder()
                        .use_object_streams(true)
                        .use_xref_streams(true)
                        .build(),
                );
                match modern {
                    Ok(()) => match Document::load_mem(&out) {
                        Ok(_) => out,
                        Err(e) => {
                            st.warnings.push(format!(
                                "full rewrite with object streams failed to reload ({e}); fell back to classic xref"
                            ));
                            let mut out2 = Vec::new();
                            doc.save_to(&mut out2)?;
                            out2
                        }
                    },
                    Err(e) => {
                        st.warnings.push(format!(
                            "full rewrite with object streams failed ({e}); fell back to classic xref"
                        ));
                        let mut out2 = Vec::new();
                        doc.save_to(&mut out2)?;
                        out2
                    }
                }
            }
        };

        // Reparse the saved bytes before adopting them.
        let reparsed = match Document::load_mem(&new_bytes) {
            Ok(d) => d,
            Err(e) => {
                self.doc = Some(Document::load_mem(&self.bytes).map_err(|e2| {
                    WriterError::Parse(format!("{e}; also failed to restore: {e2}"))
                })?);
                return Err(WriterError::Parse(format!("saved output unparseable: {e}")));
            }
        };

        // Object ids move after renumber_objects in FullRewrite; rebuild the
        // id maps from the reparsed document.
        if mode == FlushMode::FullRewrite {
            let (_, ids, foreign_ap, popups) = import::import(&reparsed, &self.page_ids);
            self.ids = ids;
            self.foreign_ap = foreign_ap;
            self.popups = popups;
        }

        let bytes_written = new_bytes.len() as u64;
        self.bytes = Arc::from(new_bytes.into_boxed_slice());
        self.doc = Some(reparsed);
        self.revision += 1;
        Ok(FlushOutput {
            bytes: self.bytes.clone(),
            report: FlushReport {
                revision: self.revision,
                bytes_written,
                mode,
                annots_written: st.annots_written,
                warnings: st.warnings,
                dirty: st.dirty,
            },
        })
    }

    // ---------------------------------------------------------- op plumbing

    fn page_id(&self, page: u32) -> Result<ObjectId, WriterError> {
        self.page_ids
            .get(page as usize)
            .copied()
            .ok_or(WriterError::PageOutOfRange(page))
    }

    fn apply_op_inc(
        &mut self,
        inc: &mut IncrementalDocument,
        op: &Op,
        model: &Model,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        let id = Uuid::parse_str(&op.id)
            .map_err(|_| WriterError::Parse(format!("op id {} is not a UUID", op.id)))?;
        match op.kind {
            OpKind::Create => {
                let annot: Annotation =
                    serde_json::from_value(op.payload.clone()).map_err(|e| {
                        WriterError::Parse(format!("create payload for {}: {e}", op.id))
                    })?;
                let new_id = self.create_annot(&mut inc.new_document, &annot, st)?;
                self.annots_add_inc(inc, self.page_id(annot.base().page)?, new_id)?;
                if let Some(pop) = self.maybe_make_popup(&mut inc.new_document, &annot, new_id)? {
                    let page = self.page_id(annot.base().page)?;
                    self.annots_add_inc(inc, page, pop)?;
                    self.popups.insert(id, pop);
                }
                self.ids.insert(id, new_id);
                st.dirty.push((
                    annot.base().page,
                    st.last_bbox
                        .unwrap_or_else(|| crate::delta::dirty::bbox(&annot).unwrap_or_default()),
                ));
            }
            OpKind::Update => {
                let Some(annot) = model.annotation(&op.id).cloned() else {
                    st.warnings
                        .push(format!("update for unknown annotation {id} skipped"));
                    return Ok(());
                };
                match self.ids.get(&id).copied() {
                    Some(oid) => {
                        let old_rect = inc
                            .get_prev_documents()
                            .get_object(oid)
                            .ok()
                            .and_then(|o| o.as_dict().ok())
                            .and_then(|d| dict::rect_of(d.get(b"Rect").ok()));
                        inc.opt_clone_object_to_new_document(oid)?;
                        self.patch_annot_obj(&mut inc.new_document, oid, &annot, st)?;
                        let mut d = st.last_bbox;
                        if let Some(r) = old_rect {
                            d = Some(match d {
                                Some(x) => x.union(&r),
                                None => r,
                            });
                        }
                        st.dirty.push((
                            annot.base().page,
                            d.unwrap_or_else(|| dirty::bbox(&annot).unwrap_or_default()),
                        ));
                    }
                    None => {
                        // Unknown to the file — treat as create.
                        let new_id = self.create_annot(&mut inc.new_document, &annot, st)?;
                        self.annots_add_inc(inc, self.page_id(annot.base().page)?, new_id)?;
                        self.ids.insert(id, new_id);
                        st.dirty.push((
                            annot.base().page,
                            st.last_bbox
                                .unwrap_or_else(|| dirty::bbox(&annot).unwrap_or_default()),
                        ));
                    }
                }
            }
            OpKind::Delete => {
                let Some(&oid) = self.ids.get(&id) else {
                    st.warnings
                        .push(format!("delete for unknown annotation {id} skipped"));
                    return Ok(());
                };
                let old = inc
                    .get_prev_documents()
                    .get_object(oid)
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .cloned();
                let page = op.page.unwrap_or(0);
                self.annots_remove_inc(inc, oid)?;
                if let Some(pop) = self.popups.get(&id).copied() {
                    self.annots_remove_inc(inc, pop)?;
                }
                self.ids.remove(&id);
                let r = old
                    .as_ref()
                    .and_then(|d| dict::rect_of(d.get(b"Rect").ok()))
                    .unwrap_or_else(empty_rect);
                st.dirty.push((page, r));
            }
        }
        Ok(())
    }

    fn apply_op_full(
        &mut self,
        doc: &mut Document,
        prev: &Document,
        op: &Op,
        model: &Model,
        st: &mut ApplyState,
    ) -> Result<(), WriterError> {
        let id = Uuid::parse_str(&op.id)
            .map_err(|_| WriterError::Parse(format!("op id {} is not a UUID", op.id)))?;
        match op.kind {
            OpKind::Create => {
                let annot: Annotation =
                    serde_json::from_value(op.payload.clone()).map_err(|e| {
                        WriterError::Parse(format!("create payload for {}: {e}", op.id))
                    })?;
                let new_id = self.create_annot(doc, &annot, st)?;
                self.annots_add_full(doc, self.page_id(annot.base().page)?, new_id)?;
                if let Some(pop) = self.maybe_make_popup(doc, &annot, new_id)? {
                    let page = self.page_id(annot.base().page)?;
                    self.annots_add_full(doc, page, pop)?;
                    self.popups.insert(id, pop);
                }
                self.ids.insert(id, new_id);
                st.dirty.push((
                    annot.base().page,
                    st.last_bbox
                        .unwrap_or_else(|| dirty::bbox(&annot).unwrap_or_default()),
                ));
            }
            OpKind::Update => {
                let Some(annot) = model.annotation(&op.id).cloned() else {
                    st.warnings
                        .push(format!("update for unknown annotation {id} skipped"));
                    return Ok(());
                };
                match self.ids.get(&id).copied() {
                    Some(oid) => {
                        let old_rect = prev
                            .get_object(oid)
                            .ok()
                            .and_then(|o| o.as_dict().ok())
                            .and_then(|d| dict::rect_of(d.get(b"Rect").ok()));
                        self.patch_annot_obj(doc, oid, &annot, st)?;
                        let mut d = st.last_bbox;
                        if let Some(r) = old_rect {
                            d = Some(match d {
                                Some(x) => x.union(&r),
                                None => r,
                            });
                        }
                        st.dirty.push((
                            annot.base().page,
                            d.unwrap_or_else(|| dirty::bbox(&annot).unwrap_or_default()),
                        ));
                    }
                    None => {
                        let new_id = self.create_annot(doc, &annot, st)?;
                        self.annots_add_full(doc, self.page_id(annot.base().page)?, new_id)?;
                        self.ids.insert(id, new_id);
                        st.dirty.push((
                            annot.base().page,
                            st.last_bbox
                                .unwrap_or_else(|| dirty::bbox(&annot).unwrap_or_default()),
                        ));
                    }
                }
            }
            OpKind::Delete => {
                let Some(&oid) = self.ids.get(&id) else {
                    st.warnings
                        .push(format!("delete for unknown annotation {id} skipped"));
                    return Ok(());
                };
                let old = prev
                    .get_object(oid)
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .cloned();
                let page = op.page.unwrap_or(0);
                self.annots_remove_full(doc, oid)?;
                if let Some(pop) = self.popups.get(&id).copied() {
                    self.annots_remove_full(doc, pop)?;
                }
                self.ids.remove(&id);
                let r = old
                    .as_ref()
                    .and_then(|d| dict::rect_of(d.get(b"Rect").ok()))
                    .unwrap_or_else(empty_rect);
                st.dirty.push((page, r));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------- object builders

    /// Create the `/AP` form XObject (plus its resource objects) and the
    /// annotation dict in `doc`. Returns the annot ObjectId.
    fn create_annot(
        &mut self,
        doc: &mut Document,
        annot: &Annotation,
        st: &mut ApplyState,
    ) -> Result<ObjectId, WriterError> {
        let (ap_obj, bbox) = self.ap_for(doc, annot, st)?;
        st.last_bbox = Some(bbox);
        let mut d = Dictionary::new();
        dict::fill_annot_dict(&mut d, annot, bbox, &self.ids, st.now);
        if let Some(ap) = ap_obj {
            d.set("AP", ap);
        }
        st.annots_written += 1;
        Ok(doc.add_object(Object::Dictionary(d)))
    }

    /// Patch a cloned annot dict in-place per the current model. Regenerates
    /// `/AP` unless `appearance.regenerate == false` (the cloned dict keeps
    /// its existing `/AP` value untouched in that case).
    fn patch_annot_obj(
        &mut self,
        doc: &mut Document,
        oid: ObjectId,
        annot: &Annotation,
        st: &mut ApplyState,
    ) -> Result<(Rect, u32), WriterError> {
        let regen = annot.base().appearance.regenerate;
        let (ap_obj, bbox) = if regen {
            self.ap_for(doc, annot, st)?
        } else {
            (None, dirty::bbox(annot).unwrap_or_else(empty_rect))
        };
        st.last_bbox = Some(bbox);
        let d = doc
            .get_object_mut(oid)
            .and_then(|o| o.as_dict_mut())
            .map_err(|e| WriterError::Parse(e.to_string()))?;
        dict::fill_annot_dict(d, annot, bbox, &self.ids, st.now);
        if let Some(ap) = ap_obj {
            d.set("AP", ap);
        }
        st.annots_written += 1;
        Ok((bbox, annot.base().page))
    }

    /// Build (or reuse) the `/AP` value for an annotation.
    fn ap_for(
        &mut self,
        doc: &mut Document,
        annot: &Annotation,
        st: &mut ApplyState,
    ) -> Result<(Option<Object>, Rect), WriterError> {
        let id = annot.base().id;
        if !annot.base().appearance.regenerate {
            if let Some(ap) = self.foreign_ap.get(&id) {
                let bbox = dirty::bbox(annot).unwrap_or_else(empty_rect);
                return Ok((Some(ap.clone()), bbox));
            }
        }
        let out = ap::build(annot, st.assets, st.fonts)?;
        st.warnings.extend(out.warnings.iter().cloned());
        let bbox = out.bbox.normalize();
        Ok((Some(add_ap(doc, out)?), bbox))
    }

    fn maybe_make_popup(
        &mut self,
        doc: &mut Document,
        annot: &Annotation,
        parent: ObjectId,
    ) -> Result<Option<ObjectId>, WriterError> {
        let Some(pop) = &annot.base().popup else {
            return Ok(None);
        };
        let d = popup_dict(parent, pop.rect.unwrap_or_else(empty_rect), pop.open);
        let pop_id = doc.add_object(Object::Dictionary(d));
        doc.get_object_mut(parent)
            .and_then(|o| o.as_dict_mut())?
            .set("Popup", Object::Reference(pop_id));
        Ok(Some(pop_id))
    }

    // ------------------------------------------------------ /Annots helpers

    /// Append `annot_id` to the page's `/Annots`, cloning into the new
    /// document first (page dict or referenced array).
    fn annots_add_inc(
        &mut self,
        inc: &mut IncrementalDocument,
        page_id: ObjectId,
        annot_id: ObjectId,
    ) -> Result<(), WriterError> {
        inc.opt_clone_object_to_new_document(page_id)?;
        let arr_ref = match inc
            .new_document
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .map(|d| d.get(b"Annots").ok().cloned())
        {
            Ok(Some(Object::Reference(r))) => Some(r),
            _ => None,
        };
        if let Some(arr_id) = arr_ref {
            inc.opt_clone_object_to_new_document(arr_id)?;
            let arr = inc
                .new_document
                .get_object_mut(arr_id)
                .and_then(|o| o.as_array_mut())?;
            arr.push(Object::Reference(annot_id));
        } else {
            let page = inc
                .new_document
                .get_object_mut(page_id)
                .and_then(|o| o.as_dict_mut())?;
            match page.get_mut(b"Annots") {
                Ok(Object::Array(a)) => a.push(Object::Reference(annot_id)),
                _ => page.set("Annots", vec![Object::Reference(annot_id)]),
            }
        }
        Ok(())
    }

    /// Remove `annot_id` from whichever page `/Annots` array holds it.
    fn annots_remove_inc(
        &mut self,
        inc: &mut IncrementalDocument,
        annot_id: ObjectId,
    ) -> Result<(), WriterError> {
        let target = Object::Reference(annot_id);
        for page_id in self.page_ids.clone() {
            inc.opt_clone_object_to_new_document(page_id)?;
            let arr_ref = match inc
                .new_document
                .get_object(page_id)
                .and_then(|o| o.as_dict())
                .map(|d| d.get(b"Annots").ok().cloned())
            {
                Ok(Some(Object::Reference(r))) => Some(r),
                _ => None,
            };
            if let Some(arr_id) = arr_ref {
                inc.opt_clone_object_to_new_document(arr_id)?;
                let arr = inc
                    .new_document
                    .get_object_mut(arr_id)
                    .and_then(|o| o.as_array_mut())?;
                let n = arr.len();
                arr.retain(|o| o != &target);
                if arr.len() != n {
                    return Ok(());
                }
            } else {
                let page = inc
                    .new_document
                    .get_object_mut(page_id)
                    .and_then(|o| o.as_dict_mut())?;
                if let Ok(Object::Array(a)) = page.get_mut(b"Annots") {
                    let n = a.len();
                    a.retain(|o| o != &target);
                    if a.len() != n {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    fn annots_add_full(
        &mut self,
        doc: &mut Document,
        page_id: ObjectId,
        annot_id: ObjectId,
    ) -> Result<(), WriterError> {
        let arr_ref = match doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .map(|d| d.get(b"Annots").ok().cloned())
        {
            Ok(Some(Object::Reference(r))) => Some(r),
            _ => None,
        };
        if let Some(arr_id) = arr_ref {
            let arr = doc.get_object_mut(arr_id).and_then(|o| o.as_array_mut())?;
            arr.push(Object::Reference(annot_id));
        } else {
            let page = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())?;
            match page.get_mut(b"Annots") {
                Ok(Object::Array(a)) => a.push(Object::Reference(annot_id)),
                _ => page.set("Annots", vec![Object::Reference(annot_id)]),
            }
        }
        Ok(())
    }

    fn annots_remove_full(
        &mut self,
        doc: &mut Document,
        annot_id: ObjectId,
    ) -> Result<(), WriterError> {
        let target = Object::Reference(annot_id);
        for page_id in self.page_ids.clone() {
            let arr_ref = match doc
                .get_object(page_id)
                .and_then(|o| o.as_dict())
                .map(|d| d.get(b"Annots").ok().cloned())
            {
                Ok(Some(Object::Reference(r))) => Some(r),
                _ => None,
            };
            if let Some(arr_id) = arr_ref {
                let arr = doc.get_object_mut(arr_id).and_then(|o| o.as_array_mut())?;
                let n = arr.len();
                arr.retain(|o| o != &target);
                if arr.len() != n {
                    return Ok(());
                }
            } else if let Ok(page) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                if let Ok(Object::Array(a)) = page.get_mut(b"Annots") {
                    let n = a.len();
                    a.retain(|o| o != &target);
                    if a.len() != n {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
}

/// Mutable per-flush state threaded through op application.
struct ApplyState<'a> {
    warnings: Vec<String>,
    dirty: Vec<(u32, Rect)>,
    annots_written: u32,
    assets: &'a dyn AssetSource,
    fonts: &'a dyn FontSource,
    now: DateTime<Utc>,
    last_bbox: Option<Rect>,
}

/// Build the `/AP` value (`<< /N form-ref >>`) plus all resource objects an
/// `ApOutput` needs: ExtGState dicts, font dicts, image + smask streams.
fn add_ap(doc: &mut Document, out: ApOutput) -> Result<Object, WriterError> {
    let mut resources = Dictionary::new();
    if !out.ext_gstate.is_empty() {
        let mut gs = Dictionary::new();
        for (name, d) in out.ext_gstate {
            let id = doc.add_object(Object::Dictionary(d));
            gs.set(name, Object::Reference(id));
        }
        resources.set("ExtGState", gs);
    }
    if !out.fonts.is_empty() {
        let mut f = Dictionary::new();
        for (name, d) in out.fonts {
            let id = doc.add_object(Object::Dictionary(d));
            f.set(name, Object::Reference(id));
        }
        resources.set("Font", f);
    }
    if !out.images.is_empty() {
        let mut xo = Dictionary::new();
        for (name, mut ixo) in out.images {
            if let Some(smask) = ixo.smask {
                let smid = doc.add_object(Object::Stream(smask));
                ixo.stream.dict.set("SMask", Object::Reference(smid));
            }
            let id = doc.add_object(Object::Stream(ixo.stream));
            xo.set(name, Object::Reference(id));
        }
        resources.set("XObject", xo);
    }
    let b = out.bbox.normalize();
    let mut form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![
                Object::Real(b.x0 as f32),
                Object::Real(b.y0 as f32),
                Object::Real(b.x1 as f32),
                Object::Real(b.y1 as f32),
            ],
            "Matrix" => vec![
                Object::Integer(1), Object::Integer(0), Object::Integer(0),
                Object::Integer(1), Object::Integer(0), Object::Integer(0),
            ],
            "Resources" => resources,
        },
        out.content,
    )
    .with_compression(true);
    form.compress()
        .map_err(|e| WriterError::Parse(e.to_string()))?;
    let fid = doc.add_object(Object::Stream(form));
    Ok(Object::Dictionary(dictionary! {
        "N" => Object::Reference(fid),
    }))
}

/// Atomic save: write `<path>.tmp` in the same directory, fsync, rename.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out.pdf".into())
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `/Subtype /Popup` companion annot.
fn popup_dict(parent: ObjectId, rect: Rect, open: bool) -> Dictionary {
    let r = rect.normalize();
    dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Parent" => Object::Reference(parent),
        "Rect" => vec![
            Object::Real(r.x0 as f32), Object::Real(r.y0 as f32),
            Object::Real(r.x1 as f32), Object::Real(r.y1 as f32),
        ],
        "Open" => Object::Boolean(open),
    }
}

fn empty_rect() -> Rect {
    Rect::new(0.0, 0.0, 0.0, 0.0)
}
