# Next development cycle — M5 reader interoperability

Continue the development cycle protocol in §0 of [`ROADMAP.md`](./ROADMAP.md).
Read this file first, then inspect the current worktree, the M5 section and the
latest M5 cycle-log entry in `ROADMAP.md`, and the signature-related sections
of `ARCHITECTURE.md` and `JSON_SCHEMA.md`. Preserve any existing user changes.
Do not stage, commit, or push.

## Objective

Complete the remaining M5 manual reader-interoperability check for signed PDFs.
The implementation already supports visible signature widget appearances
(`/FRM`, `/n0`, `/n2`), description-only and graphic-plus-description
options through the public Kotlin facade, truthful sample status, and safe
Save As flatten refusal. Rust/OpenSSL/qpdf and Android/PDFium automated
coverage passed in the previous cycle. Do not redo implementation work unless
manual inspection reveals a concrete interoperability defect.

The separate outstanding M3 FreeText font work is planned in `ROADMAP.md` §4
using `subsetter` and `skrifa`; it is not part of this interoperability check.

## Required work

1. Check whether Adobe Acrobat Reader is available. If so, create or locate
   reproducible signed fixtures for RSA-PSS and ECDSA P-256, including a
   visible appearance where supported. The Rust signing tests can preserve
   generated PDFs using:

   ```sh
   cd nib-rs
   NIB_KEEP_PDFS=/tmp/nib-m5-interop cargo test -p nib-sign
   ```

   Use the relevant test outputs, and inspect Android KeyStore fixtures if
   needed. Keep temporary PDFs outside the repository.
2. Open each relevant fixture in Acrobat Reader. Record whether the document
   opens without repair warnings, the visible appearance renders as expected,
   the signature integrity is reported valid, and the signer identity/trust
   state is represented accurately. If Acrobat trust configuration is needed,
   distinguish the manually trusted case from default trust. Do not claim trust
   validation beyond what was actually checked.
3. If Acrobat Reader is unavailable, or manual verification cannot be
   completed, record that limitation and leave M5 in progress; do not infer
   Acrobat compatibility from automated tests. Where available, use another
   reader (for example macOS Preview or Firefox/pdf.js) as supplemental
   evidence, not as a substitute for the Acrobat check.
4. Fix any concrete interoperability issue with a minimal change, add or update
   a regression test, and run the relevant Rust/Android validation from
   `ROADMAP.md` §0. Do not alter committed fixtures or goldens unless the
   behavior change intentionally requires it.
5. Update the M5 Definition of Done and newest-first cycle log in
   `ROADMAP.md` with the exact manual results, or the explicit blocker. Keep
   B-T out of scope and explicitly rejected. Update `ARCHITECTURE.md`,
   `JSON_SCHEMA.md`, or other directly related documentation only if behavior
   changes. Rewrite this file for the next actual development task at cycle
   end.

## Constraints

- Never claim M5 complete unless the manual Acrobat Reader check is recorded
  and every other M5 DoD item is satisfied.
- Preserve the separation between signature integrity and signer trust.
- The sample status must not imply that occupied signature fields have been
  verified or trusted.
- Never stage, commit, or push. At the end, provide selective `git add` and
  Conventional Commit commands for the reviewed changes, including the
  required Copilot co-author trailer in the suggested commit message.
