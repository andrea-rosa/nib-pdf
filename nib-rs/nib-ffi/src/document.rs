//! `NibDocument`: the UniFFI document object. Documents are handles inside
//! the process-wide `RenderWorker`; all PDFium work runs on its thread.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use nib_core::ap::{AssetSource, FontSource};
use nib_core::delta::{self, Engine};
use nib_core::geom;
use nib_core::model::{self, Op};
use nib_core::writer::{self, Writer};
use serde_json::Value;

use crate::error::NibError;

/// Process-wide render worker. `None` library dir binds the already-mapped
/// system library (Android); `NIB_PDFIUM_DIR` overrides on host builds.
fn worker() -> Arc<nib_render::RenderWorker> {
    static WORKER: OnceLock<Arc<nib_render::RenderWorker>> = OnceLock::new();
    Arc::clone(WORKER.get_or_init(|| {
        nib_render::RenderWorker::spawn(std::env::var("NIB_PDFIUM_DIR").ok().map(PathBuf::from))
    }))
}

// ---------- UniFFI records / enums ----------

#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl From<Rect> for geom::Rect {
    fn from(r: Rect) -> Self {
        geom::Rect::new(r.x0, r.y0, r.x1, r.y1)
    }
}
impl From<geom::Rect> for Rect {
    fn from(r: geom::Rect) -> Self {
        Rect {
            x0: r.x0,
            y0: r.y0,
            x1: r.x1,
            y1: r.y1,
        }
    }
}

/// Page rotation, multiples of 90° clockwise (PDF `/Rotate`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

impl From<geom::Rotation> for Rotation {
    fn from(r: geom::Rotation) -> Self {
        match r {
            geom::Rotation::R0 => Rotation::R0,
            geom::Rotation::R90 => Rotation::R90,
            geom::Rotation::R180 => Rotation::R180,
            geom::Rotation::R270 => Rotation::R270,
        }
    }
}

/// Integer rect in device pixels.
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Integer size in device pixels.
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct ISize {
    pub width: i32,
    pub height: i32,
}

/// Per-page geometry and metadata in PDF user space.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PageInfo {
    pub crop_box: Rect,
    pub media_box: Rect,
    pub rotation: Rotation,
    pub label: Option<String>,
    /// Display size in points (post-`/Rotate`; swapped for 90/270).
    pub width_pt: f64,
    pub height_pt: f64,
}

/// Identifies one cached tile. `flags` encodes render options
/// (render_annotations, render_forms, grayscale).
#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TileKey {
    pub page: u16,
    /// Zoom bucket; scale = 2^(bucket/2).
    pub bucket: i8,
    pub tx: u16,
    pub ty: u16,
    pub revision: u32,
    pub flags: u8,
}

/// Render priority: visible > prefetch > thumbnail.
#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Priority {
    Visible,
    Prefetch,
    Thumbnail,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TileRequest {
    pub key: TileKey,
    /// Tile rect within the page bitmap at `key.bucket`.
    pub device_rect: IRect,
    /// Whole-page bitmap size at `key.bucket`.
    pub page_device_size: ISize,
    /// Scheduling generation; older jobs are skipped after `bump_generation`.
    pub generation: u64,
    pub priority: Priority,
    /// Distance from viewport centre; breaks priority ties.
    pub distance: u32,
}

/// A rendered tile: RGBA pixels, `stride = width * 4`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TileResult {
    pub key: TileKey,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
    pub render_ms: u16,
}

/// Open-time options. `lazy_structure` defers structure-tree parsing (M4+).
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct OpenOptions {
    pub lazy_structure: bool,
}

/// Dirty page rect from an apply/import (user space).
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct DirtyRegion {
    pub page: u32,
    pub rect: Rect,
}

/// Write mode for `flush`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FlushMode {
    Incremental,
    FullRewrite,
}

/// `flush_with` options: save mode plus form flattening (widget `/AP`
/// streams burned into page contents, `/AcroForm` removed).
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct FlushOptions {
    pub mode: FlushMode,
    pub flatten: bool,
}

/// Outcome of `flush`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FlushReport {
    pub revision: u32,
    pub bytes_written: u64,
    pub mode: FlushMode,
    pub annots_written: u32,
    pub fields_written: u32,
    pub flattened: bool,
    pub warnings: Vec<String>,
    pub dirty: Vec<DirtyRegion>,
}

/// One AcroForm field: `json` is the full entity JSON, `value_json` just the
/// `value` member ("null" when unset). `page`/`rect` come from widget[0].
#[derive(Debug, Clone, uniffi::Record)]
pub struct FormFieldSummary {
    pub name: String,
    /// Model serde type: text/checkbox/radio/pushButton/combo/list/signature.
    pub field_type: String,
    pub page: Option<u32>,
    pub rect: Option<Rect>,
    pub value_json: String,
    pub json: String,
}

/// One field's value as pdfium sees the current revision
/// (`form_field_values`, diagnostic).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FormFieldValue {
    pub name: String,
    pub field_type: String,
    pub value: Option<String>,
    pub checked: Option<bool>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SigningAlgorithm {
    RsaPss,
    EcdsaP256,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SigningDigest {
    Sha256,
    Sha384,
    Sha512,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SignatureOptions {
    pub certificate: Vec<u8>,
    pub field_name: Option<String>,
    pub page: u32,
    pub algorithm: SigningAlgorithm,
    pub digest: SigningDigest,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    pub contents_hex_len: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SignatureRequest {
    pub token: String,
    pub data: Vec<u8>,
    pub digest: SigningDigest,
    pub algorithm: SigningAlgorithm,
}

struct PendingSigning {
    token: String,
    prepared: Option<nib_sign::PreparedSignature>,
    writer_revision: u32,
    engine_mutation: u64,
}

/// Foreign asset lookup (stamp images) supplied by the host app.
#[uniffi::export(with_foreign)]
pub trait AssetProvider: Send + Sync {
    fn asset(&self, asset_id: String) -> Option<Vec<u8>>;
}

/// Foreign font lookup (FreeText) supplied by the host app.
#[uniffi::export(with_foreign)]
pub trait FontProvider: Send + Sync {
    fn font(&self, family: String, bold: bool, italic: bool) -> Option<Vec<u8>>;
}

struct AssetAdapter(Option<Arc<dyn AssetProvider>>);

impl AssetSource for AssetAdapter {
    fn asset(&self, asset_id: &str) -> Option<Vec<u8>> {
        self.0.as_ref()?.asset(asset_id.to_string())
    }
}

struct FontAdapter(Option<Arc<dyn FontProvider>>);

impl FontSource for FontAdapter {
    fn font(&self, family: &str, bold: bool, italic: bool) -> Option<Vec<u8>> {
        self.0.as_ref()?.font(family.to_string(), bold, italic)
    }
}

/// One validation failure inside a rejected delta (JSON_SCHEMA §3).
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeltaValidationError {
    pub op_index: u32,
    pub path: String,
    /// e.g. `PAGE_OUT_OF_RANGE`.
    pub code: String,
    pub message: String,
}

/// Outcome of `apply`/`import_delta`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ApplyReport {
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    pub rejected: Vec<DeltaValidationError>,
    pub dirty: Vec<DirtyRegion>,
}

/// Delta export shape (JSON_SCHEMA §8).
#[derive(Debug, Copy, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ExportMode {
    Full,
    Compacted,
}

/// `import_delta` options (§8 import semantics).
#[derive(Debug, Copy, Clone, uniffi::Record)]
pub struct ImportOptions {
    pub force: bool,
    pub rebase: bool,
    pub undoable: bool,
}

/// One annotation in z-order: `json` is the full entity JSON (incl. `subtype`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct AnnotationSummary {
    pub id: String,
    pub subtype: String,
    pub page: u32,
    pub rect: Rect,
    pub json: String,
}

fn to_report(r: delta::ApplyReport) -> ApplyReport {
    ApplyReport {
        applied: r.applied.iter().map(|u| u.to_string()).collect(),
        skipped: r.skipped.iter().map(|u| u.to_string()).collect(),
        rejected: r
            .rejected
            .iter()
            .map(|e| DeltaValidationError {
                op_index: e.op_index as u32,
                path: e.path.clone(),
                code: serde_json::to_value(e.code)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", e.code)),
                message: e.message.clone(),
            })
            .collect(),
        dirty: r
            .dirty
            .iter()
            .map(|&(page, rect)| DirtyRegion {
                page,
                rect: rect.into(),
            })
            .collect(),
    }
}

fn to_nib_error(e: delta::DeltaError) -> NibError {
    match e {
        delta::DeltaError::Invalid(errs) => {
            NibError::InvalidDelta(serde_json::to_string(&errs).unwrap_or_else(|_| "[]".into()))
        }
        delta::DeltaError::RevisionMismatch { expected, got } => {
            NibError::RevisionMismatch { expected, got }
        }
    }
}

/// Signature ops never reach the engine: the writer cannot persist them and
/// digital signatures are produced by the dedicated signing API.
fn reject_signature_ops(ops: &[Op]) -> Result<(), NibError> {
    let errs: Vec<_> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| op.target == model::Target::Signature)
        .map(|(i, _)| {
            delta::ValidationError::new(
                i,
                "/target",
                delta::ErrorCode::UnsupportedOp,
                "signature ops are not supported by apply/import; use the signing API",
            )
        })
        .collect();
    if errs.is_empty() {
        return Ok(());
    }
    Err(NibError::InvalidDelta(
        serde_json::to_string(&errs).unwrap_or_else(|_| "[]".into()),
    ))
}

fn schema_error(message: String) -> NibError {
    let errs = [delta::ValidationError::new(
        0,
        "",
        delta::ErrorCode::Schema,
        message,
    )];
    NibError::InvalidDelta(serde_json::to_string(&errs).unwrap_or_else(|_| "[]".into()))
}

fn subtype_name(a: &model::Annotation) -> &'static str {
    use model::Annotation::*;
    match a {
        Ink(_) => "Ink",
        Highlight(_) => "Highlight",
        Underline(_) => "Underline",
        StrikeOut(_) => "StrikeOut",
        Squiggly(_) => "Squiggly",
        FreeText(_) => "FreeText",
        Text(_) => "Text",
        Stamp(_) => "Stamp",
        Square(_) => "Square",
        Circle(_) => "Circle",
        Line(_) => "Line",
    }
}

fn form_field_type_name(t: model::FieldType) -> &'static str {
    use model::FieldType::*;
    match t {
        Text => "text",
        Checkbox => "checkbox",
        Radio => "radio",
        PushButton => "pushButton",
        Combo => "combo",
        List => "list",
        Signature => "signature",
    }
}

fn form_field_summary(f: &model::FormField) -> FormFieldSummary {
    let w0 = f.widgets.first();
    FormFieldSummary {
        name: f.name.clone(),
        field_type: form_field_type_name(f.field_type).into(),
        page: w0.map(|w| w.page),
        rect: w0.map(|w| w.rect.into()),
        value_json: serde_json::to_string(&f.value).unwrap_or_else(|_| "null".into()),
        json: serde_json::to_string(f).unwrap_or_else(|_| "{}".into()),
    }
}

fn annotation_summary(a: &model::Annotation) -> AnnotationSummary {
    let base = a.base();
    AnnotationSummary {
        id: base.id.to_string(),
        subtype: subtype_name(a).into(),
        page: base.page,
        rect: delta::dirty::bbox(a)
            .unwrap_or_else(|| geom::Rect::new(0.0, 0.0, 0.0, 0.0))
            .into(),
        json: serde_json::to_string(a).unwrap_or_else(|_| "{}".into()),
    }
}

// ---------- NibDocument ----------

/// An open PDF document. All rendering happens on the shared `nib-render`
/// worker thread; methods enqueue work and (for async ones) await results.
#[derive(uniffi::Object)]
pub struct NibDocument {
    worker: Arc<nib_render::RenderWorker>,
    doc: Mutex<Option<u64>>,
    /// PDF writer; `Pending` holds the original bytes until the first call
    /// that needs annotation state (`lazy_structure` defers the import).
    /// Lock order: `writer` before `engine` — never the reverse.
    writer: Mutex<WriterSlot>,
    engine: Mutex<Engine>,
    pending_signing: Mutex<Option<PendingSigning>>,
    engine_mutation: AtomicU64,
    verified_signature_state: VerifiedSignatureState,
    asset_provider: Mutex<Option<Arc<dyn AssetProvider>>>,
    font_provider: Mutex<Option<Arc<dyn FontProvider>>>,
}

enum WriterSlot {
    Pending(Vec<u8>),
    Ready(Box<Writer>),
    Failed(String),
}

#[derive(Default)]
struct VerifiedSignatureState {
    present: bool,
    doc_mdp_permission: Option<u8>,
    locked_fields: Vec<String>,
}

impl VerifiedSignatureState {
    fn from_pdf(bytes: &[u8]) -> Self {
        let mut state = Self::default();
        match nib_sign::verify_signatures(bytes, &[]) {
            Ok(signatures) => {
                for signature in signatures {
                    if signature.integrity != nib_sign::IntegrityStatus::Valid {
                        log::warn!(
                            "signature {} failed integrity verification: {}",
                            signature.field_name,
                            signature.errors.join("; ")
                        );
                        continue;
                    }
                    state.present = true;
                    if let Some(permission) = signature.doc_mdp {
                        state.doc_mdp_permission = Some(
                            state
                                .doc_mdp_permission
                                .map_or(permission, |current| current.min(permission)),
                        );
                    }
                    state.locked_fields.extend(signature.locked_fields);
                }
                state.locked_fields.sort();
                state.locked_fields.dedup();
            }
            Err(error) => {
                log::warn!("signature verification while opening PDF failed: {error}");
            }
        }
        state
    }

    fn restore(&self, engine: &mut Engine) {
        if self.present {
            engine.restore_verified_signature_state(
                self.doc_mdp_permission,
                self.locked_fields.clone(),
            );
        }
    }
}

impl NibDocument {
    /// Engine lock tolerant of a poisoned mutex (an earlier panic while
    /// holding the lock must not crash subsequent FFI calls).
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn doc_id(&self) -> Result<u64, NibError> {
        self.doc
            .lock()
            .unwrap()
            .ok_or_else(|| NibError::Internal("document closed".into()))
    }

    /// Lazily opens the PDF writer (`Pending` → `Ready`) and merges its
    /// imported annotations into the engine. No-op once ready.
    fn ensure_writer(&self) -> Result<(), NibError> {
        let mut slot = self.writer.lock().unwrap();
        let WriterSlot::Pending(bytes) = &*slot else {
            return match &*slot {
                WriterSlot::Ready(_) => Ok(()),
                WriterSlot::Failed(m) => Err(NibError::WriteFailed(m.clone())),
                WriterSlot::Pending(_) => unreachable!(),
            };
        };
        match Writer::open(bytes.clone()) {
            Ok((w, annots, fields)) => {
                *slot = WriterSlot::Ready(Box::new(w));
                // Lock order: writer before engine.
                let mut engine = self.engine();
                engine.adopt_base(annots, fields);
                self.verified_signature_state.restore(&mut engine);
                Ok(())
            }
            Err(writer::WriterError::Encrypted) => Err(NibError::EncryptedUnsupported),
            Err(e) => {
                let m = e.to_string();
                *slot = WriterSlot::Failed(m.clone());
                Err(NibError::WriteFailed(m))
            }
        }
    }

    fn writer_for_ops(&self, ops: &[Op]) -> Result<Option<MutexGuard<'_, WriterSlot>>, NibError> {
        let needs_writer = ops.iter().any(|op| {
            op.target == model::Target::FormField
                || (op.target == model::Target::Annotation
                    && op.payload.get("bindToField").is_some())
        });
        if !needs_writer {
            return Ok(None);
        }
        self.ensure_writer()?;
        let slot = self.writer_lock();
        let WriterSlot::Ready(writer) = &*slot else {
            return Err(NibError::WriteFailed("writer unavailable".into()));
        };
        for (index, op) in ops.iter().enumerate() {
            if op.target != model::Target::Annotation {
                continue;
            }
            if let Some(field) = op.payload.get("bindToField").and_then(Value::as_str) {
                if !writer.form_refs().contains_key(field) {
                    return Err(to_nib_error(delta::DeltaError::Invalid(vec![
                        delta::ValidationError::new(
                            index,
                            "payload.bindToField",
                            delta::ErrorCode::UnknownId,
                            format!("signature field {field} does not exist in the PDF"),
                        ),
                    ])));
                }
            }
        }
        Ok(Some(slot))
    }

    fn writer_lock(&self) -> MutexGuard<'_, WriterSlot> {
        self.writer.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn signing_writer<'a>(&self, slot: &'a mut WriterSlot) -> Result<&'a mut Writer, NibError> {
        match slot {
            WriterSlot::Ready(writer) => Ok(writer),
            WriterSlot::Pending(_) => Err(NibError::WriteFailed("writer unavailable".into())),
            WriterSlot::Failed(message) => Err(NibError::WriteFailed(message.clone())),
        }
    }
}

#[uniffi::export]
impl NibDocument {
    /// Opens a document from its full byte content.
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>, options: OpenOptions) -> Result<Arc<Self>, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let worker = worker();
            let verified_signature_state = VerifiedSignatureState::from_pdf(&bytes);
            let document_id = Engine::document_id_for(&bytes);
            let doc = worker.open(bytes.clone()).map_err(NibError::from)?;
            let page_count = worker.page_count(doc).map_err(NibError::from)?;
            let (writer_slot, mut engine) = if options.lazy_structure {
                (
                    WriterSlot::Pending(bytes),
                    Engine::new(document_id, page_count),
                )
            } else {
                match Writer::open(bytes) {
                    Ok((w, annots, fields)) => (
                        WriterSlot::Ready(Box::new(w)),
                        Engine::with_base(document_id, page_count, annots, fields),
                    ),
                    Err(writer::WriterError::Encrypted) => {
                        return Err(NibError::EncryptedUnsupported)
                    }
                    Err(e) => {
                        log::warn!("writer open failed, document is read-only: {e}");
                        (
                            WriterSlot::Failed(e.to_string()),
                            Engine::new(document_id, page_count),
                        )
                    }
                }
            };
            verified_signature_state.restore(&mut engine);
            Ok(Arc::new(NibDocument {
                worker,
                doc: Mutex::new(Some(doc)),
                writer: Mutex::new(writer_slot),
                engine: Mutex::new(engine),
                pending_signing: Mutex::new(None),
                engine_mutation: AtomicU64::new(0),
                verified_signature_state,
                asset_provider: Mutex::new(None),
                font_provider: Mutex::new(None),
            }))
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in from_bytes".into())))
    }

    pub fn page_count(&self) -> Result<u32, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.worker
                .page_count(self.doc_id()?)
                .map_err(NibError::from)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in page_count".into())))
    }

    pub fn page_info(&self, index: u32) -> Result<PageInfo, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let info = self
                .worker
                .page_info(self.doc_id()?, index as u16)
                .map_err(NibError::from)?;
            Ok(PageInfo {
                crop_box: info.crop_box.into(),
                media_box: info.media_box.into(),
                rotation: info.rotation.into(),
                label: info.label,
                width_pt: info.width_pt,
                height_pt: info.height_pt,
            })
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in page_info".into())))
    }

    /// Renders one tile. Cancelling the coroutine drops the Rust future,
    /// which drops the reply receiver — the worker then skips the job.
    pub async fn render_tile(&self, req: TileRequest) -> Result<TileResult, NibError> {
        let req = nib_render::types::TileRequest {
            key: nib_render::types::TileKey {
                page: req.key.page,
                bucket: req.key.bucket,
                tx: req.key.tx,
                ty: req.key.ty,
                revision: req.key.revision,
                flags: req.key.flags,
            },
            device_rect: nib_render::types::IRect {
                x: req.device_rect.x,
                y: req.device_rect.y,
                width: req.device_rect.width,
                height: req.device_rect.height,
            },
            page_device_size: nib_render::types::ISize {
                width: req.page_device_size.width,
                height: req.page_device_size.height,
            },
            generation: req.generation,
            priority: match req.priority {
                Priority::Visible => nib_render::types::Priority::Visible,
                Priority::Prefetch => nib_render::types::Priority::Prefetch,
                Priority::Thumbnail => nib_render::types::Priority::Thumbnail,
            },
            distance: req.distance,
        };
        let rx = self.worker.render_tile(self.doc_id()?, req);
        match rx.await {
            Ok(Ok(t)) => Ok(TileResult {
                key: t.key.into(),
                width: t.width,
                height: t.height,
                stride: t.stride,
                pixels: t.pixels,
                render_ms: t.render_ms,
            }),
            Ok(Err(e)) => Err(e.into()),
            // Worker skipped/dropped the job (stale generation or shutdown).
            Err(_) => Err(NibError::Cancelled),
        }
    }

    /// Text inside a user-space rect on `page` (concatenated, content order).
    pub fn text_in_rect(&self, page: u32, rect: Rect) -> Result<String, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.worker
                .text_in_rect(self.doc_id()?, page as u16, rect.into())
                .map_err(NibError::from)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in text_in_rect".into())))
    }

    /// Bumps the scheduling generation; queued tiles from older generations
    /// are skipped. Call on zoom-bucket change or revision swap. Returns the
    /// new process-wide generation for tagging subsequent tile requests.
    pub fn bump_generation(&self) -> Result<u64, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.worker.bump_generation().map_err(NibError::from)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in bump_generation".into())))
    }

    /// Applies a JSON array of ops (or a single op object) atomically.
    pub fn apply(&self, ops_json: String) -> Result<ApplyReport, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let ops: Vec<Op> = match serde_json::from_str::<Value>(&ops_json) {
                Ok(Value::Array(_)) => {
                    serde_json::from_str(&ops_json).map_err(|e| schema_error(e.to_string()))?
                }
                Ok(v @ Value::Object(_)) => {
                    vec![serde_json::from_value(v).map_err(|e| schema_error(e.to_string()))?]
                }
                Ok(_) => {
                    return Err(schema_error("ops must be an object or array".into()));
                }
                Err(e) => return Err(schema_error(e.to_string())),
            };
            reject_signature_ops(&ops)?;
            let _writer = self.writer_for_ops(&ops)?;
            let mut engine = self.engine();
            let result = engine.apply(ops).map(to_report).map_err(to_nib_error);
            if result
                .as_ref()
                .is_ok_and(|report| !report.applied.is_empty())
            {
                self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            }
            result
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in apply".into())))
    }

    /// Undoes the last step; returns the applied inverse ops as JSON.
    pub fn undo(&self) -> Option<String> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut engine = self.engine();
            let undone = engine.undo();
            if undone.is_some() {
                self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            }
            undone.map(|ops| serde_json::to_string(&ops).unwrap_or_else(|_| "[]".into()))
        }))
        .unwrap_or(None)
    }

    /// Redoes the last undone step; returns the applied ops as JSON.
    pub fn redo(&self) -> Option<String> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut engine = self.engine();
            let redone = engine.redo();
            if redone.is_some() {
                self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            }
            redone.map(|ops| serde_json::to_string(&ops).unwrap_or_else(|_| "[]".into()))
        }))
        .unwrap_or(None)
    }

    pub fn can_undo(&self) -> bool {
        self.engine().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.engine().can_redo()
    }

    /// Opens an explicit undo group; returns its groupId.
    pub fn begin_group(&self) -> String {
        self.engine().begin_group().to_string()
    }

    /// Closes the group opened by `begin_group` as one undo step.
    pub fn end_group(&self) {
        self.engine().end_group();
    }

    /// Serialises the delta document (ops serialised as JSON).
    pub fn export_delta(&self, mode: ExportMode) -> String {
        let _ = self.ensure_writer();
        let mode = match mode {
            ExportMode::Full => delta::ExportMode::Full,
            ExportMode::Compacted => delta::ExportMode::Compacted,
        };
        self.engine().export_json(
            mode,
            Some(format!("nib-pdf/{} (android)", env!("CARGO_PKG_VERSION"))),
        )
    }

    /// Imports a delta document (§8 import semantics).
    pub fn import_delta(
        &self,
        json: String,
        options: ImportOptions,
    ) -> Result<ApplyReport, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let doc: model::DeltaDocument =
                serde_json::from_str(&json).map_err(|e| schema_error(e.to_string()))?;
            reject_signature_ops(&doc.ops)?;
            let _writer = self.writer_for_ops(&doc.ops)?;
            let opts = delta::ImportOptions {
                force: options.force,
                rebase: options.rebase,
                undoable: options.undoable,
            };
            let mut engine = self.engine();
            let result = engine
                .import(doc, opts)
                .map(to_report)
                .map_err(to_nib_error);
            if result
                .as_ref()
                .is_ok_and(|report| !report.applied.is_empty())
            {
                self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            }
            result
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in import_delta".into())))
    }

    /// Annotations on `page` in z-order.
    pub fn annotations(&self, page: u32) -> Vec<AnnotationSummary> {
        catch_unwind(AssertUnwindSafe(|| {
            let _ = self.ensure_writer();
            self.engine()
                .model()
                .annotations_on_page(page)
                .into_iter()
                .map(annotation_summary)
                .collect()
        }))
        .unwrap_or_default()
    }

    /// Full entity JSON for one annotation, if present.
    pub fn annotation_json(&self, id: String) -> Option<String> {
        let _ = self.ensure_writer();
        self.engine()
            .model()
            .annotation(&id)
            .map(|a| serde_json::to_string(a).unwrap_or_else(|_| "{}".into()))
    }

    /// Topmost annotation id hit at `pt` (user space), or none.
    pub fn hit_test(&self, page: u32, pt: Point, tolerance: f64) -> Option<String> {
        let _ = self.ensure_writer();
        self.engine()
            .hit_test(page, geom::Point::new(pt.x, pt.y), tolerance)
    }

    /// Text line rects inside `rect` on `page` (user space), top→bottom.
    pub fn text_lines_in_rect(&self, page: u32, rect: Rect) -> Result<Vec<Rect>, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let r: geom::Rect = rect.into();
            let chars = self
                .worker
                .char_boxes(self.doc_id()?, page as u16, r)
                .map_err(NibError::from)?;
            Ok(nib_core::text::snap_lines(&chars, r)
                .into_iter()
                .map(Rect::from)
                .collect())
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in text_lines_in_rect".into())))
    }

    /// AcroForm fields of the document, in z-order (empty after flatten).
    pub fn form_fields(&self) -> Vec<FormFieldSummary> {
        catch_unwind(AssertUnwindSafe(|| {
            let _ = self.ensure_writer();
            self.engine()
                .model()
                .form_fields_ordered()
                .into_iter()
                .map(form_field_summary)
                .collect()
        }))
        .unwrap_or_default()
    }

    /// Full entity JSON for one form field, if present.
    pub fn form_field_json(&self, name: String) -> Option<String> {
        let _ = self.ensure_writer();
        self.engine()
            .model()
            .form_field(&name)
            .map(|f| serde_json::to_string(f).unwrap_or_else(|_| "{}".into()))
    }

    /// Sets one field's `value` (JSON: string, string array, or null).
    /// Validation errors (`FIELD_READONLY`, `VALUE_NOT_IN_OPTIONS`,
    /// `MAXLEN_EXCEEDED`, …) surface as `InvalidDelta`.
    pub fn set_field_value(
        &self,
        name: String,
        value_json: String,
    ) -> Result<ApplyReport, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.ensure_writer()?;
            let value: Value = serde_json::from_str(&value_json)
                .map_err(|e| schema_error(format!("value_json: {e}")))?;
            let mut engine = self.engine();
            let page = engine
                .model()
                .form_field(&name)
                .and_then(|f| f.widgets.first().map(|w| w.page));
            let op = model::Op {
                op_id: uuid::Uuid::now_v7(),
                ts: chrono::Utc::now(),
                author: None,
                group_id: None,
                kind: model::OpKind::Update,
                target: model::Target::FormField,
                id: name,
                page,
                payload: serde_json::json!({ "value": value }),
            };
            let result = engine.apply(vec![op]).map(to_report).map_err(to_nib_error);
            if result
                .as_ref()
                .is_ok_and(|report| !report.applied.is_empty())
            {
                self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            }
            result
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in set_field_value".into())))
    }

    /// Flushes and privately prepares a signature. The returned bytes are the
    /// signed attributes for the host signer; they are never published until
    /// `complete_signature` succeeds.
    pub fn prepare_signature(
        &self,
        options: SignatureOptions,
    ) -> Result<SignatureRequest, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.ensure_writer()?;
            // Preserve the writer-before-engine lock order. No host code is
            // called from this method.
            let mut slot = self.writer_lock();
            let mut engine = self.engine();
            let mut pending = self
                .pending_signing
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if pending.is_some() {
                return Err(NibError::Signature(
                    "another signature is already prepared".into(),
                ));
            }
            let writer = self.signing_writer(&mut slot)?;

            let ops = engine.flush_ops();
            if !ops.is_empty() {
                let assets = AssetAdapter(self.asset_provider.lock().unwrap().clone());
                let fonts = FontAdapter(self.font_provider.lock().unwrap().clone());
                let out = writer
                    .flush(
                        &ops,
                        engine.model(),
                        writer::FlushOptions {
                            mode: writer::FlushMode::Incremental,
                            flatten: false,
                        },
                        &assets,
                        &fonts,
                    )
                    .map_err(NibError::from)?;
                let mut warnings = out.report.warnings.clone();
                if let Err(e) = self.worker.swap(self.doc_id()?, out.bytes.to_vec()) {
                    warnings.push(format!("render swap failed: {e}"));
                }
                engine.mark_flushed(out.report.revision);
                if !warnings.is_empty() {
                    log::warn!(
                        "signature preparation flush warnings: {}",
                        warnings.join("; ")
                    );
                }
            }

            let algorithm = match options.algorithm {
                SigningAlgorithm::RsaPss => nib_core::model::SignatureAlgorithm::RsassaPss,
                SigningAlgorithm::EcdsaP256 => nib_core::model::SignatureAlgorithm::EcdsaP256,
            };
            let digest = match options.digest {
                SigningDigest::Sha256 => nib_core::model::DigestAlgorithm::Sha256,
                SigningDigest::Sha384 => nib_core::model::DigestAlgorithm::Sha384,
                SigningDigest::Sha512 => nib_core::model::DigestAlgorithm::Sha512,
            };
            if algorithm == nib_core::model::SignatureAlgorithm::EcdsaP256
                && digest != nib_core::model::DigestAlgorithm::Sha256
            {
                return Err(NibError::Signature(
                    "ECDSA P-256 signing requires SHA-256".into(),
                ));
            }
            let target = match options.field_name {
                Some(field_name) => nib_sign::SigTarget::ExistingField(field_name),
                None => nib_sign::SigTarget::NewInvisibleField {
                    name: format!("NibSignature-{}", uuid::Uuid::now_v7()),
                    page: options.page,
                },
            };
            let mut sign_options = nib_sign::SignOptions::new(
                target,
                nib_sign::SignerIdentity::Certificate(options.certificate),
                algorithm,
            );
            sign_options.digest = digest;
            sign_options.reason = options.reason;
            sign_options.location = options.location;
            sign_options.contact_info = options.contact_info;
            if options.contents_hex_len != 0 {
                sign_options.contents_hex_len = options.contents_hex_len as usize;
            }
            let prepared = nib_sign::prepare(writer, sign_options)
                .map_err(|e| NibError::Signature(e.to_string()))?;
            let request = prepared.request();
            let token = uuid::Uuid::now_v7().to_string();
            let result = SignatureRequest {
                token: token.clone(),
                data: request.data.to_vec(),
                digest: options.digest,
                algorithm: options.algorithm,
            };
            *pending = Some(PendingSigning {
                token,
                prepared: Some(prepared),
                writer_revision: writer.revision(),
                engine_mutation: self.engine_mutation.load(Ordering::SeqCst),
            });
            Ok(result)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in prepare_signature".into())))
    }

    /// Verifies and atomically publishes a host signature. Invalid signatures
    /// leave the preparation retryable; stale preparations are aborted.
    pub fn complete_signature(
        &self,
        token: String,
        signature: Vec<u8>,
        certificate_chain: Vec<Vec<u8>>,
    ) -> Result<FlushReport, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.ensure_writer()?;
            // Both locks are held for the final token check and adoption, so
            // an edit cannot race between validation and publication.
            let mut slot = self.writer_lock();
            let mut engine = self.engine();
            let mut pending_guard = self
                .pending_signing
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let pending = pending_guard
                .as_mut()
                .filter(|p| p.token == token)
                .ok_or_else(|| NibError::Signature("unknown or expired signing token".into()))?;
            let writer = self.signing_writer(&mut slot)?;
            let current_mutation = self.engine_mutation.load(Ordering::SeqCst);
            if writer.revision() != pending.writer_revision
                || current_mutation != pending.engine_mutation
            {
                if let Some(prepared) = pending.prepared.take() {
                    prepared.abort(writer);
                }
                *pending_guard = None;
                return Err(NibError::StaleSignature(
                    "document changed after signature preparation".into(),
                ));
            }
            let prepared = pending
                .prepared
                .as_ref()
                .ok_or_else(|| NibError::Signature("signature preparation was aborted".into()))?;
            let out = prepared
                .complete(
                    writer,
                    nib_sign::Completion::Signature {
                        signature,
                        chain: certificate_chain,
                    },
                )
                .map_err(|e| NibError::Signature(e.to_string()))?;
            let mut warnings = out.report.warnings;
            if let Err(e) = self.worker.swap(self.doc_id()?, out.bytes.to_vec()) {
                warnings.push(format!("render swap failed: {e}"));
            }
            engine.mark_flushed(out.report.revision);
            engine.adopt_signed_fields(writer.imported_fields().map_err(NibError::from)?);
            let verified = VerifiedSignatureState::from_pdf(&out.bytes);
            verified.restore(&mut engine);
            self.engine_mutation.fetch_add(1, Ordering::SeqCst);
            *pending_guard = None;
            Ok(FlushReport {
                revision: out.report.revision,
                bytes_written: out.report.bytes_written,
                mode: FlushMode::Incremental,
                annots_written: out.report.annots_written,
                fields_written: out.report.fields_written,
                flattened: false,
                warnings,
                dirty: out
                    .report
                    .dirty
                    .iter()
                    .map(|(page, rect)| DirtyRegion {
                        page: *page,
                        rect: (*rect).into(),
                    })
                    .collect(),
            })
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in complete_signature".into())))
    }

    /// Aborts a prepared signature without publishing any staged bytes.
    pub fn abort_signature(&self, token: String) -> Result<(), NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut slot = self.writer_lock();
            let writer = self.signing_writer(&mut slot)?;
            let mut pending = self
                .pending_signing
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let prepared = pending
                .as_mut()
                .filter(|p| p.token == token)
                .and_then(|p| p.prepared.take())
                .ok_or_else(|| NibError::Signature("unknown or expired signing token".into()))?;
            prepared.abort(writer);
            *pending = None;
            Ok(())
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in abort_signature".into())))
    }

    /// Field values as pdfium sees the current rendered revision
    /// (diagnostic; the model in `form_fields` is authoritative).
    pub fn form_field_values(&self) -> Result<Vec<FormFieldValue>, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let vals = self
                .worker
                .form_field_values(self.doc_id()?)
                .map_err(NibError::from)?;
            Ok(vals
                .into_iter()
                .map(|v| FormFieldValue {
                    name: v.name,
                    field_type: v.field_type,
                    value: v.value,
                    checked: v.checked,
                })
                .collect())
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in form_field_values".into())))
    }

    /// Applies pending annotation ops to the PDF and swaps the rendered
    /// document. Incremental with no ops is a no-op report.
    pub fn flush(&self, mode: FlushMode) -> Result<FlushReport, NibError> {
        self.flush_with(FlushOptions {
            mode,
            flatten: false,
        })
    }

    /// `flush` with options (e.g. `flatten`). A swap failure after a
    /// successful write still marks the ops flushed — the bytes are already
    /// saved and the ops must not be re-applied on the next flush
    /// (duplicate-create bug); the failure is reported as a warning instead.
    pub fn flush_with(&self, options: FlushOptions) -> Result<FlushReport, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            self.ensure_writer()?;
            // Lock order: writer, then engine.
            let mut slot = self.writer_lock();
            let WriterSlot::Ready(w) = &mut *slot else {
                return Err(NibError::WriteFailed("writer unavailable".into()));
            };
            let mut engine = self.engine();
            let ops = engine.flush_ops();
            let core_options = writer::FlushOptions {
                mode: match options.mode {
                    FlushMode::Incremental => writer::FlushMode::Incremental,
                    FlushMode::FullRewrite => writer::FlushMode::FullRewrite,
                },
                flatten: options.flatten,
            };
            if ops.is_empty() && options.mode == FlushMode::Incremental && !options.flatten {
                return Ok(FlushReport {
                    revision: w.revision(),
                    bytes_written: 0,
                    mode: options.mode,
                    annots_written: 0,
                    fields_written: 0,
                    flattened: false,
                    warnings: vec![],
                    dirty: vec![],
                });
            }
            let assets = AssetAdapter(self.asset_provider.lock().unwrap().clone());
            let fonts = FontAdapter(self.font_provider.lock().unwrap().clone());
            let out = w
                .flush(&ops, engine.model(), core_options, &assets, &fonts)
                .map_err(NibError::from)?;
            let mut warnings = out.report.warnings;
            if let Err(e) = self.worker.swap(self.doc_id()?, out.bytes.to_vec()) {
                warnings.push(format!("render swap failed: {e}"));
            }
            engine.mark_flushed(out.report.revision);
            if out.report.flattened {
                engine.drop_form_fields();
            }
            Ok(FlushReport {
                revision: out.report.revision,
                bytes_written: out.report.bytes_written,
                mode: options.mode,
                annots_written: out.report.annots_written,
                fields_written: out.report.fields_written,
                flattened: out.report.flattened,
                warnings,
                dirty: out
                    .report
                    .dirty
                    .iter()
                    .map(|(p, r)| DirtyRegion {
                        page: *p,
                        rect: (*r).into(),
                    })
                    .collect(),
            })
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in flush_with".into())))
    }

    /// Current document bytes (post-flush revisions included).
    pub fn bytes(&self) -> Result<Vec<u8>, NibError> {
        catch_unwind(AssertUnwindSafe(|| {
            let slot = self.writer_lock();
            match &*slot {
                WriterSlot::Ready(w) => Ok(w.bytes().to_vec()),
                WriterSlot::Pending(b) => Ok(b.clone()),
                WriterSlot::Failed(m) => Err(NibError::WriteFailed(m.clone())),
            }
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in bytes".into())))
    }

    /// Writer revision (0 before the first flush).
    pub fn revision(&self) -> u32 {
        let _ = self.ensure_writer();
        match &*self.writer_lock() {
            WriterSlot::Ready(w) => w.revision(),
            _ => self.engine().revision(),
        }
    }

    /// Ids whose model state differs from the last flushed base.
    pub fn uncommitted_ids(&self) -> Vec<String> {
        let _ = self.ensure_writer();
        self.engine().uncommitted_ids()
    }

    /// Registers the host's foreign-asset provider (stamp images).
    pub fn set_asset_provider(&self, provider: Option<Arc<dyn AssetProvider>>) {
        *self.asset_provider.lock().unwrap() = provider;
    }

    /// Registers the host's foreign-font provider (FreeText).
    pub fn set_font_provider(&self, provider: Option<Arc<dyn FontProvider>>) {
        *self.font_provider.lock().unwrap() = provider;
    }

    /// Releases the document handle; the worker drops its pdfium document.
    /// Subsequent calls fail with `Internal("document closed")`.
    ///
    /// Named `close_doc` because `close` clashes with `AutoCloseable.close`
    /// on the generated Kotlin object.
    pub fn close_doc(&self) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(doc) = self.doc.lock().unwrap().take() {
                self.worker.close(doc);
            }
        }));
    }
}

impl From<nib_render::types::TileKey> for TileKey {
    fn from(k: nib_render::types::TileKey) -> Self {
        TileKey {
            page: k.page,
            bucket: k.bucket,
            tx: k.tx,
            ty: k.ty,
            revision: k.revision,
            flags: k.flags,
        }
    }
}

impl Drop for NibDocument {
    fn drop(&mut self) {
        if let Some(doc) = self.doc.get_mut().unwrap().take() {
            self.worker.close(doc);
        }
    }
}
