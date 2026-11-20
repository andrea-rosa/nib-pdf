# Nib PDF — System & Engine Architecture

> Status: **Design baseline v1** (2026-11-12). This document is the authoritative
> reference for the engine design. Milestone-specific decisions that refine it
> are recorded in [`ROADMAP.md`](./ROADMAP.md); the wire format is specified in
> [`JSON_SCHEMA.md`](./JSON_SCHEMA.md).

---

## 1. Goals, non-goals and constraints

### 1.1 Goals

| # | Goal |
|---|------|
| G1 | Render arbitrary PDFs on Android at interactive frame rates (60/120 FPS pan/zoom) with bounded memory. |
| G2 | Create, edit and delete annotations (`Ink`, `Highlight`, `Underline`, `StrikeOut`, `FreeText`, `Text`/sticky note, `Stamp`, `Square`, `Circle`, `Line`). |
| G3 | Capture handwritten signatures and apply visual e-signatures; apply and verify cryptographic digital signatures (PAdES B-B). |
| G4 | Read, fill and write AcroForm fields (text, checkbox, radio, combo/list). |
| G5 | Track every modification as a structured **JSON delta** that can be exported, imported, replayed and undone. |
| G6 | Write PDFs whose annotations and form values render **identically** in Adobe Acrobat, Chrome (PDFium), iOS/macOS Preview and Firefox (pdf.js), by always emitting Appearance Streams. |
| G7 | 100% permissive-licensed dependency tree (see §12 for the single documented exception). |

### 1.2 Non-goals (v1)

- Editing page content streams (text/image editing of the original page).
- Encrypted / password-protected PDFs (lopdf cannot incrementally save them; v1 rejects them at open time with `NibError::EncryptedUnsupported`).
- XFA forms (PDFium builds used are non-XFA, non-V8).
- Redaction, OCR, page reordering/merging (can be layered on later via lopdf).
- iOS/desktop targets (the Rust core is portable by construction, but only Android bindings are shipped).

### 1.3 Hard constraints

| Constraint | Value |
|---|---|
| Core language | Rust, edition 2021, MSRV 1.88 (dictated by `lopdf`) |
| Rendering | `pdfium-render` 0.9.x (`pdfium_8125` API flag) + prebuilt PDFium `chromium/8125` (bblanchon) — binary tag and feature flag are pinned together in `nib-rs/tools/pdfium.lock` / `nib-rs/Cargo.toml` (M0 changed the design-time `chromium/8152` to `8125` so the flag matches the binary exactly and the pin is ≥ 7 days old) |
| Structural writes | `lopdf` 0.47.x |
| Interop | UniFFI 0.32.x (proc-macro mode) |
| Android | minSdk 26, targetSdk 36, compileSdk 37 (required by Compose BOM 2026.09), ABIs `arm64-v8a`, `armeabi-v7a`, `x86_64` |
| UI | Kotlin + Jetpack Compose (BOM-managed) |
| Licenses | MIT / Apache-2.0 / BSD-3 / ISC; MPL-2.0 accepted **only** for UniFFI (unmodified) |

---

## 2. Monorepo layout

```
nib-pdf/
├── docs/                         # This directory (design docs, next-session prompt)
│   ├── ARCHITECTURE.md
│   ├── JSON_SCHEMA.md
│   ├── ROADMAP.md
│   └── NEXT_PROMPT.md
├── nib-rs/                       # Cargo workspace (Rust core)
│   ├── Cargo.toml                # [workspace] members below, shared [workspace.dependencies]
│   ├── rust-toolchain.toml       # pinned stable toolchain
│   ├── deny.toml                 # cargo-deny license allow-list
│   ├── nib-core/                 # Document model, delta engine, undo/redo, /AP generation, writer
│   │   ├── src/
│   │   │   ├── model/            # serde types (annotations, fields, signatures)   -> JSON_SCHEMA.md
│   │   │   ├── delta/            # Op, Command, UndoManager, validation, compaction
│   │   │   ├── geom/             # Rect, Matrix, PageTransform, QuadPoints, Bezier fitting
│   │   │   ├── ap/               # Appearance-stream generators (one module per subtype)
│   │   │   ├── writer/           # lopdf: incremental save, /Annots CRUD, AcroForm sync
│   │   │   └── lib.rs
│   │   ├── schema/               # JSON Schema (draft 2020-12) for the delta format
│   │   └── tests/fixtures/       # small PDFs (<200 KB each), golden JSON
│   ├── nib-render/               # pdfium-render wrapper: engine singleton, tile renderer, form-fill env
│   ├── nib-sign/                 # CMS/PAdES: ByteRange, placeholder, SignedData, verification
│   ├── nib-ffi/                  # UniFFI surface only (thin facade over the three crates above)
│   │   ├── src/lib.rs            # #[uniffi::export] items
│   │   └── uniffi.toml           # Kotlin package name, cdylib name
│   └── tools/
│       ├── fetch-pdfium.sh       # downloads + sha256-verifies PDFium tgz per ABI into nib-android/src/main/jniLibs
│       └── gen-bindings.sh       # uniffi-bindgen generate --library ... --language kotlin
├── nib-android/                  # Gradle library module -> AAR  (io.github.<org>.nib:nib-android)
│   ├── build.gradle.kts          # cargo-ndk task, uniffi bindgen task, Compose, publishing
│   ├── consumer-rules.pro        # keep rules for JNA + generated bindings
│   └── src/
│       ├── main/
│       │   ├── jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/{libpdfium.so,libnib_ffi.so}
│       │   ├── java/.../nib/
│       │   │   ├── NibDocument.kt          # Kotlin facade (coroutines, Flows) over generated bindings
│       │   │   ├── render/                 # TileCache, TileScheduler, BitmapPool
│       │   │   ├── ui/                     # NibPdfView, PageLayer, InkOverlay, FormOverlay, SelectionOverlay
│       │   │   ├── gesture/                # zoom/pan/stylus input, palm rejection hooks
│       │   │   └── sign/                   # Signer interface + KeyStoreSigner
│       │   └── AndroidManifest.xml
│       ├── test/                           # JVM unit tests (coordinate math, cache policy)
│       └── androidTest/                    # instrumentation tests (render, ink, forms, sign)
├── sample-app/                   # Demo app exercising every public API
├── .github/workflows/            # ci.yml (cargo test + lint, gradle assemble, emulator matrix), release.yml
├── LICENSE                       # Apache-2.0
├── THIRD_PARTY_LICENSES.md       # PDFium/FreeType/UniFFI/JNA notices
└── settings.gradle.kts           # includes :nib-android, :sample-app
```

Crate dependency graph (no cycles, `nib-ffi` is the only crate with `crate-type = ["cdylib"]`):

```
            ┌───────────┐
            │  nib-ffi  │  (UniFFI, cdylib: libnib_ffi.so)
            └─────┬─────┘
       ┌──────────┼──────────┐
       ▼          ▼          ▼
 ┌──────────┐ ┌──────────┐ ┌──────────┐
 │nib-render│ │ nib-core │ │ nib-sign │
 │(pdfium)  │ │ (lopdf)  │ │ (cms)    │
 └────┬─────┘ └────┬─────┘ └────┬─────┘
      │            │            │
      └──► nib-core::geom ◄─────┘   (shared geometry + error types live in nib-core)
```

`nib-render` depends on `nib-core` for geometry and the `Revision` type; `nib-core` never depends on PDFium so it compiles and tests on any host without native libraries.

---

## 3. Layered architecture

```
┌───────────────────────────────────────────────────────────────────────────┐
│ sample-app (Compose)                                                      │
├───────────────────────────────────────────────────────────────────────────┤
│ nib-android  (Kotlin / Compose)                                           │
│  ┌───────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────────────┐  │
│  │ NibPdfView    │ │ InkOverlay   │ │ FormOverlay  │ │ SelectionOverlay│  │
│  │ (LazyLayout)  │ │ (Canvas)     │ │ (TextField…) │ │ (handles)       │  │
│  └──────┬────────┘ └──────┬───────┘ └──────┬───────┘ └───────┬─────────┘  │
│         └─────────────────┴──────┬─────────┴─────────────────┘            │
│                      ┌───────────▼─────────────┐                          │
│                      │ NibDocumentState        │  viewport, zoom, dirty   │
│                      │ TileCache / TileScheduler│  tiles, undo/redo flows │
│                      └───────────┬─────────────┘                          │
│                      ┌───────────▼─────────────┐                          │
│                      │ NibDocument (facade)     │  suspend fns, Flows     │
│                      └───────────┬─────────────┘                          │
│                      generated Kotlin (uniffi)  │  JNA → libnib_ffi.so    │
├──────────────────────────────────┼────────────────────────────────────────┤
│ nib-ffi (Rust, UniFFI exports)   ▼                                        │
│  ┌──────────────────────────────────────────────────────────────────────┐ │
│  │ Document (Arc<Mutex<Inner>>)                                         │ │
│  │   ├─ revision: Arc<[u8]>            ◄── single source of truth       │ │
│  │   ├─ core:   nib_core::Session      (model + deltas + undo + writer) │ │
│  │   ├─ render: nib_render::Renderer   (pdfium doc opened from revision)│ │
│  │   └─ sign:   nib_sign::Signer glue  (callback interface to Kotlin)   │ │
│  └──────────────────────────────────────────────────────────────────────┘ │
├───────────────────────────────────────────────────────────────────────────┤
│ nib-render ──► pdfium-render ──► libpdfium.so (dlopen, BSD-3)             │
│ nib-core   ──► lopdf (MIT)        nib-sign ──► cms / x509-cert / sha2     │
└───────────────────────────────────────────────────────────────────────────┘
```

Responsibilities by layer:

| Layer | Owns | Never does |
|---|---|---|
| Compose UI | gestures, overlay drawing, native form widgets, bitmap presentation | PDF parsing, geometry math beyond matrix application |
| Kotlin facade | coroutine dispatch, tile cache/scheduler, Bitmap pooling, lifecycle | mutation of document state outside the delta API |
| `nib-ffi` | FFI types, locking, callback marshalling | business logic |
| `nib-core` | model, deltas, undo/redo, validation, /AP generation, lopdf writes | rendering, I/O policy (it is handed bytes, returns bytes) |
| `nib-render` | pdfium lifetime, page/tile rendering, text extraction, form-fill environment | writing PDFs |
| `nib-sign` | ByteRange, CMS build/verify, certificate chain checks | key custody (keys stay in Kotlin / KeyStore) |

---

## 4. Threading and concurrency model

PDFium is **not thread-safe**: all `FPDF_*` calls for a process must be serialized. `pdfium-render`'s `thread_safe` feature wraps bindings in a global mutex, but relying on it leads to lock convoys when the UI thread and a render worker contend. Nib therefore uses an explicit model:

```
Kotlin                               Rust (nib-ffi / nib-render)
──────                               ─────────────────────────────
Main thread ──(UniFFI call)──►  Document API (non-blocking: enqueue + return)
                                        │
Dispatchers.Default ◄── Flow ◄── RenderWorker (1 OS thread, owns PdfiumEngine)
   │  (tiles as ByteBuffer)             │  bounded priority queue (BinaryHeap)
   ▼                                    │  cancellation by (generation, viewport) check
Bitmap.copyPixelsFromBuffer             │  pdfium Document handle(s) per revision
```

Rules:

1. **One `PdfiumEngine` per process**, created lazily, `Send + !Sync`, owned by the `RenderWorker` thread. All PDFium access happens on that thread. Text extraction and form-field enumeration are also routed through it (as low-priority jobs).
2. `nib-core` operations (apply delta, undo, generate /AP, lopdf save) run on the **caller's thread** under `Mutex<Inner>`; they are pure CPU and complete in < 5 ms for typical ops. Saves (incremental write) run on a Kotlin IO dispatcher via a blocking UniFFI call.
3. The **revision swap** (new bytes after a lopdf save) is a message to the `RenderWorker`: it opens the new pdfium document, keeps the previous one alive until all in-flight tiles for the old generation drain (max 2 documents resident), then drops it.
4. UniFFI **async**: tile requests are exposed as `async fn render_tile(...) -> TileResult` (UniFFI async with a Rust oneshot channel), consumed as Kotlin `suspend fun`. Cancellation: Kotlin cancels the coroutine → UniFFI drops the future → the worker observes a dropped sender and skips rendering if not yet started.
5. Priority: `visible > prefetch ring > thumbnail`, ties broken by distance from viewport centre; a job is skipped if its `generation < current_generation` (generation bumps on zoom-bucket change or revision swap).

---

## 5. Dual-layer rendering pipeline

```
          ┌──────────────────────────── Compose Box (page coordinates) ───────────────┐
          │  Layer B: Overlay Canvas                                                  │
          │   • in-progress ink stroke (raw MotionEvent, pressure, historical pts)    │
          │   • uncommitted annotations (model → Compose Path)  ◄── "not yet flushed" │
          │   • selection handles, FreeText caret, form widgets (native Compose)      │
          ├───────────────────────────────────────────────────────────────────────────┤
          │  Layer A: Tile grid of Bitmaps from PDFium                                │
          │   • page content + flushed annotations (FPDF_ANNOT) + widgets (FFLDraw)   │
          │   • 512 px tiles, zoom buckets, LRU by bytes                              │
          └───────────────────────────────────────────────────────────────────────────┘
```

### 5.1 Layer A — tile rendering

**Tile key**: `TileKey { page: u16, bucket: i8, tx: u16, ty: u16, revision: u32, flags: u8 }` where `flags` encodes `render_annotations`, `render_forms`, `grayscale`.

**Zoom buckets**: `scale = 2^(bucket/2)` (√2 steps). Visible scale `s` maps to `bucket = round(2·log2(s))`. While a pinch is in progress, the UI draws the nearest cached bucket with a `drawImage(scale)` transform; the exact bucket is requested when the gesture settles (`debounce 80 ms`) or as soon as the scale ratio exceeds 1.25.

**Rendering a tile (Rust)**:

```rust
pub struct TileRequest {
    pub key: TileKey,
    pub device_rect: IRect,        // tile rect in page-device pixels at this bucket
    pub page_device_size: ISize,   // whole page size in device px at this bucket
    pub generation: u64,
}

pub struct TileResult {
    pub key: TileKey,
    pub width: u32, pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,           // RGBA (BGRA bitmap rendered with FPDF_REVERSE_BYTE_ORDER), opaque; `bytes` over UniFFI (M1)
    pub render_ms: u16,
}
```

**M1 note:** tiles are rendered into a `PdfBitmap::empty(w, h, BGRA)` with `PdfRenderConfig::set_target_size(page_device_size).set_origin(-x, -y).set_reverse_byte_order(true)`, so the raw buffer is **RGBA** and `Bitmap.copyPixelsFromBuffer` on an `ARGB_8888` bitmap needs no channel swizzle. `bump_generation()` runs synchronously under the queue lock and returns the new generation, which the Kotlin scheduler adopts (the worker generation is process-global). UniFFI resumes Kotlin continuations on the worker thread: JNA must keep that thread attached (see `Nib.init`) and the worker must never send a reply while holding the queue lock.

The worker calls `FPDF_RenderPageBitmapWithMatrix` (via `PdfRenderConfig::set_target_size / translate / scale`) with `FPDF_ANNOT | FPDF_LCD_TEXT` and then `FPDF_FFLDraw` with the shared form-fill environment so widget appearances are composited. Rendering a 512×512 tile of a text page takes ~4–12 ms on a 2022 mid-range SoC.

**Cache (Kotlin)**: `TileCache` is an `LruCache<TileKey, Bitmap>` sized by bytes (`min(1/8 app heap, 96 MB)`). Bitmaps are `ARGB_8888`, recycled through a `BitmapPool` keyed by dimensions. Eviction order: other revision → far pages → far buckets → far tiles. The cache is cleared on `onTrimMemory(TRIM_MEMORY_RUNNING_LOW)`.

**Invalidation**: a flushed delta carries `dirty: Vec<(page, Rect)>` in user space. The worker converts each to device space per cached bucket and invalidates only intersecting tiles; everything else keeps the previous `revision` tag and remains valid (the cache treats `revision` as a *minimum*, not an equality, for tiles outside dirty regions).

**Prefetch**: ring of one tile around the visible set, plus the first row of the next page in scroll direction, at the current bucket only.

### 5.2 Layer B — interactive overlay

- **Ink capture** uses `pointerInteropFilter`/`awaitPointerEventScope` with `MotionEvent.getHistoricalX/Y/Pressure` to recover sub-frame samples; stylus vs finger is distinguished via `TOOL_TYPE_STYLUS`; a `palmRejection` hook lets the host app ignore touch when a stylus is hovering.
- Points are accumulated in **screen space** during the gesture and drawn with `drawPath` on each frame (cheap; path is incremental). On `ACTION_UP` the stroke is transformed to **PDF user space**, smoothed (see §5.4), and submitted as a `create Ink` op.
- The overlay keeps drawing the committed-but-not-yet-visible annotation until a tile with `revision ≥ commitRevision` covering its bbox arrives — this guarantees no flicker between "overlay" and "baked" representations.
- **Form widgets** are native Compose composables (`BasicTextField`, checkbox / radio canvases, material3 `DropdownMenu` for combo/list) positioned at the widget `/Rect` transformed to screen space (`FormOverlay`, active only while `tool == Form`). **M4 decision (overlay covers):** the overlay box is a transparent hit-target while the field is committed and every covering tile is at `revision ≥ commitRevision` (same gate as annotations); it becomes *opaque* (`/MK /BG` or white fill, `/MK /BC` border, native content) while the field is focused, uncommitted, or not yet re-rendered — so the pdfium-drawn widget underneath is never visible at the same time as the native one. The two alternatives from the design were rejected: `FPDFAnnot_SetFlags(HIDDEN)` is a pdfium mutation (forbidden by §6), and the per-page "render without `FPDF_FFLDraw`" variant (kept as `TileKey.flags` bit `NO_FORMS = 2`, `NO_ANNOTATIONS = 1`) would re-render every tile of the page on each focus/blur.
- **Selection/move/resize** manipulate a transient copy; the `update` op is emitted once on gesture end.

### 5.3 Coordinate spaces

```
   Screen px  ──(1)──►  View space (dp·density, scroll, zoom)  ──(2)──►  Page device px
                                                                             │
                                                                            (3)
                                                                             ▼
                                                          PDF user space (points, y-up, /CropBox, /Rotate)
```

Definitions (all 3×3 affine matrices, row-vector convention `[x y 1]·M`):

| Step | Matrix | Notes |
|---|---|---|
| (1) Screen → View | `T(-scrollX, -scrollY) · S(1/zoom)` | `zoom` is the continuous scale; View space has page origins laid out by `PageLayout` (vertical, gap = 8 dp). |
| (2) View → Page device | `T(-pageOriginX, -pageOriginY) · S(bucketScale·density/72)` | Produces pixel coords in the page bitmap at the active bucket. |
| (3) Page device → User space | `inverse(DeviceMatrix)` where `DeviceMatrix = T(-cropX0, -cropY0) · R(rotate) · S(k, -k) · T(0, H_dev)` | `k = bucketScale·density/72` pt→px; `R` handles `/Rotate` ∈ {0,90,180,270}; the `S(k,-k)` flips y because PDF is y-up. Equivalent to `FPDF_PageToDevice`/`FPDF_DeviceToPage`. |

```rust
/// One instance per (page, bucket); cached in nib-core::geom.
pub struct PageTransform {
    pub crop_box: Rect,          // user space, from /CropBox ∩ /MediaBox
    pub rotate: Rotation,        // R0 | R90 | R180 | R270
    pub scale: f32,              // device px per PDF point
    device_from_user: Matrix,    // precomputed
    user_from_device: Matrix,
}
impl PageTransform {
    pub fn to_device(&self, p: Point) -> Point;
    pub fn to_user(&self, p: Point) -> Point;
    pub fn rect_to_user(&self, r: Rect) -> Rect;   // normalizes min/max after rotation
}
```

Rotation table (device size for a page with crop box `w×h` points):

| `/Rotate` | device width | device height | user→device for point `(x,y)` |
|---|---|---|---|
| 0 | `k·w` | `k·h` | `(k(x−x0), k(y1−y))` |
| 90 | `k·h` | `k·w` | `(k(y−y0), k(x−x0))` |
| 180 | `k·w` | `k·h` | `(k(x1−x), k(y−y0))` |
| 270 | `k·h` | `k·w` | `(k(y1−y), k(x1−x))` |

All geometry stored in the JSON delta and in the PDF is in **user space**; the Kotlin layer never persists device coordinates. Property tests assert `to_user(to_device(p)) ≈ p` within 1e-3 pt for all rotations and random crop boxes.

### 5.4 Ink smoothing

Raw samples → (a) **resample** to ≥ 1.5 pt spacing, (b) **Catmull-Rom → cubic Bézier** conversion (tension 0.5) for display and for the `/AP` stream, (c) optional **Ramer-Douglas-Peucker** simplification (tolerance 0.3 pt) to bound point count. The raw points *and* the smoothing parameters are stored in the delta (see `JSON_SCHEMA.md §5.1`) so the appearance is reproducible and the original data is never lost. `/InkList` in the PDF always receives the raw (resampled) points for compatibility; the smooth curve lives in `/AP`.

---

## 6. pdfium ⇄ lopdf coexistence

Two independent parsers of the same PDF must never disagree about what the document *is*. Nib enforces this with one invariant:

> **The document is an immutable `Arc<[u8]>` revision. lopdf is the only writer; PDFium is a read-only consumer that is re-pointed at each new revision.**

```
                    ┌──────────────────────────────┐
   open(path) ───►  │ Revision 0: Arc<[u8]>         │
                    └──────┬─────────────────┬──────┘
                           │                 │
                  lopdf::Document         pdfium load_from_bytes
                  (parsed once,           (RenderWorker, read-only)
                   kept in memory)
                           │
   deltas ──► nib-core model (in-memory, overlay-rendered) ──► flush()
                           │
                  IncrementalDocument::save_to(Vec<u8>)
                           │
                    ┌──────▼───────────────────────┐
                    │ Revision 1: Arc<[u8]>         │ ──► pdfium reopen (≈ 2–20 ms), dirty tiles
                    └──────────────────────────────┘
```

Consequences and mechanisms:

1. **No file locking**: the file is read once into memory (`File::read_to_end` or Android `ParcelFileDescriptor` → bytes). Saving writes to `<name>.tmp` then `rename`s (atomic on the same filesystem). Content-URI targets go through `ContentResolver.openOutputStream` from Kotlin with the bytes returned by Rust.
2. **No state desync**: there is no path by which PDFium mutates anything (`FPDFPage_*`/`FPDFAnnot_*` setters are **never** called; the form-fill environment is used only for *drawing* and *reading*). lopdf's parsed tree is the *only* mutable structural state and it is derived from the same bytes PDFium sees.
3. **Write batching**: ink and typing produce dozens of ops per second; a lopdf round-trip per op is wasteful and would thrash tiles. Ops are applied to the `nib-core` model immediately (overlay-rendered), and `flush()` runs: on explicit save, before signing, before export-to-PDF, when the uncommitted op count ≥ 50, or after 2 s idle (configurable). Uncommitted state is always fully represented by the JSON delta, so a process death loses at most the un-flushed ops **if** the host persists the delta (the Kotlin facade exposes `exportDelta()` for `SavedStateHandle`).
4. **Incremental updates** preserve prior revisions byte-for-byte (M3 note: after every save the writer **re-parses the new bytes with lopdf** instead of mutating its in-memory tree — the reparse doubles as a validity check and keeps `IncrementalDocument::create_from` honest; cost is O(file) per flush and is on the M6 measurement list), which is mandatory for signature validity and cheap for large files. `IncrementalDocument::create_from(prev_bytes, prev_doc)` is used; modified objects are cloned into `new_document` via `opt_clone_object_to_new_document`. A **compaction** save (`Document::save_to` full rewrite, renumbered, object streams) is offered as an explicit "Save As (optimized)" and is forbidden when the document has existing signatures.
5. **Identity mapping**: every annotation/widget created by Nib carries its UUID in `/NM` (standard "annotation name" key, string). On open, existing annotations get a UUID derived deterministically from `(object number, generation)` unless `/NM` already holds a valid UUID. `nib-core` maintains `HashMap<Uuid, (ObjectId, PageIndex)>`.
6. **Reopen cost**: PDFium's `FPDF_LoadMemDocument` parses lazily; for a 200-page text PDF reopen is ~5 ms. The `RenderWorker` double-buffers documents so visible tiles from the previous revision remain until replacements arrive.
7. **xref-stream / object-stream inputs**: lopdf 0.47 reads them; its incremental writer emits a classic xref table trailer (`/Prev` pointing at the previous startxref) which is legal for hybrid files per ISO 32000-1 §7.5.8. M3 includes a spike test matrix (PDF 1.4 classic, 1.5 xref streams, linearized, pre-signed) and falls back to full rewrite for unsigned documents if a case fails.

---

## 7. Appearance Stream (/AP) generation

Readers differ wildly in how they synthesize appearances from annotation dictionaries (Acrobat regenerates some, Preview ignores `/IC`, pdf.js only recently handles `/Ink` without `/AP`). Nib therefore **always writes `/AP /N`** (no `/AP /D`, even for widgets — see the M4 rows below), and sets `/NeedAppearances false` on the AcroForm so viewers trust our streams.

### 7.1 Common form XObject skeleton

```
<< /Type /XObject /Subtype /Form /FormType 1
   /BBox [x0 y0 x1 y1]          % == /Rect of the annotation (user space)
   /Matrix [1 0 0 1 0 0]        % identity; we never rely on viewer-applied rotation
   /Resources << /ExtGState << /GS0 << /CA ca /ca ca /BM /Normal >> >>
                 /Font << … >> /XObject << … >> >>
   /Length n /Filter /FlateDecode >>
stream
/GS0 gs  … content …
endstream
```

Rules: colours via `rg`/`RG` (DeviceRGB) matching `/C` and `/IC`; opacity **only** via ExtGState (`/CA` on the annotation dictionary is also set, for readers that apply it themselves — both agree so double-application is not an issue because the ExtGState inside a form XObject is what's actually used). Content is generated by a small typed builder (`ap::ContentBuilder`) that emits operators with 3-decimal precision to keep streams small.

### 7.2 Per-type strategy

| Subtype | Dictionary keys written | `/AP /N` content | Pitfalls handled |
|---|---|---|---|
| **Ink** | `/InkList` (raw points), `/BS << /W w /S /S >>`, `/C`, `/CA` | `w J 1 j 1` round cap/join; Catmull-Rom cubics `c` (tension τ: control arm `(P2−P0)·τ/3`, endpoints duplicated — identical to `InkSmoothing.kt`); one subpath per stroke; variable-width strokes (`pressureSensitive=true`) are emitted as **per-segment round-capped strokes** (`w` per segment, width `w·lerp(minWidthFactor 0.4, maxWidthFactor 1.6, avg pressure)`) — pixel-identical to the Compose overlay (M3 changed this from "filled polygon outlines") | `/BBox` padded by `w·maxWidthFactor/2 + 1`; zero-length stroke → dot via `l` to same point with round cap |
| **Highlight** | `/QuadPoints`, `/C`, `/CA` | `/GS0` with `/BM /Multiply`; one `re f` per quad (quad → axis-aligned rect, or 4-point path for rotated text) | `/Rect` = union of quads; multiply blend makes text remain visible in Preview/Chrome |
| **Underline** | `/QuadPoints` | line at `y0 + 0.07·h`, width `0.06·h`, per quad | Baseline fraction configurable |
| **StrikeOut** | `/QuadPoints` | line at `(y0+y1)/2`, width `0.06·h` | — |
| **Squiggly** | `/QuadPoints` | zig-zag path amplitude `0.1·h` | optional type |
| **FreeText** | `/DA` (`/Helv 12 Tf 0 0 0 rg`), `/Contents`, `/Q`, `/BS`, `/IC`, `/CL`+`/IT /FreeTextCallout`+`/LE` for callouts (`/RC` omitted) | border + fill, clip `re W n`, `BT /Helv size Tf … Tj ET` with greedy word-wrap by built-in AFM widths (`ap::fonts`, 12 Base-14 faces: `/Helv /HeBo /HeOb /HeBO /TiRo /TiBo /TiIt /TiBI /Cour /CoBo /CoOb /CoBO`, `/Encoding /WinAnsiEncoding`); first baseline `top − padding − 0.75·size`, line height `1.2·size`. **M3 status:** non-WinAnsi characters are written as `?` and reported in `FlushReport.warnings`; the host `FontProvider` is consulted but embedding (full or subset TTF, `Identity-H`) is deferred — lopdf 0.47 `font_embedding` only offers a raw `FontData` builder | `/Rotate` via `/Matrix` not yet emitted (page rotation is handled by the viewer transform; the overlay and `/AP` agree) |
| **Text** (sticky) | `/Name /Comment`, `/Open`, `/Popup` | Fixed 20×20 vector icon (speech bubble), `/F 28` (Print+NoZoom+NoRotate) | Rect forced to 20×20 at anchor |
| **Stamp** | `/Name`, optional `/IT`, `/Rotate` | Image: `/XObject /Im0` with `/DCTDecode` (JPEG) or `/FlateDecode` + `/SMask` (PNG alpha), `q W 0 0 H 0 0 cm /Im0 Do Q`; Vector: translated content ops; Standard: predefined text-in-rounded-rect | Images downsampled to ≤ 2048 px; sRGB ICC not embedded (DeviceRGB) |
| **Square** | `/IC`, `/BS /W`, `/BE` (none) | `re` with `B`/`S`/`f` depending on fill/stroke; inset by `w/2` | Dash via `/BS /D` and `[…] 0 d` |
| **Circle** | same | 4 Béziers, κ = 0.5523 | — |
| **Line** | `/L [x1 y1 x2 y2]`, `/LE`, `/LL`, `/LLE` | `m l S` + line-ending shapes (OpenArrow, ClosedArrow, Circle, Square, Diamond, Butt) as sub-paths | `/Rect` padded for endings |
| **Widget – text** | `/V` (PDFDoc or UTF-16BE text string), `/DA` (added as `/Helv 0 Tf 0 g` when missing), `/MK`, `/Q` | `/MK /BG` fill + `/MK /BC` border (inset w/2, dashed per `/BS /D`), then `/Tx BMC q <clip> BT /Helv size Tf … ET Q EMC`; `/DA` size 0 = auto (12 pt shrinking by 0.5 until width and `1.2·size` height fit, floor 4); single line vertically centred (`baseline = mid − 0.35·size`), multiline = FreeText wrap, comb = `inner_w/MaxLen` cells with centred glyphs, password = `*` per char | `/NeedAppearances false` on the AcroForm; `/DR /Font /Helv` + `/ZaDb` ensured; the XObject carries its own `/Resources /Font` so `/DR` is never relied upon; no `/GS0` (opacity 1) |
| **Widget – checkbox/radio** | `/V /<export>` or `/Off`, `/AS` on every kid (derived by the engine from `value` vs `widgets[].exportValue`, incl. unison groups) | `/AP /N << /<export> <stream> /Off <stream> >>`; checkbox = rect frame + ZapfDingbats `4` (advance 0.756) at `min(w,h)·0.8`; radio = **circle** frame + ZapfDingbats `l` (advance 0.791) at `min(w,h)·0.5`. **M4: no `/AP /D`** (readers only show it on mouse-down; the design-time "inverted background" variant was dropped) | Export value names from existing `/AP /N` keys preserved |
| **Widget – combo/list** | `/V` (string or array), `/I` (indices; removed when none) | combo: label of the selected option (fallback export / free text), single-line text layout; list: rows from `/TI`, row height `1.2·size` (size 0 → 12), selected rows filled `#99C1DA` under the text, clipped | pdf.js renders its own HTML widgets and shows an editable-combo free-text value only if it is in `/Opt` |
| **Widget – signature** | `/FT /Sig`, `/V → sig dict` | Adobe layer pattern: `/FRM` → `/n0` (empty) + `/n2` (visual: ink/image + name/date text) | Acrobat warns if `/n0`/`/n2` missing in some versions; followed for compatibility |

### 7.3 Verification matrix (manual, each milestone that touches /AP)

| Reader | What to check |
|---|---|
| Adobe Acrobat Reader (desktop + Android) | No "appearance regenerated" prompt; colours/opacity identical; form values visible without clicking |
| Chrome / Edge (PDFium) | Same pixel output as our tiles (same engine) |
| iOS/macOS Preview | Highlights multiply correctly; FreeText font renders; stamps not stretched |
| Firefox (pdf.js) | Ink curves, widgets `/AS` respected |
| Google Drive viewer | Flattened appearance correct |

Automated proxy (CI): (a) structural validation of every written file with `qpdf --check` (Apache-2.0 CLI, dev-only); (b) render the written `/AP` with PDFium and compare against an independent reference raster of the same annotation model drawn with `tiny-skia` (BSD-3) using a perceptual-hash threshold. No AGPL tooling is used, even as a dev dependency.

---

## 8. Digital signature flow

The digital signing flow below is planned for the remaining M5 cycles; it is
not implemented by the visual-signature feature.

```
Kotlin                         nib-ffi / nib-sign                         PDF bytes
──────                         ──────────────────                         ─────────
prepareSignature(spec, cert) ─► flush()                                     rev N
                               privately stage sig field+widget+/AP
                               /V << /Type /Sig /Filter /Adobe.PPKLite
                                     /SubFilter /ETSI.CAdES.detached
                                     /ByteRange [0 0 0 0]  (placeholder, fixed width)
                                     /Contents <0000…0000> (16 KiB hex, fixed) >>
                               incremental save to private staged bytes
                               compute real ByteRange, patch in place
                               PDF digest = SHA-256(bytes[a..a+b] ‖ bytes[c..c+d])
                               build canonical DER SET OF signed attributes:
                                 contentType, PDF messageDigest,
                                 ESS signing-certificate-v2 (leaf certificate hash)
                 ◄──────────── SignRequest { signedAttributesDer, algorithm, … }
Signer.sign(signedAttributesDer)  (Android KeyStore / PKCS#11 / remote HSM)
   returns {signature, certChain} ────► detached CMS SignedData
                               verify CMS and staged bytes; reject stale revision
                               patch /Contents, atomically publish ─────────► rev N+1 (signed)
```

Design points:

- **Keys never enter Rust.** `Signer` is a UniFFI callback interface implemented in Kotlin; the reference implementation uses `android.security.keystore` (RSA-PSS or ECDSA P-256 with SHA-256). For raw signatures the signer certificate or expected fingerprint is needed before preparing the CAdES attributes. Android `SHA256with...` signs the DER attribute bytes, not the already-hashed PDF digest. Remote services may instead provide a complete CMS blob.
- **Transactional preparation:** unsigned staged bytes stay private; compare the source revision and engine mutation token before publishing. Invoke host callbacks without writer or engine locks.
- **Placeholder size**: 16 KiB hex (8 KiB DER) accommodates 3-certificate chains; a `B-T` extension (RFC 3161 token in unsigned attrs) would need ~24 KiB and is reserved via config.
- **DocMDP**: optional certification signature (`/P 1|2|3`) when `spec.docMdp` is set and no prior signature exists.
- **Verification** (`verify_signatures()`): validate the signed revision's `/ByteRange`, CMS signature and digest; analyze later revisions separately. Report cryptographic integrity independently from certificate trust. Trust is not evaluated when no host anchors are supplied; certificate parsing alone is not path validation, and `/M` is an untrusted claimed time. No network revocation is planned for v1.
- **Visual e-signature without crypto**: an `Ink`/`Stamp` annotation flagged `intent: "signature"` can optionally be bound to an empty signature field's widget `/Rect` without filling `/V`. The Form overlay exposes `onRequestVisualSignature` only for writable, unambiguously identified empty widgets. The sample stores captured marks only for the current session; see `JSON_SCHEMA.md §6.1`.

---

## 9. Public API surface (preview)

Rust (`nib-ffi`, UniFFI proc-macros):

```rust
#[derive(uniffi::Object)]
pub struct NibDocument { inner: Mutex<Inner> }

#[uniffi::export]
impl NibDocument {
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>, options: OpenOptions) -> Result<Arc<Self>, NibError>;
    pub fn page_count(&self) -> u32;
    pub fn page_info(&self, index: u32) -> PageInfo;            // crop box, rotate, label
    pub async fn render_tile(&self, req: TileRequest) -> Result<TileResult, NibError>;
    pub fn apply(&self, ops_json: String) -> Result<ApplyReport, NibError>;   // validates + applies + pushes undo
    pub fn undo(&self) -> Option<String>;                        // returns inverse ops applied (JSON)
    pub fn redo(&self) -> Option<String>;
    pub fn export_delta(&self, mode: ExportMode) -> String;      // Full | Compacted
    pub fn import_delta(&self, json: String) -> Result<ApplyReport, NibError>;
    pub fn annotations(&self, page: u32) -> Vec<AnnotationSummary>;
    pub fn hit_test(&self, page: u32, pt: Point, tolerance: f32) -> Option<String>;
    pub fn form_fields(&self) -> Vec<FormField>;
    pub fn flush(&self) -> Result<FlushReport, NibError>;        // lopdf incremental save → new revision
    pub fn bytes(&self) -> Vec<u8>;                              // current revision
    pub fn prepare_signature(&self, spec: SignatureSpec) -> Result<SignRequest, NibError>;
    pub fn complete_signature(&self, token: SignToken, result: SignResult) -> Result<(), NibError>;
    pub fn verify_signatures(&self, anchors: Vec<Vec<u8>>) -> Vec<SignatureStatus>;
    pub fn text_in_rect(&self, page: u32, rect: Rect) -> String; // for highlight quad snapping
}

#[uniffi::export(callback_interface)]
pub trait Signer: Send + Sync {
    fn sign(&self, request: SignRequest) -> Result<SignResult, NibError>;
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum NibError { Parse(String), EncryptedUnsupported, InvalidDelta(String),
    RevisionMismatch { expected: u32, got: u32 }, SignatureLocked, Io(String), Pdfium(String) }
```

Kotlin facade (hand-written, thin):

```kotlin
class NibDocument private constructor(private val raw: nib.NibDocument) : Closeable {
    val pageCount: Int
    val undoAvailable: StateFlow<Boolean>; val redoAvailable: StateFlow<Boolean>
    val dirtyRegions: SharedFlow<List<DirtyRegion>>
    suspend fun renderTile(req: TileRequest): TileResult          // Dispatchers.Default
    fun apply(ops: List<DeltaOp>): ApplyReport                      // kotlinx.serialization → JSON
    suspend fun save(target: OutputStream)                         // flush + write on IO
    suspend fun sign(spec: SignatureSpec, signer: Signer)
}

@Composable
fun NibPdfView(state: NibDocumentState, modifier: Modifier = Modifier,
               tool: Tool = Tool.None, onAnnotationSelected: (String?) -> Unit = {})
```

JSON is used across the FFI for ops (schema-validated on the Rust side) because it is already the persistence format, avoids duplicating ~40 record types in UDL, and the payloads are small (< 10 KB typical). Hot paths (tiles) use typed records and `bytes`.

---

## 10. Memory, errors, cancellation, size

- **Memory ceilings**: tile cache ≤ 96 MB; ≤ 2 PDFium documents; lopdf `Document` for a 50 MB scanned PDF is ~1.2× file size (streams kept compressed, `load_mem` with `Document::load_filtered` not needed). Large documents (> 100 MB) are opened with `OpenOptions { lazy_structure: true }` which parses only the page tree and defers `/Annots` to first access.
- **Errors** cross the FFI as `NibError` (typed, non-fatal) — never panics. `nib-ffi` wraps each export in `catch_unwind` to convert any panic into `NibError::Internal` and logs via `android_logger`.
- **Cancellation**: all long operations are UniFFI async and respect coroutine cancellation (§4).
- **Binary size**: `libpdfium.so` ≈ 3.2 MB (arm64), `libnib_ffi.so` ≈ 1.5 MB with `opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `strip = true`; `panic = "unwind"` is kept (not `abort`) because `nib-ffi` relies on `catch_unwind` to convert panics into `NibError`. 16 KB page alignment: PDFium 64-bit prebuilts are aligned (the `armeabi-v7a` prebuilt is 4 KB-aligned, which is fine — the 16 KB requirement applies to 64-bit ABIs only, and `tools/check-elf-alignment.sh` exempts ELF32); our `cdylib` is linked with `-Wl,-z,max-page-size=16384` via `cargo-ndk` ≥ 3.5 defaults. Note that `strip = true` removes the UniFFI metadata symbols from release `.so` files, so Kotlin bindings are generated from the host **debug** `libnib_ffi` (bindings are target/profile independent).
- **R8**: `consumer-rules.pro` keeps `com.sun.jna.**`, generated `uniffi.nib_ffi.**` and the callback interface implementations.
- **Loading order**: Kotlin calls `System.loadLibrary("pdfium")` **before** `System.loadLibrary("nib_ffi")`; Rust then calls `Pdfium::bind_to_library("libpdfium.so")` (already mapped, dlopen is a no-op returning the same handle). The system `/system/lib64/libpdfium.so` is inaccessible to apps since Android N and is never used.

---

## 11. Testing strategy (summary; details per milestone in ROADMAP.md)

| Level | Tooling | Runs on |
|---|---|---|
| Rust unit | `cargo test`, `proptest` (geometry, undo/redo laws), `insta` (snapshot of generated content streams) | CI Linux (uses `pdfium-linux-x64` for `nib-render` tests) |
| Rust golden render | PDFium render of fixtures → pHash compare (`image` + `img_hash`) | CI Linux |
| Rust fuzz | `cargo-fuzz` on `import_delta` and lopdf open paths | nightly job |
| Kotlin JVM | JUnit5 + `kotlinx-coroutines-test` for cache/scheduler/transform | CI |
| Android instrumentation | `androidx.compose.ui.test`, `UiAutomator` gestures, `Macrobenchmark` for scroll jank | emulator matrix API 26/30/34, x86_64 |
| Interop | `openssl cms -verify` on signed outputs; external-reader manual matrix (§7.3) | CI (openssl) / release checklist |

---

## 12. Third-party licenses

| Component | License | Shipped in AAR? | Notes |
|---|---|---|---|
| PDFium (`libpdfium.so`) | BSD-3-Clause | yes | Bundles FreeType (FTL, BSD-style), libjpeg-turbo (IJG/BSD), libopenjpeg (BSD-2), zlib, ICU (Unicode/ICU). Notices reproduced in `THIRD_PARTY_LICENSES.md`. |
| `pdfium-render` | MIT OR Apache-2.0 | yes (static in `libnib_ffi.so`) | elect Apache-2.0 |
| `lopdf` | MIT | yes | |
| `uniffi` / `uniffi_core` | **MPL-2.0** | yes (runtime code in `libnib_ffi.so`; generated Kotlin is **not** MPL) | Accepted exception: used unmodified; MPL is file-level and imposes nothing on Nib's own files. Documented prominently. |
| JNA (`net.java.dev.jna:jna@aar`) | Apache-2.0 OR LGPL-2.1+ | yes (transitive) | elect Apache-2.0 |
| RustCrypto (`cms`, `x509-cert`, `der`, `rsa`, `p256`, `sha2`, `pkcs8`) | Apache-2.0 OR MIT | yes | |
| `serde`, `serde_json`, `thiserror`, `uuid`, `image`, `flate2` | MIT/Apache-2.0 | yes | |
| AndroidX / Compose / Kotlin | Apache-2.0 | transitive | |
| Dev-only: `cargo-ndk`, `cargo-deny`, `proptest`, `insta`, `criterion`, `img_hash` | permissive | no | |

`cargo deny check licenses` runs in CI with `allow = ["MIT","Apache-2.0","BSD-2-Clause","BSD-3-Clause","ISC","Unicode-3.0","Zlib"]` and an explicit `exceptions` entry for `uniffi*` → `MPL-2.0`. Any AGPL/GPL/LGPL crate fails the build.
