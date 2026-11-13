package io.github.nibpdf.nib

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/** `AutoFlusher`: debounced idle flush, immediate flush at `maxOps`, disabled. */
@OptIn(ExperimentalCoroutinesApi::class)
class FlushPolicyTest {
    @Test
    fun idleDebounceFlushesOnce() =
        runTest {
            var uncommitted = 1
            var flushes = 0
            val ticks = MutableStateFlow(0)
            AutoFlusher(
                FlushPolicy(idleMs = 100),
                backgroundScope,
                ticks,
                uncommitted = { uncommitted },
                opsSinceFlush = { 1 },
                flush = {
                    flushes++
                    uncommitted = 0
                },
                warn = { _, _ -> },
            ).start()
            ticks.value = 1
            advanceTimeBy(50)
            // A second mutation inside the idle window resets the timer.
            uncommitted = 1
            ticks.value = 2
            advanceTimeBy(50)
            assertEquals(0, flushes)
            advanceTimeBy(60)
            assertEquals(1, flushes)
        }

    @Test
    fun maxOpsFlushesImmediately() =
        runTest {
            var uncommitted = 5
            var flushes = 0
            val ticks = MutableStateFlow(0)
            AutoFlusher(
                FlushPolicy(idleMs = 10_000, maxOps = 50),
                backgroundScope,
                ticks,
                uncommitted = { uncommitted },
                opsSinceFlush = { 60 },
                flush = {
                    flushes++
                    uncommitted = 0
                },
                warn = { _, _ -> },
            ).start()
            ticks.value = 1
            advanceTimeBy(1)
            assertEquals(1, flushes)
        }

    @Test
    fun cleanDocumentNeverFlushes() =
        runTest {
            var flushes = 0
            val ticks = MutableStateFlow(0)
            AutoFlusher(
                FlushPolicy(idleMs = 100),
                backgroundScope,
                ticks,
                uncommitted = { 0 },
                opsSinceFlush = { 1 },
                flush = { flushes++ },
                warn = { _, _ -> },
            ).start()
            ticks.value = 1
            advanceTimeBy(200)
            assertEquals(0, flushes)
        }

    @Test
    fun disabledNeverCollects() =
        runTest {
            var flushes = 0
            val ticks = MutableStateFlow(0)
            AutoFlusher(
                FlushPolicy(enabled = false),
                backgroundScope,
                ticks,
                uncommitted = { 1 },
                opsSinceFlush = { 100 },
                flush = { flushes++ },
                warn = { _, _ -> },
            ).start()
            ticks.value = 1
            launch { delay(1) }
            advanceTimeBy(10_000)
            assertEquals(0, flushes)
        }
}
