# Next development cycle - M6 fuzz license gate and first smoke runs

Follow the mandatory cycle protocol in [ROADMAP.md](./ROADMAP.md) section 0.
Read this file first, inspect the worktree, then read M6 and the latest cycle
log in that document, [fuzz/README.md](../nib-rs/fuzz/README.md), and the
parser/AP/error boundaries in [ARCHITECTURE.md](./ARCHITECTURE.md) sections
6, 7 and 10. Preserve all user changes. Do not stage, commit or push.

## Current state

- M6 is `[~]`. The malformed-PDF slice and a short-ink-point AP panic fix pass
  focused/full Rust tests, formatting, Clippy and the production dependency
  audit. The panic regression covers nine malformed point/mode combinations;
  it was reproduced directly, not discovered in a completed fuzz run.
- Three arbitrary-byte harnesses and compact generated seeds are present in
  the isolated `nib-rs/fuzz` workspace. They use actual public interfaces:
  JSON deserialization plus `Engine::import`, `Writer::open`, and `ap::build`.
  No production dependency, Android source or UniFFI surface was changed.
- **BLOCKER:** `cargo deny --manifest-path fuzz/Cargo.toml check` rejects
  `libfuzzer-sys 0.4.13`'s `(MIT OR Apache-2.0) AND NCSA` license because NCSA
  is not allowed. The policy was not changed. The instrumented build was
  interrupted when this was found; no smoke run or 24-hour run was performed.
  Root `cargo deny check` does not include this separate workspace.
- `cargo-fuzz 0.13.2` is now installed. The installed nightly reports
  `1.99.0-nightly (af3d95584 2026-07-09)`; normal checks use Rust 1.99.1.
  New fuzz-only transitives are `arbitrary 1.5.0` and `jobserver 0.1.35`.
  All new versions were published at least seven days before 2026-11-26.
- M5 is now `[x]`: actual Acrobat Reader 26.003.22110 rendered RSA-PSS/P-256
  descriptions without regeneration prompts. Native warning UI and Reader's
  own signature diagnostics reported unmodified documents, with UNKNOWN
  identity trust because the test certificates are self-signed. No trust
  settings were changed. See ROADMAP section 6 for reproduction and hashes.
  Regenerate with `NIB_KEEP_PDFS=/tmp/nib-m5-acrobat cargo +1.99.1 test -p nib-sign
  acrobat_interop_appearances`; test certificates last one day.
- M3 annotation-appearance text extraction by a suitable CLI reader remains
  unverified; structural ToUnicode/CID checks are not a substitute. B-T is
  out of scope and explicitly rejected.

## Objective and gate

Obtain an explicit maintainer license decision before resuming fuzz
instrumented builds/runs. Do not add a license exception, downgrade a
dependency to conceal its license, or claim the existing policy approves
NCSA. If no decision is available, report this blocker rather than treating
the fuzz foundation as verified.

Once explicitly authorized and the policy decision is recorded:

1. Audit the isolated workspace separately; confirm all three targets compile
   under nightly with `cargo fuzz`. Production Rust stays pinned to 1.99.1.
2. Verify seed generation and meaningful coverage of successful and rejected
   inputs. Keep each PDF below 200 KB and the checked-in seeds unchanged
   during fuzz runs (first corpus directory writable, second read-only).
3. Run each target for a bounded, recorded duration. Fix every local crash/
   panic with a deterministic regression; document upstream parser issues.
   Never swallow a panic in a harness or turn it into a successful rejection.
4. Run focused tests first, then production formatting, Clippy, full tests and
   dependency audit, plus formatting/Clippy/audit for the separate workspace.
5. Add bounded nightly CI only once local build/run and dependency policy are
   proven; keep a 24-hour qualification separate from smoke results.
6. Update ROADMAP with exact commands/durations/results and remaining work.
   Rewrite this file for the next actual task. Keep M6 `[~]` until all its
   performance, fuzzing, coverage, API and release criteria are met.

## Commands after license authorization

```sh
cd nib-rs
cargo deny --manifest-path fuzz/Cargo.toml check
cargo +1.99.1 run --locked --manifest-path fuzz/Cargo.toml --example seed_corpus
cargo +1.99.1 fmt --manifest-path fuzz/Cargo.toml --check
cargo +1.99.1 clippy --locked --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cargo +nightly fuzz build
mkdir -p fuzz/corpus/import_delta_json fuzz/corpus/open_bytes fuzz/corpus/ap_from_annotation
cargo +nightly fuzz run import_delta_json fuzz/corpus/import_delta_json fuzz/seeds/import_delta_json -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=16384
cargo +nightly fuzz run open_bytes fuzz/corpus/open_bytes fuzz/seeds/open_bytes -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=199999
cargo +nightly fuzz run ap_from_annotation fuzz/corpus/ap_from_annotation fuzz/seeds/ap_from_annotation -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048 -max_len=16384
cargo +1.99.1 test -p nib-core -p nib-ffi
cargo +1.99.1 fmt --check
cargo +1.99.1 clippy --all-targets -- -D warnings
cargo +1.99.1 test
cargo deny check
cd ..
```

Run Android validation only if Android code or the UniFFI surface changes.
