//! Signature staging and adoption (M5 signing foundations).
//!
//! `stage_signature` builds one incremental revision that holds only the
//! signature dictionary (with fixed-width `/ByteRange` and `/Contents`
//! placeholders), the `/V` link of the target field and the `/SigFlags`
//! update. The staged bytes are returned to the caller (`nib-sign`) and are
//! never adopted by the writer until `adopt_signed` accepts the completed
//! file. While a signature is staged, full rewrites and flattening are
//! refused because they would invalidate the prepared revision.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use lopdf::{
    dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId, StringFormat,
};

use super::{FlushMode, FlushOutput, FlushReport, Writer, WriterError};
use crate::forms as form_import;
use crate::model::{FieldType, SignatureFieldState, SubFilter};

/// Sentinel written into each non-zero `/ByteRange` slot. Ten digits give
/// every slot room for offsets up to 9,999,999,999 bytes.
pub const BYTE_RANGE_SENTINEL: i64 = 9_999_999_999;

/// Default `/Contents` placeholder: 16,384 hex characters (8,192 DER bytes),
/// excluding the `<` `>` delimiters.
pub const DEFAULT_CONTENTS_HEX_LEN: usize = 16_384;

/// Upper bound for the `/Contents` placeholder (hex characters).
pub const MAX_CONTENTS_HEX_LEN: usize = 1 << 20;

/// `/F` for a new invisible signature widget: Print (4) | Locked (128).
const INVISIBLE_WIDGET_FLAGS: i64 = 132;

static NEXT_STAGING_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Where the signature value goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigTarget {
    /// An existing, writable, unambiguous, empty signature field with
    /// exactly one widget (fully-qualified name).
    ExistingField(String),
    /// A new invisible (`/Rect [0 0 0 0]`) top-level signature field.
    NewInvisibleField { name: String, page: u32 },
}

/// Signature dictionary contents chosen by the signer before staging.
#[derive(Debug, Clone)]
pub struct SigDictSpec {
    pub target: SigTarget,
    pub sub_filter: SubFilter,
    /// `/Contents` placeholder length in hex characters, excluding the
    /// delimiters. Must be even and in `2..=MAX_CONTENTS_HEX_LEN`.
    pub contents_hex_len: usize,
    /// Untrusted claimed signing time written as `/M`.
    pub claimed_time: Option<DateTime<Utc>>,
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    pub doc_mdp: Option<u8>,
    pub lock_fields: Option<Vec<String>>,
}

/// One staged, not yet signed, incremental revision. The bytes are private;
/// `nib-sign` patches a copy and hands the result to `Writer::adopt_signed`.
#[derive(Debug, Clone)]
pub struct StagedSignature {
    token: u64,
    bytes: Vec<u8>,
    base_len: usize,
    base_revision: u32,
    sig_object: ObjectId,
    field_object: ObjectId,
    field_name: String,
}

impl StagedSignature {
    /// Staged file (earlier revisions verbatim + the signature revision).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Length of the previously flushed file; every byte before this offset
    /// is identical to the writer's current bytes.
    pub fn base_len(&self) -> usize {
        self.base_len
    }

    pub fn base_revision(&self) -> u32 {
        self.base_revision
    }

    /// Object id of the `/Type /Sig` dictionary.
    pub fn sig_object(&self) -> ObjectId {
        self.sig_object
    }

    /// Object id of the terminal signature field (merged with its widget for
    /// new fields).
    pub fn field_object(&self) -> ObjectId {
        self.field_object
    }

    pub fn field_name(&self) -> &str {
        &self.field_name
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PendingSignature {
    token: u64,
    revision: u32,
    staged_len: usize,
    sig_object: ObjectId,
}

fn target_err(msg: impl Into<String>) -> WriterError {
    WriterError::SignatureTarget(msg.into())
}

fn sub_filter_name(sf: SubFilter) -> &'static str {
    match sf {
        SubFilter::EtsiCadesDetached => "ETSI.CAdES.detached",
        SubFilter::AdbePkcs7Detached => "adbe.pkcs7.detached",
    }
}

/// PDF date string (`D:YYYYMMDDHHmmSSZ`).
pub fn pdf_date(t: DateTime<Utc>) -> String {
    t.format("D:%Y%m%d%H%M%SZ").to_string()
}

impl Writer {
    /// Build a signature revision without changing the writer's bytes,
    /// document or revision. Replaces (and so invalidates) any earlier
    /// staged signature. Pending, unflushed model ops are not included:
    /// flush first so the signature revision is the last one.
    pub fn stage_signature(&mut self, spec: &SigDictSpec) -> Result<StagedSignature, WriterError> {
        let hex = spec.contents_hex_len;
        if hex < 2 || !hex.is_multiple_of(2) || hex > MAX_CONTENTS_HEX_LEN {
            return Err(WriterError::SignaturePlaceholder(format!(
                "/Contents placeholder must be an even number of hex characters in 2..={MAX_CONTENTS_HEX_LEN}, got {hex}"
            )));
        }
        if spec.doc_mdp.is_some_and(|p| !(1..=3).contains(&p)) {
            return Err(target_err("DocMDP permission must be 1, 2, or 3"));
        }
        if spec.doc_mdp.is_some() && self.has_signatures() {
            return Err(target_err(
                "a certification signature must be the first signature",
            ));
        }
        if spec
            .lock_fields
            .as_ref()
            .is_some_and(|fields| fields.iter().any(|name| name.is_empty()))
        {
            return Err(target_err("locked field names must not be empty"));
        }
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| WriterError::Parse("writer has no document".into()))?;
        let page_ids = self.page_ids.clone();
        let fimp = form_import::parse(doc, &page_ids);

        let mut inc = IncrementalDocument::create_from(self.bytes.to_vec(), doc.clone());
        let sig_object = inc
            .new_document
            .add_object(Object::Dictionary(sig_dict(spec)));
        let acro = ensure_acroform(&mut inc, &fimp)?;

        let (field_object, field_name) = match &spec.target {
            SigTarget::ExistingField(name) => {
                let matches: Vec<_> = fimp.fields.iter().filter(|f| &f.name == name).collect();
                let field = match matches.as_slice() {
                    [] => return Err(target_err(format!("no field named {name:?}"))),
                    [f] => *f,
                    _ => return Err(target_err(format!("field name {name:?} is ambiguous"))),
                };
                if field.field_type != FieldType::Signature {
                    return Err(target_err(format!(
                        "field {name:?} is not a signature field"
                    )));
                }
                if field.flags.read_only {
                    return Err(target_err(format!("field {name:?} is read-only")));
                }
                match field.signature_state {
                    Some(SignatureFieldState::Empty) => {}
                    Some(SignatureFieldState::Occupied) => {
                        return Err(target_err(format!("field {name:?} is already signed")))
                    }
                    _ => {
                        return Err(target_err(format!(
                            "field {name:?} has an unknown or malformed /V"
                        )))
                    }
                }
                let refs = fimp
                    .refs
                    .get(name)
                    .ok_or_else(|| target_err(format!("field {name:?} has no object")))?;
                if refs.widgets.len() != 1 {
                    return Err(target_err(format!(
                        "field {name:?} has {} widgets; exactly one is required",
                        refs.widgets.len()
                    )));
                }
                inc.opt_clone_object_to_new_document(refs.field)?;
                let field_dict = inc
                    .new_document
                    .get_object_mut(refs.field)
                    .and_then(|o| o.as_dict_mut())?;
                field_dict.set("V", Object::Reference(sig_object));
                if let Some(lock) = field_lock(spec) {
                    field_dict.set("Lock", Object::Dictionary(lock));
                }
                (refs.field, name.clone())
            }
            SigTarget::NewInvisibleField { name, page } => {
                if name.is_empty() || name.contains('.') {
                    return Err(target_err(format!(
                        "new field name {name:?} must be non-empty and contain no '.'"
                    )));
                }
                let prefix = format!("{name}.");
                if fimp
                    .fields
                    .iter()
                    .any(|f| &f.name == name || f.name.starts_with(&prefix))
                    || root_field_names(doc).iter().any(|n| n == name)
                {
                    return Err(target_err(format!("field name {name:?} is already in use")));
                }
                let page_id = *page_ids
                    .get(*page as usize)
                    .ok_or(WriterError::PageOutOfRange(*page))?;
                let mut widget = dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Widget",
                    "FT" => "Sig",
                    "T" => Object::string_literal(name.as_str()),
                    "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
                    "F" => INVISIBLE_WIDGET_FLAGS,
                    "P" => Object::Reference(page_id),
                    "V" => Object::Reference(sig_object),
                };
                if let Some(lock) = field_lock(spec) {
                    widget.set("Lock", Object::Dictionary(lock));
                }
                let wid = inc.new_document.add_object(Object::Dictionary(widget));
                self.annots_add_inc(&mut inc, page_id, wid)?;
                add_root_field(&mut inc, acro, wid)?;
                (wid, name.clone())
            }
        };
        set_sig_flags(&mut inc, acro)?;
        if spec.doc_mdp.is_some() {
            set_doc_mdp(&mut inc, sig_object)?;
        }

        let mut out = Vec::new();
        inc.save_to(&mut out)?;
        let base_len = self.bytes.len();
        if out.len() <= base_len || out[..base_len] != self.bytes[..] {
            return Err(WriterError::Parse(
                "staged signature revision does not preserve earlier bytes".into(),
            ));
        }
        Document::load_mem(&out)
            .map_err(|e| WriterError::Parse(format!("staged signature unparseable: {e}")))?;

        let token = NEXT_STAGING_TOKEN.fetch_add(1, Ordering::Relaxed);
        self.pending_signature = Some(PendingSignature {
            token,
            revision: self.revision,
            staged_len: out.len(),
            sig_object,
        });
        Ok(StagedSignature {
            token,
            bytes: out,
            base_len,
            base_revision: self.revision,
            sig_object,
            field_object,
            field_name,
        })
    }

    /// True while a staged signature awaits `adopt_signed`/`abort_signature`.
    pub fn signature_pending(&self) -> bool {
        self.pending_signature.is_some()
    }

    /// Drop the staged signature (no-op when another one replaced it).
    pub fn abort_signature(&mut self, staged: &StagedSignature) {
        if self.pending_signature.map(|p| p.token) == Some(staged.token) {
            self.pending_signature = None;
        }
    }

    /// Atomically adopt a completed signature revision. `signed` must be the
    /// staged file with only its placeholders filled; it is rejected when the
    /// staging was replaced, the writer moved to another revision, earlier
    /// bytes differ, the length changed or the signature dictionary does not
    /// carry a complete `/ByteRange`. On error the writer is unchanged.
    pub fn adopt_signed(
        &mut self,
        staged: &StagedSignature,
        signed: Vec<u8>,
    ) -> Result<FlushOutput, WriterError> {
        let pending = self
            .pending_signature
            .filter(|p| p.token == staged.token)
            .ok_or_else(|| {
                WriterError::StaleSignature("staged signature was replaced or aborted".into())
            })?;
        if pending.revision != self.revision {
            return Err(WriterError::StaleSignature(format!(
                "staged at revision {}, writer is at {}",
                pending.revision, self.revision
            )));
        }
        if signed.len() != pending.staged_len {
            return Err(WriterError::StaleSignature(
                "signed file length differs from the staged revision".into(),
            ));
        }
        if signed[..self.bytes.len()] != self.bytes[..] {
            return Err(WriterError::StaleSignature(
                "earlier revisions changed after staging".into(),
            ));
        }
        let reparsed = Document::load_mem(&signed)
            .map_err(|e| WriterError::Parse(format!("signed output unparseable: {e}")))?;
        let br = reparsed
            .get_object(pending.sig_object)
            .and_then(|o| o.as_dict())
            .and_then(|d| d.get(b"ByteRange"))
            .and_then(|o| o.as_array())
            .map_err(|e| WriterError::Parse(format!("signature dictionary: {e}")))?;
        let br: Vec<i64> = br.iter().filter_map(|o| o.as_i64().ok()).collect();
        let len = signed.len() as i64;
        let complete = br.len() == 4
            && br[0] == 0
            && br[1] > 0
            && br[2] > br[1]
            && br[3] >= 0
            && br[2] + br[3] == len
            && !br.contains(&BYTE_RANGE_SENTINEL);
        if !complete {
            return Err(WriterError::SignaturePlaceholder(format!(
                "signature /ByteRange {br:?} does not cover the signed file"
            )));
        }
        let (gap_open, gap_end) = (br[1] as usize, br[2] as usize);
        if signed[gap_open] != b'<' || signed[gap_end - 1] != b'>' {
            return Err(WriterError::SignaturePlaceholder(
                "/ByteRange gap is not exactly the /Contents <...> token".into(),
            ));
        }
        let slot = byte_range_slot(&staged.bytes, staged.base_len)?;
        let only_placeholders = signed
            .iter()
            .zip(staged.bytes.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .all(|(i, _)| (gap_open < i && i < gap_end - 1) || (slot.0 < i && i < slot.1));
        if !only_placeholders {
            return Err(WriterError::SignaturePlaceholder(
                "signed file differs from the staged revision outside its placeholders".into(),
            ));
        }

        let fimp = form_import::parse(&reparsed, &self.page_ids);
        let bytes_written = signed.len() as u64;
        self.fields = fimp.refs;
        self.acroform = fimp.acroform;
        self.acroform_inline = fimp.acroform_inline;
        self.bytes = Arc::from(signed.into_boxed_slice());
        self.doc = Some(reparsed);
        self.pending_signature = None;
        self.revision += 1;
        Ok(FlushOutput {
            bytes: self.bytes.clone(),
            report: FlushReport {
                revision: self.revision,
                bytes_written,
                mode: FlushMode::Incremental,
                annots_written: 0,
                fields_written: 1,
                flattened: false,
                warnings: Vec::new(),
                dirty: Vec::new(),
            },
        })
    }
}

/// `[`/`]` offsets of the unique staged `/ByteRange [0 S S S]` array in the
/// appended revision.
fn byte_range_slot(staged: &[u8], base_len: usize) -> Result<(usize, usize), WriterError> {
    let s = BYTE_RANGE_SENTINEL.to_string();
    let needle = format!("{s} {s} {s}");
    let tail = &staged[base_len..];
    let hits: Vec<usize> = tail
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_bytes())
        .map(|(i, _)| i)
        .collect();
    let [hit] = hits[..] else {
        return Err(WriterError::SignaturePlaceholder(
            "staged /ByteRange placeholder is not unique".into(),
        ));
    };
    let open = tail[..hit].iter().rposition(|&b| b == b'[');
    let close = tail[hit..].iter().position(|&b| b == b']').map(|p| hit + p);
    match (open, close) {
        (Some(o), Some(c)) => Ok((base_len + o, base_len + c)),
        _ => Err(WriterError::SignaturePlaceholder(
            "staged /ByteRange placeholder is not an array".into(),
        )),
    }
}

fn sig_dict(spec: &SigDictSpec) -> Dictionary {
    let mut d = dictionary! {
        "Type" => "Sig",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => sub_filter_name(spec.sub_filter),
        "ByteRange" => vec![
            Object::Integer(0),
            Object::Integer(BYTE_RANGE_SENTINEL),
            Object::Integer(BYTE_RANGE_SENTINEL),
            Object::Integer(BYTE_RANGE_SENTINEL),
        ],
        "Contents" => Object::String(vec![0; spec.contents_hex_len / 2], StringFormat::Hexadecimal),
    };
    if let Some(t) = spec.claimed_time {
        d.set("M", Object::string_literal(pdf_date(t)));
    }
    for (key, value) in [
        ("Name", &spec.name),
        ("Reason", &spec.reason),
        ("Location", &spec.location),
        ("ContactInfo", &spec.contact_info),
    ] {
        if let Some(v) = value {
            d.set(key, Object::string_literal(v.as_str()));
        }
    }
    let references = signature_references(spec);
    if !references.is_empty() {
        d.set("Reference", Object::Array(references));
    }
    d
}

fn field_lock(spec: &SigDictSpec) -> Option<Dictionary> {
    let fields = spec.lock_fields.as_ref()?;
    let names = fields
        .iter()
        .map(|name| Object::string_literal(name.as_str()))
        .collect();
    Some(dictionary! {
        "Type" => "SigFieldLock",
        "Action" => "Include",
        "Fields" => Object::Array(names),
    })
}

fn signature_references(spec: &SigDictSpec) -> Vec<Object> {
    let mut references = Vec::new();
    if let Some(permission) = spec.doc_mdp {
        references.push(Object::Dictionary(dictionary! {
            "Type" => "SigRef",
            "TransformMethod" => "DocMDP",
            "TransformParams" => dictionary! {
                "Type" => "TransformParams",
                "P" => i64::from(permission),
                "V" => "1.2",
            },
        }));
    }
    if let Some(fields) = &spec.lock_fields {
        let names = fields
            .iter()
            .map(|name| Object::string_literal(name.as_str()))
            .collect();
        references.push(Object::Dictionary(dictionary! {
            "Type" => "SigRef",
            "TransformMethod" => "FieldMDP",
            "TransformParams" => dictionary! {
                "Type" => "TransformParams",
                "Action" => "Include",
                "Fields" => Object::Array(names),
                "V" => "1.2",
            },
        }));
    }
    references
}

fn set_doc_mdp(inc: &mut IncrementalDocument, signature: ObjectId) -> Result<(), WriterError> {
    let root = inc
        .new_document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)?;
    inc.opt_clone_object_to_new_document(root)?;
    let existing_permissions = inc
        .new_document
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|catalog| catalog.get(b"Perms").ok())
        .cloned();
    let mut permissions = match existing_permissions {
        Some(Object::Dictionary(d)) => d,
        Some(Object::Reference(id)) => inc
            .new_document
            .get_object(id)
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default(),
        _ => Dictionary::new(),
    };
    permissions.set("DocMDP", Object::Reference(signature));
    let catalog = inc
        .new_document
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)?;
    catalog.set("Perms", Object::Dictionary(permissions));
    Ok(())
}

/// Partial names (`/T`) of the AcroForm root fields.
fn root_field_names(doc: &Document) -> Vec<String> {
    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let af = match catalog.get(b"AcroForm") {
        Ok(Object::Reference(r)) => doc.get_object(*r).and_then(|o| o.as_dict()).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let fields = match af.and_then(|a| a.get(b"Fields").ok()) {
        Some(Object::Array(v)) => v.clone(),
        Some(Object::Reference(r)) => doc
            .get_object(*r)
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    fields
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_object(id).and_then(|o| o.as_dict()).ok())
        .filter_map(|d| d.get(b"T").ok())
        .filter_map(|t| match t {
            Object::String(b, _) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        })
        .collect()
}

fn root_id(doc: &Document) -> Result<ObjectId, WriterError> {
    doc.trailer
        .get(b"Root")
        .and_then(|o| o.as_reference())
        .map_err(|e| WriterError::Parse(format!("trailer /Root: {e}")))
}

#[derive(Clone, Copy)]
enum AcroLoc {
    Indirect(ObjectId),
    Inline(ObjectId),
}

/// Locate the `/AcroForm` dict for the incremental update (cloning its
/// holder), creating an indirect one when the document has none.
fn ensure_acroform(
    inc: &mut IncrementalDocument,
    fimp: &form_import::FormImport,
) -> Result<AcroLoc, WriterError> {
    if let Some(id) = fimp.acroform {
        inc.opt_clone_object_to_new_document(id)?;
        return Ok(AcroLoc::Indirect(id));
    }
    let root = root_id(inc.get_prev_documents())?;
    inc.opt_clone_object_to_new_document(root)?;
    if fimp.acroform_inline {
        return Ok(AcroLoc::Inline(root));
    }
    let af = inc
        .new_document
        .add_object(dictionary! { "Fields" => Vec::<Object>::new() });
    inc.new_document
        .get_object_mut(root)
        .and_then(|o| o.as_dict_mut())?
        .set("AcroForm", Object::Reference(af));
    Ok(AcroLoc::Indirect(af))
}

fn acroform_mut(
    inc: &mut IncrementalDocument,
    loc: AcroLoc,
) -> Result<&mut Dictionary, WriterError> {
    Ok(match loc {
        AcroLoc::Indirect(id) => inc
            .new_document
            .get_object_mut(id)
            .and_then(|o| o.as_dict_mut())?,
        AcroLoc::Inline(root) => inc
            .new_document
            .get_object_mut(root)
            .and_then(|o| o.as_dict_mut())?
            .get_mut(b"AcroForm")
            .and_then(|o| o.as_dict_mut())?,
    })
}

fn add_root_field(
    inc: &mut IncrementalDocument,
    loc: AcroLoc,
    field: ObjectId,
) -> Result<(), WriterError> {
    let fields_ref = {
        let af = acroform_mut(inc, loc)?;
        match af.get_mut(b"Fields") {
            Ok(Object::Array(a)) => {
                a.push(Object::Reference(field));
                None
            }
            Ok(Object::Reference(r)) => Some(*r),
            _ => {
                af.set("Fields", vec![Object::Reference(field)]);
                None
            }
        }
    };
    if let Some(arr) = fields_ref {
        inc.opt_clone_object_to_new_document(arr)?;
        inc.new_document
            .get_object_mut(arr)
            .and_then(|o| o.as_array_mut())
            .map_err(|e| WriterError::SignatureTarget(format!("/AcroForm /Fields: {e}")))?
            .push(Object::Reference(field));
    }
    Ok(())
}

/// `/SigFlags 3` (SignaturesExist | AppendOnly).
fn set_sig_flags(inc: &mut IncrementalDocument, loc: AcroLoc) -> Result<(), WriterError> {
    let af = acroform_mut(inc, loc)?;
    let flags = af.get(b"SigFlags").and_then(|o| o.as_i64()).unwrap_or(0);
    af.set("SigFlags", flags | 3);
    Ok(())
}
