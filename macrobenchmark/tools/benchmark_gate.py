#!/usr/bin/env python3
"""Macrobenchmark regression gate (ROADMAP §7, M6).

Compares a Macrobenchmark `*benchmarkData.json` against a committed baseline
profile in `macrobenchmark/baselines/<profile>.json` and fails when any gated
metric regresses by more than the threshold (default 15%).

Gated values (all lower-is-better):
  * `metrics.<name>`: median across iterations (frameCount is excluded);
  * `sampledMetrics.<name>`: P50, P90 and P95 (P99 is too noisy at 3-5
    iterations).

A metric fails when ``current > base + max(abs(base) * threshold, min_abs)``.
``abs(base)`` keeps the rule meaningful for negative values (frameOverrunMs
P50 is negative when frames finish early); ``min_abs`` (default 1.0 ms)
stops 0-1 ms integer counters from failing on a single-millisecond tick.

Baselines are tied to one device + SDK: comparing a run from a different
model or SDK is refused (exit 2) so emulator, host and physical-device
numbers are never mixed.

Exit codes: 0 pass, 1 regression or missing metric, 2 usage/device mismatch.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

EXCLUDED_METRICS = {"frameCount"}
SAMPLED_PERCENTILES = ("P50", "P90", "P95")


def find_results(path: Path) -> Path:
    if path.is_file():
        return path
    found = sorted(path.rglob("*benchmarkData.json"))
    if len(found) != 1:
        raise SystemExit(f"expected exactly one *benchmarkData.json under {path}, found {len(found)}")
    return found[0]


def device_of(context: dict) -> dict:
    build = context.get("build", {})
    return {
        "model": build.get("model"),
        "device": build.get("device"),
        "sdk": build.get("version", {}).get("sdk"),
        "type": build.get("type"),
        "fingerprint": build.get("fingerprint"),
        "compilationMode": context.get("compilationMode"),
    }


def extract(data: dict) -> dict[str, float]:
    out: dict[str, float] = {}
    for bench in data.get("benchmarks", []):
        prefix = f"{bench['className'].rsplit('.', 1)[-1]}.{bench['name']}"
        for name, m in bench.get("metrics", {}).items():
            if name not in EXCLUDED_METRICS:
                out[f"{prefix}/{name}/median"] = float(m["median"])
        for name, m in bench.get("sampledMetrics", {}).items():
            for p in SAMPLED_PERCENTILES:
                if p in m:
                    out[f"{prefix}/{name}/{p}"] = float(m[p])
    return out


def compare(
    baseline: dict[str, float],
    current: dict[str, float],
    threshold: float,
    min_abs: float,
) -> tuple[list[str], list[str]]:
    """Returns (report lines, failures)."""
    lines, failures = [], []
    for key in sorted(baseline):
        base = baseline[key]
        if key not in current:
            failures.append(f"{key}: missing from results")
            lines.append(f"MISSING  {key}")
            continue
        cur = current[key]
        limit = base + max(abs(base) * threshold, min_abs)
        if base:
            delta = f"{(cur - base) / abs(base) * 100:+.1f}%"
        else:
            delta = f"{cur - base:+.3f} abs"
        ok = cur <= limit
        lines.append(f"{'ok  ' if ok else 'FAIL'}     {key}: {cur:.3f} vs {base:.3f} (limit {limit:.3f}, {delta})")
        if not ok:
            failures.append(f"{key}: {cur:.3f} > {limit:.3f} (baseline {base:.3f}, {delta})")
    for key in sorted(set(current) - set(baseline)):
        lines.append(f"new      {key}: {current[key]:.3f} (not in baseline, not gated)")
    return lines, failures


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("results", type=Path, help="benchmarkData.json or a directory containing one")
    ap.add_argument("--baseline", type=Path, required=True, help="baseline profile JSON")
    ap.add_argument("--threshold", type=float, default=0.15, help="allowed relative regression (default 0.15)")
    ap.add_argument("--min-abs", type=float, default=1.0, help="minimum absolute slack in metric units (default 1.0)")
    ap.add_argument("--update", action="store_true", help="write the baseline from these results")
    ap.add_argument("--note", default="", help="free-text note stored with --update")
    ap.add_argument("--allow-missing", action="store_true", help="exit 0 when the baseline file does not exist")
    args = ap.parse_args(argv)

    results = find_results(args.results)
    data = json.loads(results.read_text())
    current = extract(data)
    device = device_of(data.get("context", {}))

    if args.update:
        args.baseline.parent.mkdir(parents=True, exist_ok=True)
        args.baseline.write_text(
            json.dumps(
                {"note": args.note, "device": device, "threshold": args.threshold, "metrics": current},
                indent=2,
                sort_keys=True,
            )
            + "\n"
        )
        print(f"wrote {len(current)} metrics to {args.baseline}")
        return 0

    if not args.baseline.exists():
        msg = f"baseline {args.baseline} does not exist"
        if args.allow_missing:
            print(f"{msg}; skipping gate (--allow-missing). Candidate metrics:")
            for k in sorted(current):
                print(f"  {k}: {current[k]:.3f}")
            return 0
        print(msg, file=sys.stderr)
        return 2

    base = json.loads(args.baseline.read_text())
    base_dev = base.get("device", {})
    mismatch = [k for k in ("model", "sdk") if base_dev.get(k) != device.get(k)]
    if mismatch:
        print(
            f"device mismatch on {', '.join(mismatch)}: baseline {base_dev} vs results {device}; refusing to compare",
            file=sys.stderr,
        )
        return 2

    lines, failures = compare(base["metrics"], current, args.threshold, args.min_abs)
    print("\n".join(lines))
    if failures:
        print(f"\n{len(failures)} regression(s) beyond {args.threshold:.0%}:", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print(f"\nall {len(base['metrics'])} gated metrics within {args.threshold:.0%}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
