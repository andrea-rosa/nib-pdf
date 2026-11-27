# Next development cycle — rerun the M6 fuzz qualification

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree and connected devices, then read the
M6 section and latest cycle log in ROADMAP, [ARCHITECTURE.md](./ARCHITECTURE.md)
sections 5, 10 and 11, and [nib-rs/fuzz/README.md](../nib-rs/fuzz/README.md).
Preserve all user changes. Do not stage, commit, or push.

## Current state

- M6 remains `[~]`. The manual [fuzz qualification
  workflow](../.github/workflows/fuzz-qualification.yml) chains five 5-hour
  segments per target and carries each corpus forward. Its first attempt,
  run `45833963613` on 2026-11-26, failed in segment 1.
- The `ap_from_annotation` failure was a standard Stamp appearance with an
  extremely tall rectangle. The font-fit loop repeatedly subtracted 0.5 from
  a huge `f64`, so the value did not change. The loop now uses a bounded
  direct fit for large reductions; a regression test covers the case.
- The `open_bytes` failure used a malformed PDF whose `startxref` pointed into
  an object body. Writer preflight now accepts classic xref tables or valid
  xref-stream objects only, and rejects control bytes in classic tables.
  Regression tests cover invalid pointers, invalid table bytes, and valid
  xref-stream fixtures.
- Focused Rust tests passed for `ap::tests` (52) and `writer::tests` (33);
  formatting passed. The full qualification has not been rerun. These fixes
  must be committed and pushed by the maintainer before GitHub Actions can test
  them; do not dispatch the workflow against stale `main`.
- M6 still needs the physical Pixel 7a before/after profile, review of the
  API 34 x86_64 benchmark candidate, coverage thresholds, API freeze, and
  release criteria. No Android device was attached during the prior cycle.

## Objective

Verify the fuzz fixes with the available Rust checks, then rerun the 25-hour
per-target GitHub qualification once the fixes are present on the default
branch. Review every target/segment result and artifacts. Do not treat
successful smoke tests as qualification.

## Definition of done

1. Confirm the branch contains the fix commit before dispatch. If the fixes are
   not pushed to the default branch, do not start a stale run; record the
   blocker and complete all local checks instead.
2. Run Rust formatting, clippy, the targeted core tests, and the standalone
   fuzz-workspace dependency audit.
3. Dispatch
   `gh workflow run fuzz-qualification.yml -f seconds_per_segment=18000`.
   Track the run through all five segments and all three targets. Verify each
   segment consumed the prior corpus and inspect any failure artifacts.
4. Record the run URL, total fuzz time, and final conclusion in the M6 cycle
   log. If a new crash or timeout appears, reproduce it, add a focused
   regression, fix the root cause, and do not claim qualification passed.
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
