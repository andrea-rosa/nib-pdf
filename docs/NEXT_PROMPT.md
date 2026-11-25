# Next development cycle — M3 embedded FreeText fonts

Continue the cycle protocol in §0 of [`ROADMAP.md`](./ROADMAP.md). Read this
file first, then inspect the current worktree and the M3 FreeText font plan in
`ROADMAP.md` §4, plus the relevant FreeText and `FontProvider` sections of
`ARCHITECTURE.md` and `JSON_SCHEMA.md`. Preserve all existing user changes.
Do not stage, commit, or push.

## Objective

Implement host-provided TrueType subsetting and embedding for non-WinAnsi
FreeText appearances. This is the one remaining M3 implementation item. M5
remains in progress because the Acrobat Reader interoperability check is
blocked on a machine without Acrobat; do not treat PDFium, qpdf, OpenSSL,
PDFKit, or another reader as a substitute for that check. B-T remains out of
scope and explicitly rejected.

## Required work

1. Trace the existing `FontProvider`, FreeText layout/appearance generation,
   PDF font-resource writing, and `FlushReport` paths before editing.
2. Verify the proposed exact dependency versions `subsetter = 0.2.6` and
   `skrifa = 0.42.1` against the current lockfile, publication age, MSRV,
   licences, advisories, and Android target support. Preserve existing locked
   versions where possible; do not introduce a dependency less than seven
   days old. Update manifests and `Cargo.lock` only as needed.
3. Use `skrifa` for TrueType character-to-glyph mapping and actual glyph
   metrics, and `subsetter` for the embedded subset. Build the Type0
   `/Identity-H` font, CIDFont descendant, descriptor, `/FontFile2`, widths,
   and `/ToUnicode` CMap with lopdf. Keep encoded character codes, subset
   glyph IDs, `/CIDToGIDMap`, and Unicode extraction consistent, including
   repeated glyphs, distinct characters mapping to the same glyph, and
   supplementary-plane characters when supported. Use real font advances
   for FreeText layout and wrapping.
4. Preserve the existing Base-14/WinAnsi path and foreign-appearance behavior.
   Handle unavailable, invalid, unsupported, or restricted fonts; missing
   glyphs; and subset failures explicitly through established errors or
   `FlushReport` warnings. Never silently claim a font was embedded.
5. Add a small redistributable test font with documented provenance and
   licence. Cover mixed WinAnsi/Cyrillic/CJK text, supplementary-plane
   mapping when the fixture supports it, repeated glyphs, wrapping, missing
   glyphs, provider failures, and unsupported/invalid font input. Assert PDF
   subset/resource structure and Unicode mapping after save/reopen; use
   `qpdf --check`, a reader that extracts text from appearance streams, and
   PDFium rendering against a discriminating reference. Keep fixtures under
   200 KB and do not alter existing goldens unless behavior intentionally
   changes.
6. Exercise the public `FontProvider` path on Android. Update architecture/API
   documentation for the implemented behavior and limitations. Run the
   relevant focused tests, then the M3 Rust and Android checks from `ROADMAP.md`
   §0 and §4. Record exact commands and results in the newest-first cycle log.
7. At cycle end, update M3 status/DoD and rewrite this file for the next actual
   development task. Keep M5 marked in progress with the Acrobat blocker until
   a real Acrobat check is completed; do not claim M5 complete.

## Definition of Done

- Host-provided TrueType fonts are subset and embedded for non-WinAnsi FreeText;
  Cyrillic and CJK render and map back to Unicode after save/reopen.
- PDF font structures, subsetting, Unicode mapping, glyph metrics/wrapping,
  failure cases, and missing-glyph behavior have regression coverage.
- Existing WinAnsi/Base-14, foreign `/AP`, FreeText, and incremental-save
  behavior remains intact.
- Public Android `FontProvider` integration and relevant Rust/Android
  validation pass; documentation and cycle log describe actual support and
  limitations.

## Verification commands

Run focused M3 tests first (including the new font tests, existing FreeText
appearance/writer tests, incremental-save matrix, and PDFium appearance
parity), then the applicable baseline:

```sh
cd nib-rs
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
cd ..
./gradlew --no-daemon ktlintCheck :nib-android:testDebugUnitTest \
  :nib-android:assembleRelease :sample-app:assembleRelease
nib-rs/tools/check-elf-alignment.sh \
  nib-android/build/outputs/aar/nib-android-release.aar \
  sample-app/build/outputs/apk/release/sample-app-release.apk
./gradlew --no-daemon --no-parallel \
  :nib-android:connectedDebugAndroidTest :sample-app:connectedDebugAndroidTest
```

Use the exact toolchain in the repository instructions. If an Android emulator
or an external text-extraction reader is unavailable, record that limitation
and do not claim its check passed.
