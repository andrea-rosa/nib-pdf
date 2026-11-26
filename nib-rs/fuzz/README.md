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

## License and current results

The maintainer approved a version-specific NCSA exception for
`libfuzzer-sys =0.4.13` in `deny.toml`. This crate is used only by this isolated
fuzz workspace and is not included in production or shipped Android artifacts.
Audit this workspace separately with
`cargo deny --manifest-path fuzz/Cargo.toml check`; the production workspace
audit does not include it.

The three targets built and each completed a 60-second smoke run on 2026-11-26:
`import_delta_json` (316,498 executions), `open_bytes` (85,428), and
`ap_from_annotation` (316,670). The PDF target found an AddressSanitizer stack
overflow from cyclic AcroForm `/Kids`. The parser now skips repeated field
object IDs and limits tree depth to 64; deterministic regressions cover both
cases, and replaying the original crash artifact completed without a crash.
These bounded smoke runs do not constitute the M6 24-hour qualification.

## Seeds and commands

The checked-in `seeds/` are generated solely from this repository's small PDF
fixtures and synthetic JSON/malformed inputs. Every PDF is under 200 KB.
Regenerate intentionally with:

```sh
cd nib-rs
cargo +1.99.1 run --locked --manifest-path fuzz/Cargo.toml --example seed_corpus
cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check
```

Validate the separate dependency graph as well as the production workspace,
then build and smoke-run:

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
The [scheduled workflow](../../.github/workflows/fuzz.yml) runs daily at
03:17 UTC and can also be started manually. It bounds each target to five
minutes, caps the job at 45 minutes, and uploads `fuzz/artifacts/` when a run
fails. Generated corpora and artifacts remain ignored and are not committed.
A 60-second local smoke run or scheduled CI run is not the M6 24-hour
qualification. Host asset/font inputs and broader resource-limit testing
remain outstanding.

## 24-hour qualification

The M6 release gate requires 24 hours of fuzzing on each target with zero
crashes, or every finding fixed with a regression test. On GitHub-hosted
runners, dispatch
[`fuzz-qualification.yml`](../../.github/workflows/fuzz-qualification.yml)
(`gh workflow run fuzz-qualification.yml`). It chains five 5-hour segments per
target and passes the corpus between segments as artifacts. To reproduce a
`fuzz-crash-*` artifact, run `cargo +nightly fuzz run <target> <crash-file>`.
On a local machine, run each target with `-max_total_time=86400` instead.
