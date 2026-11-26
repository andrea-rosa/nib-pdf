# Untrusted-input fuzz targets

This standalone workspace does not add production dependencies or change the
Android/UniFFI build. Its lockfile preserves the production dependency versions.

| Target | Boundary |
|---|---|
| `import_delta_json` | Arbitrary JSON bytes -> `DeltaDocument` -> `Engine::import`, all eight import-option combinations; successful imports also exercise export and undo/redo |
| `open_bytes` | Arbitrary PDF bytes -> `Writer::open` (lopdf, annotation and form import); no PDFium or FFI panic catcher |
| `ap_from_annotation` | Arbitrary JSON bytes -> `Annotation` -> `ap::build`; inline image/vector stamps are reachable, but host asset/font providers are not fuzzed |

Malformed input may return a parse/validation/AP error. The harnesses do not
catch panics: a panic must remain a finding, not a successful rejection.

## Current blocker

`cargo deny --manifest-path fuzz/Cargo.toml check`, from `nib-rs/`, rejects
`libfuzzer-sys 0.4.13`: its license is `(MIT OR Apache-2.0) AND NCSA`, and NCSA
is not allowed by the existing policy. No exception has been added. Obtain an
explicit maintainer decision before resuming the instrumented build or running
these targets; do not remove the license requirement or silently use an older
manifest to bypass it.

The initial `cargo +nightly fuzz build` was interrupted when this audit failed.
No instrumented build or smoke run is claimed. The malformed-ink seed was
reproduced separately as a deterministic `nib-core` unit test and fixed.

## Seeds and commands

The checked-in `seeds/` are generated solely from this repository's small PDF
fixtures and synthetic JSON/malformed inputs. Every PDF is under 200 KB.
Regenerate intentionally with:

```sh
cd nib-rs
cargo +1.99.1 run --locked --manifest-path fuzz/Cargo.toml --example seed_corpus
cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check
```

After the license decision is resolved, validate the separate dependency graph
as well as the production workspace, then build and smoke-run:

```sh
cd nib-rs
cargo deny --manifest-path fuzz/Cargo.toml check
cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cargo +nightly fuzz build
mkdir -p fuzz/corpus/import_delta_json fuzz/corpus/open_bytes fuzz/corpus/ap_from_annotation
cargo +nightly fuzz run import_delta_json fuzz/corpus/import_delta_json fuzz/seeds/import_delta_json -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=16384
cargo +nightly fuzz run open_bytes fuzz/corpus/open_bytes fuzz/seeds/open_bytes -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=199999
cargo +nightly fuzz run ap_from_annotation fuzz/corpus/ap_from_annotation fuzz/seeds/ap_from_annotation -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=16384
```

`cargo-fuzz 0.13.2` and a nightly Rust toolchain are required for instrumented
runs; the normal workspace remains on Rust 1.99.1. The first corpus directory
is writable and ignored; the checked-in seeds are additional read-only inputs.
Record each actual duration/result and keep artifacts for deterministic
regressions. A 60-second smoke run is not the M6 24-hour qualification. Longer
runs, nightly CI, host asset/font inputs and broader resource-limit testing
remain outstanding.
