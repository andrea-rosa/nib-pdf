You are working in the `nib-pdf` repository: an open-source (Apache-2.0) Android PDF editing library with a Rust core (`pdfium-render` + `lopdf`), UniFFI Kotlin bindings, and a Jetpack Compose UI layer. **M0 is done** (cycle 2026-11-12): the monorepo builds, `cargo test` / `cargo deny` pass, the AAR ships `libpdfium.so` + `libnib_ffi.so` for 3 ABIs, `NibSmokeTest` passes on an emulator and the minified sample app shows `Nib.version()` / `Nib.pdfiumVersion()`. Your task in this session is **Milestone M1** exactly as specified in `docs/ROADMAP.md §2`.

## Read first (in this order, nothing else is required)
1. `AGENTS.md` — every build/test command that is known to work, toolchain pins, emulator instructions.
2. `docs/ROADMAP.md` — §0 (cycle protocol, mandatory), §2 (M1 scope, dependencies, risks, DoD, tests), §8 cycle log row for M0 (deviations + pitfalls).
3. `docs/ARCHITECTURE.md` — §4 (threading model: `RenderWorker`, `PdfiumEngine`, generations, async tiles), §5 (tile pipeline, zoom buckets, coordinate spaces §5.3 incl. the rotation table, `PageTransform`), §9 (public API surface — implement only the M1 subset).
4. Skim `docs/JSON_SCHEMA.md §1` only to know the model crate exists; do **not** implement model/delta types in M1 (that is M2).

## Goal
A working **read-only viewer**: Kotlin `NibDocument` opens a PDF from bytes, `NibPdfView` lays pages out vertically, renders 512 px tiles through a single Rust `RenderWorker` thread with priority/cancellation, supports pinch-zoom (0.5×–8×), fling, double-tap-to-zoom and zoom buckets. The sample app opens any PDF via `ACTION_OPEN_DOCUMENT`.

## Scope (do all of these; details in ROADMAP §2)
- `nib-core::geom`: `Point`, `Rect`, `Matrix`, `Rotation`, `PageTransform` (ARCHITECTURE §5.3 rotation table), `QuadPoints` normalisation. Property tests (`proptest`) `transform_roundtrip` (10k cases, all 4 rotations, random crop boxes), `rotation_table`.
- `nib-render`: `PdfiumEngine` (one per process, lazily created, `Send + !Sync`, lives on the `RenderWorker` thread; debug assertion on thread id), `Renderer` with `open(bytes)`, `page_count`, `page_info(index) -> PageInfo { crop_box, rotation, label }`, `render_tile(TileRequest) -> TileResult` (BGRA, raw `Vec<u8>`, **no `image` feature**), bounded `BinaryHeap` priority queue (`visible > prefetch > thumbnail`, then distance to viewport centre), generation-based cancellation, double-buffered documents (prepared for M3 revision swaps: at most 2 pdfium documents resident). Host tests use `nib-rs/target/pdfium/<host>` exactly like `pdfium_probe_loads` does.
- `nib-ffi`: `NibDocument` object (`#[derive(uniffi::Object)]`), `from_bytes`, `page_count`, `page_info`, `async fn render_tile` (UniFFI async with a oneshot future; dropping the future cancels), `text_in_rect`, extend `NibError` with `Parse(String)`, `EncryptedUnsupported`, `Internal(String)`. Keep `nib_version()` / `pdfium_version()` and `NibSmokeTest` green.
- Kotlin (`nib-android`): `NibDocument` facade (coroutines; `Dispatchers.Default`), `TileCache` (byte-budget LRU `min(1/8 heap, 96 MB)` + `BitmapPool`), `TileScheduler` (visible → prefetch ring), `NibPdfView` composable (vertical `LazyLayout`, gap 8 dp, pinch 0.5×–8×, fling, double-tap-to-zoom, zoom buckets `scale = 2^(bucket/2)`, nearest-bucket upscaling during gestures, 80 ms settle debounce), `onTrimMemory` hook, semantics `testTag("tile:<page>:<tx>:<ty>")`.
- Sample app: open a PDF via `ACTION_OPEN_DOCUMENT` and display it with `NibPdfView`.
- Fixtures (`nib-rs/nib-core/tests/fixtures/`, each < 200 KB, generated in-repo — no copyrighted documents): text, vector, image, rotated-90, cropbox-offset, and a 100-page text PDF for the scroll test. Golden PNGs for the 5 render fixtures.

## Constraints
- Pin every new dependency to a version published **≥ 7 days before today**; check crates.io / Maven dates (`curl -sS -A "nib-pdf" https://crates.io/api/v1/crates/<name>/versions`; Maven: `curl -sSI <pom-url> | grep -i last-modified`). Record versions in the M1 dependency table in `docs/ROADMAP.md §2`.
- Do **not** enable pdfium-render `image` or `thread_safe` features; keep `default-features = false`, `features = ["pdfium_8125"]`. Do not change the PDFium binary tag (`chromium/8125`) — flag and binary are pinned together.
- All `FPDF_*` calls stay on the `RenderWorker` thread; no `tokio`. UniFFI async uses `futures-channel` oneshot (or `std` + a tiny waker) — check license (must be MIT/Apache-2.0).
- Never weaken `deny.toml` or license policy to make CI pass; if blocked, stop and report.
- No annotations, no writing, no lopdf usage beyond the existing dependency. Do not start M2.
- Do not push; do not create a PR unless asked. Commit as `cycle(M1): <summary>`.

## Definition of Done (all must be true; copy the checklist into the cycle log with results)
- [ ] Rust: golden-render tests for 5 fixtures (text, vector, image, rotated 90, cropbox offset) pass with pHash distance ≤ 4 against committed PNGs (`image` + `img_hash` as **dev-dependencies** only).
- [ ] Rust: `PageTransform` property tests (`to_user(to_device(p)) ≈ p` within 1e-3 pt, rect normalisation) pass for 10k cases.
- [ ] Rust: cancellation test — enqueue 200 tiles, bump generation, assert ≤ 5 old-generation tiles rendered.
- [ ] Android `NibPdfViewTest`: opens the 100-page fixture, `swipeUp()` ×20 to the end and back, no OOM, semantics node `tile:99:0:0` exists.
- [ ] Android `ZoomTest`: pinch to 4×, bucket = 4, a bucket-4 tile is requested within 300 ms after gesture end.
- [ ] Macrobenchmark `ScrollBenchmark` recorded on the emulator (baseline only, not gating).
- [ ] Sample app opens any PDF from the system picker.
- [ ] `cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check` pass; `./gradlew ktlintCheck :nib-android:assembleRelease :sample-app:assembleRelease` pass; `nib-rs/tools/check-elf-alignment.sh` on the AAR passes; `./gradlew :nib-android:connectedDebugAndroidTest` passes (incl. `NibSmokeTest`).
- [ ] `AGENTS.md` updated with any new command.

## Known pitfalls carried over from M0 (toolchain facts — do not rediscover them)
- **AGP 9.5.0 / Gradle 9.8.2, built-in Kotlin, new DSL.** Do not apply `org.jetbrains.kotlin.android`; only `org.jetbrains.kotlin.plugin.compose` is applied (sample-app). `android.ndkDirectory` does not exist — use `androidComponents.sdkComponents.ndkDirectory`. Source-set additions use `sourceSets.<variant>.{jniLibs,kotlin}.directories.add(...)` plus explicit task `dependsOn` (see `nib-android/build.gradle.kts`); `variant.sources.*.addGeneratedSourceDirectory` needs a task-owned `DirectoryProperty` (would require `buildSrc`). `compileSdk = 37` is required by Compose BOM 2026.09.00; `targetSdk = 36`. Configuration cache is **on** — avoid `upToDateWhen` closures capturing `project`.
- **UniFFI 0.32.2**: `uniffi-bindgen generate <lib>` takes the library path positionally (`--library` is a no-op). Bindings are generated from the host **debug** `libnib_ffi` (`bindgenLib<Variant>` task) because the release profile's `strip = true` removes UniFFI metadata. `cargo ndk` / `cargo metadata` must run from inside `nib-rs/` (manifest-path is ignored for metadata). Generated Kotlin currently needs no kotlinx-coroutines; adding `async fn` exports **will** make it import `kotlinx.coroutines` — add `kotlinx-coroutines-android` (`1.11.0`, 2026-05-07 is safe) to `nib-android` when you do.
- cargo-ndk also emits `libpdfium_render-*.so`; `copyRustJniLibs<Variant>` filters to `libnib_ffi.so` only. Keep that filter.
- The system `/system/lib64/libpdfium.so` is not accessible to apps; `Nib` loads `pdfium` then `nib_ffi`; Rust binds with `Pdfium::bind_to_system_library()` on Android and `bind_to_library(dir)` on hosts (`NIB_PDFIUM_DIR` or `nib-rs/target/pdfium/{linux-x64,mac-arm64}`; run `nib-rs/tools/fetch-pdfium.sh` first).
- `armeabi-v7a` PDFium prebuilt is 4 KB-aligned; `check-elf-alignment.sh` exempts ELF32 by design. 64-bit `.so` must stay `0x4000`.
- `deny.toml` has per-crate MPL-2.0 exceptions for `uniffi*` and ignores `RUSTSEC-2026-0374` (`basic-toml`, bindgen-time only). Add new crates only if their license is in the allow-list.
- Local emulator images are **arm64-only** (Apple Silicon host). Use AVD `nib_api34` (API 34 google_apis arm64-v8a; start command in `AGENTS.md`). Debug builds compile only the host ABI by default (`-Pnib.abis=` to override); CI passes `-Pnib.abis=x86_64`.
- Remote CI (`.github/workflows/ci.yml`) has **never run** — nothing has been pushed. If you are asked to push, watch the first run and fix the workflow (likely spots: SDK package install, emulator boot time, cargo-ndk cache key).
- Debug ABI/`lto="fat"` release builds of `nib-ffi` take a few minutes per ABI; use `assembleDebug` / `connectedDebugAndroidTest` while iterating and reserve `assembleRelease` for the final DoD gate.

## End-of-session protocol (mandatory — `docs/ROADMAP.md §0`)
1. Run the DoD commands; record exact commands + results.
2. Update `docs/ROADMAP.md`: summary table row M1 → `[x]` (or `[~]` with a precise remaining list), §2 status + dependency table with pinned versions, append a row to §8 Cycle log (date, summary, deferred, deviations, new risks). If the design changed, update `ARCHITECTURE.md` and reference it in the log.
3. Rewrite **this file** (`docs/NEXT_PROMPT.md`) for **Milestone M2** following the same structure: read-first list (ROADMAP §3, JSON_SCHEMA §2–§8, ARCHITECTURE §5.2, §5.4), goal, scope, constraints, DoD, pitfalls (include anything you learned in M1), end-of-session protocol.
4. Update `AGENTS.md` with every new command you ran.
5. Commit `cycle(M1): …` with a clean tree (`git status` empty). Do not push.
