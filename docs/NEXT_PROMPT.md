# Next development cycle — M6 fuzz-target foundation

Continue the mandatory cycle protocol in §0 of [`ROADMAP.md`](./ROADMAP.md).
Read this file first, inspect the worktree, then review M6 in `ROADMAP.md` §7,
the latest cycle-log entry in §8, and the parser/import/error boundaries in
`ARCHITECTURE.md` §§6, 7, and 10. Preserve all user changes. Do not stage,
commit, or push.

## Context carried forward

- M6 is `[~]`. The 2026-11-26 malformed-PDF slice rejects truncated classic
  xref sections and malformed `/Annots`, preserves malformed foreign `/AP`
  values without regeneration, and covers eager/lazy FFI writer-failure
  behavior.
- `Writer::open` has typed parse and malformed-annotation errors. The FFI
  retains PDFium-readable documents in read-only mode on eager writer import
  failure; lazy import failures surface when a writer-dependent operation is
  attempted.
- Existing valid-PDF behavior (incremental prefix preservation, imported
  annotation IDs, foreign `/AP` preservation, and signed-document restrictions)
  has focused regression coverage.
- M5 remains `[~]` until actual Acrobat Reader interoperability is checked.
  PDFium, qpdf, OpenSSL, PDFKit, and other readers are not substitutes.
- M3 `/ToUnicode` and CID mappings are validated structurally after
  save/reopen, but annotation-appearance text extraction by an installed CLI
  reader remains unverified. Do not claim it passed. B-T remains out of scope
  and explicitly rejected.

## Objective

Add the first M6 `cargo-fuzz` harnesses for the untrusted-input paths listed in
`ROADMAP.md` §7: delta JSON import, PDF open/import, and annotation appearance
generation. Seed them with small generated or existing repository fixtures;
do not add large or copyrighted PDFs. The focus is to find crashes/panics and
document any parser behavior that cannot be handled locally.

## Required work

1. Inspect current workspace crate APIs and test helpers before choosing target
   signatures. Keep fuzz-only code isolated from production dependencies and
   avoid a new runtime dependency.
2. Add harnesses for `import_delta_json`, `open_bytes` / `Writer::open`, and
   `ap_from_annotation` (or the closest public/narrow interfaces actually
   available). Each target must accept arbitrary bytes and never use
   `unwrap`/`expect` on fuzz-controlled data.
3. Seed each target from compact existing fixtures and generated malformed
   inputs. Keep each PDF under 200 KB.
4. Build all targets and run a bounded smoke fuzz session for each. Record the
   exact duration and command. Do not describe a smoke run as the required
   24-hour fuzz run.
5. Add any crash/panic findings as deterministic regression tests; preserve
   explicit errors and warnings rather than success-shaped fallbacks.
6. Run focused Rust tests first, then the M6/Rust checks from `ROADMAP.md` §0
   and repository instructions. Run Android validation only if Android code or
   the UniFFI surface changes.
7. Update `ROADMAP.md` with exact commands/results, remaining work, and any
   newly discovered risks. Keep M6 and M5 `[~]` until their full criteria are
   met, and rewrite this file for the next actual task.

## Definition of Done

- Fuzz targets compile and cover the three required trust boundaries.
- Each target gets a bounded run with no crash, or each discovered crash is
  fixed and covered by a deterministic regression.
- Existing Rust formatting, Clippy, targeted tests, full tests, and `cargo deny
  check` pass (or an unrelated baseline blocker is documented precisely).
- Cycle log truthfully distinguishes smoke fuzzing from the outstanding 24-hour
  run. No files are staged or committed.

## Verification commands

Use the pinned Rust toolchain from `nib-rs/rust-toolchain.toml`:

```sh
cd nib-rs
cargo +1.99.1 fmt --check
cargo +1.99.1 test -p nib-core
cargo +1.99.1 test -p nib-ffi
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test
cargo deny check
cargo fuzz build
# Then run each target for a bounded, recorded duration, e.g.:
cargo fuzz run import_delta_json -- -max_total_time=60
cargo fuzz run open_bytes -- -max_total_time=60
cargo fuzz run ap_from_annotation -- -max_total_time=60
cd ..
```

If `cargo-fuzz` is unavailable, report that as a toolchain limitation rather
than silently skipping target execution or claiming fuzz coverage.
