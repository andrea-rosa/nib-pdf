package io.github.nibpdf.benchmark

import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.ExperimentalMetricApi
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * ROADMAP §7 budget: ink touch-to-draw ≤ 1 frame. `nib.ink.latencyMs` is
 * the input event's uptime → the live-stroke draw pass that first shows it
 * (excludes display scan-out). Each swipe is one stroke with the Ink tool.
 */
@LargeTest
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalMetricApi::class)
class InkLatencyBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun inkTouchToDraw() =
        benchmarkRule.measureRepeated(
            packageName = TARGET_PACKAGE,
            metrics = listOf(CounterSamplesMetric(NibTraceNames.INK_LATENCY_MS, "inkTouchToDrawMs")),
            compilationMode = CompilationMode.None(),
            iterations = 3,
            startupMode = null,
            setupBlock = {
                killProcess()
                launchSample("nib.asset" to "text-100.pdf", "nib.tool" to "Ink")
                device.waitForIdle()
            },
        ) {
            val w = device.displayWidth
            val h = device.displayHeight
            repeat(5) { i ->
                val y = h * (3 + i) / 10
                device.swipe(w / 5, y, w * 4 / 5, y + h / 20, 60)
                device.waitForIdle()
            }
        }
}
