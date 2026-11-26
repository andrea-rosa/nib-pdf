package io.github.nibpdf.benchmark

import androidx.benchmark.macro.ExperimentalMetricApi
import androidx.benchmark.macro.TraceMetric
import androidx.benchmark.traceprocessor.TraceProcessor

/**
 * Trace markers emitted by `nib-android` (`NibTrace.kt`); keep in sync.
 */
object NibTraceNames {
    const val FIRST_VISIBLE_PAGE = "nib.firstVisiblePage"
    const val OPEN = "nib.open"
    const val FROM_BYTES = "nib.fromBytes"
    const val PAGE_INFO = "nib.pageInfo"
    const val FLUSH = "nib.flush"
    const val TILE_RENDER_MS = "nib.tile.renderMs"
    const val TILE_LATENCY_US = "nib.tile.latencyUs"
    const val INK_LATENCY_MS = "nib.ink.latencyMs"
}

const val TARGET_PACKAGE = "io.github.nibpdf.sample"

private fun processFilter(pkg: String) = "(p.name = '$pkg' OR p.name LIKE '$pkg:%')"

/**
 * Duration in ms of the first slice named [sliceName] (thread or async track)
 * in the target process — one value per iteration.
 */
@OptIn(ExperimentalMetricApi::class)
class FirstSliceMsMetric(
    private val sliceName: String,
    private val label: String,
) : TraceMetric() {
    override fun getMeasurements(
        captureInfo: CaptureInfo,
        traceSession: TraceProcessor.Session,
    ): List<Measurement> {
        val durNs =
            traceSession
                .query(
                    """
                    SELECT s.dur AS dur FROM slice s
                    LEFT JOIN thread_track tt ON s.track_id = tt.id
                    LEFT JOIN thread t ON tt.utid = t.utid
                    LEFT JOIN process_track pt ON s.track_id = pt.id
                    JOIN process p ON p.upid = COALESCE(t.upid, pt.upid)
                    WHERE s.name = '$sliceName' AND s.dur > 0
                      AND ${processFilter(captureInfo.targetPackageName)}
                    ORDER BY s.ts LIMIT 1
                    """.trimIndent(),
                ).firstOrNull()
                ?.long("dur")
                ?: error("no '$sliceName' slice in ${captureInfo.targetPackageName}")
        return listOf(Measurement(label, durNs / 1_000_000.0))
    }
}

/**
 * Every value of the process counter [counterName] in the target process,
 * reported as a sampled metric (P50/P90/P95/P99 across all iterations),
 * each value multiplied by [scale] (e.g. 0.001 for µs → ms).
 */
@OptIn(ExperimentalMetricApi::class)
class CounterSamplesMetric(
    private val counterName: String,
    private val label: String,
    private val scale: Double = 1.0,
) : TraceMetric() {
    override fun getMeasurements(
        captureInfo: CaptureInfo,
        traceSession: TraceProcessor.Session,
    ): List<Measurement> {
        val values =
            traceSession
                .query(
                    """
                    SELECT c.value AS value FROM counter c
                    JOIN process_counter_track ct ON c.track_id = ct.id
                    JOIN process p ON ct.upid = p.upid
                    WHERE ct.name = '$counterName'
                      AND ${processFilter(captureInfo.targetPackageName)}
                    ORDER BY c.ts
                    """.trimIndent(),
                ).map { it.double("value") * scale }
                .toList()
        check(values.isNotEmpty()) { "no '$counterName' counter samples in ${captureInfo.targetPackageName}" }
        return listOf(Measurement(label, values))
    }
}
