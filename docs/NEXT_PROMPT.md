# Next cycle: transactional UniFFI signing and Android KeyStore

Nib PDF is an Apache-2.0 Android PDF library with a Rust core, UniFFI
bindings, and a Compose UI. M0-M4 are implemented; M3 embedded FreeText fonts
remain deferred. M5 is in progress:

- Visual Ink/Stamp signatures bind to writable, unambiguous empty signature
  widgets. They are **not** cryptographic and do not fill `/V`.
- Internal Rust PDF/CMS signing foundations are implemented in `nib-sign`
  and `nib-core::writer::sign`: private staging, detached CMS creation and
  completion, strict adoption checks, and RSA-PSS/PKCS#1 plus P-256/P-384.
- Rust-only `nib_sign::verify_signatures(bytes, trust_anchors_der)` now
  reports integrity separately from host-anchor trust, classifies later
  revisions/DocMDP/field locks, and restores edit barriers on reopen.
- Signature delta operations are rejected at `apply`/`import_delta`
  (`UNSUPPORTED_OP`). There is no UniFFI signing API, Android `Signer`,
  KeyStore integration, signature appearance composition, or sample status
  yet.

## Read first

1. [AGENTS.md](../AGENTS.md) for toolchain, commands (including required
   OpenSSL ≥ 3 and qpdf oracles), and the no-commit rule.
2. [ROADMAP.md](./ROADMAP.md) §0, §6 (M5 remaining list and dependencies),
   and the latest §8 cycle log entry.
3. [ARCHITECTURE.md](./ARCHITECTURE.md) §4, §6-8 and
   [JSON_SCHEMA.md](./JSON_SCHEMA.md) §6.1-6.2.
4. Inspect the existing Rust signing/verifying APIs, `nib-ffi` document
   locking and writer lifecycle, Android facade patterns, and current
   instrumentation tests.

## Goal and scope

Implement transactional signature completion through UniFFI and add Android
KeyStore signing. Keep signatures invisible in this cycle; do not implement
signature appearances, sample UI/status, B-T timestamps, or accept general
signature delta operations.

- Provide a UniFFI-facing prepare/complete/abort flow around the existing
  internal `nib-sign` and writer staging APIs.
- Host signer callbacks must execute with neither the writer nor engine lock
  held. Before adoption, validate both the writer revision and an engine
  mutation token so any intervening edit makes the prepared signature stale.
- Preserve the established writer-before-engine lock order whenever both are
  needed. Do not hold either lock while invoking Kotlin/host code.
- Add an Android `Signer` implementation backed by Android KeyStore using
  supported RSA-PSS or ECDSA P-256 signing, with certificate/chain handling
  consistent with the Rust signing request.
- Keep cancellation, host callback failure, malformed signatures, concurrent
  edits, stale tokens, and adoption failures explicit and rollback-safe.
- Keep `verify_signatures` internal Rust-only; use it to independently verify
  completed documents in tests, with trust anchors supplied only by the host.

## Acceptance and verification

- Rust tests prove prepare → callback → complete is atomic, host callbacks
  run outside locks, and a concurrent engine edit or writer revision change
  rejects adoption without publishing staged bytes.
- Rust tests cover abort, callback error, invalid returned signature/CMS,
  retry where supported, and successful completion verified by
  `nib_sign::verify_signatures`; cross-check CMS with OpenSSL and written PDFs
  with qpdf.
- Android instrumentation tests generate/use an Android KeyStore key, sign a
  fixture through the public Kotlin API, and verify signature integrity.
- Run the per-cycle baseline:

```sh
(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
```

## Known constraints and handoff

- The internal writer staging/adoption API already checks a staging token,
  writer revision, unchanged prefix, and exact placeholder edits; extend it
  rather than duplicating it.
- Internal `nib-sign::prepare` / `PreparedSignature::complete` already
  perform strict CMS/digest/attribute checks. Never expose private keys to
  Rust; Kotlin returns only the signature and certificate chain (or complete
  CMS where supported).
- Reopened documents verify without anchors and restore policy only from
  integrity-valid signatures. Do not treat this as trust validation.
- `Signature` delta ops remain unsupported; signing must not silently pass
  through `apply`, `import_delta`, or flush.
- OpenSSL ≥ 3 (LibreSSL is refused) and qpdf are required by the Rust signing
  tests. Emulator AVD: `nib_api34`.
- At the cycle end update M5 remaining work, the cycle log, architecture/schema
  as applicable, and this prompt. **Do not stage, commit, or push.** Report
  `git status` and exact selective `git add` and Conventional Commit commands,
  including `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
  Preserve unrelated and earlier uncommitted changes.
