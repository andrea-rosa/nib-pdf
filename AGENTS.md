# AGENTS

Every session follows the cycle protocol in `docs/ROADMAP.md` §0; read
`docs/NEXT_PROMPT.md` first.

Git: do not stage or commit changes. At the end of every cycle, provide the
user with exact `git add` and `git commit` commands for the reviewed changes,
using Conventional Commit subjects (`feat(render): …`, `fix(android): …`,
`test(core): …`, `docs: …`, `build: …`). Do not push unless asked.

## Toolchain

- Rust `1.99.1` via `nib-rs/rust-toolchain.toml` (components `rustfmt`, `clippy`;
  targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`)
- `cargo-ndk` 4.1.2
- `cargo-deny` 0.20.2
- Android NDK `29.0.14206865`
- JDK 17+
- `openssl` ≥ 3 (OpenSSL, not LibreSSL; `brew install openssl`; CI: `apt-get install -y openssl`) — required `openssl cms -verify` oracle for `nib-sign` tests
- `qpdf` (`brew install qpdf`; CI: `apt-get install -y qpdf openssl fonts-liberation`) — dev-only CLI for `--check` and the linearized fixture; the parity test rasterizes text with Arial / Liberation Sans / DejaVu from host font dirs
- Emulator AVD: `nib_api34` (`system-images;android-34;google_apis;arm64-v8a`, `pixel_6`)

## Commands

Environment setup (one-off):

```sh
rustup toolchain install 1.99.1 --profile minimal -c rustfmt -c clippy \
  -t aarch64-linux-android -t armv7-linux-androideabi -t x86_64-linux-android
cargo +1.99.1 install cargo-deny --locked --version 0.20.2
~/Library/Android/sdk/cmdline-tools/latest/bin/sdkmanager \
  "system-images;android-34;google_apis;arm64-v8a"
~/Library/Android/sdk/cmdline-tools/latest/bin/avdmanager create avd \
  -n nib_api34 -k "system-images;android-34;google_apis;arm64-v8a" -d pixel_6 --force
```

PDFium binaries:

```sh
nib-rs/tools/fetch-pdfium.sh            # all assets (host + android)
nib-rs/tools/fetch-pdfium.sh --host-only
nib-rs/tools/fetch-pdfium.sh --android-only
```

Rust checks (run inside `nib-rs/`; `rust-toolchain.toml` pins 1.99.1):

```sh
cd nib-rs
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

Fixtures and golden images (committed; regenerate only when intentionally
changing them — goldens are compared by pHash ≤ 4):

```sh
cd nib-rs
cargo test -p nib-core --test gen_fixtures regenerate_fixtures -- --ignored   # needs qpdf for incr-linearized.pdf
cargo test -p nib-render regenerate_goldens -- --ignored
```

Writer / appearance-stream tests (M3):

```sh
cd nib-rs
cargo test -p nib-core ap::tests                 # 11 insta snapshots in nib-core/src/ap/snapshots/ + builder tests
cargo test -p nib-core writer::tests             # /Annots CRUD, import, 5-class incremental matrix (qpdf --check when installed)
NIB_KEEP_PDFS=/tmp/nib-m3 cargo test -p nib-core writer::tests   # keep every written PDF for manual reader checks
cargo test -p nib-render --test ap_parity        # pdfium render of written /AP vs tiny-skia reference, pHash ≤ 6 (FreeText@1.0 ≤ 9)
cargo test -p nib-ffi                            # flush/reopen, lazy_structure, asset provider, pdfium reload matrix
cargo insta test -p nib-core --accept            # only if /AP snapshots intentionally change (or delete the .snap and re-run)
qlmanage -t -s 1200 -o /tmp/nib-m3 /tmp/nib-m3/annots_crud_11_subtypes.pdf     # macOS Preview/PDFKit render → PNG
/Applications/Firefox.app/Contents/MacOS/firefox --headless --no-remote --profile /tmp/ffprof \
  --window-size=900,1000 --screenshot /tmp/nib-m3/pdfjs.png "file:///tmp/nib-m3/annots_crud_11_subtypes.pdf"  # pdf.js render
```

AcroForm tests (M4):

```sh
cd nib-rs
cargo test -p nib-core forms::                   # field-tree parser (fixture + synthetic 3-level hierarchy, merged dicts, unison)
cargo test -p nib-core ap::tests::snapshot_widget # 11 insta snapshots of widget /AP (text/comb/password/multiline/autosize, checkbox, radio, combo, list)
cargo test -p nib-core writer::tests::form_      # set values → flush → lopdf reload (+ qpdf --check), full rewrite, readonly untouched
cargo test -p nib-core writer::tests::flatten_   # flatten: /AcroForm gone, widgets baked into /Contents
cargo test -p nib-render --test forms            # pdfium form-fill read-back, flatten parity pHash ≤ 4, NO_FORMS render flag
cargo test -p nib-ffi form_tests                 # form_fields / set_field_value / validation codes / flush_with(flatten)
```

Visual signature tests (M5):

```sh
cd nib-rs
cargo test -p nib-core --test gen_fixtures regenerate_signature_fixture -- --ignored  # regenerates both signature fixtures
cargo test -p nib-core visual_signature
cargo test -p nib-ffi visual_signature
cd ..
# With an emulator:
./gradlew :nib-android:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=io.github.nibpdf.nib.VisualSignatureTest
./gradlew :sample-app:connectedDebugAndroidTest
```

Digital signing (M5):

```sh
cd nib-rs
cargo test -p nib-sign                           # RSA-2048 PSS/PKCS#1, P-256, P-384 keys + certs generated in-test; openssl + qpdf oracles are REQUIRED (tests panic if missing)
NIB_OPENSSL=/opt/homebrew/bin/openssl cargo test -p nib-sign   # macOS: /usr/bin/openssl is LibreSSL and is refused
NIB_KEEP_PDFS=/tmp/nib-m5 cargo test -p nib-sign                # keep every signed PDF
cargo test -p nib-core writer::tests::stage      # private staging / adoption / stale detection
cargo test -p nib-ffi signature_ops              # apply/import reject target "signature" with UNSUPPORTED_OP
cargo test -p nib-ffi                            # FFI private staging, retry, verified completion, stale edit
```

`FlushOptions { mode, flatten }` drives `Writer::flush`; `TileKey.flags` bits are `NO_ANNOTATIONS = 1`, `NO_FORMS = 2` (0 = default full render). `cargo deny check` ignores `RUSTSEC-2026-0543` (chrono unmaintained, no vulnerability) next to the existing `RUSTSEC-2026-0374`.

Delta JSON Schema (`nib-rs/nib-core/schema/nib-delta.schema.json`, generated by
`schemars` from the `nib_core::model` types; `tests/schema.rs::schema_is_up_to_date`
fails on drift, `schema_examples_validate` checks every fenced JSON block in
`docs/JSON_SCHEMA.md` — keep those examples strictly valid JSON):

```sh
cd nib-rs
cargo test -p nib-core --test schema schema_regenerate -- --ignored
cargo test -p nib-core                      # model/delta unit tests + proptest laws
cargo insta test -p nib-core --accept       # only if export snapshots intentionally change (or delete the .snap and re-run)
```

Generated Kotlin bindings and ELF page-size check:

```sh
nib-rs/tools/gen-bindings.sh <path-to-libnib_ffi.so|dylib> <out-dir>
nib-rs/tools/check-elf-alignment.sh <dir|.so|.aar|.apk>...
```

Note: `uniffi-bindgen generate` takes the library path **positionally**;
the `--library` flag is a deprecated no-op since uniffi 0.31.

Android cross-build (run from `nib-rs/`; `cargo ndk` resolves the manifest
from the working directory):

```sh
cd nib-rs
ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/29.0.14206865 \
  cargo ndk -t arm64-v8a -P 26 -o <out-dir> build -p nib-ffi --release
```

## Android

Gradle (wrapper is Gradle 9.8.2, AGP 9.5.0 — no local Gradle needed):

```sh
./gradlew :nib-android:assembleRelease   # AAR at nib-android/build/outputs/aar/
./gradlew :sample-app:assembleRelease    # R8-minified APK
./gradlew ktlintFormat ktlintCheck
./gradlew :nib-android:uniffiBindgenDebug   # regenerate Kotlin bindings only
./gradlew :nib-android:testDebugUnitTest    # JVM tests (TileCache, ZoomBucket, DeltaOpSerialization, PageTransform, InkSmoothing, FlushPolicy, FormFieldParsing)
./gradlew :nib-android:connectedDebugAndroidTest   # NibSmokeTest, NibPdfViewTest, ZoomTest, InkTest, UndoTest, HighlightSnapTest, SelectionTest, PersistenceTest, NoFlickerTest, SaveAsOptimizedTest, ImportedNotDoubleDrawnTest, FormFillTest, ImeScrollTest, FormUndoTest, KeyStoreSignTest
./gradlew :sample-app:testDebugUnitTest    # SyntheticPdfTest (generated benchmark PDFs)
./gradlew :macrobenchmark:connectedBenchmarkAndroidTest   # StartupBenchmark, ScrollBenchmark, InkLatencyBenchmark, FlushBenchmark (~5 min; keep the device awake + unlocked)
python3 macrobenchmark/tools/benchmark_gate.py macrobenchmark/build/outputs/connected_android_test_additional_output/benchmark/connected \
  --baseline macrobenchmark/baselines/pixel7a-api37.json   # >15 % regression gate; exit 2 on device/SDK mismatch
python3 -m unittest discover -s macrobenchmark/tools       # gate unit tests
```

Benchmark JSON lands in
`macrobenchmark/build/outputs/connected_android_test_additional_output/benchmark/connected/<device>/`.
With several devices attached, pick one with `ANDROID_SERIAL=<serial>`. Suites,
metrics, launch extras (`nib.synthetic=text:<MB>`, `nib.tool`,
`nib.bench.flushOps`) and the `NibTrace` marker names are in
`macrobenchmark/README.md`. On Android 17 (API 37) devices the Compose UI
instrumentation tests fail in `Espresso.onIdle` (`InputManager.getInstance`
was removed); run `connectedDebugAndroidTest` on the API 34 AVD.

The build runs `cargoBuild<Variant>` (cargo-ndk) and `uniffiBindgen<Variant>`
automatically. Debug builds only the host ABI; override with
`-Pnib.abis=arm64-v8a,x86_64`. UniFFI bindings are generated from the host
debug `libnib_ffi` because `strip = true` in the release profile removes the
ELF symbols the UniFFI metadata lives in.

CI (`.github/workflows/ci.yml`) runs:

```sh
./gradlew --no-daemon ktlintCheck :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh \
  nib-android/build/outputs/aar/nib-android-release.aar \
  sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon :nib-android:connectedDebugAndroidTest -Pnib.abis=x86_64
```

Emulator (`adb` lives in `~/Library/Android/sdk/platform-tools`):

```sh
~/Library/Android/sdk/emulator/emulator -avd nib_api34 \
  -no-window -no-audio -no-boot-anim -no-snapshot &
adb wait-for-device
adb shell getprop sys.boot_completed          # wait for "1"
adb install -r sample-app/build/outputs/apk/release/sample-app-release.apk
adb shell am start -n io.github.nibpdf.sample/.MainActivity
adb shell am start -n io.github.nibpdf.sample/.MainActivity --es nib.asset text-100.pdf  # open a bundled fixture
adb shell am start -n io.github.nibpdf.sample/.MainActivity --es nib.asset form.pdf      # AcroForm fixture; tap the Form chip, then a field
adb shell input text Andrea                  # types into the focused form field
adb exec-out screencap -p > /tmp/nib-sample.png
adb shell uiautomator dump /sdcard/ui.xml && adb shell cat /sdcard/ui.xml
adb shell input swipe 400 900 700 1100 300   # draw with the active tool (Ink/Highlight/Square…)
adb logcat -d | grep -E "AndroidRuntime|FATAL|nib"
```

Sample-app palette chips carry `testTag("tool:<Tool>")`; annotations expose
`contentDescription = "annotation:<Subtype>"` and `testTag("annot:<id>")`; the
selection Delete button is `testTag("deleteSelected")`; Save / Save As / Save As (flattened) chips are `testTag("action:save")` / `testTag("action:saveAs")` / `testTag("action:saveAsFlat")`; form widgets (Form tool) carry `testTag("field:<name>")` (radio kids `field:<name>:<export>`) and `contentDescription = "widget:<fieldType>"`; `NibPdfViewState.imeBottomOverridePx` fakes the IME inset in tests; tile boxes carry `stateDescription = "revision:N"`. Instrumentation tests open views with `setUpView(flushPolicy = FlushPolicy(enabled = false))` (`M2TestSupport.kt`) so auto-flush never races assertions. Stylus input with pressure
is injected in tests through `activity.window.decorView.dispatchTouchEvent`
(`MotionEvent.TOOL_TYPE_STYLUS`), see `nib-android/src/androidTest/.../M2TestSupport.kt`.
