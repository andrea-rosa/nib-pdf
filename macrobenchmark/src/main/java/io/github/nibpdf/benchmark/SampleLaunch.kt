package io.github.nibpdf.benchmark

import android.content.Intent
import androidx.benchmark.macro.MacrobenchmarkScope
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until

/** Title prefix the sample shows once a document is open (`"Nib — N pages"`). */
private const val OPEN_TITLE_PREFIX = "Nib — "

/**
 * Starts the sample's MainActivity with [extras] (see sample-app
 * `BenchmarkHooks.kt`) and waits until the document is open.
 */
fun MacrobenchmarkScope.launchSample(
    vararg extras: Pair<String, Any>,
    openTimeoutMs: Long = 120_000,
) {
    startActivityAndWait(
        Intent().apply {
            setClassName(packageName, "io.github.nibpdf.sample.MainActivity")
            extras.forEach { (k, v) ->
                when (v) {
                    is String -> putExtra(k, v)
                    is Int -> putExtra(k, v)
                    else -> error("unsupported extra type for $k")
                }
            }
        },
    )
    check(device.wait(Until.hasObject(By.textStartsWith(OPEN_TITLE_PREFIX)), openTimeoutMs)) {
        "document did not open within $openTimeoutMs ms"
    }
}
