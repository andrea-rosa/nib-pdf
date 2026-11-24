# Next cycle: signature verification and revision classification

Nib PDF is an Apache-2.0 Android PDF library with a Rust core, UniFFI
bindings, and a Compose UI. M0-M4 are implemented; M3 embedded FreeText fonts
remain deferred. M5 is in progress:

- Visual Ink/Stamp signatures bind to writable, unambiguous empty signature
  widgets. They are **not** cryptographic and do not fill `/V`.
- Internal Rust PDF/CMS signing foundations exist (2026-11-24). `nib-sign`
  provides `prepare`/`complete`/`abort` for invisible `ETSI.CAdES.detached` and
  `adbe.pkcs7.detached` B-B signatures. `nib-core::writer::sign` provides
  private staging and adoption.
- Signature ops are still rejected at `apply`/`import_delta`
  (`UNSUPPORTED_OP`). There is no UniFFI signing, KeyStore, verification or
  appearance support yet.

## Read first

1. [AGENTS.md](../AGENTS.md) for toolchain, commands (including the
   required openssl ≥ 3 and qpdf oracles), and the no-commit rule.
2. [ROADMAP.md](./ROADMAP.md) §0, §6 (M5 remaining list and pinned
   dependency table), and the latest §8 cycle log entry.
3. [ARCHITECTURE.md](./ARCHITECTURE.md) §4, §6-8 and
   [JSON_SCHEMA.md](./JSON_SCHEMA.md) §6.1-6.2.
4. Inspect `nib-sign` (`lib.rs`, `cms.rs`, `placeholder.rs`, `algorithm.rs`,
   `tests.rs`), `nib-core::writer::sign`, the signature model and
   signature-field parser, and the delta engine's `signed_barrier`.

## Goal and scope

Implement and independently test **Rust signature verification and revision
classification** for signatures in opened documents. Do not add UniFFI or
KeyStore signing, signature appearances, or sample UI in this cycle.

- **Independent results.** For every signed field, report two separate
  results:
  - Integrity: the ByteRange covers the whole file except exactly the
    `/Contents` token; the digest matches; the signed attributes and the
    signature are valid; the signing certificate matches ESS
    signing-certificate-v2.
  - Trust: trusted-path validation against **host-supplied** anchors only.
    With no anchors the result is "not evaluated", never "untrusted" or
    "valid". `/M` is an untrusted claimed time. Revocation and LTV are out of
    scope and must be reported as not checked.
- **Revisions.** Map each signature to its revision. Classify later
  incremental changes as permitted or disallowed under DocMDP P=1/2/3 and
  field locks (`/Lock`, FieldMDP). Detect signatures that do not cover the
  whole file at their revision, and shadowing or overlapping ByteRanges.
- **Reopen.** Reconstruct the engine's `signed_barrier` and locked fields
  from verified state when a document is reopened, so edits that would
  invalidate a signature are refused, or flagged according to the documented
  policy.
- **Hostile input.** Reject or flag malformed CMS, unknown algorithms,
  multiple SignerInfos, missing attributes and oversize structures without
  panicking. Add `proptest` or fuzz-style coverage for the parsers.
- **Dependencies.** Reuse the pinned `cms`/`x509-cert` stack. Vet any new
  crate as before (age at least seven days, not yanked, MSRV 1.88, licences,
  advisories), pin it, and pass `cargo deny check` with no new exceptions.

## Acceptance and verification

- Verify signatures produced by `nib-sign` and cross-check against
  `openssl cms -verify` (with `-noverify` for integrity; with `-CAfile` and
  test-generated anchors for trust).
- Include tampered bytes, appended disallowed revisions, permitted form-fill
  revisions, a wrong or missing anchor, and an expired or not-yet-valid leaf.
  Generate keys in tests only and never commit private keys.
- Run the per-cycle baseline:

```sh
(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
```

## Future work and handoff

Later M5 cycles:

- Transactional UniFFI signing and Android KeyStore integration:
  - Host callbacks run outside writer/engine locks.
  - An engine mutation token is checked together with the writer revision
    before atomic adoption.
  - Engine-side "signature last in its revision".
  - Signature ops accepted at `apply`/`import_delta`.
- Visible, layered signature appearances. A flushed and reopened visual image
  mark keeps its foreign `/AP`, but its importer does not rebuild the
  original image bytes, so appearance composition must copy its resources.
- Truthful sample status.
- In the sample, validate before opening a destination with truncation when
  refusing to flatten a signed document.

Do not start M6. At this cycle's end update the exact M5 remaining list,
cycle log, architecture/schema as applicable, and this next prompt. **Do not
stage, commit, or push.** Report `git status` and provide exact reviewed
`git add` and Conventional `git commit` commands including
`Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
Preserve unrelated and earlier uncommitted changes; use selective staging if
cycles overlap.
