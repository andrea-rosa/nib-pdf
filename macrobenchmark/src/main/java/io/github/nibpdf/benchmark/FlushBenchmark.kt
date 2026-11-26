package io.github.nibpdf.benchmark

import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.ExperimentalMetricApi
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * ROADMAP §7 budget: `flush()` with 50 ops ≤ 150 ms on a 20 MB PDF. The
 * sample applies 50 Square creates and flushes once before showing the
 * document; `nib.flush` wraps only the Rust `flush_with` call.
 */
@LargeTest
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalMetricApi::class)
class FlushBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun flush50Ops20Mb() =
        benchmarkRule.measureRepeated(
            packageName = TARGET_PACKAGE,
            metrics = listOf(FirstSliceMsMetric(NibTraceNames.FLUSH, "flush50OpsMs")),
            compilationMode = CompilationMode.None(),
            iterations = 5,
            startupMode = StartupMode.COLD,
            setupBlock = { pressHome() },
        ) {
            launchSample("nib.synthetic" to "text:20", "nib.bench.flushOps" to 50)
        }
}
