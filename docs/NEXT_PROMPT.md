# Next cycle: internal PDF/CMS signing foundations

Nib PDF is an Apache-2.0 Android PDF library with a Rust core, UniFFI
bindings, and a Compose UI. M0-M4 are implemented; M3 embedded FreeText fonts
remain deferred. M5 is in progress: visual Ink/Stamp signatures can be
bound to writable, unambiguous empty signature widgets, and the sample offers
draw, type, image, and session-only reuse. These marks are **not**
cryptographic signatures and do not fill `/V`.

## Read first

1. [AGENTS.md](../AGENTS.md) for toolchain, commands, and the no-commit rule.
2. [ROADMAP.md](./ROADMAP.md) §0, §6, and the latest §8 cycle log.
3. [ARCHITECTURE.md](./ARCHITECTURE.md) §4, §6-8 and
   [JSON_SCHEMA.md](./JSON_SCHEMA.md) §6.1-6.2.
4. Inspect the existing `nib-sign`, `nib-core` writer, signature model,
   signature-field parser, delta engine, and their tests before designing the
   narrow integration boundary.

## Goal and scope

Implement and independently test **internal Rust PDF/CMS signing
foundations**. Do not expose production signing UI or claim that digital
signature verification, trust, Android KeyStore, or signature appearances are
complete; those are subsequent M5 cycles.

- Check the publication age (at least seven full days), yanked status, Rust
  1.88 MSRV, licenses, and advisories of all added direct and transitive crate
  versions. Prefer stable crates; a tested eligible release candidate is
  allowed. Pin versions, update the M5 dependency table, and pass
  `cargo deny check` without new exceptions. `cms` 0.2.3's builder pulls RSA
  0.9 affected by RUSTSEC-2023-0071; investigate a compatible typed-CMS path
  or a suitable newer release rather than ignoring the advisory.
- Support an existing empty signature field and creation of a new field in an
  incremental revision. Preserve all earlier bytes, reserve fixed-width
  `/ByteRange` and `/Contents` placeholders, locate the exact signature
  unambiguously, and exclude the complete `<...>` token from the ByteRange.
  The default is 16,384 hex characters (8,192 DER bytes), excluding the
  delimiters. Reject malformed or ambiguous targets, malformed ranges, odd
  sizes, offset overflow, and oversize CMS explicitly; document the unit of
  `PlaceholderTooSmall { required }`. Never publish partial prepared bytes as
  a completed signature.
- Build detached CMS/CAdES with `content-type`, the PDF ByteRange
  `message-digest`, and ESS `signing-certificate-v2`; omit CAdES
  `signingTime`. Obtain the leaf certificate or expected fingerprint before
  preparing attributes and confirm its identity on completion. The signer
  signs the canonical DER **signed-attributes bytes**, not the PDF digest;
  support an explicitly prehashed algorithm only when its parameters say so.
  A supplied complete CMS must be checked for digest, attributes, signature,
  and certificate before it can be accepted.
- Support the documented `ETSI.CAdES.detached` and
  `adbe.pkcs7.detached` profiles and supported digest/signature combinations;
  reject unsupported combinations and B-T explicitly. Wire invisible test
  signatures and ensure signature operations are not silently ignored.
  Preserve a route for signature operations to be last in their revision;
  reject rewrite/flatten of signed or prepared state.
- Keep staged bytes private and maintain the writer-before-engine lock order.
  Production private keys never enter Rust. Later host callbacks must run
  outside writer/engine locks; completion must eventually detect stale
  revisions and engine mutations before atomic adoption. PDFium remains
  read-only on its worker, lopdf remains the writer, and no Tokio is added.

## Acceptance and verification

- Generate RSA-2048/PSS and P-256 test keys and certificates **in tests**;
  never commit private-key fixtures. Verify detached signatures internally.
- Test exact offsets, unchanged earlier revisions, collisions, malformed
  ranges, incorrect signatures and certificates, unsupported algorithms,
  placeholder overflow, and rollback without state corruption.
- Extract exact DER CMS and ByteRange bytes and run
  `openssl cms -verify -binary -inform DER -noverify` as a required CI oracle;
  check generated PDFs with qpdf. Do not silently skip unavailable tools.
- Run `cargo test -p nib-sign`, relevant `nib-core` writer tests, and the
  per-cycle baseline:

```sh
(cd nib-rs && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo deny check)
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh nib-android/build/outputs/aar/nib-android-release.aar sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon --no-parallel :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
```

## Future work and handoff

Later cycles must implement independent integrity and certificate trust
results, trusted-path validation against host anchors, revision and DocMDP
classification, lock reconstruction on reopen, transactional UniFFI and
Android KeyStore integration, layered signature appearances, and truthful
sample status. With no anchors, trust is not evaluated; `/M` is an untrusted
claimed time. Revocation and LTV are out of scope. A flushed and reopened
visual image mark retains its foreign `/AP` but its importer does not rebuild
the original image bytes: appearance composition must copy its resources.
Before refusing signed-document flattening in the sample, validate before
opening a destination with truncation.

Do not start M6. At this cycle's end update the exact M5 remaining list,
cycle log, architecture/schema as applicable, and this next prompt. **Do not
stage, commit, or push.** Report `git status` and provide exact reviewed
`git add` and Conventional `git commit` commands including
`Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
Preserve unrelated and earlier uncommitted changes; use selective staging if
cycles overlap.
