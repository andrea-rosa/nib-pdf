# Next development cycle — M6 performance qualification

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree, then read M6 and the latest cycle
log in that document, [ARCHITECTURE.md](./ARCHITECTURE.md) sections 6 and 10,
the existing Macrobenchmark module, and `nib-rs/nib-core/benches/m6_performance.rs`.
Preserve all user changes. Do not stage, commit, or push.

## Current state

- M6 remains `[~]`. Malformed-input hardening, initial fuzz smoke runs, and
  daily scheduled fuzz CI are complete; 24-hour fuzz qualification, coverage,
  performance qualification, API freeze, and release criteria remain.
- Criterion `=0.7.0` benches now cover geometry, Ink `/AP` generation, 50-op
  delta apply, and 50-op compacted export. The named host baseline is
  `m6-mac-arm64-final-2026-11-26`; Criterion data is in ignored `nib-rs/target/`.
- Apple Silicon arm64 / Rust 1.99.1 host medians: geometry 1.7094 µs,
  `ap/ink_128_points` 46.751 µs, `delta/apply_50` 143.59 µs, and
  `delta/compact_50` 171.71 µs. These are host microbenchmarks, not Android
  performance-budget evidence.
- The existing `ScrollBenchmark` passed on Android 14 API 34 arm64-v8a with
  `frameOverrunMs` P50 27.2 ms / P90 51.8 ms. This is diagnostic only and is
  not the required x86_64 emulator run.
- The installed `nib_api34` AVD is arm64-v8a; there is no API 34 x86_64 system
  image installed and no physical mid-range arm64 device is connected.
- First visible 10 MB PDF load, tile P50/P95, ink touch-to-draw, flush with
  50 ops on 20 MB, and the >15% Macrobenchmark regression gate remain
  unmeasured or unimplemented. Existing scroll coverage is already present;
  avoid duplicating it.
- Incremental flush reparses the output PDF (O(file)); measure before any
  optimization. B-T is out of scope and explicitly rejected. M3 annotation
  appearance text extraction by a suitable CLI reader remains unverified.

## Objective

Continue the M6 performance slice by obtaining target-equivalent Android
measurements and establishing a reproducible regression gate. Reuse the Rust
Criterion and Macrobenchmark infrastructure; add only the smallest missing
coverage needed to measure ROADMAP §7 budgets. Do not make speculative
performance changes. If a required device/image is unavailable, state the
blocker, run only clearly labeled supplemental measurements, and leave the
budget marked unmet.

## Definition of done

1. Verify device ABI/API before measuring. Record API 34 x86_64 emulator
   results and physical mid-range arm64 results separately; never treat
   arm64-emulator or host Criterion numbers as satisfying either target.
2. Measure the first visible page from `from_bytes` (10 MB text PDF), tile
   P50/P95, scroll `frameOverrunMs` P90, ink touch-to-draw, and 50-op `flush()`
   on a 20 MB PDF against ROADMAP §7 budgets.
3. Add Macrobenchmark coverage only for a missing metric that can be run and
   validated. Establish and validate the >15% regression gate; do not duplicate
   existing scroll coverage.
4. Compare the Rust benches against the named baseline on the same host and
   save a new labeled baseline if fixtures or benchmark behavior changes.
5. Make no optimization unless a measurement demonstrates the need; preserve
   existing behavior and add a focused regression test for any fix.
6. Run targeted benchmark/build/test/lint commands and Android validation for
   Android benchmark/source changes. Update M6 status/details and the cycle
   log in ROADMAP, then rewrite this prompt for the next remaining M6 slice.
   Do not mark M6 complete until all fuzz, coverage, performance, API, and
   release criteria are satisfied.
7. Leave all changes uncommitted. Report final `git status` and exact
   Conventional Commit commands for reviewed changes.

## Verification commands

```sh
cd nib-rs
cargo +1.99.1 fmt --check
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test -p nib-core
cargo +1.99.1 bench -p nib-core --bench m6_performance -- \
  --noplot --baseline m6-mac-arm64-final-2026-11-26
cargo deny check
cd ..
./gradlew --no-daemon :macrobenchmark:connectedBenchmarkAndroidTest
```

For emulator setup and Android checks, use the toolchain and AVD instructions
in [AGENTS.md](../AGENTS.md). Do not claim x86_64 or physical-device results
unless the connected target's ABI and API have been verified.
