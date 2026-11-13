package io.github.nibpdf.nib.render

import kotlin.math.log2
import kotlin.math.pow
import kotlin.math.roundToInt

/**
 * Zoom buckets: `scale = 2^(bucket/2)` (√2 steps). Tiles are rendered at the
 * bucket scale; between buckets the view upscales the nearest cached bucket.
 */
public object ZoomBuckets {
    public const val TILE_SIZE: Int = 512
    public const val MIN_ZOOM: Float = 0.5f
    public const val MAX_ZOOM: Float = 8f
    private const val MIN_BUCKET: Int = -2
    private const val MAX_BUCKET: Int = 6

    /** Bucket for a continuous scale, clamped to 0.5×–8×. */
    public fun bucketFor(scale: Float): Int =
        (2 * log2(scale.toDouble().coerceIn(MIN_ZOOM.toDouble(), MAX_ZOOM.toDouble())))
            .roundToInt()
            .coerceIn(MIN_BUCKET, MAX_BUCKET)

    /** Scale for a bucket. */
    public fun scaleFor(bucket: Int): Float = 2.0.pow(bucket.coerceIn(MIN_BUCKET, MAX_BUCKET) / 2.0).toFloat()
}
