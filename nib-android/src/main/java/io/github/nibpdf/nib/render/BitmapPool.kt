package io.github.nibpdf.nib.render

import android.graphics.Bitmap
import java.util.ArrayDeque

/**
 * Reuse pool for tile bitmaps, keyed by (width, height). Bounded: excess
 * bitmaps are recycled rather than pooled.
 */
internal class BitmapPool(
    private val maxPooled: Int = 16,
) {
    private val pools = HashMap<Long, ArrayDeque<Bitmap>>()
    private var pooled = 0

    private fun keyOf(
        width: Int,
        height: Int,
    ): Long = width.toLong() shl 32 or (height.toLong() and 0xFFFF_FFFFL)

    @Synchronized
    fun obtain(
        width: Int,
        height: Int,
    ): Bitmap {
        val key = keyOf(width, height)
        val b = pools[key]?.pollFirst()
        if (b != null) {
            pooled--
            return b
        }
        return Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
    }

    @Synchronized
    fun recycle(bitmap: Bitmap) {
        if (bitmap.isRecycled || pooled >= maxPooled) {
            if (!bitmap.isRecycled) bitmap.recycle()
            return
        }
        pools.getOrPut(keyOf(bitmap.width, bitmap.height)) { ArrayDeque() }.addLast(bitmap)
        pooled++
    }

    @Synchronized
    fun clear() {
        pools.values.forEach { q -> q.forEach { it.recycle() } }
        pools.clear()
        pooled = 0
    }
}
