//! PDF writer: persists the annotation model into the document via lopdf
//! incremental saves (or a full rewrite), generating `/AP` appearance streams
//! through `crate::ap` and tracking `/NM` identity + foreign `/AP`s across
//! revisions.

pub mod dict;
pub mod flatten;
pub mod forms;
pub mod import;
pub mod sign;

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
use crate::forms::{self as form_import, FieldRefs};
use crate::geom::Rect;
use crate::model::{Annotation, FormField, Op, OpKind};

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
    #[error("malformed page annotations: {0}")]
    MalformedAnnots(String),
    #[error("document has signatures; full rewrite and flatten refused")]
    SignedDocument,
    #[error("io: {0}")]
    Io(String),
    #[error("ap: {0}")]
    Ap(#[from] crate::ap::ApError),
    #[error("unknown annotation {0}")]
    UnknownId(Uuid),
    #[error("page {0} out of range")]
    PageOutOfRange(u32),
    /// Ops the writer cannot persist (e.g. `target: "signature"`, which only
    /// the signing transaction may write).
    #[error("unsupported op: {0}")]
    UnsupportedOp(String),
    /// A signature is staged; full rewrite and flatten would invalidate it.
    #[error("a signature is staged; full rewrite and flatten are refused")]
    SignaturePending,
    #[error("signature target: {0}")]
    SignatureTarget(String),
    #[error("signature placeholder: {0}")]
    SignaturePlaceholder(String),
    #[error("stale signature: {0}")]
    StaleSignature(String),
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

fn validate_classic_xref(bytes: &[u8]) -> Result<(), WriterError> {
    let Some(startxref) = bytes
        .windows(b"startxref".len())
        .rposition(|window| window == b"startxref")
    else {
        let last_classic_xref = bytes
            .windows(b"xref\n".len())
            .rposition(|window| window == b"xref\n");
        let last_eof = bytes
            .windows(b"%%EOF".len())
            .rposition(|window| window == b"%%EOF");
        if last_classic_xref.is_some_and(|xref| last_eof.is_none_or(|eof| xref > eof)) {
            return Err(WriterError::Parse(
                "truncated classic cross-reference section: missing startxref".into(),
            ));
        }
        return Ok(());
    };
    let offset = std::str::from_utf8(&bytes[startxref + b"startxref".len()..])
        .ok()
        .and_then(|tail| tail.split_ascii_whitespace().next())
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| WriterError::Parse("invalid startxref offset".into()))?;
    let Some(section) = bytes.get(offset..) else {
        return Err(WriterError::Parse(
            "startxref points beyond end of file".into(),
        ));
    };
    let Some(classic_section) = bytes.get(offset..startxref) else {
        return Err(WriterError::Parse(
            "startxref does not point to an earlier cross-reference section".into(),
        ));
    };
    let is_classic_xref = section.starts_with(b"xref");
    if !is_classic_xref && !is_xref_stream(section) {
        return Err(WriterError::Parse(
            "startxref does not point to a cross-reference section".into(),
        ));
    }
    let Some(trailer) = classic_section
        .windows(b"trailer".len())
        .position(|window| window == b"trailer")
    else {
        if is_classic_xref {
            return Err(WriterError::Parse(
                "truncated classic cross-reference section: missing trailer".into(),
            ));
        }
        return Ok(());
    };
    if is_classic_xref
        && classic_section[..trailer]
            .iter()
            .any(|byte| !byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
    {
        return Err(WriterError::Parse(
            "invalid control byte in classic cross-reference section".into(),
        ));
    }
    Ok(())
}

fn is_xref_stream(section: &[u8]) -> bool {
    let mut header = section
        .split(u8::is_ascii_whitespace)
        .filter(|token| !token.is_empty());
    if !header
        .next()
        .is_some_and(|token| token.iter().all(u8::is_ascii_digit))
        || !header
            .next()
            .is_some_and(|token| token.iter().all(u8::is_ascii_digit))
        || header.next() != Some(b"obj".as_slice())
    {
        return false;
    }
    let Some(dict_start) = section.windows(2).position(|window| window == b"<<") else {
        return false;
    };
    let Some(stream) = section[dict_start..]
        .windows(b"stream".len())
        .position(|window| window == b"stream")
        .map(|index| dict_start + index)
    else {
        return false;
    };
    let dictionary = &section[dict_start..stream];
    let mut rest = dictionary;
    while let Some(index) = rest
        .windows(b"/Type".len())
        .position(|window| window == b"/Type")
    {
        let value_start = index + b"/Type".len();
        let after_key = rest.get(value_start).copied();
        if after_key.is_none_or(|byte| byte.is_ascii_whitespace() || byte == b'/') {
            let mut value = value_start;
            while rest
                .get(value)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                value += 1;
            }
            if rest.get(value) == Some(&b'/') {
                value += 1;
                let end = value
                    + rest[value..]
                        .iter()
                        .position(|byte| {
                            byte.is_ascii_whitespace()
                                || matches!(*byte, b'/' | b'<' | b'>' | b'[' | b']' | b'(' | b')')
                        })
                        .unwrap_or(rest.len() - value);
                if rest.get(value..end) == Some(b"XRef") {
                    return true;
                }
            }
        }
        rest = &rest[index + b"/Type".len()..];
    }
    false
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlushMode {
    Incremental,
    FullRewrite,
}

/// `Writer::flush` arguments: save mode plus optional form flattening
/// (widget `/AP` streams burned into page contents, `/AcroForm` removed).
#[derive(Copy, Clone, Debug)]
pub struct FlushOptions {
    pub mode: FlushMode,
    pub flatten: bool,
}

impl FlushOptions {
    pub fn incremental() -> Self {
        FlushOptions {
            mode: FlushMode::Incremental,
            flatten: false,
        }
    }

    pub fn full_rewrite() -> Self {
        FlushOptions {
            mode: FlushMode::FullRewrite,
            flatten: false,
        }
    }

    pub fn flatten(mut self, flatten: bool) -> Self {
        self.flatten = flatten;
        self
    }
}

#[derive(Debug, Clone)]
pub struct FlushReport {
    pub revision: u32,
    pub bytes_written: u64,
    pub mode: FlushMode,
    pub annots_written: u32,
    pub fields_written: u32,
    pub flattened: bool,
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
    fields: HashMap<String, FieldRefs>,
    acroform: Option<ObjectId>,
    acroform_inline: bool,
    revision: u32,
    pending_signature: Option<sign::PendingSignature>,
}

/// Page object ids in document order. Unlike `Document::get_pages`, this
/// never preallocates from the untrusted `/Pages /Count`, which lopdf reports
/// as the iterator's size hint.
fn page_ids(doc: &Document) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    for id in doc.page_iter() {
        ids.push(id);
    }
    ids
}

impl Writer {
    /// Parse the document and import pre-existing annotations and AcroForm
    /// fields.
    pub fn open(bytes: Vec<u8>) -> Result<(Writer, Vec<Annotation>, Vec<FormField>), WriterError> {
        validate_classic_xref(&bytes)?;
        let doc = Document::load_mem(&bytes).map_err(|e| WriterError::Parse(e.to_string()))?;
        if doc.is_encrypted() {
            return Err(WriterError::Encrypted);
        }
        let page_ids = page_ids(&doc);
        let (annots, ids, foreign_ap, popups) =
            import::import(&doc, &page_ids).map_err(WriterError::MalformedAnnots)?;
        let fimp = form_import::parse(&doc, &page_ids);
        Ok((
            Writer {
                bytes: Arc::from(bytes.into_boxed_slice()),
                doc: Some(doc),
                page_ids,
                ids,
                foreign_ap,
                popups,
                fields: fimp.refs,
                acroform: fimp.acroform,
                acroform_inline: fimp.acroform_inline,
                revision: 0,
                pending_signature: None,
            },
            annots,
            fimp.fields,
        ))
    }

    /// Field-name → object refs of the imported AcroForm fields.
    pub fn form_refs(&self) -> &HashMap<String, FieldRefs> {
        &self.fields
    }

    /// Re-imports AcroForm fields from the writer's current PDF revision.
    pub fn imported_fields(&self) -> Result<Vec<FormField>, WriterError> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| WriterError::Parse("writer has no document".into()))?;
        Ok(form_import::parse(doc, &self.page_ids).fields)
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
        options: FlushOptions,
        assets: &dyn AssetSource,
        fonts: &dyn FontSource,
    ) -> Result<FlushOutput, WriterError> {
        if let Some(op) = ops
            .iter()
            .find(|op| op.target == crate::model::Target::Signature)
        {
            return Err(WriterError::UnsupportedOp(format!(
                "signature op {} must go through the signing transaction",
                op.id
            )));
        }
        let rewrites = options.mode == FlushMode::FullRewrite || options.flatten;
        if rewrites && self.pending_signature.is_some() {
            return Err(WriterError::SignaturePending);
        }
        if rewrites && (self.has_signatures() || !model.signatures.is_empty()) {
            return Err(WriterError::SignedDocument);
        }
        let prev_doc = self
            .doc
            .take()
            .ok_or_else(|| WriterError::Parse("writer has no document".into()))?;
        // Rollback snapshot: if anything below fails, the writer is left
        // exactly as it was (same doc, same id maps) so it stays usable.
        let snap = (
            self.ids.clone(),
            self.foreign_ap.clone(),
            self.popups.clone(),
            self.page_ids.clone(),
            self.fields.clone(),
            self.acroform,
            self.acroform_inline,
        );
        match self.flush_inner(ops, model, options, prev_doc.clone(), assets, fonts) {
            Ok(out) => Ok(out),
            Err(e) => {
                self.doc = Some(prev_doc);
                self.ids = snap.0;
                self.foreign_ap = snap.1;
                self.popups = snap.2;
                self.page_ids = snap.3;
                self.fields = snap.4;
                self.acroform = snap.5;
                self.acroform_inline = snap.6;
                Err(e)
            }
        }
    }

    fn flush_inner(
        &mut self,
        ops: &[Op],
        model: &Model,
        options: FlushOptions,
        prev_doc: Document,
        assets: &dyn AssetSource,
        fonts: &dyn FontSource,
    ) -> Result<FlushOutput, WriterError> {
        let mode = options.mode;
        let now = Utc::now();
        let mut st = ApplyState {
            warnings: Vec::new(),
            dirty: Vec::new(),
            annots_written: 0,
            fields_written: 0,
            flattened: false,
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
                    match op.target {
                        crate::model::Target::Annotation => {
                            st.last_bbox = None;
                            self.apply_op_inc(&mut inc, op, model, &mut st)?;
                        }
                        crate::model::Target::FormField => match op.kind {
                            OpKind::Update => {
                                if let Some(field) = model.form_field(&op.id).cloned() {
                                    self.apply_field_inc(&mut inc, &op.id, &field, &mut st)?;
                                } else {
                                    st.warnings.push(format!(
                                        "update for unknown form field {} skipped",
                                        op.id
                                    ));
                                }
                            }
                            _ => st.warnings.push(
                                "formField create/delete is not written (authoring unsupported)"
                                    .into(),
                            ),
                        },
                        crate::model::Target::Signature => {
                            return Err(WriterError::UnsupportedOp(format!(
                                "signature op {}",
                                op.id
                            )))
                        }
                    }
                }
                if options.flatten {
                    self.flatten_inc(&mut inc, &mut st)?;
                }
                let mut out = Vec::new();
                inc.save_to(&mut out)?;
                out
            }
            FlushMode::FullRewrite => {
                if Self::signatures_in(&prev_doc) {
                    return Err(WriterError::SignedDocument);
                }
                let mut doc = prev_doc.clone();
                for op in ops {
                    match op.target {
                        crate::model::Target::Annotation => {
                            st.last_bbox = None;
                            self.apply_op_full(&mut doc, &prev_doc, op, model, &mut st)?;
                        }
                        crate::model::Target::FormField => match op.kind {
                            OpKind::Update => {
                                if let Some(field) = model.form_field(&op.id).cloned() {
                                    self.apply_field_full(&mut doc, &op.id, &field, &mut st)?;
                                } else {
                                    st.warnings.push(format!(
                                        "update for unknown form field {} skipped",
                                        op.id
                                    ));
                                }
                            }
                            _ => st.warnings.push(
                                "formField create/delete is not written (authoring unsupported)"
                                    .into(),
                            ),
                        },
                        crate::model::Target::Signature => {
                            return Err(WriterError::UnsupportedOp(format!(
                                "signature op {}",
                                op.id
                            )))
                        }
                    }
                }
                if options.flatten {
                    self.flatten_full(&mut doc, &mut st)?;
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
        let reparsed = Document::load_mem(&new_bytes)
            .map_err(|e| WriterError::Parse(format!("saved output unparseable: {e}")))?;

        // Object ids move after renumber_objects in FullRewrite; rebuild the
        // id maps from the reparsed document.
        if mode == FlushMode::FullRewrite {
            self.page_ids = page_ids(&reparsed);
            let (_, ids, foreign_ap, popups) =
                import::import(&reparsed, &self.page_ids).map_err(WriterError::MalformedAnnots)?;
            self.ids = ids;
            self.foreign_ap = foreign_ap;
            self.popups = popups;
            let fimp = form_import::parse(&reparsed, &self.page_ids);
            self.fields = fimp.refs;
            self.acroform = fimp.acroform;
            self.acroform_inline = fimp.acroform_inline;
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
                fields_written: st.fields_written,
                flattened: st.flattened,
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
    fields_written: u32,
    flattened: bool,
    assets: &'a dyn AssetSource,
    fonts: &'a dyn FontSource,
    now: DateTime<Utc>,
    last_bbox: Option<Rect>,
}

/// Create the form XObject stream (plus its resource objects) for an
/// `ApOutput`: ExtGState dicts, font dicts, image + smask streams. Returns
/// the stream's ObjectId.
fn add_form_xobject(doc: &mut Document, out: ApOutput) -> Result<ObjectId, WriterError> {
    let mut resources = Dictionary::new();
    if !out.ext_gstate.is_empty() {
        let mut gs = Dictionary::new();
        for (name, d) in out.ext_gstate {
            let id = doc.add_object(Object::Dictionary(d));
            gs.set(name, Object::Reference(id));
        }
        resources.set("ExtGState", gs);
    }
    if !out.fonts.is_empty() || !out.embedded_fonts.is_empty() {
        let mut f = Dictionary::new();
        for (name, d) in out.fonts {
            let id = doc.add_object(Object::Dictionary(d));
            f.set(name, Object::Reference(id));
        }
        for (name, embedded) in out.embedded_fonts {
            let id = add_embedded_font(doc, embedded)?;
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
    Ok(doc.add_object(Object::Stream(form)))
}

fn add_embedded_font(
    doc: &mut Document,
    font: crate::ap::embedded_font::EmbeddedFont,
) -> Result<ObjectId, WriterError> {
    let crate::ap::embedded_font::EmbeddedFont {
        base_font,
        program,
        font_bbox,
        ascent,
        descent,
        cap_height,
        widths,
        cid_to_gid,
        to_unicode,
    } = font;
    let name = Object::Name(base_font.as_bytes().to_vec());
    let mut font_program = Stream::new(
        dictionary! { "Length1" => Object::Integer(program.len() as i64) },
        program,
    )
    .with_compression(true);
    font_program
        .compress()
        .map_err(|e| WriterError::Parse(e.to_string()))?;
    let font_program_id = doc.add_object(Object::Stream(font_program));

    let descriptor_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => name.clone(),
        "Flags" => Object::Integer(32),
        "FontBBox" => font_bbox
            .into_iter()
            .map(|value| Object::Integer(value as i64))
            .collect::<Vec<_>>(),
        "ItalicAngle" => Object::Integer(0),
        "Ascent" => Object::Integer(ascent as i64),
        "Descent" => Object::Integer(descent as i64),
        "CapHeight" => Object::Integer(cap_height as i64),
        "StemV" => Object::Integer(80),
        "FontFile2" => Object::Reference(font_program_id),
    }));

    let mut cid_to_gid_stream = Stream::new(Dictionary::new(), cid_to_gid).with_compression(true);
    cid_to_gid_stream
        .compress()
        .map_err(|e| WriterError::Parse(e.to_string()))?;
    let cid_to_gid_id = doc.add_object(Object::Stream(cid_to_gid_stream));
    let mut to_unicode_stream = Stream::new(Dictionary::new(), to_unicode).with_compression(true);
    to_unicode_stream
        .compress()
        .map_err(|e| WriterError::Parse(e.to_string()))?;
    let to_unicode_id = doc.add_object(Object::Stream(to_unicode_stream));

    let cid_system_info = Object::Dictionary(dictionary! {
        "Registry" => Object::string_literal("Adobe"),
        "Ordering" => Object::string_literal("Identity"),
        "Supplement" => Object::Integer(0),
    });
    let descendant_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => name.clone(),
        "CIDSystemInfo" => cid_system_info,
        "FontDescriptor" => Object::Reference(descriptor_id),
        "DW" => Object::Integer(1000),
        "W" => vec![
            Object::Integer(1),
            Object::Array(
                widths
                    .into_iter()
                    .map(|width| Object::Integer(width as i64))
                    .collect(),
            ),
        ],
        "CIDToGIDMap" => Object::Reference(cid_to_gid_id),
    }));
    Ok(doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => name,
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant_id)],
        "ToUnicode" => Object::Reference(to_unicode_id),
    })))
}

/// Build the `/AP` value (`<< /N form-ref >>`) for an `ApOutput`.
fn add_ap(doc: &mut Document, out: ApOutput) -> Result<Object, WriterError> {
    let fid = add_form_xobject(doc, out)?;
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
