package io.github.nibpdf.nib

import android.graphics.Bitmap
import kotlin.math.cos
import kotlin.math.sqrt

/**
 * 64-bit perceptual hash: grayscale → 32×32 → DCT-II, take the 8×8
 * low-frequency block (skipping DC), threshold at the median.
 */
internal object PHash {
    fun hash(bmp: Bitmap): Long {
        val small = Bitmap.createScaledBitmap(bmp, 32, 32, true)
        val px = IntArray(32 * 32)
        small.getPixels(px, 0, 32, 0, 0, 32, 32)
        val g =
            DoubleArray(32 * 32) { i ->
                val p = px[i]
                0.299 * (p shr 16 and 0xFF) + 0.587 * (p shr 8 and 0xFF) + 0.114 * (p and 0xFF)
            }
        // DCT-II, keep the 8x8 top-left block; index 0 is DC and skipped.
        val low = DoubleArray(8 * 8)
        for (v in 0 until 8) {
            for (u in 0 until 8) {
                var sum = 0.0
                for (y in 0 until 32) {
                    for (x in 0 until 32) {
                        sum +=
                            g[y * 32 + x] *
                            cos((2 * x + 1) * u * Math.PI / 64.0) *
                            cos((2 * y + 1) * v * Math.PI / 64.0)
                    }
                }
                val cu = if (u == 0) 1.0 / sqrt(2.0) else 1.0
                val cv = if (v == 0) 1.0 / sqrt(2.0) else 1.0
                low[v * 8 + u] = 0.25 * cu * cv * sum
            }
        }
        val median = low.copyOfRange(1, 64).sorted()[31]
        var h = 0L
        for (i in 1 until 64) {
            if (low[i] > median) h = h or (1L shl i)
        }
        if (small !== bmp) small.recycle()
        return h
    }

    fun distance(
        a: Bitmap,
        b: Bitmap,
    ): Int = (hash(a) xor hash(b)).countOneBits()
}
