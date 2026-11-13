package io.github.nibpdf.benchmark

import android.content.Intent
import androidx.benchmark.macro.CompilationMode
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
class ScrollBenchmark {
    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun scroll() =
        benchmarkRule.measureRepeated(
            packageName = "io.github.nibpdf.sample",
            metrics = listOf(FrameTimingMetric()),
            compilationMode = CompilationMode.None(),
            iterations = 3,
            startupMode = StartupMode.COLD,
            setupBlock = {
                pressHome()
                startActivityAndWait(
                    Intent().apply {
                        setClassName(packageName, "io.github.nibpdf.sample.MainActivity")
                        putExtra("nib.asset", "text-100.pdf")
                    },
                )
                // Give the document time to open and first tiles to render.
                Thread.sleep(2_000)
                device.waitForIdle()
            },
        ) {
            val cx = device.displayWidth / 2
            repeat(5) {
                device.swipe(cx, 1600, cx, 400, 10)
                device.waitForIdle()
            }
        }
}
