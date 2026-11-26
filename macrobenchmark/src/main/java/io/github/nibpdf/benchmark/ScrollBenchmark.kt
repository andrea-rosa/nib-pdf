package io.github.nibpdf.benchmark

import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.ExperimentalMetricApi
import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@LargeTest
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalMetricApi::class)
class ScrollBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun scroll() =
        benchmarkRule.measureRepeated(
            packageName = TARGET_PACKAGE,
            metrics =
                listOf(
                    FrameTimingMetric(),
                    CounterSamplesMetric(NibTraceNames.TILE_RENDER_MS, "tileRasterMs"),
                    CounterSamplesMetric(NibTraceNames.TILE_LATENCY_US, "tileLatencyMs", scale = 0.001),
                ),
            compilationMode = CompilationMode.None(),
            iterations = 3,
            startupMode = StartupMode.COLD,
            setupBlock = {
                pressHome()
            },
        ) {
            // COLD mode kills the app right before measurement, so the
            // measured section must launch it.
            launchSample("nib.asset" to "text-100.pdf")
            val cx = device.displayWidth / 2
            repeat(5) {
                device.swipe(cx, 1600, cx, 400, 10)
                device.waitForIdle()
            }
        }
}
