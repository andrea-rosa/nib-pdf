# Next development cycle — M6 profile batched page info and qualify API 34 x86_64

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree and connected devices, then read M6
and the latest cycle log in ROADMAP, [ARCHITECTURE.md](./ARCHITECTURE.md)
sections 5, 10 and 11, [macrobenchmark/README.md](../macrobenchmark/README.md),
`nib-android/.../NibDocument.kt` (`open`/`openLoaded`), `NibTrace.kt`,
`nib-rs/nib-ffi/src/document.rs` (`page_info`/`page_infos`) and
`nib-rs/nib-render/src/worker.rs`. Preserve all user changes. Do not stage,
commit, or push.

## Current state

- M6 remains `[~]`. The completed batch optimization adds
  `RenderWorker::page_infos` (one control message), the UniFFI
  `page_infos()` endpoint, and uses that single batch in `NibDocument.open`.
  The existing per-page `page_info()` remains available. PDFium continues to
  supply page sizes, rotation, crop box and labels.
- The Pixel 7a/API 37 baseline is unchanged. Before the optimization its
  median `pageInfoMs` was 856.6 ms of 1,033.7 ms `openMs` (~700 pages);
  `fromBytesMs` was 147.6 ms. That trace did not separate the FFI/worker
  round-trip overhead from per-page PDFium geometry work.
- Post-change local API 34 arm64 AVD diagnostic: first visible page 936.7 ms,
  `openMs` 590.1 ms, `fromBytesMs` 176.1 ms, `pageInfoMs` 415.4 ms, flush
  85.0 ms, ink P50/P90 18/24 ms, tile delivery P50/P95 43.4/128.3 ms. This
  is not a physical-device result, not a controlled before/after comparison,
  and not an x86_64 baseline.
- Rust parity tests cover rotated, cropped and 100-page documents.
  `NibDocumentOpenTest` also covers those cases. API 34 arm64 connected
  instrumentation passed 26 library + 6 sample tests.
- `.github/workflows/android-benchmark.yml` runs weekly and on manual
  dispatch on API 34 x86_64, uploads benchmark JSON and uses
  `--allow-missing` until a reviewed baseline is committed. It has not yet
  run in CI. The Apple Silicon host cannot boot the x86_64 emulator image.
- `.github/workflows/fuzz-qualification.yml` is a manual workflow for the
  24-hour fuzz gate: five chained 5-hour segments per target that carry the
  corpus forward. It has not run yet. It is free only if the repo is public;
  dispatch it with `gh workflow run fuzz-qualification.yml`.
- Other M6 work remains: physical-device budgets, 24-hour fuzz qualification,
  coverage thresholds, API freeze, publishing and release. On Android 17,
  Compose instrumentation currently fails in Espresso `onIdle` with
  `NoSuchMethodException: InputManager.getInstance`; use API 34 for those
  tests until androidx.test is upgraded.

## Objective

Validate the batched open path with controlled measurements, produce and
review the first API 34 x86_64 CI candidate, and continue M6 only where traces
identify a hotspot. Never mix emulator, host and physical-device results.

## Definition of done

1. On a device, profile the old single-page `page_info()` FFI calls against
   `page_infos()` for the same large PDF. Separate per-call bridge/worker
   overhead from per-page PDFium work; use a test/diagnostic path rather than
   adding an unnecessary public API. Preserve rotation and crop-box semantics.
2. Re-run `StartupBenchmark` on the physical Pixel 7a (prefer two runs) and
   report `openMs`, `pageInfoMs`, `fromBytesMs` and `firstVisiblePageMs`.
   Update `pixel7a-api37.json` only with a note describing the change and
   conditions; retain the old numbers in the cycle log.
3. Trigger `gh workflow run android-benchmark.yml`; verify the emulator model,
   ABI and API from its JSON; review the uploaded results and gate output.
   Commit an `api34-x86_64` baseline only after the candidate is stable and
   reviewed. Until then keep `--allow-missing`.
4. Investigate tile queueing or scroll frame overruns only if the new traces
   still identify them as significant.
5. Run targeted Rust/Gradle checks and the API 34 connected instrumentation
   suites. Update M6 status and the cycle log in ROADMAP, then rewrite this
   prompt for the following cycle.
6. Leave all changes uncommitted. Report final `git status` and exact
   Conventional Commit commands.

## Verification commands

```sh
cd nib-rs
cargo +1.99.1 fmt --check
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test
cargo deny check
cd ..
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :sample-app:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease
python3 -m unittest discover -s macrobenchmark/tools
ANDROID_SERIAL=emulator-5554 ./gradlew --no-daemon :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
ANDROID_SERIAL=<physical-device> ./gradlew --no-daemon :macrobenchmark:connectedBenchmarkAndroidTest
gh workflow run android-benchmark.yml
```

For emulator setup and Android checks, use the toolchain and AVD instructions
in [AGENTS.md](../AGENTS.md). Verify every benchmark target's model, ABI and
API before recording results.
