# Macrobenchmarks (M6)

Android performance suites for the ROADMAP §7 budgets. They drive the
`sample-app` `benchmark` build type and read `nib-android` trace markers
(`NibTrace.kt`) through custom Perfetto metrics (`NibMetrics.kt`).

`NibDocument.open` collects all page metadata in one FFI/render-worker batch;
`pageInfoMs` measures that batch and still includes PDFium's per-page geometry
work.

| Suite | Input | Metrics | Budget |
|---|---|---|---|
| `StartupBenchmark.firstVisiblePage10Mb` | generated 10 MB text PDF (`nib.synthetic=text:10`), 5 COLD iterations | `firstVisiblePageMs` (`NibDocument.open` → every visible tile of page 1 drawn), `openMs`, `fromBytesMs`, `pageInfoMs` | first page ≤ 300 ms |
| `ScrollBenchmark.scroll` | `text-100.pdf`, 3 COLD iterations, 5 swipes | `frameOverrunMs`, `frameDurationCpuMs`, `tileRasterMs` (Rust `render_ms`, PDFium raster only), `tileLatencyMs` (`renderTile` FFI incl. render-worker queue wait + Bitmap conversion) | `frameOverrunMs` P90 ≤ 4 ms; tile P50 ≤ 12 / P95 ≤ 30 ms |
| `InkLatencyBenchmark.inkTouchToDraw` | `text-100.pdf`, Ink tool preselected, 5 strokes × 3 iterations | `inkTouchToDrawMs` (input event uptime → live-stroke draw pass; excludes scan-out) | ≤ 1 frame |
| `FlushBenchmark.flush50Ops20Mb` | generated 20 MB text PDF, 50 Square creates, 5 COLD iterations | `flush50OpsMs` (Rust `flush_with` only) | ≤ 150 ms |

Sample launch extras (`sample-app/.../BenchmarkHooks.kt`):

- `nib.asset=<name>`: open a bundled fixture.
- `nib.synthetic=text:<MB>` (1–64): open a deterministic, uncompressed
  text PDF generated in memory (`SyntheticPdf.kt`); nothing large is committed.
- `nib.tool=<Tool>`: preselect a palette tool (e.g. `Ink`).
- `nib.bench.flushOps=<n>`: apply `n` Square creates and flush once before
  the document is shown.

## Running

```sh
./gradlew --no-daemon :macrobenchmark:connectedBenchmarkAndroidTest   # all connected devices; set ANDROID_SERIAL to pick one
gh workflow run android-benchmark.yml                                # API 34 x86_64 CI candidate (also runs Mondays)
```

Keep the device awake and unlocked for the whole run (about 5 minutes). A
secure lockscreen makes the synthetic-PDF suites time out waiting for
`"Nib — "`. Results land in
`macrobenchmark/build/outputs/connected_android_test_additional_output/benchmark/connected/<device>/`.

## Regression gate (> 15 %)

```sh
python3 macrobenchmark/tools/benchmark_gate.py \
  macrobenchmark/build/outputs/connected_android_test_additional_output/benchmark/connected \
  --baseline macrobenchmark/baselines/<profile>.json            # exit 1 on > 15 % regression
python3 macrobenchmark/tools/benchmark_gate.py <results> --baseline <new>.json --update --note "<device, conditions>"
python3 -m unittest discover -s macrobenchmark/tools          # gate unit tests
```

The gate compares metric medians and sampled P50/P90/P95 values
(`frameCount` and P99 are excluded). A metric fails when
`current > base + max(|base| × 0.15, 1.0)`. The gate refuses to compare runs
from a different device model or SDK (exit 2), so emulator, host and
physical-device numbers never mix.

Baselines:

- `pixel7a-api37.json`: physical Pixel 7a (Tensor G2), Android 17 / API 37
  user build, `CompilationMode.None`, battery power, 2026-11-26. Manual
  physical-device reference; this is not the API 34 x86_64 CI baseline.
- `api34-x86_64.json`: reserved for a reviewed API 34 x86_64 CI candidate.
  Until it is committed, `.github/workflows/android-benchmark.yml` uses
  `--allow-missing` and uploads the raw `benchmarkData.json` artifact for review.
