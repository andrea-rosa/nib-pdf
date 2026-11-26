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
 * ROADMAP §7 budget: first visible page ≤ 300 ms from `from_bytes` for a
 * 10 MB text PDF. Measures `nib.firstVisiblePage` (NibDocument.open →
 * every visible tile of the first page drawn); PDF generation runs before
 * the section starts.
 */
@LargeTest
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalMetricApi::class)
class StartupBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun firstVisiblePage10Mb() =
        benchmarkRule.measureRepeated(
            packageName = TARGET_PACKAGE,
            metrics =
                listOf(
                    FirstSliceMsMetric(NibTraceNames.FIRST_VISIBLE_PAGE, "firstVisiblePageMs"),
                    FirstSliceMsMetric(NibTraceNames.OPEN, "openMs"),
                    FirstSliceMsMetric(NibTraceNames.FROM_BYTES, "fromBytesMs"),
                    FirstSliceMsMetric(NibTraceNames.PAGE_INFO, "pageInfoMs"),
                ),
            compilationMode = CompilationMode.None(),
            iterations = 5,
            startupMode = StartupMode.COLD,
            setupBlock = { pressHome() },
        ) {
            launchSample("nib.synthetic" to "text:10")
            // The document title appears before tiles land; let them render.
            device.waitForIdle()
            Thread.sleep(2_000)
        }
}
