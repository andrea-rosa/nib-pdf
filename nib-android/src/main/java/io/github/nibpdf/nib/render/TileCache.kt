package io.github.nibpdf.nib.render

import androidx.collection.LruCache
import kotlin.math.min

/**
 * Byte-budgeted LRU for tiles. Generic so it is pure-JVM testable (`T` is
 * `android.graphics.Bitmap` in production).
 *
 * @param maxBytes budget in bytes (clamped to `Int.MAX_VALUE`)
 * @param sizeOf byte cost of one entry
 * @param onEvict called when an entry leaves the cache for any reason other
 *   than being replaced by a `put` for the same key
 */
internal class TileCache<K : Any, T : Any>(
    maxBytes: Long,
    private val sizeOf: (T) -> Int,
    private val onEvict: (T) -> Unit = {},
) {
    private val lru =
        object : LruCache<K, T>(min(maxBytes, Int.MAX_VALUE.toLong()).toInt()) {
            override fun sizeOf(
                key: K,
                value: T,
            ): Int = this@TileCache.sizeOf(value)

            override fun entryRemoved(
                evicted: Boolean,
                key: K,
                oldValue: T,
                newValue: T?,
            ) {
                // `evicted == false` covers explicit remove() and replacement
                // by put(); replaced values are still released to the pool.
                if (oldValue != null && (evicted || newValue !== oldValue)) {
                    this@TileCache.onEvict(oldValue)
                }
            }
        }

    operator fun get(key: K): T? = lru[key]

    fun put(
        key: K,
        value: T,
    ) {
        lru.put(key, value)
    }

    fun remove(key: K) {
        lru.remove(key)
    }

    fun clear() {
        lru.evictAll()
    }

    fun trimToSize(maxSize: Int) {
        lru.trimToSize(maxSize)
    }

    fun size(): Int = lru.size()

    fun snapshot(): Map<K, T> = lru.snapshot()

    /** Entries whose key fails `keep` are evicted. */
    fun evictMatching(keep: (K) -> Boolean) {
        val stale = lru.snapshot().keys.filterNot(keep)
        stale.forEach { lru.remove(it) }
    }

    /**
     * `ComponentCallbacks2` trim levels (mirrored here so the class stays
     * pure-JVM testable): clears at TRIM_MEMORY_RUNNING_LOW (10) or more
     * severe, halves at TRIM_MEMORY_RUNNING_MODERATE (5).
     */
    fun trim(level: Int) {
        when {
            level >= TRIM_MEMORY_RUNNING_LOW -> clear()
            level >= TRIM_MEMORY_RUNNING_MODERATE -> trimToSize(lru.size() / 2)
        }
    }

    companion object {
        // ComponentCallbacks2 values; duplicated to keep this class JVM-only.
        const val TRIM_MEMORY_RUNNING_MODERATE: Int = 5
        const val TRIM_MEMORY_RUNNING_LOW: Int = 10

        /** `min(heap/8, 96 MB)` per ARCHITECTURE §5.1. */
        fun defaultBudget(): Long = min(Runtime.getRuntime().maxMemory() / 8, 96L * 1024 * 1024)
    }
}
