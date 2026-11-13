package io.github.nibpdf.nib.ui.ink

import androidx.compose.ui.geometry.Offset
import io.github.nibpdf.nib.ui.ink.InkSmoothing.Sample
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.PI
import kotlin.math.hypot
import kotlin.math.sin

class InkSmoothingTest {
    @Test
    fun resampleRespectsSpacingAndCount() {
        val samples =
            (0..100).map { i -> Sample(x = i * 0.5f, y = 0f, pressure = 0.5f, tMs = i.toLong()) }
        val out = InkSmoothing.resample(samples, spacing = 2f)
        assertTrue(out.size <= samples.size)
        assertEquals(samples.first(), out.first())
        assertEquals(samples.last(), out.last())
        out.zipWithNext().forEach { (a, b) ->
            assertTrue(hypot(b.x - a.x, b.y - a.y) >= 2f - 1e-4f)
        }
    }

    @Test
    fun resampleKeepsLast() {
        val samples = listOf(Sample(0f, 0f, 1f, 0), Sample(0.1f, 0f, 1f, 1))
        assertEquals(2, InkSmoothing.resample(samples, 10f).size)
    }

    @Test
    fun rdpStraightLineKeepsEndpoints() {
        val line = (0..99).map { i -> Sample(i.toFloat(), i.toFloat(), 0.5f, i.toLong()) }
        assertEquals(2, InkSmoothing.rdp(line, tolerance = 0.3f).size)
    }

    @Test
    fun rdpSinusoidKeepsMostPoints() {
        // Five periods: curvature keeps the simplifier honest (sag > tol
        // on most segments), so most of the 60 samples survive.
        val sine =
            (0 until 60).map { i ->
                Sample(i.toFloat(), 10f * sin(i / 60f * 10f * PI.toFloat()), 0.5f, i.toLong())
            }
        val out = InkSmoothing.rdp(sine, tolerance = 0.3f)
        assertTrue("kept ${out.size}", out.size >= 40)
    }

    @Test
    fun catmullRomPathNonEmpty() {
        val path =
            InkSmoothing.catmullRomPath(
                listOf(Offset(0f, 0f), Offset(10f, 5f), Offset(20f, 0f)),
            )
        assertFalsePathEmpty(path)
    }

    private fun assertFalsePathEmpty(path: androidx.compose.ui.graphics.Path) {
        assertTrue(!path.isEmpty)
    }
}
