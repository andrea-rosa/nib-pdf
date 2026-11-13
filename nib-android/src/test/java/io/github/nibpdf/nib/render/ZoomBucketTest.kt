package io.github.nibpdf.nib.render

import kotlin.math.sqrt
import org.junit.Assert.assertEquals
import org.junit.Test

class ZoomBucketTest {
    @Test
    fun bucketFor_exactPowersOfTwo() {
        assertEquals(0, ZoomBuckets.bucketFor(1f))
        assertEquals(2, ZoomBuckets.bucketFor(2f))
        assertEquals(4, ZoomBuckets.bucketFor(4f))
        assertEquals(6, ZoomBuckets.bucketFor(8f))
    }

    @Test
    fun bucketFor_rootTwo() {
        assertEquals(1, ZoomBuckets.bucketFor(sqrt(2f)))
        assertEquals(-1, ZoomBuckets.bucketFor(1f / sqrt(2f)))
    }

    @Test
    fun bucketFor_clamps() {
        assertEquals(-2, ZoomBuckets.bucketFor(0.1f))
        assertEquals(-2, ZoomBuckets.bucketFor(ZoomBuckets.MIN_ZOOM))
        assertEquals(6, ZoomBuckets.bucketFor(ZoomBuckets.MAX_ZOOM))
        assertEquals(6, ZoomBuckets.bucketFor(100f))
    }

    @Test
    fun scaleFor_roundtrip() {
        assertEquals(4f, ZoomBuckets.scaleFor(4), 1e-6f)
        assertEquals(sqrt(2f), ZoomBuckets.scaleFor(1), 1e-4f)
        assertEquals(1f, ZoomBuckets.scaleFor(0), 1e-6f)
    }
}
