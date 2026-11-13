package io.github.nibpdf.nib

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.launch

/**
 * Auto-flush policy for `NibPdfViewState`: flush after [idleMs] without new
 * ops, or immediately once [maxOps] unflushed ops accumulate.
 */
public data class FlushPolicy(
    val idleMs: Long = 2000,
    val maxOps: Int = 50,
    val enabled: Boolean = true,
)

/**
 * Collects [ticks] (an annotation-version flow) and flushes per [policy]:
 * an immediate flush when `opsSinceFlush() >= maxOps`, else a debounced
 * flush [policy.idleMs] after the last tick. Flush errors are logged, never
 * thrown. Lambdas are injected so this is JVM-testable.
 */
internal class AutoFlusher(
    private val policy: FlushPolicy,
    private val scope: CoroutineScope,
    private val ticks: Flow<Int>,
    private val uncommitted: () -> Int,
    private val opsSinceFlush: () -> Int,
    private val flush: () -> Unit,
    private val warn: (String, Throwable) -> Unit = { m, t -> Log.w("nib", m, t) },
) {
    private var job: Job? = null

    fun start() {
        if (!policy.enabled || job != null) return
        job =
            scope.launch {
                var pending: Job? = null
                ticks.collect {
                    if (opsSinceFlush() >= policy.maxOps) {
                        pending?.cancel()
                        tryFlush()
                    } else {
                        pending?.cancel()
                        pending =
                            launch {
                                delay(policy.idleMs)
                                tryFlush()
                            }
                    }
                }
            }
    }

    fun stop() {
        job?.cancel()
        job = null
    }

    private fun tryFlush() {
        if (uncommitted() <= 0) return
        runCatching { flush() }.onFailure { warn("auto-flush failed", it) }
    }
}
