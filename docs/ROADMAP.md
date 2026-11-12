# Nib PDF — Phased Development Roadmap

> **Living document.** This file is updated at the end of **every** development cycle
> (see §0). Status legend: `[ ]` not started · `[~]` in progress · `[x]` done · `[!]` blocked.
> Design references: [`ARCHITECTURE.md`](./ARCHITECTURE.md), [`JSON_SCHEMA.md`](./JSON_SCHEMA.md).
> The prompt for the next session is always in [`NEXT_PROMPT.md`](./NEXT_PROMPT.md).

| Milestone | Title | Status | Cycle(s) | Notes |
|---|---|---|---|---|
| M0 | Repository scaffold, toolchain & CI | `[x]` | 2026-11-12 | PDFium pinned to `chromium/8125` (not 8152); CI proven locally, first remote run pending push |
| M1 | Core viewer: tiles, zoom, coordinates | `[ ]` | — | |
| M2 | JSON delta engine & overlay annotations | `[ ]` | — | |
| M3 | PDF writer & appearance streams | `[ ]` | — | |
| M4 | AcroForms | `[ ]` | — | |
| M5 | Signatures (visual + PAdES) | `[ ]` | — | |
| M6 | Hardening, performance, release | `[ ]` | — | |

Dependency graph:

```
M0 ──► M1 ──► M2 ──► M3 ──► M4 ──► M5 ──► M6
              │             ▲
              └── (M2 overlay rendering is reused by M4 form overlay and M5 signature capture)
```

---

## 0. Cycle protocol (mandatory for every Devin session)

A **cycle** is one Devin session that executes one milestone (or a well-defined slice of it). Every cycle MUST end with these steps, in order:

1. **Verify DoD.** Run the milestone's automated test criteria; paste the exact commands + a one-line result summary into the cycle log (§8).
2. **Update this file.**
   - Flip the status cell in the summary table and in the milestone section.
   - Fill in the **Cycle log** (§8): date, what was done, what was deferred, deviations from the design docs and why, newly discovered risks.
   - If the design changed, update `ARCHITECTURE.md` / `JSON_SCHEMA.md` and reference the change in the log.
3. **Rewrite `NEXT_PROMPT.md`** so that it contains a complete, self-contained prompt for the *next* session: goal, scope, links to the sections of these docs to read first, exact DoD, known pitfalls carried over, and the verification commands. The next session must be able to start with zero conversational context.
4. **Commit** with message `cycle(Mx): <summary>` and leave the tree clean (`git status` empty). Do not push unless asked.

Rules of engagement for every cycle:

- Read `docs/NEXT_PROMPT.md` first, then the referenced sections — not the whole corpus.
- Pin new dependencies to versions published ≥ 7 days before the session date; record the version in the milestone's dependency table below.
- Never downgrade license policy (`deny.toml`) to make CI pass — stop and report.
- Keep fixtures small (< 200 KB per PDF; no copyrighted documents).
- Any milestone slice that cannot be finished is marked `[~]` with a precise "remaining" list, never silently dropped.

---

## 1. Milestone M0 — Repository scaffold, toolchain & CI

**Status:** `[x]` (cycle 2026-11-12)

### Goals & scope
- Create the monorepo exactly as in ARCHITECTURE §2: `nib-rs/` Cargo workspace (`nib-core`, `nib-render`, `nib-sign`, `nib-ffi`), `nib-android/`, `sample-app/`, `.github/workflows/`.
- `tools/fetch-pdfium.sh`: download `pdfium-android-{arm64,arm,x64}.tgz` for tag `chromium/8152` from bblanchon, verify SHA-256 against a checked-in `pdfium.lock` file, extract `libpdfium.so` into `nib-android/src/main/jniLibs/<abi>/`. Also fetch `pdfium-linux-x64.tgz` into `nib-rs/target/pdfium/` for host tests.
- `cargo-ndk` integration in `nib-android/build.gradle.kts` building `libnib_ffi.so` for the 3 ABIs (debug: only the host emulator ABI to save time).
- UniFFI bindgen Gradle task (`uniffi-bindgen generate --library`) writing Kotlin into `build/generated/uniffi`.
- A single "hello" export: `fn nib_version() -> String` and `fn pdfium_version() -> String` (the latter loads PDFium via `Pdfium::bind_to_library` to prove the dlopen path works on device).
- `cargo-deny` license policy, `rustfmt`, `clippy -D warnings`, `ktlint`/`detekt` (baseline), EditorConfig.
- CI: `ci.yml` with jobs `rust` (fmt, clippy, test on ubuntu with linux PDFium), `android-build` (assemble AAR + sample), `android-test` (emulator API 34 x86_64 running instrumentation tests). Caching for cargo and gradle.
- `LICENSE` (Apache-2.0), `THIRD_PARTY_LICENSES.md`, `README.md` (status badge, one-paragraph description, "not ready" banner), `AGENTS.md` with build/test commands.

Out of scope: any rendering beyond the version call, any UI beyond a text label in the sample app.

### Dependencies
| Rust crate | Version (pinned 2026-11-12; all ≥ 7 days old) | Purpose |
|---|---|---|
| Rust toolchain | `1.99.1` (2026-10-15) via `rust-toolchain.toml` | MSRV of lopdf is 1.88 |
| `uniffi` | `0.32.2` (2026-09-23); `features = ["cli"]` only in the `uniffi-bindgen` bin crate | bindings |
| `pdfium-render` | `0.9.5` (2026-11-01), `default-features = false`, `features = ["pdfium_8125"]` ¹ | version call |
| PDFium binary | bblanchon `chromium/8125` (2026-10-26), SHA-256 in `nib-rs/tools/pdfium.lock` | dlopen'd at runtime |
| `lopdf` | `0.47.0` (2026-11-04), `features = ["serde"]` | present in workspace; unused yet |
| `thiserror` `2.0.21`, `log` `0.4.35`, `android_logger` `0.15.1` (android only) | | |
| Dev: `cargo-ndk` `4.1.2`, `cargo-deny` `0.20.2` | | |

¹ pdfium-render 0.9.5 exposes `pdfium_5961 … pdfium_7881, pdfium_8125, pdfium_future`. The newest flag ≤ the bundled binary is `pdfium_8125`; the binary was moved from the design-time `chromium/8152` (published 2026-11-10, too new for the 7-day rule) to `chromium/8125` so flag and binary match exactly. Verified on device: `pdfium_version()` returns `pdfium ok (api pdfium_8125, nib-render 0.1.0, FPDF_GetLastError=0)`.

| Android | Version |
|---|---|
| AGP / Gradle | `9.5.0` (2026-10-28) / `9.8.2` (2026-10-19) — AGP 9 built-in Kotlin, new DSL; no `kotlin-android` plugin |
| Kotlin | `2.4.21` (only `org.jetbrains.kotlin.plugin.compose` applied); Compose BOM `2026.09.00`; `activity-compose` `1.13.0` |
| `net.java.dev.jna:jna:5.19.1@aar` | UniFFI runtime (generated Kotlin needs no kotlinx-coroutines in M0) |
| `org.jlleitschuh.gradle.ktlint` `14.2.0` | Kotlin lint (`ktlintCheck` in CI) |
| androidx.test `ext:junit 1.3.0`, `runner 1.7.0`, `core 1.7.0`, junit `4.13.2` | instrumentation |
| compileSdk 37 (forced by Compose BOM aarMetadata), targetSdk 36, minSdk 26, NDK `29.0.14206865` | |
| CI actions: checkout v7.0.1, Swatinem/rust-cache v2.9.3, cargo-deny-action v2.1.1, setup-java v6.2.0, android-actions/setup-android v4.0.4, actions/cache v5.1.0, gradle/actions/setup-gradle v6.4.1, android-emulator-runner v2.38.0, upload-artifact v7.0.2 | |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| PDFium feature-flag vs binary mismatch causes symbol-missing at runtime | `pdfium_version()` smoke test on emulator in CI; pin both together in `pdfium.lock`. |
| 16 KB page alignment rejected by Play | `cargo-ndk` passes `-Wl,-z,max-page-size=16384`; CI runs `check_elf_alignment.sh` (from AOSP) on all `.so`. |
| JNA + R8 stripping | `consumer-rules.pro` committed in M0 and exercised by a minified sample build in CI. |
| Emulator flakiness in CI | `reactivecircus/android-emulator-runner` with API 34 x86_64, retries = 2. |

### Definition of Done
- [x] `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` passes locally (macOS arm64 host PDFium); Linux CI job `rust` is defined, first remote run pending push.
- [x] `cargo deny check` passes with the documented allow-list + per-crate `uniffi*` MPL-2.0 exceptions (plus one advisory ignore, see cycle log).
- [x] `./gradlew :nib-android:assembleRelease` produces an AAR with exactly `jni/{arm64-v8a,armeabi-v7a,x86_64}/{libpdfium.so,libnib_ffi.so}`; `tools/check-elf-alignment.sh` passes (64-bit LOAD align `0x4000`; 32-bit exempt).
- [x] `./gradlew :nib-android:connectedDebugAndroidTest` on AVD `nib_api34` (API 34, arm64-v8a): `NibSmokeTest.versionsAreReported` 1/1 passed.
- [x] `./gradlew :sample-app:assembleRelease` (R8 minified, consumer rules only) installs and shows `nib 0.1.0` and `pdfium ok (api pdfium_8125, nib-render 0.1.0, FPDF_GetLastError=0)`.
- [x] `AGENTS.md` documents every command above.

### Automated tests
- Rust: `nib_ffi::tests::version_matches_cargo`.
- Android instrumentation: `NibSmokeTest.versionsAreReported`.
- CI script: ELF alignment check over `jniLibs/**/*.so`.

---

## 2. Milestone M1 — Core viewer: tiles, zoom, coordinates

**Status:** `[ ]`

### Goals & scope
- `nib-render`: `PdfiumEngine` singleton on a dedicated `RenderWorker` thread (ARCHITECTURE §4); open from bytes; `page_count`, `page_info` (crop box, rotation, label); `render_tile` async with priority queue, generation-based cancellation, double-buffered documents (prepared for revision swaps even though M1 has no writes).
- `nib-core::geom`: `Rect`, `Matrix`, `PageTransform` with the rotation table from ARCHITECTURE §5.3; `QuadPoints` normalisation.
- `nib-ffi`: `NibDocument.from_bytes`, `page_count`, `page_info`, `render_tile`, `text_in_rect` (needed later for highlight snapping; cheap to add now), `NibError`.
- Kotlin: `NibDocument` facade, `TileCache` (byte-budget LRU + `BitmapPool`), `TileScheduler` (visible → prefetch ring), `NibPdfView` composable with vertical page layout, pinch-zoom (0.5×–8×), fling, double-tap-to-zoom, zoom buckets with nearest-bucket upscaling during gestures, `onTrimMemory` hook.
- Sample app: open a PDF via `ACTION_OPEN_DOCUMENT`, display it.

Out of scope: annotations, forms, writing, text selection UI.

### Dependencies
| Rust | Purpose |
|---|---|
| `pdfium-render` (image feature **off**; we return raw BGRA) | rendering |
| `crossbeam-channel` or `std::sync::mpsc` + `BinaryHeap` | worker queue |
| `tokio` **not** used; UniFFI async via `oneshot` futures (`futures-channel`) | async tiles |
| Dev: `proptest`, `image` + `img_hash` (golden tests) | |

| Android | Purpose |
|---|---|
| `androidx.compose.foundation` (gestures, `LazyLayout`), `androidx.collection:collection-ktx` (`LruCache`) | UI/cache |
| `kotlinx-coroutines-android` | dispatch |
| Dev: `androidx.compose.ui:ui-test-junit4`, `androidx.benchmark:benchmark-macro-junit4` | tests |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| PDFium calls from multiple threads crash | All PDFium access confined to `RenderWorker`; `PdfiumEngine: !Sync` enforced by type; debug assertion on thread id. |
| Tile copy cost across FFI (Vec<u8> → ByteBuffer → Bitmap) | 512×512×4 = 1 MB per tile; JNA direct `ByteBuffer` + `Bitmap.copyPixelsFromBuffer` ≈ 0.3 ms. Measure; if needed, M6 switches to `AHardwareBuffer` via a tiny JNI shim. |
| OOM on large zoom | Byte-budgeted cache; cap bucket at 8× page scale; drop other buckets first. |
| Rotated pages / odd crop boxes | Property tests over all 4 rotations and random crop boxes; fixtures include rotated pages. |

### Definition of Done
- [ ] Rust: golden-render tests for 5 fixtures (text, vector, image, rotated 90, cropbox offset) pass with pHash distance ≤ 4 against committed PNGs.
- [ ] Rust: `PageTransform` property tests (`to_user(to_device(p)) ≈ p`, rect normalisation) pass for 10k cases.
- [ ] Rust: cancellation test — enqueue 200 tiles, bump generation, assert ≤ 5 old-generation tiles rendered.
- [ ] Android: `NibPdfViewTest` opens a 100-page fixture, scrolls to end and back with `performTouchInput { swipeUp() }` ×20, asserts no OOM and the last page's first tile is rendered (semantics node `tile:99:0:0` exists).
- [ ] Android: `ZoomTest` pinches to 4×, asserts bucket = 4 and that a tile at bucket 4 is requested within 300 ms after gesture end.
- [ ] Macrobenchmark: scroll jank `frameOverrunMs P90 < 8 ms` on API 34 emulator (recorded as baseline, not gating yet).
- [ ] Sample app opens any PDF from the system picker.

### Automated tests
- Rust: `nib_render::tests::{golden_*, cancel_stale_generation, page_info_rotation}`, `nib_core::geom::tests::{transform_roundtrip, rotation_table}`.
- Kotlin JVM: `TileCacheTest` (eviction order, byte budget), `ZoomBucketTest`.
- Android: `NibSmokeTest`, `NibPdfViewTest`, `ZoomTest`, `ScrollBenchmark`.

---

## 3. Milestone M2 — JSON delta engine & overlay annotations

**Status:** `[ ]`

### Goals & scope
- `nib-core::model`: all `serde` types from JSON_SCHEMA §2–§7 (signature/form types included even though their *apply* paths arrive in M4/M5 — the schema ships complete).
- `nib-core::delta`: `Op`, validation (all error codes in JSON_SCHEMA §3), merge-patch apply, inverse derivation, `UndoManager` with grouping/coalescing, `Full`/`Compacted` export, import with idempotency and `rebase`.
- Dirty-region computation per command.
- JSON Schema file `nib-core/schema/nib-delta.schema.json` generated from the Rust types (`schemars`) and checked in; CI fails if regenerated output differs.
- `nib-ffi`: `apply`, `undo`, `redo`, `export_delta`, `import_delta`, `annotations(page)`, `hit_test`, `begin_group`/`end_group`.
- Kotlin: `kotlinx.serialization` mirror of the op types (generated-by-hand, kept small); `InkOverlay` (stylus/finger capture with historical samples, pressure, live path), `AnnotationOverlay` (draws every uncommitted/committed annotation type from the model in Compose — this is the **reference renderer** that `/AP` output is compared against in M3), `SelectionOverlay` (select, move, resize, delete), tool palette in sample app (ink, highlight via text snapping, shapes, line, free text, sticky note, image stamp).
- Highlight creation uses `text_in_rect`/pdfium text page char boxes to snap quads to lines.

Out of scope: writing anything to the PDF (annotations vanish on reopen — by design at this stage), form editing, signatures.

### Dependencies
| Rust | Purpose |
|---|---|
| `serde`, `serde_json` (`preserve_order`), `uuid` (`v4`, `v7`, `serde`), `chrono` (`serde`) | model |
| `json-patch` (MIT/Apache) for RFC 7386 merge patch, or a 40-line in-house implementation (preferred: fewer deps) | update ops |
| `schemars` | JSON Schema generation |
| `kurbo` (Apache/MIT) | Bézier fitting, RDP simplification, bbox |
| Dev: `proptest`, `insta` | laws, snapshots |

| Android | Purpose |
|---|---|
| `kotlinx-serialization-json` | ops |
| `androidx.compose.ui:ui-graphics` (`Path`, `drawPath`) | overlay |
| Dev: `androidx.test.uiautomator` | gesture injection with pressure |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Undo/redo inverse bugs | Property-based laws (JSON_SCHEMA §8) over random op sequences; `insta` snapshots of exports. |
| Ink latency (touch-to-pixel) | Draw raw path immediately on the overlay; smoothing only on gesture end; measure with `Choreographer` frame callbacks in tests. |
| Highlight quads on rotated/vertical text | Use pdfium char boxes + rotation-aware merge; fixtures with rotated page. |
| Schema drift between Rust and Kotlin | Kotlin side treats payload as opaque `JsonObject` except for the few fields the UI edits; Rust validates everything. |

### Definition of Done
- [ ] `cargo test -p nib-core` includes ≥ 30 unit tests and 4 property tests (laws 1–4) passing.
- [ ] `schema/nib-delta.schema.json` validates all examples in `JSON_SCHEMA.md` (a test extracts the fenced JSON blocks).
- [ ] Round-trip: every example in `JSON_SCHEMA.md` deserializes → serializes → deserializes to an equal value.
- [ ] Android `InkTest`: injects a 60-point stylus gesture with UiAutomator, asserts one `create Ink` op with ≥ 40 points after resampling and `smoothing.algorithm == "catmullRom"`.
- [ ] Android `UndoTest`: draw, highlight, move, undo ×3, redo ×3 — overlay semantics tree matches expected annotation count at each step.
- [ ] Android `HighlightSnapTest`: drag across two lines of the text fixture, assert 2 quads whose heights match line heights ± 1 pt.
- [ ] Sample app: full tool palette functional; export JSON to a share sheet.

### Automated tests
- Rust: `delta::tests::{validate_*, inverse_*, coalesce_typing, compaction_equivalence (proptest), full_export_replay (proptest)}`, `model::tests::schema_examples_roundtrip`.
- Android: `InkTest`, `UndoTest`, `HighlightSnapTest`, `SelectionTest`.

---

## 4. Milestone M3 — PDF writer & appearance streams

**Status:** `[ ]`

### Goals & scope
- `nib-core::writer`: open with lopdf alongside pdfium (same bytes), `IncrementalDocument` save, `/Annots` CRUD with object cloning, `/NM` UUID round-trip, import of pre-existing annotations into the model on open (with `appearance.regenerate=false` + `apHash`), image XObject embedding (JPEG passthrough, PNG → Flate + SMask), atomic temp-file save helper, revision counter.
- `nib-core::ap`: appearance generators for all 11 annotation subtypes per ARCHITECTURE §7.2 incl. Base-14 fonts, word-wrap for FreeText, pressure-width ink outlines, line endings, multiply blend highlight. Font subsetting for non-WinAnsi FreeText via lopdf `font_embedding` + host `FontProvider` callback.
- `nib-render`: revision swap (`reopen(bytes)`), dirty-tile invalidation protocol.
- `nib-ffi`: `flush`, `bytes`, `FlushReport`, `OpenOptions.lazy_structure`, `FontProvider`/`AssetProvider` callback interfaces.
- Kotlin: debounce/flush policy (idle 2 s, ≥ 50 ops, explicit save), `save(OutputStream)`, "Save As (optimized)" full rewrite path, overlay-until-flushed handoff (no flicker).
- Spike + test matrix for incremental save over: PDF 1.4 classic xref, 1.5 xref streams + object streams, linearized, pre-signed (must remain valid), hybrid.

Out of scope: forms (M4), signatures (M5).

### Dependencies
| Rust | Purpose |
|---|---|
| `lopdf` (`serde`, `font_embedding`) | writing |
| `flate2` (MIT/Apache; `rust_backend` or `zlib-rs`) | stream compression |
| `image` (PNG/JPEG decode for metadata & SMask split) | stamps |
| `kurbo` | AP path math |
| Dev: `insta` (content-stream snapshots), `img_hash`, `tempfile` | |
| Dev CI tool (not linked): `qpdf --check` (Apache-2.0) for structural validation | |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| lopdf incremental save fails on some xref-stream/hybrid files | Test matrix fixtures in CI; fallback to full rewrite when `has_signatures == false`; report `FlushReport.mode`. |
| Appearance mismatch across readers | Rust re-renders the written `/AP` with pdfium and compares it to an independent **Rust reference raster** of the model drawn with `tiny-skia` (BSD-3), pHash ≤ 6. The Compose overlay from M2 implements the same drawing rules, so one spec drives three renderers. Manual matrix (ARCHITECTURE §7.3) run before marking M3 done. |
| Fonts: non-Latin FreeText | Host-provided TTF via `FontProvider`; subset + `Identity-H`; fixture with Cyrillic + CJK text. |
| File size growth with incremental saves | Each flush is one revision; the debounce policy keeps revisions ≤ dozens per session; "Save As (optimized)" compacts. |
| Reopen flicker | Overlay keeps drawing until tile `revision ≥ commitRevision` (M2 overlay already model-driven). |

### Definition of Done
- [ ] For each of the 11 subtypes: write → `lopdf` reload → structural assertions (`insta` snapshot of annotation dict + `/AP` content stream) pass.
- [ ] `qpdf --check` passes on every written fixture in CI.
- [ ] Render parity: pdfium render of written `/AP` vs `tiny-skia` reference raster, pHash ≤ 6, for all subtypes and 3 opacities.
- [ ] Incremental matrix: 5 input classes × create/update/delete → reload in pdfium and lopdf both succeed; pre-signed fixture's signature `/ByteRange` bytes unchanged.
- [ ] Existing annotations imported on open round-trip through `export_delta(Compacted)` with zero ops when untouched.
- [ ] Android `PersistenceTest`: draw ink + highlight + stamp, `save()`, reopen bytes, assert annotation count and that tiles render them (pHash vs pre-save tiles ≤ 6).
- [ ] Android `NoFlickerTest`: after `flush()`, overlay semantics for the annotation disappears only after the covering tile reports `revision ≥ commitRevision`.
- [ ] Manual reader matrix checklist completed and recorded in the cycle log.

### Automated tests
- Rust: `writer::tests::{incremental_*, annots_crud, nm_roundtrip, import_existing}`, `ap::tests::{snapshot_<subtype>, parity_<subtype>}`, CI step `qpdf --check`.
- Android: `PersistenceTest`, `NoFlickerTest`, `SaveAsOptimizedTest`.

---

## 5. Milestone M4 — AcroForms

**Status:** `[ ]`

### Goals & scope
- `nib-core`: field tree parsing from lopdf (`/AcroForm /Fields`, inheritance of `/FT`, `/Ff`, `/DA`, `/Q`, `/V` through parents), fully-qualified names, widget enumeration; `formField` op application (all `fieldType`s in JSON_SCHEMA §7), validation codes, `/AP` regeneration for text/checkbox/radio/combo/list widgets, `/NeedAppearances false`, optional **flatten** (bake widget `/AP` into page content via `Do` and remove fields) as `FlushReport.flattened`.
- `nib-render`: form-fill environment (`FPDFDOC_InitFormFillEnvironment`, `FPDF_FFLDraw`) for rendering widgets; `FPDF_RenderPageBitmap` flag control to hide the active widget; read-only access to field values for cross-checks in tests.
- `nib-ffi`: `form_fields()`, `set_field_value` convenience (wraps an `update formField` op), `flatten` option.
- Kotlin: `FormOverlay` — native Compose widgets (`BasicTextField`, `Checkbox`, `RadioButton`, dropdown) positioned via `PageTransform`; keyboard/IME handling with scroll-into-view; tab order from `/Fields` order; sample app form mode.

Out of scope: JavaScript actions/calculations, XFA, field authoring UI (create ops are supported by the engine but not by the sample UI).

### Dependencies
| Rust | Purpose |
|---|---|
| `lopdf`, `kurbo` | as before |
| Dev: public-domain form fixtures (generate our own with lopdf in a `build-fixtures` test; do **not** vendor IRS/government PDFs unless verified public domain) | |

| Android | Purpose |
|---|---|
| `androidx.compose.material3` (dropdown, checkbox) | widgets |
| Dev: `androidx.compose.ui:ui-test` text input APIs | |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Inherited attributes & merged field/widget dicts | Dedicated resolver with unit tests for 3-level hierarchies and merged dicts. |
| Radio groups with duplicate export values / `radiosInUnison` | Model keeps per-kid export values; tests cover unison. |
| Active widget double-draw (pdfium + Compose) | Spike both strategies (ARCHITECTURE §5.2); choose by measuring; record decision in cycle log. |
| Auto font size (`/DA` size 0) | Implement Acrobat-like fit: shrink from 12 pt until text fits; snapshot-test. |
| Comb fields, max-len, multiline wrapping | Dedicated AP tests per flag. |

### Definition of Done
- [ ] Rust: generated fixture form (text ×3 incl. multiline/comb/password, checkbox ×2, radio group ×3 kids, combo editable + non-editable, list multi) → set every value via ops → reload with lopdf **and** read back via pdfium form-fill API → all values equal.
- [ ] Rust: `/AP` snapshots for each widget type and state (`On`/`Off`, focused `/D`).
- [ ] Rust: flatten test — after flatten, `/AcroForm` absent, page render pHash ≤ 4 vs un-flattened render with widgets.
- [ ] Rust: validation tests for `FIELD_READONLY`, `VALUE_NOT_IN_OPTIONS`, `MAXLEN_EXCEEDED`.
- [ ] Android `FormFillTest`: type into text field, toggle checkbox, pick radio & combo, `save()`, reopen, `formFields()` returns the values; tiles render the values (pHash check on widget rect).
- [ ] Android `ImeScrollTest`: focusing a field near the bottom scrolls it above the IME.
- [ ] Undo/redo works across form edits (laws from M2 extended to `formField` target).

### Automated tests
- Rust: `forms::tests::{hierarchy_resolution, set_values_roundtrip, radio_unison, ap_snapshots, flatten_parity, validation_codes}`.
- Android: `FormFillTest`, `ImeScrollTest`, `FormUndoTest`.

---

## 6. Milestone M5 — Signatures (visual e-signature + PAdES digital)

**Status:** `[ ]`

### Goals & scope
- Visual e-signature (JSON_SCHEMA §6.1): `intent: "signature"`, `signer` metadata, `bindToField` placement into an empty `/Sig` widget, signature capture sheet in sample app (draw / type / image), reusable stored signatures (host-side).
- `nib-sign`: signature field + widget creation, `/V` sig dict with placeholders, incremental save, exact `/ByteRange` patching, digest, CMS `SignedData` (detached, ESS `signing-certificate-v2`, no `signingTime` for CAdES; optional for `adbe.pkcs7.detached`), hex patching into `/Contents`, DocMDP `/Perms`, field `/Lock`, self-verification after signing.
- Verification: `verify_signatures()` — ByteRange coverage, digest, CMS signature (`rsa` PSS/PKCS1v15, `p256`/`p384` ECDSA), chain building to host-supplied anchors (`x509-cert`), validity period checks, post-signature revision classification; `SignatureStatus { field, valid, coversWholeDocument, signerCn, signedAt, docMdp, modificationsAfter: AnnotationsOnly|FormsOnly|Content|None, errors[] }`.
- Signature appearance `/AP` layer pattern (`/FRM`, `/n0`, `/n2`) with graphic copied from the referenced `Ink`/`Stamp`.
- `nib-ffi`: `prepare_signature`, `complete_signature`, `verify_signatures`, `Signer` callback interface.
- Kotlin: `Signer` interface, `KeyStoreSigner` (Android KeyStore, `SHA256withRSA/PSS` and `SHA256withECDSA`), `sign(spec, signer)` suspend helper, signature status badge in sample app.
- Stretch (only if everything above is done): `level: "B-T"` — RFC 3161 request/response (`x509-tsp` crate or minimal DER) via a host `TimestampProvider` callback; unsigned attribute `signature-time-stamp`.

Out of scope: revocation (OCSP/CRL), LTV (B-LT/B-LTA), smartcard/PKCS#11 transport (host can implement `Signer`), MDP policy enforcement UI.

### Dependencies
| Rust | License | Purpose |
|---|---|---|
| `cms` (`builder` feature), `x509-cert` (`builder` for test certs), `der`, `spki`, `pkcs8`, `const-oid` | Apache-2.0/MIT | CMS |
| `rsa` (`sha2`), `p256`, `p384`, `ecdsa`, `sha2`, `signature` | Apache-2.0/MIT | verification + test signing |
| `rand_core` / `getrandom` | Apache-2.0/MIT | test keys |
| Dev CI tool: `openssl` CLI (`openssl cms -verify -binary -content …`) | Apache-2.0 | independent verification |

| Android | Purpose |
|---|---|
| `android.security.keystore` (platform) | `KeyStoreSigner` |
| Dev: `androidx.test` for KeyStore key generation on emulator | |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| `/ByteRange`/`/Contents` offsets off-by-one → invalid signature | Fixed-width ASCII placeholders written by our own serializer step after lopdf save; byte-level tests; `openssl cms -verify` in CI as an oracle. |
| Acrobat rejects CAdES attribute set | Follow ETSI EN 319 122-1 baseline B-B: `content-type`, `message-digest`, `signing-certificate-v2`; no `signingTime`; `/SubFilter /ETSI.CAdES.detached`. Validate with Acrobat + pdf.js + `pyHanko`-free CLI (`openssl`). Record manual results. |
| KeyStore padding/hash mismatch with declared `signatureAlgorithm` | `SignRequest` carries exact algorithm; `KeyStoreSigner` maps to `Signature.getInstance` names and verifies locally before returning. |
| Signed doc then edited → broken signature UX | Lock rules (JSON_SCHEMA §6.2); `SignatureStatus.modificationsAfter` surfaced in UI. |
| Placeholder too small for long chains | 16 KiB default; `SignatureSpec.contentsLength` override; error `PlaceholderTooSmall` with required size. |

### Definition of Done
- [ ] Rust: sign a fixture with an in-test RSA-2048 (PSS) key and a P-256 key → `verify_signatures()` valid; `openssl cms -verify` on the extracted CMS with the extracted ByteRange content succeeds in CI.
- [ ] Rust: tamper test — flip one byte inside ByteRange → `valid == false`, error `DigestMismatch`; append an annotation revision → `valid == true`, `modificationsAfter == AnnotationsOnly`.
- [ ] Rust: DocMDP `/P 1` + later form edit → `modificationsAfter == FormsOnly` and `docMdpViolated == true`.
- [ ] Rust: `/AP` snapshot for `graphicAndDescription` appearance; signature widget renders in pdfium.
- [ ] Rust: double-signature test (two fields, two revisions) — both valid; first `coversWholeDocument == false`, second `true`.
- [ ] Android `KeyStoreSignTest`: generate EC key in AndroidKeyStore, sign fixture via `KeyStoreSigner`, `verifySignatures()` valid.
- [ ] Android `VisualSignatureTest`: capture ink in the signature sheet bound to `Signature1`, assert the annotation rect equals the widget rect (contain-fit) and `locked == true`.
- [ ] Manual: Acrobat Reader shows "Signature is valid / signed by …" (trust manually set) for both algorithms; recorded in cycle log.

### Automated tests
- Rust: `nib_sign::tests::{sign_rsa_pss, sign_ecdsa_p256, byterange_exact, tamper_detected, incremental_after_sign, docmdp_violation, double_signature, appearance_snapshot}`; CI step `openssl cms -verify`.
- Android: `KeyStoreSignTest`, `VisualSignatureTest`, `SignatureBadgeTest`.

---

## 7. Milestone M6 — Hardening, performance & release

**Status:** `[ ]`

### Goals & scope
- Performance budgets (measured on API 34 x86_64 emulator as CI baseline and on a physical mid-range arm64 device manually): first visible page ≤ 300 ms from `from_bytes` for a 10 MB text PDF; tile P50 ≤ 12 ms / P95 ≤ 30 ms; scroll `frameOverrunMs` P90 ≤ 4 ms; ink touch-to-draw ≤ 1 frame; `flush()` with 50 ops ≤ 150 ms on a 20 MB PDF; memory ceiling respected under `monkey` stress.
- `criterion` benches for geometry, AP generation, delta apply/compaction; `Macrobenchmark` suites gating on regressions > 15 %.
- Fuzzing: `cargo-fuzz` targets `import_delta_json`, `open_bytes` (lopdf path), `ap_from_annotation`; run nightly; seed corpus from fixtures.
- Robustness: malformed PDFs (truncated xref, missing `/Annots`, broken `/AP`), huge pages (200 in), 1000-page docs with `lazy_structure`, low-memory callbacks, process death recovery via `exportDelta(Full)` in `SavedStateHandle` (sample app).
- Optional `AHardwareBuffer` tile transport if M1 measurements showed copy cost > 1 ms/tile.
- API freeze: `@Stable` Kotlin API review, KDoc for every public symbol, rustdoc `#![deny(missing_docs)]` in `nib-ffi`, `BinaryCompatibilityValidator` baseline.
- Docs: `README.md` quick-start, `docs/API_GUIDE.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`.
- Publishing: Maven Central (Sonatype Central Portal) via `com.vanniktech.maven.publish`, signed artifacts, `release.yml` tagging `v0.1.0`; reproducible `fetch-pdfium.sh` recorded in release notes with SHA-256s.

Out of scope: new features.

### Dependencies
| Rust | Purpose |
|---|---|
| Dev: `criterion`, `cargo-fuzz` (nightly), `cargo-llvm-cov` | perf, fuzz, coverage |

| Android | Purpose |
|---|---|
| `androidx.benchmark:benchmark-macro-junit4`, `androidx.profileinstaller` (baseline profiles) | perf |
| `org.jetbrains.kotlinx:binary-compatibility-validator`, `com.vanniktech:gradle-maven-publish-plugin` | API/publishing |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Emulator benchmarks noisy | Gate on relative regression with 3-run median; physical-device numbers recorded manually in the cycle log. |
| Fuzzer finds lopdf panics we cannot fix upstream quickly | `catch_unwind` boundary already in `nib-ffi`; report upstream; add regression fixtures. |
| Maven Central credentials | Lead-only action; CI uses repository secrets; dry-run `publishToMavenLocal` gates. |

### Definition of Done
- [ ] All perf budgets met on CI baseline; physical-device numbers recorded.
- [ ] 24 h fuzz run with zero crashes on all three targets (or all findings fixed with regression tests).
- [ ] Coverage ≥ 80 % lines in `nib-core`, ≥ 70 % in `nib-sign`/`nib-render` (`cargo-llvm-cov`).
- [ ] `apiCheck` passes; every public Kotlin/Rust symbol documented.
- [ ] `./gradlew publishToMavenLocal` produces a consumable AAR + sources + javadoc; a fresh sample project depending on it builds.
- [ ] `v0.1.0` tag + GitHub release notes drafted (publishing itself is a lead/user action).

### Automated tests
- Rust: `criterion` benches with saved baselines; fuzz targets; malformed-PDF regression tests.
- Android: `StartupBenchmark`, `ScrollBenchmark`, `InkLatencyBenchmark`, `ProcessDeathRecoveryTest`, `LowMemoryTest`.

---

## 8. Cycle log

Append one entry per session, newest first.

| Date | Milestone | Session summary | Deferred / remaining | Design deviations | New risks |
|---|---|---|---|---|---|
| 2026-11-12 | M0 | Scaffolded the monorepo per ARCHITECTURE §2: `nib-rs` workspace (nib-core, nib-render, nib-sign, nib-ffi, uniffi-bindgen), `deny.toml`, `tools/{fetch-pdfium.sh,pdfium.lock,gen-bindings.sh,check-elf-alignment.sh}`; `nib-android` (AGP 9.5.0, cargo-ndk + UniFFI bindgen Gradle tasks, JNA, consumer R8 rules, `Nib.kt`, `NibSmokeTest`), minified Compose `sample-app`, ktlint, `.github/workflows/ci.yml` (rust / android-build / android-test with emulator retry), LICENSE, THIRD_PARTY_LICENSES, README, AGENTS. DoD commands run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` → pass (2 tests); `cargo deny check` → pass; `./gradlew :nib-android:assembleRelease` + `check-elf-alignment.sh` on AAR → 6 `.so`, OK; `./gradlew :nib-android:connectedDebugAndroidTest` (AVD nib_api34) → 1/1 pass; `./gradlew :sample-app:assembleRelease` + `adb install` → both versions shown; `./gradlew ktlintCheck` → pass; `actionlint` → clean. | Remote CI has not run yet (no push this cycle) — first push must be watched. `detekt` not added (ktlint only). | (1) PDFium `chromium/8152` → `chromium/8125` + `pdfium_8125` flag (7-day rule; exact flag/binary match). (2) compileSdk 36 → 37 (Compose BOM 2026.09.00 aarMetadata), targetSdk stays 36. (3) `armeabi-v7a` PDFium prebuilt is 4 KB-aligned; alignment check exempts ELF32 (Google requirement is 64-bit only). (4) `deny.toml` ignores `RUSTSEC-2026-0374` (`basic-toml` unmaintained, reachable only via `uniffi_bindgen → askama`, bindgen-time only). (5) Kotlin bindings generated from host debug `libnib_ffi` because release `strip = true` removes UniFFI metadata. (6) Generated-source wiring uses `sourceSets.<v>.{jniLibs,kotlin}.directories.add` + explicit task deps (AGP `addGeneratedSourceDirectory` needs a task-owned `DirectoryProperty`, which requires `buildSrc`). (7) `fetch-pdfium.sh` also fetches `mac-arm64` for host tests on this machine. | cargo-ndk also emits `libpdfium_render-*.so` (pdfium-render ships a cdylib target) — the `copyRustJniLibs` Sync filters to `libnib_ffi.so` only; keep that filter when adding crates. `uniffi-bindgen generate` takes the lib path positionally (`--library` is a no-op since 0.31). `cargo ndk`/`cargo metadata` must run from inside `nib-rs/` (manifest-path is not honoured for metadata). Local emulator images are arm64-only (Apple Silicon) so x86_64 ABI is only exercised in CI. |
| 2026-11-12 | — (design) | Produced `ARCHITECTURE.md`, `JSON_SCHEMA.md`, `ROADMAP.md`, `NEXT_PROMPT.md`. Decisions: core dir `nib-rs`, UniFFI MPL-2.0 exception accepted, signatures = external-signer PAdES B-B + verification (B-T stretch), minSdk 26 / 3 ABIs. | All milestones | — | pdfium-render feature flag vs `chromium/8152` binary compatibility to be confirmed in M0. |
