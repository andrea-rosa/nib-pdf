//! `NibDocument`: the UniFFI document object. Documents are handles inside
//! the process-wide `RenderWorker`; all PDFium work runs on its thread.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use nib_core::delta::{self, Engine};
use nib_core::geom;
use nib_core::model::{self, Op};
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
    engine: Mutex<Engine>,
}

impl NibDocument {
    fn doc_id(&self) -> Result<u64, NibError> {
        self.doc
            .lock()
            .unwrap()
            .ok_or_else(|| NibError::Internal("document closed".into()))
    }
}

#[uniffi::export]
impl NibDocument {
    /// Opens a document from its full byte content.
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>, options: OpenOptions) -> Result<Arc<Self>, NibError> {
        let _ = options.lazy_structure; // reserved for M4
        catch_unwind(AssertUnwindSafe(|| {
            let worker = worker();
            let document_id = Engine::document_id_for(&bytes);
            let doc = worker.open(bytes).map_err(NibError::from)?;
            let page_count = worker.page_count(doc).map_err(NibError::from)?;
            Ok(Arc::new(NibDocument {
                worker,
                doc: Mutex::new(Some(doc)),
                engine: Mutex::new(Engine::new(document_id, page_count)),
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
            self.engine
                .lock()
                .unwrap()
                .apply(ops)
                .map(to_report)
                .map_err(to_nib_error)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in apply".into())))
    }

    /// Undoes the last step; returns the applied inverse ops as JSON.
    pub fn undo(&self) -> Option<String> {
        catch_unwind(AssertUnwindSafe(|| {
            self.engine
                .lock()
                .unwrap()
                .undo()
                .map(|ops| serde_json::to_string(&ops).unwrap_or_else(|_| "[]".into()))
        }))
        .unwrap_or(None)
    }

    /// Redoes the last undone step; returns the applied ops as JSON.
    pub fn redo(&self) -> Option<String> {
        catch_unwind(AssertUnwindSafe(|| {
            self.engine
                .lock()
                .unwrap()
                .redo()
                .map(|ops| serde_json::to_string(&ops).unwrap_or_else(|_| "[]".into()))
        }))
        .unwrap_or(None)
    }

    pub fn can_undo(&self) -> bool {
        self.engine.lock().unwrap().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.engine.lock().unwrap().can_redo()
    }

    /// Opens an explicit undo group; returns its groupId.
    pub fn begin_group(&self) -> String {
        self.engine.lock().unwrap().begin_group().to_string()
    }

    /// Closes the group opened by `begin_group` as one undo step.
    pub fn end_group(&self) {
        self.engine.lock().unwrap().end_group();
    }

    /// Serialises the delta document (ops serialised as JSON).
    pub fn export_delta(&self, mode: ExportMode) -> String {
        let mode = match mode {
            ExportMode::Full => delta::ExportMode::Full,
            ExportMode::Compacted => delta::ExportMode::Compacted,
        };
        self.engine.lock().unwrap().export_json(
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
            let opts = delta::ImportOptions {
                force: options.force,
                rebase: options.rebase,
                undoable: options.undoable,
            };
            self.engine
                .lock()
                .unwrap()
                .import(doc, opts)
                .map(to_report)
                .map_err(to_nib_error)
        }))
        .unwrap_or_else(|_| Err(NibError::Internal("panic in import_delta".into())))
    }

    /// Annotations on `page` in z-order.
    pub fn annotations(&self, page: u32) -> Vec<AnnotationSummary> {
        catch_unwind(AssertUnwindSafe(|| {
            self.engine
                .lock()
                .unwrap()
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
        self.engine
            .lock()
            .unwrap()
            .model()
            .annotation(&id)
            .map(|a| serde_json::to_string(a).unwrap_or_else(|_| "{}".into()))
    }

    /// Topmost annotation id hit at `pt` (user space), or none.
    pub fn hit_test(&self, page: u32, pt: Point, tolerance: f64) -> Option<String> {
        self.engine
            .lock()
            .unwrap()
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
