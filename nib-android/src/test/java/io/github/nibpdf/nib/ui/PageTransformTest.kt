package io.github.nibpdf.nib.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import io.github.nibpdf.nib.PageRotation
import org.junit.Assert.assertEquals
import org.junit.Test
import kotlin.random.Random

class PageTransformTest {
    private val crop = Rect(100f, 150f, 500f, 750f)

    @Test
    fun roundTripAllRotations() {
        val rnd = Random(42)
        for (rot in PageRotation.entries) {
            val t = PageTransform(crop, rot, k = 2.5f)
            repeat(50) {
                val p = Offset(100f + rnd.nextFloat() * 400f, 150f + rnd.nextFloat() * 600f)
                val back = t.toUser(t.toContent(p))
                assertEquals(p.x, back.x, 1e-3f)
                assertEquals(p.y, back.y, 1e-3f)
            }
        }
    }

    @Test
    fun rotationTableCorners() {
        // Crop box (100,150)-(500,750), k=1. Content origin = page top-left, y-down.
        val t0 = PageTransform(crop, PageRotation.R0, 1f)
        assertContent(t0.toContent(Offset(100f, 150f)), 0f, 600f)
        assertContent(t0.toContent(Offset(100f, 750f)), 0f, 0f)
        assertContent(t0.toContent(Offset(500f, 750f)), 400f, 0f)

        val t90 = PageTransform(crop, PageRotation.R90, 1f)
        // (k(y−y0), k(x−x0))
        assertContent(t90.toContent(Offset(100f, 150f)), 0f, 0f)
        assertContent(t90.toContent(Offset(100f, 750f)), 600f, 0f)
        assertContent(t90.toContent(Offset(500f, 750f)), 600f, 400f)

        val t180 = PageTransform(crop, PageRotation.R180, 1f)
        // (k(x1−x), k(y−y0))
        assertContent(t180.toContent(Offset(100f, 150f)), 400f, 0f)
        assertContent(t180.toContent(Offset(500f, 750f)), 0f, 600f)

        val t270 = PageTransform(crop, PageRotation.R270, 1f)
        // (k(y1−y), k(x1−x))
        assertContent(t270.toContent(Offset(100f, 150f)), 600f, 400f)
        assertContent(t270.toContent(Offset(500f, 750f)), 0f, 0f)
    }

    private fun assertContent(
        p: Offset,
        x: Float,
        y: Float,
    ) {
        assertEquals(x, p.x, 1e-4f)
        assertEquals(y, p.y, 1e-4f)
    }

    @Test
    fun contentSizeSwapsAxes() {
        assertEquals(400f, PageTransform(crop, PageRotation.R0, 1f).contentSize.width, 0f)
        assertEquals(600f, PageTransform(crop, PageRotation.R90, 1f).contentSize.width, 0f)
    }

    @Test
    fun rectToUserNormalises() {
        val t = PageTransform(crop, PageRotation.R0, 1f)
        // Content rect with inverted ordering still maps to a valid user rect.
        val r = t.rectToUser(Rect(50f, 40f, 10f, 10f))
        assertEquals(110f, r.left, 1e-3f)
        assertEquals(150f, r.right, 1e-3f)
        assertEquals(710f, r.top, 1e-3f)
        assertEquals(740f, r.bottom, 1e-3f)
    }
}
