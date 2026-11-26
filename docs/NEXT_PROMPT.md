# Next development cycle - M6 scheduled fuzz CI

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree, then read M6 and the latest cycle
log in that document, [fuzz/README.md](../nib-rs/fuzz/README.md), the existing
GitHub Actions workflows, and [ARCHITECTURE.md](./ARCHITECTURE.md) sections
6, 7 and 10. Preserve all user changes. Do not stage, commit or push.

## Current state

- M6 is `[~]`. The malformed-PDF slice, short-ink-point AP fix, and AcroForm
  field-tree cycle/depth guards have deterministic regression tests.
- The standalone `nib-rs/fuzz` workspace has three arbitrary-byte targets:
  `import_delta_json`, `open_bytes`, and `ap_from_annotation`. Its dependency
  graph is audited separately from production.
- The maintainer approved an exact-version NCSA exception for fuzz-only
  `libfuzzer-sys =0.4.13`. It must remain isolated from production and shipped
  Android artifacts.
- On 2026-11-26, the three local 60-second smoke runs passed without crashes:
  delta import: 316,498 executions; PDF open: 85,428; AP generation: 316,670.
  The PDF smoke found a stack overflow from cyclic AcroForm `/Kids`. The
  original artifact now completes under `-runs=1`; deterministic tests cover
  both repeated object IDs and depth greater than 64.
- M5 is `[x]`: Acrobat Reader 26.003.22110 rendered RSA-PSS/P-256 descriptions
  without regeneration prompts. Reader reported unchanged documents and
  UNKNOWN identity trust for self-signed certificates. See ROADMAP §6.
- M3 annotation-appearance text extraction by a suitable CLI reader remains
  unverified. B-T is out of scope and explicitly rejected.

## Objective

Add a bounded, scheduled GitHub Actions workflow for the three fuzz targets,
with a manual dispatch option. The local fuzz build, isolated dependency audit,
and bounded smoke runs are now proven, so this is no longer blocked on license
review.

## Definition of done

1. Inspect and follow existing workflow conventions. Run the separate
   `cargo deny --manifest-path fuzz/Cargo.toml check` in CI; do not rely on the
   root workspace audit.
2. Pin `cargo-fuzz` to `0.13.2` and use nightly Rust only for instrumentation;
   production remains on Rust 1.99.1.
3. Give each target an explicit bounded runtime and the job an overall timeout.
   Run against ignored writable corpora plus checked-in read-only seeds. Keep
   crash artifacts available on failure without committing generated corpora.
4. Support both a daily schedule and `workflow_dispatch`. Do not add a
   24-hour job to routine CI; that qualification remains a separate M6 task.
5. Validate workflow syntax and run all affected local Rust/fuzz checks.
   Android validation is unnecessary unless Android or UniFFI code changes.
6. Update ROADMAP's cycle log and rewrite this prompt for the next remaining
   M6 slice. Do not mark M6 complete until performance, qualification,
   coverage, API, and release criteria are satisfied.

Useful local checks, from `nib-rs/`:

```sh
cargo deny --manifest-path fuzz/Cargo.toml check
cargo +1.99.1 fmt --check
cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check
cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cargo +1.99.1 test -p nib-core -p nib-ffi
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test
cargo deny check
```
