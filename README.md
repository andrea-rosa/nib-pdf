# nib-pdf

**Pre-alpha — not ready for use.**

![CI](https://github.com/andrea-rosa/nib-pdf/actions/workflows/ci.yml/badge.svg)

nib-pdf is an Android PDF editing library: a Rust core built on
`pdfium-render` (PDFium) and `lopdf`, exposed to Kotlin through UniFFI
bindings, with a Jetpack Compose UI layer.

## Quick start

```sh
# fetch the pinned PDFium binaries (hashes in nib-rs/tools/pdfium.lock)
nib-rs/tools/fetch-pdfium.sh

# Rust checks and tests
cd nib-rs && cargo test

# Android library
./gradlew :nib-android:assembleRelease
```

## License

Nib is licensed under Apache-2.0 (see `LICENSE`). Third-party notices,
including the PDFium BSD-3-Clause license, are in `THIRD_PARTY_LICENSES.md`.
