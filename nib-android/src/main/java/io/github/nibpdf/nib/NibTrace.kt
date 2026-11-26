package io.github.nibpdf.nib

import android.os.Build
import android.os.Trace
import java.util.concurrent.atomic.AtomicInteger

/**
 * System-trace markers read by the `:macrobenchmark` performance suites
 * (ROADMAP §7). Async sections and counters need API 29; on older releases
 * they are no-ops. Names are part of the benchmark contract — keep them in
 * sync with `macrobenchmark/.../NibMetrics.kt`.
 */
internal object NibTrace {
    /** `NibDocument.open` → first visible page drawn (async section). */
    const val FIRST_VISIBLE_PAGE = "nib.firstVisiblePage"

    /** Synchronous `from_bytes` + page metadata load. */
    const val OPEN = "nib.open"

    /** Inside [OPEN]: the Rust `from_bytes` call. */
    const val FROM_BYTES = "nib.fromBytes"

    /** Inside [OPEN]: per-page metadata load over FFI. */
    const val PAGE_INFO = "nib.pageInfo"

    /** Synchronous `NibDocument.flush`. */
    const val FLUSH = "nib.flush"

    /** Per-tile PDFium render time reported by Rust (`render_ms`, counter). */
    const val TILE_RENDER_MS = "nib.tile.renderMs"

    /**
     * Per-tile delivery latency in µs (counter): the `renderTile` FFI call
     * (Rust render-worker queue wait, page load, raster, pixel copy) plus
     * Bitmap conversion.
     */
    const val TILE_LATENCY_US = "nib.tile.latencyUs"

    /** Ink input event uptime → live-stroke draw, in ms (counter). */
    const val INK_LATENCY_MS = "nib.ink.latencyMs"

    private val cookies = AtomicInteger()

    fun newCookie(): Int = cookies.incrementAndGet()

    inline fun <T> section(
        name: String,
        block: () -> T,
    ): T {
        Trace.beginSection(name)
        try {
            return block()
        } finally {
            Trace.endSection()
        }
    }

    fun beginAsync(
        name: String,
        cookie: Int,
    ) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) Trace.beginAsyncSection(name, cookie)
    }

    fun endAsync(
        name: String,
        cookie: Int,
    ) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) Trace.endAsyncSection(name, cookie)
    }

    fun counter(
        name: String,
        value: Long,
    ) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) Trace.setCounter(name, value)
    }
}
