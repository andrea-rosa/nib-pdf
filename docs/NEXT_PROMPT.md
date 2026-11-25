# Next development cycle — M6 malformed-PDF hardening

Continue the cycle protocol in §0 of [`ROADMAP.md`](./ROADMAP.md). Read this
file first, inspect the current worktree, then review M6 in `ROADMAP.md` §7 and
the PDF open/import, writer, and annotation-appearance paths in
`ARCHITECTURE.md`. Preserve all existing user changes. Do not stage, commit,
or push.

## Objective

Start M6 with a focused malformed-PDF robustness slice: add regression coverage
for truncated cross-reference data, malformed `/Annots`, and broken foreign
`/AP` objects. Ensure these inputs produce the repository's documented error
or warning behavior without panicking, silently corrupting state, or
regenerating foreign appearances unexpectedly.

M3 is complete. Its `/ToUnicode` and CID mappings are validated structurally
after save/reopen, but no installed CLI reader extracts text from annotation
appearance streams; PDFium `textInRect` was confirmed to read page-content text
only. Do not claim that reader-specific extraction check passed. M5 remains
`[~]` because Acrobat Reader interoperability is blocked on this machine;
PDFium, qpdf, OpenSSL, PDFKit, or another reader is not a substitute. B-T
remains out of scope and explicitly rejected.

## Required work

1. Trace the existing PDF open/import paths in `nib-core`, `nib-ffi`, and
   `nib-render`, including `OpenOptions.lazy_structure`, existing-annotation
   parsing, foreign `/AP` preservation, and panic/error boundaries.
2. Add small synthetic or minimal fixtures for truncated xref data, malformed
   `/Annots`, and invalid `/AP` streams/dictionaries. Prefer generated
   fixtures over large third-party PDFs.
3. Assert the exact public outcome for each case: typed open/flush error,
   explicit warning, or successful preservation where safe. Verify a failed
   open/flush leaves the writer/document usable and does not publish partial
   bytes or model changes.
4. Add focused Rust regression tests at the narrowest appropriate layer.
   Include `qpdf --check` for any intentionally valid output; do not make
   invalid-input tests depend on qpdf accepting malformed input.
5. Keep existing valid-PDF behavior unchanged, especially incremental-save
   prefix preservation, imported-annotation IDs, foreign `/AP` preservation,
   and signed-document restrictions. Do not introduce a dependency unless
   required; if one is needed, verify lockfile reuse, publication age, MSRV,
   licence, advisories, and Android target support.
6. Run focused tests first, then the applicable M6/Rust checks from
   `ROADMAP.md` §0 and repository instructions. Run Android validation only
   if the changed surface requires it. Record exact commands, results,
   limitations, and any newly discovered risks in the newest-first cycle log.
7. At cycle end, update M6 status/DoD only for completed work and rewrite this
   file for the next actual task. Keep M5 marked in progress until an actual
   Acrobat Reader interoperability check is completed.

## Definition of Done

- Truncated xref, malformed `/Annots`, and broken foreign `/AP` cases have
  focused regression coverage with explicit, documented outcomes.
- No panic or success-shaped fallback occurs for invalid input; failed
  operations do not publish partial document state.
- Existing valid-PDF import, foreign-appearance, incremental-save, and
  signature protections pass their focused regression tests.
- `cargo fmt`, Clippy, targeted tests, full Rust tests, and `cargo deny check`
  pass (or any unrelated baseline blocker is clearly documented).
- `ROADMAP.md` records exact commands/results and M5/M3 limitations truthfully;
  no files are staged or committed.

## Verification commands

Use the exact toolchain in the repository instructions. Run the focused tests
before the full suite:

```sh
cd nib-rs
cargo +1.99.1 fmt --check
cargo +1.99.1 test -p nib-core writer::tests
cargo +1.99.1 test -p nib-ffi
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test
cargo deny check
cd ..
```

If Android code or the UniFFI surface changes, also run the Android unit,
release, alignment, and connected checks listed in `ROADMAP.md` §0. If an
emulator or an external reader is unavailable, record that limitation and do
not claim its check passed.
