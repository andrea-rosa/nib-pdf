# Next development cycle — rerun the M6 fuzz qualification

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree and connected devices, then read the
M6 section and latest cycle log in ROADMAP, [ARCHITECTURE.md](./ARCHITECTURE.md)
sections 5, 10 and 11, and [nib-rs/fuzz/README.md](../nib-rs/fuzz/README.md).
Preserve all user changes. Do not stage, commit, or push.

## Current state

- M6 remains `[~]`. The manual [fuzz qualification
  workflow](../.github/workflows/fuzz-qualification.yml) chains five 5-hour
  segments per target and carries each corpus forward. The rerun on commit
  `13dceb3` is [run 45967181151](https://github.com/andrea-rosa/nib-pdf/actions/runs/45967181151);
  it failed in `segment-1 / ap_from_annotation`. The next run on commit
  `aa1844c` is [run 45969651692](https://github.com/andrea-rosa/nib-pdf/actions/runs/45969651692);
  it failed in `segment-1 / open_bytes` and cannot complete the chain.
- The `ap_from_annotation` failure was a standard Stamp appearance with an
  extremely tall rectangle. The font-fit loop repeatedly subtracted 0.5 from
  a huge `f64`, so the value did not change. The loop now uses a bounded
  direct fit for large reductions; a regression test covers the case.
- The `open_bytes` failure used a malformed PDF whose `startxref` pointed into
  an object body. Writer preflight now accepts classic xref tables or valid
  xref-stream objects only, and rejects control bytes in classic tables.
  Regression tests cover invalid pointers, invalid table bytes, and valid
  xref-stream fixtures.
- A second minimized reproducer from that run is a Squiggly annotation with a
  zero-height quad. The computed zig-zag step was zero, so its `while` loop
  never advanced. `markup::gen` now bounds zig-zag steps to 4096 and falls back
  to a straight line for degenerate/pathological geometry. A regression test
  uses the exact input.
- Current local validation passed: Rust formatting, Clippy for all `nib-core`
  targets, `cargo +1.99.1 test -p nib-core ap::tests` (53 tests), and the
  updated `writer::tests` (34 tests). The open_bytes crash was a malformed
  annotation color array with two numeric elements; `dict::color_of` indexed
  element 2 and panicked. It now ignores unsupported arrays shorter than three
  components, with a Writer-open regression test.
- The Squiggly and malformed-color fixes are uncommitted and must be committed
  and pushed before another qualification; do not dispatch against stale
  `main`. At review time, `ap_from_annotation` and `import_delta_json` were
  still running in run 45969651692; inspect their final outcomes after they
  finish.
- M6 still needs the physical Pixel 7a before/after profile, review of the
  API 34 x86_64 benchmark candidate, coverage thresholds, API freeze, and
  release criteria. No Android device was attached during the prior cycle.

## Objective

Verify the malformed-color fix with the available Rust checks, then rerun the
25-hour per-target GitHub qualification once the fix is present on the default
branch. Review every target/segment result and artifacts. Do not treat
successful smoke tests as qualification.

## Definition of done

1. Confirm the branch contains the malformed-color fix commit before dispatch. If it
   is not pushed to the default branch, do not start a stale run; record the
   blocker and complete all local checks instead.
2. Run Rust formatting, clippy, the targeted core tests, and the standalone
   fuzz-workspace dependency audit.
3. Dispatch
   `gh workflow run fuzz-qualification.yml -f seconds_per_segment=18000`.
   Track the run through all five segments and all three targets. Verify each
   segment consumed the prior corpus and inspect any failure artifacts.
4. Record the run URL, total fuzz time, and final conclusion in the M6 cycle
   log. Prior reruns found huge Stamp font-fit work, invalid `startxref`
   handling, a Squiggly non-advancing loop, and a two-component color panic;
   ensure the fixes remain covered. If another crash or timeout appears,
   reproduce it, add a focused regression, fix the root cause, and do not claim
   qualification passed.
5. Update M6 status, the cycle log, and this prompt for the next cycle. Leave
   changes uncommitted; report final `git status` and exact Conventional Commit
   commands.

## Verification commands

```sh
cd nib-rs
cargo +1.99.1 fmt --check
cargo +1.99.1 clippy -p nib-core --all-targets -- -D warnings
cargo +1.99.1 test -p nib-core ap::tests
cargo +1.99.1 test -p nib-core writer::tests
cargo deny --manifest-path fuzz/Cargo.toml check
cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cd ..
gh workflow run fuzz-qualification.yml -f seconds_per_segment=18000
```

The qualification uses 4,500 runner-minutes (25 hours per target). Confirm
repository visibility and workflow-run status before starting, and review
`fuzz-crash-*` artifacts before any rerun.
