# Next development cycle — M6 performance: open-path hotspot and x86_64 CI baseline

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree and connected devices, then read M6
and the latest cycle log in ROADMAP, [ARCHITECTURE.md](./ARCHITECTURE.md)
sections 5, 10 and 11, [macrobenchmark/README.md](../macrobenchmark/README.md),
`nib-android/.../NibDocument.kt` (`open`/`openLoaded`), `NibTrace.kt`, and
`nib-rs/nib-ffi/src/document.rs` (`page_info`, `page_count`). Preserve all
user changes. Do not stage, commit, or push.

## Current state

- M6 remains `[~]`. Malformed-input hardening, fuzz smoke runs, daily fuzz CI,
  host Criterion baseline `m6-mac-arm64-final-2026-11-26`, Android performance
  suites and a validated >15% Macrobenchmark gate are done. 24-hour fuzz,
  coverage, the x86_64 CI performance baseline, performance budgets, API
  freeze and release remain.
- Macrobenchmark suites: `StartupBenchmark`, `ScrollBenchmark` (frames, tile
  raster and tile delivery latency), `InkLatencyBenchmark` and
  `FlushBenchmark`. They read `NibTrace` markers; large inputs are generated
  in memory via `nib.synthetic=text:<MB>`. Gate:
  `macrobenchmark/tools/benchmark_gate.py` (9 unit tests, run in CI
  `android-build`). It refuses cross-device comparisons.
- Physical baseline `macrobenchmark/baselines/pixel7a-api37.json` (Pixel 7a,
  Tensor G2, Android 17 / API 37, `CompilationMode.None`, two runs). No
  budget is met:
  - First visible page (10 MB): 1317 / 1455 ms against 300 ms. `nib.open` is
    1034 / 1135 ms: `from_bytes` 148 / 159 ms plus the eager per-page
    `pageInfo` FFI loop over ~700 pages, 857 / 970 ms.
  - Tile raster (`render_ms`) P95 1 ms. Delivery, including the single
    render-worker queue, is P50 ~35 ms and P95 93–100 ms.
  - `frameOverrunMs` P90 13.8 ms against 4 ms.
  - Ink P50 19 ms and P90 35–37 ms against 16.7 ms.
  - Flush of 50 ops on 20 MB: 184 / 177 ms against 150 ms.
- API 34 arm64 emulator (diagnostic only): first page 1256 ms, flush 71 ms,
  `frameOverrunMs` P90 60.8 ms. This Apple Silicon host cannot run the API 34
  x86_64 image; the CI `android-test` job (ubuntu, KVM) can.
- On Android 17 devices every Compose UI instrumentation test fails in
  `Espresso.onIdle` (`NoSuchMethodException: InputManager.getInstance`). Use
  the API 34 AVD for `connectedDebugAndroidTest` (31/31 pass) until
  androidx.test is upgraded.
- Startup medians drift up to +13% run-to-run, close to the 15% gate. Keep the
  device awake and unlocked during benchmark runs; a secure lockscreen makes
  the synthetic suites time out.

## Objective

Fix the largest measured hotspot, the eager `pageInfo` loop in
`NibDocument.open`, and produce the first API 34 x86_64 CI benchmark baseline.
Fix only what the traces show. Re-measure with the existing suites and the
gate. Never treat emulator or host numbers as physical-device results.

## Definition of done

1. Profile `nib.pageInfo` (per-call FFI overhead vs per-page PDFium/lopdf
   work) before changing anything. Then replace the per-page round trips, e.g.
   with a single batched `page_infos()` FFI call, or by reading sizes from the
   already-parsed structure. Preserve `PageInfo` semantics (rotation,
   cropBox) and add focused Rust and JVM/instrumentation tests.
2. Re-run `StartupBenchmark` on a physical device. Record before/after
   `openMs`, `pageInfoMs` and `firstVisiblePageMs`. Update the device
   baseline only with a note describing the change.
3. Add a CI benchmark job, manual dispatch or scheduled, on the API 34 x86_64
   emulator. It runs `:macrobenchmark:connectedBenchmarkAndroidTest`, uploads
   the JSON, and runs the gate with `--allow-missing` until a reviewed
   `api34-x86_64` baseline is committed.
4. If time remains, investigate the next measured hotspot (tile queueing or
   scroll frame overruns) using the traces only.
5. Run targeted Rust/Gradle checks, `connectedDebugAndroidTest` on the API 34
   AVD, and the benchmark suite. Update M6 status and the cycle log in
   ROADMAP, then rewrite this prompt.
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
ANDROID_SERIAL=<device> ./gradlew --no-daemon :macrobenchmark:connectedBenchmarkAndroidTest
python3 macrobenchmark/tools/benchmark_gate.py \
  macrobenchmark/build/outputs/connected_android_test_additional_output/benchmark/connected \
  --baseline macrobenchmark/baselines/pixel7a-api37.json
```

For emulator setup and Android checks, use the toolchain and AVD instructions
in [AGENTS.md](../AGENTS.md). Verify the connected target's model, ABI and API
before claiming any result.
