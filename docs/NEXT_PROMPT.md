# Next development cycle - M6 performance baseline

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree, then read M6 and the latest cycle
log in that document, [ARCHITECTURE.md](./ARCHITECTURE.md) sections 6 and 10,
the existing macrobenchmark module, and relevant Rust benchmark/test setup.
Preserve all user changes. Do not stage, commit or push.

## Current state

- M6 remains `[~]`. Malformed-input fixes, initial fuzz smoke runs, and daily
  scheduled fuzz CI are in place; M6 performance, qualification, coverage, API,
  and release criteria are not complete.
- The scheduled workflow in `.github/workflows/fuzz.yml` runs all three targets
  for five minutes daily and on manual dispatch. It audits the isolated fuzz
  graph and uploads crash artifacts on failure. This does not replace the
  separately required 24-hour qualification.
- The remaining performance criteria are defined in ROADMAP §7: API 34 x86_64
  emulator and physical mid-range arm64 measurements, Rust `criterion` benches
  for geometry/AP generation/delta apply-compaction, and Macrobenchmark
  coverage with regression gating.
- The architecture notes that incremental flush reparses the output PDF and
  identifies this O(file) cost as a measurement target. Preserve existing
  behavior; do not make speculative performance changes without measurements.
- M3 annotation-appearance text extraction by a suitable CLI reader remains
  unverified. B-T is out of scope and explicitly rejected.

## Objective

Establish the next M6 performance baseline and implement the smallest complete
benchmark slice supported by the existing Rust and Android benchmark
infrastructure. Report measured results, platform/configuration, and gaps
against the ROADMAP budgets. Fix only regressions directly caused by changes
made in this cycle.

## Definition of done

1. Inspect existing benches and Macrobenchmark patterns before adding anything;
   avoid duplicate coverage and keep benchmark fixtures small and reproducible.
2. Measure the relevant ROADMAP budgets on the API 34 x86_64 emulator. Record
   the physical-device measurements as outstanding if that device is
   unavailable; do not claim the budgets are met from emulator-only evidence.
3. Add or extend `criterion` benches for geometry, AP generation, and delta
   apply/compaction where infrastructure permits; add/extend Macrobenchmark
   coverage only where it can be run and validated.
4. Run targeted benchmark/build/test commands and the relevant Rust/Android
   checks. Android validation is required for Android benchmark or source
   changes.
5. Update the M6 status/details and cycle log in ROADMAP, then rewrite this
   prompt for the next remaining M6 slice. Do not mark M6 complete until all
   qualification, coverage, API, and release criteria are satisfied.
6. Do not stage, commit, or push. Report the final worktree status and provide
   exact Conventional Commit commands for the reviewed changes.
