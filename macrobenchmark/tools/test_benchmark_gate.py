"""Tests for benchmark_gate.py: python3 -m unittest discover -s macrobenchmark/tools"""

import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import benchmark_gate as gate

BUILD = {"model": "Pixel 7a", "device": "lynx", "version": {"sdk": 37}}


def results(scale=1.0, overrun_p50=-8.0, model="Pixel 7a", drop=None):
    data = {
        "context": {"build": {**BUILD, "model": model}, "compilationMode": "run-from-apk"},
        "benchmarks": [
            {
                "className": "io.github.nibpdf.benchmark.FlushBenchmark",
                "name": "flush50Ops20Mb",
                "metrics": {"flush50OpsMs": {"median": 180.0 * scale}},
                "sampledMetrics": {},
            },
            {
                "className": "io.github.nibpdf.benchmark.ScrollBenchmark",
                "name": "scroll",
                "metrics": {"frameCount": {"median": 89.0 * scale}},
                "sampledMetrics": {
                    "frameOverrunMs": {"P50": overrun_p50, "P90": 15.0 * scale, "P95": 47.0 * scale, "P99": 999.0},
                },
            },
        ],
    }
    if drop:
        data["benchmarks"] = [b for b in data["benchmarks"] if b["name"] != drop]
    return data


class GateTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.baseline = self.dir / "baselines" / "p.json"
        self.assertEqual(self.gate(results(), "--update", "--note", "test"), 0)

    def tearDown(self):
        self.tmp.cleanup()

    def gate(self, data, *flags):
        f = self.dir / "run" / "x-benchmarkData.json"
        f.parent.mkdir(exist_ok=True)
        f.write_text(json.dumps(data))
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return gate.main([str(f.parent), "--baseline", str(self.baseline), *flags])

    def test_baseline_excludes_frame_count_and_p99(self):
        keys = json.loads(self.baseline.read_text())["metrics"]
        self.assertNotIn("ScrollBenchmark.scroll/frameCount/median", keys)
        self.assertNotIn("ScrollBenchmark.scroll/frameOverrunMs/P99", keys)
        self.assertIn("FlushBenchmark.flush50Ops20Mb/flush50OpsMs/median", keys)
        self.assertIn("ScrollBenchmark.scroll/frameOverrunMs/P90", keys)

    def test_ten_percent_slower_passes(self):
        self.assertEqual(self.gate(results(scale=1.10)), 0)

    def test_twenty_percent_slower_fails(self):
        self.assertEqual(self.gate(results(scale=1.20)), 1)

    def test_faster_passes(self):
        self.assertEqual(self.gate(results(scale=0.5)), 0)

    def test_negative_baseline_uses_absolute_slack(self):
        # base -8, 15% of |base| = 1.2 → limit -6.8
        self.assertEqual(self.gate(results(overrun_p50=-7.0)), 0)
        self.assertEqual(self.gate(results(overrun_p50=-6.0)), 1)

    def test_min_abs_slack_for_tiny_values(self):
        key = "FlushBenchmark.flush50Ops20Mb/flush50OpsMs/median"
        self.baseline.write_text(json.dumps({"device": {"model": "Pixel 7a", "sdk": 37}, "metrics": {key: 0.0}}))
        data = results()
        data["benchmarks"][0]["metrics"]["flush50OpsMs"]["median"] = 1.0
        self.assertEqual(self.gate(data), 0)
        data["benchmarks"][0]["metrics"]["flush50OpsMs"]["median"] = 1.5
        self.assertEqual(self.gate(data), 1)

    def test_missing_metric_fails(self):
        self.assertEqual(self.gate(results(drop="flush50Ops20Mb")), 1)

    def test_device_mismatch_refused(self):
        self.assertEqual(self.gate(results(model="sdk_gphone64_x86_64")), 2)

    def test_missing_baseline(self):
        self.baseline.unlink()
        self.assertEqual(self.gate(results()), 2)
        self.assertEqual(self.gate(results(), "--allow-missing"), 0)


if __name__ == "__main__":
    unittest.main()
