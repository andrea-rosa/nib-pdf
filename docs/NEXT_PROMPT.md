# Next cycle: signature appearances and honest sample status

Nib PDF is an Apache-2.0 Android PDF library with a Rust core, UniFFI
bindings, and a Compose UI. M0-M4 are implemented; M3 embedded FreeText fonts
remain deferred. M5 now includes:

- Visual Ink/Stamp marks bind to writable, unambiguous empty signature
  widgets; visual marks do not fill `/V`.
- `nib-sign` provides internal PDF/CMS signing and verification, strict
  private staging/adoption, trust separation, and revision/DocMDP/field-lock
  analysis. Signature delta operations remain unsupported.
- UniFFI exposes tokenized prepare/complete/abort. It flushes first, keeps the
  staged revision private, and checks both writer revision and engine mutation
  token before adoption. The Kotlin `Signer` callback runs outside Rust locks.
- The Android facade provides `Signer`, `KeyStoreSigner` (RSA-PSS and ECDSA
  P-256), certificate-chain handling, and `NibDocument.sign`.
- Signatures are currently invisible. Signature appearance composition,
  an honest sample status indicator, refusal of signed-document flattening
  before destination truncation, double-signature coverage, and manual reader
  interoperability remain.

## Read first

1. [AGENTS.md](../AGENTS.md) for toolchain, verification commands (OpenSSL ≥ 3
   and qpdf are required), and the no-commit rule.
2. [ROADMAP.md](./ROADMAP.md) §0, §6 and the latest §8 cycle log entry.
3. [ARCHITECTURE.md](./ARCHITECTURE.md) §4, §7-8 and
   [JSON_SCHEMA.md](./JSON_SCHEMA.md) §6.1-6.2.
4. Inspect `nib-core` annotation `/AP` resource preservation, signature
   staging/adoption, `nib-ffi` locks and field-state sync, the Kotlin signing
   facade, sample Save/Save As paths, and current Rust/Android signature tests.

## Goal and scope

Implement visible signature appearance composition and safe sample-facing
status for completed invisible/visible signatures. Keep the existing
transactional signing guarantees. Do not expose `verify_signatures` or claim
certificate trust from a signature's appearance or occupied field state.

- Compose a signature widget `/AP` (`/FRM`, `/n0`, `/n2`) before the signed
  ByteRange is prepared. Support a description and a graphic bound to an
  existing Ink/Stamp mark, including copying the imported `/AP` resources
  needed by image marks. Do not modify any bytes covered by the prepared
  signature after the ByteRange has been computed.
- Add the minimal public data needed for signature appearance options while
  keeping unsupported delta signature ops rejected.
- Add a sample status badge whose language distinguishes “signature present”
  from “integrity verified” and “trusted”; when verification is not available
  to the sample, it must explicitly say that integrity/trust were not
  evaluated.
- Validate signed-document flattening before opening/truncating a Save As
  destination; preserve normal unsigned Save/Save As behavior.
- Add/complete the two-signature revision test: both signatures verify, the
  first does not cover the whole document, the second does.
- Keep B-T timestamps, general signature delta operations, and any public
  trust claim out of scope.

## Acceptance and verification

- Rust appearance snapshots cover description-only and graphic+description,
  including a foreign image `/AP`; pdfium renders a signed widget and the
  written PDF passes qpdf.
- Rust double-signature coverage verifies both revisions and exact
  whole-document coverage flags.
- Android tests exercise the public signing facade and truthful status copy;
  Save As flatten refusal leaves a pre-existing destination untouched.
- The current `KeyStoreSignTest` continues to pass for RSA-PSS and ECDSA
  P-256, including invalid signatures, host failure, and concurrent edits.
- Run the per-cycle baseline:

```sh
(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
```

## Known constraints and handoff

- Preserve staged signature bytes and the writer-before-engine lock order.
  Host signing callbacks must remain outside both locks.
- Keep `nib_sign::verify_signatures` internal Rust-only. Sample text may report
  signature presence, but must not present presence as valid integrity or
  trusted identity.
- Existing signature appearances may be foreign Form XObjects. Copy required
  resources instead of attempting to regenerate an image from missing source
  bytes.
- Do not silently flatten or overwrite signed input; validate before opening
  any truncating output stream.
- OpenSSL ≥ 3 (LibreSSL is refused) and qpdf are required by the Rust signing
  tests. Emulator AVD: `nib_api34`.
- At cycle end update M5 remaining work, the cycle log, architecture/schema as
  applicable, and this prompt. **Do not stage, commit, or push.** Report
  `git status` and exact selective `git add` and Conventional Commit commands,
  including `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
  Preserve unrelated and earlier uncommitted changes.
