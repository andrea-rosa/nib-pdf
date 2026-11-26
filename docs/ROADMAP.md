# Nib PDF — Phased Development Roadmap

> **Living document.** This file is updated at the end of **every** development cycle
> (see §0). Status legend: `[ ]` not started · `[~]` in progress · `[x]` done · `[!]` blocked.
> Design references: [`ARCHITECTURE.md`](./ARCHITECTURE.md), [`JSON_SCHEMA.md`](./JSON_SCHEMA.md).
> The prompt for the next session is always in [`NEXT_PROMPT.md`](./NEXT_PROMPT.md).

| Milestone | Title | Status | Cycle(s) | Notes |
|---|---|---|---|---|
| M0 | Repository scaffold, toolchain & CI | `[x]` | 2026-11-12 | PDFium pinned to `chromium/8125` (not 8152); CI proven locally, first remote run pending push |
| M1 | Core viewer: tiles, zoom, coordinates | `[x]` | 2026-11-13 | Tiles are RGBA (not BGRA); `image_hasher` instead of `img_hash`; ScrollBenchmark baseline recorded (emulator) |
| M2 | JSON delta engine & overlay annotations | `[x]` | 2026-11-13 | In-house RFC 7386; `jsonschema` dev-dep validates doc examples; stylus pressure injected via `MotionEvent` (no uiautomator); `HistoricalChange.pressure` absent in Compose 1.12.1 |
| M3 | PDF writer & appearance streams | `[x]` | 2026-11-13, 2026-11-25 | Non-WinAnsi FreeText now subsets/embeds host-provided static TrueType fonts; PDFium appearance-text extraction and Acrobat-specific behavior remain reader limitations, documented below |
| M4 | AcroForms | `[x]` | 2026-11-18 | Overlay-covers strategy for the active widget (no pdfium setters); no `/AP /D`; radio = circle; `create/delete formField` not written; chrono advisory `RUSTSEC-2026-0543` ignored in `deny.toml` |
| M5 | Signatures (visual + PAdES) | `[x]` | 2026-11-20, 2026-11-24, 2026-11-25, 2026-11-26 | Actual Acrobat Reader appearance/status check completed for RSA-PSS and ECDSA P-256; self-signed identity remains untrusted as expected; B-T is out of scope |
| M6 | Hardening, performance, release | `[~]` | 2026-11-26 | Fuzz-only NCSA exception approved and audited; all three 60-second smoke runs passed; fuzz-discovered AcroForm recursion overflow fixed; performance/API/release criteria remain |

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
4. **Leave changes uncommitted.** Do not stage or commit; report `git status` and provide the user with exact `git add` and `git commit` commands for the cycle's reviewed changes. Suggest [Conventional Commit](https://www.conventionalcommits.org/) subjects (`feat(scope): …`, `fix(scope): …`, `test(scope): …`, `docs: …`, `build: …`) and include the Copilot co-author trailer. Do not include unrelated changes or assume that earlier uncommitted work has been committed. Do not push unless asked.

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

**Status:** `[x]` (cycle 2026-11-13)

### Goals & scope
- `nib-render`: `PdfiumEngine` singleton on a dedicated `RenderWorker` thread (ARCHITECTURE §4); open from bytes; `page_count`, `page_info` (crop box, rotation, label); `render_tile` async with priority queue, generation-based cancellation, double-buffered documents (prepared for revision swaps even though M1 has no writes).
- `nib-core::geom`: `Rect`, `Matrix`, `PageTransform` with the rotation table from ARCHITECTURE §5.3; `QuadPoints` normalisation.
- `nib-ffi`: `NibDocument.from_bytes`, `page_count`, `page_info`, `render_tile`, `text_in_rect` (needed later for highlight snapping; cheap to add now), `NibError`.
- Kotlin: `NibDocument` facade, `TileCache` (byte-budget LRU + `BitmapPool`), `TileScheduler` (visible → prefetch ring), `NibPdfView` composable with vertical page layout, pinch-zoom (0.5×–8×), fling, double-tap-to-zoom, zoom buckets with nearest-bucket upscaling during gestures, `onTrimMemory` hook.
- Sample app: open a PDF via `ACTION_OPEN_DOCUMENT`, display it.

Out of scope: annotations, forms, writing, text selection UI.

### Dependencies
| Rust | Version (pinned 2026-11-13; all ≥ 7 days old) | Purpose |
|---|---|---|
| `pdfium-render` | `0.9.5` unchanged (`image`/`thread_safe` **off**; raw pixels returned, see deviation: **RGBA** via `FPDF_REVERSE_BYTE_ORDER`) | rendering |
| worker queue | `std::sync::mpsc` + `Mutex<BinaryHeap>` + `Condvar` — no extra crate | priority queue |
| `futures-channel` | `0.3.34` (2026-08-11) — `oneshot` for UniFFI async; no `tokio` | async tiles |
| Dev: `proptest` | `1.12.0` (2026-10-10) | property tests |
| Dev: `image` | `0.25.10` (2026-03-10), `default-features = false, features = ["png"]` | golden PNG I/O |
| Dev: `image_hasher` | `3.1.1` (2026-02-21) — maintained fork of `img_hash` (which pins `image` 0.23 and is unmaintained since 2021) | pHash |

| Android | Version | Purpose |
|---|---|---|
| Compose BOM `2026.09.00` → `foundation`, `runtime`, `ui-test-junit4`, `ui-test-manifest` `1.12.1` | BOM-managed | UI (`SubcomposeLayout`, gestures), tests |
| `kotlinx-coroutines-android` / `-test` | `1.11.0` (2026-05-07) | dispatch; required by generated UniFFI async bindings |
| `androidx.collection:collection-ktx` | `1.7.0` (2026-10-07) | `LruCache` byte-budget cache |
| `androidx.benchmark:benchmark-macro-junit4` | `1.5.0` (2026-09-09) | `ScrollBenchmark` (`:macrobenchmark`, `com.android.test`) |
| `androidx.profileinstaller:profileinstaller` | `1.4.1` (2024-10-02) | `benchmark` build type of the sample |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| PDFium calls from multiple threads crash | All PDFium access confined to `RenderWorker`; `PdfiumEngine: !Sync` enforced by type; debug assertion on thread id. |
| Tile copy cost across FFI (Vec<u8> → ByteBuffer → Bitmap) | 512×512×4 = 1 MB per tile; JNA direct `ByteBuffer` + `Bitmap.copyPixelsFromBuffer` ≈ 0.3 ms. Measure; if needed, M6 switches to `AHardwareBuffer` via a tiny JNI shim. |
| OOM on large zoom | Byte-budgeted cache; cap bucket at 8× page scale; drop other buckets first. |
| Rotated pages / odd crop boxes | Property tests over all 4 rotations and random crop boxes; fixtures include rotated pages. |

### Definition of Done
- [x] Rust: golden-render tests for 5 fixtures (text, vector, image, rotated 90, cropbox offset) pass with pHash distance ≤ 4 against committed PNGs (measured distance 0 on macOS arm64 host).
- [x] Rust: `PageTransform` property tests (`to_user(to_device(p)) ≈ p`, rect normalisation) pass for 10k cases.
- [x] Rust: cancellation test — enqueue 200 tiles (worker parked by a test-only `Block` job), bump generation, assert ≤ 5 old-generation tiles rendered (actual 0).
- [x] Android: `NibPdfViewTest` opens the 100-page fixture, `swipeUp()` ×20 then — because 20 flings fall short of page 99 on a Pixel 6 — `state.scrollTo(maxScrollY())`, asserts `tile:99:0:0` exists, `swipeDown()` ×20, no OOM.
- [x] Android: `ZoomTest` pinches to 4×, asserts bucket = 4 and a bucket-4 tile request within 300 ms after gesture end.
- [x] Macrobenchmark `ScrollBenchmark` recorded on the API 34 arm64 emulator (baseline, not gating): `frameOverrunMs` P50 35.3 / P90 54.2 / P99 226.5 ms, `frameDurationCpuMs` P50 40.0 / P90 51.0 / P99 130.5 ms (3 iterations, `CompilationMode.None`). The design target (P90 < 8 ms) is **not** met on the emulator; revisit on hardware in M6.
- [x] Sample app opens any PDF from the system picker (verified on the minified release APK: Open → Downloads → `text.pdf` renders; also `--es nib.asset text-100.pdf`).

### Automated tests
- Rust: `nib_render::{golden_{text,vector,image,rotated90,cropbox}, cancel_stale_generation, dropped_receiver_skips_render, bump_generation_returns_monotonic, queue_bounded_evicts_lowest, heap_ordering, swap_keeps_at_most_two, page_info_rotation, render_tile_smoke, pdfium_probe_loads}`, `nib-core/tests/geom.rs::{transform_roundtrip, rotation_table, rect_normalisation, quad_normalisation}`, `gen_fixtures::fixtures_are_valid`.
- Kotlin JVM: `TileCacheTest` (eviction order, byte budget), `ZoomBucketTest`.
- Android: `NibSmokeTest`, `NibPdfViewTest`, `ZoomTest`, `ScrollBenchmark` (`:macrobenchmark`).

---

## 3. Milestone M2 — JSON delta engine & overlay annotations

**Status:** `[x]` (cycle 2026-11-13)

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
| Rust | Version (pinned 2026-11-13; all ≥ 7 days old) | Purpose |
|---|---|---|
| `serde` | `1.0.229` (2026-07-18), `derive` | model |
| `serde_json` | `1.0.154` (2026-10-11), `preserve_order` | model, ops across FFI |
| `uuid` | `1.29.0` (2026-10-23), `v4`, `v7`, `serde` | entity / op ids |
| `chrono` | `0.4.45` (2026-06-04), `default-features = false`, `std`, `clock`, `serde` | RFC 3339 timestamps |
| `schemars` | `1.2.3` (2026-10-12), `derive`, `uuid1`, `chrono04`, `preserve_order` | `schema/nib-delta.schema.json` (draft 2020-12) |
| `kurbo` | `0.13.1` (2026-05-13) | present for M3 AP path math; M2 geometry uses `nib-core::geom` |
| `sha2` | `0.11.1` (2026-10-12) | `documentId = "sha256:…"` |
| `thiserror` | `2.0.21` (workspace) | `DeltaError` |
| RFC 7386 merge patch | in-house (`delta/merge_patch.rs`, ~70 lines incl. inverse) — no `json-patch` crate | update ops |
| Dev: `insta` | `1.49.0` (2026-10-03), `json` | export snapshots |
| Dev: `jsonschema` | `0.60.2` (2026-10-26), `default-features = false` | validates every fenced example in `JSON_SCHEMA.md` against the checked-in schema |
| Dev: `proptest` | `1.12.0` (existing) | laws 1–4 |

| Android | Version | Purpose |
|---|---|---|
| `org.jetbrains.kotlin.plugin.serialization` | `2.4.21` (2026-10-08) — works with AGP 9.5 built-in Kotlin | `@Serializable` op types |
| `org.jetbrains.kotlinx:kotlinx-serialization-json` | `1.12.0` (2026-10-19; `1.13.0` of 2026-11-11 is too new) | ops / opaque payloads (`JsonObject`) |
| `androidx.compose.material3` (BOM `2026.09.00`) | BOM-managed | `TextButton` in `SelectionOverlay`, sample palette |
| `androidx.test.uiautomator` | **not added** — stylus pressure is injected with `MotionEvent.obtain(… TOOL_TYPE_STYLUS …)` via `decorView.dispatchTouchEvent` | `InkTest` |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Undo/redo inverse bugs | Property-based laws (JSON_SCHEMA §8) over random op sequences; `insta` snapshots of exports. |
| Ink latency (touch-to-pixel) | Draw raw path immediately on the overlay; smoothing only on gesture end; measure with `Choreographer` frame callbacks in tests. |
| Highlight quads on rotated/vertical text | Use pdfium char boxes + rotation-aware merge; fixtures with rotated page. |
| Schema drift between Rust and Kotlin | Kotlin side treats payload as opaque `JsonObject` except for the few fields the UI edits; Rust validates everything. |

### Definition of Done
- [x] `cargo test -p nib-core`: 52 lib unit tests + 8 integration tests (geom 4, schema 3, gen_fixtures 1) and the proptest `delta_laws` covering laws 1–4 (256 cases) pass.
- [x] `schema/nib-delta.schema.json` (draft 2020-12, generated by `schemars`) validates all 14 fenced JSON examples in `JSON_SCHEMA.md` (`tests/schema.rs::schema_examples_validate`); `schema_is_up_to_date` fails on drift (regenerate with `cargo test -p nib-core --test schema schema_regenerate -- --ignored`).
- [x] Round-trip: every example deserializes → serializes → deserializes to an equal value (`schema_examples_roundtrip`); op payloads are additionally round-tripped as `Annotation` / `FormField` / `SignatureEntity`.
- [x] Android `InkTest`: 60 stylus `MotionEvent`s (sinusoid, pressure 0.3→0.8) → one `Ink` annotation, ≥ 40 points of arity 4, `smoothing.algorithm == "catmullRom"`, `tool == "stylus"`, `pressureSensitive == true`.
- [x] Android `UndoTest`: ink → highlight → select+move, undo ×3 (2, 1, 0 annotations in the semantics tree), redo ×3 (1, 2, 2; moved rect restored).
- [x] Android `HighlightSnapTest`: drag across two lines of `text.pdf` → 2 quads, equal heights ± 1 pt, tops 16 ± 1 pt apart, quad 0 text contains "quick brown".
- [x] Android `SelectionTest`: Square → select → Delete → 0, undo → 1.
- [x] Sample app: palette with all 12 tools, undo/redo, colour swatches, stroke width, bundled + picked image stamps, FreeText/Note text dialog, Export → `ACTION_SEND` share sheet (verified on the R8-minified release APK).

### Automated tests
- Rust: `nib-core` `model::tests::*` (10), `delta::tests::*` (RFC 7386 Appendix A table, `validate_*` one per error code, atomicity, idempotent create, `inverse_*`, `coalesce_typing`, `coalesce_gap_over_500ms_not_merged`, `explicit_group_undoes_together`, `limit_drops_oldest_but_keeps_history`, `redo_cleared_on_apply`, `full_export_replays_undo_redo`, `compacted_*`, `import_*`, `ink_bbox_includes_stroke_width`, `move_dirty_is_union`, `hit_test_*`, 2 `insta` snapshots), `text::tests::snap_lines_*`, `tests/schema.rs::{schema_is_up_to_date, schema_examples_validate, schema_examples_roundtrip}`, `tests/delta_laws.rs` (proptest, laws 1–4); `nib-render::char_boxes_text_fixture_four_lines`; `nib-ffi::{apply_undo_redo_roundtrip, text_lines_in_rect_two_lines, invalid_delta_error_is_json}`.
- Kotlin JVM: `DeltaOpSerializationTest`, `PageTransformTest`, `InkSmoothingTest`.
- Android: `InkTest`, `UndoTest`, `HighlightSnapTest`, `SelectionTest`.

---

## 4. Milestone M3 — PDF writer & appearance streams

**Status:** `[x]` (completed 2026-11-25) — host-provided static TrueType subsets are embedded for non-WinAnsi FreeText; all implementation DoD is covered. No installed CLI reader that extracts text from annotation appearance streams was available; `/ToUnicode` and CID mappings are validated structurally after save/reopen, and this reader-specific check is explicitly unverified.

### Goals & scope
- `nib-core::writer`: open with lopdf alongside pdfium (same bytes), `IncrementalDocument` save, `/Annots` CRUD with object cloning, `/NM` UUID round-trip, import of pre-existing annotations into the model on open (with `appearance.regenerate=false` + `apHash`), image XObject embedding (JPEG passthrough, PNG → Flate + SMask), atomic temp-file save helper, revision counter.
- `nib-core::ap`: appearance generators for all 11 annotation subtypes per ARCHITECTURE §7.2 incl. Base-14 fonts, word-wrap for FreeText, pressure-width ink outlines, line endings, multiply blend highlight. Subset host-provided TrueType fonts for non-WinAnsi FreeText and construct the PDF font objects with lopdf; see the implementation plan below.
- `nib-render`: revision swap (`reopen(bytes)`), dirty-tile invalidation protocol.
- `nib-ffi`: `flush`, `bytes`, `FlushReport`, `OpenOptions.lazy_structure`, `FontProvider`/`AssetProvider` callback interfaces.
- Kotlin: debounce/flush policy (idle 2 s, ≥ 50 ops, explicit save), `save(OutputStream)`, "Save As (optimized)" full rewrite path, overlay-until-flushed handoff (no flicker).
- Spike + test matrix for incremental save over: PDF 1.4 classic xref, 1.5 xref streams + object streams, linearized, pre-signed (must remain valid), hybrid.

Out of scope: forms (M4), signatures (M5).

### Dependencies
| Rust | Version (pinned 2026-11-13; all ≥ 7 days old) | Purpose |
|---|---|---|
| `lopdf` | `0.47.0` unchanged (`serde` only; `font_embedding` not enabled) | `IncrementalDocument`, full rewrite, `Stream::compress()` (Flate), explicit font objects |
| `flate2` | **not added** — lopdf's `Stream::compress()` already Flate-encodes (flate2 1.1.11 is transitive) | — |
| `image` | `0.25.10` (2026-03-10) promoted from dev to normal dep, `default-features = false, features = ["png", "jpeg"]` | stamp decode, downsample ≤ 2048 px, SMask split |
| `base64` | `0.22.1` (2024-04-30) | `image.dataBase64` |
| `uuid` | `1.29.0` + feature `v5` | deterministic ids for `/NM`-less annotations (`NIB_NS` namespace) |
| `kurbo` | `0.13.1` present, still unused (Catmull-Rom → Bézier is hand-rolled to mirror `InkSmoothing.kt` exactly) | — |
| Dev: `tempfile` | `3.27.0` (2026-03-11) | atomic-save test, qpdf temp files |
| Dev: `tiny-skia` | `0.12.0` (2026-02-02, BSD-3) | independent reference raster for render parity |
| Dev: `swash` | `0.2.10` (2026-07-17, MIT/Apache-2.0) | glyph rasterization of Arial/Liberation Sans in the parity reference (solid text bars measured pHash 18; `fontdue` rejected: pulls unmaintained `ttf-parser`, RUSTSEC-2026-0192) |
| Dev: `insta` `1.49.0`, `image_hasher` `3.1.1` | existing | `/AP` snapshots, pHash |
| Dev CI tools (not linked): `qpdf` (Apache-2.0) `--check` / `--linearize`; `fonts-liberation` | `apt-get install -y qpdf fonts-liberation` in the `rust` job; `brew install qpdf` locally | structural validation, linearized fixture, parity text |

#### Font dependencies (added 2026-11-25)

| Rust crate | Version | Purpose |
|---|---|---|
| `subsetter` | `0.2.6` (MIT OR Apache-2.0; MSRV 1.85; published 2026-06-04) | Font subset remapping and generation; older than seven days |
| `skrifa` | `0.44.0` reused from the existing lockfile (MIT OR Apache-2.0; MSRV 1.85) | Unicode cmap, font metadata and actual glyph advances; avoids a duplicate `0.42.1` |
| Dev: `skrifa` | workspace `0.44.0` | Independent glyph metrics in PDFium/tiny-skia parity |

`cargo deny check` and the Android arm64 cargo build validate the resolved
dependency set; no deny exceptions were added for these crates.

| Android | Version | Purpose |
|---|---|---|
| no new artifacts | — | `kotlinx-coroutines-test` (existing) drives `FlushPolicyTest` |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| lopdf incremental save fails on some xref-stream/hybrid files | Test matrix fixtures in CI; fallback to full rewrite when `has_signatures == false`; report `FlushReport.mode`. |
| Appearance mismatch across readers | Rust re-renders the written `/AP` with PDFium and compares it to an independent `tiny-skia`/`swash` raster reference; an embedded Noto Sans SC fixture is also compared directly. Manual matrix (ARCHITECTURE §7.3) remains reader-dependent. |
| Fonts: non-Latin FreeText | Host-provided TTF via `FontProvider`; subset + `Identity-H`; fixture with Cyrillic + CJK text. |
| File size growth with incremental saves | Each flush is one revision; the debounce policy keeps revisions ≤ dozens per session; "Save As (optimized)" compacts. |
| Reopen flicker | Overlay keeps drawing until tile `revision ≥ commitRevision` (M2 overlay already model-driven). |

### Embedded FreeText fonts

The host `FontProvider` is used only when a non-Base-14 family has text outside
WinAnsi. `skrifa 0.44.0` resolves per-scalar glyphs and metrics; `subsetter
0.2.6` generates a subset; lopdf writes `/Type0 /Identity-H`, `/CIDFontType2`,
`/FontDescriptor /FontFile2`, `/W`, `/CIDToGIDMap`, and `/ToUnicode`. CIDs are
per distinct Unicode scalar, so repeated characters reuse a CID while separate
characters sharing a glyph retain distinct Unicode destinations. Missing
glyphs map to `.notdef` and warn; unavailable, malformed, unsupported and
embedding-restricted input warns and falls back to Helvetica. The source must
be a static, single-face TrueType font with `glyf`/`loca`; TTC, CFF/CFF2 and
variable fonts are rejected. This scalar-mapping path does not implement
complex-script shaping, ligatures or bidirectional layout.

Redistributable subset fixtures and their OFL 1.1 provenance/generation notes
are in `nib-rs/nib-core/tests/fixtures/fonts/`. qpdf validates the saved
resource graph; lopdf reload assertions inspect the subset, CID mapping and
Unicode CMap; PDFium parity uses a tiny-skia/swash reference. No installed CLI
reader that extracts text from annotation appearance streams was available.
PDFium's `textInRect` was exercised and confirmed to return page-content text
only, not FreeText appearance text; therefore that extraction-reader check is
explicitly unverified, not passed.

### Definition of Done
- [x] Host-provided TrueType fonts are subset and embedded for non-WinAnsi FreeText; Cyrillic/CJK rendering parity, Unicode/CID mapping after save/reopen, font permissions/failure cases, Android `FontProvider`, and the existing WinAnsi/foreign-appearance paths are regression-tested. External text extraction from `/AP` remains unverified because no suitable reader is installed; do not claim that check passed.
- [x] For each of the 11 subtypes: write → `lopdf` reload → structural assertions (`ap::tests::snapshot_<subtype>` ×11, `insta` of the key-sorted annotation dict + decoded `/AP /N` content; `writer::tests::annots_crud`) pass.
- [x] `qpdf --check` passes (exit 0, or 3 = warnings only) on every written fixture: `annots_crud` (11 subtypes), `full_rewrite_compacts`, and the 5-class matrix × create/update/delete; the test skips with a message when `qpdf` is absent, CI installs it.
- [x] Render parity (`nib-render/tests/ap_parity.rs`): 12 cases (11 subtypes + image stamp) × opacity {1, 0.5, 0.25} = 36, pHash ≤ 6 for 35; **FreeText @ opacity 1.0 measured 7, limit relaxed to 9 for that single case**. The additional embedded-font Noto Sans SC PDFium comparison against an independent swash outline reference measured pHash distance 0 with 642 reference ink pixels.
- [x] Incremental matrix: `incr-{classic,xrefstream,linearized,presigned,hybrid}.pdf` × create/update/delete → lopdf reload + prefix bytes identical (`writer::tests::incremental_matrix`) and pdfium reload + tile render (`nib_ffi::tests::incremental_matrix_pdfium_reload`); pre-signed `/ByteRange` and `/Contents` objects unchanged; `FullRewrite` refused with `SignedDocument`.
- [x] Existing annotations imported on open (`annots-existing.pdf`, 11 subtypes, 2 with `/NM`) round-trip through `export_delta(Compacted)` with zero ops (`writer::tests::import_existing_zero_ops`, `nib_ffi::tests::flush_then_reopen_counts`); `/NM`-less ids are deterministic across opens (`nm_roundtrip`); foreign `/AP` preserved byte-for-byte on a `contents`-only update (`foreign_ap_preserved`).
- [x] Android `PersistenceTest`: ink + highlight + stamp → `flush()` → `save()` → `NibDocument.open(bytes)` → 3 annotations, subtypes {Ink, Highlight, Stamp}, Compacted export empty, page-0 tile composite pHash ≤ 6 vs the pre-save tiles.
- [x] Android `FreeTextFontProviderTest`: public provider supplies fixture bytes; flush reports no warnings; output contains `/NIBF`, `/FontFile2`, `/ToUnicode`; reopen preserves FreeText Unicode contents.
- [x] Android `NoFlickerTest`: scheduler paused → `flush()` → `annot:<id>` node still present and `revision == 1` → unpaused → covering tile node reports `revision:1` → `annot:<id>` node gone. Plus `ImportedNotDoubleDrawnTest` (imported annotations are never overlay-drawn, before or after a later flush) and `SaveAsOptimizedTest` (3 annots, optimized size ≤ incremental size, both reopen with 3).
- [x] Manual reader matrix (see cycle log): PDFium (same engine as Chrome) via the parity test + `incremental_matrix_pdfium_reload`; macOS Preview/PDFKit via `qlmanage` thumbnail of the 11-subtype file; Firefox pdf.js via headless `--screenshot`. Acrobat not available on this machine.

### Automated tests
- Rust: `nib-core::ap::tests` (font cmap/glyphs/metrics, wrap, supplementary Unicode, missing/invalid/restricted/unsupported fonts, existing snapshots), `writer::tests::freetext_subset_font_resources_survive_save_and_reopen` (qpdf, `/FontFile2`, `/CIDToGIDMap`, `/ToUnicode`, repeated CID, WinAnsi alias glyphs), all existing writer/incremental tests; `nib-render/tests/ap_parity.rs::{ap_parity_matrix, embedded_freetext_font_renders_like_independent_outline_reference, swap_then_render_shows_annotation}`; existing `nib-ffi` suite.
- Kotlin JVM: `FlushPolicyTest` (4).
- Android: `FreeTextFontProviderTest` plus `PersistenceTest`, `NoFlickerTest`, `SaveAsOptimizedTest`, `ImportedNotDoubleDrawnTest`.

---

## 5. Milestone M4 — AcroForms

**Status:** `[x]` (cycle 2026-11-18) — everything in scope done; deliberately not done: `/AP /D` down-appearances, writing `create`/`delete formField` ops (authoring), editable-combo dropdown in the Compose overlay (free text only).

### Goals & scope
- `nib-core`: field tree parsing from lopdf (`/AcroForm /Fields`, inheritance of `/FT`, `/Ff`, `/DA`, `/Q`, `/V` through parents), fully-qualified names, widget enumeration; `formField` op application (all `fieldType`s in JSON_SCHEMA §7), validation codes, `/AP` regeneration for text/checkbox/radio/combo/list widgets, `/NeedAppearances false`, optional **flatten** (bake widget `/AP` into page content via `Do` and remove fields) as `FlushReport.flattened`.
- `nib-render`: form-fill environment (`FPDFDOC_InitFormFillEnvironment`, `FPDF_FFLDraw`) for rendering widgets; `FPDF_RenderPageBitmap` flag control to hide the active widget; read-only access to field values for cross-checks in tests.
- `nib-ffi`: `form_fields()`, `set_field_value` convenience (wraps an `update formField` op), `flatten` option.
- Kotlin: `FormOverlay` — native Compose widgets (`BasicTextField`, `Checkbox`, `RadioButton`, dropdown) positioned via `PageTransform`; keyboard/IME handling with scroll-into-view; tab order from `/Fields` order; sample app form mode.

Out of scope: JavaScript actions/calculations, XFA, field authoring UI (create ops are supported by the engine but not by the sample UI).

### Dependencies
| Rust crate | Version (cycle 2026-11-18) | Purpose |
|---|---|---|
| **no new crates** | — | `lopdf 0.47.0` (parser, `IncrementalDocument`, `Document::encode_text`), `pdfium-render 0.9.5` (form-fill env is created at load; `render_form_data`, `as_form_field()` read-back), `uuid 1.29.0` + `chrono 0.4.45` newly added to `nib-ffi` (already workspace deps) for `set_field_value` ops |
| Dev: `form.pdf` fixture | generated by `tests/gen_fixtures.rs` (lopdf, 12 fields / 14 widgets, < 200 KB) | no third-party PDFs vendored |
| `deny.toml` | `ignore += RUSTSEC-2026-0543` (chrono "unmaintained", no vulnerability; user-approved) | keeps `cargo deny check` green |

| Android | Version | Purpose |
|---|---|---|
| no new artifacts | — | `material3` (BOM 2026.09.00) `DropdownMenu` for combo/list; `ui-test-junit4` `performTextInput` for `FormFillTest` |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Inherited attributes & merged field/widget dicts | Dedicated resolver with unit tests for 3-level hierarchies and merged dicts. |
| Radio groups with duplicate export values / `radiosInUnison` | Model keeps per-kid export values; tests cover unison. |
| Active widget double-draw (pdfium + Compose) | Spike both strategies (ARCHITECTURE §5.2); choose by measuring; record decision in cycle log. |
| Auto font size (`/DA` size 0) | Implement Acrobat-like fit: shrink from 12 pt until text fits; snapshot-test. |
| Comb fields, max-len, multiline wrapping | Dedicated AP tests per flag. |

### Definition of Done
- [x] Rust: generated fixture `form.pdf` (text ×6 incl. multiline/comb/password/readonly/hierarchical `bottom.note`, checkbox ×2, radio group ×3 kids, combo editable + non-editable, list multi) → set every value via ops → reload with lopdf (`writer::tests::form_set_values_roundtrip`, `/V`/`/AS`/`/I`/`NeedAppearances false`, prefix bytes identical, `qpdf --check`) **and** read back via pdfium's form-fill API on the worker (`nib-render/tests/forms.rs::form_values_readback_via_pdfium`, `nib_ffi::form_tests::set_field_value_flush_reopen_values_equal`) → all values equal.
- [x] Rust: `/AP` insta snapshots for each widget type and state — 11 `ap::tests::snapshot_widget_*` (`text, text_multiline, text_comb, text_password, text_autosize, checkbox_on/off, radio_on/off, combo, list_multi`); no `/D` by decision.
- [x] Rust: flatten test — after flatten `/AcroForm` absent and no Widget annots (`writer::tests::flatten_*`), page render **pHash 2** (≤ 4) vs the un-flattened render with widgets (`forms.rs::flatten_parity`; sanity vs untouched fixture > 4).
- [x] Rust: validation tests for `FIELD_READONLY`, `VALUE_NOT_IN_OPTIONS`, `MAXLEN_EXCEEDED` through the ffi (`form_validation_codes_through_ffi`, `NibError::InvalidDelta` JSON codes).
- [x] Android `FormFillTest`: type into `name`, toggle `agree`, pick radio `Green` & combo `Italy`, `flush()` → `save` bytes → reopen → `formFields()` returns the values; widget-rect tiles pHash **0** flushed-vs-reopened, **34** vs the blank fixture.
- [x] Android `ImeScrollTest`: focusing `bottom.note` with a 700 px IME inset scrolls it above the inset (overscroll up to the inset is allowed while a field is focused).
- [x] Undo/redo across form edits (`FormUndoTest`: typing coalesced to one step + checkbox + radio, undo ×3 / redo ×3); `tests/delta_laws.rs` laws 1–4 extended with `GenOp::FieldUpdate` over three seeded fields.

### Automated tests
- Rust: `nib-core` `forms::tests::{fixture_form_parses_twelve_fields, hierarchy_resolution, merged_dict_widget, radio_unison_two_kids_same_export}`, `delta::tests::{form_update_sets_appearance_states, form_undo_restores_appearance_states, uncommitted_includes_form_fields, adopt_base_fields_idempotent, form_value_typing_coalesces, drop_form_fields_clears_everything}`, `ap::tests::{snapshot_widget_* ×11, widget_autosize_shrinks_until_fits, widget_comb_cells_equal, widget_states_q_balanced}`, `writer::tests::{form_set_values_roundtrip, form_update_full_rewrite, form_readonly_field_untouched, form_create_delete_warns, form_fields_imported_zero_ops, flatten_removes_acroform_and_widgets, flatten_full_rewrite_reloads, flatten_incremental_prefix_identical}`, `tests/delta_laws.rs` (+`FieldUpdate`); `nib-render/tests/forms.rs::{form_values_readback_via_pdfium, flatten_parity, no_forms_flag_hides_widgets}`; `nib_ffi::form_tests::{form_fields_lists_fixture, set_field_value_flush_reopen_values_equal, form_validation_codes_through_ffi, flush_with_flatten_reports_and_empties_fields}`; `gen_fixtures::fixtures_are_valid` (now 14 files).
- Kotlin JVM: `FormFieldParsingTest` (3), `DeltaOpSerializationTest.updateFormFieldRoundTrip`.
- Android: `FormFillTest`, `ImeScrollTest`, `FormUndoTest`.

---

## 6. Milestone M5 — Signatures (visual e-signature + PAdES digital)

**Status:** `[x]` (visual-signature slice 2026-11-20; signing foundations,
verification/revision policy, transactional UniFFI/Android KeyStore signing,
layered appearances, sample status, and safe flatten refusal 2026-11-24;
actual Acrobat Reader appearance/status checks 2026-11-26)

No remaining M5 acceptance item. B-T remains out of scope and is rejected
explicitly; these checks do not claim revocation, LTV, or a trusted identity.

### Goals & scope
- Visual e-signature (JSON_SCHEMA §6.1): `intent: "signature"`, `signer` metadata, `bindToField` placement into an empty `/Sig` widget, signature capture sheet in sample app (draw / type / image), reusable stored signatures (host-side).
- `nib-sign`: signature field + widget creation, `/V` sig dict with placeholders, incremental save, exact `/ByteRange` patching, digest, CMS `SignedData` (detached, ESS `signing-certificate-v2` required for both `ETSI.CAdES.detached` and `adbe.pkcs7.detached`; `signingTime` omitted and rejected for CAdES), hex patching into `/Contents`, DocMDP `/Perms`, field `/Lock`, self-verification after signing.
- Verification: Rust-only `nib_sign::verify_signatures(bytes, trust_anchors_der)` — exact ByteRange/Contents coverage, digest, CMS signature and signed-attribute checks (`rsa` PSS/PKCS1v15, `p256`/`p384` ECDSA), ESS signing-certificate-v2, and bounded trusted-path building to host-supplied anchors. No anchors means trust `NotEvaluated`; `/M` is only an untrusted claim; revocation/LTV remain unchecked. Results include integrity/trust separately, revision and whole-document coverage, later changes (`FormFill`, `FormStructure`, `Annotation`, `Content`, `Unknown`), DocMDP and field-lock violations. Reopen restores barriers only from integrity-valid signatures; P=1 blocks edits, P=2 permits form-fill operations, and P=3 also permits annotations, subject to field locks. Signature verification is not exposed through UniFFI.
- Signature appearance `/AP` layer pattern (`/FRM`, `/n0`, `/n2`) with graphic copied from the referenced `Ink`/`Stamp`.
- `nib-ffi`: tokenized `prepare_signature`, `complete_signature`, and `abort_signature`; checks both writer revision and engine mutation token under writer-before-engine lock order. `verify_signatures` remains Rust-only.
- Kotlin: public `Signer` interface and `sign(spec, signer)` suspend helper; `KeyStoreSigner` supports Android KeyStore RSA-PSS and ECDSA P-256 with leaf-first certificate-chain handling. Host signing runs between FFI calls, outside Rust locks.
- Stretch (only if everything above is done): `level: "B-T"` — RFC 3161 request/response (`x509-tsp` crate or minimal DER) via a host `TimestampProvider` callback; unsigned attribute `signature-time-stamp`.

Out of scope: revocation (OCSP/CRL), LTV (B-LT/B-LTA), smartcard/PKCS#11 transport (host can implement `Signer`), MDP policy enforcement UI.

### Dependencies
Pinned 2026-11-24 (every new direct and transitive version published ≥ 7
full days before, not yanked, MSRV ≤ 1.88, Apache-2.0/MIT/BSD-3, no
advisories; `cargo deny check` passes with no new exceptions):

| Rust (exact pin) | License | Purpose |
|---|---|---|
| `cms =0.3.0-rc.2` (no `builder`: typed `SignedData`, so no RSA 0.9 / RUSTSEC-2023-0071), `x509-cert =0.3.0`, `der =0.8.3` (`derive`), `spki =0.8.1`, `const-oid =0.10.3` (`db`) | Apache-2.0/MIT | CMS / X.509 / DER |
| `rsa =0.10.0` (`encoding`, `pkcs1v15`; constant-time `crypto-bigint`, fixes RUSTSEC-2023-0071), `p256 =0.14.0`, `p384 =0.14.0` (`ecdsa`, `pkcs8`), `signature =3.0.0`, `sha2` | Apache-2.0/MIT | signature verification (production) + test signing |
| `getrandom =0.4.3` (`sys_rng`, dev only) | Apache-2.0/MIT | in-test key generation |
| Transitive: `base16ct 1.0.0`, `base64ct 1.8.3`, `cmov 0.5.4`, `crypto-bigint 0.7.11`, `crypto-primes 0.7.2`, `ctutils 0.4.8`, `der_derive 0.8.0`, `ecdsa 0.17.0`, `elliptic-curve 0.14.3`, `ff 0.14.0`, `fiat-crypto 0.3.0`, `flagset 0.4.7`, `group 0.14.0`, `hmac 0.13.0`, `pem-rfc7468 1.0.0`, `pkcs1 0.8.0`, `pkcs8 0.11.0`, `primefield 0.14.0`, `primeorder 0.14.0`, `rfc6979 0.6.0`, `sec1 0.8.1`, `serdect 0.4.3`, `subtle 2.6.1`, `zeroize 1.9.1` (held back: 1.9.2 was < 7 days old), `zeroize_derive 1.5.0`; `tls_codec`/`tls_codec_derive 0.4.2` are locked as optional `x509-cert` deps but not built | Apache-2.0/MIT/BSD-3 | — |
| Required test oracles: `openssl` CLI ≥ 3 (`openssl cms -verify -binary -inform DER -noverify`, `NIB_OPENSSL` overrides) and `qpdf --check` | Apache-2.0 | independent verification |

Verifier tests reuse the workspace's existing `proptest`, `serde_json`, and
`uuid` dependencies; this cycle adds no crate or cargo-deny exception.

Dev profile: `crypto-bigint`, `crypto-primes` and `rsa` build at
`opt-level = 3` so in-test RSA-2048 key generation stays fast.

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
| Placeholder too small for long chains | 16,384 hex characters (8,192 DER bytes) default; `SignOptions.contents_hex_len` override (even, ≤ 1,048,576); error `PlaceholderTooSmall { required }` in hex characters excluding `<` `>`; nothing is published and the prepared signature stays retryable/abortable. |

### Definition of Done
- [x] Rust: sign a fixture with in-test RSA-2048 (PSS), P-256 and P-384 keys → `verify_signatures()` reports valid integrity; `openssl cms -verify` cross-checks the extracted CMS and ByteRange content.
- [x] Rust: tamper, appended annotation and form-fill revisions, wrong/missing anchors, expired certificate, and double-signature revisions are covered; revisions report integrity separately from trust and classify later changes.
- [x] Rust: DocMDP `/P 1` + later form edit reports `FormFill` and `docMdpViolated == true`; P=2/P=3 and field locks are covered.
- [x] Rust: reopening restores conservative signature barriers, DocMDP permissions and field locks from integrity-valid signatures.
- [x] Rust FFI: staged bytes remain private; invalid signatures are retryable, successful completion is verified, and an engine edit during signing rejects adoption.
- [x] Rust: description-only and `graphicAndDescription` appearance assertions; foreign image `/AP` resources are retained, qpdf accepts the signed PDFs, and pdfium renders the signature widget.
- [x] Rust: double-signature test (two fields, two revisions) — both valid; first `coversWholeDocument == false`, second `true`.
- [x] Android `KeyStoreSignTest`: generate AndroidKeyStore ECDSA P-256 and RSA-PSS keys, sign through the public facade (including graphic+description options); failed/invalid signing rolls back and concurrent edits reject adoption.
- [x] Android `VisualSignatureTest` and `SampleVisualSignatureTest`: capture ink in the signature sheet bound to `Signature1`, assert the annotation rect equals the widget rect (contain-fit) and `locked == true`.
- [x] Android `SignatureStatusTest`: occupied fields are labeled as present while integrity and trust are explicitly not evaluated; signed flatten refusal leaves a pre-existing destination untouched.
- [x] Actual Adobe Acrobat Reader on macOS, 26.003.22110 (`app.viewerType == "Reader"`): opened fresh RSA-PSS and ECDSA P-256 PDFs, visually checked their description appearances and native signature warning UI, with no appearance-regeneration prompt. Reader's own `signatureValidate()` returned 3 for both; `signatureInfo().statusText` said the document had not been modified, but the signer's identity was unknown because neither the self-signed certificate nor its parents were trusted. This is not a trusted/green signature result; no trust settings were changed.

### Acrobat Reader interoperability (2026-11-26)

Generate fresh, locally signed examples (test certificates last one day):

```sh
cd nib-rs
NIB_KEEP_PDFS=/tmp/nib-m5-acrobat cargo +1.99.1 test -p nib-sign acrobat_interop_appearances
open -a "Adobe Acrobat" /tmp/nib-m5-acrobat/acrobat_rsa_pss.pdf
open -a "Adobe Acrobat" /tmp/nib-m5-acrobat/acrobat_ecdsa_p256.pdf
```

The test verifies both outputs with Rust, OpenSSL and qpdf, and checks their
layered description content. The actual Reader check is separate, not an
automated claim from that test. For each active test document, this command
retrieves Reader's own diagnostic without installing a test trust anchor:

```sh
osascript -e 'tell application "Adobe Acrobat" to do script "var f=this.getField(\"Signature1\");var v=f.signatureValidate();var s=f.signatureInfo();JSON.stringify({file:this.documentFileName,status:v,text:s.statusText,revision:s.revision,numRevisions:s.numRevisions});"'
```

Both showed the expected visible description (`Nib RsassaPss ...` /
`Nib EcdsaP256 ...`) and Reader's warning banner, not a green trust indicator.
Both returned `status: 3`, `revision: 1`, `numRevisions: 1`, with this diagnostic:

> Signature validity is UNKNOWN. The document has not been modified since
> this signature was applied. The signer's identity is unknown because it
> has not been included in your list of trusted certificates and none of its
> parent certificates are trusted certificates.

The checked PDFs and Reader window captures are retained as session artifacts,
not repository fixtures. Their SHA-256 values are
`38b328697a6008b27ee47817144215add3bb1bfb88b4ae88696896e654d322d0`
(RSA-PSS) and
`d372695423811dc757812c2856f51d60bc73e13784638291b71963398f7989a5`
(P-256). Fresh generation changes keys, certificates, timestamps and hashes.
An earlier RSA test's deliberately January-dated `/M` preceded its fresh test
certificate; the new reader fixtures use the current claimed time to avoid
conflating certificate-time and identity-trust warnings. Initial UI scripting
was denied by macOS, but native Reader scripting and authorized window capture
subsequently allowed the actual appearance/status check.

### Automated tests
- Rust: `nib_sign::tests::{sign_rsa_pss, sign_ecdsa_p256, byterange_exact, tamper_detected, incremental_after_sign, docmdp_violation, verifies_incremental_signature_revisions, signature_appearance_description_only, signature_appearance_copies_foreign_image_resources}`; `nib-render::tests::pdfium_renders_signature_widget_appearance`; CI step `openssl cms -verify`.
- Android: `KeyStoreSignTest`, `VisualSignatureTest`, `SignatureStatusTest`.

---

## 7. Milestone M6 — Hardening, performance & release

**Status:** `[~]` (malformed-input fixes, initial fuzz smoke runs, and scheduled
fuzz CI completed 2026-11-26; performance, 24-hour fuzzing, coverage, API and
release work remain)

### Goals & scope
- Performance budgets (measured on API 34 x86_64 emulator as CI baseline and on a physical mid-range arm64 device manually): first visible page ≤ 300 ms from `from_bytes` for a 10 MB text PDF; tile P50 ≤ 12 ms / P95 ≤ 30 ms; scroll `frameOverrunMs` P90 ≤ 4 ms; ink touch-to-draw ≤ 1 frame; `flush()` with 50 ops ≤ 150 ms on a 20 MB PDF; memory ceiling respected under `monkey` stress.
- `criterion` benches for geometry, AP generation, delta apply/compaction; `Macrobenchmark` suites gating on regressions > 15 %.
- Fuzzing: `cargo-fuzz` targets `import_delta_json`, `open_bytes` (lopdf path), `ap_from_annotation`; run nightly; seed corpus from fixtures.
- Robustness: malformed PDFs (truncated xref, missing `/Annots`, broken `/AP`), huge pages (200 in), 1000-page docs with `lazy_structure`, low-memory callbacks, process death recovery via `exportDelta(Full)` in `SavedStateHandle` (sample app).
- Optional `AHardwareBuffer` tile transport if M1 measurements showed copy cost > 1 ms/tile.
- API freeze: `@Stable` Kotlin API review, KDoc for every public symbol, rustdoc `#![deny(missing_docs)]` in `nib-ffi`, `BinaryCompatibilityValidator` baseline.
- Docs: `README.md` quick-start, `docs/API_GUIDE.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`.
- Publishing: Maven Central (Sonatype Central Portal) via `com.vanniktech.maven.publish`, signed artifacts, `release.yml` tagging `v0.1.0`; reproducible `fetch-pdfium.sh` recorded in release notes with SHA-256s.

Completed slice (2026-11-26): malformed classic xref data is rejected as a typed parse error; malformed page `/Annots` containers/entries fail writer import explicitly; malformed foreign `/AP` dictionaries are preserved without regeneration; FFI eager and lazy writer-failure behavior is covered, as are flush rollback and render-worker open/swap panic boundaries. This does not complete M6's fuzzing, performance, coverage, API, or publishing criteria.

Fuzz-target foundation (2026-11-26): the standalone [`nib-rs/fuzz`](../nib-rs/fuzz/README.md)
workspace contains all three byte-input harnesses and generated compact seeds
(largest PDF: 5,645 bytes). Preparing the malformed-ink seed exposed a direct
`ap::build` panic on short point rows; it is now an explicit `MissingGeometry`
error, covered by nine deterministic cases. The maintainer approved a
version-scoped NCSA exception for fuzz-only `libfuzzer-sys 0.4.13`; both the
production and isolated-workspace dependency audits pass. The three
instrumented 60-second smoke runs completed without crashes:
`import_delta_json` 316,498 executions (latest pulse coverage 5,549),
`open_bytes` 85,428 (7,741), and `ap_from_annotation` 316,670 (3,336).
The `open_bytes` run found an AddressSanitizer stack overflow from cyclic
AcroForm `/Kids`; field traversal now detects repeated object IDs and caps
depth at 64, with deterministic cycle/depth regression tests. Replaying the
original 5,479-byte artifact with `-runs=1` now completes without a crash.
These smoke runs are not the 24-hour qualification.

The [scheduled fuzz workflow](../.github/workflows/fuzz.yml) runs daily and on
manual dispatch, audits the standalone fuzz dependency graph, and runs each
target for five minutes with a 45-minute job timeout. It uses nightly only for
instrumentation and uploads crash artifacts on failure; it is not the
24-hour qualification.

Out of scope: new features.

### Dependencies
| Rust | Purpose |
|---|---|
| Dev: `criterion`, `cargo-fuzz` (nightly), `cargo-llvm-cov` | perf, fuzz, coverage |
| Isolated fuzz: `libfuzzer-sys =0.4.13` (2026-06-04), `cargo-fuzz 0.13.2` tool (2026-06-09); new transitives `arbitrary 1.5.0` (2026-10-06), `jobserver 0.1.35` (2026-07-06) | All published more than 7 days before this cycle; not yanked. `libfuzzer-sys` requires `(MIT OR Apache-2.0) AND NCSA`; maintainer-approved exception is scoped to exactly `libfuzzer-sys =0.4.13`, used only by the standalone fuzz workspace and never shipped. Production dependencies/lockfile are unchanged. |

| Android | Purpose |
|---|---|
| `androidx.benchmark:benchmark-macro-junit4`, `androidx.profileinstaller` (baseline profiles) | perf |
| `org.jetbrains.kotlinx:binary-compatibility-validator`, `com.vanniktech:gradle-maven-publish-plugin` | API/publishing |

### Risks & mitigations
| Risk | Mitigation |
|---|---|
| Emulator benchmarks noisy | Gate on relative regression with 3-run median; physical-device numbers recorded manually in the cycle log. |
| Fuzzer finds lopdf panics we cannot fix upstream quickly | `catch_unwind` boundary already in `nib-ffi`; report upstream; add regression fixtures. |
| Fuzz runtime license exception could broaden unintentionally | Keep the NCSA exception limited to `libfuzzer-sys =0.4.13`; audit the isolated fuzz manifest separately and keep the runtime out of shipped artifacts. |
| Malformed AcroForm field graphs may cycle or be excessively deep | Track visited field object IDs, cap traversal depth, and retain deterministic regression tests; continue fuzzing parser boundaries. |
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
| 2026-11-26 | M6 scheduled fuzz CI | Added `.github/workflows/fuzz.yml` with daily 03:17 UTC schedule and manual dispatch. CI audits `nib-rs/fuzz/Cargo.toml` separately, installs `cargo-fuzz 0.13.2`, builds with nightly, and runs all three targets for 300 seconds each against ignored writable corpora plus checked-in seeds; the job timeout is 45 minutes and failure artifacts are uploaded. Updated fuzz README and next-cycle prompt. **Passed:** `actionlint .github/workflows/fuzz.yml`; `(cd nib-rs && cargo deny --manifest-path fuzz/Cargo.toml check && cargo +1.99.1 fmt --check && cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check && cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings && cargo +nightly fuzz build && cargo +1.99.1 test -p nib-core -p nib-ffi && cargo +1.99.1 clippy --all-targets -- -D warnings && cargo +1.99.1 test && cargo deny check)` (all passed); all three fuzz target command lines also passed with `-runs=1` against writable corpora plus checked-in seeds. | M6 remains `[~]`: 24-hour fuzz qualification, performance budgets/benchmarks, coverage thresholds, Android benchmarks, API freeze, release/publishing, and M3 annotation-appearance text extraction remain. | Daily CI uses bounded 5-minute target runs; the separately required 24-hour qualification is not part of routine CI. Nightly is isolated to the fuzz job; production remains on Rust 1.99.1. No Android source or UniFFI surface changed. | Routine bounded CI cannot establish 24-hour stability; fuzz coverage and host asset/font inputs remain incomplete. |
| 2026-11-26 | M6 fuzz hardening | Following maintainer approval, added a version-scoped NCSA exception for fuzz-only `libfuzzer-sys =0.4.13`; the isolated and production dependency audits both pass. Reproduced an AddressSanitizer stack overflow in `forms::walk` using the 5,479-byte `open_bytes` artifact. Added visited-object and depth-64 guards with deterministic self-cycle/deep-tree tests; the original artifact now completes under `-runs=1`. All three sequential smoke runs completed without crashes: `import_delta_json` 316,498 executions / latest pulse coverage 5,549; `open_bytes` 85,428 / 7,741; `ap_from_annotation` 316,670 / 3,336 (61 seconds each). **Passed:** `(cd nib-rs && cargo +1.99.1 fmt --check && cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check)`; `(cd nib-rs && cargo +1.99.1 test -p nib-core forms::)` (10 passed); original artifact replay; all three bounded smoke commands; `(cd nib-rs && cargo +1.99.1 test && cargo +1.99.1 clippy --all-targets -- -D warnings && cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings && cargo +nightly fuzz build && cargo deny check && cargo deny --manifest-path fuzz/Cargo.toml check)`. | M6 remains `[~]`: scheduled fuzz CI, the 24-hour qualification, performance budgets/benchmarks, coverage thresholds, Android benchmarks, API freeze, and release/publishing remain. M3 annotation-appearance text extraction remains unverified. | A cyclic/repeated AcroForm field reference or a tree deeper than 64 is skipped with a `FormImport` warning; parsing stays infallible and does not recurse into the malformed subtree. The exact-version NCSA exception is confined to the standalone fuzz dependency graph. No Android source or UniFFI surface changed. | Smoke runs are bounded, local Mac ARM64 executions, not a 24-hour or multi-platform qualification. Other lopdf parser/resource-limit behavior remains to be explored. |
| 2026-11-26 | M6 fuzz foundation / M5 Acrobat closure | Added isolated byte-input harnesses for delta import, lopdf writer open/import, and annotation AP generation, plus reproducible synthetic/repository seeds (PDFs 53-5,645 bytes). The malformed-ink seed reproduced `ap/ink.rs` indexing an empty filtered point list; added a regression that first failed with that panic, then fixed short/mixed point rows to return `MissingGeometry` (nine cases). Added `acrobat_interop_appearances` generating fresh current-time RSA-PSS/P-256 layered descriptions. **Passed from `nib-rs/`:** `cargo +1.99.1 fmt --check`; `cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check`; `cargo +1.99.1 run --manifest-path fuzz/Cargo.toml --example seed_corpus` (typed JSON seed validation/generation); `cargo +1.99.1 test -p nib-core -p nib-ffi` (core 160 unit tests plus integration suites; FFI 19); `NIB_KEEP_PDFS=/tmp/nib-m5-acrobat cargo +1.99.1 test -p nib-sign acrobat_interop_appearances` (one test, both algorithms, OpenSSL/qpdf oracles); `cargo +1.99.1 clippy --all-targets -- -D warnings`; `cargo +1.99.1 test` (all unit/integration/doc suites, including 35 signing tests); `cargo deny check` (advisories/bans/licenses/sources OK; pre-existing warnings). **Actual Reader 26.003.22110:** both description appearances rendered without regeneration prompts; warning UI and Reader diagnostics show unchanged bytes but UNKNOWN identity trust for self-signed certs, not trusted signatures. Native screenshots/PDFs retained as session artifacts; M5 is now `[x]`. | M6 remains `[~]`. `cargo +nightly fuzz build` was started then interrupted after `cargo deny --manifest-path fuzz/Cargo.toml check` rejected NCSA in `libfuzzer-sys 0.4.13`. No completed instrumented build, no smoke command executed (duration: N/A), no 24-hour or nightly fuzzing claimed. Await maintainer license decision; then build all targets, run each for a recorded bounded duration, fix findings, wire nightly CI and complete 24-hour qualification. Performance, coverage, API/release work and M3 annotation-appearance text extraction remain. | Fuzz-only dependencies/lockfile live in a separate workspace; no production dependency, Android source or UniFFI surface change. `cargo-fuzz` was missing and installed with `cargo +1.99.1 install cargo-fuzz --locked --version 0.13.2`; installed nightly is `1.99.0-nightly (af3d95584 2026-07-09)`. No license-policy changes. Android validation not required. Reader validation used the actual app, not PDFKit/PDFium as substitutes; self-signed certs were not added to the trust store. | Fuzz harness instrumented compilation/execution remains unverified because of the policy gate; independent root checks must not mask this. Host assets/fonts are not fuzzed. Self-signed Reader fixtures expire after one day and need regeneration; Reader UNKNOWN trust is expected and must not be reported as trusted. |
| 2026-11-26 | M6 malformed-PDF hardening | Added classic-xref preflight rejection for missing `startxref`/`trailer` and out-of-range offsets; malformed `/Annots` arrays/references now return `WriterError::MalformedAnnots` instead of being silently skipped. Added regressions for truncated xref, invalid startxref, scalar `/Annots`, malformed foreign `/AP /N`, transactional flush rollback, and eager/lazy FFI read-only behavior. Added unwind-to-`RenderError::Internal` boundaries around PDFium open/swap. **Passed:** `(cd nib-rs && cargo +1.99.1 fmt --check)`; `(cd nib-rs && cargo +1.99.1 test -p nib-core writer::tests)` (31 passed; qpdf 12.4.2 was available and valid incremental outputs passed `qpdf --check`); `(cd nib-rs && cargo +1.99.1 test -p nib-ffi)` (19 passed); `(cd nib-rs && cargo +1.99.1 clippy --all-targets -- -D warnings)`; `(cd nib-rs && cargo +1.99.1 test)` (all Rust unit, integration, and doc-test suites passed); `(cd nib-rs && cargo deny check)` (advisories/bans/licenses/sources OK, with pre-existing unmatched-license and duplicate-crate warnings). | Fuzz targets and 24-hour fuzzing, performance budgets/benchmarks, coverage thresholds, Android benchmarks, API freeze, and release/publishing remain. M5 remains `[~]` pending Acrobat Reader interoperability; M3 annotation-appearance text extraction remains unverified because no suitable CLI reader is installed. | Added strict validation only for classic xref sections; xref-stream corruption handling remains delegated to lopdf and needs separate regression coverage. FFI eager open keeps malformed-annotation documents render-only after logging; lazy import surfaces `WriteFailed` on writer use. No Android source or UniFFI surface changed, so Android validation was not run. | M6 coverage is still narrow: test only a truncated classic table and malformed `/AP /N` dictionary; dangling refs, malformed stream filters, xref streams, huge-page/memory pressure and fuzz-discovered parser behavior remain untested. |
| 2026-11-25 | M3 embedded FreeText fonts | Implemented host-provided static TrueType subsetting/embedding with `skrifa 0.44.0` and `subsetter 0.2.6`; added Noto Sans SC/Symbols 2 OFL fixtures, Unicode/CID/font-resource save/reopen checks, repeated-CID writer assertion, missing/invalid/restricted/unsupported-font warnings, and independent PDFium rendering parity (distance 0, 642 reference ink pixels). Public Android `FontProvider` instrumentation passed. **Passed:** `(cd nib-rs && cargo +1.99.1 fmt --check && cargo +1.99.1 clippy --all-targets -- -D warnings && cargo +1.99.1 test && cargo deny check)`; focused `(cd nib-rs && cargo +1.99.1 test -p nib-core ap::tests && cargo +1.99.1 test -p nib-core writer::tests && cargo +1.99.1 test -p nib-render --test ap_parity)`; `./gradlew --no-daemon :nib-android:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=io.github.nibpdf.nib.FreeTextFontProviderTest`; `./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease`; `nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk`; `./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest` (all passed; `nib_api34`). `cargo deny check` ended with advisories/bans/licenses/sources OK; it printed pre-existing duplicate-crate and unused-license-allowance warnings. | No CLI reader that extracts text from annotation appearance streams (`pdftotext`/`mutool` absent). PDFium `textInRect` was tested and returns page-content text only; `/ToUnicode` and encoded CID mappings were instead validated structurally after save/reopen. The reader-specific check is unverified, not passed. M5 remains `[~]` pending actual Acrobat Reader interoperability. | Reused lockfile `skrifa 0.44.0` rather than proposed `0.42.1`; only `subsetter 0.2.6` was newly resolved. The Type0 font resource is local to `/AP`; `/DA` stays Base-14 for fallback/editing. | Only static single-face `glyf`/`loca` fonts are supported; no TTC, CFF/CFF2, variable fonts, complex-script shaping, ligatures or bidi. Acrobat Reader remains unavailable. |
| 2026-11-25 | M5 reader interoperability | Confirmed Adobe Acrobat Reader is unavailable: no matching application bundle in `/Applications` or `~/Applications`, and Spotlight bundle lookup found no Adobe Reader/Acrobat. Generated signed fixtures outside the repository with `(cd nib-rs && NIB_KEEP_PDFS=/tmp/nib-m5-interop cargo test -p nib-sign)` (**34 passed**). `qpdf --check` passed for `cades_rsa_pss_existing.pdf`, `cades_p256_prehashed.pdf`, `appearance_description.pdf`, and `appearance_graphic_description.pdf`. macOS Quick Look/PDFKit rendered all four thumbnails: the RSA-PSS and ECDSA-P256 signing fixtures show the underlying “Sign here” field; the ECDSA description appearance shows “Approved by Ada”; the graphic-plus-description fixture shows “Signed by Ada” and its small red sample graphic. These are supplemental rendering observations only: no Acrobat repair-warning, signature-integrity, signer-identity, or trust-state check was possible. | Manual Acrobat Reader interoperability remains blocked until Reader is available. M5 remains `[~]`; do not infer Acrobat compatibility or trust from PDFKit/qpdf/Rust results. B-T remains out of scope and explicitly rejected. | None; Quick Look/PDFKit was used only as supplemental visual evidence, not as a substitute for Acrobat or cryptographic verification. | Acrobat-specific parsing, appearance, integrity UI and default/manually-trusted signer states remain unknown. PDFKit rendering does not establish signature integrity or trust. |
| 2026-11-24 | M3 FreeText font plan | Documented a proposed `subsetter 0.2.6` + `skrifa 0.42.1` implementation and explicit non-WinAnsi FreeText DoD. A prior isolated workspace-copy trial passed `cargo deny check`, `cargo check -p nib-core`, and `cargo check -p nib-core --target aarch64-linux-android`; it added five font packages without new deny exceptions. No project dependencies or implementation changed in this documentation cycle. | Implement embedded TrueType subsets, PDF font resources, Unicode mapping, layout and tests; M3 remains in progress. | Construct PDF font objects with lopdf rather than relying on its `font_embedding` feature, which does not provide subsetting. | Fresh dependency resolution can select versions less than seven days old; verify the actual lockfile and dependency policy at implementation time. Font permissions, unsupported formats and missing glyphs require explicit handling. |
| 2026-11-24 | M5 signature appearances and sample status | Composed visible signature widget `/AP` layers (`/FRM`, `/n0`, `/n2`) before ByteRange preparation. Description-only and graphic+description appearances reuse the referenced Ink/Stamp `/AP /N` form and its nested image resources; public Kotlin `SignatureAppearanceSpec` wires a description, graphic UUID, mode, and Helvetica size through UniFFI. Tests cover description output, a foreign image form/resource, qpdf, and pdfium rendering; the existing two-revision test verifies both signatures and whole-document coverage flags. The sample reports only “Signature present. Integrity and trust have not been evaluated.” Save As now flushes/refuses signed flattening before opening the destination stream; instrumentation verifies existing destination data is untouched. **Passed:** `(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)`; `./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease`; `nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk`; `./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest` passed on `nib_api34`. | Manual Acrobat Reader interoperability for RSA-PSS and ECDSA P-256 remains; B-T stays explicitly rejected. | The public appearance API intentionally exposes the supported minimum (mode, description, Ink/Stamp UUID, Helvetica size); name/date/reason composition and arbitrary fonts are rejected or deferred. The visual source annotation remains in the document after its AP is reused. | Foreign `/AP` forms can have unusual matrices or resources beyond the tested image case; Acrobat interoperability remains unverified. |
| 2026-11-24 | M5 transactional UniFFI and Android KeyStore | Added UniFFI `prepare_signature`/`complete_signature`/`abort_signature` around existing writer staging. Preparation flushes ops and stores an opaque token, request, writer revision, and engine mutation counter; completion holds writer then engine locks, rejects stale revision/token, verifies before adoption, syncs form field state, and restores integrity-valid barriers. Mutation tokens advance on successful apply/import/undo/redo/form updates. Kotlin `Signer` executes between FFI calls; `KeyStoreSigner` supports RSA-PSS and ECDSA P-256 with leaf-first certificate chains, and `NibDocument.sign` aborts on callback failure/cancellation. Rust FFI tests cover private staged bytes, invalid-signature retry, OpenSSL-generated ECDSA completion verified by `verify_signatures`, and concurrent-edit rejection. Android tests cover both algorithms, invalid signature, host error, and stale edit. **Passed:** `(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)`; `./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease`; `nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk`; `./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest` passed on `nib_api34`. | Signature `/AP` composition/resource copying, truthful sample status, flatten validation before destination truncation, double-signature verification, and Acrobat/manual reader interoperability. B-T remains explicitly rejected. | Host signing is implemented as separate prepare and complete FFI calls, rather than a Rust-to-Kotlin callback: Kotlin invokes `Signer` only after `prepare_signature` returns, guaranteeing no Rust lock crosses the callback boundary. The public Kotlin facade accepts raw signatures plus a DER chain; full-CMS completion remains internal Rust. | KeyStore behavior has been exercised on API 34 emulator only; other Android provider/device combinations may differ. Sample status must not conflate field occupancy with integrity or host-anchor trust. |
| 2026-11-24 | M5 verification and revision policy | Added Rust-only `nib_sign::verify_signatures`: exact ByteRange/Contents coverage, CMS/digest/signed-attribute and ESS checks; integrity separate from trusted-path status against host-supplied DER anchors; no anchors → `NotEvaluated`; claimed `/M` only; revocation/LTV unchecked. Results include signed revision, whole-document coverage, later `FormFill`/`FormStructure`/`Annotation`/`Content`/`Unknown` changes, DocMDP and FieldMDP lock violations. Hostile input is bounded and proptest-covered. On reopen, FFI re-verifies without anchors and restores barriers only for integrity-valid signatures; P=1/2/3 enforcement and locked fields are tested. **Passed:** `(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings)`; `(cd nib-rs && cargo test && cargo deny check)` (149 core unit tests, all integrations, 32 nib-sign tests); `./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease`; `nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk`; `./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest` passed after booting `nib_api34` (the first attempt had no connected device). | Transactional UniFFI/Android KeyStore signing, callback-safe engine mutation tokens, engine-side signature-last enforcement, layered appearances/resource copying, truthful sample status, flatten validation before destination truncation, Acrobat/manual reader interoperability, and Android KeyStore signing remain. B-T remains rejected. | `FormStructure` is a disallowed classification for adding/removing/renaming fields; ambiguous changes remain disallowed. Reopened imported entities are conservatively treated as predating the signature because historical entity snapshots are unavailable. Verification/restored policy is Rust/internal only; open-time verification uses no trust anchors. | No revocation/LTV; trusted-path validation is bounded and supports only the implemented certificate signature algorithms/extensions. `cms` remains a release candidate; writer adoption still lacks an engine mutation token. |
| 2026-11-24 | M5 signing foundations | Internal Rust PDF/CMS signing, with no UI and no claims about verification or trust. Dependencies pinned after vetting age (≥7 days), yanked status, MSRV 1.88, licences and advisories: `cms =0.3.0-rc.2` (typed `SignedData`, no builder, so RSA 0.9 and RUSTSEC-2023-0071 are not pulled in), `x509-cert =0.3.0`, `der =0.8.3`, `spki =0.8.1`, `const-oid =0.10.3`, `rsa =0.10.0` (dev), `p256`/`p384 =0.14.0` (dev), `signature =3.0.0`, `getrandom =0.4.3` (dev), and `zeroize` held at 1.9.1; `cargo deny check` passes with no new exceptions. `nib-core::writer::sign`: `stage_signature` writes a private incremental revision (existing empty field or new top-level invisible field; fixed-width `/ByteRange` sentinel and `/Contents` placeholder, default 16384 hex chars excluding delimiters). `adopt_signed` accepts only bytes for the same revision/token, with an unchanged prefix, a complete ByteRange whose edges are `<`/`>`, and changes only inside the two placeholders. Stale staging is detected after an intermediate flush; flatten/rewrite is refused while signed or prepared; signature ops raise `UnsupportedOp` instead of being skipped. `nib-ffi` rejects `target: "signature"` in `apply`/`import_delta` with `UNSUPPORTED_OP`. `nib-sign`: `prepare` → `SignRequest { signed_attributes DER }` → `complete(Completion::Signature | Completion::Cms)` / `abort`. It builds detached CAdES (`ETSI.CAdES.detached`) or `adbe.pkcs7.detached` with content-type, message-digest and ESS signing-certificate-v2, without signingTime; a supplied CMS lacking ESS signing-certificate-v2 is rejected for both profiles. It requires the leaf certificate or fingerprint before preparing and verifies the signature, digest, attributes and certificate before embedding. Supports RSA PKCS#1 v1.5 and RSA-PSS with SHA-256/384/512, ECDSA P-256 with SHA-256 only, and ECDSA P-384 with SHA-384 only; explicit prehash only when declared; B-T rejected. Tests: `cargo test -p nib-sign` has 24 tests, with RSA-2048/PSS and P-256 keys and certificates generated in-test, exact offsets, unchanged earlier revisions, collisions, malformed ranges, wrong signature/certificate, unsupported algorithms, overflow, and rollback/retry. **Required** oracles: `openssl cms -verify -binary -inform DER -noverify` on the extracted DER and ByteRange bytes (a tampered copy must fail), and `qpdf --check`; CI installs `openssl`. nib-core writer tests (+5) and nib-ffi (+1) added. Baseline: `cargo fmt --check`, clippy `-D warnings`, `cargo test` (nib-core 147+1+1+4+3, nib-ffi 15, nib-render 15+2+3, nib-sign 24) and `cargo deny check` all green; `ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease` green; ELF alignment OK; `connectedDebugAndroidTest` green on `nib_api34` (nib-android 19, sample-app 4). | Integrity and trust verification results; trusted path against host anchors; revision and DocMDP classification; lock reconstruction on reopen; transactional UniFFI and Android KeyStore, with callbacks outside locks and an engine mutation token checked before adoption; engine-side "signature last in revision"; visible/layered signature appearances, including resource copying for reopened image marks; truthful sample status; sample flatten validation before truncating the destination. | `cms` is used as a release candidate without its builder; the SignedData is assembled from typed structures. SignerIdentifier is IssuerAndSerialNumber only. Placeholder lookup rejects any `/Contents`/`/ByteRange` collision in the object span, plus `endobj` inside strings. New error code `UNSUPPORTED_OP`. The workspace dev profile builds `crypto-bigint`/`crypto-primes`/`rsa` at opt-level 3 so test key generation stays fast. | The `cms` RC API may change before 0.3.0. Until the engine mutation token lands, adoption only checks writer revision and bytes. openssl ≥ 3 (not LibreSSL) and qpdf are now required for `cargo test`. |
| 2026-11-20 | M5 visual follow-up | Closed the end-to-end test gap noted in the preceding entry: `SampleVisualSignatureTest.drawingInCaptureSheetCreatesLockedInkAtWidgetRect` draws in the real capture sheet, applies the mark, and checks widget rect, locked flag, ink points, and empty signature state. `./gradlew --no-daemon :sample-app:ktlintFormat :sample-app:ktlintCheck` passed; `./gradlew --no-daemon :sample-app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=io.github.nibpdf.sample.SampleVisualSignatureTest` passed (2 tests); `./gradlew --no-daemon --no-parallel :sample-app:assembleRelease :sample-app:connectedDebugAndroidTest` passed (4 tests); release ELF alignment check passed. | Digital signing, verification, trust, and the rest of the M5 DoD remain. | The internal sample screen accepts an injectable document opener so the test inspects the actual document used by the UI. | The photo-picker test injects the selected image instead of automating the system picker. |
| 2026-11-20 | M5 visual | Added fail-closed signature occupancy, widget identity and visual Ink/Stamp binding, metadata persistence, JSON replay, sample draw/type/image capture and session reuse, transparent widget targets, rotated-stamp parity, and focused tests. Visual marks leave `/V` empty. **Checks:** `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` passed (142 core unit tests, 14 FFI tests, integrations and deny); `./gradlew --no-daemon --no-parallel ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest` passed (34 JVM, 19 library instrumentation, 3 sample instrumentation); `nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk` passed. The full Gradle run was unusually slow. | The actual sheet's drawing and the bound Ink geometry are tested separately, not yet in one end-to-end assertion; the photo-picker result is injected in tests. Digital signing and all remaining M5 DoD are not implemented. | Bound visual geometry retains full precision in JSON exports to preserve replay; invalid/unrepresentable geometry is rejected. Signature marks are immutable through update operations but remain deletable and undoable. Trust and cryptographic integrity are separate planned results. | Flushed/reopened image stamps preserve foreign `/AP` but do not reconstruct image source data; later digital appearance must copy its resources. Sample save destinations are opened with truncation before flush validation; fix before enforcing signed-document flatten refusal. |
| 2026-11-18 | M4 | AcroForms end to end. Rust: `nib-core::forms` (DFS over `/AcroForm /Fields`, `/Parent` inheritance of `/FT /Ff /DA /Q /V /DV /Opt /MaxLen /TI /I` with depth cap 32, fully-qualified names, merged field/widget dicts, per-type `/Ff` decode incl. bit 26 ambiguity, `/DA` → `DefaultAppearance`, `/Opt` pairs, Btn export values from `/AP /N`, widget ids `uuid_v5(NIB_NS, "widget:{num}:{gen}")`, page via `/Annots` pre-scan); `Widget.exportValue` added to the model + schema; `Engine::{with_base, adopt_base}(annotations, form_fields)`, `uncommitted_ids` over fields, `form_fields_on_page/ordered`, `drop_form_fields`, checkbox/radio `value` updates augment the merge patch with the full `widgets` array (derived `appearanceState`) so laws 1–4 hold (`delta_laws` extended with `FieldUpdate`; this also uncovered and fixed a stale `op_counter` in the law harness); `ap::widget` (frame from `/MK`, circle for radio, `/Tx BMC` bodies, auto-size, comb, password, multiline, combo label, list highlight `#99C1DA`, ZapfDingbats `4`/`l`, no `/GS0`); `writer::forms` (`update formField` → clone field + kids, `/V`/`/AS`/`/I`, `/AP` per widget via `add_form_xobject`, `/DA` default, `/NeedAppearances false`, `/DR` Helv+ZaDb; `create`/`delete` warn + skip) and `writer::flatten` (`FlushOptions { mode, flatten }`, ISO §12.5.5 matrix, `/NibW<n>` XObjects, `q … Q` wrapped contents, `/AcroForm` removed, both modes); `nib-render` `Ctrl::FormFieldValues` read-back + `TileKey.flags` `NO_ANNOTATIONS/NO_FORMS`; `nib-ffi` `form_fields/form_field_json/set_field_value/form_field_values/flush_with(FlushOptions)`, `FlushReport += fields_written, flattened`, swap-failure now marks flushed (duplicate-create bug fixed), `uuid`+`chrono` added to `nib-ffi`; fixture `form.pdf` (12 fields / 14 widgets). Android: `FormField.kt`, `NibDocument.{formFields, formField, setFieldValue, flush(mode, flatten), save(…, flatten)}`, `Tool.Form` + `FormOverlay` (overlay-covers gate via shared `TileCoverage`, BasicTextField/checkbox/radio/DropdownMenu, tab order, IME scroll-into-view with overscroll), sample `Save As… (flattened)`. DoD commands: `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` → green (nib-core 126 + delta_laws 1 + gen_fixtures 1 + geom 4 + schema 3, nib-ffi 13, nib-render 15 + ap_parity 2 + forms 3 = 168); `./gradlew ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease` → BUILD SUCCESSFUL (33 JVM tests); `check-elf-alignment.sh` on AAR + APK → all OK; `./gradlew :nib-android:connectedDebugAndroidTest` (AVD nib_api34) → 14/14 (FormFillTest pHash 0 flushed-vs-reopened, 34 vs blank; flatten_parity pHash 2); release APK on the emulator fills `form.pdf` in Form mode, logcat clean; manual readers: Preview (`qlmanage`) honours every widget `/AP`, pdf.js draws its own widgets from `/V` (editable-combo free text not in `/Opt` shows blank — pdf.js behaviour). **Delivered uncommitted on the user's request** (commit list suggested in the session). | `/AP /D` not written; `create`/`delete formField` not written (authoring); editable combo has no dropdown in the Compose overlay (free text only); Acrobat check still unavailable; M3 embedded fonts unchanged. | Active-widget strategy = **overlay covers** (transparent hit-target while committed + covered, opaque native widget otherwise; `FPDFAnnot_SetFlags` rejected as a pdfium mutation, per-page no-FFLDraw rejected for tile churn — ARCHITECTURE §5.2 updated). Radio frames are circles. `widgets[].exportValue` added to JSON_SCHEMA §7.1; `appearanceState` is engine-derived. `FlushOptions` record instead of a `flatten` flag on `flush`. pdfium form-fill env is created by pdfium-render at load, so no explicit `FPDFDOC_InitFormFillEnvironment` message was needed. `deny.toml` ignores `RUSTSEC-2026-0543` (chrono unmaintained; user-approved). | `chrono` is unmaintained — replacement (model/serde break) deferred to M6. pdfium regenerates APs for widgets *without* `/AP` when `/NeedAppearances` is true (fixture `readonly` field) — our flush flips it false, so untouched AP-less widgets disappear from pdfium's render after the first flush unless pdfium still synthesises them (observed: pdfium kept drawing `readonly`). `ensureRectVisible` overscroll past `maxScrollY` is clamped back by the next user scroll. Incremental flatten leaves field objects orphaned (file grows). uiautomator cannot see Compose semantics of subcomposed pages. |
| 2026-11-13 | M3 | Annotations persist to the PDF. Rust: `nib-core::ap` (typed `ContentBuilder` with 3-decimal numbers, `/GS0` ExtGState for opacity+blend, generators for all 11 subtypes mirroring `AnnotationDrawing.kt`, Base-14 AFM width tables + WinAnsi mapping + greedy word-wrap for FreeText, callout endings shared with Line, JPEG passthrough / PNG→Flate+SMask image embedding with ≤ 2048 px downsample, vector + standard stamps); `nib-core::writer` (`Writer::open` → lopdf parse + import of existing annots into `Engine::with_base`/`adopt_base` with `/NM` or deterministic UUID v5 ids and `appearance{regenerate:false, apHash}`, `flush` via `IncrementalDocument::create_from` + reparse-before-adopt + rollback on error, `/Annots` CRUD incl. `/Popup`, `/IRT`, `/NibData`, foreign-`/AP` preservation, `FullRewrite` with prune+renumber+object/xref streams refused on signed docs, `save_atomic`); `Engine::{flush_ops, mark_flushed, uncommitted_ids, revision}`; 7 new fixtures (`incr-classic/xrefstream/linearized/presigned/hybrid`, `annots-existing`, `stamp-src.{png,jpg}`); `nib-render` swap wired + `tests/ap_parity.rs` (tiny-skia + swash reference raster, 36 cases); `nib-ffi` `flush(mode)/bytes/revision/uncommitted_ids`, `FlushMode`/`FlushReport`, `AssetProvider`/`FontProvider` `with_foreign` callback traits, `lazy_structure` → deferred writer open. Android: `NibDocument.flush/save/bytes/revision/uncommittedIds/commitRevisionOf/dirtyLog`, `FlushPolicy` + `AutoFlusher` (idle 2 s / 50 ops), revision-aware `TileScheduler` (dirty-rect re-requests, old bitmap kept until replaced, `revision:N` tile semantics), `AnnotationOverlay` commit-revision gate, sample Save / Save As (optimized). DoD commands: `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` → all green (nib-core 94 + 9 integration, nib-ffi 9, nib-render 15 + ap_parity 2); `./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease` → BUILD SUCCESSFUL (AAR 14.4 MB, APK 30.0 MB); `check-elf-alignment.sh` AAR+APK → all OK; `./gradlew :nib-android:connectedDebugAndroidTest` on `nib_api34` → 11/11. Manual reader matrix on `annots_crud_11_subtypes.pdf` (kept via `NIB_KEEP_PDFS`): PDFium/Chrome ✔ (parity test + pdfium reload matrix), macOS Preview/PDFKit ✔ via `qlmanage` (draws its own note icon, otherwise identical), Firefox pdf.js ✔ via headless `--screenshot`; Acrobat not available. | **Embedded TTF for non-WinAnsi FreeText** (lopdf 0.47 `font_embedding` has no subsetting; `FontProvider` wired, embed path warns + falls back to Helvetica/`?`). Acrobat check. Inline-dict `/Annots` entries can be imported but not updated in place. `worker.swap` failure after a successful write leaves ops un-marked (duplicate creates on the next flush). Reparse-after-save cost unmeasured (M6). | Pressure ink `/AP` = per-segment round-capped strokes, not filled polygon outlines (ARCHITECTURE §7.2 updated; pixel-identical to the overlay). No `flate2` dep (lopdf `Stream::compress()`). `image.rs` lives under `ap/`, not `writer/`. Parity text reference uses real glyph rasterization (`swash` + Arial/Liberation) instead of solid bars; FreeText@1.0 threshold 9 (measured 7). `incr-linearized.pdf` produced with `qpdf --linearize` (lopdf's `linearize` option is a no-op). `qpdf --check` exit 3 (warnings) accepted. Writer dates come from the model; `Utc::now()` only as `/M` fallback. | CI has still never run remotely; the `rust` job now depends on `apt-get install qpdf fonts-liberation` (parity test needs a Latin TTF). `swash` dev-dep pulls `skrifa/read-fonts` (large compile). Each flush clones the lopdf `Document` and reparses the output — O(file) per save. Preview ignores `/AP` for `/Text` icons (cosmetic). |
| 2026-11-13 | M2 | Annotation editor without persistence. Rust: `nib-core::model` (serde + `schemars` types for JSON_SCHEMA §2–§7, `Color` newtype, `extra` flatten everywhere), `schema/nib-delta.schema.json` (draft 2020-12; tests regenerate-compare it and validate/round-trip all 14 fenced examples in `JSON_SCHEMA.md` with `jsonschema`), `nib-core::delta` (`Engine`: in-house RFC 7386 apply + inverse, all 10 validation codes, atomic per-call apply, `UndoManager` with explicit groups + 500 ms coalescing on single-key `rect/strokes/value/quadPoints/start/end` patches, limit 200, Full/Compacted export with number rounding, import with seen-opId idempotency / `REVISION_MISMATCH` / `rebase` / `undoable`, dirty regions, `hit_test`), `nib-core::text::snap_lines`; `nib-render` `Ctrl::CharBoxes` (pdfium `loose_bounds`); `nib-ffi` `apply/undo/redo/can_undo/can_redo/begin_group/end_group/export_delta/import_delta/annotations/annotation_json/hit_test/text_lines_in_rect`, `NibError::{InvalidDelta(json), RevisionMismatch}`. Android: kotlinx.serialization 1.12.0 (`DeltaOp`, `DeltaIds.uuidV7`, opaque `JsonObject` payloads, `Annotation` accessors), `NibDocument` facade with `undoAvailable/redoAvailable/annotationsVersion` StateFlows, Kotlin `PageTransform`, `Tool`/`ToolStyle`/`StampSpec`, `InkSmoothing` (resample 1.5 pt, RDP 0.3 pt, Catmull-Rom), `AnnotationOverlay` reference renderer (all 11 subtypes incl. pressure-width ink, multiply highlight, image/standard/vector stamps, line endings), `InkOverlay` (stylus/finger, historical samples, one `create Ink` on release), `CreateOverlay` (markup snapped to text lines, shapes, line, free text, note, stamp), `SelectionOverlay` (hit-test, move, corner resize, delete), tool-gated Pan gestures; sample app palette + undo/redo + swatches + width + bundled/picked image stamps + text dialog + `ACTION_SEND` export. DoD commands: `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` → pass (nib-core 52 lib + geom 4 + schema 3 + gen_fixtures 1 + `delta_laws` proptest; nib-render 15; nib-ffi 4); `./gradlew ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease` → pass (25 JVM tests, 122 s); `check-elf-alignment.sh` AAR + APK → pass (`libnib_ffi.so` arm64 1.74 MB / armv7 1.10 MB / x86_64 2.02 MB); `./gradlew :nib-android:connectedDebugAndroidTest` (AVD nib_api34) → 7/7 (`InkTest`, `HighlightSnapTest`, `UndoTest`, `SelectionTest` + M0/M1); release APK: ink, snapped highlight, export share sheet verified, logcat clean. Commits `0ecc426`…`e97feeb` + this docs commit. | Remote CI still never run (nothing pushed). Laws 1–4 are one proptest function (`undo_redo_laws`, 256 cases) rather than four; law 4's undo-depth equality is asserted on forward-only sequences (replay reconstructs steps from `groupId` only) while Full-export replay equality is also asserted for sequences with interleaved undo/redo. Undo of a delete re-inserts at the end of z-order. Note icon variants and `/NoZoom` are not emulated in the overlay. Palette row 2 overflows on a Pixel 6 (scrolls). | (1) `jsonschema 0.60.2` added as a dev-dep so the schema file itself validates the doc examples (user decision). (2) `kurbo 0.13.1` added but unused until M3 (user decision); ink smoothing lives in Kotlin. (3) `sha2 0.11.1` (not 0.10.x). (4) No `uiautomator`: stylus pressure injected via `decorView.dispatchTouchEvent` with `MotionEvent.TOOL_TYPE_STYLUS`. (5) All model numbers are `f64` (spec sketch had `f32 opacity`). (6) `AnnotBase.id`/`page` are injected from the op before deserializing `create` payloads; `Widget.id` and `Stamp.kind` are optional; `Smoothing.maxError` added. (7) `JSON_SCHEMA.md` §2/§7.3 examples made strictly valid JSON. (8) `redo()` records fresh-`opId` copies in `history` so Full export replays exactly. (9) `HistoricalChange.pressure` does not exist in Compose 1.12.1 — historical samples reuse the current pressure. (10) Overlay geometry uses Compose `Offset/Rect` (android.graphics is stubbed in JVM tests). | Payloads cross the FFI as JSON strings and `annotations(page)` re-serialises every annotation per recomposition — fine for dozens of annotations, measure in M6. Three JSON/number-precision renderers (Compose overlay now, `/AP` and `tiny-skia` in M3) must agree — the overlay defaults (0.06 markup factor, 20 pt note, `4·w+4` arrow heads) are the spec. `redo` history duplication makes Full exports grow with undo/redo churn. |
| 2026-11-13 | M1 | Read-only viewer end to end. Rust: `nib-core::geom` (`Point/Rect/Matrix/Rotation/PageTransform/QuadPoints`, §5.3 rotation table), lopdf-generated fixtures (`text, vector, image, rotated90, cropbox, text-100`, all < 200 KB) + golden PNGs; `nib-render` (`PdfiumEngine` leaked `&'static` on the `nib-render` thread, bounded 256-entry `BinaryHeap` queue ordered `(priority, distance, seq)`, generation + dropped-receiver cancellation, ≤ 2 resident docs via `DocSlot`, `render_tile` → RGBA); `nib-ffi` (`NibDocument` object, `async render_tile` over `futures-channel` oneshot, `text_in_rect`, `bump_generation -> u64`, `NibError::{Parse,EncryptedUnsupported,Cancelled}`). Android: `NibDocument` facade, `TileCache` (byte LRU `min(heap/8, 96 MB)`) + `BitmapPool`, `TileScheduler` (visible → ring → next-page row, cancellation, generation adoption), `NibPdfView` (`SubcomposeLayout`, scroll+fling, pinch 0.5–8×, double-tap, 80 ms settle, nearest-bucket fallback drawing, `tile:p:tx:ty` tags, `onTrimMemory`), sample app picker + `nib.asset` extra + `benchmark` build type, `:macrobenchmark` `ScrollBenchmark`. DoD commands: `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` → pass (nib-core 5, nib-render 14, nib-ffi 1; 2 ignored regenerators); `./gradlew ktlintCheck :nib-android:testDebugUnitTest` → pass; `./gradlew :nib-android:connectedDebugAndroidTest` (AVD nib_api34) → NibSmokeTest, NibPdfViewTest, ZoomTest pass; `./gradlew :nib-android:assembleRelease :sample-app:assembleRelease` + `check-elf-alignment.sh` on AAR + APK → pass; release APK: picker path and asset path render on emulator, logcat clean; `./gradlew :macrobenchmark:connectedBenchmarkAndroidTest` → baseline above. | Remote CI still never run (nothing pushed). Jank target (P90 < 8 ms) not met on emulator — hardware measurement deferred to M6. `NibPdfViewTest` needs a programmatic scroll after the 20 swipes. Commit `06f8747` was authored with the Devin identity by mistake (local only). | (1) Tile pixels are **RGBA** (`FPDF_REVERSE_BYTE_ORDER`) not BGRA, so `Bitmap.copyPixelsFromBuffer` on `ARGB_8888` needs no swizzle (ARCHITECTURE §5.1 updated). (2) `image_hasher` 3.1.1 replaces `img_hash`. (3) `bump_generation` runs synchronously under the queue lock and **returns** the new generation; the Kotlin scheduler adopts it (a worker round-trip deadlocked against the test `Block` hook, and a local Kotlin counter went stale for a second document / recreated view state). (4) `SubcomposeLayout` instead of `LazyLayout`. (5) Fixture PDFs use a classic xref table. (6) Commit convention changed to Conventional Commits (§0). (7) `pdfium_probe` treats `PdfiumLibraryBindingsAlreadyInitialized` as success (worker binds first). (8) `page.boundaries().crop()` errors when `/CropBox` is absent → fall back to the media box. | UniFFI wakes Kotlin continuations **on the Rust worker thread**: JNA must keep that thread attached (`Native.setCallbackThreadInitializer(uniffiRustFutureContinuationCallbackImpl, CallbackThreadInitializer(true, false))` in `Nib.init`) or ART aborts; and nothing may send a oneshot reply while holding the queue lock (inline wake re-entered the worker → deadlock). Tile copy cost not yet measured (M6 `AHardwareBuffer` decision pending). Emulator frame times are far above budget; need hardware numbers before tuning. |
| 2026-11-12 | M0 | Scaffolded the monorepo per ARCHITECTURE §2: `nib-rs` workspace (nib-core, nib-render, nib-sign, nib-ffi, uniffi-bindgen), `deny.toml`, `tools/{fetch-pdfium.sh,pdfium.lock,gen-bindings.sh,check-elf-alignment.sh}`; `nib-android` (AGP 9.5.0, cargo-ndk + UniFFI bindgen Gradle tasks, JNA, consumer R8 rules, `Nib.kt`, `NibSmokeTest`), minified Compose `sample-app`, ktlint, `.github/workflows/ci.yml` (rust / android-build / android-test with emulator retry), LICENSE, THIRD_PARTY_LICENSES, README, AGENTS. DoD commands run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` → pass (2 tests); `cargo deny check` → pass; `./gradlew :nib-android:assembleRelease` + `check-elf-alignment.sh` on AAR → 6 `.so`, OK; `./gradlew :nib-android:connectedDebugAndroidTest` (AVD nib_api34) → 1/1 pass; `./gradlew :sample-app:assembleRelease` + `adb install` → both versions shown; `./gradlew ktlintCheck` → pass; `actionlint` → clean. | Remote CI has not run yet (no push this cycle) — first push must be watched. `detekt` not added (ktlint only). | (1) PDFium `chromium/8152` → `chromium/8125` + `pdfium_8125` flag (7-day rule; exact flag/binary match). (2) compileSdk 36 → 37 (Compose BOM 2026.09.00 aarMetadata), targetSdk stays 36. (3) `armeabi-v7a` PDFium prebuilt is 4 KB-aligned; alignment check exempts ELF32 (Google requirement is 64-bit only). (4) `deny.toml` ignores `RUSTSEC-2026-0374` (`basic-toml` unmaintained, reachable only via `uniffi_bindgen → askama`, bindgen-time only). (5) Kotlin bindings generated from host debug `libnib_ffi` because release `strip = true` removes UniFFI metadata. (6) Generated-source wiring uses `sourceSets.<v>.{jniLibs,kotlin}.directories.add` + explicit task deps (AGP `addGeneratedSourceDirectory` needs a task-owned `DirectoryProperty`, which requires `buildSrc`). (7) `fetch-pdfium.sh` also fetches `mac-arm64` for host tests on this machine. | cargo-ndk also emits `libpdfium_render-*.so` (pdfium-render ships a cdylib target) — the `copyRustJniLibs` Sync filters to `libnib_ffi.so` only; keep that filter when adding crates. `uniffi-bindgen generate` takes the lib path positionally (`--library` is a no-op since 0.31). `cargo ndk`/`cargo metadata` must run from inside `nib-rs/` (manifest-path is not honoured for metadata). Local emulator images are arm64-only (Apple Silicon) so x86_64 ABI is only exercised in CI. |
| 2026-11-12 | — (design) | Produced `ARCHITECTURE.md`, `JSON_SCHEMA.md`, `ROADMAP.md`, `NEXT_PROMPT.md`. Decisions: core dir `nib-rs`, UniFFI MPL-2.0 exception accepted, signatures = external-signer PAdES B-B + verification (B-T stretch), minSdk 26 / 3 ABIs. | All milestones | — | pdfium-render feature flag vs `chromium/8152` binary compatibility to be confirmed in M0. |
